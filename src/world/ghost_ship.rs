use super::*;
use crate::{
    condition,
    encounter::{Anchor, Motion, nr_speed},
};
impl World {
    pub(super) fn ghost_ship_mechanics(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<()> {
        let kind = self.entities[&id].kind;
        if kind != 0xe37 {
            return self.ghost_follower_mechanics(id, target, elapsed);
        }
        if self.entities[&id].stage.is_none() {
            let unbound = self
                .entities
                .values()
                .filter(|e| e.kind == 0xe38 && e.quest.is_none())
                .map(|e| e.id)
                .collect::<Vec<_>>();
            for anchor in unbound {
                self.landmark_bind(anchor)?;
            }
            ensure!(
                self.landmark_members(id, 0xe38).len() == 1,
                "Ghost Ship requires exactly one encounter-owned anchor"
            );
            self.landmark_enter(id, 0);
        }
        let e = &self.entities[&id];
        let stage = e.stage.unwrap();
        let age = self.now - e.activated.context("ship phase clock missing")?;
        let max = self.content.objects[&kind].hp;
        let next = match stage {
            0 if e.hp < max * 98 / 100 => Some(1),
            1 if age >= 3000 => Some(2),
            2 if age >= 300 => Some(3),
            3 if e.hp < max * 90 / 100 => Some(4),
            4 if age >= 2000 => Some(5),
            5 if e.hp < max * 70 / 100 => Some(6),
            5 if age >= 9500 => Some(4),
            6 if e.hp < max / 2 => Some(7),
            7 if e.hp < max * 4 / 10 => Some(8),
            8 if !self.landmark_members(id, 0xe3b).is_empty() => Some(9),
            9 if e.hp < max / 5 => Some(10),
            10 if age >= 100 => Some(11),
            0..=11 => None,
            _ => bail!("invalid ship phase"),
        };
        if let Some(next) = next {
            self.landmark_enter(id, next);
            let e = self.entities.get_mut(&id).unwrap();
            if !matches!(next, 4 | 5) {
                e.next_aux = self.now + if matches!(next, 6 | 9) { 3000 } else { 0 };
                e.reaction_until = self.now + 4000;
            }
        }
        let e = &self.entities[&id];
        let stage = e.stage.unwrap();
        let age = self.now - e.activated.unwrap();
        let shield = match stage {
            1 | 2 | 4 => true,
            5 => {
                let t = age % 9500;
                (2000..4500).contains(&t) || (5500..6500).contains(&t)
            }
            6 | 7 | 9 | 11 => age % 5000 < 2000,
            _ => false,
        };
        // The later table shields Phase2 through its parent, which makes its damage transition unreachable.
        self.landmark_shield(id, shield);
        self.landmark_texture(
            id,
            match stage {
                0 => 1,
                2 => 2,
                _ => 0,
            },
        );
        let e = self.entities.get_mut(&id).unwrap();
        e.orbit.get_or_insert(realm_events::Orbit {
            speed: nr_speed(0.2),
            radius: 2.,
            direction: -1.,
        });
        if matches!(stage, 3..=6 | 9) && self.now >= self.entities[&id].reaction_until {
            let born = self.landmark_birth(id, 0xe39, 4, 1, 1.)?;
            for child in born {
                self.entities
                    .get_mut(&child)
                    .unwrap()
                    .script
                    .get_or_insert_with(landmark_ai::Script::default)
                    .no_experience = true;
            }
            self.entities.get_mut(&id).unwrap().reaction_until = self.now + 4000;
        }
        let e = &self.entities[&id];
        if matches!(stage, 3..=7 | 9 | 11) && self.now >= e.next_aux {
            if matches!(stage, 6 | 9) {
                let born = self.landmark_birth(id, 0xe4f, 5, 1, 1.)?;
                for child in born {
                    self.entities
                        .get_mut(&child)
                        .unwrap()
                        .script
                        .get_or_insert_with(landmark_ai::Script::default)
                        .no_experience = true;
                }
                self.entities.get_mut(&id).unwrap().next_aux = self.now + 3000;
            } else if e.conditions(self.now) & condition::bit(condition::STUNNED) == 0 {
                let p = e.pos;
                let r = self.rng.gen_range(3. ..=7.);
                let a = 1f32.to_radians();
                self.named_toss(
                    id,
                    0xe4f,
                    5,
                    &[Pos {
                        x: p.x + r * a.cos(),
                        y: p.y + r * a.sin(),
                    }],
                )?;
                self.entities.get_mut(&id).unwrap().next_aux = self.now + 5000;
            }
        }
        if stage == 8
            && self.now >= self.entities[&id].next_aux
            && self.entities[&id].conditions(self.now) & condition::bit(condition::STUNNED) == 0
        {
            let positions = self.landmark_circle(id, 8., 10);
            self.named_toss(id, 0xe3b, 10, &positions)?;
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 10000;
        }
        Ok(())
    }
    fn ghost_follower_mechanics(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<()> {
        let kind = self.entities[&id].kind;
        let owner = self
            .landmark_members(id, 0xe37)
            .into_iter()
            .next()
            .filter(|owner| {
                self.entities[&id].pos.distance(self.entities[owner].pos)
                    < match kind {
                        0xe3b => 30.,
                        0xe3c => 60.,
                        _ => 20.,
                    }
            });
        if self.entities[&id].stage.is_none() {
            self.landmark_enter(id, 0);
        }
        match kind {
            0xe38 | 0xe3c => {
                self.entities.get_mut(&id).unwrap().condition |=
                    condition::bit(condition::INVINCIBLE);
                let Some(owner) = owner else {
                    self.landmark_fade(id, 50);
                    return Ok(());
                };
                if kind == 0xe3c
                    && self.entities[&owner].stage.is_some_and(|stage| stage >= 2)
                    && target.is_some_and(|p| self.entities[&id].pos.distance(p) < 7.)
                    && self.now >= self.entities[&id].next_aux
                {
                    if !self
                        .entities
                        .values()
                        .any(|e| e.kind == 0xe3d && e.parent == Some(id))
                    {
                        let born = self.landmark_birth(id, 0xe3d, 4, 1, 1.)?;
                        for child in born {
                            let e = self.entities.get_mut(&child).unwrap();
                            e.parent.set(Some(id), child, &mut self.timed_ids);
                            e.script
                                .get_or_insert_with(landmark_ai::Script::default)
                                .no_experience = true;
                        }
                    }
                    self.entities.get_mut(&id).unwrap().next_aux = self.now + 1000;
                }
            }
            0xe39 => {
                let e = self.entities.get_mut(&id).unwrap();
                e.size = (e.size + 20).min(100);
                if owner.is_none() {
                    self.landmark_fade(id, 50);
                }
            }
            0xe3b => {
                self.entities.get_mut(&id).unwrap().condition |=
                    condition::bit(condition::INVINCIBLE);
                let stage = self.entities[&id].stage.unwrap();
                if owner.is_none() && stage != 2 {
                    self.landmark_enter(id, 2);
                    self.landmark_fade(id, 2000);
                } else if stage == 0 && elapsed >= 2000 {
                    self.landmark_enter(id, 1);
                }
                let e = &self.entities[&id];
                let stage = e.stage.unwrap();
                let age = self.now - e.activated.unwrap();
                self.landmark_texture(
                    id,
                    if stage == 2 {
                        (9 - age as i32 / 200).max(0)
                    } else {
                        (age / 200 % 9 + 1) as i32
                    },
                );
                let e = self.entities.get_mut(&id).unwrap();
                e.size = (e.size + 10).min(140);
                e.orbit.get_or_insert(realm_events::Orbit {
                    speed: nr_speed(0.4),
                    radius: 8.,
                    direction: -1.,
                });
            }
            0xe4f => {
                let e = self.entities.get_mut(&id).unwrap();
                if e.expires.is_none() {
                    e.expires
                        .set(Some(self.now + 10000), id, &mut self.timed_ids);
                }
                if e.stage == Some(0)
                    && (owner.is_none() || target.is_some_and(|p| e.pos.distance(p) < 3.))
                {
                    self.landmark_enter(id, 1);
                    self.landmark_fade(id, 50);
                }
            }
            0xe3d => {
                let e = &self.entities[&id];
                if e.stage == Some(0) && target.is_none_or(|p| e.pos.distance(p) >= 7.) {
                    self.landmark_enter(id, 1);
                    self.landmark_fade(id, 3000);
                }
                let e = self.entities.get_mut(&id).unwrap();
                if e.stage == Some(1) {
                    e.condition |= condition::bit(condition::INVINCIBLE);
                    e.size = (e.size - 20).max(0);
                } else {
                    e.size = (e.size + 20).min(100);
                }
            }
            _ => bail!("unsupported ship follower"),
        }
        Ok(())
    }
    pub(super) fn ghost_ship_movement(&self, id: i32) -> Result<(Motion, f32)> {
        let e = &self.entities[&id];
        if e.kind == 0xe37 && e.pos.distance(e.home) > 7. {
            return Ok((
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 7.,
                },
                nr_speed(0.4),
            ));
        }
        Ok(match e.kind {
            0xe37 => match e.stage.context("ship movement phase missing")? {
                0 => (
                    Motion::Orbit {
                        anchor: Anchor::QuestMember(0xe38),
                        radius: 2.,
                    },
                    nr_speed(0.2),
                ),
                1 | 2 | 6 | 9 => (
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 0.,
                    },
                    nr_speed(if matches!(e.stage, Some(1 | 2)) {
                        0.5
                    } else {
                        0.4
                    }),
                ),
                4 if self.now - e.activated.unwrap() < 1000 => (
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 0.,
                    },
                    nr_speed(0.3),
                ),
                4 => (Motion::Wander, nr_speed(0.3)),
                3 | 5 => (
                    Motion::Follow {
                        sight: 10.,
                        stop: 3.,
                        idle: nr_speed(0.3),
                        duration: 0,
                        cooldown: 0,
                    },
                    nr_speed(0.2),
                ),
                7 | 11 => (
                    Motion::Follow {
                        sight: 10.,
                        stop: 3.,
                        idle: nr_speed(0.4),
                        duration: 0,
                        cooldown: 0,
                    },
                    nr_speed(0.2),
                ),
                _ => (Motion::Stationary, 0.),
            },
            0xe3b if e.stage != Some(1) || self.landmark_members(id, 0xe37).is_empty() => {
                (Motion::Stationary, 0.)
            }
            0xe3d if e.stage == Some(1) => (Motion::Stationary, 0.),
            0xe4f if e.stage == Some(1) => (Motion::Stationary, 0.),
            kind => {
                let p = crate::encounter::landmarks::profile(kind);
                (p.motion, p.speed)
            }
        })
    }
}
