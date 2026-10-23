use super::*;
use crate::{
    condition,
    encounter::attacks::{Action, Aim, Prediction, Schedule},
};

impl World {
    pub(super) fn suspend_npc(&mut self, id: i32, delta: u64) {
        let e = self.entities.get_mut(&id).expect("suspended NPC missing");
        if let Some(activated) = &mut e.activated {
            *activated += delta;
        }
        if e.next_shot >= self.now.saturating_sub(delta) {
            e.next_shot += delta;
        }
        if e.next_aux >= self.now.saturating_sub(delta) {
            e.next_aux += delta;
        }
        if let Some(since) = &mut e.blocked_since {
            *since += delta;
        }
        if e.reaction_until > self.now.saturating_sub(delta) {
            e.reaction_until += delta;
        }
        e.attacks.suspend(delta);
        if let Some(charge) = &mut e.charge {
            charge.suspend(delta);
        }
    }
    pub(super) fn npc_attack_schedule(&self, id: i32, elapsed: u64) -> Result<Option<Schedule>> {
        use crate::{
            encounter::attacks,
            endgame::{ORYX, ORYX2, OryxPhase},
        };
        let e = &self.entities[&id];
        if matches!(
            e.kind,
            crate::dungeon::manor::KAGE | crate::dungeon::manor::MINE
        ) {
            return Ok(Some(attacks::manor_entry_schedule(
                e.kind,
                e.stage.context("Manor entrance phase missing")?,
                elapsed,
            )));
        }
        if crate::dungeon::jungle::ROSTER.contains(&e.kind) {
            return Ok(Some(self.jungle_schedule(id)?));
        }
        if crate::dungeon::davy::profile(e.kind).is_some() {
            let mut schedule = attacks::davy_schedule(e.kind, e.stage.unwrap_or(0), elapsed);
            if e.kind == 0xe50 {
                let owner = e
                    .parent
                    .and_then(|id| self.entities.get(&id))
                    .context("senior ghost lacks its captain")?;
                schedule.attacks[0].count = if owner.hp < self.content.objects[&owner.kind].hp / 2 {
                    3
                } else {
                    2
                };
            }
            return Ok(Some(schedule));
        }
        if crate::encounter::landmarks::ROSTER.contains(&e.kind) {
            let state = e.stage.context("landmark attack phase missing")?;
            let script = e
                .script
                .as_ref()
                .context("landmark attack script missing")?;
            return Ok(Some(crate::encounter::landmarks::schedule(
                e.kind,
                e.hp,
                state,
                script.choice,
                elapsed,
            )));
        }
        if e.kind == ORYX2 {
            let brain = self
                .finale
                .bosses
                .get(&id)
                .context("activated Oryx2 brain missing")?;
            let phase = brain.phase().context("activated Oryx2 phase missing")?;
            let age = match phase {
                OryxPhase::Opening => elapsed,
                OryxPhase::PrepareRage => brain
                    .rage_elapsed(elapsed)
                    .context("rage preparation clock missing")?,
                OryxPhase::Rage => brain
                    .rage_elapsed(elapsed)
                    .context("rage clock missing")?
                    .checked_sub(10000)
                    .context("rage began before protected preparation completed")?,
                _ => bail!("invalid Oryx2 phase"),
            };
            return Ok(Some(attacks::oryx2_schedule(phase, age)));
        }
        if e.kind == 0xd7e {
            let stage = e.stage.context("activated Suit of Armor phase missing")?;
            let age = if stage == 2 {
                self.now - (e.reaction_until - 1500)
            } else {
                elapsed
            };
            return Ok(Some(attacks::armor_schedule(stage, age)));
        }
        let mut schedule = match attacks::schedule(e.kind, e.hp, elapsed) {
            Some(schedule) => Some(schedule),
            None => {
                attacks::endgame_schedule(e.kind, e.hp, self.content.objects[&e.kind].hp, elapsed)
            }
        };
        if matches!(e.kind, 0x903 | 0x904) {
            let stage = e.stage.context("Flamer attack phase missing")?;
            let s = schedule.as_mut().context("Flamer schedule missing")?;
            s.state = stage;
            s.attacks[0].enabled = stage == 1;
            s.attacks[1].enabled = stage == 3;
        }
        if e.kind == 0x20d {
            schedule
                .as_mut()
                .context("Gray Blob schedule missing")?
                .attacks[0]
                .enabled = e.stage == Some(1);
        }
        if e.kind == ORYX && e.hp <= self.content.objects[&e.kind].hp / 5 {
            schedule
                .as_mut()
                .context("Oryx rage attack schedule missing")?
                .age = self
                .finale
                .bosses
                .get(&id)
                .context("activated Oryx brain missing")?
                .rage_elapsed(elapsed)
                .context("Oryx rage entry clock missing")?;
        }
        if e.kind == 0xd06 && e.reaction_until > self.now {
            schedule
                .as_mut()
                .context("Limon attack schedule missing")?
                .attacks[2]
                .enabled = true;
        }
        Ok(schedule)
    }
    pub(super) fn legacy_enemy_shots(
        &self,
        id: i32,
        target: Pos,
        elapsed: u64,
    ) -> Result<Vec<(u8, f32)>> {
        let e = &self.entities[&id];
        let kind = e.kind;
        let heading = (target.y - e.pos.y).atan2(target.x - e.pos.x);
        if crate::dungeon::trench::ROSTER.contains(&kind) {
            self.ocean_shots(id, target)
        } else if e.quest.is_some() {
            self.quest_shots(id, target, elapsed)
        } else if crate::endgame::ROSTER.contains(&kind) {
            self.finale_shots(id, heading, elapsed)
        } else if matches!(kind, 0x900..=0x902) {
            self.shield_shots(id, heading)
        } else if gods::GODS.contains(&kind)
            || gods::CONSTRUCTS.contains(&kind)
            || midlands::AIMED.contains(&kind)
        {
            self.god_shots(id, target, heading, elapsed)
        } else {
            let elapsed = if kind == 0x909 && e.stage == Some(1) {
                5000
            } else {
                elapsed
            };
            Ok(crate::encounter::phased_attacks(
                kind, e.hp, heading, elapsed,
            ))
        }
    }
    pub(super) fn scheduled_attacks(
        &mut self,
        id: i32,
        target: Option<Pos>,
        schedule: &Schedule,
    ) -> Result<Vec<(u8, f32)>> {
        let due = self
            .entities
            .get_mut(&id)
            .context("scheduled attacker missing")?
            .attacks
            .due(schedule, self.now);
        let pos = self.entities[&id].pos;
        let mut shots = vec![];
        for slot in due {
            let mut attack = schedule.attacks[slot];
            let heading = match attack.aim {
                Aim::Fixed(angle) => angle,
                Aim::Target => {
                    let Some(target) = target.filter(|p| pos.distance(*p) <= attack.range) else {
                        continue;
                    };
                    (target.y - pos.y).atan2(target.x - pos.x)
                }
            };
            match attack.action {
                Action::Grenade { radius, damage } => {
                    self.throw_grenade(
                        id,
                        target.context("targeted grenade has no target")?,
                        radius,
                        damage,
                    )?;
                }
                Action::Shoot { projectile, choice } => {
                    let projectile = match choice {
                        Some(other) if self.rng.gen_bool(0.5) => {
                            attack.count = other.count;
                            attack.gap = other.gap;
                            attack.offset = other.offset;
                            other.projectile
                        }
                        _ => projectile,
                    };
                    let mut volley = attack.shots(projectile, heading);
                    let factor = match attack.predict {
                        Prediction::None => 0.,
                        Prediction::Lead(factor) => factor,
                        Prediction::Chance(chance) => {
                            if self.rng.gen_bool(chance) {
                                1.
                            } else {
                                0.
                            }
                        }
                    };
                    if factor > 0. {
                        self.predict_enemy_shots(
                            id,
                            target.context("predictive attack has no target")?,
                            &mut volley,
                            |_| factor,
                        )?;
                    }
                    if self.entities[&id].conditions(self.now) & condition::bit(condition::DAZED)
                        != 0
                    {
                        volley = enemy_shots::dazed(volley);
                    }
                    shots.extend(volley);
                }
            }
            tracing::trace!(
                entity = id,
                slot,
                phase = schedule.state,
                now = self.now,
                interval = ?attack.interval,
                delay = attack.delay,
                range = attack.range,
                "NPC weapon deadline fired"
            );
        }
        Ok(shots)
    }
}
