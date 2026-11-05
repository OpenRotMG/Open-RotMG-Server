//! Oryx the Mad God 1 in his chamber: the brain (`endgame::oryx1`) drives taunts, summons, the rage heal and his movement here.
use super::*;
use crate::{condition, endgame as finale};
use finale::oryx1::{self, Event, Phase};

impl World {
    pub(super) fn oryx_step(&mut self, id: i32) -> Result<()> {
        let (hp, now) = (self.entities[&id].hp, self.now);
        let mut brain = self
            .finale
            .oryx
            .remove(&id)
            .unwrap_or_else(|| oryx1::Brain::new(now));
        let events = brain.update(hp, now, &mut self.rng);
        let (phase, invulnerable, healing) = (brain.phase(), brain.invulnerable(), brain.healing());
        self.finale.oryx.insert(id, brain);
        let e = self.entities.get_mut(&id).unwrap();
        e.condition &= !condition::bit(condition::INVULNERABLE);
        if invulnerable {
            e.condition |= condition::bit(condition::INVULNERABLE);
        }
        // Armored from the rage to the end (Japanese wiki; old-wiki guide "given extra armor").
        if phase == Phase::Rage && !healing {
            e.condition |= condition::bit(condition::ARMORED);
        }
        for event in events {
            match event {
                Event::Say(text) => self.oryx_say(id, &text),
                Event::Begin(next) => {
                    self.entities.get_mut(&id).unwrap().next_shot = now;
                    tracing::info!(entity = id, phase = ?next, hp, "Oryx phase committed");
                    self.oryx_begin(id, next)?;
                }
                Event::Toss(phase) => self.oryx_toss(id, phase),
                Event::Dismiss => {
                    // Leftover Minions vanish within 1-2 s of the end line; Assassins stay until killed (tg7tAkfKGUE 2012-09-12).
                    let minions: Vec<i32> =
                        self.summoned_children(id, 0x1749).map(|e| e.id).collect();
                    for minion in minions {
                        let at = now + self.rng.gen_range(500..=1500);
                        self.entities.get_mut(&minion).unwrap().expires.set(
                            Some(at),
                            minion,
                            &mut self.timed_ids,
                        );
                    }
                }
                Event::Green => self.npc_flash(id, 0xff00ff00, 3.5, 3.5),
                Event::Red => {
                    self.npc_flash(id, 0xffff0000, 0.3, oryx1::RAGE_HEAL_MS as f32 / 1000.)
                }
                Event::Heal(amount) => {
                    self.heal_enemy(id, id, amount)?;
                    let e = self.entities.get_mut(&id).unwrap();
                    e.size = (e.size + oryx1::GROWTH).min(oryx1::RAGE_SIZE);
                }
                Event::Unleash => {
                    let e = self.entities.get_mut(&id).unwrap();
                    e.size = oryx1::RAGE_SIZE;
                    e.next_aux = now;
                }
            }
        }
        let landed: Vec<_> = self
            .finale
            .tosses
            .extract_if(.., |toss| toss.parent == id && now >= toss.at)
            .collect();
        for toss in landed {
            self.oryx_place(id, toss.kind, vec![toss.pos], oryx1::lifetime(toss.kind))?;
        }
        if now < self.entities[&id].next_aux {
            return Ok(());
        }
        let age = self.finale.oryx[&id].age(now);
        match phase {
            Phase::Rage if !healing => {
                self.entities.get_mut(&id).unwrap().next_aux = now + oryx1::RAGE_SUMMON_MS;
                self.oryx_summon(id, 0x1741, oryx1::RAGE_ASSASSINS)?;
                self.oryx_summon(id, 0x1749, oryx1::RAGE_MINIONS)?;
            }
            // Three red bombs every ≈1.3 s from ≈4.5 s to ≈22.5 s, landing 3.4-5.4 tiles out on open floor in a Y (M3Of4Q7Ag7Q t=449.5-467.6;
            // "3 at once in a symmetrical triangle", old-wiki guide). The dance throws the same bombs per three texts (not seen in footage).
            Phase::Silence | Phase::Dance if (4_500..22_500).contains(&age) => {
                self.entities.get_mut(&id).unwrap().next_aux = now + oryx1::BOMB_MS;
                let centre = self.entities[&id].pos;
                let turn = self.rng.gen_range(0. ..std::f32::consts::TAU);
                for k in 0..3 {
                    let angle = turn + k as f32 * std::f32::consts::TAU / 3.;
                    let reach = self.rng.gen_range(3.4..=5.4);
                    let at = Pos {
                        x: centre.x + reach * angle.cos(),
                        y: centre.y + reach * angle.sin(),
                    };
                    self.throw_grenade(id, at, oryx1::BOMB_RADIUS, oryx1::BOMB_DAMAGE)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Speech bubble and chat line, like every 2012 taunt.
    fn oryx_say(&self, id: i32, text: &str) {
        tracing::info!(entity = id, %text, "Oryx taunt");
        let packet = chat::text_packet("#Oryx the Mad God", id, 5, text);
        for p in self.players.values() {
            send(p, packet.id, packet.body.clone());
        }
    }
    fn oryx_begin(&mut self, id: i32, phase: Phase) -> Result<()> {
        match phase {
            Phase::Opening => self.oryx_summon(id, 0x1749, oryx1::OPENING_MINIONS)?,
            Phase::Minions => {
                self.oryx_summon(id, 0x1749, oryx1::PHASE_MINIONS)?;
                self.oryx_summon(id, 0x1741, oryx1::PHASE_ASSASSINS)?;
            }
            _ => {}
        }
        Ok(())
    }
    /// Throw the phase's summons to their slots around Oryx; each lands oryx1::TOSS_MS later.
    fn oryx_toss(&mut self, id: i32, phase: Phase) {
        use std::f32::consts::{FRAC_PI_4, TAU};
        let centre = self.entities[&id].pos;
        let turn = self.rng.gen_range(0. ..TAU);
        let ring = |kind: u16, count: usize, offset: f32| -> Vec<(u16, Pos)> {
            let radius = oryx1::orbit(kind).map_or(oryx1::RING_RADIUS, |o| o.0);
            (0..count)
                .map(|i| {
                    let angle = offset + i as f32 * TAU / count as f32;
                    (
                        kind,
                        Pos {
                            x: centre.x + radius * angle.cos(),
                            y: centre.y + radius * angle.sin(),
                        },
                    )
                })
                .collect()
        };
        let slots: Vec<(u16, Pos)> = match phase {
            // 15 Ring Elements 24° apart at 8.8 tiles, standing still (tg7tAkfKGUE t=166-175.5, 337 detections), and the Anti-Spectators
            // (present in both phases; 3-4 seen in the dance, 4 per the Japanese wiki).
            Phase::Silence | Phase::Dance => {
                [ring(0x1748, 15, turn), ring(0x1747, 4, turn)].concat()
            }
            // Four near artifacts in a cross, one of each Guardian Element, and three far ones (the 2012 wiki and the Japanese wiki; at most
            // two were ever in view at once).
            Phase::Artifacts => {
                let near = ring(0x174a, 4, turn)
                    .into_iter()
                    .enumerate()
                    .map(|(i, (kind, pos))| (kind + i as u16, pos));
                near.chain(ring(0x174e, 3, turn)).collect()
            }
            Phase::Planets => ring(0x1743, 4, turn),
            Phase::Cosmic => [ring(0x1746, 4, turn), ring(0x1745, 4, turn + FRAC_PI_4)].concat(),
            // A 45°-rotated square lattice centred where Oryx stands: neighbours ≈7.7 tiles apart on the diagonals, none on the axes
            // 5.4 tiles out (ZdjeB9t-n5w t=351.2-352.0); the Japanese wiki's 13 spots = |u|, |v| ≤ 2 with u + v even.
            Phase::Grid => (-2..=2i32)
                .flat_map(|u| (-2..=2i32).map(move |v| (u, v)))
                .filter(|(u, v)| (u + v) % 2 == 0)
                .map(|(u, v)| {
                    (
                        0x1744,
                        Pos {
                            x: centre.x + u as f32 * 5.45,
                            y: centre.y + v as f32 * 5.45,
                        },
                    )
                })
                .collect(),
            _ => vec![],
        };
        let at = self.now + oryx1::TOSS_MS;
        for (kind, pos) in slots {
            if !self.walkable(pos) {
                continue;
            }
            self.telegraph_throw(id, pos, 0xffffff00);
            self.finale.tosses.push(super::endgame_ai::Toss {
                at,
                parent: id,
                kind,
                pos,
            });
        }
    }
    /// Minions and Assassins burst out on or within ≈2-4 tiles of Oryx; the whole wave at once.
    fn oryx_summon(&mut self, id: i32, kind: u16, count: usize) -> Result<()> {
        self.finale_wave(
            id,
            finale::Wave {
                kind,
                count,
                radius: 2.,
                lifetime: if kind == 0x1749 { 20_000 } else { 60_000 },
            },
        )
    }
    fn oryx_place(&mut self, id: i32, kind: u16, points: Vec<Pos>, lifetime: u64) -> Result<()> {
        crate::encounter::validate(&self.content, kind)?;
        crate::loot::validate(&self.content, kind)?;
        for pos in points {
            if !self.walkable(pos) {
                continue;
            }
            let child = self.spawn_entity(kind, pos, vec![])?;
            let now = self.now;
            let e = self.entities.get_mut(&child).unwrap();
            e.parent.set(Some(id), child, &mut self.timed_ids);
            e.expires
                .set(Some(now + lifetime), child, &mut self.timed_ids);
            e.activated = Some(now);
            if let Some((radius, speed, direction)) = oryx1::orbit(kind) {
                e.orbit = Some(realm_events::Orbit {
                    speed: finale::tiles_per_s(speed),
                    radius,
                    direction,
                });
            }
            if oryx1::shielded(kind) {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
        }
        Ok(())
    }
    pub(super) fn oryx_orbit(&mut self, id: i32) -> (crate::encounter::Motion, f32) {
        let now = self.now;
        let e = self.entities.get_mut(&id).unwrap();
        let age = now - e.activated.unwrap_or(now);
        let radius = oryx1::orbit_radius(e.kind, age);
        let orbit = e
            .orbit
            .as_mut()
            .expect("orbiting Oryx summon without an orbit");
        if let Some(radius) = radius {
            orbit.radius = radius;
        }
        (
            crate::encounter::Motion::Orbit {
                anchor: crate::encounter::Anchor::Parent,
                radius: orbit.radius,
            },
            orbit.speed,
        )
    }
    pub(super) fn oryx_motion(&self, id: i32) -> Result<(crate::encounter::Motion, f32)> {
        use crate::encounter::{Anchor, Motion};
        let e = &self.entities[&id];
        let brain = self
            .finale
            .oryx
            .get(&id)
            .context("moving Oryx brain missing")?;
        let speed = finale::tiles_per_s(oryx1::SPEED);
        Ok(match brain.phase() {
            Phase::Alone => (Motion::Stationary, 0.),
            Phase::Rage if brain.healing() => (Motion::Stationary, 0.),
            // He moves toward the players in minion phases (ZdjeB9t-n5w t=140-144) and chases the nearest one in the grid and the rage.
            Phase::Opening | Phase::Minions => (
                Motion::Follow {
                    sight: 12.,
                    stop: 4.,
                    idle: 0.,
                    duration: 0,
                    cooldown: 0,
                },
                speed,
            ),
            Phase::Grid if brain.age(self.now) >= oryx1::GRID_MS => (
                Motion::Chase {
                    sight: 15.,
                    stop: 1.,
                },
                speed,
            ),
            Phase::Rage => (
                Motion::Chase {
                    sight: 20.,
                    stop: 1.,
                },
                speed,
            ),
            Phase::Grid => (Motion::Stationary, 0.),
            // Every other phase is meant to run at the room centre ("If he does not manage to get to the center of the chamber before
            // spawning planets…", 2012 wiki; "returns to the middle of the room" after the chase, Japanese wiki).
            _ => (
                Motion::Protect {
                    anchor: Anchor::Point {
                        x: e.home.x,
                        y: e.home.y,
                    },
                    distance: 0.2,
                },
                speed,
            ),
        })
    }
    #[cfg(test)]
    fn oryx_count(&self, id: i32, kinds: std::ops::RangeInclusive<u16>) -> usize {
        self.actors()
            .filter(|e| e.parent == Some(id) && kinds.contains(&e.kind))
            .count()
    }
    /// Every Minion, Assassin and element is gone within 0.5 s of his death (exMHglbJ98E).
    pub(super) fn oryx_defeated(&mut self, id: i32) {
        self.finale.oryx.remove(&id);
        self.finale.tosses.retain(|toss| toss.parent != id);
        let summons: Vec<i32> = self
            .timed_ids
            .iter()
            .filter(|child| self.entities[child].parent == Some(id))
            .copied()
            .collect();
        for child in summons {
            let now = self.now;
            self.entities.get_mut(&child).unwrap().expires.set(
                Some(now),
                child,
                &mut self.timed_ids,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn run(world: &mut World, ms: u64) {
        for _ in 0..ms / 50 {
            world.step();
        }
    }

    #[test]
    fn oryx_wakes_on_damage_tosses_each_phase_and_rages() {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let map = Map::load(std::path::Path::new("data/oryx-chamber.wmap"), &content).unwrap();
        let mut world = World::new(content.clone(), Arc::new(map)).unwrap();
        // Phase coverage must not depend on the World's entropy seed.
        world.rng = rand::rngs::StdRng::seed_from_u64(7);
        world.name = "Oryx's Chamber".into();
        world.encounter = true;
        world.prepare_endgame().unwrap();
        let mut character = Character::fresh(content.get(0x307).unwrap()).unwrap();
        character.account = 1;
        character.id = 1;
        let player = world.enter(character, None).unwrap();
        world
            .apply_condition(player, condition::INVINCIBLE, 3_600_000)
            .unwrap();
        let oryx = world
            .entities
            .values()
            .find(|e| e.kind == finale::ORYX)
            .unwrap()
            .id;
        let home = world.entities[&oryx].home;
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: home.x,
            y: home.y + 6.,
        };
        // Alone until he has lost ≈3 %, then the wake burst of Minions, gone after the end line.
        run(&mut world, 2000);
        assert_eq!(world.finale.oryx[&oryx].phase(), Phase::Alone);
        assert_eq!(world.oryx_count(oryx, 0x1741..=0x174e), 0);
        world.entities.get_mut(&oryx).unwrap().hp = 82_000;
        run(&mut world, 1000);
        assert_eq!(world.finale.oryx[&oryx].phase(), Phase::Opening);
        assert_eq!(
            world.oryx_count(oryx, 0x1749..=0x1749),
            oryx1::OPENING_MINIONS
        );
        run(&mut world, 17_000);
        assert_eq!(world.oryx_count(oryx, 0x1749..=0x1749), 0);
        // Each phase's tossed summons are on the floor once they land.
        let (mut checked, mut low) = (BTreeSet::new(), BTreeSet::new());
        for step in 0..9_000 {
            if step == 4_500 {
                world.entities.get_mut(&oryx).unwrap().hp = 30_000;
            }
            world.step();
            let brain = &world.finale.oryx[&oryx];
            let (phase, age) = (brain.phase(), brain.age(world.now));
            if step > 4_500 && age < 50 {
                low.insert(phase);
            }
            let count = |kinds| world.oryx_count(oryx, kinds);
            let landed = match phase {
                Phase::Silence | Phase::Dance if (2_000..2_050).contains(&age) => {
                    count(0x1748..=0x1748) == 15 && count(0x1747..=0x1747) == 4
                }
                Phase::Artifacts if (7_000..7_050).contains(&age) => {
                    count(0x174a..=0x174d) == 4 && count(0x174e..=0x174e) == 3
                }
                Phase::Planets if (3_000..3_050).contains(&age) => count(0x1743..=0x1743) == 4,
                Phase::Cosmic if (8_200..8_250).contains(&age) => {
                    count(0x1746..=0x1746) == 4 && count(0x1745..=0x1745) == 4
                }
                Phase::Grid if (3_700..3_750).contains(&age) => count(0x1744..=0x1744) == 13,
                Phase::Minions if (500..550).contains(&age) => {
                    count(0x1749..=0x1749) == oryx1::PHASE_MINIONS
                        && count(0x1741..=0x1741) >= oryx1::PHASE_ASSASSINS
                }
                _ => continue,
            };
            assert!(landed, "{phase:?} summons missing at {age} ms");
            checked.insert(phase);
        }
        assert!(checked.len() >= 5, "only {checked:?} ran");
        assert!(!low.contains(&Phase::Silence) && !low.contains(&Phase::Artifacts));
        // At 5000 HP: heal 7 × 2500 while invulnerable, grow, then Armored with Assassins and Minions.
        world.entities.get_mut(&oryx).unwrap().hp = 4_000;
        run(&mut world, 1000);
        assert_eq!(world.finale.oryx[&oryx].phase(), Phase::Rage);
        assert_ne!(
            world.entities[&oryx].conditions(world.now) & condition::bit(condition::INVULNERABLE),
            0
        );
        run(&mut world, 4000);
        let e = &world.entities[&oryx];
        assert_eq!(e.hp, 4_000 + 7 * oryx1::HEAL);
        assert_eq!(e.size, oryx1::RAGE_SIZE);
        assert_ne!(
            e.conditions(world.now) & condition::bit(condition::ARMORED),
            0
        );
        assert_eq!(
            e.conditions(world.now) & condition::bit(condition::INVULNERABLE),
            0
        );
        assert!(world.oryx_count(oryx, 0x1741..=0x1741) >= oryx1::RAGE_ASSASSINS);
    }
}
