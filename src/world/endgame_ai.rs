use super::*;
use crate::{condition, endgame as finale};
use finale::{
    BossBrain, BruteState, GUARDIAN_RUN_MS, GUARDIAN_RUN_SPEED, GUARDIANS, GuardianBlock,
    GuardianBrain, GuardianClock, GuardianDuo, GuardianMode, ORYX, ORYX2, OryxPhase,
};

/// Where a guardian is going within its current block.
struct GuardianMove {
    serial: u64,
    block: Option<GuardianBlock>,
    anchor: Pos,
    goal: Pos,
    next_goal: u64,
    orbit: Option<(u64, f32)>,
}

#[derive(Default)]
pub(super) struct Actors {
    pub(super) bosses: BTreeMap<i32, BossBrain>,
    guardians: BTreeMap<i32, GuardianBrain>,
    guardians_wake: Option<u64>,
    duo: Option<GuardianDuo>,
    core: Option<Pos>,
    brutes: BTreeMap<i32, BruteState>,
    moves: BTreeMap<i32, GuardianMove>,
}
impl Actors {
    pub(super) fn guardian_clock(&self, id: i32) -> Option<GuardianClock> {
        self.duo.as_ref().and_then(|duo| duo.current(id))
    }
    pub(super) fn brute(&self, id: i32) -> Option<BruteState> {
        self.brutes.get(&id).copied()
    }
}

impl World {
    pub(super) fn summoned_children(
        &self,
        parent: i32,
        kind: u16,
    ) -> impl Iterator<Item = &Entity> {
        // Parent assignments enter timed_ids; scenery cannot contribute to a summon wave.
        self.timed_ids
            .iter()
            .map(|id| &self.entities[id])
            .filter(move |e| e.parent == Some(parent) && e.kind == kind)
    }
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
            let current = self.summoned_children(id, wave.kind).count();
            let count = (current + self.rng.gen_range(1..=3)).min(wave.count);
            self.finale_wave(id, finale::Wave { count, ..*wave })?;
        }
        Ok(())
    }
    fn armor_mechanics(&mut self, id: i32, target: Option<Pos>) -> Result<()> {
        let e = &self.entities[&id];
        let old = e.stage.unwrap_or(0);
        let mut stage = old;
        if old == 0
            && (e.hp < self.content.objects[&e.kind].hp
                || target.is_some_and(|t| e.pos.distance(t) <= finale::ARMOR_WAKE_RANGE))
        {
            stage = 1;
        }
        if old == 1 && e.hp <= finale::ARMOR_REGEN_HP {
            stage = 2;
        }
        if old == 2 && self.now >= e.reaction_until {
            stage = 3;
        }
        let e = self.entities.get_mut(&id).unwrap();
        if stage == 2 && old != 2 {
            e.reaction_until = self.now + finale::ARMOR_REGEN_MS;
            e.next_aux = self.now + finale::ARMOR_HEAL_MS;
        }
        e.stage = Some(stage);
        let regen = condition::bit(condition::INVULNERABLE) | condition::bit(condition::PARALYZED);
        e.condition &= !regen;
        if stage == 2 {
            e.condition |= regen;
        }
        if old != stage {
            // Red eyes (AltTexture 1) while awake; the plain sprite while dormant and regenerating (bBQRbAoRP_g t=150-172).
            e.extra_stats.retain(|(stat, _)| *stat != 61);
            e.extra_stats.push((61, i32::from(matches!(stage, 1 | 3))));
            tracing::debug!(entity = id, stage, "Suit of Armor phase entered");
            if stage == 2 {
                // Every regen opens with the taunt "..." (speech bubble and chat line; bBQRbAoRP_g, fzgFSFVN8nY t=255-285).
                let packet = chat::text_packet("#Haunted Armor", id, 5, "...");
                for p in self.players.values().filter(|p| p.known.contains(&id)) {
                    send(p, packet.id, packet.body.clone());
                }
            }
        }
        let e = &self.entities[&id];
        if stage == 2 && self.now >= e.next_aux {
            self.entities.get_mut(&id).unwrap().next_aux = self.now + finale::ARMOR_HEAL_MS;
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
        let children: Vec<_> = self
            .summoned_children(parent, wave.kind)
            .filter(|e| e.expires.is_none())
            .map(|e| e.id)
            .collect();
        for id in children {
            let e = self.entities.get_mut(&id).expect("summoned child missing");
            e.expires
                .set(self.now.checked_add(wave.lifetime), id, &mut self.timed_ids);
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
            // A hit on either statue wakes both together 2-7 s later; proximity does not (HLraOCBJO94 t=630.5-637.5, bBQRbAoRP_g t=339-341, 5ZRsiTUcKts t=103-106).
            if self.finale.guardians_wake.is_none() && !self.entities[&id].damage.is_empty() {
                let delay = self.rng.gen_range(2000..=7000);
                self.finale.guardians_wake = Some(self.now + delay);
                tracing::info!(
                    guardian = id,
                    delay,
                    "Guardian statue hit; pair wake scheduled"
                );
            }
            let mut mode = None;
            if self.finale.guardians_wake.is_some_and(|at| self.now >= at) {
                if self.finale.duo.is_none() {
                    self.finale.core = Some(
                        self.entities
                            .values()
                            .find(|e| e.kind == 0xd7a)
                            .context("Guardian blocks need the ring core")?
                            .pos,
                    );
                    self.finale.duo = Some(GuardianDuo::new(self.now, &mut self.rng));
                    tracing::info!("Guardian pair woke; shared block clock started");
                }
                let clock = self
                    .finale
                    .duo
                    .as_mut()
                    .unwrap()
                    .clock(id, self.now, &mut self.rng);
                if clock.start == self.now {
                    tracing::info!(guardian = id, block = ?clock.block, "Guardian block started");
                }
                let sword = self.summoned_children(id, 0xd7c).next().is_some();
                let core = self
                    .finale
                    .core
                    .context("Guardian blocks need the ring core")?;
                let home = pos.distance(Pos {
                    x: core.x,
                    y: core.y - finale::GUARDIAN_DAIS_Y,
                }) <= 1.;
                let current = self.finale.guardians.entry(id).or_default().update(
                    self.now,
                    sword,
                    home,
                    &mut self.rng,
                )?;
                mode = Some(current);
                // Red bombs at a nearby player: every 0.5-0.8 s in the side block (5ZRsiTUcKts t=116.1-117.4, 139.5; bBQRbAoRP_g t=393.0-394.5), landing
                // at or near the player; once seen from the core (bBQRbAoRP_g t=381.5-383.5). ≈50 damage from a "-7" on a DEF 44 Rogue (LOW-MED).
                // The 6.5-tile trigger and the slower centre cadence are estimates.
                let bombs = match clock.block {
                    GuardianBlock::Side => Some(500..=800),
                    GuardianBlock::Centre => Some(1500..=2500),
                    _ => None,
                };
                if matches!(current, GuardianMode::Active | GuardianMode::Returning)
                    && let Some(cadence) = bombs
                    && self.now >= clock.start + GUARDIAN_RUN_MS
                    && self.now >= self.entities[&id].next_aux
                    && let Some(player) = target.filter(|t| pos.distance(*t) <= 6.5)
                {
                    self.throw_grenade(id, player, 1.3, 50)?;
                    self.entities.get_mut(&id).unwrap().next_aux =
                        self.now + self.rng.gen_range(cadence);
                }
            }
            let active = matches!(mode, Some(GuardianMode::Active | GuardianMode::Returning));
            let e = self.entities.get_mut(&id).unwrap();
            e.condition &=
                !(condition::bit(condition::PARALYZED) | condition::bit(condition::INVULNERABLE));
            if !active {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
            if matches!(mode, None | Some(GuardianMode::Stone { .. })) {
                e.condition |= condition::bit(condition::PARALYZED);
            }
            if e.stage != Some(u64::from(active)) {
                e.stage = Some(u64::from(active));
                tracing::info!(guardian = id, ?mode, "Guardian phase entered");
            }
            // AltTexture 1 is the grey statue (asleep, stone, the grey throw); awake it is 0, or 3/4 while spinning.
            let texture = match (active, self.finale.guardian_clock(id)) {
                (false, _) => 1,
                (true, Some(clock)) => {
                    finale::guardian_texture(clock.block, self.now - clock.start)
                }
                (true, None) => 0,
            };
            let e = self.entities.get_mut(&id).unwrap();
            if e.extra_stats != [(61, texture)] {
                e.extra_stats = vec![(61, texture)];
            }
            if mode == Some(GuardianMode::Stone { heal: true }) {
                self.heal_enemy(id, id, finale::guardian_heal(kind))?;
            }
            if mode == Some(GuardianMode::Waiting { toss: true }) {
                let core = self
                    .finale
                    .core
                    .context("Guardian sword throw needs the ring core")?;
                // A golden lob from the dais lands the sword in the ring core (always the 3x3 core; flight 1.2-1.5 s).
                self.manor_toss(id, 0xd7c, core, finale::SWORD_MS)?;
            }
            return Ok(());
        }
        if kind == 0xd7c {
            // The sword dies at about half health: an 8-shot burst, then a red particle stream flies back to the survivor (5ZRsiTUcKts t=173.8, 197.9, 209.2; exMHglbJ98E t=90.3-105.2).
            // The Japanese sword page (≤2012-06-26) calls the stream a Grenade Toss (50 damage) toward the guardian.
            if hp * 2 < self.content.objects[&kind].hp && self.entities[&id].stage != Some(1) {
                let owner = *self.entities[&id].parent;
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(1);
                e.expires.set(Some(self.now + 100), id, &mut self.timed_ids);
                if let Some(guardian) = owner.and_then(|owner| self.entities.get(&owner)) {
                    let at = guardian.pos;
                    self.throw_grenade(id, at, 1.3, 50)?;
                }
                tracing::info!(sword = id, hp, "Guardian sword broke at half health");
            }
            return Ok(());
        }
        if kind == 0xd80 {
            // Gold (AltTexture 2) until its Commander is hit, then red with Sting Status rings (bBQRbAoRP_g 2012-04-01 t=16.73-16.77, ≈25.1, 299.3).
            let red = self.entities[&id].stage == Some(1)
                || (*self.entities[&id].parent)
                    .and_then(|commander| self.entities.get(&commander))
                    .is_some_and(|commander| !commander.damage.is_empty());
            let e = self.entities.get_mut(&id).unwrap();
            if e.stage != Some(u64::from(red)) {
                e.stage = Some(u64::from(red));
                e.extra_stats.retain(|(stat, _)| *stat != 61);
                e.extra_stats.push((61, if red { 0 } else { 2 }));
            }
        }
        if kind == 0xd82 {
            let now = self.now;
            let brute = self
                .finale
                .brutes
                .entry(id)
                .or_insert_with(|| BruteState::new(now, &mut self.rng));
            if brute.update(now, &mut self.rng) {
                tracing::debug!(
                    entity = id,
                    charge = brute.charge,
                    "Oryx Brute state entered"
                );
            }
        }
        if kind == 0xd7e {
            return self.armor_mechanics(id, target);
        }
        if kind == 0x944 && self.entities[&id].orbit.is_none() {
            self.entities.get_mut(&id).unwrap().orbit = Some(realm_events::Orbit {
                speed: crate::encounter::nr_speed(0.1 + self.rng.gen_range(-0.01..0.01)),
                radius: self.rng.gen_range(0.5..1.5),
                direction: -1.,
            });
        }
        if matches!(kind, 0x67e..=0x682) {
            self.entities.get_mut(&id).unwrap().stage.get_or_insert(0);
        }
        if finale::HEROES.contains(&kind) {
            let initial = self.entities[&id].stage.is_none();
            if initial {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(0);
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
        let gap = match kind {
            0xde2 | 0xdf1 => 1000,
            0xd7f => 3000,
            0xd82 => 7000,
            // Per tower: footage HLraOCBJO94 t=613-617 shows throws every 0.4-1.2 s from two towers in view.
            // Each floor threw every ≈1.2-2.2 s (bBQRbAoRP_g 2012-04-01 t=244-289, HLraOCBJO94 t=613-617).
            0xd87 => self.rng.gen_range(1200..=2200),
            _ => 5000,
        };
        self.entities.get_mut(&id).unwrap().next_aux = self.now + gap;
        for wave in finale::summons(kind) {
            let count = match kind {
                0xd7f => self.rng.gen_range(8..=15),
                0xd82 => self.rng.gen_range(3..=5),
                _ => wave.count,
            };
            self.finale_wave(id, finale::Wave { count, ..*wave })?;
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
            // Quiet Bombs land and rest ≈1 s (HLraOCBJO94 t=616.2-617.0); the 10-tile reach is from a later comparator (2012 wiki: "very long reach"): https://github.com/ossimc82/fabiano-swagger-of-doom/blob/6fd20aad4a7905b13f25389c68368a942a2b68cb/wServer/logic/db/BehaviorDb.OryxCastle.cs#L303-L320
            0xd87 if target.is_some_and(|p| pos.distance(p) <= 10.) => {
                self.manor_toss(id, 0xd86, target.unwrap(), 850)?
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
            0xdf1 => {
                // One Goo per second, at most 20, per a disabled later comparator line: https://github.com/Omniraptyr/Valor-Server/blob/40fab9e80d8a44e922036d178b9d1f03aca66a44/VotR-Server/wServer/logic/db/BehaviorDb.Oryx.cs#L499
                // Goo lasting as long as its 6000 ms projectile is a reconstruction.
                let live = self
                    .actors()
                    .filter(|e| e.parent == Some(id) && e.kind == 0xdf0)
                    .count();
                self.finale_wave(
                    id,
                    finale::Wave {
                        kind: 0xdf0,
                        count: (live + 1).min(20),
                        radius: 0.1,
                        lifetime: 6000,
                    },
                )?
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
    pub(super) fn hero_movement(
        &mut self,
        id: i32,
        target: Option<Pos>,
        delta: u64,
    ) -> Result<(crate::encounter::Motion, f32, bool)> {
        use crate::encounter::{Anchor, Motion};
        let e = &self.entities[&id];
        let paralyzed = e.conditions(self.now) & condition::bit(condition::PARALYZED) != 0;
        let stage = e
            .stage
            .context("activated Realm hero movement phase missing")?;
        let distance = target.map(|p| e.pos.distance(p));
        let (motion, speed, completed, next) = match e.kind {
            0x668 => match stage {
                0 => (Motion::WanderHome { radius: 1. }, 3., false, 1),
                1 => (
                    if paralyzed || distance.is_some_and(|d| d > 2. && d < 10.) {
                        Motion::Chase {
                            sight: 10.,
                            stop: 2.,
                        }
                    } else {
                        Motion::Stationary
                    },
                    3.,
                    !paralyzed && distance.is_none_or(|d| d <= 2. || d >= 10.),
                    2,
                ),
                2 => (
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 1.,
                    },
                    3.,
                    !paralyzed
                        && (e.pos.x - e.home.x).abs() <= 1.
                        && (e.pos.y - e.home.y).abs() <= 1.,
                    0,
                ),
                _ => bail!("invalid Red Demon movement phase {stage}"),
            },
            0x67e..=0x682 => {
                if distance.is_none_or(|d| d >= 10.) {
                    let e = self.entities.get_mut(&id).unwrap();
                    if e.reaction_until > self.now.saturating_sub(delta) {
                        e.reaction_until += delta;
                    }
                    return Ok((Motion::Wander, 10., false));
                }
                match stage {
                    0 => (
                        Motion::Chase {
                            sight: 10.,
                            stop: 2.,
                        },
                        10.,
                        !paralyzed && distance.is_some_and(|d| d <= 2.),
                        1,
                    ),
                    1 => (Motion::Retreat { distance: 8. }, 8., false, 0),
                    _ => bail!("invalid Cyclops movement phase {stage}"),
                }
            }
            _ => bail!("unsupported queued Realm hero {:#x}", e.kind),
        };
        let timed: Option<u64> = if e.kind == 0x668 && stage < 2 {
            Some(5000)
        } else if e.kind != 0x668 && stage == 1 {
            Some(1000)
        } else {
            None
        };
        let e = self.entities.get_mut(&id).unwrap();
        if let Some(duration) = timed
            && e.reaction_until == 0
        {
            e.reaction_until = self.now + duration.saturating_sub(delta);
        }
        let completed = completed || timed.is_some() && self.now >= e.reaction_until;
        e.stage = Some(if completed { next } else { stage });
        if completed {
            e.reaction_until = 0;
            tracing::debug!(
                entity = id,
                stage = next,
                "Realm hero movement child completed"
            );
        }
        // ReturnSpawn's axis-aligned completion must not move again before the next child.
        Ok((
            if completed && timed.is_none() {
                Motion::Stationary
            } else {
                motion
            },
            speed,
            completed,
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
            // Statues sleep on their own dais spot; the stone survivor stays where it stood and the grey thrower stays at the dais centre.
            0xd78 | 0xd79 if e.stage != Some(1) => return Ok((Motion::Stationary, 0.)),
            0xd78 | 0xd79 => {
                let Some(core) = self.finale.core else {
                    return Ok((Motion::Stationary, 0.));
                };
                // The survivor runs to the dais centre to throw (bBQRbAoRP_g t=448.6-450.6).
                if self.finale.guardians.get(&id).and_then(GuardianBrain::mode)
                    == Some(GuardianMode::Returning)
                {
                    return Ok((
                        Motion::Protect {
                            anchor: Anchor::Point {
                                x: core.x,
                                y: core.y - finale::GUARDIAN_DAIS_Y,
                            },
                            distance: 0.3,
                        },
                        GUARDIAN_RUN_SPEED,
                    ));
                }
                let Some(clock) = self.finale.guardian_clock(id) else {
                    return Ok((Motion::Stationary, 0.));
                };
                return Ok(self.guardian_motion(id, clock, core));
            }
            0xd7e if !matches!(e.stage, Some(1 | 3)) => return Ok((Motion::Stationary, 0.)),
            0xd7e => {
                return Ok((
                    Motion::Follow {
                        sight: 10.,
                        stop: 2.,
                        idle: nr_speed(0.2),
                        duration: 0,
                        cooldown: 0,
                    },
                    nr_speed(0.4),
                ));
            }
            0xd82 => {
                // RING: it keeps 5-7 tiles off at 1-3 tiles/s. CHARGE: each 2.5 s dash closes ≈4 tiles at ≈10 tiles/s, then it Rushes while
                // following at ≈2.7 tiles/s, 3.2-4.4 tiles from the player (fzgFSFVN8nY 2012-08-10; bBQRbAoRP_g 2012-04-01).
                let Some(brute) = self.finale.brute(id) else {
                    return Ok((Motion::Stationary, 0.));
                };
                let dash = (self.now - brute.since) % 2500 < 450;
                return Ok(match (brute.charge, dash) {
                    (true, true) => (
                        Motion::Chase {
                            sight: 20.,
                            stop: 3.2,
                        },
                        finale::tiles_per_s(10.),
                    ),
                    (true, false) => (
                        Motion::Chase {
                            sight: 20.,
                            stop: 3.2,
                        },
                        finale::tiles_per_s(2.7),
                    ),
                    (false, _) => (
                        Motion::Chase {
                            sight: 20.,
                            stop: 5.,
                        },
                        finale::tiles_per_s(2.),
                    ),
                });
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
                        duration: 0,
                        cooldown: 0,
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
    /// Guardian movement per block. Engine y grows south, so counter-clockwise with north up lowers the angle.
    fn guardian_motion(
        &mut self,
        id: i32,
        clock: GuardianClock,
        core: Pos,
    ) -> (crate::encounter::Motion, f32) {
        use crate::encounter::{Anchor, Motion};
        use finale::{
            GUARDIAN_CHASE_SPEED, GUARDIAN_ORBIT_RADIUS as RADIUS, GUARDIAN_QUAD_DEG_S,
            GUARDIAN_SPIN_CORE_MS, GUARDIAN_SPIN_DEG_S, GUARDIAN_SPIN_FIRST_MS,
            GUARDIAN_SPIN_ORBIT_MS, GUARDIAN_WALL_X,
        };
        let e = &self.entities[&id];
        let (pos, home) = (e.pos, e.home);
        let side = if e.kind == GUARDIANS[0] { 1. } else { -1. };
        let (now, age) = (self.now, self.now - clock.start);
        let to = |x: f32, y: f32, speed: f32| {
            (
                Motion::Protect {
                    anchor: Anchor::Point { x, y },
                    distance: 0.05,
                },
                speed,
            )
        };
        let ring = |angle: f32| Pos {
            x: core.x + RADIUS * angle.cos(),
            y: core.y + RADIUS * angle.sin(),
        };
        let state = self.finale.moves.entry(id).or_insert(GuardianMove {
            serial: u64::MAX,
            block: None,
            anchor: pos,
            goal: pos,
            next_goal: 0,
            orbit: None,
        });
        if state.serial != clock.serial {
            let previous = state.block;
            state.serial = clock.serial;
            state.block = Some(clock.block);
            state.orbit = None;
            state.next_goal = 0;
            state.anchor = match previous {
                // The first ring walk walks slowly off the dais, away from the players (HLraOCBJO94 t=637.0-640.9).
                None => Pos {
                    x: home.x + 2. * side,
                    y: home.y + 2.,
                },
                // After the side block they come back in slowly (bBQRbAoRP_g t=394.35-395.35).
                Some(GuardianBlock::Side) => Pos {
                    x: pos.x - 2. * side,
                    y: pos.y,
                },
                _ => pos,
            };
        }
        match clock.block {
            // No chase while ring-walking: they stand near where the last block left them and shuffle (1-s chords 0.8-1.6 tiles/s, net 0.1-0.3).
            GuardianBlock::Ring => {
                if now >= state.next_goal {
                    let (dx, dy) = (
                        self.rng.gen_range(-0.8f32..0.8),
                        self.rng.gen_range(-0.8f32..0.8),
                    );
                    state.goal = Pos {
                        x: state.anchor.x + dx,
                        y: state.anchor.y + dy,
                    };
                    state.next_goal = now + self.rng.gen_range(700..1300);
                }
                to(state.goal.x, state.goal.y, finale::tiles_per_s(1.2))
            }
            GuardianBlock::Fan => (
                Motion::Chase {
                    sight: 20.,
                    stop: 2.2,
                },
                GUARDIAN_CHASE_SPEED,
            ),
            // Run to the core, split to the ring's east/west point, orbit counter-clockwise 180° apart, stop dead with the last volley.
            GuardianBlock::Spin => {
                if age < GUARDIAN_SPIN_CORE_MS {
                    to(core.x, core.y, GUARDIAN_RUN_SPEED)
                } else if age < GUARDIAN_SPIN_ORBIT_MS {
                    to(core.x + side * RADIUS, core.y, GUARDIAN_RUN_SPEED)
                } else if age < GUARDIAN_SPIN_FIRST_MS + 26 * 200 {
                    let start = if side > 0. { 0. } else { std::f32::consts::PI };
                    let turned = GUARDIAN_SPIN_DEG_S.to_radians()
                        * (age - GUARDIAN_SPIN_ORBIT_MS) as f32
                        / 1000.;
                    let at = ring(start - turned);
                    to(at.x, at.y, GUARDIAN_RUN_SPEED)
                } else {
                    (Motion::Stationary, 0.)
                }
            }
            // Go straight to the nearest point of the circle (not via the core), then orbit counter-clockwise faster.
            GuardianBlock::Quad => {
                let (start, angle) = match state.orbit {
                    Some(orbit) => orbit,
                    None => {
                        let angle = if pos.distance(core) < 0.1 {
                            if side > 0. { 0. } else { std::f32::consts::PI }
                        } else {
                            (pos.y - core.y).atan2(pos.x - core.x)
                        };
                        let entry = ring(angle);
                        if pos.distance(entry) > 0.4 {
                            return to(entry.x, entry.y, GUARDIAN_RUN_SPEED);
                        }
                        state.orbit = Some((now, angle));
                        (now, angle)
                    }
                };
                let at =
                    ring(angle - GUARDIAN_QUAD_DEG_S.to_radians() * (now - start) as f32 / 1000.);
                to(at.x, at.y, GUARDIAN_RUN_SPEED)
            }
            // Run to the core and stand back to back, the Right ≈0.75 tile east of the Left, through the block and the next ring walk.
            GuardianBlock::Centre => to(core.x + 0.4 * side, core.y, GUARDIAN_RUN_SPEED),
            // Both run along the core's row to their own side, just inside the armour strip.
            GuardianBlock::Side => to(core.x + GUARDIAN_WALL_X * side, core.y, GUARDIAN_RUN_SPEED),
        }
    }
    pub(super) fn finale_death(&mut self, defeated: &Entity) -> Result<()> {
        ensure!(
            defeated.hp <= 0,
            "endgame defeat callback requires actual zero health"
        );
        if GUARDIANS.contains(&defeated.kind) {
            let survivor = self
                .entities
                .values()
                .find(|e| GUARDIANS.contains(&e.kind) && e.hp > 0)
                .map(|e| e.id);
            if let Some(id) = survivor {
                self.finale.guardians.entry(id).or_default().alone(self.now);
                self.entities.get_mut(&id).unwrap().condition |=
                    condition::bit(condition::INVULNERABLE);
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
            for e in self.entities.matching_mut(|e| e.kind == 0x71d) {
                e.published = true;
            }
            tracing::info!(
                boss = defeated.kind,
                "actual Oryx defeat opened earned exits"
            );
        }
        self.finale.bosses.remove(&defeated.id);
        self.finale.guardians.remove(&defeated.id);
        self.finale.brutes.remove(&defeated.id);
        self.finale.moves.remove(&defeated.id);
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn cyclops_ownership_tags_preserve_the_family_controller() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    for tagged in [false, true] {
        let map = Map {
            width: 32,
            height: 32,
            cells: vec![
                crate::content::Cell {
                    ground: 0x46,
                    object: None,
                    config: String::new(),
                    terrain: 1,
                    region: 0,
                };
                1024
            ],
            spawns: vec![(8.5, 8.5)],
        };
        let mut world = World::new(content.clone(), Arc::new(map)).unwrap();
        world.encounter = true;
        let mut character = Character::fresh(content.get(0x307).unwrap()).unwrap();
        character.account = 1;
        character.id = 1;
        let player = world.enter(character, None).unwrap();
        let root = world
            .spawn_entity(0x67d, Pos { x: 10.5, y: 8.5 }, vec![])
            .unwrap();
        if tagged {
            world.entities.get_mut(&root).unwrap().quest.set(
                Some(root),
                root,
                &mut world.quest_ids,
            );
        }
        world.now = 50;
        world.npc_step();
        assert_eq!(*world.entities[&root].quest, tagged.then_some(root));
        assert_eq!(
            world.entities[&root].motion,
            Some(crate::encounter::Motion::WanderHome { radius: 3. })
        );
        for kind in 0x67e..=0x682 {
            assert!(
                (1..=3).contains(&world.summoned_children(root, kind).count()),
                "missing Cyclops species {kind:#x}, tagged={tagged}"
            );
        }
        let target = world.players[&player].pos;
        for (age, expected) in [(0, vec![10, 10, 10]), (5000, vec![0, 1, 2, 3])] {
            let shots = world.legacy_enemy_shots(root, target, age).unwrap();
            assert_eq!(
                shots.iter().map(|s| s.0).collect::<Vec<_>>(),
                expected,
                "tagged={tagged}"
            );
        }
    }
}

#[cfg(test)]
mod castle_tests {
    use super::*;

    fn castle() -> (World, i32) {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let map = Map::load(std::path::Path::new("data/oryx-castle.wmap"), &content).unwrap();
        let mut world = World::new(content.clone(), Arc::new(map)).unwrap();
        world.name = "Oryx's Castle".into();
        world.encounter = true;
        world.prepare_endgame().unwrap();
        let mut character = Character::fresh(content.get(0x307).unwrap()).unwrap();
        character.account = 1;
        character.id = 1;
        let player = world.enter(character, None).unwrap();
        world
            .apply_condition(player, condition::INVINCIBLE, 600_000)
            .unwrap();
        (world, player)
    }
    fn run(world: &mut World, ms: u64) {
        for _ in 0..ms / 50 {
            world.step();
        }
    }
    fn flags(world: &World, id: i32) -> i32 {
        world.entities[&id].conditions(world.now)
    }

    #[test]
    fn castle_map_matches_the_march_2012_wikiwiki_map() {
        // wikiwiki.jp rmd "Oryx's Castle" oryxcastlemap.png (CDN t=20120331234252); its cell = map cell + (3, 0).
        use sha2::{Digest, Sha256};
        let (world, _) = castle();
        let map = &world.map;
        let cells = |kind| -> String {
            (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| map.cells[(y * map.width + x) as usize].object == Some(kind))
                .map(|(x, y)| format!("{},{y};", x + 3))
                .collect()
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(cells(0xd70))),
            "117b62d0734bbe163568d188c1fa5ee10657c7fa47c480ffb747ee16e7516568"
        );
        assert_eq!(
            cells(0xd87),
            "108,22;114,22;111,33;111,45;108,53;114,53;70,91;152,91;26,92;196,92;23,96;29,96;193,96;199,96;77,98;145,98;65,127;157,127;62,131;68,131;154,131;160,131;"
        );
        // Hand-drawn start squares; each island's spawn cell lies on or beside its own square.
        let squares = [
            (25, 149, 27, 150),
            (195, 149, 197, 150),
            (65, 185, 66, 186),
            (157, 184, 158, 185),
        ];
        assert_eq!(map.spawns.len(), 4);
        for (x0, y0, x1, y1) in squares {
            assert_eq!(
                map.spawns
                    .iter()
                    .filter(|(x, y)| {
                        let (x, y) = (*x as i32 + 3, *y as i32);
                        (x0 - 1..=x1 + 1).contains(&x) && (y0 - 1..=y1 + 1).contains(&y)
                    })
                    .count(),
                1
            );
        }
    }

    #[test]
    fn guardians_wake_on_a_hit_and_the_survivor_throws_its_sword() {
        let (mut world, player) = castle();
        let [right, left] =
            GUARDIANS.map(|kind| world.entities.values().find(|e| e.kind == kind).unwrap().id);
        let home = world.entities[&left].home;
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: home.x + 3.,
            y: home.y + 4.,
        };
        let stone = condition::bit(condition::INVULNERABLE) | condition::bit(condition::PARALYZED);
        run(&mut world, 5000);
        assert!(
            [left, right]
                .iter()
                .all(|id| flags(&world, *id) & stone == stone)
        );
        world
            .damage_enemy(player, left, 1000, false, &[], Some(0))
            .unwrap();
        assert_eq!(world.entities[&left].hp, 60_000);
        run(&mut world, 1950);
        assert!(
            [left, right]
                .iter()
                .all(|id| flags(&world, *id) & stone == stone)
        );
        run(&mut world, 5200);
        assert!(
            [left, right]
                .iter()
                .all(|id| flags(&world, *id) & stone == 0 && world.entities[id].stage == Some(1))
        );
        world.entities.get_mut(&right).unwrap().hp = 55_000;
        world
            .damage_enemy(player, left, 100_000, true, &[], Some(0))
            .unwrap();
        assert!(!world.entities.contains_key(&left));
        // Stone from 0.6 s for 5.0 s, healing +2400 every 0.4 s from 0.5 s in.
        run(&mut world, 3900);
        assert_eq!(flags(&world, right) & stone, stone);
        assert_eq!(world.entities[&right].hp, 60_000);
        run(&mut world, 1800);
        assert_eq!(flags(&world, right) & stone, 0);
        // It walks back to its dais, greys and lobs the sword into the ring core.
        let core = world
            .entities
            .values()
            .find(|e| e.kind == 0xd7a)
            .unwrap()
            .pos;
        let mut sword = None;
        for _ in 0..200 {
            run(&mut world, 50);
            if let Some(e) = world.entities.values().find(|e| e.kind == 0xd7c) {
                sword = Some(e.id);
                break;
            }
        }
        let sword = sword.expect("survivor threw its sword after the stone");
        // It throws from the dais centre, 10 tiles north of the core, and stays there while grey.
        let dais = Pos {
            x: core.x,
            y: core.y - finale::GUARDIAN_DAIS_Y,
        };
        assert!(world.entities[&right].pos.distance(dais) <= 1.);
        assert!(
            *world.entities[&sword].parent == Some(right)
                && world.entities[&sword].pos.distance(core) < 0.01
        );
        assert_eq!(
            flags(&world, right) & stone,
            condition::bit(condition::INVULNERABLE)
        );
        // The sword fires set A on landing and set B 0.6 s later: five shots each, 72° apart, 36° between the sets.
        run(&mut world, 700);
        let mut angles: Vec<i32> = world
            .bullets
            .iter()
            .filter(|b| b.owner == sword)
            .map(|b| (b.angle.to_degrees().rem_euclid(360.)).round() as i32 % 360)
            .collect();
        angles.sort();
        assert_eq!(angles, (0..10).map(|k| k * 36).collect::<Vec<_>>());
        // No timeout: shot to half health it bursts 8 shots and is gone; the survivor returns 1.9 s later.
        run(&mut world, 10_000);
        assert!(world.entities.contains_key(&sword));
        world.entities.get_mut(&sword).unwrap().hp = 9_999;
        run(&mut world, 200);
        assert!(!world.entities.contains_key(&sword));
        assert_eq!(
            world
                .bullets
                .iter()
                .filter(|b| b.owner == sword && b.born + 250 > world.now)
                .count(),
            8
        );
        assert_ne!(flags(&world, right) & stone, 0);
        run(&mut world, 2000);
        assert_eq!(flags(&world, right) & stone, 0);
        assert!(!world.cleared);
    }

    #[test]
    fn guardian_blocks_follow_the_2012_patterns() {
        use crate::encounter::attacks::guardian_schedule;
        use finale::{
            GUARDIAN_RUN_MS as RUN, GUARDIAN_SPIN_FIRST_MS as SPIN, GUARDIAN_SPLIT_MS as SPLIT,
            GuardianBlock as B, guardian_texture,
        };
        let angles = |shots: Vec<(u8, f32)>| -> Vec<i32> {
            let mut out: Vec<i32> = shots
                .iter()
                .map(|s| s.1.to_degrees().rem_euclid(360.).round() as i32 % 360)
                .collect();
            out.sort();
            out
        };
        let at = |block, right, age| guardian_schedule(block, 4, right, age).snapshot(1.234);
        // Ring walk: 10 Small White Blasts at fixed angles, one due east, whatever the target heading.
        let ring = at(B::Ring, true, 200);
        assert!(ring.iter().all(|s| s.0 == 2));
        assert_eq!(angles(ring), (0..10).map(|k| k * 36).collect::<Vec<_>>());
        // Spin: both guardians start at 0°/180° and turn 40° clockwise per 0.2 s at the same angles.
        for right in [true, false] {
            assert_eq!(angles(at(B::Spin, right, SPIN)), vec![0, 180]);
            assert_eq!(angles(at(B::Spin, right, SPIN + 200)), vec![40, 220]);
        }
        // The spin animation: AltTexture 3 on the first volley, 4 on the next, and so on; 0 outside the spin.
        assert_eq!(guardian_texture(B::Spin, SPIN - 50), 0);
        assert_eq!(
            [SPIN, SPIN + 200, SPIN + 400, SPIN + 26 * 200]
                .map(|age| guardian_texture(B::Spin, age)),
            [3, 4, 3, 3]
        );
        assert_eq!(guardian_texture(B::Spin, SPIN + 27 * 200), 0);
        assert_eq!(guardian_texture(B::Quad, SPLIT), 0);
        // Four-way pairs at fixed directions; the Right fires 0.2 s first.
        assert_eq!(
            angles(at(B::Quad, true, SPLIT)),
            vec![0, 80, 90, 170, 180, 260, 270, 350]
        );
        assert!(at(B::Quad, false, SPLIT).is_empty());
        assert_eq!(at(B::Quad, false, SPLIT + 200).len(), 8);
        // Centre: Right A at 0, Right B with Left A' at 0.20 s, Left B' at 0.38 s.
        assert_eq!(
            angles(at(B::Centre, true, RUN)),
            vec![0, 26, 52, 78, 282, 308, 334]
        );
        assert_eq!(at(B::Centre, true, RUN + 200).len(), 8);
        assert_eq!(
            angles(at(B::Centre, false, RUN + 200)),
            vec![102, 128, 154, 180, 206, 232, 258]
        );
        assert_eq!(at(B::Centre, false, RUN + 380).len(), 8);
        // Side: two streams ±120° about the inward normal, closing 10° per volley.
        assert_eq!(angles(at(B::Side, false, RUN)), vec![120, 240]);
        assert_eq!(angles(at(B::Side, true, RUN)), vec![60, 300]);
        assert_eq!(angles(at(B::Side, false, RUN + 12 * 200)), vec![0, 0]);
        // Joint blocks are shared; ring walks and chase-fans are each guardian's own.
        let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(9);
        let mut duo = finale::GuardianDuo::new(0, &mut rng);
        let (mut joint, mut fans, mut apart) = (0, 0, 0);
        let mut last = None;
        for now in (0..600_000).step_by(50) {
            let (one, two) = (duo.clock(1, now, &mut rng), duo.clock(2, now, &mut rng));
            if matches!(one.block, B::Ring | B::Fan) {
                assert!(matches!(two.block, B::Ring | B::Fan));
                fans += usize::from(one.block == B::Fan && one.start == now);
                apart += usize::from(one.block != two.block);
            } else {
                assert_eq!(one, two);
                if last != Some(one.serial) {
                    joint += 1;
                    last = Some(one.serial);
                }
            }
        }
        assert!(
            (40..=90).contains(&joint),
            "{joint} joint blocks in 10 minutes"
        );
        assert!(fans > 10 && apart > 0);
    }

    #[test]
    fn spinning_guardians_orbit_the_core_counter_clockwise_and_flip_textures() {
        let (mut world, player) = castle();
        let [right, left] =
            GUARDIANS.map(|kind| world.entities.values().find(|e| e.kind == kind).unwrap().id);
        let home = world.entities[&left].home;
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: home.x + 3.,
            y: home.y + 4.,
        };
        world
            .damage_enemy(player, left, 1000, false, &[], Some(0))
            .unwrap();
        let mut checked = 0;
        let mut last: Option<(f32, f32)> = None;
        let mut textures = BTreeSet::new();
        for _ in 0..240_000 / 50 {
            world.step();
            let Some(clock) = world.finale.guardian_clock(right) else {
                continue;
            };
            let age = world.now - clock.start;
            if clock.block != finale::GuardianBlock::Spin
                || !(finale::GUARDIAN_SPIN_ORBIT_MS + 600
                    ..finale::GUARDIAN_SPIN_FIRST_MS + 25 * 200)
                    .contains(&age)
            {
                last = None;
                continue;
            }
            let core = world.finale.core.unwrap();
            let angle = |id: i32| {
                let p = world.entities[&id].pos;
                assert!(
                    (p.distance(core) - 4.).abs() < 0.3,
                    "orbit radius {}",
                    p.distance(core)
                );
                (p.y - core.y).atan2(p.x - core.x)
            };
            let (r, l) = (angle(right), angle(left));
            let apart = (r - l).rem_euclid(std::f32::consts::TAU);
            assert!(
                (apart - std::f32::consts::PI).abs() < 0.3,
                "180 degrees apart: {apart}"
            );
            if let Some((before, _)) = last {
                // Engine y grows south: counter-clockwise with north up lowers the angle.
                assert!((r - before).rem_euclid(std::f32::consts::TAU) > std::f32::consts::PI);
            }
            last = Some((r, l));
            textures.extend(world.entities[&right].extra_stats.iter().map(|s| s.1));
            checked += 1;
        }
        assert!(checked > 20, "no spin block within four minutes");
        assert!(textures.contains(&3) && textures.contains(&4));
    }

    #[test]
    fn guardians_drop_the_2012_list_and_minor_castle_enemies_drop_nothing() {
        use crate::loot::Pick;
        for kind in GUARDIANS {
            let table = finale::loot(kind);
            assert!(
                table
                    .iter()
                    .any(|(p, pick)| *p == 1. && matches!(pick, Pick::Item(0xa20)))
            );
            assert!(
                table
                    .iter()
                    .any(|(_, pick)| matches!(pick, Pick::Item(finale::INCANTATION)))
            );
            assert!(
                table
                    .iter()
                    .any(|(_, pick)| matches!(pick, Pick::Item(0xc05)))
            );
        }
        for kind in [0xd7c, 0xd7e, 0xd80] {
            assert!(finale::loot(kind).is_empty());
        }
        assert!(!crate::loot::private(0xd7f));
    }

    #[test]
    fn brute_rings_twelve_crescents_then_twelve_lines() {
        use crate::encounter::attacks::brute_schedule;
        let first = brute_schedule(false, 0, 1500).snapshot(0.);
        assert_eq!(first.len(), 12);
        assert!(first.iter().all(|s| s.0 == 2));
        let second = brute_schedule(false, 0, 1920).snapshot(0.);
        assert_eq!(second.len(), 12);
        assert!(second.iter().all(|s| s.0 == 1));
        assert!(brute_schedule(false, 0, 1700).snapshot(0.).is_empty());
        let rush = brute_schedule(true, 1, 450).snapshot(0.);
        assert!(rush.len() == 3 && rush.iter().all(|s| s.0 == 0));
    }

    #[test]
    fn insect_minions_ring_six_and_add_status_rings_when_red() {
        use crate::encounter::attacks::insect_schedule;
        let gold = insect_schedule(false, 0).snapshot(0.);
        assert_eq!(gold.iter().filter(|s| s.0 == 0).count(), 7);
        assert_eq!(insect_schedule(false, 750).snapshot(0.).len(), 1);
        let red = insect_schedule(true, 750).snapshot(0.);
        assert!(red.len() == 7 && red.iter().filter(|s| s.0 == 1).count() == 6);
    }

    #[test]
    fn haunted_armor_wakes_close_and_regenerates_six_times() {
        let (mut world, player) = castle();
        let armor = world
            .entities
            .values()
            .find(|e| e.kind == 0xd7e)
            .unwrap()
            .id;
        let at = world.entities[&armor].pos;
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: at.x + 6.,
            y: at.y,
        };
        run(&mut world, 1000);
        assert_eq!(world.entities[&armor].stage.unwrap_or(0), 0);
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: at.x + 2.,
            y: at.y,
        };
        run(&mut world, 200);
        assert_eq!(world.entities[&armor].stage, Some(1));
        world.entities.get_mut(&armor).unwrap().hp = 1000;
        run(&mut world, 100);
        assert_eq!(world.entities[&armor].stage, Some(2));
        assert_ne!(
            flags(&world, armor) & condition::bit(condition::INVULNERABLE),
            0
        );
        run(&mut world, 3850);
        assert_eq!(world.entities[&armor].hp, 1000 + 6 * 200);
        run(&mut world, 200);
        assert_eq!(world.entities[&armor].stage, Some(3));
        assert_eq!(
            flags(&world, armor) & condition::bit(condition::INVULNERABLE),
            0
        );
        // Once per armor.
        world.entities.get_mut(&armor).unwrap().hp = 500;
        run(&mut world, 500);
        assert_eq!(world.entities[&armor].stage, Some(3));
    }

    #[test]
    fn living_floors_ring_close_players_and_lob_bursting_quiet_bombs() {
        let (mut world, player) = castle();
        let floor = world
            .entities
            .values()
            .find(|e| e.kind == 0xd87 && e.pos.distance(Pos { x: 108.5, y: 33.5 }) < 0.01)
            .unwrap()
            .id;
        world.players.get_mut(&player).unwrap().pos = Pos { x: 107.5, y: 35.5 };
        world.step();
        assert_eq!(
            world.bullets.iter().filter(|b| b.owner == floor).count(),
            18
        );
        let mut burst: Option<(i32, usize)> = None;
        for _ in 0..80 {
            world.step();
            let bomb = world.entities.values().find(|e| {
                e.kind == 0xd86 && e.parent == Some(floor) && burst.is_none_or(|(id, _)| id == e.id)
            });
            match bomb {
                Some(bomb) => {
                    assert!(bomb.pos.distance(Pos { x: 107.5, y: 35.5 }) < 0.01);
                    let id = bomb.id;
                    burst = Some((id, world.bullets.iter().filter(|b| b.owner == id).count()));
                }
                None if burst.is_some() => break,
                None => {}
            }
        }
        assert_eq!(burst.expect("no Quiet Bomb landed").1, 18);
    }
}
