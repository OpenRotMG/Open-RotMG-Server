use super::*;
use crate::{
    condition,
    dungeon::manor::{self, *},
    encounter::Motion,
};

pub(super) struct Toss {
    parent: i32,
    kind: u16,
    position: Pos,
    deadline: u64,
    lifetime: u64,
}

impl World {
    pub(super) fn manor_toss(
        &mut self,
        parent: i32,
        kind: u16,
        position: Pos,
        lifetime: u64,
    ) -> Result<()> {
        // Phoenix TossEnemy lands after its flight timer, not alongside the telegraph.
        self.manor_toss_in(parent, kind, position, lifetime, 1500)
    }
    pub(super) fn manor_toss_in(
        &mut self,
        parent: i32,
        kind: u16,
        position: Pos,
        lifetime: u64,
        flight: u64,
    ) -> Result<()> {
        crate::encounter::validate(&self.content, kind)?;
        crate::loot::validate(&self.content, kind)?;
        if self.walkable(position) {
            ensure!(
                self.summon_occupancy() < self.summon_limit(),
                SummonBudget("Manor toss exceeds summon budget".into())
            );
            self.manor_births.push(Toss {
                parent,
                kind,
                position,
                deadline: self.now + flight,
                lifetime,
            });
        } else {
            tracing::debug!(parent, kind, ?position, "Manor toss has a blocked landing");
        }
        self.telegraph_throw(parent, position, 0xffffbf00);
        Ok(())
    }

    pub(super) fn manor_birth_step(&mut self) {
        let mut i = 0;
        while i < self.manor_births.len() {
            if self.manor_births[i].deadline > self.now {
                i += 1;
                continue;
            }
            let toss = self.manor_births.remove(i);
            if toss.kind == CREATURE && !self.entities.contains_key(&toss.parent) {
                tracing::debug!(
                    parent = toss.parent,
                    "Ruthven's death canceled his pending coffin"
                );
                continue;
            }
            if let Err(error) =
                self.manor_child(toss.parent, toss.kind, toss.position, Some(toss.lifetime))
            {
                tracing::error!(parent=toss.parent,kind=toss.kind,position=?toss.position,%error,"admitted Manor toss failed to land");
            }
        }
    }

    fn manor_child(
        &mut self,
        parent: i32,
        kind: u16,
        pos: Pos,
        lifetime: Option<u64>,
    ) -> Result<i32> {
        ensure!(
            self.summon_occupancy() < self.summon_limit(),
            SummonBudget("Manor summon budget exhausted".into())
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
        let (kind, pos, hp) = (e.kind, e.pos, e.hp);
        if matches!(kind, KAGE | MINE) {
            return self.manor_entry_mechanics(id, target, elapsed);
        }
        if kind == BOSS {
            let (stage, age) = manor::phase(elapsed);
            let part = stage % 3;
            if e.stage != Some(stage) {
                if part == 0 {
                    self.manor_clear_swarms(id);
                }
                if part == 2 {
                    ensure!(
                        self.summon_occupancy() + 49 <= self.summon_limit(),
                        SummonBudget("Ruthven bat formation exceeds summon budget".into())
                    );
                    let mut sites = vec![];
                    let mut cells = BTreeSet::new();
                    for i in 0..33 {
                        let a = i as f32 * std::f32::consts::TAU / 32.;
                        let r = if i == 0 { 0. } else { 2. };
                        let tile = (
                            (pos.x + r * a.cos()).floor() as i32,
                            (pos.y + r * a.sin()).floor() as i32,
                        );
                        let site = Pos {
                            x: tile.0 as f32 + 0.5,
                            y: tile.1 as f32 + 0.5,
                        };
                        if cells.insert(tile) && self.walkable(site) {
                            sites.push(site);
                        }
                    }
                    ensure!(
                        sites.len() >= 4,
                        "Ruthven bat formation requires four walkable nearby cells"
                    );
                    for i in 0..4 {
                        // His own swarm is ≈18-20 bats where they converge (2012 footage); the decoy groups are not counted.
                        let (root, bat, count, site) = if i == 0 {
                            (SWARM, ESCORT, 19, sites[0])
                        } else {
                            (
                                DECOY,
                                CHASER,
                                10,
                                sites[1 + (i - 1) * (sites.len() - 1) / 3],
                            )
                        };
                        let group = self.manor_child(id, root, site, None)?;
                        self.spawn_children(group, bat, count, 0.4)?;
                    }
                }
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(stage);
                e.goal = None;
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
            // No damage numbers from the sprite change until at least 4.9 s after ring 1, and no vulnerable gap before the bats was seen.
            if part == 1 {
                e.condition |= condition::bit(condition::INVULNERABLE);
            } else if part == 2 {
                e.condition |= condition::bit(condition::INVINCIBLE);
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
            if part == 1 && age >= manor::COFFIN_TOSS_MS && e.next_aux == 0 {
                let sites = [(12., 0.), (-12., 0.), (0., 12.), (0., -12.)].map(|(dx, dy)| Pos {
                    x: pos.x + dx,
                    y: pos.y + dy,
                });
                let landed = sites.iter().filter(|p| self.walkable(**p)).count();
                ensure!(
                    self.summon_occupancy() + landed <= self.summon_limit(),
                    SummonBudget("Ruthven coffin formation exceeds summon budget".into())
                );
                // The period corridor tactic relies on wall-bound throws failing, not being relocated.
                // https://www.reddit.com/r/RotMG/comments/u6lp3/damn_those_lil_feratu/
                for site in sites {
                    self.manor_toss_in(id, CREATURE, site, 20000, manor::COFFIN_FLIGHT_MS)?;
                }
                self.entities.get_mut(&id).unwrap().next_aux = 1;
                tracing::info!(
                    entity = id,
                    attempted = 4,
                    landed,
                    x = pos.x,
                    y = pos.y,
                    "Ruthven coffin tosses scheduled"
                );
            }
        } else if kind == COFFIN && hp <= 15000 && e.stage != Some(1) {
            // The first drop below 15 000 decides it: an impostor (which took ≈5 600 damage first, uOYS8_HhIZ4 2012-08-29) becomes a
            // Coffin Creature that is killed, not timed out; a real coffin releases 1 Nosferatu and ≈20 bats once (15-25 seen; wRCpJLqPxLw
            // 2012-07-25, RouEGVH0_eQ 2012-08-05) and drops its loot later, with no second wave below 8 000. `stage` Some(0) marks a coffin
            // already known to be real. The impostor share is unmeasured (1 of 4 seen).
            if e.stage.is_none() && self.rng.gen_ratio(1, 5) {
                let new = self.transform_fauna(id, CREATURE)?;
                tracing::info!(
                    entity = id,
                    replacement = new,
                    "false treasure coffin awakened without rewards"
                );
                return Ok(());
            }
            self.entities.get_mut(&id).unwrap().stage = Some(1);
            self.spawn_children(id, BAT, 20, 1.5)?;
            self.spawn_children(id, 0x1728, 1, 2.)?;
        } else if kind == 0x1729
            && self.now >= e.next_aux
            && let Some(site) = target
        {
            if self.walkable(site) {
                self.manor_toss(id, ROCK, site, 550)?;
            }
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 3000;
        } else if kind == ROCK && e.expires.is_none() {
            self.entities.get_mut(&id).unwrap().expires.set(
                Some(self.now + 550),
                id,
                &mut self.timed_ids,
            );
        }
        Ok(())
    }
    pub(super) fn manor_movement(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> (Motion, f32) {
        let e = &self.entities[&id];
        if e.kind == KAGE {
            return match e.stage.expect("Kage movement phase") {
                2 => (
                    Motion::WanderHome { radius: 1. },
                    crate::endgame::tiles_per_s(1.),
                ),
                3 => (
                    Motion::Chase {
                        sight: 20.,
                        stop: 1.,
                    },
                    crate::encounter::nr_speed(0.8),
                ),
                0 | 1 => (Motion::Stationary, 0.),
                _ => unreachable!("invalid Kage movement phase"),
            };
        }
        if e.kind == BOSS {
            match manor::phase(elapsed).0 % 3 {
                0 => (
                    Motion::WanderHome { radius: 1. },
                    manor::profile(BOSS).speed,
                ),
                1 => (Motion::Stationary, 0.),
                // In bat form he is the point his swarm follows, so the cloud's speed is his.
                2 => (
                    Motion::Chase {
                        sight: 20.,
                        stop: 2.,
                    },
                    manor::profile(SWARM).speed,
                ),
                _ => unreachable!(),
            }
        } else {
            let idle = match e.kind {
                BAT => target.is_none_or(|p| e.pos.distance(p) <= 2.),
                CHASER => target.is_none(),
                ESCORT => {
                    e.pos.distance(
                        self.entities[&e.parent.expect("Manor escort requires its swarm")].pos,
                    ) <= 4.
                }
                _ => false,
            };
            if idle {
                let e = self.entities.get_mut(&id).unwrap();
                if e.stage.is_none() {
                    e.home = e.pos;
                    e.stage = Some(0);
                }
                return (Motion::WanderHome { radius: 2. }, 1. / 1.5);
            }
            if matches!(e.kind, 0x1727..=0x1729) && target.is_none() {
                return (Motion::Stationary, 0.);
            }
            let p = manor::profile(e.kind);
            (p.motion, p.speed)
        }
    }
    pub(super) fn manor_death(&mut self, e: &Entity) -> Result<()> {
        if e.kind == BOSS {
            // His bats go with him; his Coffin Creatures and Lil Feratus stay and keep attacking (2012 footage, 5 kills).
            self.manor_clear_swarms(e.id);
            let children: Vec<_> = self
                .actors()
                .filter(|x| x.parent == Some(e.id))
                .map(|x| (x.id, matches!(x.kind, CREATURE | FERATU)))
                .collect();
            for (child, stays) in children {
                if stays {
                    self.entities.get_mut(&child).unwrap().parent.set(
                        None,
                        child,
                        &mut self.timed_ids,
                    );
                } else {
                    self.remove_entity(child);
                }
            }
        } else if e.kind == CREATURE {
            // An impostor's Coffin Creature left at least 2 Lil Feratus (uOYS8_HhIZ4 2012-08-29).
            let count = self.rng.gen_range(2..=4);
            let sites: Vec<_> = (0..32)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 32.;
                    Pos {
                        x: e.pos.x + 0.2 * a.cos(),
                        y: e.pos.y + 0.2 * a.sin(),
                    }
                })
                .filter(|p| self.walkable(*p))
                .collect();
            ensure!(
                sites.len() >= count,
                "Coffin Creature brood has insufficient walkable positions"
            );
            ensure!(
                self.summon_occupancy() + count <= self.summon_limit(),
                SummonBudget("Coffin Creature brood exceeds summon budget".into())
            );
            for i in 0..count {
                let pos = sites[i * sites.len() / count];
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

#[cfg(test)]
mod tests {
    use super::*;

    fn arena() -> (World, i32) {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let map = manor::generate(&content, 12351).unwrap();
        let mut world = World::new(content, Arc::new(map)).unwrap();
        world.name = "Manor of the Immortals".into();
        let boss = world.actors().find(|e| e.kind == BOSS).unwrap().id;
        (world, boss)
    }

    fn cohort(world: &World, boss: i32) -> BTreeSet<i32> {
        let roots: BTreeSet<_> = world
            .actors()
            .filter(|e| e.parent == Some(boss) && matches!(e.kind, SWARM | DECOY))
            .map(|e| e.id)
            .collect();
        world
            .actors()
            .filter(|e| roots.contains(&e.id) || e.parent.is_some_and(|id| roots.contains(&id)))
            .map(|e| e.id)
            .collect()
    }

    #[test]
    fn generated_bookshelves_protect_only_ordinary_walls() {
        let content = arena().0.content;
        let map = manor::generate(&content, 13166792894241567768).unwrap();
        let world = World::new(content, Arc::new(map)).unwrap();
        let mut counts = [0, 0];
        for e in world.actors().filter(|e| e.kind == BOOKCASE) {
            let secret = world.map.cells
                [(e.pos.y as i32 * world.map.width + e.pos.x as i32) as usize]
                .region
                == 250;
            counts[usize::from(secret)] += 1;
            assert_eq!(
                e.condition,
                if secret {
                    0
                } else {
                    condition::bit(condition::INVINCIBLE)
                }
            );
        }
        assert!(counts.iter().all(|count| *count > 0));
    }

    fn secret_bookshelf_view() -> (World, i32, i32, Pos) {
        let (mut world, _) = arena();
        let (wall, outside, inside) = world
            .actors()
            .filter(|e| {
                e.kind == BOOKCASE && world.map.cell(e.pos.x, e.pos.y).unwrap().region == 250
            })
            .find_map(|e| {
                [(0., 1.), (1., 0.)].into_iter().find_map(|(dx, dy)| {
                    let outside = Pos {
                        x: e.pos.x + dx,
                        y: e.pos.y + dy,
                    };
                    let inside = Pos {
                        x: e.pos.x - dx,
                        y: e.pos.y - dy,
                    };
                    (world.walkable(outside) && world.walkable(inside))
                        .then_some((e.id, outside, inside))
                })
            })
            .expect("generated Manor has a secret bookshelf between walkable tiles");
        assert!(world.content.objects[&BOOKCASE].blocks_sight);
        assert_eq!(world.entities[&wall].hp, 3000);
        assert_eq!(world.entities[&wall].condition, 0);
        let (fixture, player) = super::super::movement_tests::fixture(0x18, false);
        let player = world
            .enter(fixture.players[&player].character.clone(), None)
            .unwrap();
        world.players.get_mut(&player).unwrap().pos = outside;
        eprintln!(
            "Manor seed 12351: wall={:?}, outside={outside:?}, inside={inside:?}",
            world.entities[&wall].pos
        );
        (world, player, wall, inside)
    }

    #[test]
    fn manor_secret_bookshelf_hides_unexplored_tiles_and_actors_until_opened() {
        let (mut world, player, wall, inside) = secret_bookshelf_view();
        let tile = (inside.x as i16, inside.y as i16);
        let wall_pos = world.entities[&wall].pos;
        let wall_tile = (wall_pos.x as i16, wall_pos.y as i16);
        let marker = world.spawn_entity(ROCK, inside, vec![]).unwrap();
        let outside = world.players[&player].pos;
        assert!(
            !world
                .room_sight((outside.x as i32, outside.y as i32))
                .contains(&tile)
        );
        assert_ne!(world.quest_target(player), Some(marker));
        world.updates(player, true).unwrap();
        let p = &world.players[&player];
        assert!(
            p.known.contains(&wall),
            "the unopened bookshelf face must be visible"
        );
        assert!(p.tiles.contains(&wall_tile));
        assert!(
            !p.tiles.contains(&tile),
            "unopened Manor bookshelf leaked terrain behind it"
        );
        assert!(
            !p.known.contains(&marker),
            "unopened Manor bookshelf leaked an actor behind it"
        );
        world.remove_entity(wall);
        world.updates(player, false).unwrap();
        let p = &world.players[&player];
        assert_eq!((p.pos.x, p.pos.y), (outside.x, outside.y));
        assert!(
            p.tiles.contains(&tile),
            "opened bookshelf did not reveal terrain to a stationary player"
        );
        assert!(p.known.contains(&marker));
        assert!(!p.known.contains(&wall));
    }

    #[test]
    fn manor_secret_bookshelf_removal_invalidates_stationary_tile_scan() {
        let (mut world, player, wall, _) = secret_bookshelf_view();
        world.updates(player, true).unwrap();
        assert!(world.players[&player].tile_scan.is_some());
        world.remove_entity(wall);
        assert_eq!(
            world.players[&player].tile_scan, None,
            "Manor bookshelf removal retained the closed-room scan"
        );
    }

    #[test]
    fn manor_admitted_staff_projectiles_defeat_original_bookshelf_and_open_sight() {
        let (fixture, _) = super::super::movement_tests::fixture(0x18, false);
        let mut map = (*fixture.map).clone();
        for y in 0..map.height {
            let cell = &mut map.cells[(y * map.width + 10) as usize];
            cell.object = Some(if y == 8 { BOOKCASE } else { 0x180a });
            cell.region = if y == 8 { 250 } else { 0 };
        }
        let mut world = World::new(fixture.content.clone(), Arc::new(map)).unwrap();
        world.name = "Manor of the Immortals".into();
        world.encounter = true;
        let outside = Pos { x: 8.5, y: 8.5 };
        let wall_pos = Pos { x: 10.5, y: 8.5 };
        let inside = (11, 8);
        let wall = world
            .actors()
            .find(|e| e.kind == BOOKCASE && e.pos.distance(wall_pos) == 0.)
            .unwrap()
            .id;
        assert_eq!(world.map.cell(wall_pos.x, wall_pos.y).unwrap().region, 250);
        assert_eq!(world.entities[&wall].hp, 3000);
        assert_eq!(world.entities[&wall].condition, 0);
        let mut character =
            Character::fresh(world.content.get(crate::content::WIZARD).unwrap()).unwrap();
        character.id = 1384;
        character.account = 1;
        character.name = "ManorStaff".into();
        character.items.fill(-1);
        character.items[0] = 0xb08;
        character
            .gain_experience(
                &world.content,
                crate::store::level_start(20).unwrap(),
                &mut rand::rngs::StdRng::seed_from_u64(1384),
            )
            .unwrap();
        let player = world
            .enter_at(
                character,
                None,
                Some(outside),
                AccountPresentation {
                    guild: None,
                    credits: 0,
                    fame: 0,
                    best_fame: BTreeMap::new(),
                },
                false,
            )
            .unwrap();
        assert_eq!(world.players[&player].character.level, 20);
        assert_eq!(world.players[&player].character.shots, 0);
        assert_eq!(world.players[&player].character.hits, 0);
        assert_eq!(world.players[&player].conditions(world.now), 0);
        world.updates(player, true).unwrap();
        assert!(world.players[&player].known.contains(&wall));
        assert!(!world.players[&player].tiles.contains(&inside));
        let start = Pos {
            x: outside.x + 0.3,
            y: outside.y,
        };
        let mut damaged = false;
        for _ in 0..40 {
            if !world.entities.contains_key(&wall) {
                break;
            }
            for _ in 0..2 {
                let bullet = world.players[&player].next_bullet;
                world
                    .shoot(player, world.now as i32, bullet, 0xb08, start, 0.)
                    .unwrap();
            }
            for _ in 0..10 {
                world.step();
                let p = &world.players[&player];
                assert!(
                    p.character.alive && p.character.hp > 0 && !p.death_pending,
                    "engine player became terminal: time={} hp={} alive={} pending={}",
                    world.now,
                    p.character.hp,
                    p.character.alive,
                    p.death_pending
                );
                if let Some(e) = world.entities.get(&wall) {
                    if e.hp < 3000 {
                        assert!(e.hp > 0);
                        assert_eq!(e.damage.get(&1384), Some(&(3000 - e.hp)));
                        assert!(p.character.hits > 0 && p.character.lifetime.projectile_hits > 0);
                        damaged = true;
                    }
                } else {
                    break;
                }
            }
        }
        assert!(
            damaged,
            "no surviving original-wall projectile damage observed"
        );
        assert!(
            !world.entities.contains_key(&wall),
            "staff did not defeat original bookshelf"
        );
        world.updates(player, false).unwrap();
        let p = &world.players[&player];
        assert_eq!((p.pos.x, p.pos.y), (outside.x, outside.y));
        assert!(p.tiles.contains(&inside));
        assert!(!p.known.contains(&wall));
        assert!(p.character.shots > 0 && p.character.lifetime.projectile_hits > 0);
        assert!(p.character.kills > 0 && p.character.lifetime.kills > 0);
        assert_eq!(p.character.manors, 0);
        eprintln!(
            "engine staff bookshelf: time={}ms shots={} hits={} projectile_hits={} kills={}",
            world.now,
            p.character.shots,
            p.character.hits,
            p.character.lifetime.projectile_hits,
            p.character.kills
        );
    }

    #[test]
    fn genuine_coffin_threshold_waves_do_not_repeat() {
        let (mut world, boss) = arena();
        let id = world
            .spawn_entity(COFFIN, world.entities[&boss].home, vec![])
            .unwrap();
        world.entities.get_mut(&id).unwrap().stage = Some(0);
        for (hp, bats, vampires) in [(14999, 20, 1), (7999, 20, 1)] {
            world.entities.get_mut(&id).unwrap().hp = hp;
            for _ in 0..2 {
                world.manor_mechanics(id, None, 0).unwrap();
                for (kind, count) in [(BAT, bats), (0x1728, vampires)] {
                    assert_eq!(
                        world
                            .actors()
                            .filter(|e| e.parent == Some(id) && e.kind == kind)
                            .count(),
                        count
                    );
                }
            }
        }
    }

    #[test]
    fn manor_bats_move_at_their_2012_speeds() {
        // Tiles per second: `profile()` divides by 1.5 and the engine divides by 1.5 again.
        let tiles = |kind| manor::profile(kind).speed / 1.5;
        let measured = [
            (0x1727, 6.5),
            (BAT, 5.6),
            (CHASER, 6.),
            (ESCORT, 6.),
            (SWARM, 4.7),
            (DECOY, 4.7),
        ];
        for (kind, speed) in measured {
            assert!(
                (tiles(kind) - speed).abs() < 1e-4,
                "{kind:#x} moves {} tiles/s",
                tiles(kind)
            );
        }
        // In bat form Ruthven moves with his swarm.
        let (mut world, boss) = arena();
        let bats = manor::DAGGERS_MS + manor::RITUAL_MS;
        assert_eq!(
            world.manor_movement(boss, None, bats).1,
            manor::profile(SWARM).speed
        );
        // The chasers cover that ground in one second of engine ticks toward a player 18 tiles away.
        for (kind, speed) in &measured[..3] {
            let (mut world, player) = crate::world::movement_tests::fixture(0x18, false);
            world.name = "Manor of the Immortals".into();
            world.encounter = true;
            world.players.get_mut(&player).unwrap().pos = Pos { x: 22.5, y: 16.5 };
            let bat = world
                .spawn_entity(*kind, Pos { x: 4.5, y: 16.5 }, vec![])
                .unwrap();
            world.now += 50;
            world.npc_step();
            let start = world.entities[&bat].pos;
            for _ in 0..20 {
                world.now += 50;
                world.npc_step();
            }
            let moved = world.entities[&bat].pos.x - start.x;
            assert!(
                (moved - speed).abs() < 0.05 * speed,
                "{kind:#x} moved {moved} tiles in one second"
            );
        }
    }

    #[test]
    fn ruthven_fires_the_2012_cycle() {
        use crate::encounter::attacks::schedule;
        let degrees = |shots: Vec<(u8, f32)>| -> Vec<i32> {
            let mut out: Vec<i32> = shots
                .iter()
                .map(|s| s.1.to_degrees().rem_euclid(360.).round() as i32 % 360)
                .collect();
            out.sort();
            out.dedup();
            out
        };
        assert_eq!(manor::CYCLE_MS, 20200);
        // Daggers every 0.45 s: 3 at full HP, 5 in the middle, a full ring of 18 late.
        for (hp, count) in [(15000, 3), (9000, 5), (4000, 18)] {
            for t in [0, 450] {
                assert_eq!(schedule(BOSS, hp, t).unwrap().snapshot(0.).len(), count);
            }
        }
        assert_eq!(
            degrees(schedule(BOSS, 4000, 0).unwrap().snapshot(0.)).len(),
            18
        );
        // Four rings of 24 Ruthven Shots 15° apart, one due east, 0.2 s after the sprite change and 0.4 s apart.
        for k in 0..4 {
            let ring = schedule(BOSS, 15000, manor::DAGGERS_MS + 200 + 400 * k)
                .unwrap()
                .snapshot(0.);
            assert!(ring.iter().all(|s| s.0 == 1));
            assert_eq!(degrees(ring), (0..24).map(|i| i * 15).collect::<Vec<_>>());
        }
        // Nothing but bats for the last 12 s.
        assert!(
            schedule(BOSS, 15000, manor::DAGGERS_MS + manor::RITUAL_MS + 450)
                .unwrap()
                .snapshot(0.)
                .is_empty()
        );
    }

    #[test]
    fn kage_kami_reveals_on_the_first_hit_and_cycles_its_2012_phases() {
        let (mut world, boss) = arena();
        let home = world.entities[&boss].home;
        let kage = world.spawn_entity(KAGE, home, vec![]).unwrap();
        world.manor_mechanics(kage, None, 0).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(0));
        world.entities.get_mut(&kage).unwrap().hp -= 1;
        world.manor_mechanics(kage, None, 0).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(1));
        world.manor_mechanics(kage, None, 2399).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(1));
        world.manor_mechanics(kage, None, 2400).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(2));
        let left = world.entities[&kage].reaction_until - world.now;
        assert!((6000..=10000).contains(&left), "phase A for {left} ms");
        world.now += left;
        world.manor_mechanics(kage, None, 0).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(3));
        world.manor_mechanics(kage, None, 6999).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(3));
        world.manor_mechanics(kage, None, 7000).unwrap();
        assert_eq!(world.entities[&kage].stage, Some(2));
    }

    #[test]
    fn ruthven_return_clears_only_his_bats_once_per_cycle() {
        let (mut world, boss) = arena();
        let home = world.entities[&boss].home;
        let other = world.spawn_entity(BOSS, home, vec![]).unwrap();
        let bats_at = manor::DAGGERS_MS + manor::RITUAL_MS;
        world.manor_mechanics(other, None, bats_at).unwrap();
        let foreign = cohort(&world, other);
        world.entities.get_mut(&boss).unwrap().hp = 14000;
        world
            .entities
            .get_mut(&boss)
            .unwrap()
            .damage
            .insert(123, 1000);
        for cycle in 0..2 {
            let start = cycle * manor::CYCLE_MS + bats_at;
            world.now = start;
            let coffin = world
                .manor_child(boss, CREATURE, home, Some(20000))
                .unwrap();
            world.manor_mechanics(boss, None, start).unwrap();
            let bats = cohort(&world, boss);
            assert_eq!(bats.len(), 53);
            assert!(
                bats.iter()
                    .all(|id| world.content.objects[&world.entities[id].kind].xp_multiplier == 0.)
            );
            world.manor_mechanics(boss, None, start + 50).unwrap();
            assert_eq!(cohort(&world, boss), bats);
            world.now = start + manor::BATS_MS;
            world
                .manor_mechanics(boss, None, start + manor::BATS_MS)
                .unwrap();
            assert!(cohort(&world, boss).is_empty());
            assert!(bats.iter().all(|id| !world.entities.contains_key(id)));
            assert_eq!(cohort(&world, other), foreign);
            assert!(world.entities.contains_key(&coffin));
            assert_eq!(world.entities[&boss].hp, 14000);
            assert_eq!(world.entities[&boss].damage[&123], 1000);
        }
    }

    #[test]
    fn ruthven_death_clears_his_bats_keeps_his_coffins_and_cancels_only_his_pending_ones() {
        let (mut world, boss) = arena();
        let home = world.entities[&boss].home;
        let other = world.spawn_entity(BOSS, home, vec![]).unwrap();
        world
            .manor_mechanics(boss, None, manor::DAGGERS_MS + manor::RITUAL_MS)
            .unwrap();
        let bats = cohort(&world, boss);
        let mut kept = BTreeSet::new();
        let coffin = world.manor_child(boss, CREATURE, home, None).unwrap();
        let defeated = world.remove_entity(coffin);
        world.manor_death(&defeated).unwrap();
        let brood: Vec<_> = world
            .actors()
            .filter(|e| e.parent == Some(boss) && e.kind == FERATU)
            .map(|e| e.id)
            .collect();
        assert!((2..=4).contains(&brood.len()));
        kept.extend(brood);
        kept.insert(world.manor_child(boss, CREATURE, home, None).unwrap());
        let foreign = world.manor_child(other, CREATURE, home, None).unwrap();
        world.manor_toss(boss, CREATURE, home, 20000).unwrap();
        world.manor_toss(other, CREATURE, home, 20000).unwrap();
        let defeated = world.remove_entity(boss);
        world.manor_death(&defeated).unwrap();
        assert!(bats.iter().all(|id| !world.entities.contains_key(id)));
        assert!(kept.iter().all(|id| world.entities[id].parent.is_none()));
        assert!(world.entities.contains_key(&foreign));
        world.now = 1499;
        world.manor_birth_step();
        assert_eq!(world.manor_births.len(), 2);
        world.now = 1500;
        world.manor_birth_step();
        assert!(world.manor_births.is_empty());
        assert!(!world.actors().any(|e| e.parent == Some(boss)));
        assert_eq!(
            world
                .actors()
                .filter(|e| e.parent == Some(other) && e.kind == CREATURE)
                .count(),
            2
        );
        world.manor_birth_step();
        assert_eq!(
            world
                .actors()
                .filter(|e| e.parent == Some(other) && e.kind == CREATURE)
                .count(),
            2
        );
    }
}
