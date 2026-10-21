use super::*;
use crate::{
    condition,
    dungeon::davy::{self, BOSS, KEYS, LIT, UNLIT},
};

#[derive(Default)]
pub(super) struct Locker {
    pub keys: u8,
    boss: i32,
    lamps: BTreeSet<i32>,
    doors: Vec<(i32, usize)>,
    respawns: Vec<(u16, Pos, Option<i32>, u64)>,
    all_lit_at: Option<u64>,
    vulnerable_until: Option<u64>,
    lights: BTreeMap<i32, (u64, Option<u64>)>,
}

impl World {
    pub(super) fn prepare_locker(&mut self) -> Result<()> {
        let bosses: Vec<_> = self
            .entities
            .values()
            .filter(|e| e.kind == BOSS)
            .map(|e| e.id)
            .collect();
        ensure!(
            bosses.len() == 1,
            "Davy Locker requires exactly one captain"
        );
        let boss = bosses[0];
        let lamps = self
            .entities
            .values()
            .filter(|e| e.kind == UNLIT)
            .map(|e| e.id)
            .collect::<BTreeSet<_>>();
        ensure!(
            lamps.len() == 8,
            "Davy Locker requires eight unlit lanterns"
        );
        for lamp in &lamps {
            ensure!(
                self.entities[lamp].pos.distance(self.entities[&boss].home) < 11.,
                "Davy lantern outside captain's room"
            );
        }
        let doors = self
            .entities
            .values()
            .filter_map(|e| davy::door_color(e.kind).map(|c| (e.id, c)))
            .collect();
        let respawns = self
            .actors()
            .filter(|e| matches!(e.kind, 0xe31 | 0xe3a))
            .map(|e| (e.kind, e.home, Some(e.id), 0))
            .collect();
        self.entities.get_mut(&boss).unwrap().published = false;
        self.locker = Some(Locker {
            boss,
            lamps,
            doors,
            respawns,
            ..Locker::default()
        });
        Ok(())
    }
    pub(super) fn locker_key_ui(&self, player: i32) {
        let Some(locker) = &self.locker else {
            return;
        };
        let p = &self.players[&player];
        for text in std::iter::once("showKeyUI").chain(
            davy::COLORS
                .iter()
                .enumerate()
                .filter_map(|(color, name)| (locker.keys & (1 << color) != 0).then_some(*name)),
        ) {
            let mut out = Writer::new();
            out.i32(0).utf(text);
            send(p, 9, out.0);
        }
    }
    fn locker_notice(&self, text: &str) {
        let mut out = Writer::new();
        out.i32(0).utf(text);
        for p in self.players.values() {
            send(p, 9, out.0.clone());
        }
    }
    pub(super) fn light_locker_lantern(&mut self, id: i32) -> Result<()> {
        let locker = self.locker.as_ref().context("lantern has no Locker")?;
        ensure!(
            locker.lamps.contains(&id) && self.entities[&id].kind == UNLIT,
            "unowned or already lit lantern"
        );
        let boss = locker.boss;
        let pos = self.entities[&id].pos;
        self.remove_entity(id);
        let lit = self.spawn_entity(LIT, pos, vec![])?;
        let e = self.entities.get_mut(&lit).unwrap();
        e.activated = Some(self.now);
        let locker = self.locker.as_mut().unwrap();
        locker.lamps.remove(&id);
        locker.lamps.insert(lit);
        locker.lights.insert(lit, (self.now, None));
        tracing::info!(
            lantern = id,
            lit,
            boss,
            "Locker lantern lit by ordinary damage"
        );
        Ok(())
    }
    fn reset_locker_lantern(&mut self, locker: &mut Locker, id: i32) -> Result<()> {
        let pos = self.entities[&id].home;
        self.remove_entity(id);
        let unlit = self.spawn_entity(UNLIT, pos, vec![])?;
        locker.lamps.remove(&id);
        locker.lamps.insert(unlit);
        ensure!(
            locker.lights.remove(&id).is_some(),
            "reset lantern lacks its own clock"
        );
        Ok(())
    }
    pub(super) fn locker_step(&mut self) -> Result<()> {
        let Some(mut locker) = self.locker.take() else {
            return Ok(());
        };
        let result = self.step_locker(&mut locker);
        self.locker = Some(locker);
        result
    }
    fn step_locker(&mut self, locker: &mut Locker) -> Result<()> {
        let players: Vec<_> = self
            .players
            .values()
            .filter(|p| p.active() && !p.paused)
            .map(|p| p.pos)
            .collect();
        if !players.is_empty() {
            self.locker_respawn(locker, &players)?;
        }
        let souls: Vec<_> = self
            .actors()
            .filter(|e| {
                e.kind == 0xe48
                    && e.stage == Some(1)
                    && e.expires.is_some_and(|at| self.now + 150 >= at)
            })
            .map(|e| e.id)
            .collect();
        for id in souls {
            self.locker_mechanics(id, None, 0)?;
        }
        for (color, key) in KEYS.iter().enumerate() {
            if locker.keys & (1 << color) != 0 {
                continue;
            }
            let found = self
                .actors()
                .find(|e| e.kind == *key && players.iter().any(|p| p.distance(e.pos) < 1.))
                .map(|e| e.id);
            if let Some(id) = found {
                self.remove_entity(id);
                locker.keys |= 1 << color;
                self.locker_notice(davy::COLORS[color]);
                tracing::info!(
                    key = id,
                    color = davy::COLORS[color],
                    "shared Locker key discovered"
                );
            }
        }
        let mut opened = Vec::new();
        for (id, color) in &locker.doors {
            if locker.keys & (1 << color) != 0
                && players
                    .iter()
                    .any(|p| p.distance(self.entities[id].pos) < 1.6)
            {
                opened.push(*id);
            }
        }
        // Remove both halves together; the key authorizes this door, not every door in the maze.
        for id in opened {
            let Some(door) = self.entities.get(&id) else {
                continue;
            };
            let pos = door.pos;
            let color = davy::door_color(self.entities[&id].kind).unwrap();
            let pair: Vec<_> = locker
                .doors
                .iter()
                .filter(|(other, c)| *c == color && self.entities[other].pos.distance(pos) < 1.5)
                .map(|(id, _)| *id)
                .collect();
            for id in pair {
                self.remove_entity(id);
                locker.doors.retain(|(door, _)| *door != id);
                tracing::info!(
                    door = id,
                    color = davy::COLORS[color],
                    "Locker door touched and opened"
                );
            }
        }
        ensure!(
            locker.lamps.len() == 8 && locker.lamps.iter().all(|id| self.entities.contains_key(id)),
            "Locker lost an encounter-owned lantern"
        );
        if !self.entities.contains_key(&locker.boss) {
            return self.locker_lamps(locker);
        }
        let boss = locker.boss;
        let home = self.entities[&boss].home;
        if self.entities[&boss].stage.is_none() {
            // Doors govern ordinary access without forbidding legitimate teleport shortcuts.
            if players.iter().any(|p| p.distance(home) < 6.) {
                let e = self.entities.get_mut(&boss).unwrap();
                e.stage = Some(0);
                e.activated = Some(self.now);
                e.published = true;
                e.size = 0;
                e.extra_stats = vec![(61, 4)];
                tracing::info!(boss, "Davy Jones appearing after room approach");
            } else {
                return self.locker_lamps(locker);
            }
        }
        let age = self.now
            - self.entities[&boss]
                .activated
                .context("Davy clock missing")?;
        if self.entities[&boss].stage == Some(0) {
            self.entities.get_mut(&boss).unwrap().size = (((age / 125) as i32) * 25).min(100);
            if age < 500 {
                return self.locker_lamps(locker);
            }
            self.entities.get_mut(&boss).unwrap().stage = Some(1);
            self.entities.get_mut(&boss).unwrap().extra_stats = vec![(61, 3)];
            tracing::info!(boss, "Davy Jones lantern cycle started");
        }
        let all_lit = locker.lamps.iter().all(|id| self.entities[id].kind == LIT);
        if self.entities[&boss].stage == Some(1) {
            if all_lit {
                let since = *locker.all_lit_at.get_or_insert(self.now);
                if self.now >= since + 1000 {
                    locker.vulnerable_until = Some(self.now + 5000);
                    let e = self.entities.get_mut(&boss).unwrap();
                    e.condition &= !condition::bit(condition::INVULNERABLE);
                    e.stage = Some(2);
                    e.extra_stats = vec![(61, 5)];
                    tracing::info!(
                        boss,
                        until = self.now + 5000,
                        "all eight lanterns opened Davy damage window"
                    );
                }
            } else {
                locker.all_lit_at = None;
            }
        }
        if locker
            .vulnerable_until
            .is_some_and(|until| self.now >= until)
        {
            locker.vulnerable_until = None;
            locker.all_lit_at = None;
            let e = self.entities.get_mut(&boss).unwrap();
            e.condition |= condition::bit(condition::INVULNERABLE);
            e.stage = Some(3);
            e.extra_stats = vec![(61, 4)];
            tracing::info!(
                boss,
                "Davy shield restored while awaiting lantern deactivation"
            );
        }
        self.locker_lamps(locker)?;
        if self.entities[&boss].stage == Some(3) && locker.lights.is_empty() {
            let e = self.entities.get_mut(&boss).unwrap();
            e.stage = Some(1);
            e.extra_stats = vec![(61, 3)];
            tracing::info!(boss, "Davy lantern cycle restarted");
        }
        if !players.is_empty()
            && self.entities[&boss].stage == Some(1)
            && self.now >= self.entities[&boss].next_aux
        {
            self.spawn_children(boss, 0xe50, 4, 5.)?;
            self.entities.get_mut(&boss).unwrap().next_aux = self.now
                + if self.entities[&boss].hp < self.content.objects[&BOSS].hp / 2 {
                    3000
                } else {
                    6000
                };
        }
        Ok(())
    }
    // FSoD@6fd20aa BehaviorDb.DavyJones.cs gives each lantern its own clock, not a captain-phase timer.
    fn locker_lamps(&mut self, locker: &mut Locker) -> Result<()> {
        // One snapshot makes simultaneous bursts independent of entity iteration order.
        let all_lit = locker.lamps.iter().all(|id| self.entities[id].kind == LIT);
        for (id, (lit_at, burst_at)) in locker.lights.clone() {
            if burst_at.is_some_and(|at| self.now >= at + 1000) {
                self.reset_locker_lantern(locker, id)?;
            } else if burst_at.is_none() && self.now >= lit_at + 5000 && all_lit {
                locker.lights.get_mut(&id).unwrap().1 = Some(self.now);
                let e = self
                    .entities
                    .get_mut(&id)
                    .context("lit lantern disappeared")?;
                e.stage = Some(2);
                e.activated = Some(self.now);
                if let Some(boss) = self.entities.get_mut(&locker.boss) {
                    boss.reaction_until = self.now + 3000;
                }
                tracing::info!(lantern = id, "Locker lantern paralyze burst armed");
            } else if burst_at.is_none() && self.now >= lit_at + 15000 {
                self.reset_locker_lantern(locker, id)?;
            }
        }
        Ok(())
    }
    fn locker_respawn(&mut self, locker: &mut Locker, players: &[Pos]) -> Result<()> {
        for (kind, pos, entity, at) in &mut locker.respawns {
            if entity.is_some_and(|id| !self.entities.contains_key(&id)) {
                *entity = None;
                *at = self.now + 5000;
            }
            if entity.is_none() && self.now >= *at && players.iter().any(|p| p.distance(*pos) < 12.)
            {
                ensure!(
                    self.actors()
                        .filter(|e| davy::ROSTER.contains(&e.kind))
                        .count()
                        < 256,
                    "Locker population budget exhausted"
                );
                let id = self.spawn_entity(*kind, *pos, vec![])?;
                *entity = Some(id);
                tracing::debug!(entity = id, kind, "bounded Locker ghost respawn");
            }
        }
        Ok(())
    }
    pub(super) fn locker_mechanics(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<()> {
        let kind = self.entities[&id].kind;
        if matches!(kind, 0xe31 | 0xe3a | 0xe50) {
            let e = self.entities.get_mut(&id).unwrap();
            let growth = if kind == 0xe3a { 35 } else { 30 };
            e.size = (growth + (elapsed / 50) as i32 * growth).min(120);
            if kind == 0xe50 && e.size == 120 {
                e.condition &= !condition::bit(condition::INVULNERABLE);
            }
        }
        if kind == 0xe43 {
            let e = self.entities.get_mut(&id).unwrap();
            e.condition &= !condition::bit(condition::ARMORED);
            if elapsed % 5700 >= 2850 {
                e.condition |= condition::bit(condition::ARMORED);
            }
        }
        if kind == 0xe45 && self.now >= self.entities[&id].next_aux {
            self.spawn_children(id, 0xe48, 4, 3.)?;
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 5000;
        }
        if kind == 0xe48 {
            let e = &self.entities[&id];
            let stage = e.stage.unwrap_or(0);
            let next = match stage {
                0 if target.is_some_and(|p| p.distance(e.pos) < 4.) => Some(1),
                1 if e.expires.is_some_and(|at| self.now + 150 >= at) => Some(2),
                _ => None,
            };
            if let Some(next) = next {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(next);
                e.activated = Some(self.now);
                let lifetime = if next == 1 { 2350 } else { 150 };
                e.expires
                    .set(Some(self.now + lifetime), id, &mut self.timed_ids);
            }
        }
        Ok(())
    }
    pub(super) fn locker_movement(&self, id: i32, elapsed: u64) -> (crate::encounter::Motion, f32) {
        use crate::encounter::{Anchor, Motion, nr_speed};
        let e = &self.entities[&id];
        match e.kind {
            BOSS if e.stage == Some(1) && self.now < e.reaction_until => (
                Motion::Chase {
                    sight: 12.,
                    stop: 1.,
                },
                nr_speed(0.4),
            ),
            BOSS if e.stage != Some(1) => (
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 0.,
                },
                nr_speed(0.1),
            ),
            0xe31 if elapsed % 4700 >= 2850 => (Motion::Retreat { distance: 3. }, nr_speed(0.2)),
            0xe42 if elapsed % 4700 >= 2850 => (Motion::Circle, nr_speed(0.2)),
            0xe48 if e.stage == Some(1) => (
                Motion::Charge {
                    sight: 8.,
                    cooldown: 2000,
                    delay: 0,
                    stop: 0.,
                    idle: 0.,
                },
                nr_speed(0.5),
            ),
            0xe48 if e.stage == Some(2) => (Motion::Stationary, 0.),
            kind => {
                let p = crate::dungeon::davy::profile(kind).expect("validated Locker actor");
                (p.motion, p.speed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn locker() -> (World, i32) {
        let content = Arc::new(Content::load(Path::new("data")).unwrap());
        let map = Arc::new(davy::generate(&content, 12351).unwrap());
        let mut w = World::new(content, map).unwrap();
        w.name = davy::NAME.into();
        w.encounter = true;
        w.prepare_locker().unwrap();
        let p = w.enter(super::super::tests::ch(&w), None).unwrap();
        (w, p)
    }
    fn unlock(w: &mut World, p: i32) {
        for key in KEYS {
            let pos = w.entities.values().find(|e| e.kind == key).unwrap().pos;
            w.players.get_mut(&p).unwrap().pos = pos;
            w.locker_step().unwrap();
        }
        let boss = w.locker.as_ref().unwrap().boss;
        w.players.get_mut(&p).unwrap().pos = w.entities[&boss].home;
        w.locker_step().unwrap();
        w.now += 500;
        w.locker_step().unwrap();
    }
    fn light_all(w: &mut World, p: i32) {
        let lamps = w
            .locker
            .as_ref()
            .unwrap()
            .lamps
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for id in lamps {
            assert!(w.damage_enemy(p, id, 10000, true, &[], None).unwrap() > 0);
        }
        w.locker_step().unwrap();
        w.now += 1000;
        w.locker_step().unwrap();
    }
    #[test]
    fn paralyzed_players_take_priority_only_inside_the_captains_sight() {
        let (mut w, p) = locker();
        unlock(&mut w, p);
        let boss = w.locker.as_ref().unwrap().boss;
        let home = w.entities[&boss].home;
        let near = Pos {
            x: home.x + 3.,
            y: home.y,
        };
        w.players.get_mut(&p).unwrap().pos = near;
        let mut guest = super::super::tests::ch(&w);
        guest.id = 2;
        guest.account = 2;
        let second = w.enter(guest, None).unwrap();
        w.players.get_mut(&second).unwrap().pos = Pos {
            x: home.x + 30.,
            y: home.y,
        };
        w.apply_condition(second, condition::PARALYZED, 2000)
            .unwrap();
        w.entities.get_mut(&boss).unwrap().reaction_until = w.now + 3000;
        w.npc_step();
        assert!(
            w.entities[&boss]
                .goal
                .expect("nearby target was ignored")
                .distance(near)
                < 0.00001
        );
        let paralyzed = Pos {
            x: home.x + 5.,
            y: home.y,
        };
        w.players.get_mut(&second).unwrap().pos = paralyzed;
        w.now += 50;
        w.npc_step();
        assert!(
            w.entities[&boss]
                .goal
                .expect("paralyzed target was ignored")
                .distance(paralyzed)
                < 0.00001
        );
    }
    #[test]
    fn room_approach_not_key_possession_reveals_the_captain_and_early_lamps_keep_time() {
        let (mut w, p) = locker();
        let boss = w.locker.as_ref().unwrap().boss;
        let lamp = *w.locker.as_ref().unwrap().lamps.first().unwrap();
        assert!(w.damage_enemy(p, lamp, 10000, true, &[], None).unwrap() > 0);
        assert_eq!(w.locker.as_ref().unwrap().lights.len(), 1);
        w.now += 15000;
        w.locker_step().unwrap();
        assert!(w.locker.as_ref().unwrap().lights.is_empty());
        assert!(!w.entities[&boss].published);
        let doors = w.locker.as_ref().unwrap().doors.len();
        w.players.get_mut(&p).unwrap().pos = w.entities[&boss].home;
        w.locker_step().unwrap();
        assert!(w.entities[&boss].published);
        assert_eq!(w.locker.as_ref().unwrap().keys, 0);
        assert_eq!(w.locker.as_ref().unwrap().doors.len(), doors);
        assert_eq!(w.entities[&boss].stage, Some(0));
    }
    #[test]
    fn lantern_volley_requires_ack_and_applies_its_actual_paralyze_once() {
        let (mut w, p) = locker();
        unlock(&mut w, p);
        light_all(&mut w, p);
        w.now += 4000;
        w.locker_step().unwrap();
        let lit = *w.locker.as_ref().unwrap().lights.keys().next().unwrap();
        let at = w.entities[&lit].pos;
        let (packets, mut rx) = mpsc::channel(4096);
        let (lost, _loss) = watch::channel(false);
        let player = w.players.get_mut(&p).unwrap();
        player.tx = Some(Output { packets, lost });
        player.controller = Controller::Human;
        player.known.insert(lit);
        player.pos = Pos {
            x: at.x + 1.,
            y: at.y,
        };
        w.now += 100;
        let packets = w.npc_step();
        for (owner, body) in packets.into_iter().filter(|(id, _)| *id == lit) {
            w.send_enemy_shot(owner, body);
        }
        let shots: Vec<_> = w.bullets.iter().filter(|b| b.owner == lit).collect();
        assert_eq!(shots.len(), 6);
        assert!((shots[0].angle - (-150f32).to_radians()).abs() < 0.00001);
        assert!((shots[5].angle - 150f32.to_radians()).abs() < 0.00001);
        let bullet = shots[0].id;
        assert_eq!(rx.try_recv().unwrap().id, 19);
        let hp = w.players[&p].character.hp;
        w.native_player_hit(p, lit, bullet).unwrap();
        assert_eq!(w.players[&p].character.hp, hp);
        w.acknowledge_shoot(p, 100).unwrap();
        w.native_player_hit(p, lit, bullet).unwrap();
        assert_eq!(w.players[&p].character.hp, hp - 85);
        assert_ne!(
            w.players[&p].conditions(w.now) & condition::bit(condition::PARALYZED),
            0
        );
        w.native_player_hit(p, lit, bullet).unwrap();
        assert_eq!(w.players[&p].character.hp, hp - 85);
        w.now += 2000;
        assert_eq!(
            w.players[&p].conditions(w.now) & condition::bit(condition::PARALYZED),
            0
        );
    }
    #[test]
    fn soul_charge_locks_the_acquired_position_instead_of_homing() {
        let (mut w, p) = locker();
        let target = w.players[&p].pos;
        let soul = w
            .spawn_entity(
                0xe48,
                Pos {
                    x: target.x - 3.,
                    y: target.y - 1.,
                },
                vec![],
            )
            .unwrap();
        w.locker_mechanics(soul, Some(target), 0).unwrap();
        w.now += 50;
        assert!(w.npc_step().iter().all(|(id, _)| *id != soul));
        assert!(w.entities[&soul].goal.unwrap().distance(target) < 0.0001);
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: target.x,
            y: target.y + 2.,
        };
        w.now += 50;
        assert!(w.npc_step().iter().all(|(id, _)| *id != soul));
        assert!(w.entities[&soul].goal.unwrap().distance(target) < 0.0001);
        assert_eq!(w.entities[&soul].stage, Some(1));
    }
    #[test]
    fn charging_soul_finishes_without_audience_and_its_real_explosion_fires_once() {
        let (mut w, p) = locker();
        let at = w.players[&p].pos;
        let soul = w.spawn_entity(0xe48, at, vec![]).unwrap();
        w.locker_mechanics(soul, Some(at), 0).unwrap();
        assert_eq!(w.entities[&soul].stage, Some(1));
        w.players.get_mut(&p).unwrap().paused = true;
        w.suspend_npc(soul, 2000);
        w.now += 2200;
        w.locker_step().unwrap();
        assert_eq!(w.entities[&soul].stage, Some(2));
        w.players.get_mut(&p).unwrap().paused = false;
        let packets = w.npc_step();
        assert_eq!(packets.iter().filter(|(id, _)| *id == soul).count(), 1);
        let blast: Vec<_> = w.bullets.iter().filter(|b| b.owner == soul).collect();
        assert_eq!(blast.len(), 6);
        assert!((blast[0].angle - (-150f32).to_radians()).abs() < 0.00001);
        assert!(w.npc_step().iter().all(|(id, _)| *id != soul));
        let kills = w.players[&p].character.kills;
        w.now += 150;
        w.mechanic_expiry();
        assert!(!w.entities.contains_key(&soul));
        assert_eq!(w.players[&p].character.kills, kills);
    }
    #[test]
    fn keys_are_shared_touch_authority_and_replayed_to_late_joiners() {
        let (mut w, p) = locker();
        let boss = w.locker.as_ref().unwrap().boss;
        assert!(!w.entities[&boss].published);
        assert_ne!(w.quest_target(p), Some(boss));
        let green = w.entities.values().find(|e| e.kind == KEYS[1]).unwrap().pos;
        w.players.get_mut(&p).unwrap().pos = green;
        w.locker_step().unwrap();
        assert_eq!(w.locker.as_ref().unwrap().keys, 2);
        let purple = w.entities.values().find(|e| e.kind == KEYS[0]).unwrap().pos;
        w.players.get_mut(&p).unwrap().pos = purple;
        w.locker_step().unwrap();
        assert_eq!(w.locker.as_ref().unwrap().keys, 3);
        let doors = w.locker.as_ref().unwrap().doors.len();
        let door = w
            .locker
            .as_ref()
            .unwrap()
            .doors
            .iter()
            .find(|(_, c)| *c == 0)
            .unwrap()
            .0;
        let at = w.entities[&door].pos;
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: at.x - 1.,
            y: at.y,
        };
        w.locker_step().unwrap();
        assert!(!w.entities.contains_key(&door));
        assert_eq!(w.locker.as_ref().unwrap().doors.len(), doors - 2);
        let (tx, mut rx) = mpsc::channel(128);
        let mut ch = super::super::tests::ch(&w);
        ch.id = 2;
        ch.account = 2;
        w.enter(
            ch,
            Some(Output {
                packets: tx,
                lost: watch::channel(false).0,
            }),
        )
        .unwrap();
        let mut messages = vec![];
        while let Ok(packet) = rx.try_recv() {
            if packet.id == 9 {
                let mut r = crate::wire::Reader::new(&packet.body);
                assert_eq!(r.i32().unwrap(), 0);
                messages.push(r.utf().unwrap());
            }
        }
        assert_eq!(messages, ["showKeyUI", "purple", "green"]);
        let (mut other, q) = locker();
        let key = other
            .entities
            .values()
            .find(|e| e.kind == KEYS[0])
            .unwrap()
            .id;
        let pos = other.entities[&key].pos;
        other.remove_entity(key);
        other.players.get_mut(&q).unwrap().pos = pos;
        other.locker_step().unwrap();
        assert_eq!(other.locker.as_ref().unwrap().keys, 0);
    }
    #[test]
    fn lantern_clocks_are_independent_of_npc_sleep_and_the_earned_window() {
        let (mut w, p) = locker();
        unlock(&mut w, p);
        let boss = w.locker.as_ref().unwrap().boss;
        let first = *w.locker.as_ref().unwrap().lamps.first().unwrap();
        w.damage_enemy(p, first, 10000, true, &[], None).unwrap();
        let lit = *w.locker.as_ref().unwrap().lights.keys().next().unwrap();
        w.suspend_npc(lit, 10000);
        w.now += 15000;
        w.locker_step().unwrap();
        assert!(w.locker.as_ref().unwrap().lights.is_empty());
        let first = *w.locker.as_ref().unwrap().lamps.first().unwrap();
        w.damage_enemy(p, first, 10000, true, &[], None).unwrap();
        w.now += 4000;
        let remaining = w
            .locker
            .as_ref()
            .unwrap()
            .lamps
            .iter()
            .copied()
            .filter(|id| w.entities[id].kind == UNLIT)
            .collect::<Vec<_>>();
        for id in remaining {
            w.damage_enemy(p, id, 10000, true, &[], None).unwrap();
        }
        w.locker_step().unwrap();
        w.now += 1000;
        w.locker_step().unwrap();
        assert_eq!(w.entities[&boss].stage, Some(2));
        assert_eq!(
            w.locker
                .as_ref()
                .unwrap()
                .lights
                .values()
                .filter(|(_, burst)| burst.is_some())
                .count(),
            1
        );
        w.now += 1000;
        w.locker_step().unwrap();
        assert_eq!(w.locker.as_ref().unwrap().lights.len(), 7);
        assert_eq!(w.entities[&boss].stage, Some(2));
        w.leave(p).unwrap();
        w.now += 4000;
        w.locker_step().unwrap();
        assert_eq!(w.entities[&boss].stage, Some(3));
        assert_ne!(
            w.entities[&boss].condition & condition::bit(condition::INVULNERABLE),
            0
        );
    }
    #[test]
    fn renewable_ghost_slots_are_bounded_and_unsupported_xml_is_not_activated() {
        let (mut w, p) = locker();
        let ghost = w.entities.values().find(|e| e.kind == 0xe31).unwrap().id;
        let at = w.entities[&ghost].home;
        let population = w.entities.values().filter(|e| e.kind == 0xe31).count();
        w.players.get_mut(&p).unwrap().pos = at;
        for _ in 0..5 {
            let ghost = w
                .entities
                .values()
                .find(|e| e.kind == 0xe31 && e.home.distance(at) < 0.01)
                .unwrap()
                .id;
            w.damage_enemy(p, ghost, 10000, true, &[], None).unwrap();
            w.locker_step().unwrap();
            assert_eq!(
                w.entities.values().filter(|e| e.kind == 0xe31).count(),
                population - 1
            );
            w.now += 4999;
            w.locker_step().unwrap();
            assert_eq!(
                w.entities.values().filter(|e| e.kind == 0xe31).count(),
                population - 1
            );
            w.now += 1;
            w.locker_step().unwrap();
            assert_eq!(
                w.entities.values().filter(|e| e.kind == 0xe31).count(),
                population
            );
        }
        assert!(!w.entities.values().any(|e| matches!(e.kind, 0xe46 | 0xe47)));
        for kind in davy::ROSTER {
            crate::encounter::validate(&w.content, kind).unwrap();
            crate::loot::validate(&w.content, kind).unwrap();
        }
        let roger = w.spawn_entity(0xe45, at, vec![]).unwrap();
        for _ in 0..3 {
            w.locker_mechanics(roger, Some(at), 0).unwrap();
            w.now += 5000;
        }
        assert_eq!(
            w.actors()
                .filter(|e| e.parent == Some(roger) && e.kind == 0xe48)
                .count(),
            4
        );
        assert_eq!(
            davy::initial_condition(0xe48) & condition::bit(condition::INVINCIBLE),
            condition::bit(condition::INVINCIBLE)
        );
    }
    #[test]
    fn ghost_ship_lake_spawns_an_owned_hero_and_preserves_the_reference_water() {
        let content = super::super::tests::world().content;
        let map = Map {
            width: 128,
            height: 128,
            cells: vec![
                crate::content::Cell {
                    ground: 0x61,
                    object: None,
                    config: String::new(),
                    terrain: 1,
                    region: 0
                };
                128 * 128
            ],
            spawns: vec![(1.5, 1.5)],
        };
        let mut w = World::new(content, Arc::new(map)).unwrap();
        w.name = "Realm".into();
        w.encounter = true;
        w.populate_ghost_ship().unwrap();
        let ship = w.entities.values().find(|e| e.kind == 0xe37).unwrap().id;
        let anchor = w.entities.values().find(|e| e.kind == 0xe38).unwrap().id;
        let home = w.entities[&ship].home;
        assert_eq!(w.entities[&ship].quest, Some(ship));
        assert_eq!(w.entities[&anchor].quest, Some(ship));
        assert!(w.hero_identities().contains(&ship));
        assert_eq!(w.map.cell(home.x, home.y).unwrap().ground, 0xfa);
        assert_eq!(w.map.cell(home.x + 15., home.y).unwrap().ground, 0xed);
        assert_eq!(w.map.cell(home.x + 18., home.y).unwrap().ground, 0x61);
        assert!(w.walkable(home));
        assert!(w.populate_ghost_ship().is_err());
        let p = w.enter(super::super::tests::ch(&w), None).unwrap();
        w.players.get_mut(&p).unwrap().pos = home;
        assert_eq!(
            w.damage_enemy(p, ship, 100000, true, &[], None).unwrap(),
            50000
        );
        assert!(w.critical_deaths.contains(&ship));
    }
    #[test]
    fn ghost_ship_roll_is_once_only_earned_and_delayed_at_its_owned_anchor() {
        let mut earned = 0;
        for seed in 0..16 {
            let mut w = super::super::tests::arena(0);
            w.name = "Realm".into();
            w.rng = rand::rngs::StdRng::seed_from_u64(seed);
            let p = w.enter(super::super::tests::ch(&w), None).unwrap();
            let pos = Pos { x: 4.5, y: 4.5 };
            let ship = w.spawn_entity(0xe37, pos, vec![]).unwrap();
            let anchor = w.spawn_entity(0xe38, pos, vec![]).unwrap();
            w.landmark_bind(ship).unwrap();
            w.landmark_bind(anchor).unwrap();
            w.damage_enemy(p, ship, 100000, true, &[], None).unwrap();
            assert!(w.portal_requests.is_empty());
            let rolled = w.entities[&anchor].stage == Some(2);
            w.leave(p).unwrap();
            for _ in 0..19 {
                w.step();
            }
            assert!(w.portal_requests.is_empty());
            w.step();
            assert_eq!(w.portal_requests.len(), usize::from(rolled));
            if rolled {
                earned += 1;
                assert_eq!(w.portal_requests[0].0, 0x741);
                assert!(w.walkable(w.portal_requests[0].1));
                w.ghost_ship_mechanics(anchor, Some(pos), 1000).unwrap();
                assert_eq!(w.portal_requests.len(), 1);
            }
            for _ in 0..9 {
                w.step();
            }
            assert_eq!(w.entities.values().filter(|e| e.kind == 0x18e).count(), 0);
            w.step();
            let palms: Vec<_> = w.entities.values().filter(|e| e.kind == 0x18e).collect();
            assert_eq!(palms.len(), if rolled { 2 } else { 0 });
            assert!(
                palms
                    .iter()
                    .all(|e| e.pos.y == pos.y && (e.pos.x - pos.x).abs() == 1.)
            );
            assert!(!w.entities.contains_key(&anchor));
            assert_eq!(w.portal_requests.len(), usize::from(rolled));
        }
        assert!(earned > 0 && earned < 16);
        let mut w = super::super::tests::arena(0);
        w.name = "Realm".into();
        let pos = Pos { x: 4.5, y: 4.5 };
        let ship = w.spawn_entity(0xe37, pos, vec![]).unwrap();
        let anchor = w.spawn_entity(0xe38, pos, vec![]).unwrap();
        w.landmark_bind(ship).unwrap();
        w.landmark_bind(anchor).unwrap();
        w.remove_entity(ship);
        w.ghost_ship_mechanics(anchor, Some(pos), 0).unwrap();
        assert!(w.portal_requests.is_empty());
    }
    #[test]
    fn earned_lantern_cycle_has_no_kill_credit_and_clear_waits_for_commit() {
        let (mut w, p) = locker();
        unlock(&mut w, p);
        let boss = w.locker.as_ref().unwrap().boss;
        let hp = w.entities[&boss].hp;
        assert_eq!(w.damage_enemy(p, boss, 1000, true, &[], None).unwrap(), 0);
        let kills = w.players[&p].character.kills;
        let stray = w
            .spawn_entity(UNLIT, w.entities[&boss].home, vec![])
            .unwrap();
        light_all(&mut w, p);
        assert_eq!(w.entities[&stray].kind, UNLIT);
        assert_eq!(w.players[&p].character.kills, kills);
        assert_eq!(w.players[&p].experience_queued, 0);
        assert_eq!(w.entities[&boss].stage, Some(2));
        assert!(w.damage_enemy(p, boss, 100, true, &[], None).unwrap() > 0);
        assert_eq!(w.entities[&boss].hp, hp - 100);
        w.now += 5000;
        w.locker_step().unwrap();
        assert_eq!(w.entities[&boss].stage, Some(3));
        assert_eq!(w.damage_enemy(p, boss, 100, true, &[], None).unwrap(), 0);
        let burst = crate::encounter::attacks::davy_schedule(LIT, 2, 100).snapshot(0.);
        assert_eq!(burst.len(), 6);
        assert_eq!(
            w.content.objects[&LIT].projectiles[&0].effects,
            [(condition::PARALYZED, 2000)]
        );
        w.now += 1000;
        w.locker_step().unwrap();
        assert!(
            w.locker
                .as_ref()
                .unwrap()
                .lamps
                .iter()
                .all(|id| w.entities[id].kind == UNLIT)
        );
        light_all(&mut w, p);
        w.damage_enemy(p, boss, 100000, true, &[], None).unwrap();
        assert!(w.cleared);
        w.mechanic_expiry();
        assert!(
            w.locker
                .as_ref()
                .unwrap()
                .lamps
                .iter()
                .all(|id| w.entities[id].kind == LIT)
        );
        assert!(!w.actors().any(|e| e.kind == 0xe50));
        let exit = w.clear_exit.unwrap();
        assert_eq!(w.entities[&exit].kind, 0x704);
        assert!(!w.entities[&exit].published);
        let plans = w.pending_progress_where(|_, _| true);
        assert_eq!(plans.len(), 1);
        let (id, snapshot) = plans.into_iter().next().unwrap();
        assert_eq!(snapshot.davy_lockers, 1);
        assert_eq!(snapshot.jungles, 0);
        assert_eq!(snapshot.manors, 0);
        w.finish_progress(id, snapshot).unwrap();
        assert!(w.entities[&exit].published);
        assert_eq!(w.players[&p].character.davy_lockers, 1);
        assert!(w.pending_progress_where(|_, _| true).is_empty());
        let mut legacy = serde_json::to_value(&w.players[&p].character).unwrap();
        legacy.as_object_mut().unwrap().remove("davy_lockers");
        assert_eq!(
            serde_json::from_value::<Character>(legacy)
                .unwrap()
                .davy_lockers,
            0
        );
    }
}
