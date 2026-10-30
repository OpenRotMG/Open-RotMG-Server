use super::*;

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
