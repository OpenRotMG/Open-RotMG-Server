// GPL-3.0 comparator: realm-src@1855c017eab870d13f0b61b1cf0b9e66a01922ff,
// BehaviorDb.Lab.cs (sebastianfra12, ossimc82); data/REALM-SRC-LICENSE.
use super::*;
use crate::{
    condition,
    dungeon::lab,
    encounter::{Anchor, Motion, nr_speed},
};

pub(super) struct Toss {
    owner: i32,
    pub(super) kind: u16,
    position: Pos,
    deadline: u64,
}

// 123.5.1 chat: t0f0_BWpBtw (2012-09-08) t=170-330, t=908-926. Which phase speaks which line is an estimate.
pub(super) const OUT_TAUNTS: &[&str] = &[
    "For Science!",
    "Try my latest concoction.",
    "Don't worry, your death will be documented closely.",
    "Side effects may include your death!",
    "Time to delegate your death to my minions!",
    "If my calculations are correct, this will hurt you.",
    "You'll make a nice snack for my creations.",
];
pub(super) const BUBBLE_TAUNTS: &[&str] = &[
    "Great Scott! I must retreat.",
    "Time to feed my pets!",
    "You've beaten me back.",
];
// C4sKt3Se0T0 (2012-09-27) t=162-330; the 10 s cadence is an estimate.
pub(super) const CREATION_TAUNTS: &[&str] = &[
    "Attacks no hurt me!!!",
    "RAHHHHHHHHH",
    "Little people's attacks tickle.",
    "You hurt master!",
    "Me can do this all day!",
    "You no hurt me.",
    "What, you scared???",
];

impl World {
    /// Speech bubble and chat line. Horrific Creation shouts from its sealed room to the whole Lab.
    fn lab_say(&mut self, id: i32, lines: &[&str]) {
        let text = *lines.choose(&mut self.rng).expect("Lab taunt lines");
        let e = &self.entities[&id];
        tracing::info!(entity = id, kind = e.kind, %text, "Mad Lab taunt");
        let name = format!("#{}", self.content.objects[&e.kind].name);
        let packet = chat::text_packet(&name, id, 5, text);
        for p in self
            .players
            .values()
            .filter(|p| e.kind == lab::CREATION || p.pos.distance(e.pos) < 20.)
        {
            send(p, packet.id, packet.body.clone());
        }
    }

    pub(super) fn lab_step(&mut self) {
        let pools: Vec<_> = self
            .world_effect_actors()
            .filter(|e| matches!(e.kind, lab::HEXXER | lab::DEHEXXER))
            .map(|e| (e.kind, e.pos))
            .collect();
        for (id, player) in &mut self.players {
            if !player.character.alive || player.death_pending || player.departing {
                player.lab_conditions = 0;
                continue;
            }
            if player.paused {
                continue;
            }
            // Pools act on players standing in a vat, so a walkway beside one stays dry.
            let wet = matches!(
                self.map.cells
                    [(player.pos.y as i32 * self.map.width + player.pos.x as i32) as usize]
                    .ground,
                lab::BAD_VAT | lab::GOOD_VAT
            );
            let inside = |center: Pos| {
                (center.x as i32 - player.pos.x as i32).abs() < 2
                    && (center.y as i32 - player.pos.y as i32).abs() < 2
            };
            // Actor geometry is reconstructed, not inferred from particle extents or water IDs; blue wins overlaps.
            let blue = wet
                && pools
                    .iter()
                    .any(|(kind, pos)| *kind == lab::DEHEXXER && inside(*pos));
            let old = player.lab_conditions;
            if blue {
                player.lab_conditions = 0;
                player.statuses.remove(&condition::HEXED);
            } else if wet
                && pools
                    .iter()
                    .any(|(kind, pos)| *kind == lab::HEXXER && inside(*pos))
            {
                player.lab_conditions =
                    condition::bit(condition::HEXED) | condition::bit(condition::SPEEDY) | STUNNED;
            }
            if old != player.lab_conditions {
                tracing::info!(
                    player = id,
                    before = old,
                    after = player.lab_conditions,
                    remedy = blue,
                    "Lab pool source conditions changed"
                );
            }
        }
        let mut i = 0;
        while i < self.lab_tosses.len() {
            if self.lab_tosses[i].deadline > self.now {
                i += 1;
                continue;
            }
            let toss = self.lab_tosses.remove(i);
            if let Err(error) = self.lab_spawn(toss.owner, toss.kind, toss.position) {
                tracing::error!(owner=toss.owner, kind=toss.kind, %error, "admitted Lab toss failed to land");
            }
        }
    }

    fn lab_spawn(&mut self, owner: i32, kind: u16, position: Pos) -> Result<i32> {
        ensure!(self.walkable(position), "Lab summon landing blocked");
        crate::encounter::validate(&self.content, kind)?;
        crate::loot::validate(&self.content, kind)?;
        ensure!(
            self.summon_count() + self.lab_tosses.len() < self.summon_limit(),
            SummonBudget("Lab shared summon budget exhausted".into())
        );
        let id = self.spawn_entity(kind, position, vec![])?;
        // TossObject's admitted 1500ms landing survives its thrower's death.
        let parent = self.entities.contains_key(&owner).then_some(owner);
        self.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(parent, id, &mut self.timed_ids);
        Ok(id)
    }

    fn lab_stage(&mut self, id: i32, stage: u64) {
        let e = self.entities.get_mut(&id).unwrap();
        e.stage = Some(stage);
        e.activated = Some(self.now);
        e.next_aux = self.now;
        e.next_shot = self.now;
        tracing::info!(
            entity = id,
            kind = e.kind,
            stage,
            "Lab encounter phase entered"
        );
    }

    pub(super) fn lab_mechanics(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<()> {
        let e = &self.entities[&id];
        let kind = e.kind;
        if kind == lab::BOSS {
            let mut stage = e.stage.unwrap_or(0);
            // Approved reconstruction makes the comparator's idle-only rage gate global and irreversible.
            let rage = e.hp < self.content.objects[&kind].hp / 5;
            // Even stages walk out with gas, odd ones hide in the bubble, 20 s each; stage 8 is rage.
            // t0f0_BWpBtw (2012-09-08, 2x clip) t=269-330 and hiaac1s96Ow (2012-09-01) t=484-545: 40 s period.
            let entered = e.stage.is_none() || stage < 8 && (rage || elapsed >= 20000);
            if entered {
                stage = if rage {
                    8
                } else if e.stage.is_none() {
                    0
                } else {
                    (stage + 1) % 8
                };
                self.lab_stage(id, stage);
                let hide = stage % 2 == 1;
                let e = self.entities.get_mut(&id).unwrap();
                if hide {
                    e.condition |= condition::bit(condition::INVINCIBLE);
                } else {
                    e.condition &= !condition::bit(condition::INVINCIBLE);
                }
                self.landmark_texture(id, i32::from(hide));
                if hide {
                    let home = self.entities[&id].home;
                    let cages: Vec<_> = self
                        .entities
                        .values()
                        .filter(|e| e.kind == lab::CAGE)
                        .map(|e| e.pos)
                        .collect();
                    for position in cages {
                        let caged: Vec<_> = self
                            .actors()
                            .filter(|e| e.parent == Some(id) && matches!(e.kind, 0x1e23..=0x1e25))
                            .map(|e| e.home)
                            .collect();
                        // One minion per cage (https://wikiwiki.jp/rmd/Dr%20Terrible, undated body), at most four alive
                        // (C4sKt3Se0T0 2012-09-27 minimap). The uniform kind draw is an estimate.
                        if caged.len() >= 4 || caged.iter().any(|h| h.distance(position) < 0.1) {
                            continue;
                        }
                        // Selected cages occupy their wall cell: release straight into the room without bypassing collision.
                        let landing = Pos {
                            x: position.x + (home.x - position.x).signum(),
                            y: position.y,
                        };
                        let kind = [0x1e23, 0x1e24, 0x1e25][self.rng.gen_range(0..3)];
                        let child = self.lab_spawn(id, kind, landing)?;
                        self.entities.get_mut(&child).unwrap().home = position;
                    }
                }
            }
            let elapsed = if entered { 0 } else { elapsed };
            let hide = stage % 2 == 1;
            let safe = (stage / 2) as u16;
            let fixtures: Vec<_> = self
                .entities
                .values()
                .filter(|e| matches!(e.kind, lab::BUBBLE | lab::CAGE | 0x993..=0x996))
                .map(|e| e.id)
                .collect();
            for fixture in fixtures {
                let e = self.entities.get_mut(&fixture).unwrap();
                e.parent.set(Some(id), fixture, &mut self.timed_ids);
                if (0x993..=0x996).contains(&e.kind) {
                    e.stage = Some(u64::from(!hide && stage < 8 && e.kind != 0x993 + safe));
                }
                let kind = e.kind;
                if kind == lab::BUBBLE {
                    self.landmark_texture(
                        fixture,
                        if hide {
                            1 + ((self.now / 1000) % 2) as i32
                        } else {
                            0
                        },
                    );
                }
                if kind == lab::CAGE {
                    self.landmark_texture(fixture, if hide { 2 } else { 0 });
                }
            }
            if entered || !hide && self.now >= self.entities[&id].reaction_until {
                self.lab_say(id, if hide { BUBBLE_TAUNTS } else { OUT_TAUNTS });
                self.entities.get_mut(&id).unwrap().reaction_until = self.now + 10000;
            }
            if !hide
                && self.now >= self.entities[&id].next_aux
                && self.entities[&id].conditions(self.now) & STUNNED == 0
            {
                // Potions reach a player about 8 tiles away: t0f0_BWpBtw (2012-09-08) t=912-926. 10 tiles is an estimate.
                let pos = self.entities[&id].pos;
                let players: Vec<_> = self
                    .players
                    .values()
                    .filter(|p| {
                        p.character.alive
                            && !p.death_pending
                            && !p.departing
                            && !p.paused
                            && p.conditions(self.now) & condition::bit(condition::INVISIBLE) == 0
                            && p.pos.distance(pos) <= 10.
                            && self.walkable(p.pos)
                    })
                    .map(|p| p.pos)
                    .collect();
                if let Some(&position) = players.choose(&mut self.rng) {
                    // No new turrets in rage (t=908-926); OUT alternating potions and turrets is an estimate.
                    let kind = if stage < 8 && elapsed / 8000 % 2 == 1 {
                        0x12aa
                    } else {
                        0x978
                    };
                    ensure!(
                        self.actors().filter(|e| e.parent == Some(id)).count()
                            + self.lab_tosses.iter().filter(|t| t.owner == id).count()
                            < 128,
                        SummonBudget("Dr Terrible owned summon budget exhausted".into())
                    );
                    self.telegraph_throw(id, position, 0xffffbf00);
                    self.lab_tosses.push(Toss {
                        owner: id,
                        kind,
                        position,
                        deadline: self.now + 1500,
                    });
                }
                // Rage potions land about every 8 s (t0f0_BWpBtw 2x clip t=912.5-925.5); OUT reuses it as an estimate.
                self.entities.get_mut(&id).unwrap().next_aux = self.now + 8000;
            }
        } else if matches!(kind, 0x981 | 0x1e23) {
            // Dr Terrible's Experiments heal him +500, also inside his bubble: t0f0_BWpBtw (2012-09-08) t=332-335,
            // C4sKt3Se0T0 (2012-09-27) t=40. Dungeon ones heal Crushers +300: https://wikiwiki.jp/rmd/Escaped%20Experiment
            // (2012-12-13). The 2/3-tile reach and the 2.5 s cadence are estimates.
            let pos = e.pos;
            let patient = if kind == 0x1e23 {
                e.parent.filter(|boss| {
                    self.entities
                        .get(boss)
                        .is_some_and(|b| b.pos.distance(pos) <= 2.)
                })
            } else {
                self.actors()
                    .filter(|c| c.kind == 0x983 && c.pos.distance(pos) <= 3.)
                    .min_by(|a, b| a.pos.distance(pos).total_cmp(&b.pos.distance(pos)))
                    .map(|c| c.id)
            };
            if let Some(patient) = patient
                && self.now >= e.next_aux
            {
                self.heal_enemy(id, patient, if kind == 0x1e23 { 500 } else { 300 })?;
                self.entities.get_mut(&id).unwrap().next_aux = self.now + 2500;
            }
        } else if kind == 0x978 {
            if elapsed >= 2000 && e.stage.is_none() {
                self.lab_stage(id, 1);
                self.entities.get_mut(&id).unwrap().expires.set(
                    Some(self.now + 50),
                    id,
                    &mut self.timed_ids,
                );
            }
        } else if matches!(kind, 0x980 | 0x1e24) {
            let stage = e.stage.unwrap_or(0);
            if e.hp < self.content.objects[&kind].hp / 5 && stage < 3 {
                self.lab_stage(id, 3);
            } else if stage == 3 && elapsed >= 2000 {
                self.lab_stage(id, 4);
                self.entities.get_mut(&id).unwrap().expires.set(
                    Some(self.now + 50),
                    id,
                    &mut self.timed_ids,
                );
            } else if stage == 0 && elapsed >= 10000 {
                self.lab_stage(id, 1);
            } else if stage == 1 && elapsed >= 3000 {
                self.lab_stage(id, 2);
            }
        } else if kind == 0x983 {
            let maximum = self.content.objects[&kind].hp;
            let stage = [3, 2, 1]
                .into_iter()
                .filter(|quarter| i64::from(e.hp) * 4 < i64::from(maximum) * quarter)
                .count() as u64;
            let e = self.entities.get_mut(&id).unwrap();
            e.stage = Some(stage);
            e.size = [150, 150, 175, 200][stage as usize];
        } else if matches!(kind, 0x990 | 0x991) {
            // Keep each turret on its own beat: the shot due (cooldown less half an interval, or now before the first
            // shot and once the turret has idled past it) moves to the turret's next beat.
            let interval = lab::profile(kind).interval;
            let beat = lab::turret_beat(kind, e.home.y as i32, interval);
            let e = self.entities.get_mut(&id).unwrap();
            let due = if e.next_shot == 0 || e.next_shot + interval / 2 < self.now {
                self.now
            } else {
                e.next_shot.saturating_sub(interval / 2)
            };
            e.next_shot = due + (beat + interval - due % interval) % interval;
        } else if kind == lab::CREATION {
            let (stage, pos, home) = (e.stage, e.pos, e.home);
            let inside = |pool| {
                self.actors().any(|p| {
                    p.kind == pool
                        && (p.pos.x as i32 - pos.x as i32).abs() < 2
                        && (p.pos.y as i32 - pos.y as i32).abs() < 2
                })
            };
            match stage {
                // Coils AND Dr Terrible open the room: https://web.archive.org/web/20261025211723/https://wikiwiki.jp/rmd/Tesla%20Coil (2012-08-30). realm-src's coil-only Open Wall is marked "need fix".
                None if self.cleared && !self.actors().any(|e| e.kind == lab::TESLA) => {
                    let walls: Vec<_> = self
                        .entities
                        .values()
                        .filter(|e| e.kind == lab::EMERGENCY)
                        .map(|e| e.id)
                        .collect();
                    for &wall in &walls {
                        self.remove_entity(wall);
                    }
                    tracing::info!(walls = walls.len(), "Mad Lab secret room opened");
                    self.lab_stage(id, 1);
                    // t0f0_BWpBtw (2012-09-08) t=982-984.
                    self.lab_say(id, &["Me door is open. Come let me crush you!"]);
                    self.entities.get_mut(&id).unwrap().reaction_until = self.now + 10000;
                }
                // Attack/pool shapes: https://web.archive.org/web/20261025211809/https://wikiwiki.jp/rmd/Horrific%20Creation; cycle order and 10/16/1 s windows are reconstruction, not recovered AI or HP thresholds.
                Some(1 | 5) if inside(lab::HEXXER) => self.lab_stage(id, 2),
                Some(2) if elapsed >= 10000 && inside(lab::DEHEXXER) => self.lab_stage(id, 8),
                Some(1) if elapsed >= 10000 || target.is_none() => self.lab_stage(id, 3),
                Some(3 | 8) if pos.distance(home) < 0.5 => self.lab_stage(id, 4),
                Some(4) if elapsed >= 16000 => self.lab_stage(id, 5),
                Some(5) if elapsed >= 10000 || target.is_some_and(|p| pos.distance(p) < 1.5) => {
                    self.lab_stage(id, 6);
                }
                Some(6) if elapsed >= 1000 => self.lab_stage(id, 7),
                Some(7) if elapsed >= 1000 => self.lab_stage(id, 1),
                _ => {}
            }
            let current = self.entities[&id].stage;
            if current != stage {
                let pet = current == Some(2);
                let e = self.entities.get_mut(&id).unwrap();
                e.goal = None;
                if pet {
                    e.condition &= !condition::bit(condition::INVINCIBLE);
                } else {
                    e.condition |= condition::bit(condition::INVINCIBLE);
                }
                let texture = if pet {
                    self.rng.gen_range(2..=7)
                } else {
                    i32::from(matches!(current, Some(4 | 7)))
                };
                self.landmark_texture(id, texture);
            }
            if current.is_some()
                && target.is_some()
                && self.now >= self.entities[&id].reaction_until
            {
                self.lab_say(id, CREATION_TAUNTS);
                self.entities.get_mut(&id).unwrap().reaction_until = self.now + 10000;
            }
        }
        Ok(())
    }

    pub(super) fn lab_movement(&self, id: i32) -> (Motion, f32) {
        let e = &self.entities[&id];
        if e.kind == lab::BOSS && e.stage.is_some_and(|s| s % 2 == 1) {
            (
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 0.1,
                },
                nr_speed(1.),
            )
        } else if e.kind == 0x1e23 && e.parent.is_some() {
            // Experiments walk to Dr Terrible to heal him: t0f0_BWpBtw (2012-09-08) t=332.
            (
                Motion::Protect {
                    anchor: Anchor::Parent,
                    distance: 1.,
                },
                lab::profile(e.kind).speed,
            )
        } else if e.kind == lab::CREATION
            && e.stage == Some(2)
            && e.activated.is_some_and(|t| self.now >= t + 10000)
        {
            // The pet returns to a DeHexxer "after a while" (https://web.archive.org/web/20261025211809/https://wikiwiki.jp/rmd/Horrific%20Creation); 10 s is reconstruction.
            (
                Motion::Protect {
                    anchor: Anchor::Kind(lab::DEHEXXER),
                    distance: 0.1,
                },
                nr_speed(0.5),
            )
        } else if e.kind == lab::CREATION {
            match e.stage {
                None | Some(4 | 6 | 7) => (Motion::Stationary, 0.),
                Some(3 | 8) => (
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 0.1,
                    },
                    nr_speed(0.5),
                ),
                Some(5) => (
                    Motion::Chase {
                        sight: 11.25,
                        stop: 0.5,
                    },
                    nr_speed(1.),
                ),
                Some(1 | 2) => {
                    let p = lab::profile(e.kind);
                    (p.motion, p.speed)
                }
                Some(stage) => panic!("invalid Creation phase {stage}"),
            }
        } else {
            let p = lab::profile(e.kind);
            (p.motion, p.speed)
        }
    }

    pub(super) fn lab_shots(&self, id: i32, target: Pos) -> Vec<(u8, f32)> {
        let e = &self.entities[&id];
        let heading = (target.y - e.pos.y).atan2(target.x - e.pos.x);
        let fan = |index, count: usize, gap: f32, angle: f32| {
            (0..count)
                .map(|n| {
                    (
                        index,
                        angle + (n as f32 - (count - 1) as f32 / 2.) * gap.to_radians(),
                    )
                })
                .collect()
        };
        match e.kind {
            0x978 if e.stage == Some(1) => {
                fan(0, 6, 60., 22.5f32.to_radians() + 150f32.to_radians())
            }
            0x978 => vec![],
            0x993..=0x996 if e.stage != Some(1) => vec![],
            // 18 shots per burst: https://wikiwiki.jp/rmd/Red%20Gas%20Spawner (2012-10-05).
            0x993..=0x996 => fan(0, 18, 20., 22.5f32.to_radians() + 170f32.to_radians()),
            lab::CREATION => match e.stage {
                Some(1 | 8) => fan(1, 3, 20., heading),
                Some(4) => fan(6, 1, 0., heading),
                Some(6) => fan(5, 8, 45., heading),
                Some(7) => fan(4, 8, 45., heading),
                _ => vec![],
            },
            0x990 => vec![(0, 0.)],
            0x991 => vec![(0, std::f32::consts::PI)],
            0x979 | 0x1e25 | 0x12aa => fan(0, 2, 20., heading),
            0x980 | 0x1e24 if e.stage == Some(3) => vec![],
            0x980 | 0x1e24 if e.stage == Some(4) => {
                fan(2, 8, 45., 22.5f32.to_radians() + 157.5f32.to_radians())
            }
            0x980 | 0x1e24 => vec![(u8::from(e.stage.is_some_and(|s| s > 0)), heading)],
            // The comparator names nonexistent projectile 1; selected 123.5.1 defines only 0.
            0x981 | 0x1e23 => vec![(0, heading)],
            0x982 => {
                let mut shots = fan(0, 3, 20., heading);
                shots.extend(fan(1, 4, 20., heading));
                shots
            }
            0x983 => fan(
                e.stage.unwrap_or(0) as u8,
                if e.stage.unwrap_or(0) == 0 { 3 } else { 2 },
                20.,
                heading,
            ),
            _ => vec![],
        }
    }

    pub(super) fn lab_death(&mut self, defeated: &Entity) -> Result<()> {
        if defeated.kind == lab::BOSS {
            let children: Vec<_> = self
                .actors()
                .filter(|e| e.parent == Some(defeated.id))
                .map(|e| e.id)
                .collect();
            for child in children {
                let e = self.entities.get_mut(&child).unwrap();
                e.parent.set(None, child, &mut self.timed_ids);
                if (0x993..=0x996).contains(&e.kind) {
                    e.stage = Some(0);
                }
                if e.kind == lab::BUBBLE {
                    self.landmark_texture(child, 0);
                }
            }
            let cages: Vec<_> = self
                .entities
                .values()
                .filter(|e| e.kind == lab::CAGE)
                .map(|e| e.id)
                .collect();
            for cage in cages {
                self.entities
                    .get_mut(&cage)
                    .unwrap()
                    .parent
                    .set(None, cage, &mut self.timed_ids);
                self.landmark_texture(cage, 0);
            }
            // C4sKt3Se0T0 (2012-09-27) t=162.
            let creation = self
                .actors()
                .find(|e| e.kind == lab::CREATION)
                .map(|e| e.id);
            if let Some(creation) = creation {
                self.lab_say(creation, &["WHO KILL MASTER? RAHHHH!"]);
            }
        } else if defeated.kind == 0x982 {
            // 1-3 Mini Bots: https://web.archive.org/web/20261025211742/https://wikiwiki.jp/rmd/Mad%20Lab (body 2013-01-30).
            let count = self.rng.gen_range(1..=3);
            for n in 0..count {
                let angle = n as f32 * std::f32::consts::TAU / count as f32;
                let position = Pos {
                    x: defeated.pos.x + 0.2 * angle.cos(),
                    y: defeated.pos.y + 0.2 * angle.sin(),
                };
                self.spawn_entity(0x979, position, vec![])?;
            }
        }
        Ok(())
    }

    pub(super) fn lab_loot(&mut self, boss: &Entity) -> Result<()> {
        // Loots.cs compares threshold .32/.1 against integer damage, not a fraction of boss HP.
        let owners: BTreeSet<_> = self
            .players
            .values()
            .filter(|p| {
                p.character.alive
                    && !p.death_pending
                    && !p.departing
                    && boss.damage.get(&p.character.id).is_some_and(|n| *n > 0)
            })
            .map(|p| p.character.account)
            .collect();
        // The wiki guarantees at least one DEF; retain Lab's per-contributor allocation as reconstruction: https://wikiwiki.jp/rmd/::cmd/source?page=Horrific+Creation
        let potion = i32::from(
            crate::loot::participation_reward(boss.kind)
                .with_context(|| format!("lab loot requested for {:#x}", boss.kind))?,
        );
        for account in owners {
            let mut rewards: Vec<_> = crate::loot::rewards(&self.content, boss.kind, &mut self.rng)
                .into_iter()
                .map(i32::from)
                .collect();
            rewards.push(potion);
            self.loot_bags(boss.pos, Some(account), &rewards, boss.kind)?;
        }
        Ok(())
    }
}
