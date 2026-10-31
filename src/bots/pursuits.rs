use super::*;
use serde::{Deserialize, Serialize};

pub(crate) mod market;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Cognition {
    pub attention: usize,
    pub velocity_step: f32,
    pub directions: usize,
    pub pickup_clearance: f32,
}
impl Default for Cognition {
    fn default() -> Self {
        Self {
            attention: 8,
            velocity_step: 0.25,
            directions: 16,
            pickup_clearance: 1.05,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ItemFact {
    pub kind: i32,
    pub assessments: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Mind {
    pub schema: u8,
    #[serde(default)]
    pub intentions: Option<intentions::Intentions>,
    pub account: i32,
    pub revision: i64,
    pub cognition: Cognition,
    pub facts: Vec<ItemFact>,
    #[serde(default)]
    pub future_class: Option<u16>,
    #[serde(default)]
    pub market: market::Market,
    #[serde(default)]
    pub social: social::Life,
    #[serde(default)]
    pub events: events::Events,
    // The living character a named persona resumes in its next session.
    #[serde(default)]
    pub character: Option<i32>,
    /// A veteran's level-20 alt was topped up once; later deaths are permanent as usual.
    #[serde(default)]
    pub veteran_alt: bool,
}
impl Mind {
    pub(crate) fn new(account: i32) -> Self {
        Self {
            schema: 2,
            intentions: Some(intentions::Intentions::default()),
            account,
            revision: 0,
            cognition: Cognition::default(),
            facts: vec![],
            future_class: None,
            market: market::Market::default(),
            social: social::Life::default(),
            events: events::Events::default(),
            character: None,
            veteran_alt: false,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            matches!(
                (self.schema, self.intentions.is_some()),
                (1, false) | (2, true)
            ) && self.account > 0
                && self.revision >= 0
                && self.character.is_none_or(|c| c > 0),
            "invalid account mind binding/schema"
        );
        ensure!(self.facts.len() <= 64, "account mind exceeds bounds");
        ensure!(
            self.facts
                .iter()
                .all(|f| (0..=65535).contains(&f.kind) && f.assessments <= 2)
                && self
                    .facts
                    .iter()
                    .map(|f| f.kind)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == self.facts.len(),
            "invalid or duplicate learned item facts"
        );
        if let Some(intentions) = &self.intentions {
            intentions.validate()?;
        }
        self.market.validate(self.account)?;
        self.social.validate(self.account)?;
        self.events.validate()?;
        let c = &self.cognition;
        ensure!(
            (1..=32).contains(&c.attention)
                && [4, 8, 16].contains(&c.directions)
                && c.velocity_step.is_finite()
                && (0.125..=4.).contains(&c.velocity_step)
                && c.pickup_clearance.is_finite()
                && (0.1..=2.).contains(&c.pickup_clearance),
            "invalid cognitive estimates"
        );
        Ok(())
    }
    pub(super) fn end(&mut self, reason: &str) {
        if let Some(intentions) = &mut self.intentions {
            intentions.end(reason);
        }
    }
    pub(super) fn bind(&mut self, v: &PlayerView) -> Result<()> {
        self.validate()?;
        ensure!(
            self.account == v.session.account,
            "account mind cannot cross accounts"
        );
        if self.schema == 1 {
            self.schema = 2;
            self.intentions = Some(intentions::Intentions::default());
        }
        self.intentions
            .as_mut()
            .expect("validated intentions")
            .bind(v);
        self.market.bind();
        self.market
            .set_target(self.intentions.as_ref().and_then(|i| i.item_target()));
        self.social.bind();
        self.events.bind();
        self.character = Some(v.session.character);
        tracing::info!(
            account = self.account,
            character = v.session.character,
            "account mind bound"
        );
        Ok(())
    }
    pub(super) fn inspecting(&self, v: &PlayerView, content: &Content) -> bool {
        v.character
            .items
            .iter()
            .skip(4)
            .filter(|k| **k >= 0)
            .any(|kind| {
                let item = &content.objects[&(*kind as u16)];
                content.objects[&v.character.class]
                    .slots
                    .iter()
                    .take(4)
                    .any(|slot| *slot == item.slot_type)
                    && !self
                        .facts
                        .iter()
                        .any(|f| f.kind == *kind && f.assessments >= 2)
            })
    }
    pub(super) fn assess(&mut self, v: &PlayerView, content: &Content) -> Result<Option<Action>> {
        let class = content.get(v.character.class)?;
        for (from, kind) in v
            .character
            .items
            .iter()
            .enumerate()
            .skip(4)
            .filter(|(_, k)| **k >= 0)
        {
            let item = content.get(*kind as u16)?;
            let Some(to) = class
                .slots
                .iter()
                .take(4)
                .position(|slot| *slot == item.slot_type)
            else {
                continue;
            };
            if item.unsupported_equipment {
                continue;
            }
            let known = self.facts.iter().position(|f| f.kind == *kind);
            let index = match known {
                Some(index) => index,
                None => {
                    if self.facts.len() == 64 {
                        self.facts.remove(0);
                    }
                    self.facts.push(ItemFact {
                        kind: *kind,
                        assessments: 0,
                    });
                    self.facts.len() - 1
                }
            };
            let evidence = self.facts[index].assessments;
            if evidence < 2 {
                self.facts[index].assessments += 1;
                tracing::info!(
                    account = self.account,
                    kind,
                    evidence = evidence + 1,
                    "autonomous item inspected"
                );
                // Inspection competes for attention but finishes after two attended assessments.
                return Ok(None);
            }
            let old = v.character.items[to];
            if progression::quality(v, content, *kind)? > progression::quality(v, content, old)? {
                tracing::info!(
                    account = self.account,
                    kind,
                    old,
                    "autonomous assessed equipment upgrade"
                );
                return Ok(Some(Action::Swap {
                    from: own(v, from),
                    to: own(v, to),
                }));
            }
        }
        Ok(None)
    }
}
fn settled(v: &PlayerView) -> bool {
    v.lifecycle.alive
        && !v.lifecycle.busy
        && !v.lifecycle.death_pending
        && !v.lifecycle.departing
        && !v.lifecycle.quarantined
        && matches!(v.lifecycle.output, OutputState::Live)
}
