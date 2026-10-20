use super::*;
use crate::{
    condition,
    dungeon::manor::{self, *},
    encounter::Motion,
};

impl World {
    fn manor_child(
        &mut self,
        parent: i32,
        kind: u16,
        pos: Pos,
        lifetime: Option<u64>,
    ) -> Result<i32> {
        ensure!(
            self.actors().filter(|e| e.parent.is_some()).count() < 256,
            "Manor summon budget exhausted"
        );
        ensure!(
            self.walkable(pos),
            "Manor summon {kind:#x} has blocked landing at {pos:?}"
        );
        crate::encounter::validate(&self.content, kind)?;
        crate::loot::validate(&self.content, kind)?;
        let child = self.spawn_entity(kind, pos, vec![])?;
        let e = self.entities.get_mut(&child).unwrap();
        e.activated = Some(self.now);
        e.parent.set(Some(parent), child, &mut self.timed_ids);
        e.expires
            .set(lifetime.map(|ms| self.now + ms), child, &mut self.timed_ids);
        Ok(child)
    }
    fn manor_clear_swarms(&mut self, boss: i32) {
        let roots: BTreeSet<_> = self
            .actors()
            .filter(|e| e.parent == Some(boss) && matches!(e.kind, SWARM | DECOY))
            .map(|e| e.id)
            .collect();
        let children: Vec<_> = self
            .actors()
            .filter(|e| roots.contains(&e.id) || e.parent.is_some_and(|p| roots.contains(&p)))
            .map(|e| e.id)
            .collect();
        for id in children {
            self.remove_entity(id);
        }
    }
    pub(super) fn manor_mechanics(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<()> {
        let e = &self.entities[&id];
        let (kind, pos, home, hp) = (e.kind, e.pos, e.home, e.hp);
        if kind == BOSS {
            let (stage, age) = manor::phase(elapsed);
            let part = stage % 3;
            if e.stage != Some(stage) {
                if part == 0 {
                    self.manor_clear_swarms(id);
                }
                if part == 2 {
                    ensure!(
                        self.actors().filter(|e| e.parent.is_some()).count() + 49 <= 256,
                        "Ruthven bat formation exceeds summon budget"
                    );
                    let mut groups = vec![(SWARM, ESCORT, 15, pos)];
                    for i in 0..3 {
                        let a = i as f32 * std::f32::consts::TAU / 3.;
                        groups.push((
                            DECOY,
                            CHASER,
                            10,
                            Pos {
                                x: home.x + 3. * a.cos(),
                                y: home.y + 3. * a.sin(),
                            },
                        ));
                    }
                    ensure!(
                        groups.iter().all(|(_, _, _, p)| (0..32).all(|i| {
                            let a = i as f32 * std::f32::consts::TAU / 32.;
                            self.walkable(Pos {
                                x: p.x + 1.5 * a.cos(),
                                y: p.y + 1.5 * a.sin(),
                            })
                        })),
                        "Ruthven bat formation lacks clear arena"
                    );
                    for (root, bat, count, site) in groups {
                        let group = self.manor_child(id, root, site, None)?;
                        self.spawn_children(group, bat, count, 1.5)?;
                        self.entities.get_mut(&group).unwrap().stage = Some(0);
                    }
                }
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(stage);
                e.next_aux = 0;
                tracing::info!(
                    entity = id,
                    stage,
                    part,
                    hp,
                    "Ruthven transformation phase entered"
                );
            }
            let e = self.entities.get_mut(&id).unwrap();
            e.condition &=
                !(condition::bit(condition::INVULNERABLE) | condition::bit(condition::INVINCIBLE));
            if part != 0 {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
            e.extra_stats = vec![(
                61,
                if part == 2 {
                    1
                } else if part == 1 {
                    2
                } else {
                    0
                },
            )];
            if part == 1 && age >= 2650 && e.next_aux == 0 {
                ensure!(
                    self.actors().filter(|e| e.parent.is_some()).count() + 4 <= 256,
                    "Ruthven coffin formation exceeds summon budget"
                );
                ensure!(
                    [(12., 0.), (-12., 0.), (0., 12.), (0., -12.)]
                        .iter()
                        .all(|(dx, dy)| self.walkable(Pos {
                            x: home.x + dx,
                            y: home.y + dy
                        })),
                    "Ruthven coffin formation lacks clear arena"
                );
                for (dx, dy) in [(12., 0.), (-12., 0.), (0., 12.), (0., -12.)] {
                    let site = Pos {
                        x: home.x + dx,
                        y: home.y + dy,
                    };
                    self.telegraph_throw(id, site, 0xff8f3030);
                    self.manor_child(id, CREATURE, site, Some(20000))?;
                }
                self.entities.get_mut(&id).unwrap().next_aux = 1;
            }
        } else if matches!(kind, SWARM | DECOY) && e.stage.is_none() {
            let (bat, n) = if kind == SWARM {
                (ESCORT, 15)
            } else {
                (CHASER, 10)
            };
            self.spawn_children(id, bat, n, 1.5)?;
            self.entities.get_mut(&id).unwrap().stage = Some(0);
        } else if kind == COFFIN {
            if hp < self.content.objects[&COFFIN].hp && e.next_aux == 0 {
                self.entities.get_mut(&id).unwrap().next_aux = 1;
                if self.rng.gen_ratio(1, 5) {
                    let new = self.transform_fauna(id, CREATURE)?;
                    self.entities.get_mut(&new).unwrap().expires.set(
                        Some(self.now + 20000),
                        new,
                        &mut self.timed_ids,
                    );
                    tracing::info!(
                        entity = id,
                        replacement = new,
                        "false treasure coffin awakened without rewards"
                    );
                    return Ok(());
                }
            }
            let e = &self.entities[&id];
            let stage = if hp <= 8000 {
                2
            } else if hp <= 15000 {
                1
            } else {
                0
            };
            let old = e.stage.unwrap_or(0);
            if old < 1 && stage >= 1 {
                self.spawn_children(id, BAT, 15, 1.5)?;
            }
            if old < 2 && stage == 2 {
                self.spawn_children(id, BAT, 23, 2.)?;
                self.spawn_children(id, 0x1728, 2, 3.)?;
            }
            self.entities.get_mut(&id).unwrap().stage = Some(stage);
        } else if kind == CREATURE && e.expires.is_none() {
            self.entities.get_mut(&id).unwrap().expires.set(
                Some(self.now + 20000),
                id,
                &mut self.timed_ids,
            );
        } else if kind == 0x1729
            && self.now >= e.next_aux
            && let Some(site) = target
        {
            if self.walkable(site) {
                self.telegraph_throw(id, site, 0xffffaa00);
                self.manor_child(id, ROCK, site, Some(3050))?;
            }
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 3000;
        } else if kind == ROCK && e.expires.is_none() {
            self.entities.get_mut(&id).unwrap().expires.set(
                Some(self.now + 3050),
                id,
                &mut self.timed_ids,
            );
        }
        Ok(())
    }
    pub(super) fn manor_movement(&self, id: i32, elapsed: u64) -> (Motion, f32) {
        let e = &self.entities[&id];
        if e.kind == BOSS {
            match manor::phase(elapsed).0 % 3 {
                0 => (Motion::WanderHome { radius: 3. }, 2.),
                1 => (
                    Motion::Protect {
                        anchor: crate::encounter::Anchor::Home,
                        distance: 0.1,
                    },
                    8.,
                ),
                2 => (
                    Motion::Chase {
                        sight: 8.,
                        stop: 2.,
                    },
                    6.,
                ),
                _ => unreachable!(),
            }
        } else {
            let p = manor::profile(e.kind);
            (p.motion, p.speed)
        }
    }
    pub(super) fn manor_death(&mut self, e: &Entity) -> Result<()> {
        if e.kind == BOSS {
            self.manor_clear_swarms(e.id);
            let children: Vec<_> = self
                .actors()
                .filter(|x| x.parent == Some(e.id))
                .map(|e| e.id)
                .collect();
            for child in children {
                self.remove_entity(child);
            }
        } else if e.kind == CREATURE {
            let count = self.rng.gen_range(1..=4);
            for i in 0..count {
                let a = i as f32 * std::f32::consts::TAU / count as f32;
                let pos = Pos {
                    x: e.pos.x + 0.2 * a.cos(),
                    y: e.pos.y + 0.2 * a.sin(),
                };
                if let Some(parent) = *e.parent {
                    self.manor_child(parent, FERATU, pos, Some(30000))?;
                } else {
                    self.spawn_entity(FERATU, pos, vec![])?;
                }
            }
            tracing::debug!(entity = e.id, count, "Coffin Creature released Lil Feratus");
        }
        Ok(())
    }
}
