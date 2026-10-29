use super::*;
use serde::{Deserialize, Serialize};

const SAFE_ID: u64 = 9_007_199_254_740_991;
const SET_ASIDE_MS: u64 = 30 * 60_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Objective {
    Reach20,
    Speed,
    Fame,
    Item { reason: String, revision: i32 },
}
impl Objective {
    fn kind(&self) -> &'static str {
        match self {
            Self::Reach20 => "reach20",
            Self::Speed => "speed",
            Self::Fame => "fame",
            Self::Item { .. } => "item",
        }
    }
    /// How the goal is pursued; the adventure scorer acts on these concrete sources.
    fn how(&self) -> &'static str {
        match self {
            Self::Reach20 => "Farm XP in the most demanding observed band suitable for this level",
            Self::Speed => "Hunt observed Snake Pit sources and clear Snake Pits for Speed potions",
            Self::Fame => "Farm XP in the most demanding observed band suitable for this gear",
            Self::Item { .. } => {
                "Collect the target from loot or save tradeable surplus and buy it"
            }
        }
    }
    /// Without measurable progress for this long the goal is abandoned and set aside.
    pub(super) fn patience_ms(&self) -> u64 {
        match self {
            Self::Reach20 | Self::Fame => 15 * 60_000,
            Self::Speed | Self::Item { .. } => 30 * 60_000,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Main {
    id: u64,
    character: i32,
    objective: Objective,
    target: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Intentions {
    next_id: u64,
    main: Option<Main>,
    transition: String,
    #[serde(default)]
    set_aside: Option<(String, u64)>,
}
impl Default for Intentions {
    fn default() -> Self {
        Self {
            next_id: 1,
            main: None,
            transition: "not yet selected".into(),
            set_aside: None,
        }
    }
}
fn item_upgrade(v: &PlayerView, c: &Content, identity: &str) -> Result<Option<(i32, String)>> {
    let class = c.get(v.character.class)?;
    let preferred = (persona::seed(identity) / 3 % 4) as usize;
    let mut best: Option<((usize, u64, i32), String)> = None;
    for kind in c.item_values.kinds() {
        let Ok(price) = c.item_values.quote_range(kind) else {
            continue;
        };
        if v.character.items.contains(&kind)
            || v.map.name == "Vault"
                && v.objects
                    .iter()
                    .any(|o| o.kind == 0x504 && o.contents.contains(&kind))
            || progression::priority(v, c, kind, false, None)? != 140
        {
            continue;
        }
        let item = c.get(kind as u16)?;
        let Some(slot) = class
            .slots
            .iter()
            .take(4)
            .position(|s| *s == item.slot_type)
        else {
            continue;
        };
        let quality = progression::quality(v, c, kind)?;
        let mut improves = true;
        for held in v.character.items.iter().filter(|k| **k >= 0) {
            if c.get(*held as u16)?.slot_type == item.slot_type
                && progression::quality(v, c, *held)? >= quality
            {
                improves = false;
                break;
            }
        }
        if !improves {
            continue;
        }
        // Prefer a lower-priced next upgrade, not the catalog's strongest item for every life.
        let rank = ((slot + 4 - preferred) % 4, price.max, kind);
        if best.as_ref().is_none_or(|(old, _)| rank < *old) {
            best = Some((
                rank,
                format!(
                    "Current-class slot {slot} upgrade: {}; improves on own equipped and carried gear",
                    item.name
                ),
            ));
        }
    }
    Ok(best.map(|((_, _, kind), reason)| (kind, reason)))
}
impl Intentions {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.next_id > 0 && self.next_id <= SAFE_ID && self.transition.len() <= 128,
            "invalid main intention bounds"
        );
        if let Some(m) = &self.main {
            ensure!(
                m.id > 0
                    && m.id < self.next_id
                    && m.character > 0
                    && match &m.objective {
                        Objective::Reach20 => m.target == 20,
                        Objective::Speed => (1..=255).contains(&m.target),
                        Objective::Fame => m.target > 0 && m.target % 200 == 0,
                        Objective::Item { reason, revision } =>
                            (0..=65535).contains(&m.target)
                                && !reason.is_empty()
                                && reason.len() <= 256
                                && *revision >= 0,
                    },
                "invalid character main goal"
            );
        }
        Ok(())
    }
    pub(super) fn bind(&mut self, v: &PlayerView) {
        if self
            .main
            .as_ref()
            .is_some_and(|m| m.character != v.session.character)
        {
            self.end("character changed; old goal and steps invalidated");
        }
    }
    pub(super) fn end(&mut self, reason: &str) {
        if let Some(m) = &self.main {
            tracing::info!(
                goal = m.id,
                character = m.character,
                target = m.target,
                reason,
                "autonomous main goal ended"
            );
        }
        self.main = None;
        self.transition = reason.into();
    }
    /// The current goal's measurable progress, compared by the plan to detect a stall.
    pub(super) fn progress(&self, v: &PlayerView) -> Option<(u64, i64, u64)> {
        let m = self.main.as_ref()?;
        let metric = match &m.objective {
            Objective::Reach20 => i64::from(v.character.experience),
            Objective::Fame => i64::from(v.character.experience) + i64::from(v.character.fame),
            Objective::Speed => {
                i64::from(v.character.base_stats[4]) * 1000 + v.character.snake_pits as i64
            }
            Objective::Item { .. } => i64::from(v.character.inventory_revision),
        };
        Some((m.id, metric, m.objective.patience_ms()))
    }
    pub(super) fn abandon(&mut self, reason: &str, now: u64) {
        if let Some(m) = &self.main {
            self.set_aside = Some((m.objective.kind().into(), now + SET_ASIDE_MS));
            self.end(reason);
        }
    }
    pub(super) fn label(&self, c: &Content) -> Option<String> {
        self.main.as_ref().map(|m| match &m.objective {
            Objective::Reach20 => "Reach level 20".into(),
            Objective::Speed => "Max permanent Speed".into(),
            Objective::Fame => format!("Earn {} base fame", m.target),
            Objective::Item { .. } => format!("Acquire {}", c.objects[&(m.target as u16)].name),
        })
    }
    pub(super) fn observe(&mut self, v: &PlayerView, c: &Content, identity: &str) -> Result<()> {
        if self
            .main
            .as_ref()
            .is_some_and(|m| m.character != v.session.character)
        {
            self.end("character changed; old goal and steps invalidated");
        }
        if !v.lifecycle.alive || v.lifecycle.death_pending {
            self.end("character died; old goal and steps invalidated");
            return Ok(());
        }
        // Busy snapshots can precede the committed character write.
        if v.lifecycle.busy || v.lifecycle.departing || v.lifecycle.quarantined {
            return Ok(());
        }
        if let Some(Main {
            objective: Objective::Item { revision, .. },
            target,
            ..
        }) = &self.main
        {
            c.item_values.quote_range(*target).map_err(|reason| {
                anyhow::anyhow!("saved item goal {target} ineligible: {reason}")
            })?;
            ensure!(
                v.character.inventory_revision >= *revision,
                "item goal holdings revision regressed"
            );
        }
        let completed = self
            .main
            .as_ref()
            .filter(|m| match &m.objective {
                Objective::Reach20 => v.character.level >= m.target,
                Objective::Speed => v.character.base_stats[4] >= m.target,
                Objective::Fame => v.character.fame >= m.target,
                Objective::Item { revision, .. } => {
                    v.character.inventory_revision > *revision
                        && v.character.items.contains(&m.target)
                }
            })
            .map(|m| (m.id, m.target));
        if let Some((id, target)) = completed {
            self.end(&format!(
                "goal {id} target {target} completed by committed own observation"
            ));
        } else if let Some(target) = self.item_target()
            && !v.character.items.contains(&target)
        {
            let item = c.get(target as u16)?;
            let target_quality = progression::quality(v, c, target)?;
            let mut obsolete = progression::priority(v, c, target, false, None)? != 140;
            for held in v.character.items.iter().filter(|k| **k >= 0) {
                if c.get(*held as u16)?.slot_type == item.slot_type
                    && progression::quality(v, c, *held)? >= target_quality
                {
                    obsolete = true;
                }
            }
            if obsolete {
                self.end("item goal aborted: no longer improves current own gear");
            }
        }
        if self.main.is_none() {
            let now = plan::now();
            let aside = |kind: &str| {
                self.set_aside
                    .as_ref()
                    .is_some_and(|(k, until)| k == kind && now < *until)
            };
            let cap = c.classes[&v.character.class].caps[4];
            let seeks_item = persona::seed(identity).is_multiple_of(3) && !aside("item");
            let upgrade = if seeks_item {
                item_upgrade(v, c, identity)?
            } else {
                None
            };
            let (objective, target) = if let Some((kind, reason)) = upgrade {
                (
                    Objective::Item {
                        reason,
                        revision: v.character.inventory_revision,
                    },
                    kind,
                )
            } else if v.character.level < 20 && !aside("reach20") {
                (Objective::Reach20, 20)
            } else if v.character.level == 20 && v.character.base_stats[4] < cap && !aside("speed")
            {
                (Objective::Speed, cap)
            } else {
                (
                    Objective::Fame,
                    v.character
                        .fame
                        .checked_div(200)
                        .and_then(|n| n.checked_add(1))
                        .and_then(|n| n.checked_mul(200))
                        .context("personal base fame milestone exhausted")?,
                )
            };
            let id = self.next_id;
            ensure!(id < SAFE_ID, "main goal identity exhausted");
            self.next_id += 1;
            tracing::info!(
                character = v.session.character,
                goal = id,
                ?objective,
                target,
                previous = self.transition,
                "autonomous main goal selected"
            );
            self.main = Some(Main {
                id,
                character: v.session.character,
                objective,
                target,
            });
        }
        if let Some(m) = &self.main {
            ensure!(
                m.objective != Objective::Speed
                    || m.target == c.classes[&v.character.class].caps[4],
                "saved Speed goal disagrees with class cap"
            );
        }
        Ok(())
    }
    pub(crate) fn item_target(&self) -> Option<i32> {
        self.main
            .as_ref()
            .filter(|m| matches!(m.objective, Objective::Item { .. }))
            .map(|m| m.target)
    }
    pub(super) fn item_goal(&self) -> Option<(u64, i32)> {
        self.main
            .as_ref()
            .filter(|m| matches!(m.objective, Objective::Item { .. }))
            .map(|m| (m.id, m.target))
    }
    pub(super) fn speed(&self) -> bool {
        self.main
            .as_ref()
            .is_some_and(|m| m.objective == Objective::Speed)
    }
    /// Objective-specific value of an adventure choice.
    pub(super) fn bonus(&self, v: &PlayerView, goal: &Goal) -> f32 {
        let target_loot = self.item_target().is_some_and(|item| {
            matches!(goal, Goal::Loot(id) if v.objects.iter().any(|o| o.id == *id && o.contents.contains(&item)))
        });
        if target_loot {
            return 30.;
        }
        if !self.speed() {
            return 0.;
        }
        match goal {
            Goal::Portal(id) if v.objects.iter().any(|p| p.id == *id && p.kind == 0x718) => 10.,
            Goal::Loot(id)
                if v.objects.iter().any(|b| {
                    b.id == *id
                        && b.contents.contains(&0xa21)
                        && b.contents
                            .iter()
                            .filter(|k| **k >= 0)
                            .all(|k| matches!(*k, 0xa21..=0xa23))
                }) =>
            {
                30.
            }
            _ => 0.,
        }
    }
    pub(super) fn payload(
        &self,
        v: &PlayerView,
        c: &Content,
        activity: Activity,
        step: String,
        errand: Option<&plan::Step>,
    ) -> serde_json::Value {
        let (goal, plan) = self.main.as_ref().map_or((serde_json::Value::Null, "No character goal selected"), |m| {
            // An item kind is not a quantity; the label names the item and the website omits Progress.
            let (current, target, unit) = match &m.objective {
                Objective::Reach20 => (Some(v.character.level), Some(m.target), Some("level")),
                Objective::Speed => (Some(v.character.base_stats[4]), Some(m.target), Some("Speed")),
                Objective::Fame => (Some(v.character.fame), Some(m.target), Some("base fame")),
                Objective::Item { .. } => (None, None, None),
            };
            (serde_json::json!({"id":m.id,"label":self.label(c),"current":current,"target":target,"unit":unit}), m.objective.how())
        });
        let necessary = errand.is_some()
            || !matches!(
                activity,
                Activity::Adventure | Activity::Event | Activity::Linger
            );
        let reason = match (errand, activity) {
            (Some(e), _) => format!("Finish the current {:?} errand: {}", e.owner, e.target),
            (None, Activity::Survive) => {
                "Observed danger or health requires a safety reflex before continuing".into()
            }
            (None, Activity::Busy) => "Wait for pending ordinary work before another action".into(),
            (None, Activity::Equip) => "Assess or equip observed own gear before continuing".into(),
            (None, Activity::Consume) => "Use a carried stat potion before continuing".into(),
            (None, Activity::Social) => "Honor existing company or social coordination".into(),
            _ => match &self.main {
                Some(Main {
                    objective: Objective::Item { reason, .. },
                    ..
                }) => reason.clone(),
                _ => self.transition.clone(),
            },
        };
        serde_json::json!({"goal":goal,"plan":plan,"step":step,
            "mode":if necessary {"necessary"} else {"progress"},"reason":reason,
            "resume":necessary.then_some(plan)})
    }
}

#[cfg(test)]
mod tests;
