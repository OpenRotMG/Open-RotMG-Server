use super::*;
use crate::condition;
use crate::dungeon::snake::{BOSS, BUTTON, DART, GUARD, PET, SPAWNER, SWARM, phase, shield};

impl World {
    pub(super) fn snake_mechanics(&mut self, id: i32, elapsed: u64) -> Result<()> {
        if self.entities[&id].kind != BOSS {
            return self.snake_treasure(id);
        }
        let (stage, age) = phase(elapsed);
        let changed = self.entities[&id].stage != Some(stage);
        let e = self.entities.get_mut(&id).unwrap();
        e.condition &= !condition::bit(condition::INVULNERABLE);
        if shield(elapsed) {
            e.condition |= condition::bit(condition::INVULNERABLE);
        }
        if changed {
            e.stage = Some(stage);
            e.next_aux = self.now;
            tracing::info!(
                entity = id,
                kind = BOSS,
                stage,
                hp = e.hp,
                "Stheno timed phase entered"
            );
        }
        self.spawn_children(id, PET, 4, 10.)?;
        // Orbit's angle must follow actual position and the motor's speed: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/behaviors/Orbit.cs#L68-L86
        for pet_id in &self.actor_ids {
            let pet = &self.entities[pet_id];
            if pet.kind == PET && pet.parent == Some(id) && pet.orbit.is_none() {
                self.entities.get_mut(pet_id).unwrap().orbit = Some(realm_events::Orbit {
                    speed: crate::encounter::nr_speed(7.5),
                    radius: 10.,
                    direction: 1.,
                });
            }
        }
        if stage % 3 != 1 || age >= 16000 {
            let children: Vec<_> = self
                .actors()
                .filter(|e| e.parent == Some(id) && e.kind == SWARM)
                .map(|e| e.id)
                .collect();
            for child in children {
                self.remove_entity(child);
            }
        } else if self.now >= self.entities[&id].next_aux {
            let count = self
                .actors()
                .filter(|e| e.parent == Some(id) && e.kind == SWARM)
                .count();
            self.spawn_children(id, SWARM, (count + 1).min(8), 2.5)?;
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 750;
        }
        Ok(())
    }

    fn snake_treasure(&mut self, id: i32) -> Result<()> {
        let e = &self.entities[&id];
        if e.kind != BUTTON || e.stage.is_some() {
            return Ok(());
        }
        let pos = e.pos;
        if !self
            .players
            .values()
            .any(|p| p.active() && p.pos.distance(pos) <= 0.5)
        {
            return Ok(());
        }
        let spawners: Vec<_> = self
            .actors()
            .filter(|e| e.kind == SPAWNER && e.home.distance(pos) < 15.)
            .map(|e| e.id)
            .collect();
        ensure!(
            spawners.len() == 1,
            "Snake Pit button {id} requires one local Guard spawner, found {}",
            spawners.len()
        );
        let owner = spawners[0];
        let center = self.entities[&owner].home;
        let traps: Vec<_> = self
            .actors()
            .filter(|e| e.kind == DART && e.home.distance(center) < 15.)
            .map(|e| e.id)
            .collect();
        ensure!(
            traps.len() == 10,
            "Snake Pit spawner {owner} requires ten local dart sources, found {}",
            traps.len()
        );
        self.spawn_children(owner, GUARD, 1, 0.)?;
        let guard = self
            .actors()
            .find(|e| e.kind == GUARD && e.parent == Some(owner))
            .context("Snake Pit Guard missing after successful summon")?
            .id;
        self.entities.get_mut(&guard).unwrap().size = 100;
        for trap in traps {
            self.entities.get_mut(&trap).unwrap().parent.set(
                Some(owner),
                trap,
                &mut self.timed_ids,
            );
        }
        let button = self.entities.get_mut(&id).unwrap();
        button.stage = Some(1);
        button.extra_stats.push((61, 1));
        tracing::info!(
            button = id,
            spawner = owner,
            guard,
            "Snake Pit treasure room activated once"
        );
        Ok(())
    }
}

#[cfg(test)]
mod treasure_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{PI, TAU};

    fn fixture() -> (World, i32, i32) {
        let content = super::super::tests::world().content;
        let map = Arc::new(crate::dungeon::snake::generate(&content, 12351).unwrap());
        let mut w = World::new(content, map).unwrap();
        w.encounter = true;
        let boss = w.actors().find(|e| e.kind == BOSS).unwrap().id;
        let center = w.entities[&boss].pos;
        let player = w.enter(super::super::tests::ch(&w), None).unwrap();
        w.players.get_mut(&player).unwrap().pos = center;
        w.now = 50;
        w.npc_step();
        (w, boss, player)
    }

    fn pets(w: &World, boss: i32) -> Vec<i32> {
        w.actors()
            .filter(|e| e.kind == PET && e.parent == Some(boss))
            .map(|e| e.id)
            .collect()
    }

    fn angle(a: Pos, center: Pos) -> f32 {
        (a.y - center.y).atan2(a.x - center.x)
    }

    fn turn(a: f32, b: f32) -> f32 {
        (b - a + PI).rem_euclid(TAU) - PI
    }

    #[test]
    fn stheno_guardians_keep_their_radius_and_forward_motion_around_real_cover() {
        let (mut w, boss, player) = fixture();
        let center = w.entities[&boss].pos;
        let ids = pets(&w, boss);
        assert_eq!(ids.len(), 4);
        let mut minimum = 10f32;
        let mut maximum = 10f32;
        let mut reversals = 0;
        let mut blocked = 0;
        let mut largest_step = 0f32;
        for _ in 0..120 {
            let before: Vec<_> = ids.iter().map(|id| w.entities[id].pos).collect();
            w.now += 50;
            w.npc_step();
            for (id, old) in ids.iter().zip(before) {
                let pos = w.entities[id].pos;
                assert!(pos.x.is_finite() && pos.y.is_finite());
                minimum = minimum.min(pos.distance(center));
                maximum = maximum.max(pos.distance(center));
                largest_step = largest_step.max(pos.distance(old));
                reversals += usize::from(turn(angle(old, center), angle(pos, center)) < -0.0001);
                blocked += usize::from(pos.distance(old) < 0.001);
            }
        }
        println!(
            "{}",
            serde_json::json!({"minimum_radius":minimum,"maximum_radius":maximum,"reversals":reversals,"blocked_steps":blocked,"largest_step":largest_step})
        );
        assert!(
            minimum >= 9.9 && maximum <= 10.1,
            "guardian orbit collapsed: {minimum}..{maximum}"
        );
        assert_eq!((reversals, blocked), (0, 0));
        assert!(largest_step <= crate::encounter::nr_speed(7.5) / 1.5 * 0.05 + 0.001);
        w.damage_enemy(player, ids[0], 10000, false, &[], None)
            .unwrap();
        w.now += 50;
        w.npc_step();
        let replacements = pets(&w, boss);
        assert_eq!(replacements.len(), 4);
        assert!(!replacements.contains(&ids[0]));
        assert!(replacements.iter().all(|id| w.entities[id].orbit.is_some()));
    }

    #[test]
    fn stheno_guardians_slow_and_paralyze_both_orbit_and_motor() {
        let (mut w, boss, _) = fixture();
        let center = w.entities[&boss].pos;
        let id = pets(&w, boss)[0];
        w.now += 50;
        w.npc_step();
        let before = w.entities[&id].pos;
        w.entities.get_mut(&id).unwrap().condition |= condition::bit(condition::SLOWED);
        w.now += 50;
        w.npc_step();
        let slowed = w.entities[&id].pos;
        let expected = crate::encounter::nr_speed(7.5) / 1.5 / 10. * 0.05 * 0.5;
        assert!((turn(angle(before, center), angle(slowed, center)) - expected).abs() < 0.001);
        assert!((slowed.distance(center) - 10.).abs() < 0.001);
        w.entities.get_mut(&id).unwrap().condition |= condition::bit(condition::PARALYZED);
        let phase = w.entities[&id].phase;
        w.now += 50;
        w.npc_step();
        assert_eq!(
            (w.entities[&id].pos.x, w.entities[&id].pos.y),
            (slowed.x, slowed.y)
        );
        assert!(turn(phase, w.entities[&id].phase).abs() < 0.0001);
    }
}
