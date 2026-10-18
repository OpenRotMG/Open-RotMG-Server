// GPL-3.0 reference; data/REALM-SRC-LICENSE: realm-src@1855c017eab870d13f0b61b1cf0b9e66a01922ff
// wServer/logic/db/BehaviorDb.{SkullShrine,CubeGod,Pentaract}.cs. Ownership replaces nearest-object damage transfer.
use super::*;
use crate::{
    condition,
    encounter::{Anchor, Motion},
    quest,
};

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Orbit {
    pub(super) speed: f32,
    pub(super) radius: f32,
    pub(super) direction: f32,
}

// The shared mover divides by 1.5; preserve the reference Utils.GetSpeed velocity.
fn movement_speed(value: f32) -> f32 {
    (5.55 * value + 0.74) * 1.5
}

impl World {
    pub(super) fn populate_realm_events(&mut self) -> Result<()> {
        for kind in 0xd56..=0xd60 {
            quest::validate(&self.content, kind)?;
            crate::loot::validate(&self.content, kind)?;
        }
        for kind in quest::EVENTS {
            let pos = self.place_named_setpiece(kind, &[1])?;
            let root = self.spawn_entity(kind, pos, vec![])?;
            let e = self.entities.get_mut(&root).unwrap();
            e.quest = Some(root);
            e.stage = Some(0);
            e.next_aux = match kind {
                0xd56 => 500,
                0xd59 => 1500,
                _ => 0,
            };
            if kind == 0xd5f {
                e.condition |= condition::bit(condition::INVINCIBLE);
                for n in 0..5 {
                    let a = n as f32 * std::f32::consts::TAU / 5.;
                    let tower = Pos {
                        x: pos.x + ((a.cos() * 15. + 17.) as i32 - 17) as f32,
                        y: pos.y + ((a.sin() * 15. + 17.) as i32 - 17) as f32,
                    };
                    let id = self.spawn_entity(0xd5e, tower, vec![])?;
                    let e = self.entities.get_mut(&id).unwrap();
                    e.quest = Some(root);
                    e.home = tower;
                    e.stage = Some(0);
                }
            }
            tracing::info!(entity = root, kind, "finite Realm event placed");
        }
        Ok(())
    }
    fn event_birth(&mut self, id: i32, kind: u16, count: usize, radius: f32) -> Result<Vec<i32>> {
        let old = self.quest_members(id, &[kind]);
        let e = &self.entities[&id];
        self.quest_birth(
            (e.quest.context("event ownership missing")?, e.home, e.pos),
            kind,
            usize::MAX,
            count,
            radius,
        )?;
        let children: Vec<_> = self
            .quest_members(id, &[kind])
            .into_iter()
            .filter(|id| !old.contains(id))
            .collect();
        let home = self.entities[&id].pos;
        for child in &children {
            let orbit = match kind {
                0xd58 => Some((1.5, 0.6, 15., 10., true)),
                0xd5a => Some((0.375, 0.075, 10., 5., false)),
                0xd5b => Some((1.05, 0.15, 5., 3., false)),
                0xd5c => Some((1.05, 0.15, 7.5, 3., false)),
                _ => None,
            }
            .map(
                |(speed, variance, radius, spread, random_direction)| Orbit {
                    speed: movement_speed(speed + self.rng.gen_range(-variance..variance)),
                    radius: radius + self.rng.gen_range(-spread..spread),
                    direction: if random_direction && self.rng.gen_bool(0.5) {
                        1.
                    } else {
                        -1.
                    },
                },
            );
            let phase = self.rng.gen_range(0.0..std::f32::consts::TAU);
            let e = self.entities.get_mut(child).unwrap();
            e.orbit = orbit;
            e.phase = phase;
            e.home = home;
            e.expires = None;
            e.activated = None;
            if kind == 0xd5a {
                e.next_aux = self.now + 1000;
            }
        }
        Ok(children)
    }
    fn event_reproduce(&mut self, id: i32, kind: u16, radius: f32, maximum: usize) -> Result<()> {
        let pos = self.entities[&id].pos;
        // Reference Reproduce caps nearby density, not the number born to one parent.
        if self
            .actors()
            .filter(|e| e.kind == kind && e.pos.distance(pos) < radius)
            .count()
            < maximum
        {
            self.event_birth(id, kind, 1, 0.)?;
        }
        Ok(())
    }
    pub(super) fn event_mechanics(&mut self, id: i32) -> Result<()> {
        let e = &self.entities[&id];
        let kind = e.kind;
        if kind == 0xd5c {
            let nearby = |kind| {
                self.quest_members(id, &[kind])
                    .into_iter()
                    .any(|other| self.entities[&other].pos.distance(e.pos) < 10.)
            };
            let follow = e.stage == Some(1);
            if !follow && !nearby(0xd5a) {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(1);
                e.next_aux = self.now + 5000;
            } else if follow && (!nearby(0xd5b) || self.now >= e.next_aux) {
                self.entities.get_mut(&id).unwrap().stage = Some(0);
            }
            return Ok(());
        }
        if kind == 0xd5d {
            if self.now >= e.next_aux {
                let pos = e.pos;
                let target = self
                    .event_target(pos, 20.)
                    .filter(|t| t.x != pos.x && t.y != pos.y);
                let e = self.entities.get_mut(&id).unwrap();
                if let Some(target) = target {
                    let distance = pos.distance(target);
                    let c = (64. - distance * distance).abs().sqrt() / 2.;
                    e.home = Pos {
                        x: (pos.x + target.x) / 2. + c * (pos.y - target.y) / distance,
                        y: (pos.y + target.y) / 2. + c * (target.x - pos.x) / distance,
                    };
                    e.stage = Some(1);
                    e.goal = None;
                    let speed = movement_speed(2.) / 1.5
                        * if e.conditions(self.now) & condition::bit(condition::SLOWED) != 0 {
                            0.5
                        } else {
                            1.
                        };
                    e.next_aux =
                        self.now + (std::f32::consts::TAU * 8. / speed * 1000.).ceil() as u64;
                } else {
                    e.stage = Some(0);
                    e.next_aux = self.now + 5000;
                }
            }
            return Ok(());
        }
        if kind == 0xd5e {
            if self.now >= e.stage.unwrap_or(0) {
                let eyes = self
                    .quest_members(id, &[0xd5d])
                    .into_iter()
                    .filter(|eye| self.entities[eye].parent == Some(id))
                    .count();
                let count = if e.stage.unwrap_or(0) == 0 {
                    5usize.saturating_sub(eyes)
                } else {
                    usize::from(eyes < 5)
                };
                for eye in self.event_birth(id, 0xd5d, count, 1.)? {
                    let e = self.entities.get_mut(&eye).unwrap();
                    e.parent = Some(id);
                    e.home = e.pos;
                    e.stage = Some(0);
                }
                self.entities.get_mut(&id).unwrap().stage = Some(self.now + 5000);
            }
            let e = &self.entities[&id];
            if self.now >= e.next_aux
                && e.conditions(self.now) & condition::bit(condition::STUNNED) == 0
            {
                let pos = e.pos;
                let target = self.event_target(pos, 8.);
                if let Some(target) = target {
                    self.throw_grenade(id, target, 4., 100)?;
                }
                self.entities.get_mut(&id).unwrap().next_aux = self.now + 5000;
            }
        } else if self.now >= e.next_aux {
            match kind {
                0xd56 => {
                    self.event_reproduce(id, 0xd57, 40., 20)?;
                    self.event_reproduce(id, 0xd58, 40., 20)?;
                }
                0xd59 => self.event_reproduce(id, 0xd5a, 30., 10)?,
                0xd5a => {
                    self.event_reproduce(id, 0xd5b, 12., 10)?;
                    self.event_reproduce(id, 0xd5c, 30., 10)?;
                }
                _ => return Ok(()),
            }
            self.entities.get_mut(&id).unwrap().next_aux = self.now
                + match kind {
                    0xd56 => 500,
                    0xd59 => 1500,
                    _ => 1000,
                };
        }
        Ok(())
    }
    pub(super) fn event_tower_death(&mut self, defeated: &Entity) -> Result<()> {
        let tag = defeated.quest.context("Pentaract tower has no encounter")?;
        let root = self
            .entities
            .get_mut(&tag)
            .context("Pentaract controller absent during tower defeat")?;
        ensure!(
            root.kind == 0xd5f,
            "Pentaract tower owner is not its controller"
        );
        for (character, damage) in &defeated.damage {
            *root.damage.entry(*character).or_default() += *damage;
        }
        let corpse = self.spawn_entity(0xd60, defeated.pos, vec![])?;
        let e = self.entities.get_mut(&corpse).unwrap();
        e.quest = Some(tag);
        e.home = defeated.home;
        e.damage = defeated.damage.clone();
        e.activated = Some(self.now);
        e.condition |= condition::bit(condition::INVINCIBLE);
        tracing::info!(
            tower = defeated.id,
            corpse,
            quest = tag,
            revive_at = self.now + 15000,
            "Pentaract tower down"
        );
        Ok(())
    }
    pub(super) fn event_step(&mut self) {
        let roots: Vec<_> = self
            .non_population_actors()
            .filter(|e| e.kind == 0xd5f && e.quest == Some(e.id))
            .map(|e| e.id)
            .collect();
        for root in roots {
            if let Err(error) = self.pentaract_step(root) {
                tracing::error!(quest=root,%error,"Pentaract clock refused");
            }
        }
    }
    fn pentaract_step(&mut self, root: i32) -> Result<()> {
        let towers = self.quest_members(root, &[0xd5e]);
        let corpses = self.quest_members(root, &[0xd60]);
        ensure!(
            towers.len() + corpses.len() == 5,
            "Pentaract lost a tower identity"
        );
        if towers.is_empty() {
            for corpse in corpses {
                self.defeat_entity(corpse)?;
            }
            for eye in self.quest_members(root, &[0xd5d]) {
                self.remove_entity(eye);
            }
            self.finish_timed_defeat(root)?;
            tracing::info!(
                quest = root,
                "Pentaract completed after all five towers fell"
            );
        } else {
            for corpse in corpses {
                let e = &self.entities[&corpse];
                if e.conditions(self.now) & condition::bit(condition::STASIS) != 0 {
                    *self
                        .entities
                        .get_mut(&corpse)
                        .unwrap()
                        .activated
                        .as_mut()
                        .context("Pentaract corpse has no clock")? += 50;
                    continue;
                }
                if self.now - e.activated.context("Pentaract corpse has no clock")? >= 15000 {
                    let tower = self.quest_transform(corpse, 0xd5e)?;
                    let e = self.entities.get_mut(&tower).unwrap();
                    e.damage.clear();
                    e.stage = Some(0);
                    e.next_aux = self.now;
                    tracing::info!(quest = root, tower, "Pentaract tower revived");
                }
            }
        }
        Ok(())
    }
    fn event_target(&self, pos: Pos, range: f32) -> Option<Pos> {
        self.players
            .values()
            .filter(|p| {
                p.active()
                    && !p.paused
                    && p.conditions(self.now) & condition::bit(condition::INVISIBLE) == 0
            })
            .map(|p| p.pos)
            .chain(self.decoy_positions())
            .filter(|p| p.distance(pos) < range)
            .min_by(|a, b| a.distance(pos).total_cmp(&b.distance(pos)))
    }
    pub(super) fn event_movement(&self, id: i32, _t: u64) -> (Motion, f32) {
        let e = &self.entities[&id];
        let kind = e.kind;
        let own = |kind, radius| {
            self.quest_members(id, &[kind])
                .into_iter()
                .any(|id| self.entities[&id].pos.distance(e.pos) < radius)
        };
        let orbit = |kind| {
            let o = e
                .orbit
                .expect("event orbit parameters were not initialized at birth");
            (
                Motion::Orbit {
                    anchor: Anchor::QuestMember(kind),
                    radius: o.radius,
                },
                o.speed,
            )
        };
        match kind {
            0xd56 | 0xd5e..=0xd60 => (Motion::Stationary, 0.),
            0xd57 if own(0xd56, 30.) && !own(0xd56, 15.) => (
                Motion::Protect {
                    anchor: Anchor::QuestMember(0xd56),
                    distance: 15.,
                },
                movement_speed(0.3),
            ),
            0xd58 if own(0xd56, 40.) => orbit(0xd56),
            0xd5a if own(0xd59, 30.) => orbit(0xd59),
            0xd5b if own(0xd5a, 15.) => orbit(0xd5a),
            0xd5c if e.stage == Some(1) => (
                Motion::Chase {
                    sight: 10.,
                    stop: 1.,
                },
                movement_speed(0.75),
            ),
            0xd5c if own(0xd5a, 40.) => orbit(0xd5a),
            0xd5d if e.stage == Some(1) => (
                Motion::Orbit {
                    anchor: Anchor::Home,
                    radius: 8.,
                },
                movement_speed(2.),
            ),
            0xd5d if own(0xd5e, 20.) => (
                Motion::Protect {
                    anchor: Anchor::QuestMember(0xd5e),
                    distance: 6.,
                },
                movement_speed(2.),
            ),
            0xd5d => (Motion::Wander, movement_speed(2.) * 0.2),
            0xd58 => (Motion::Wander, movement_speed(1.5)),
            0xd5a => (Motion::Wander, movement_speed(0.375)),
            0xd5b | 0xd5c => (Motion::Wander, movement_speed(1.05)),
            _ => (Motion::Wander, movement_speed(0.3)),
        }
    }
    pub(super) fn event_shots(&self, id: i32, target: Pos, t: u64) -> Result<Vec<(u8, f32)>> {
        let e = &self.entities[&id];
        let heading = (target.y - e.pos.y).atan2(target.x - e.pos.x);
        let fan = |i, n| quest::fan(i, n, 10., heading);
        let mut shots = Vec::new();
        match e.kind {
            0xd56 | 0xd59 if t.is_multiple_of(750) => shots.extend(fan(0, 9)),
            0xd57 | 0xd58 if t.is_multiple_of(750) => shots.extend(fan(0, 2)),
            0xd5a if t.is_multiple_of(750) => shots.extend(fan(0, 4)),
            0xd5b if t.is_multiple_of(500) => shots.extend(fan(0, 1)),
            0xd5c if t.is_multiple_of(1500) => shots.extend(fan(0, 1)),
            0xd5d if t.is_multiple_of(1000) => shots.extend(fan(0, 1)),
            _ => {}
        }
        match e.kind {
            0xd59 if t.is_multiple_of(1500) => shots.extend(fan(1, 4)),
            0xd5a if t.is_multiple_of(1500) => shots.extend(fan(1, 1)),
            0xd5c if t.is_multiple_of(500) => shots.extend(fan(1, 2)),
            _ => {}
        }
        let prediction = match e.kind {
            0xd56 | 0xd5c => 1.,
            0xd59 => 0.5,
            _ => 0.,
        };
        self.predict_enemy_shots(id, target, &mut shots, |_| prediction)?;
        Ok(shots)
    }
}
