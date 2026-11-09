use super::*;
use crate::{
    condition,
    dungeon::davy::{self, BOSS, KEYS, LIT, UNLIT},
};

/// Davy stops within 0.7 s of the eighth lantern lighting: https://www.youtube.com/watch?v=PGEViiWb5H4&t=18 (2012-11-22).
const LIT_HOLD_MS: u64 = 500;
/// The window ends after "a certain amount of damage" (https://www.youtube.com/watch?v=3BJGr4JLXu0&t=273, 2012-08-13). The
/// burst followed ~1.4-2.1k of his 9000 HP: https://www.youtube.com/watch?v=PGEViiWb5H4&t=37,
/// https://www.youtube.com/watch?v=JV80D2whb1Y&t=215 (2012-08-04). 20% is the midpoint estimate.
const WINDOW_DAMAGE_PERCENT: i32 = 20;
/// Cap for a window nobody damages; the 5 s timer is a reconstruction, not a measurement.
const WINDOW_CAP_MS: u64 = 5000;
/// Safety cap on the chase; one ran ~100 s while the player only kited: https://www.youtube.com/watch?v=nzZgHBdlEH8&t=666
/// (2012-08-03).
const CHASE_CAP_MS: u64 = 100_000;
/// He follows and chases into the top of the south neck but never past ~13 tiles from spawn:
/// https://www.youtube.com/watch?v=nzZgHBdlEH8&t=169 (2012-08-03).
const LEASH: f32 = 13.;
/// He picks up players 10.9-13 tiles away (measured lower bound); 14 is an estimate.
pub(super) const SIGHT: f32 = 14.;
/// Real tiles/s = 5.55 * rate + 0.74 (`nr_speed` already carries the engine's 1.5). Shielded follow 1.9 (1.5-2.4) t/s:
/// https://www.youtube.com/watch?v=JV80D2whb1Y&t=134; the existing 0.2 (1.85 t/s) sits in that range.
const FOLLOW: f32 = 0.2;
/// Second-form chase ~6.0 t/s (5.7-6.6): https://www.youtube.com/watch?v=nzZgHBdlEH8&t=666,
/// https://www.youtube.com/watch?v=3BJGr4JLXu0&t=331.
const CHASE: f32 = 0.95;
/// Walks straight back to spawn at ~3.3 (2.5-3.8) t/s: https://www.youtube.com/watch?v=JV80D2whb1Y&t=244.
const RETURN: f32 = 0.46;

#[derive(Default)]
pub(super) struct Locker {
    pub keys: u8,
    boss: i32,
    /// Live lantern id -> its authored tile, so a lost lantern can be put back.
    lamps: BTreeMap<i32, Pos>,
    doors: Vec<(i32, usize)>,
    respawns: Vec<(u16, Pos, Option<i32>, u64)>,
    all_lit_at: Option<u64>,
    /// When Davy began to appear, on the Locker's own clock: his NPC clock pauses while nobody is near.
    appeared_at: Option<u64>,
    vulnerable_until: Option<u64>,
    /// Davy's HP when the window opened; it closes once he has lost `WINDOW_DAMAGE_PERCENT` of his maximum.
    window_hp: Option<i32>,
    /// Called off his chase by a lantern hit, Davy walks back to spawn before following again.
    returning: bool,
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
            .map(|e| (e.id, e.pos))
            .collect::<BTreeMap<_, _>>();
        ensure!(
            lamps.len() == 8,
            "Davy Locker requires eight unlit lanterns"
        );
        for pos in lamps.values() {
            ensure!(
                pos.distance(self.entities[&boss].home) < 11.,
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
            locker.lamps.contains_key(&id) && self.entities[&id].kind == UNLIT,
            "unowned or already lit lantern"
        );
        let boss = locker.boss;
        let pos = locker.lamps[&id];
        self.remove_entity(id);
        let lit = self.spawn_entity(LIT, pos, vec![])?;
        let e = self.entities.get_mut(&lit).unwrap();
        e.activated = Some(self.now);
        let locker = self.locker.as_mut().unwrap();
        locker.lamps.remove(&id);
        locker.lamps.insert(lit, pos);
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
        let pos = locker.lamps[&id];
        self.remove_entity(id);
        let unlit = self.spawn_entity(UNLIT, pos, vec![])?;
        locker.lamps.remove(&id);
        locker.lamps.insert(unlit, pos);
        ensure!(
            locker.lights.remove(&id).is_some(),
            "reset lantern lacks its own clock"
        );
        Ok(())
    }
    /// Puts back, unlit, any lantern that left the world outside its own cycle. Refusing every later step instead
    /// would leave Davy shielded for the rest of the run.
    fn restore_locker_lanterns(&mut self, locker: &mut Locker) -> Result<()> {
        let lost: Vec<_> = locker
            .lamps
            .iter()
            .filter(|(id, _)| !self.entities.contains_key(id))
            .map(|(id, pos)| (*id, *pos))
            .collect();
        for (id, pos) in lost {
            locker.lamps.remove(&id);
            locker.lights.remove(&id);
            let unlit = self.insert_entity(UNLIT, pos, vec![])?;
            locker.lamps.insert(unlit, pos);
            tracing::warn!(lantern = id, unlit, "lost Locker lantern restored unlit");
        }
        Ok(())
    }
    /// A hit on any lantern calls Davy off his chase; he walks home, as when the player relights one:
    /// https://www.youtube.com/watch?v=PGEViiWb5H4&t=58 (2012-11-22), https://www.youtube.com/watch?v=JV80D2whb1Y&t=156.
    pub(super) fn locker_lantern_hit(&mut self) {
        let Some(locker) = self.locker.as_mut() else {
            return;
        };
        if let Some(boss) = self.entities.get_mut(&locker.boss)
            && boss.reaction_until > self.now
        {
            boss.reaction_until = self.now;
            locker.returning = true;
            tracing::info!(
                boss = locker.boss,
                "Davy Jones called off his chase by a lantern hit"
            );
        }
    }
    /// Arms one lit lantern's paralyze burst; the first burst of a wave sends Davy on his chase.
    fn burst_locker_lantern(&mut self, locker: &mut Locker, id: i32) -> Result<()> {
        locker
            .lights
            .get_mut(&id)
            .context("bursting lantern lacks its own clock")?
            .1 = Some(self.now);
        let e = self
            .entities
            .get_mut(&id)
            .context("lit lantern disappeared")?;
        e.stage = Some(2);
        e.activated = Some(self.now);
        locker.returning = false;
        if let Some(boss) = self.entities.get_mut(&locker.boss) {
            let chase = boss.reaction_until <= self.now;
            boss.reaction_until = self.now + CHASE_CAP_MS;
            // 2012 footage shows this bubble as the lanterns fire; once per wave is a reconstruction.
            if chase {
                let packet =
                    chat::text_packet("#Davy Jones", locker.boss, 5, "Avast, ye filthy mongrel!");
                for p in self
                    .players
                    .values()
                    .filter(|p| p.known.contains(&locker.boss))
                {
                    send(p, packet.id, packet.body.clone());
                }
                tracing::info!(boss = locker.boss, "Davy Jones taunted at lantern burst");
            }
        }
        tracing::info!(lantern = id, "Locker lantern paralyze burst armed");
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
        self.restore_locker_lanterns(locker)?;
        if !self.entities.contains_key(&locker.boss) {
            return self.locker_lamps(locker);
        }
        let boss = locker.boss;
        let home = self.entities[&boss].home;
        if self.entities[&boss].stage.is_none() {
            // Doors govern ordinary access without forbidding legitimate teleport shortcuts. His lanterns stand up
            // to 7 tiles out and can be lit from beyond his 6-tile approach, so a lit lantern also calls him.
            let lit = !locker.lights.is_empty();
            if lit || players.iter().any(|p| p.distance(home) < 6.) {
                let e = self.entities.get_mut(&boss).unwrap();
                e.stage = Some(0);
                e.activated = Some(self.now);
                locker.appeared_at = Some(self.now);
                e.published = true;
                e.size = 0;
                e.extra_stats = vec![(61, 4)];
                tracing::info!(boss, lit, "Davy Jones appearing after room approach");
            } else {
                return self.locker_lamps(locker);
            }
        }
        let age = self.now
            - locker
                .appeared_at
                .context("Davy appearance clock missing")?;
        if self.entities[&boss].stage == Some(0) {
            self.entities.get_mut(&boss).unwrap().size = (((age / 125) as i32) * 25).min(100);
            if age < 500 {
                return self.locker_lamps(locker);
            }
            self.entities.get_mut(&boss).unwrap().stage = Some(1);
            tracing::info!(boss, "Davy Jones lantern cycle started");
        }
        let all_lit = locker.lamps.keys().all(|id| self.entities[id].kind == LIT);
        if self.entities[&boss].stage == Some(1) {
            if all_lit {
                let since = *locker.all_lit_at.get_or_insert(self.now);
                if self.now >= since + LIT_HOLD_MS {
                    locker.vulnerable_until = Some(self.now + WINDOW_CAP_MS);
                    let e = self.entities.get_mut(&boss).unwrap();
                    locker.window_hp = Some(e.hp);
                    e.condition &= !condition::bit(condition::INVULNERABLE);
                    e.stage = Some(2);
                    e.extra_stats = vec![(61, 5)];
                    tracing::info!(
                        boss,
                        hp = e.hp,
                        until = self.now + WINDOW_CAP_MS,
                        "all eight lanterns opened Davy damage window"
                    );
                }
            } else {
                locker.all_lit_at = None;
            }
        }
        let hurt = locker.window_hp.is_some_and(|hp| {
            hp - self.entities[&boss].hp
                >= self.content.objects[&BOSS].hp * WINDOW_DAMAGE_PERCENT / 100
        });
        if hurt
            || locker
                .vulnerable_until
                .is_some_and(|until| self.now >= until)
        {
            locker.vulnerable_until = None;
            locker.window_hp = None;
            locker.all_lit_at = None;
            let e = self.entities.get_mut(&boss).unwrap();
            e.condition |= condition::bit(condition::INVULNERABLE);
            e.stage = Some(3);
            e.extra_stats = vec![(61, 4)];
            tracing::info!(
                boss,
                hurt,
                "Davy shield restored while awaiting lantern deactivation"
            );
            // "when he is invincible again, the lanterns will go out" (https://www.youtube.com/watch?v=3BJGr4JLXu0&t=273,
            // 2012-08-13): all eight burst together as the window ends (https://www.youtube.com/watch?v=PGEViiWb5H4&t=20).
            let burning: Vec<_> = locker
                .lights
                .iter()
                .filter(|(_, (_, burst))| burst.is_none())
                .map(|(id, _)| *id)
                .collect();
            for id in burning {
                self.burst_locker_lantern(locker, id)?;
            }
        }
        self.locker_lamps(locker)?;
        // Only the closing burst holds him: "You must relight the lanterns to attack him again" (realmeye.com/wiki/
        // davy-jones-locker/4, 123.4.1), so lanterns lit since stay lit toward the next window.
        if self.entities[&boss].stage == Some(3)
            && locker.lights.values().all(|(_, burst)| burst.is_none())
        {
            self.entities.get_mut(&boss).unwrap().stage = Some(1);
            tracing::info!(boss, "Davy lantern cycle restarted");
        }
        if self.entities[&boss].stage == Some(1) {
            let e = self.entities.get_mut(&boss).unwrap();
            if locker.returning && e.pos.distance(e.home) < 0.5 {
                locker.returning = false;
            }
            // The chase is drawn dark with a black mist trail (AltTexture 3); following, he keeps his red coat:
            // https://www.youtube.com/watch?v=JV80D2whb1Y&t=146, https://www.youtube.com/watch?v=3BJGr4JLXu0&t=297.
            let form = vec![(61, if self.now < e.reaction_until { 3 } else { 1 })];
            if e.extra_stats != form {
                e.extra_stats = form;
            }
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
    /// Lit lanterns stay lit until the set is complete, however long that takes, and go out only with the burst that
    /// ends the window: lanterns lit 22-28 s earlier were still lit at https://www.youtube.com/watch?v=JV80D2whb1Y&t=210
    /// (2012-08-04), and every lantern goes dark together at the burst.
    fn locker_lamps(&mut self, locker: &mut Locker) -> Result<()> {
        for (id, (_, burst_at)) in locker.lights.clone() {
            if burst_at.is_some_and(|at| self.now >= at + 1000) {
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
        let home = |speed| {
            (
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 0.,
                },
                nr_speed(speed),
            )
        };
        match e.kind {
            // Hidden, appearing, vulnerable and bursting he stands where he is; the window never sends him home:
            // https://www.youtube.com/watch?v=JV80D2whb1Y&t=138 (2012-08-04).
            BOSS if e.stage != Some(1) => (Motion::Stationary, 0.),
            BOSS if self.locker.as_ref().is_some_and(|l| l.returning) => home(RETURN),
            BOSS => {
                // Shielded he follows the nearest player; after a burst his second form dashes after them:
                // https://www.youtube.com/watch?v=PGEViiWb5H4&t=12 (2012-11-22), https://www.youtube.com/watch?v=JV80D2whb1Y&t=142.
                let (stop, speed) = if self.now < e.reaction_until {
                    (1., CHASE)
                } else {
                    // He seldom came within 3 tiles while following (estimate).
                    (3., FOLLOW)
                };
                let seen = self.players.values().any(|p| {
                    p.active()
                        && !p.paused
                        && p.conditions(self.now) & condition::bit(condition::INVISIBLE) == 0
                        && p.pos.distance(e.pos) < SIGHT
                });
                if !seen {
                    home(RETURN)
                } else if e.pos.distance(e.home) > LEASH {
                    (
                        Motion::Protect {
                            anchor: Anchor::Home,
                            distance: LEASH - 1.,
                        },
                        nr_speed(speed),
                    )
                } else {
                    (Motion::Chase { sight: SIGHT, stop }, nr_speed(speed))
                }
            }
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

    const SEED: u64 = 12351;

    fn locker(seed: u64) -> (World, i32) {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let content = Arc::new(Content::load(&data).unwrap());
        let map = Arc::new(davy::generate(&content, seed).unwrap());
        let mut w = World::new(content, map).unwrap();
        w.name = davy::NAME.into();
        w.encounter = true;
        w.prepare_locker().unwrap();
        let p = join(&mut w, 1);
        (w, p)
    }
    fn join(w: &mut World, id: i32) -> i32 {
        let character: Character = serde_json::from_value(serde_json::json!({
            "schema":1,"lifetime":crate::store::FameLifetime::complete(),
            "class":0x300,"base":[200,100,15,0,75,15,15,10],
            "id":id,"account":id,"name":format!("Diver{id}"),"level":20,"exp":0,
            "fame":0,"hp":200,"mp":100,"revision":0,"alive":true,
            "items":[0xa14,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
        }))
        .unwrap();
        w.enter(character, None).unwrap()
    }
    fn boss(w: &World) -> i32 {
        w.locker.as_ref().unwrap().boss
    }
    fn stage(w: &World) -> Option<u64> {
        w.entities.get(&boss(w)).and_then(|e| e.stage)
    }
    fn unlit(w: &World) -> Vec<i32> {
        let lamps = &w.locker.as_ref().unwrap().lamps;
        lamps
            .keys()
            .copied()
            .filter(|id| w.entities.get(id).is_some_and(|e| e.kind == UNLIT))
            .collect()
    }
    fn light(w: &mut World, p: i32, lamps: &[i32]) {
        for &id in lamps {
            w.damage_enemy(p, id, 10000, true, &[], None).unwrap();
        }
    }
    /// Relights every lantern that is out, as players do.
    fn relight(w: &mut World, p: i32) {
        let lamps = unlit(w);
        light(w, p, &lamps);
    }
    /// One 50 ms server tick of the Locker, checking what every tick must keep.
    fn tick(w: &mut World) {
        w.now += 50;
        w.locker_step().expect("Locker step refused");
        let lamps = &w.locker.as_ref().unwrap().lamps;
        assert_eq!(lamps.len(), 8);
        assert!(lamps.keys().all(|id| {
            w.entities
                .get(id)
                .is_some_and(|e| matches!(e.kind, UNLIT | LIT))
        }));
        let lanterns = w
            .entities
            .values()
            .filter(|e| matches!(e.kind, UNLIT | LIT));
        assert_eq!(lanterns.count(), 8);
    }
    /// Ticks until `done` holds or `ms` pass; how long it took, if it held.
    fn until(w: &mut World, ms: u64, mut done: impl FnMut(&mut World) -> bool) -> Option<u64> {
        let start = w.now;
        while w.now < start + ms {
            if done(w) {
                return Some(w.now - start);
            }
            tick(w);
        }
        done(w).then(|| w.now - start)
    }
    /// Keeps every lantern lit until Davy's damage window opens; how long that took, if it did.
    fn window(w: &mut World, p: i32, ms: u64) -> Option<u64> {
        until(w, ms, |w| {
            relight(w, p);
            stage(w) == Some(2)
        })
    }
    fn reveal(w: &mut World, p: i32) {
        let home = w.entities[&boss(w)].home;
        w.players.get_mut(&p).unwrap().pos = home;
        assert!(until(w, 1000, |w| stage(w) == Some(1)).is_some());
    }
    /// One lantern lit, the other seven 4 s later: the window opens with the first lantern's clock nearly spent.
    fn staggered_window(w: &mut World, p: i32) {
        reveal(w, p);
        let first = unlit(w)[0];
        light(w, p, &[first]);
        until(w, 4000, |_| false);
        relight(w, p);
        assert!(until(w, 1500, |w| stage(w) == Some(2)).is_some());
        assert!(until(w, 6000, |w| stage(w) != Some(2)).is_some());
    }

    #[test]
    fn lanterns_relit_after_a_staggered_window_open_the_next_one() {
        let (mut w, p) = locker(SEED);
        staggered_window(&mut w, p);
        let reopened = window(&mut w, p, 3000);
        assert!(
            reopened.is_some(),
            "relit lanterns left Davy shielded in stage {:?}",
            stage(&w)
        );
    }

    #[test]
    fn the_window_closing_puts_out_every_lantern() {
        let (mut w, p) = locker(SEED);
        staggered_window(&mut w, p);
        let out = until(&mut w, 1100, |w| unlit(w).len() == 8);
        assert!(
            out.is_some(),
            "{} lanterns outlived the window",
            8 - unlit(&w).len()
        );
        assert_eq!(stage(&w), Some(1));
    }

    #[test]
    fn lanterns_lit_from_beyond_his_reach_still_raise_davy() {
        let (mut w, p) = locker(SEED);
        let home = w.entities[&boss(&w)].home;
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: home.x + 9.,
            y: home.y,
        };
        relight(&mut w, p);
        let window = until(&mut w, 3000, |w| stage(w) == Some(2));
        assert!(
            window.is_some(),
            "Davy stayed {:?} while all eight lanterns burned",
            stage(&w)
        );
    }

    #[test]
    fn a_lantern_lost_to_another_system_comes_back_unlit() {
        let (mut w, p) = locker(SEED);
        reveal(&mut w, p);
        let lamps = unlit(&w);
        light(&mut w, p, &lamps[..3]);
        let lost = *w.locker.as_ref().unwrap().lights.keys().next().unwrap();
        let at = w.entities[&lost].pos;
        w.remove_entity(lost);
        tick(&mut w);
        let lights = &w.locker.as_ref().unwrap().lights;
        assert!(!lights.contains_key(&lost) && lights.len() == 2);
        assert!(
            w.entities
                .values()
                .any(|e| e.kind == UNLIT && e.pos.distance(at) < 0.01)
        );
        assert!(window(&mut w, p, 3000).is_some());
    }

    #[test]
    fn a_slow_set_stays_lit_and_the_window_follows_the_eighth() {
        let (mut w, p) = locker(SEED);
        reveal(&mut w, p);
        // Seven lanterns stay lit for as long as the eighth takes; nothing goes out and no window opens.
        let lamps = unlit(&w);
        light(&mut w, p, &lamps[..7]);
        assert!(until(&mut w, 40_000, |w| stage(w) != Some(1)).is_none());
        assert_eq!(unlit(&w).len(), 1);
        // The eighth: Davy stops and turns vulnerable half a second later, with every lantern still lit.
        relight(&mut w, p);
        let open = until(&mut w, 1500, |w| stage(w) == Some(2));
        assert!(open.is_some_and(|ms| (LIT_HOLD_MS..=LIT_HOLD_MS + 50).contains(&ms)));
        assert!(unlit(&w).is_empty());
        // Unhurt, he shields again at the cap; all eight burst together and go out a second later.
        assert_eq!(
            until(&mut w, 5100, |w| stage(w) != Some(2)),
            Some(WINDOW_CAP_MS)
        );
        assert_eq!(stage(&w), Some(3));
        let lights = &w.locker.as_ref().unwrap().lights;
        assert!(lights.len() == 8 && lights.values().all(|(_, burst)| burst.is_some()));
        assert_eq!(until(&mut w, 1100, |w| stage(w) == Some(1)), Some(1000));
        assert_eq!(unlit(&w).len(), 8);
    }

    #[test]
    fn the_window_closes_once_davy_has_taken_a_fifth_of_his_health() {
        let (mut w, p) = locker(SEED);
        reveal(&mut w, p);
        assert!(window(&mut w, p, 1500).is_some());
        let davy = boss(&w);
        let fifth = w.content.objects[&BOSS].hp / 5;
        w.damage_enemy(p, davy, fifth - 1, true, &[], None).unwrap();
        tick(&mut w);
        assert_eq!(stage(&w), Some(2));
        w.damage_enemy(p, davy, 1, true, &[], None).unwrap();
        tick(&mut w);
        assert_eq!(stage(&w), Some(3));
        let e = &w.entities[&davy];
        assert_ne!(e.condition & condition::bit(condition::INVULNERABLE), 0);
        assert!(
            unlit(&w).is_empty()
                && w.locker
                    .as_ref()
                    .unwrap()
                    .lights
                    .values()
                    .all(|l| l.1.is_some())
        );
    }

    #[test]
    fn davy_dying_inside_the_window_leaves_the_lanterns_working() {
        let (mut w, p) = locker(SEED);
        reveal(&mut w, p);
        assert!(window(&mut w, p, 1500).is_some());
        let davy = boss(&w);
        w.damage_enemy(p, davy, 100_000, true, &[], None).unwrap();
        assert!(!w.entities.contains_key(&davy));
        until(&mut w, 20_000, |w| {
            relight(w, p);
            false
        });
        // After the kill the lanterns stay lit: https://www.youtube.com/watch?v=PGEViiWb5H4&t=81 (2012-11-22).
        until(&mut w, 16_000, |_| false);
        assert!(unlit(&w).is_empty());
    }

    #[test]
    fn the_cycle_survives_players_dying_or_leaving_and_others_finishing_it() {
        let (mut w, p) = locker(SEED);
        let q = join(&mut w, 2);
        reveal(&mut w, p);
        let lamps = unlit(&w);
        light(&mut w, p, &lamps[..4]);
        light(&mut w, q, &lamps[4..]);
        assert!(until(&mut w, 1500, |w| stage(w) == Some(2)).is_some());
        w.players.get_mut(&q).unwrap().character.alive = false;
        w.leave(p).unwrap();
        // With nobody left standing, Davy shields again and every lantern goes out.
        assert!(
            until(&mut w, 20_000, |w| stage(w) == Some(1)
                && unlit(w).len() == 8)
            .is_some()
        );
        let r = join(&mut w, 3);
        let home = w.entities[&boss(&w)].home;
        w.players.get_mut(&r).unwrap().pos = home;
        assert!(window(&mut w, r, 3000).is_some());
    }

    #[test]
    fn whole_fights_never_stall_or_lose_a_lantern() {
        for seed in 0..8 {
            let (mut w, p) = locker(seed);
            let home = w.entities[&boss(&w)].home;
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            for round in 0..8 {
                // Light from inside or outside his reach, in any order and at any pace, sometimes not at all.
                let reach = rng.gen_range(0.0..10.0);
                w.players.get_mut(&p).unwrap().pos = Pos {
                    x: home.x + reach,
                    y: home.y,
                };
                let pace = rng.gen_range(0.0..0.2);
                until(&mut w, rng.gen_range(0..20_000), |w| {
                    let lamps = unlit(w);
                    if !lamps.is_empty() && rng.gen_bool(pace) {
                        light(w, p, &[lamps[rng.gen_range(0..lamps.len())]]);
                    }
                    false
                });
                // Relit as fast as they go out, the lanterns must open a window.
                assert!(
                    window(&mut w, p, 3000).is_some(),
                    "seed {seed} round {round}: Davy held in stage {:?}",
                    stage(&w)
                );
            }
        }
    }

    #[test]
    fn a_solo_player_lighting_one_lantern_at_a_time_opens_a_window() {
        // Raw damage per 100 ms against the lanterns' 2000 HP and 15 DEF: about 250, 450 and 850 DPS.
        for raw in [40, 60, 100] {
            let (mut w, p) = locker(SEED);
            reveal(&mut w, p);
            let open = until(&mut w, 120_000, |w| {
                if w.now % 100 == 0
                    && let Some(&lamp) = unlit(w).first()
                {
                    w.damage_enemy(p, lamp, raw, false, &[], None).unwrap();
                }
                stage(w) == Some(2)
            });
            assert!(
                open.is_some(),
                "{raw} per 100 ms: Davy still shielded with {} lanterns lit",
                8 - unlit(&w).len()
            );
        }
    }

    /// How far Davy moves over `ms` of Locker and NPC steps.
    fn travel(w: &mut World, ms: u64) -> f32 {
        let from = w.entities[&boss(w)].pos;
        for _ in 0..ms / 50 {
            tick(w);
            w.npc_step();
        }
        w.entities[&boss(w)].pos.distance(from)
    }

    #[test]
    fn davy_follows_shielded_stands_vulnerable_dashes_after_the_burst_and_walks_home_on_a_lantern_hit()
     {
        let (mut w, p) = locker(SEED);
        reveal(&mut w, p);
        let davy = boss(&w);
        let home = w.entities[&davy].home;
        let south = |d: f32| Pos {
            x: home.x,
            y: home.y + d,
        };
        let form = |w: &World| w.entities[&davy].extra_stats.clone();
        // Shielded, he follows the player at ~1.9 tiles/s in his red coat.
        w.players.get_mut(&p).unwrap().pos = south(9.);
        let follow = travel(&mut w, 1000);
        assert!((1.4..=1.95).contains(&follow), "follow {follow}");
        assert_eq!(form(&w), vec![(61, 1)]);
        // Vulnerable, he stands where he is.
        assert!(window(&mut w, p, 1500).is_some());
        assert!(travel(&mut w, 1000) < 0.01);
        // After the burst his second form dashes at ~6 tiles/s.
        assert!(until(&mut w, 6000, |w| stage(w) == Some(1)).is_some());
        w.position_entity(davy, home).unwrap();
        w.players.get_mut(&p).unwrap().pos = south(11.);
        let dash = travel(&mut w, 1000);
        assert!((5.0..=6.1).contains(&dash), "dash {dash}");
        assert_eq!(form(&w), vec![(61, 3)]);
        // A hit on a lantern calls him off; he walks home at ~3.3 tiles/s.
        let lamp = unlit(&w)[0];
        w.damage_enemy(p, lamp, 10, false, &[], None).unwrap();
        let before = w.entities[&davy].pos.distance(home);
        let back = travel(&mut w, 1000);
        assert!((2.8..=3.4).contains(&back), "return {back}");
        assert!(w.entities[&davy].pos.distance(home) < before - 2.5);
        // Chasing into the south neck, he goes no further than 13 tiles from spawn.
        w.entities.get_mut(&davy).unwrap().reaction_until = w.now + CHASE_CAP_MS;
        w.locker.as_mut().unwrap().returning = false;
        w.position_entity(davy, south(11.)).unwrap();
        w.players.get_mut(&p).unwrap().pos = south(17.);
        for _ in 0..60 {
            travel(&mut w, 50);
            assert!(w.entities[&davy].pos.distance(home) <= LEASH + 0.5);
        }
    }

    /// A walkable tile within `spread` of `at`, if one turns up.
    fn spot(w: &World, rng: &mut rand::rngs::StdRng, at: Pos, spread: f32) -> Option<Pos> {
        (0..64)
            .map(|_| Pos {
                x: (at.x + rng.gen_range(-spread..=spread)).floor() + 0.5,
                y: (at.y + rng.gen_range(-spread..=spread)).floor() + 0.5,
            })
            .find(|p| w.walkable(*p))
    }

    #[test]
    fn whole_fights_with_a_moving_party_never_leave_davy_stuck_shielded() {
        for seed in 0..6 {
            let (mut w, first) = locker(seed);
            let home = w.entities[&boss(&w)].home;
            let (pair, _, step) = neck(&w, 3, home);
            let gate = w.entities[&pair[0]].pos;
            open(&mut w, first, 3, &pair, step);
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mut party = vec![first, join(&mut w, 2), join(&mut w, 3)];
            let mut next = 4;
            let (mut frozen, mut last) = (0, w.entities[&boss(&w)].pos);
            let mut complete_since: Option<u64> = None;
            for n in 0..2400 {
                // Every 2 s one player moves: into the octagon, out past his gate, off down the halls, or away.
                if n % 40 == 0 {
                    let k = rng.gen_range(0..party.len());
                    let to = match rng.gen_range(0..4) {
                        0 => spot(&w, &mut rng, home, 9.),
                        1 => spot(
                            &w,
                            &mut rng,
                            Pos {
                                x: gate.x + 5. * step.0,
                                y: gate.y + 5. * step.1,
                            },
                            4.,
                        ),
                        2 => spot(&w, &mut rng, home, 30.),
                        _ => {
                            w.leave(party[k]).unwrap();
                            party[k] = join(&mut w, next);
                            next += 1;
                            None
                        }
                    };
                    if let Some(to) = to {
                        w.players.get_mut(&party[k]).unwrap().pos = to;
                    }
                }
                // Lanterns are lit at random moments in every stage, by whoever stands within weapon range of one.
                if rng.gen_bool(0.1) {
                    let lamps = unlit(&w);
                    if !lamps.is_empty() {
                        let lamp = lamps[rng.gen_range(0..lamps.len())];
                        let at = w.entities[&lamp].pos;
                        if let Some(&p) =
                            party.iter().find(|p| w.players[*p].pos.distance(at) < 10.)
                        {
                            light(&mut w, p, &[lamp]);
                        }
                    }
                }
                tick(&mut w);
                w.npc_step();
                let now = w.now;
                let locker = w.locker.as_ref().unwrap();
                let davy = &w.entities[&boss(&w)];
                // A complete set of lit lanterns always becomes a window: shielded with all eight lit only through his
                // appearance and the hold.
                let complete = locker.lamps.keys().all(|id| w.entities[id].kind == LIT)
                    && locker.lights.values().all(|(_, burst)| burst.is_none());
                complete_since =
                    (complete && davy.stage != Some(2)).then(|| complete_since.unwrap_or(now));
                assert!(
                    complete_since.is_none_or(|since| now - since <= LIT_HOLD_MS + 700),
                    "seed {seed} t={now}: all eight lit and Davy still shielded in stage {:?}",
                    davy.stage
                );
                // Shielded and not chasing, he keeps after a player in sight unless already within his 3-tile stop.
                let shielded = davy.published
                    && davy.conditions(now) & condition::bit(condition::INVULNERABLE) != 0;
                let nearest = w
                    .players
                    .values()
                    .filter(|p| p.active() && !p.paused)
                    .map(|p| p.pos.distance(davy.pos))
                    .fold(f32::MAX, f32::min);
                let following = davy.stage == Some(1) && now >= davy.reaction_until;
                frozen = if shielded
                    && following
                    && davy.pos.distance(last) < 0.01
                    && (3.5..SIGHT).contains(&nearest)
                {
                    frozen + 50
                } else {
                    0
                };
                assert!(
                    frozen <= 5000,
                    "seed {seed} t={now}: Davy shielded and still in stage {:?} at {:?}, {:.1} tiles from home",
                    davy.stage,
                    (davy.pos.x, davy.pos.y),
                    davy.pos.distance(home)
                );
                assert!(
                    davy.pos.distance(home) <= LEASH + 0.5,
                    "seed {seed} t={now}: Davy {:.1} tiles from home",
                    davy.pos.distance(home)
                );
                last = davy.pos;
                // What each client is told: inside his octagon, Davy and all eight lanterns stay in view.
                let lamps: Vec<_> = w.locker.as_ref().unwrap().lamps.keys().copied().collect();
                let (b, published) = (boss(&w), w.entities[&boss(&w)].published);
                for &p in &party {
                    w.updates(p, false).unwrap();
                    let player = &w.players[&p];
                    if player.pos.distance(home) < 9. {
                        let missing: Vec<_> = lamps
                            .iter()
                            .chain(published.then_some(&b))
                            .filter(|id| !player.known.contains(id))
                            .collect();
                        assert!(
                            missing.is_empty(),
                            "seed {seed} t={now}: player at {:?} lost sight of {missing:?}",
                            (player.pos.x, player.pos.y)
                        );
                    }
                }
            }
        }
    }

    const PRISM: u16 = 0xae3;

    fn trickster(w: &mut World) -> i32 {
        let mut ch = Character::fresh(w.content.get(0x324).unwrap()).unwrap();
        (ch.id, ch.account) = (7, 7);
        ch.items[1] = PRISM.into();
        w.enter(ch, None).unwrap()
    }
    /// The closed `color` door pair nearest `inside`, a hall tile three steps out from it, and that outward step.
    fn neck(w: &World, color: usize, inside: Pos) -> (Vec<i32>, Pos, (f32, f32)) {
        let doors: Vec<_> = w.locker.as_ref().unwrap().doors.to_vec();
        let near = |id: &i32| w.entities[id].pos.distance(inside);
        let first = doors
            .iter()
            .filter(|(_, c)| *c == color)
            .map(|(id, _)| *id)
            .min_by(|a, b| near(a).total_cmp(&near(b)))
            .unwrap();
        let at = w.entities[&first].pos;
        let pair: Vec<_> = doors
            .iter()
            .filter(|(id, c)| *c == color && w.entities[id].pos.distance(at) < 1.5)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(pair.len(), 2);
        // Side-by-side halves sit in a horizontal wall, so the hall lies above or below; stacked halves face sideways.
        let flat = w.entities[&pair[0]].pos.y == w.entities[&pair[1]].pos.y;
        let step = if flat {
            (0., (at.y - inside.y).signum())
        } else {
            ((at.x - inside.x).signum(), 0.)
        };
        let outside = Pos {
            x: at.x + 3. * step.0,
            y: at.y + 3. * step.1,
        };
        (pair, outside, step)
    }
    /// Gives the keys up to `color` and walks the player into the door pair.
    fn open(w: &mut World, p: i32, color: usize, pair: &[i32], step: (f32, f32)) {
        w.locker.as_mut().unwrap().keys |= (2u8 << color) - 1;
        let at = w.entities[&pair[0]].pos;
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: at.x + step.0,
            y: at.y + step.1,
        };
        w.locker_step().unwrap();
        assert!(pair.iter().all(|id| !w.entities.contains_key(id)));
    }
    fn seen(w: &World, p: i32, at: Pos) -> bool {
        w.players[&p].tiles.contains(&(at.x as i16, at.y as i16))
    }

    #[test]
    fn a_locked_key_room_stays_dark_and_closed_to_blinks_until_its_door_opens() {
        use super::super::sight::tests::{blink, refused};
        let (mut w, _) = locker(SEED);
        let key = w.entities.values().find(|e| e.kind == KEYS[1]).unwrap();
        let (key, inside) = (key.id, key.pos);
        let (pair, outside, step) = neck(&w, 0, inside);
        let p = trickster(&mut w);
        w.players.get_mut(&p).unwrap().pos = outside;
        w.updates(p, true).unwrap();
        assert!(!seen(&w, p, inside) && !w.players[&p].known.contains(&key));
        assert!(pair.iter().all(|id| w.players[&p].known.contains(id)));
        refused(&mut w, p, PRISM, inside);
        // A blink along the hall, in sight, still lands and costs the prism's 60 MP.
        let sight = w.room_sight((outside.x as i32, outside.y as i32));
        let along = [3., -3., 4., -4., 5., -5., 2., -2.].map(|k| Pos {
            x: outside.x + k * step.1.abs(),
            y: outside.y + k * step.0.abs(),
        });
        let hall = *along
            .iter()
            .find(|t| w.walkable(**t) && sight.contains(&(t.x as i16, t.y as i16)))
            .unwrap();
        blink(&mut w, p, PRISM, hall).unwrap();
        let caster = &w.players[&p];
        assert_eq!(
            (caster.pos.x, caster.pos.y, caster.character.mp),
            (hall.x, hall.y, 192)
        );
        open(&mut w, p, 0, &pair, step);
        w.updates(p, false).unwrap();
        assert!(seen(&w, p, inside) && w.players[&p].known.contains(&key));
        blink(&mut w, p, PRISM, inside).unwrap();
    }

    #[test]
    fn the_boss_room_stays_dark_and_closed_to_blinks_until_the_yellow_door_opens() {
        use super::super::sight::tests::{blink, refused};
        let (mut w, _) = locker(SEED);
        let home = w.entities[&boss(&w)].home;
        let (pair, outside, step) = neck(&w, 3, home);
        let lamps: Vec<_> = w.locker.as_ref().unwrap().lamps.keys().copied().collect();
        let p = trickster(&mut w);
        w.players.get_mut(&p).unwrap().pos = outside;
        w.updates(p, true).unwrap();
        for id in &lamps {
            assert!(!seen(&w, p, w.entities[id].pos) && !w.players[&p].known.contains(id));
        }
        let near = |id: &i32| w.entities[id].pos.distance(outside);
        let lamp = *lamps
            .iter()
            .min_by(|a, b| near(a).total_cmp(&near(b)))
            .unwrap();
        let target = w.entities[&lamp].pos;
        assert!(target.distance(outside) <= 20.);
        refused(&mut w, p, PRISM, target);
        open(&mut w, p, 3, &pair, step);
        w.updates(p, false).unwrap();
        assert!(seen(&w, p, target) && w.players[&p].known.contains(&lamp));
        blink(&mut w, p, PRISM, target).unwrap();
    }

    #[test]
    fn davy_taunts_once_per_lantern_burst_wave() {
        let (mut w, player) = super::super::movement_tests::fixture(0x7e, false);
        let (packets, mut inbox) = mpsc::channel(64);
        let (lost, _) = watch::channel(None);
        let p = w.players.get_mut(&player).unwrap();
        p.tx = Some(Output {
            trace: None,
            packets,
            lost,
        });
        let pos = p.pos;
        let boss = w.spawn_entity(BOSS, pos, vec![]).unwrap();
        let lamps: Vec<_> = (0..8)
            .map(|k| {
                w.spawn_entity(
                    UNLIT,
                    Pos {
                        x: pos.x + k as f32,
                        y: pos.y + 2.,
                    },
                    vec![],
                )
            })
            .collect::<Result<_>>()
            .unwrap();
        w.prepare_locker().unwrap();
        w.players.get_mut(&player).unwrap().known.insert(boss);
        for id in lamps {
            w.light_locker_lantern(id).unwrap();
        }
        // Appearance, hold, an unhurt window to its cap, and the burst that ends it.
        for (advance, taunts) in [(8000, 1), (500, 0)] {
            for _ in 0..advance / 50 {
                w.now += 50;
                w.locker_step().unwrap();
            }
            let said = std::iter::from_fn(|| inbox.try_recv().ok())
                .filter(|p| {
                    p.id == wire::TEXT && String::from_utf8_lossy(&p.body).contains("Avast")
                })
                .count();
            assert_eq!(said, taunts);
        }
    }
}
