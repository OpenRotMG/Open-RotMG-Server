// Behavior reference (GPL-3.0; data/REALM-SRC-LICENSE): https://github.com/moistosaurus/realm-src/tree/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/db
// EntAncient/Lich/GhostKing/Phoenix contracts, credited upstream to NR-Core and Skilly; Rust reconstruction, not official AI.
// Stable encounter identity survives transformations; summon parenthood would delete surviving followers.
use super::*;
use crate::{
    condition,
    encounter::{Anchor, Motion},
    quest,
};

impl World {
    pub(super) fn populate_named_quests(&mut self) -> Result<()> {
        for kind in quest::ROSTER {
            quest::validate(&self.content, *kind)?;
            crate::loot::validate(&self.content, *kind)?;
        }
        for (kind, terrains) in [
            (0x91f, &[7, 6][..]),
            (0x91b, &[6, 7][..]),
            (0x928, &[4, 3][..]),
            (0x675, &[5, 2][..]),
        ] {
            let pos = self.place_named_setpiece(kind, terrains)?;
            let id = self.spawn_entity(kind, pos, vec![])?;
            let e = self.entities.get_mut(&id).unwrap();
            e.quest = Some(id);
            e.stage = Some(0);
            e.size = self.content.objects[&kind].size;
            tracing::info!(
                entity = id,
                kind,
                terrain = self.map.cell(pos.x, pos.y).unwrap().terrain,
                "finite named Realm quest placed"
            );
        }
        Ok(())
    }
    pub(super) fn quest_members(&self, id: i32, kinds: &[u16]) -> Vec<i32> {
        let tag = self.entities[&id].quest.expect("named quest membership");
        self.non_population_actors()
            .filter(|e| e.quest == Some(tag) && kinds.contains(&e.kind))
            .map(|e| e.id)
            .collect()
    }
    fn quest_leader(&self, id: i32) -> Option<i32> {
        self.quest_members(
            id,
            &[
                0x91f, 0x920, 0x91b, 0x91c, 0x928, 0x92d, 0x675, 0x6c2, 0x6c1,
            ],
        )
        .into_iter()
        .next()
    }
    fn quest_phase(&mut self, id: i32, stage: u64) {
        let e = self.entities.get_mut(&id).unwrap();
        e.stage = Some(stage);
        e.activated = Some(self.now);
        e.next_shot = self.now;
        e.next_aux = self.now;
        tracing::debug!(
            entity = id,
            kind = e.kind,
            quest = e.quest,
            stage,
            "named quest phase entered"
        );
        let text = match (e.kind, stage) {
            (0x91f, 1) => "Uhh. So... sleepy...",
            (0x91b, 1) => "New recruits for my undead army? How delightful!",
            (0x928, 1) => "No corporeal creature can kill my sorrow",
            (0x928, 7) => "I cannot be defeated while my loyal subjects sustain me!",
            (0x929, 3) => "Your secret soul master is dying, Your Majesty",
            (0x6c1, 0) => "Some die and are ashes, but I am ever reborn!",
            _ => return,
        };
        let packet = chat::text_packet(
            &format!("#{}", self.content.objects[&e.kind].name),
            id,
            5,
            text,
        );
        for p in self.players.values().filter(|p| p.known.contains(&id)) {
            send(p, packet.id, packet.body.clone());
        }
    }
    pub(super) fn quest_shield(&mut self, id: i32, shield: bool) {
        let e = self.entities.get_mut(&id).unwrap();
        e.condition &= !condition::bit(condition::INVULNERABLE);
        if shield {
            e.condition |= condition::bit(condition::INVULNERABLE);
        }
    }
    fn quest_spawn(
        &mut self,
        id: i32,
        kind: u16,
        maximum: usize,
        count: usize,
        radius: f32,
    ) -> Result<()> {
        let e = &self.entities[&id];
        self.quest_birth(
            (
                e.quest.context("quest summon has no encounter identity")?,
                e.home,
                e.pos,
            ),
            kind,
            maximum,
            count,
            radius,
        )
    }
    pub(super) fn quest_birth(
        &mut self,
        owner: (i32, Pos, Pos),
        kind: u16,
        maximum: usize,
        count: usize,
        radius: f32,
    ) -> Result<()> {
        crate::encounter::validate(&self.content, kind)?;
        let (tag, home, center) = owner;
        let own =
            self.actors().filter(|e| e.quest == Some(tag)).count() + self.pending_named(tag, None);
        let live = self
            .actors()
            .filter(|e| e.quest == Some(tag) && e.kind == kind)
            .count()
            + self.pending_named(tag, Some(kind));
        let count = count.min(maximum.saturating_sub(live));
        if count == 0 {
            return Ok(());
        }
        if own + count > 32 {
            tracing::debug!(
                quest = tag,
                kind,
                own,
                count,
                "named quest birth refused at encounter capacity"
            );
            return Ok(());
        }
        ensure!(
            self.actors()
                .filter(|e| e.parent.is_some() || e.quest.is_some())
                .count()
                + self.named_births.len()
                + count
                <= 256,
            "shared summon budget exhausted"
        );
        let mut positions = Vec::new();
        for n in 0..128 {
            let a = n as f32 * std::f32::consts::TAU / 32.;
            let r = radius * (1. + (n / 32) as f32 * 0.2);
            let p = Pos {
                x: center.x + r * a.cos(),
                y: center.y + r * a.sin(),
            };
            if self.walkable(p) && positions.iter().all(|p0: &Pos| p.distance(*p0) >= 0.5) {
                positions.push(p);
            }
        }
        ensure!(
            positions.len() >= count,
            "quest summon has insufficient walkable positions"
        );
        for n in 0..count {
            let p = positions[n * positions.len() / count];
            let child = self.spawn_entity(kind, p, vec![])?;
            let e = self.entities.get_mut(&child).unwrap();
            e.quest = Some(tag);
            e.home = home;
            e.stage = Some(0);
            e.activated = Some(self.now);
            e.expires = Some(
                self.now
                    + match kind {
                        0x91d => 130000,
                        0x92a..=0x92c => 160000,
                        _ => 90000,
                    },
            );
            e.size = self.content.objects[&kind].size;
            if matches!(kind, 0x91e | 0x923 | 0x929..=0x92c) {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
        }
        tracing::debug!(
            quest = tag,
            kind,
            count,
            maximum,
            "bounded named quest followers spawned"
        );
        Ok(())
    }
    pub(super) fn quest_transform(&mut self, id: i32, kind: u16) -> Result<i32> {
        quest::validate(&self.content, kind)?;
        let old = &self.entities[&id];
        ensure!(old.quest.is_some(), "quest transformation lacks ownership");
        let new = self.spawn_entity(kind, old.pos, vec![])?;
        let old = self.remove_entity(id);
        let e = self.entities.get_mut(&new).unwrap();
        e.quest = old.quest;
        e.home = old.home;
        e.damage = old.damage;
        e.stage = None;
        e.activated = Some(self.now);
        e.expires = match kind {
            0x924 => Some(self.now + 60000),
            0x92b | 0x92c => Some(self.now + 160000),
            _ => old.expires,
        };
        e.size = self.content.objects[&kind].size;
        if quest::final_root(kind) || kind == 0x6c2 {
            e.expires = None;
        }
        if matches!(kind, 0x920 | 0x6c2 | 0x92b | 0x92c) {
            e.condition |= condition::bit(condition::INVULNERABLE);
        }
        tracing::info!(
            entity = id,
            replacement = new,
            kind,
            quest = e.quest,
            "named quest transformed without rewards"
        );
        Ok(new)
    }
    pub(super) fn quest_spawn_survivors(
        &mut self,
        defeated: &Entity,
        kind: u16,
        count: usize,
    ) -> Result<()> {
        let tag = defeated.quest.context("quest survivor ownership absent")?;
        self.quest_birth((tag, defeated.home, defeated.pos), kind, 24, count, 0.5)
    }
    pub(super) fn quest_lethal_transform(&mut self, id: i32) -> Result<bool> {
        let e = &self.entities[&id];
        if e.quest.is_none() {
            return Ok(false);
        }
        let kind = match e.kind {
            0x675 => 0x6c2,
            0x6c2 => 0x6c1,
            0x92a => 0x92b,
            0x92b => 0x92c,
            _ => return Ok(false),
        };
        self.quest_transform(id, kind)?;
        Ok(true)
    }
    fn quest_tier(&self, id: i32) -> u64 {
        let e = &self.entities[&id];
        let lost = self.content.objects[&e.kind].hp - e.hp;
        if lost > 37500 {
            3
        } else if lost > 14400 {
            2
        } else if lost > 4500 {
            1
        } else {
            0
        }
    }
    pub(super) fn quest_mechanics(&mut self, id: i32) -> Result<()> {
        if (0xd56..=0xd60).contains(&self.entities[&id].kind) {
            return self.event_mechanics(id);
        }
        if self.entities[&id].stage.is_none() {
            self.quest_phase(id, 0);
        }
        let e = &self.entities[&id];
        let (kind, hp, stage, t) = (
            e.kind,
            e.hp,
            e.stage.unwrap_or(0),
            self.now - e.activated.unwrap(),
        );
        if matches!(kind, 0x91b | 0x91f | 0x928) {
            match stage {
                0 if hp < self.content.objects[&kind].hp => {
                    self.quest_phase(id, 1);
                    self.quest_shield(id, true);
                }
                1 if t >= 2500 => {
                    self.quest_phase(id, 2);
                    self.quest_shield(id, false);
                }
                2 if t >= 6000 || hp < 261000 => {
                    let tier = self.quest_tier(id);
                    self.quest_phase(id, 3 + tier);
                    self.quest_shield(id, kind != 0x91b);
                    match kind {
                        0x91f => {
                            for (k, n) in [
                                (0x923, usize::from(tier == 1)),
                                (0x921, [0, 2, 5, 11][tier as usize]),
                            ] {
                                self.quest_spawn(id, k, n, n, 3.)?;
                            }
                        }
                        0x91b => {
                            let n = [0, 2, 3, 5][tier as usize];
                            self.quest_spawn(id, 0x91d, 8, n, 5.5)?;
                        }
                        0x928 => self.ghost_wave(id, tier, false)?,
                        _ => unreachable!(),
                    }
                }
                3..=6 => {
                    let tier = (stage - 3) as usize;
                    if kind == 0x91f {
                        if t >= if tier < 2 { 3000 } else { 5000 } {
                            self.quest_transform(id, 0x920)?;
                        } else if tier >= 2
                            && t > 0
                            && t.is_multiple_of(if tier == 3 { 400 } else { 1000 })
                        {
                            self.quest_spawn(id, 0x923, if tier == 3 { 6 } else { 3 }, 1, 3.)?;
                        }
                    } else if kind == 0x91b {
                        let delay = [3000, 20000, 27000, 30000][tier];
                        if t >= delay {
                            self.quest_transform(id, 0x91c)?;
                        } else {
                            if t == [0, 15000, 22000, 25000][tier] && tier >= 2 {
                                self.quest_spawn(id, 0x91d, 8, 3, 5.5)?;
                            }
                            let (max, interval) = match tier {
                                1 if t >= 15000 => (1, 9000),
                                2 => (2, 2000),
                                3 => (5, 3000),
                                _ => (0, 1),
                            };
                            if max > 0
                                && t > 0
                                && (if tier == 1 { t - 15000 } else { t }).is_multiple_of(interval)
                            {
                                self.quest_spawn(id, 0x91e, max, 1, 3.)?;
                            }
                        }
                    } else if kind == 0x928 {
                        let delay = [1000, 32000, 32000, 62000][tier];
                        if tier == 3 && t == 30000 {
                            self.ghost_wave(id, 3, false)?;
                        }
                        if tier > 0 && t == delay - 2000 {
                            self.ghost_wave(id, tier as u64, true)?;
                        }
                        if t >= delay {
                            self.quest_phase(id, 7);
                        }
                    }
                }
                7 if kind == 0x928 && t >= 140000 => {
                    let pos = self.entities[&id].pos;
                    for ghost in self.quest_members(id, &[0x929, 0x92a, 0x92b, 0x92c]) {
                        if self.entities[&ghost].pos.distance(pos) <= 30. {
                            self.remove_entity(ghost);
                        }
                    }
                    self.quest_transform(id, 0x92d)?;
                }
                _ => {}
            }
            return Ok(());
        }
        match kind {
            0x920 => {
                // The readable reference's timed transitions are reachable while growth is invulnerable; HP triggers are not.
                let phase = (t / 1600).min(9);
                let changed = e.stage != Some(phase);
                if changed {
                    self.entities.get_mut(&id).unwrap().stage = Some(phase);
                    tracing::debug!(entity = id, phase, "Ent growth entered");
                }
                self.entities.get_mut(&id).unwrap().size = 160 + phase as i32 * 20;
                if phase == 9 && changed {
                    for sprite in self.quest_members(id, &[0x923]) {
                        self.quest_transform(sprite, 0x924)?;
                    }
                    if !self.quest_members(id, &[0x924]).is_empty() {
                        self.entities.get_mut(&id).unwrap().condition |=
                            condition::bit(condition::ARMORED);
                    }
                }
                self.quest_shield(id, t < 16400 || (t - 16400) % 8000 < 4000);
                if t > 0 && t.is_multiple_of(3000) {
                    self.quest_spawn(id, 0x922, 3, 1, 2.)?;
                }
            }
            0x91c => {
                if t == 0 || t.is_multiple_of(4000) {
                    for (k, n, initial) in [(0x65f, 4, 2), (0x660, 2, 1), (0x661, 1, 0)] {
                        self.quest_spawn(id, k, n, if t == 0 { initial } else { 1 }, 2.)?;
                    }
                }
            }
            0x91d | 0x924 => {
                let shield = kind == 0x91d && hp < 1800 && t % 5000 < 2500;
                self.quest_shield(id, shield);
                if t.is_multiple_of(200)
                    && self.entities[&id].conditions(self.now) & condition::bit(condition::STUNNED)
                        == 0
                    && let Some(leader) = self.quest_leader(id).filter(|leader| {
                        self.entities[leader].pos.distance(self.entities[&id].pos) < 15.
                    })
                {
                    self.heal_enemy(
                        id,
                        leader,
                        self.content.objects[&self.entities[&leader].kind].hp,
                    )?;
                }
                if kind == 0x924
                    && t > 0
                    && t.is_multiple_of(3000)
                    && self.quest_leader(id).is_some()
                {
                    self.quest_spawn(id, 0x921, 23, 1, 2.)?;
                }
            }
            0x91e => self.quest_shield(id, t % 10000 < 7000),
            0x923 => self.quest_shield(id, true),
            0x929 => {
                if hp < 94000 {
                    if stage != 3 {
                        self.quest_phase(id, 3);
                        for king in self.quest_members(id, &[0x928]) {
                            self.quest_transform(king, 0x92d)?;
                        }
                    } else if t >= 3000 {
                        self.remove_entity(id);
                    }
                } else {
                    let next = if hp < 98000 {
                        2
                    } else if hp < 99000 {
                        1
                    } else {
                        0
                    };
                    let t = if next != stage {
                        self.quest_phase(id, next);
                        0
                    } else {
                        t
                    };
                    let intro = u64::from(next > 0) * 1000;
                    self.quest_shield(
                        id,
                        t < intro || (t - intro) % [10000, 7000, 4000][next as usize] < 1000,
                    );
                }
            }
            0x92a..=0x92c => {
                let period = match kind {
                    0x92a => 10000,
                    0x92b => 7000,
                    _ => 4000,
                };
                let intro = u64::from(kind != 0x92a) * 1000;
                self.quest_shield(id, t < intro || (t - intro) % period < 1000);
            }
            0x675 | 0x6c1 => {
                if t == 0 {
                    self.quest_spawn(id, 0x676, 8, if kind == 0x675 { 4 } else { 1 }, 2.)?;
                    self.quest_spawn(id, 0x677, 8, if kind == 0x675 { 4 } else { 1 }, 2.5)?;
                }
                let interval = if kind == 0x675 { 5000 } else { 1000 };
                let births = if kind == 0x675 { 8 } else { 2 };
                if t > 0 && t % interval == 0 && t / interval <= births {
                    let k = if (t / interval) % 2 == 0 {
                        0x676
                    } else {
                        0x677
                    };
                    self.quest_spawn(id, k, 8, 1, 2.)?;
                }
            }
            0x6c2 => {
                self.quest_shield(id, t < 2000);
                self.entities.get_mut(&id).unwrap().size =
                    if (t / 800) % 2 == 0 { 150 } else { 130 };
            }
            _ => {}
        }
        Ok(())
    }
    fn ghost_wave(&mut self, id: i32, tier: u64, company: bool) -> Result<()> {
        if company || tier == 0 {
            self.quest_spawn(id, 0x929, 1, 1, 4.)?;
        }
        let (small, medium, large) = if tier == 0 {
            (4, 0, 0)
        } else if company {
            if tier == 1 { (2, 3, 0) } else { (0, 2, 3) }
        } else if tier == 1 {
            (3, 3, 0)
        } else {
            (3, 0, 3)
        };
        for (k, n) in [(0x92a, small), (0x92b, medium), (0x92c, large)] {
            self.quest_spawn(id, k, 24, n, 4.)?;
        }
        Ok(())
    }
    pub(super) fn quest_movement(
        &self,
        id: i32,
        target: impl Into<Option<Pos>>,
        elapsed: u64,
    ) -> (Motion, f32) {
        if (0xd56..=0xd60).contains(&self.entities[&id].kind) {
            return self.event_movement(id, elapsed);
        }
        let target = target.into();
        let e = &self.entities[&id];
        let leash = match e.kind {
            0x91f | 0x920 => 6.,
            0x91b | 0x91c | 0x91d | 0x6c1 => 5.,
            0x923 | 0x924 | 0x91e => 11.,
            0x929..=0x92c => 7.,
            0x675 => 2.,
            0x928 if e.stage.unwrap_or(0) >= 7 => 8.,
            0x928 => 3.,
            _ => 16.,
        };
        if e.pos.distance(e.home) > leash {
            return (
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 1.,
                },
                12.,
            );
        }
        let (motion, rate_tenths) = match e.kind {
            0x6c2 => (Motion::Stationary, 0.),
            0x92d => (Motion::Pace { distance: 3. }, 5.),
            0x928 if e.stage == Some(0) => (Motion::Pace { distance: 3. }, 3.),
            0x928 if e.stage.unwrap_or(0) >= 7 && elapsed % 4000 >= 2000 => {
                (Motion::Stationary, 0.)
            }
            0x928 if e.stage.unwrap_or(0) >= 7 => (
                Motion::Chase {
                    sight: 10.,
                    stop: 2.,
                },
                6.,
            ),
            0x91c => {
                if let Some(bearer) = self.quest_members(id, &[0x91d]).first()
                    && e.pos.distance(self.entities[bearer].pos) > 2.
                {
                    (
                        Motion::Protect {
                            anchor: Anchor::QuestMember(self.entities[bearer].kind),
                            distance: 2.,
                        },
                        9.,
                    )
                } else {
                    (Motion::Wander, 5.)
                }
            }
            0x676 | 0x677 | 0x65f..=0x661 => {
                let protection = if e.kind == 0x677 { 11. } else { 10. };
                let birdman = matches!(e.kind, 0x676 | 0x677);
                if let Some(leader) = self.quest_leader(id)
                    && e.pos.distance(self.entities[&leader].pos) > protection
                {
                    (
                        Motion::Protect {
                            anchor: Anchor::QuestMember(self.entities[&leader].kind),
                            distance: protection,
                        },
                        if birdman { 5. } else { 10. },
                    )
                } else if target.is_some_and(|p| {
                    e.pos.distance(p) < 10.
                        && e.pos.distance(p) > if e.kind == 0x676 { 9. } else { 7. }
                }) {
                    (
                        Motion::Chase {
                            sight: 10.,
                            stop: if e.kind == 0x676 { 9. } else { 7. },
                        },
                        if birdman { 10. } else { 12. },
                    )
                } else {
                    (Motion::Wander, if birdman { 5. } else { 4. })
                }
            }
            0x921 | 0x922 => {
                let (distance, speed) = if e.kind == 0x921 {
                    (7., 2.5)
                } else {
                    (4., 5.5)
                };
                if let Some(leader) = self.quest_leader(id)
                    && e.pos.distance(self.entities[&leader].pos) > distance
                {
                    (
                        Motion::Protect {
                            anchor: Anchor::QuestMember(self.entities[&leader].kind),
                            distance,
                        },
                        speed,
                    )
                } else if e.kind == 0x921 {
                    (
                        Motion::Chase {
                            sight: 9.,
                            stop: 1.,
                        },
                        speed,
                    )
                } else {
                    (Motion::Wander, speed)
                }
            }
            0x923 | 0x924 if target.is_some_and(|p| e.pos.distance(p) < 7.) => (
                Motion::Orbit {
                    anchor: Anchor::Player,
                    radius: 4.,
                },
                15.,
            ),
            0x923 | 0x924 => (Motion::Stationary, 0.),
            0x91d if e.hp < 1800 => (
                Motion::Wander,
                if elapsed % 5000 < 2500 { 13. } else { 10. },
            ),
            0x91d if elapsed % 3000 < 1500 => {
                let (speed, sight) = if e.hp >= 3900 { (5.5, 5.) } else { (6.5, 10.) };
                if target.is_some_and(|p| e.pos.distance(p) < sight) {
                    (
                        Motion::Orbit {
                            anchor: Anchor::Player,
                            radius: 4.,
                        },
                        speed,
                    )
                } else {
                    (Motion::Stationary, 0.)
                }
            }
            0x91d if target.is_some_and(|p| e.pos.distance(p) < 2.) => (
                Motion::Retreat { distance: 2. },
                if e.hp >= 3900 { 5.5 } else { 6.5 },
            ),
            0x91d => (Motion::Wander, if e.hp >= 3900 { 5.5 } else { 6.5 }),
            0x91e if elapsed % 10000 >= 7000 => (Motion::Stationary, 0.),
            0x91e => (Motion::Wander, 10.),
            0x929..=0x92c
                if self.entities[&id].condition & condition::bit(condition::INVULNERABLE) == 0 =>
            {
                (Motion::Stationary, 0.)
            }
            0x929..=0x92c => (Motion::Wander, 20.),
            0x920 => (Motion::Wander, 2.),
            0x6c1 => (Motion::Wander, 9.),
            0x675 => (Motion::Wander, 4.),
            0x91f if e.stage == Some(0) => (Motion::Wander, 1.),
            _ => (Motion::Wander, 4.),
        };
        // NR-Core adds a base motor rate; a linear speed conversion loses it on slow quest bosses.
        (
            motion,
            if rate_tenths == 0. {
                0.
            } else {
                (5.55 * rate_tenths / 10. + 0.74) * 1.5
            },
        )
    }
    pub(super) fn quest_shots(&self, id: i32, target: Pos, t: u64) -> Result<Vec<(u8, f32)>> {
        if (0xd56..=0xd60).contains(&self.entities[&id].kind) {
            return self.event_shots(id, target, t);
        }
        let e = &self.entities[&id];
        let k = e.kind;
        let s = e.stage.unwrap_or(0);
        let heading = (target.y - e.pos.y).atan2(target.x - e.pos.x);
        let ring = |index, n, offset| quest::ring(index, n, offset);
        let fan = |index, n, angle| quest::fan(index, n, angle, heading);
        let mut shots = match k {
            0x91f | 0x91b | 0x928 if s == 2 => match k {
                0x91f if t.is_multiple_of(800) => ring(0, 2, heading),
                0x928 if t.is_multiple_of(1000) => fan(0, 4, 30.),
                0x91b if [200, 400, 2200, 2400, 4200, 4400].contains(&t) => ring(1, 3, heading),
                _ => vec![],
            },
            0x928 if s >= 7 && t.is_multiple_of(1000) => fan(0, 1, 0.),
            0x920 => {
                let stage = (t / 1600).min(9) as u8;
                let mut shots = if t.is_multiple_of(1000) {
                    if matches!(stage, 1 | 4 | 7) {
                        ring(stage, 3, heading)
                    } else {
                        fan(stage, 1, 0.)
                    }
                } else {
                    vec![]
                };
                if stage == 9
                    && e.condition & condition::bit(condition::INVULNERABLE) == 0
                    && t.is_multiple_of(700)
                {
                    shots.extend(fan(3, 3, 120.));
                }
                shots
            }
            0x91c => {
                let mut shots = if t.is_multiple_of(800) {
                    fan(0, 2, 7.)
                } else {
                    vec![]
                };
                if t % 14000 >= 8000 && t.is_multiple_of(1000) {
                    shots.extend(fan(1, 4, 7.));
                }
                shots
            }
            0x91d if e.hp >= 3900 && t % 900 == 400 => ring(0, 3, heading),
            0x91d if (1800..3900).contains(&e.hp) && t % 600 == 200 => fan(0, 3, 15.),
            0x91d if e.hp < 1800 && t.is_multiple_of(800) => fan(1, 1, 0.),
            0x91e => {
                let phase = t % 10000;
                let mut shots = if phase < 7000 && phase.is_multiple_of(700)
                    || phase >= 7000 && (phase - 7000).is_multiple_of(700)
                {
                    fan(0, 1, 0.)
                } else {
                    vec![]
                };
                if phase >= 7200 && (phase - 7200).is_multiple_of(700) {
                    shots.extend(fan(0, 2, 70.));
                }
                shots
            }
            0x921 if t.is_multiple_of(600) => fan(0, 1, 0.),
            0x922 if t.is_multiple_of(1000) => fan(0, 1, 0.),
            0x923 | 0x924 if t.is_multiple_of(1000) => fan(0, 4, 10.),
            0x929 if s == 3 && t == 2950 => ring(0, 8, 22.5f32.to_radians()),
            0x929
                if s < 3
                    && e.condition & condition::bit(condition::INVULNERABLE) == 0
                    && t.is_multiple_of(400) =>
            {
                ring(0, 4, [0f32, 45., 22.5][s as usize].to_radians())
            }
            0x92a..=0x92c
                if e.condition & condition::bit(condition::INVULNERABLE) == 0
                    && t.is_multiple_of(if k == 0x92a { 400 } else { 800 }) =>
            {
                ring(
                    0,
                    if k == 0x92c { 8 } else { 4 },
                    if k == 0x92a { 0. } else { 45f32.to_radians() },
                )
            }
            0x675 if t.is_multiple_of(600) => fan(0, 4, 7.),
            0x676 | 0x677 | 0x65f..=0x661 if t.is_multiple_of(1000) => fan(0, 1, 0.),
            0x669 if t.is_multiple_of(250) => fan(0, 1, 0.),
            0x6c1 => {
                let cycle = if t < 1800 { t } else { (t - 1800) % 7300 };
                if t < 1800 && [500, 1000].contains(&t) {
                    ring(0, 12, if t == 500 { 10f32 } else { 25. }.to_radians())
                } else if t >= 1800 && cycle < 2800 && cycle.is_multiple_of(600) {
                    fan(1, 4, 7.)
                } else if t >= 1800 && cycle >= 2800 {
                    let x = cycle - 2800;
                    let mut out = if [500, 1000].contains(&x) {
                        ring(0, 12, if x == 500 { 10f32 } else { 25. }.to_radians())
                    } else {
                        vec![]
                    };
                    if x.is_multiple_of(350) {
                        out.extend(fan(1, 1, 0.));
                    }
                    out
                } else {
                    vec![]
                }
            }
            _ => vec![],
        };
        let factor = match k {
            0x675 | 0x6c1 | 0x677 => 0.5,
            0x921 => 0.4,
            0x91d if (1800..3900).contains(&e.hp) => 0.1,
            0x91e => {
                if t % 10000 < 7000 {
                    0.2
                } else {
                    0.3
                }
            }
            _ => 0.,
        };
        self.predict_enemy_shots(id, target, &mut shots, |_| factor)?;
        Ok(shots)
    }
}
