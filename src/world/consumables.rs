use super::*;
use crate::content::{ConsumableEffect, Object};

#[derive(Clone)]
pub(super) struct Boost {
    source: (i32, i32),
    activation: usize,
    index: usize,
    amount: i32,
    until: Option<u64>,
}
impl Player {
    pub(super) fn effective(&self, content: &Content, now: u64) -> Result<[i32; 8]> {
        let mut values = self.character.effective(content)?;
        for boost in &self.boosts {
            if boost.until.is_some_and(|until| until > now) {
                values[boost.index] = values[boost.index]
                    .checked_add(boost.amount)
                    .context("temporary stat overflow")?;
            }
        }
        Ok(values)
    }
}
impl World {
    pub(super) fn expire_consumable_boosts(&mut self) {
        for p in self.players.values_mut() {
            p.boosts
                .retain(|boost| boost.until.is_none_or(|until| until > self.now));
            let values = p
                .effective(&self.content, self.now)
                .expect("validated temporary stats");
            p.character.hp = p.character.hp.min(values[0]);
            p.character.mp = p.character.mp.min(values[1]);
        }
    }
    fn consumable_targets(&self, id: i32, range: Option<f32>) -> Vec<i32> {
        let position = self.players[&id].pos;
        self.players
            .iter()
            .filter(|(other, p)| {
                p.active()
                    && !p.paused
                    && !p.pending
                    && p.character.hp > 0
                    && match range {
                        Some(range) => p.pos.distance(position) < range,
                        None => **other == id,
                    }
            })
            .map(|(id, _)| *id)
            .collect()
    }
    pub(super) fn begin_consumable(
        &mut self,
        id: i32,
        slot: Slot,
        item: &Object,
    ) -> Result<InventoryPlan> {
        ensure!(
            item.consumable
                && !item.consumable_effects.is_empty()
                && item.consumable_effects.len() == item.activations,
            "item is not a supported consumable"
        );
        for effect in &item.consumable_effects {
            if let ConsumableEffect::Unsupported(name) = effect {
                bail!("item activation not implemented: {name}");
            }
        }
        let next = if let Some(name) = &item.successor {
            let next = self
                .content
                .objects
                .values()
                .find(|next| &next.name == name)
                .context("consumable successor is missing")?;
            ensure!(
                next.consumable
                    && next.slot_type == item.slot_type
                    && item
                        .doses
                        .is_some_and(|doses| doses > 1 && next.doses == Some(doses - 1)),
                "invalid consumable dose succession"
            );
            i32::from(next.kind)
        } else {
            -1
        };
        let source = (
            self.players[&id].character.id,
            self.players[&id].character.revision,
        );
        let mut reservations = Vec::new();
        let mut counts = BTreeMap::<i32, usize>::new();
        for (activation, effect) in item.consumable_effects.iter().enumerate() {
            if let ConsumableEffect::Boost {
                index,
                amount,
                range,
                ..
            } = effect
            {
                for target in self.consumable_targets(id, *range) {
                    *counts.entry(target).or_default() += 1;
                    reservations.push((
                        target,
                        Boost {
                            source,
                            activation,
                            index: *index,
                            amount: *amount,
                            until: None,
                        },
                    ));
                }
            }
        }
        for (target, count) in counts {
            let p = self.players.get_mut(&target).unwrap();
            p.boosts
                .retain(|b| b.until.is_none_or(|until| until > self.now));
            ensure!(
                p.boosts.len() + count <= 128,
                "temporary stat effect budget exhausted"
            );
        }
        for (target, boost) in reservations {
            self.players.get_mut(&target).unwrap().boosts.push(boost);
        }
        let mut plan = self.consumable_plan(id, slot, next);
        plan.effects = item.consumable_effects.clone();
        Ok(plan)
    }
    pub(super) fn discard_consumable(&mut self, source: (i32, i32)) {
        for p in self.players.values_mut() {
            p.boosts.retain(|b| b.until.is_some() || b.source != source);
        }
    }
    pub(super) fn finish_consumable(
        &mut self,
        player: i32,
        source: (i32, i32),
        effects: Vec<ConsumableEffect>,
    ) -> Result<()> {
        for (activation, effect) in effects.iter().enumerate() {
            if let ConsumableEffect::Boost { duration, .. } = effect {
                for p in self.players.values_mut() {
                    if p.active() && p.character.hp > 0 {
                        for boost in p.boosts.iter_mut().filter(|b| {
                            b.source == source && b.activation == activation && b.until.is_none()
                        }) {
                            boost.until = Some(
                                self.now
                                    .checked_add(*duration)
                                    .context("boost deadline overflow")?,
                            );
                        }
                    }
                }
                continue;
            }
            if !self.players[&player].active() || self.players[&player].character.hp <= 0 {
                continue;
            }
            match *effect {
                ConsumableEffect::Heal {
                    amount,
                    range: Some(range),
                } => self.consumable_heal_nova(player, amount, range)?,
                ConsumableEffect::Heal {
                    amount,
                    range: None,
                } => {
                    let p = self.players.get_mut(&player).unwrap();
                    if p.conditions(self.now) & crate::condition::bit(crate::condition::SICK) == 0 {
                        p.character.hp =
                            (p.character.hp + amount).min(p.effective(&self.content, self.now)?[0]);
                    }
                }
                ConsumableEffect::Mana { amount, range } => {
                    for target in self.consumable_targets(player, range) {
                        let p = self.players.get_mut(&target).unwrap();
                        if p.conditions(self.now) & crate::condition::bit(crate::condition::QUIET)
                            == 0
                        {
                            p.character.mp = (p.character.mp + amount)
                                .min(p.effective(&self.content, self.now)?[1]);
                        }
                    }
                }
                ConsumableEffect::Condition { effect, duration } => {
                    self.apply_condition(player, effect, duration)?
                }
                ConsumableEffect::Clear { effect, check } => {
                    let p = self.players.get_mut(&player).unwrap();
                    if check.is_none_or(|c| p.conditions(self.now) & crate::condition::bit(c) != 0)
                    {
                        p.statuses.remove(&effect);
                    }
                }
                ConsumableEffect::Cleanse => self
                    .players
                    .get_mut(&player)
                    .unwrap()
                    .statuses
                    .retain(|effect, _| !matches!(effect, 2..=11 | 14 | 16 | 27 | 28)),
                ConsumableEffect::Boost { .. } => unreachable!(),
                ConsumableEffect::Unsupported(ref name) => {
                    bail!("unvalidated committed consumable effect: {name}")
                }
            }
        }
        self.discard_consumable(source);
        if !effects.is_empty() {
            tracing::info!(
                player = player,
                revision = source.1,
                effects = effects.len(),
                "consumable effects published after inventory commit"
            );
        }
        Ok(())
    }
}
