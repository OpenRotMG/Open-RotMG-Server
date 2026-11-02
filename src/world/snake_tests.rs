use super::*;

fn swarm_fixture(offset: f32) -> (World, i32, i32) {
    use crate::dungeon::snake::{BOSS, SWARM};
    let (mut world, player) = super::movement_tests::fixture(0x18, false);
    world.name = "Snake Pit".into();
    world.encounter = true;
    world.rng = rand::rngs::StdRng::seed_from_u64(1363);
    let center = Pos { x: 16.5, y: 16.5 };
    world.players.get_mut(&player).unwrap().pos = center;
    let boss = world.spawn_entity(BOSS, center, vec![]).unwrap();
    for _ in 0..161 {
        world.now += 50;
        world.npc_step();
    }
    assert_eq!(world.entities[&boss].stage, Some(1));
    let center = world.entities[&boss].pos;
    let swarm = world
        .spawn_entity(
            SWARM,
            Pos {
                x: center.x + offset,
                y: center.y,
            },
            vec![],
        )
        .unwrap();
    world
        .entities
        .get_mut(&swarm)
        .unwrap()
        .parent
        .set(Some(boss), swarm, &mut world.timed_ids);
    world.players.values_mut().next().unwrap().pos = Pos {
        x: center.x + 5.,
        y: center.y,
    };
    (world, boss, swarm)
}

#[test]
fn stheno_swarm_uses_protect_speed_and_one_tile_inner_range() {
    for offset in [1.5, 3.] {
        let (mut world, boss, swarm) = swarm_fixture(offset);
        let start = world.entities[&swarm].pos;
        world.now += 50;
        world.npc_step();
        let end = world.entities[&swarm].pos;
        let expected = (5.55 * 0.3 + 0.74) * 0.05;
        eprintln!(
            "swarm approach: offset={offset} start={start:?} end={end:?} expected_step={expected}"
        );
        assert!((start.distance(end) - expected).abs() < 0.0001);
        let leader = world.entities[&boss].pos;
        assert!(end.distance(leader) < start.distance(leader));
        assert!(
            end.x < start.x,
            "swarm followed the player, not its own boss"
        );
    }
}

#[test]
fn stheno_swarm_wanders_on_top_and_reprotects_only_beyond_two_tiles() {
    let (mut world, boss, swarm) = swarm_fixture(0.);
    let start = world.entities[&swarm].pos;
    for _ in 0..4 {
        world.now += 50;
        world.npc_step();
    }
    let e = &world.entities[&swarm];
    eprintln!(
        "swarm on top: start={start:?} end={:?} following={:?}",
        e.pos, e.following
    );
    assert!(
        e.pos.distance(start) > 0.1,
        "protected swarm froze on its boss"
    );
    assert_eq!(e.following, Following::Resting);
    for (distance, expected) in [(1.5, Following::Resting), (2.3, Following::Acquired)] {
        let pos = world.entities[&swarm].pos;
        world
            .position_entity(
                boss,
                Pos {
                    x: pos.x + distance,
                    y: pos.y,
                },
            )
            .unwrap();
        world.now += 50;
        world.npc_step();
        assert_eq!(world.entities[&swarm].following, expected);
    }
}

#[test]
fn stheno_swarm_never_pursues_a_player_without_its_parent() {
    let (mut world, boss, swarm) = swarm_fixture(3.);
    world.remove_entity(boss);
    assert!(world.entities.contains_key(&swarm));
    let start = world.entities[&swarm].pos;
    world.now += 50;
    world.npc_step();
    assert_eq!(world.entities[&swarm].pos.distance(start), 0.);
}

#[test]
fn guardians_circle_through_sthenos_phases_without_stalling_on_cover() {
    let (fixture, player) = super::movement_tests::fixture(0x18, false);
    for seed in [855, 12351] {
        let map = crate::dungeon::snake_pit(&fixture.content, seed).unwrap();
        let mut world = World::new(fixture.content.clone(), Arc::new(map)).unwrap();
        world.name = "Snake Pit".into();
        world.encounter = true;
        let boss = world
            .actors()
            .find(|e| e.kind == crate::dungeon::snake::BOSS)
            .unwrap()
            .id;
        let center = world.entities[&boss].pos;
        let player = world
            .enter(fixture.players[&player].character.clone(), None)
            .unwrap();
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: center.x - 3.,
            y: center.y,
        };
        let mut stalled = BTreeMap::new();
        let mut stages = BTreeSet::new();
        for tick in 1..=680 {
            world.now = tick * 50;
            world.npc_step();
            stages.insert(world.entities[&boss].stage.unwrap());
            let pets: Vec<_> = world
                .actors()
                .filter(|e| e.kind == crate::dungeon::snake::PET && e.parent == Some(boss))
                .collect();
            assert_eq!(pets.len(), 4);
            for pet in pets {
                assert!(
                    world.walkable(pet.pos),
                    "guardian entered cover: seed {seed}, pet {}",
                    pet.id
                );
                let idle = stalled.entry(pet.id).or_insert(0);
                *idle = if pet.last_step.distance(Pos { x: 0., y: 0. }) < 0.01 {
                    *idle + 1
                } else {
                    0
                };
                assert!(
                    *idle < 20,
                    "guardian stalled for one second: seed {seed}, tick {tick}, pet {}, position {:?}",
                    pet.id,
                    pet.pos
                );
            }
        }
        assert_eq!(stages, BTreeSet::from([0, 1, 2, 3]));
        assert!(
            world
                .actors()
                .filter(|e| matches!(e.kind, 0x170 | 0x172))
                .all(|e| !world.walkable(e.pos))
        );
    }
}

#[test]
fn stheno_guardians_keep_the_authored_orbit_rate_inside_cover() {
    let (fixture, player) = super::movement_tests::fixture(0x18, false);
    let map = crate::dungeon::snake_pit(&fixture.content, 855).unwrap();
    let mut world = World::new(fixture.content.clone(), Arc::new(map)).unwrap();
    world.name = "Snake Pit".into();
    world.encounter = true;
    let boss = world
        .actors()
        .find(|e| e.kind == crate::dungeon::snake::BOSS)
        .unwrap()
        .id;
    let center = world.entities[&boss].pos;
    let player = world
        .enter(fixture.players[&player].character.clone(), None)
        .unwrap();
    world.players.get_mut(&player).unwrap().pos = Pos {
        x: center.x - 3.,
        y: center.y,
    };
    let mut angles = Vec::new();
    for tick in 1..=40 {
        world.now = tick * 50;
        world.npc_step();
        let boss_pos = world.entities[&boss].pos;
        let pet = world
            .actors()
            .find(|e| e.kind == crate::dungeon::snake::PET && e.parent == Some(boss))
            .unwrap();
        angles.push((pet.pos.y - boss_pos.y).atan2(pet.pos.x - boss_pos.x));
    }
    let step = (5.55 * 7.5 + 0.74) / 10. * 0.05;
    let mut moved = 0;
    let mut turn = 0.;
    for pair in angles.windows(2) {
        let mut delta = pair[1] - pair[0];
        if delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU;
        } else if delta < -std::f32::consts::PI {
            delta += std::f32::consts::TAU;
        }
        if delta.abs() < 0.01 {
            continue;
        }
        moved += 1;
        turn += delta;
    }
    let expected = step * moved as f32;
    assert!(moved >= 30, "guardian barely moved: {moved} steps");
    assert!(
        (turn - expected).abs() < 0.05,
        "moving steps turned {turn} rad, authored Orbit(7.5, 10) rate is {expected}"
    );
}
