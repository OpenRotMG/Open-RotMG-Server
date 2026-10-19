use super::*;
use crate::{condition, endgame as finale};
use finale::{BossBrain, GUARDIANS, GuardianBrain, ORYX, ORYX2, OryxPhase};

#[derive(Default)]
pub(super) struct Actors {
    pub(super) bosses: BTreeMap<i32, BossBrain>,
    guardians: BTreeMap<i32, GuardianBrain>,
}

impl World {
    fn hero_reinforcements(&mut self, id: i32, initial: bool) -> Result<()> {
        let waves = finale::summons(self.entities[&id].kind);
        ensure!(!waves.is_empty(), "hero reinforcement roster missing");
        let selected = if initial {
            None
        } else {
            Some(self.rng.gen_range(0..=waves.len()))
        };
        for (index, wave) in waves.iter().enumerate() {
            if selected.is_some_and(|choice| choice != index) {
                continue;
            }
            let current = self
                .entities
                .values()
                .filter(|e| e.parent == Some(id) && e.kind == wave.kind)
                .count();
            let count = (current + self.rng.gen_range(1..=3)).min(wave.count);
            self.finale_wave(id, finale::Wave { count, ..*wave })?;
        }
        Ok(())
    }
    fn armor_mechanics(&mut self, id: i32) -> Result<()> {
        let e = &self.entities[&id];
        let old = e.stage;
        let mut stage = old.unwrap_or(1);
        if stage == 1 && e.hp < self.content.objects[&e.kind].hp / 5 {
            stage = 2;
            let e = self.entities.get_mut(&id).unwrap();
            e.reaction_until = self.now + 1500;
            e.next_aux = self.now;
        } else if stage == 2 && self.now >= e.reaction_until {
            stage = 1;
        }
        let e = self.entities.get_mut(&id).unwrap();
        e.stage = Some(stage);
        e.condition &= !condition::bit(condition::INVULNERABLE);
        if stage == 2 {
            e.condition |= condition::bit(condition::INVULNERABLE);
        }
        if old != Some(stage) {
            e.extra_stats.retain(|(stat, _)| *stat != 61);
            e.extra_stats.push((61, i32::from(stage == 1)));
            tracing::debug!(entity = id, stage, "Suit of Armor phase entered");
        }
        if stage == 2 && self.now >= e.next_aux {
            e.next_aux = self.now + 50;
            self.heal_enemy(id, id, 200)?;
        }
        Ok(())
    }
    fn finale_wave(&mut self, parent: i32, wave: finale::Wave) -> Result<()> {
        ensure!(
            wave.lifetime > 0,
            "endgame wave requires a bounded lifetime"
        );
        self.spawn_children(parent, wave.kind, wave.count, wave.radius)?;
        for e in self
            .entities
            .values_mut()
            .filter(|e| e.parent == Some(parent) && e.kind == wave.kind && e.expires.is_none())
        {
            e.expires
                .set(Some(self.now + wave.lifetime), e.id, &mut self.timed_ids);
            e.activated.get_or_insert(self.now);
            if matches!(e.kind, 0x174a | 0x174b | 0x944) {
                let rate = match e.kind {
                    0x174a => 1.,
                    0x174b => 1.3,
                    0x944 => 0.1,
                    _ => unreachable!(),
                };
                e.orbit = Some(realm_events::Orbit {
                    speed: crate::encounter::nr_speed(
                        rate + self.rng.gen_range(-rate * 0.1..rate * 0.1),
                    ),
                    radius: if e.kind == 0x944 {
                        self.rng.gen_range(0.5..1.5)
                    } else if e.kind == 0x174a {
                        1.
                    } else {
                        9.
                    },
                    direction: -1.,
                });
            }
            if matches!(e.kind, 0x1748 | 0x174a | 0x174b | 0xde4) {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
        }
        Ok(())
    }
    pub(super) fn finale_mechanics(
        &mut self,
        id: i32,
        target: impl Into<Option<Pos>>,
        elapsed: u64,
    ) -> Result<()> {
        let target = target.into();
        let e = &self.entities[&id];
        let (kind, hp, pos) = (e.kind, e.hp, e.pos);
        if GUARDIANS.contains(&kind) {
            let peer = self
                .entities
                .values()
                .any(|e| e.id != id && GUARDIANS.contains(&e.kind) && e.hp > 0);
            let idle = peer
                && self.entities[&id].stage.is_none_or(|s| s == 0)
                && target.is_none_or(|p| pos.distance(p) > 10.);
            let facts = if idle {
                None
            } else {
                Some(
                    self.finale
                        .guardians
                        .entry(id)
                        .or_default()
                        .update(peer, self.now)?,
                )
            };
            let e = self.entities.get_mut(&id).unwrap();
            e.stage = Some(if idle { 0 } else { 1 });
            e.condition &=
                !(condition::bit(condition::PARALYZED) | condition::bit(condition::INVULNERABLE));
            if idle {
                e.condition |=
                    condition::bit(condition::PARALYZED) | condition::bit(condition::INVULNERABLE);
            }
            let Some(facts) = facts else { return Ok(()) };
            if facts.invulnerable {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
            if let Some(sword) = self.finale.guardians[&id].awaiting_sword() {
                ensure!(
                    self.entities.contains_key(&sword),
                    "bound Guardian sword disappeared without its actual defeat"
                );
            }
            if facts.summon_sword {
                self.spawn_children(id, 0xd7c, 1, 5.)?;
                let sword = self
                    .entities
                    .values()
                    .find(|e| e.parent == Some(id) && e.kind == 0xd7c)
                    .context("Guardian sword spawn did not produce its bound identity")?
                    .id;
                self.finale
                    .guardians
                    .get_mut(&id)
                    .unwrap()
                    .bind_sword(sword)?;
                if facts.heal {
                    let e = self.entities.get_mut(&id).unwrap();
                    if e.conditions(self.now) & condition::bit(condition::SICK) == 0 {
                        e.hp = (e.hp + 7200).min(self.content.objects[&kind].hp);
                    }
                }
                tracing::info!(
                    guardian = id,
                    sword,
                    "surviving Guardian recovered and bound its real sword"
                );
            }
            return Ok(());
        }
        if kind == 0xd7e {
            return self.armor_mechanics(id);
        }
        if kind == 0x944 && self.entities[&id].orbit.is_none() {
            self.entities.get_mut(&id).unwrap().orbit = Some(realm_events::Orbit {
                speed: crate::encounter::nr_speed(0.1 + self.rng.gen_range(-0.01..0.01)),
                radius: self.rng.gen_range(0.5..1.5),
                direction: -1.,
            });
        }
        if finale::HEROES.contains(&kind) {
            let initial = self.entities[&id].stage.is_none();
            if initial {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(0);
                if kind == 0x668 {
                    e.reaction_until = self.now + 5000;
                }
            }
            if initial || self.now >= self.entities[&id].next_aux {
                self.entities.get_mut(&id).unwrap().next_aux = self.now + 2000;
                self.hero_reinforcements(id, initial)?;
            }
            return Ok(());
        }
        if kind == 0x174a && elapsed >= 10000 && self.entities[&id].size != 200 {
            self.entities.get_mut(&id).unwrap().size = 200;
            tracing::debug!(entity = id, "inner Guardian Element grew");
        }
        if matches!(kind, ORYX | ORYX2) {
            let mut brain = self.finale.bosses.entry(id).or_default().clone();
            let facts = brain.update(kind, hp, self.content.objects[&kind].hp, elapsed)?;
            let e = self.entities.get_mut(&id).unwrap();
            e.condition &=
                !(condition::bit(condition::INVULNERABLE) | condition::bit(condition::PARALYZED));
            if facts.invulnerable {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
            if facts.phase == OryxPhase::PrepareRage {
                e.condition |= condition::bit(condition::PARALYZED);
            }
            for wave in facts.waves {
                self.finale_wave(id, wave)?;
            }
            if facts.changed {
                self.entities.get_mut(&id).unwrap().next_shot = self.now;
                tracing::info!(entity=id,kind,phase=?facts.phase,"Oryx phase committed");
            }
            self.finale.bosses.insert(id, brain);
        }
        if matches!(kind, 0x1748 | 0x174a | 0x174b | 0xde4) {
            self.entities.get_mut(&id).unwrap().condition |=
                condition::bit(condition::INVULNERABLE);
        }
        if self.now < self.entities[&id].next_aux {
            return Ok(());
        }
        self.entities.get_mut(&id).unwrap().next_aux = self.now
            + if matches!(kind, 0xde2 | 0xdf1) {
                1000
            } else {
                5000
            };
        for wave in finale::summons(kind) {
            self.finale_wave(id, *wave)?;
        }
        match kind {
            ORYX if target.is_some()
                && matches!(
                    self.finale.bosses[&id].phase(),
                    Some(OryxPhase::Artifacts | OryxPhase::Rage)
                ) =>
            {
                self.throw_grenade(id, target.unwrap(), 4., 120)?
            }
            ORYX2 if self.finale.bosses[&id].phase() == Some(OryxPhase::Rage) => self.finale_wave(
                id,
                finale::Wave {
                    kind: 0xde2,
                    count: 1,
                    radius: 7.,
                    lifetime: 10_000,
                },
            )?,
            0xde2 if target.is_some_and(|p| pos.distance(p) < 2.) => {
                self.area_damage(
                    kind,
                    pos,
                    3.,
                    self.content.objects[&kind].projectiles[&0].min,
                )?;
                self.entities.get_mut(&id).unwrap().expires.set(
                    Some(self.now + 50),
                    id,
                    &mut self.timed_ids,
                );
            }
            0xdf1 if target.is_some_and(|p| pos.distance(p) < 2.) => {
                self.area_damage(kind, pos, 2., 40)?
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn finale_shots(
        &self,
        id: i32,
        heading: f32,
        elapsed: u64,
    ) -> Result<Vec<(u8, f32)>> {
        let e = &self.entities[&id];
        if GUARDIANS.contains(&e.kind) && e.stage == Some(0) {
            return Ok(vec![]);
        }
        if let Some(schedule) = self.npc_attack_schedule(id, elapsed)? {
            return Ok(schedule.snapshot(heading));
        }
        Ok(finale::attacks(
            e.kind,
            e.hp,
            self.content.objects[&e.kind].hp,
            heading,
            elapsed,
        ))
    }
    pub(super) fn finale_movement(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<(crate::encounter::Motion, f32)> {
        use crate::encounter::{Anchor, Motion, nr_speed};
        let e = &self.entities[&id];
        match e.kind {
            0x668 => {
                let old = e
                    .stage
                    .context("activated Red Demon movement phase missing")?;
                let stage = match old {
                    0 if self.now >= e.reaction_until => 1,
                    1 if self.now >= e.reaction_until
                        || e.conditions(self.now) & condition::bit(condition::PARALYZED) == 0
                            && target.is_none_or(|p| {
                                e.pos.distance(p) <= 2. || e.pos.distance(p) >= 10.
                            }) =>
                    {
                        2
                    }
                    2 if e.conditions(self.now) & condition::bit(condition::PARALYZED) == 0
                        && (e.pos.x - e.home.x).abs() <= 1.
                        && (e.pos.y - e.home.y).abs() <= 1. =>
                    {
                        0
                    }
                    _ => old,
                };
                if stage != old {
                    let e = self.entities.get_mut(&id).unwrap();
                    e.stage = Some(stage);
                    e.reaction_until = if stage < 2 { self.now + 5000 } else { 0 };
                    tracing::debug!(entity = id, stage, "Red Demon movement phase entered");
                }
                return Ok(match stage {
                    0 => (Motion::WanderHome { radius: 1. }, 3.),
                    1 if target.is_some() => (
                        Motion::Chase {
                            sight: 10.,
                            stop: 2.,
                        },
                        3.,
                    ),
                    1 => (Motion::WanderHome { radius: 1. }, 3.),
                    2 => (
                        Motion::Protect {
                            anchor: Anchor::Home,
                            distance: 1.,
                        },
                        3.,
                    ),
                    _ => bail!("invalid Red Demon movement phase"),
                });
            }
            0x67e..=0x682 => {
                let Some(target) = target.filter(|p| e.pos.distance(*p) < 10.) else {
                    return Ok((Motion::Wander, 10.));
                };
                if self.now < e.reaction_until || e.pos.distance(target) <= 2. {
                    if self.now >= e.reaction_until {
                        self.entities.get_mut(&id).unwrap().reaction_until = self.now + 1000;
                    }
                    return Ok((Motion::Retreat { distance: 8. }, 8.));
                }
                return Ok((
                    Motion::Chase {
                        sight: 10.,
                        stop: 2.,
                    },
                    10.,
                ));
            }
            0xd7e if e.stage == Some(2) => return Ok((Motion::Stationary, 0.)),
            0xd7e => {
                return Ok((
                    Motion::Follow {
                        sight: 10.,
                        stop: 2.,
                        idle: nr_speed(0.2),
                    },
                    nr_speed(0.4),
                ));
            }
            0xd82 => {
                let charge = elapsed % 9000 >= 5000;
                return Ok((
                    Motion::Follow {
                        sight: 10.,
                        stop: 1.,
                        idle: nr_speed(if charge { 0.3 } else { 0.2 }),
                    },
                    nr_speed(if charge { 1.2 } else { 0.4 }),
                ));
            }
            0xd83 => {
                return Ok((
                    Motion::Follow {
                        sight: 10.,
                        stop: 3.,
                        idle: nr_speed(0.2),
                    },
                    nr_speed(if elapsed % 7500 >= 5000 { 0.7 } else { 0.6 }),
                ));
            }
            0x174a | 0x174b => {
                let orbit = e
                    .orbit
                    .context("Guardian Element orbit was not initialized at birth")?;
                return Ok((
                    Motion::Orbit {
                        anchor: Anchor::Parent,
                        radius: orbit.radius,
                    },
                    orbit.speed,
                ));
            }
            0x944 => {
                if let Some((_, anchor)) = self
                    .escort_anchor(id, ORYX2)?
                    .filter(|(boss, _)| e.pos.distance(self.entities[boss].pos) < 10.)
                {
                    let orbit = e
                        .orbit
                        .context("Henchman orbit was not initialized at activation")?;
                    return Ok((
                        Motion::Orbit {
                            anchor,
                            radius: orbit.radius,
                        },
                        orbit.speed,
                    ));
                }
                return Ok((
                    Motion::Follow {
                        sight: 8.,
                        stop: 3.,
                        idle: nr_speed(0.2),
                    },
                    nr_speed(0.2),
                ));
            }
            ORYX if self
                .finale
                .bosses
                .get(&id)
                .context("moving Oryx brain missing")?
                .phase()
                == Some(OryxPhase::Rage) =>
            {
                return Ok(if target.is_some() {
                    (
                        Motion::Chase {
                            sight: 10.,
                            stop: 1.,
                        },
                        nr_speed(0.85),
                    )
                } else {
                    (Motion::Stationary, 0.)
                });
            }
            ORYX2
                if self
                    .finale
                    .bosses
                    .get(&id)
                    .context("moving Oryx2 brain missing")?
                    .phase()
                    == Some(OryxPhase::Rage) =>
            {
                return Ok((
                    Motion::Chase {
                        sight: 25.,
                        stop: 3.,
                    },
                    8.,
                ));
            }
            _ => {}
        }
        Ok(crate::encounter::movement(e.kind, e.hp, elapsed))
    }
    pub(super) fn finale_death(&mut self, defeated: &Entity) -> Result<()> {
        ensure!(
            defeated.hp <= 0,
            "endgame defeat callback requires actual zero health"
        );
        if defeated.kind == 0xd7c {
            let parent = defeated
                .parent
                .context("defeated Guardian sword has no actual parent")?;
            self.finale
                .guardians
                .get_mut(&parent)
                .context("Guardian sword parent brain missing")?
                .sword_defeated(defeated.id, self.now)?;
            tracing::info!(
                sword = defeated.id,
                guardian = parent,
                "actual bound Guardian sword defeat credited"
            );
        }
        if GUARDIANS.contains(&defeated.kind) {
            let survivor = self
                .entities
                .values()
                .find(|e| GUARDIANS.contains(&e.kind) && e.hp > 0)
                .map(|e| e.id);
            if let Some(id) = survivor {
                self.finale
                    .guardians
                    .entry(id)
                    .or_default()
                    .update(false, self.now)?;
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(1);
                e.condition |= condition::bit(condition::INVULNERABLE);
            } else {
                ensure!(!self.cleared, "Guardian pair completion replayed");
                self.cleared = true;
                tracing::info!("actual Guardian pair defeated; Chamber binding requested");
            }
        }
        if matches!(defeated.kind, ORYX | ORYX2) {
            ensure!(!self.cleared, "Oryx completion replayed");
            self.cleared = true;
            if defeated.kind == ORYX {
                let locked = self.spawn_exit(finale::LOCKED_CELLAR, defeated.pos)?;
                self.bag_expiry.insert(locked, self.now + 120_000);
            } else {
                self.spawn_exit(0x712, defeated.pos)?;
            }
            for e in self.entities.values_mut().filter(|e| e.kind == 0x71d) {
                e.published = true;
            }
            tracing::info!(
                boss = defeated.kind,
                "actual Oryx defeat opened earned exits"
            );
        }
        self.finale.bosses.remove(&defeated.id);
        self.finale.guardians.remove(&defeated.id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kill(w: &mut World, id: i32) {
        w.entities.get_mut(&id).unwrap().hp = 0;
        let dead = w.remove_entity(id);
        w.encounter_death(&dead).unwrap();
    }
    fn player(w: &mut World, pos: Pos) -> i32 {
        let ch = super::super::tests::ch(w);
        let id = w.enter(ch, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = pos;
        id
    }
    #[test]
    fn replenishing_boss_waves_never_refresh_existing_child_lifetimes() {
        let mut d = super::super::tests::directory();
        let mut w = d
            .make(finale::CHAMBER, "Oryx's Chamber", false, None)
            .unwrap();
        let boss = w.entities.values().find(|e| e.kind == ORYX).unwrap().id;
        let wave = finale::Wave {
            kind: 0x1749,
            count: 5,
            radius: 3.,
            lifetime: 60_000,
        };
        w.finale_wave(boss, wave).unwrap();
        let initial: BTreeMap<_, _> = w
            .entities
            .values()
            .filter(|e| e.parent == Some(boss))
            .map(|e| (e.id, e.expires.unwrap()))
            .collect();
        assert_eq!(initial.len(), 5);
        w.now = 1500;
        w.finale_wave(boss, wave).unwrap();
        for (id, deadline) in &initial {
            assert_eq!(w.entities[id].expires, Some(*deadline));
        }
        let defeated = *initial.keys().next().unwrap();
        kill(&mut w, defeated);
        w.finale_wave(boss, wave).unwrap();
        let replacement = w
            .entities
            .values()
            .find(|e| e.parent == Some(boss) && !initial.contains_key(&e.id))
            .unwrap();
        assert_eq!(replacement.expires, Some(61_500));
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.parent == Some(boss))
                .count(),
            5
        );
    }
    #[test]
    fn oryx_minion_wanders_and_fires_both_xml_volleys_only_at_close_range() {
        let mut d = super::super::tests::directory();
        let mut w = d
            .make(finale::CHAMBER, "Oryx's Chamber", false, None)
            .unwrap();
        for id in w.entities.keys().copied().collect::<Vec<_>>() {
            w.remove_entity(id);
        }
        let id = player(&mut w, Pos { x: 60., y: 50. });
        let minion = w
            .spawn_entity(0x1749, Pos { x: 60., y: 56. }, vec![])
            .unwrap();
        for tick in 1..=10 {
            w.now = tick * 50;
            w.npc_step();
        }
        assert!(w.bullets.is_empty(), "distant minion must not fire");
        assert!(matches!(
            finale::profile(0x1749).unwrap().motion,
            crate::encounter::Motion::Wander
        ));
        let pos = w.entities[&minion].pos;
        w.players.get_mut(&id).unwrap().pos = Pos {
            x: pos.x,
            y: pos.y - 1.5,
        };
        w.now = 2000;
        w.npc_step();
        assert_eq!(w.bullets.len(), 6);
        for index in 0..=1 {
            let spec = &w.content.objects[&0x1749].projectiles[&index];
            assert_eq!(
                w.bullets
                    .iter()
                    .filter(|b| b.spec.min == spec.min && b.spec.max == spec.max)
                    .count(),
                3
            );
        }
    }
    #[test]
    fn actor_guardian_recovery_requires_its_real_sword_and_paired_completion_binds_chamber() {
        let mut d = super::super::tests::directory();
        let mut w = d
            .make(finale::CASTLE, "Oryx's Castle", false, None)
            .unwrap();
        let right = w
            .entities
            .values()
            .find(|e| e.kind == GUARDIANS[0])
            .unwrap()
            .id;
        let left = w
            .entities
            .values()
            .find(|e| e.kind == GUARDIANS[1])
            .unwrap()
            .id;
        let id = player(&mut w, Pos { x: 0., y: 0. });
        w.players.get_mut(&id).unwrap().pos = w.entities[&left].pos;
        w.entities.get_mut(&left).unwrap().hp = 50_000;
        w.npc_step();
        assert_eq!(
            w.entities[&left].conditions(w.now) & condition::bit(condition::INVULNERABLE),
            0
        );
        assert!(w.encounter_death(&w.entities[&right].clone()).is_err());
        kill(&mut w, right);
        assert!(w.entities[&left].conditions(w.now) & condition::bit(condition::INVULNERABLE) != 0);
        w.now = 4000;
        w.npc_step();
        let sword = w.finale.guardians[&left].awaiting_sword().unwrap();
        assert_eq!(w.entities[&sword].parent, Some(left));
        assert_eq!(w.entities[&sword].hp, 20_000);
        assert_eq!(w.entities[&left].hp, 57_200);
        kill(&mut w, sword);
        w.now = 5350;
        w.npc_step();
        assert!(w.entities[&left].conditions(w.now) & condition::bit(condition::INVULNERABLE) != 0);
        w.now = 5400;
        w.npc_step();
        assert_eq!(
            w.entities[&left].conditions(w.now) & condition::bit(condition::INVULNERABLE),
            0
        );
        kill(&mut w, left);
        assert!(w.cleared);
        let marker = w.entities.values().find(|e| e.kind == 0xd7a).unwrap();
        assert!(
            w.walkable(marker.pos),
            "authored marker {:?} on ground {:#x} is blocked",
            marker.pos,
            w.map.cell(marker.pos.x, marker.pos.y).unwrap().ground
        );
        d.worlds.insert(50, w);
        d.endgame_step();
        let portal = d.worlds[&50]
            .entities
            .values()
            .find(|e| e.kind == 0xd7b)
            .unwrap()
            .id;
        assert!(d.worlds[&50].entities[&portal].published);
        let chamber = d.portals[&portal].destination.unwrap();
        for _ in 0..3 {
            d.step();
        }
        assert!(d.worlds[&50].entities.contains_key(&portal));
        assert!(!d.worlds[&50].bag_expiry.contains_key(&portal));
        let deadline = d.portals[&portal].expires;
        d.worlds.get_mut(&50).unwrap().now += 120_000;
        d.tick = deadline - 1;
        d.step();
        assert!(!d.worlds[&50].entities.contains_key(&portal));
        assert!(!d.portals.contains_key(&portal));
        d.step();
        assert_eq!(d.worlds[&chamber].name, "Oryx's Chamber");
        assert_eq!(
            d.worlds[&chamber]
                .entities
                .values()
                .filter(|e| e.kind == ORYX)
                .count(),
            1
        );
    }
    #[test]
    fn actor_oryx_phases_spawn_bounded_parented_fields_and_cellar_rage_waits_ten_seconds() {
        let mut d = super::super::tests::directory();
        let mut w = d
            .make(finale::CHAMBER, "Oryx's Chamber", false, None)
            .unwrap();
        let boss = w.entities.values().find(|e| e.kind == ORYX).unwrap().id;
        let pos = w.entities[&boss].pos;
        player(&mut w, pos);
        w.npc_step();
        assert_eq!(w.entities[&boss].hp, 85_000);
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.parent == Some(boss) && e.kind == 0x1749)
                .count(),
            5
        );
        w.now = 20_000;
        w.npc_step();
        assert!(w.entities[&boss].conditions(w.now) & condition::bit(condition::INVULNERABLE) != 0);
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.parent == Some(boss) && e.kind == 0x1748 && e.expires == Some(40_000))
                .count(),
            15
        );
        w.now = 45_000;
        w.mechanic_expiry();
        w.npc_step();
        assert_eq!(
            w.entities[&boss].conditions(w.now) & condition::bit(condition::INVULNERABLE),
            0
        );
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.parent == Some(boss) && matches!(e.kind, 0x174a | 0x174b))
                .count(),
            8
        );
        assert!(
            w.entities
                .values()
                .filter(|e| e.kind == 0x71d)
                .all(|e| !e.published)
        );
        kill(&mut w, boss);
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.kind == finale::LOCKED_CELLAR)
                .count(),
            1
        );
        assert!(
            w.entities
                .values()
                .filter(|e| e.kind == 0x71d)
                .all(|e| e.published)
        );
        let mut w = d.make(finale::CELLAR, "Wine Cellar", false, None).unwrap();
        let boss = w.entities.values().find(|e| e.kind == ORYX2).unwrap().id;
        let pos = w.entities[&boss].pos;
        player(&mut w, pos);
        assert_eq!(w.entities[&boss].hp, 75_000);
        w.entities.get_mut(&boss).unwrap().hp = 15_000;
        w.npc_step();
        w.now = 4000;
        w.npc_step();
        assert_eq!(
            w.finale_shots(boss, 0., 4000)
                .unwrap()
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>(),
            [13, 14]
        );
        w.now = 9950;
        w.npc_step();
        assert!(w.entities[&boss].conditions(w.now) & condition::bit(condition::INVULNERABLE) != 0);
        w.now = 10_000;
        w.npc_step();
        assert_eq!(
            w.entities[&boss].conditions(w.now) & condition::bit(condition::INVULNERABLE),
            0
        );
        assert_eq!(w.finale.bosses[&boss].phase(), Some(OryxPhase::Rage));
        assert_eq!(w.finale_movement(boss, Some(pos), 10_000).unwrap().1, 8.);
        kill(&mut w, boss);
        let exits = w
            .entities
            .values()
            .filter(|e| e.kind == 0x712)
            .collect::<Vec<_>>();
        assert_eq!(exits.len(), 1);
        assert_eq!(exits[0].name, "Nexus Portal");
        assert!(exits[0].pos.distance(pos) > 1.9);
    }
    #[test]
    fn oryx_dance_preserves_all_ring_shots_without_overflowing_native_output() {
        let mut d = super::super::tests::directory();
        let mut w = d
            .make(finale::CHAMBER, "Oryx's Chamber", false, None)
            .unwrap();
        let boss = w.entities.values().find(|e| e.kind == ORYX).unwrap().id;
        let pos = w.entities[&boss].pos;
        let id = player(&mut w, pos);
        w.npc_step();
        w.now = 20_000;
        w.npc_step();
        let fields: BTreeSet<_> = w
            .entities
            .values()
            .filter(|e| e.kind == 0x1748)
            .map(|e| e.id)
            .collect();
        assert_eq!(fields.len(), 15);
        let (packets, mut rx) = mpsc::channel(64);
        let (lost, notice) = watch::channel(false);
        let p = w.players.get_mut(&id).unwrap();
        p.known = w.entities.keys().copied().collect();
        p.tx = Some(Output { packets, lost });
        w.apply_condition(id, condition::INVULNERABLE, 1000)
            .unwrap();
        w.now = 20_150;
        w.step();
        assert!(
            !*notice.borrow(),
            "one legitimate Dance tick exhausted native output"
        );
        let mut delivered = 0;
        while let Ok(packet) = rx.try_recv() {
            if packet.id != 19 {
                continue;
            }
            let mut r = wire::Reader::new(&packet.body);
            r.u8().unwrap();
            let owner = r.i32().unwrap();
            r.u8().unwrap();
            r.f32().unwrap();
            r.f32().unwrap();
            r.f32().unwrap();
            r.i16().unwrap();
            let count = if packet.body.len() > 20 {
                let count = r.u8().unwrap();
                r.f32().unwrap();
                count
            } else {
                1
            };
            r.done().unwrap();
            if fields.contains(&owner) {
                delivered += usize::from(count);
            }
        }
        assert_eq!(delivered, 15 * 12);
        assert_eq!(
            w.bullets
                .iter()
                .filter(|b| fields.contains(&b.owner))
                .count(),
            15 * 12
        );
    }
    #[test]
    fn authored_castle_summoners_can_place_each_complete_bounded_family() {
        let mut d = super::super::tests::directory();
        let mut w = d
            .make(finale::CASTLE, "Oryx's Castle", false, None)
            .unwrap();
        let parents: Vec<_> = w
            .entities
            .values()
            .filter(|e| finale::ROSTER.contains(&e.kind) && !finale::summons(e.kind).is_empty())
            .map(|e| (e.id, e.kind))
            .collect();
        for (id, kind) in parents {
            for wave in finale::summons(kind) {
                w.finale_wave(id, *wave).unwrap_or_else(|error| {
                    panic!("authored {kind:#x} at {:?}: {error:#}", w.entities[&id].pos)
                });
                assert_eq!(
                    w.entities
                        .values()
                        .filter(|e| e.parent == Some(id) && e.kind == wave.kind)
                        .count(),
                    wave.count
                );
            }
            let children: Vec<_> = w
                .entities
                .values()
                .filter(|e| e.parent == Some(id))
                .map(|e| e.id)
                .collect();
            for child in children {
                w.remove_entity(child);
            }
        }
    }
}
