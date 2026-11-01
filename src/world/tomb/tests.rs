use super::*;

fn fixture() -> (World, i32) {
    let (mut world, player) = super::super::movement_tests::fixture(0xa0, false);
    let character = world.leave(player).unwrap();
    let map = Arc::new(d::generate(&world.content, 12).unwrap());
    let mut world = World::new(world.content.clone(), map).unwrap();
    world.name = "Tomb of the Ancients".into();
    world.encounter = true;
    let player = world.enter(character, None).unwrap();
    (world, player)
}

#[test]
fn numbered_artifacts_keep_stasis_and_original_boss_bindings_across_player_reentry() {
    let (mut world, player) = fixture();
    let objectives: Vec<_> = world
        .actors()
        .filter(|e| e.kind == d::SARCOPHAGUS)
        .map(|e| e.id)
        .collect();
    for id in objectives {
        world
            .damage_enemy(player, id, 1_000_000, true, &[], None)
            .unwrap();
    }
    let bosses = d::BOSSES.map(|kind| world.actors().find(|e| e.kind == kind).unwrap().id);
    for id in bosses {
        world.tomb_mechanics(id, None).unwrap();
        world.tomb_enter(id, 5, false).unwrap();
        world.tomb_mechanics(id, None).unwrap();
    }
    let mut artifacts = Vec::new();
    for (producer, base) in bosses.into_iter().zip([0xd1c, 0xd1f, 0xd22]) {
        for (number, anchor) in [bosses[2], bosses[1], bosses[0]].into_iter().enumerate() {
            let owned = world.tomb_owned(producer, base + number as u16);
            assert_eq!(owned.len(), 1);
            let id = owned[0];
            assert_eq!(
                world.entities[&id].tomb.as_ref().unwrap().anchor,
                Some(anchor)
            );
            world.entities.get_mut(&id).unwrap().activated = Some(world.now);
            world.tomb_mechanics(id, None).unwrap();
            world.apply_condition(id, condition::STASIS, 1000).unwrap();
            assert_ne!(
                world.entities[&id].conditions(world.now) & condition::bit(condition::STASIS),
                0
            );
            artifacts.push((id, producer, anchor, world.entities[&id].hp));
        }
    }
    let saved = world.leave(player).unwrap();
    assert_eq!(saved.tombs, 0);
    let reentered = world.enter(saved, None).unwrap();
    assert_ne!(reentered, player);
    assert_eq!(world.players[&reentered].character.id, 1);
    for &(id, producer, anchor, hp) in &artifacts {
        let e = &world.entities[&id];
        assert_eq!(
            (
                e.tomb.as_ref().unwrap().producer,
                e.tomb.as_ref().unwrap().anchor,
                e.hp
            ),
            (Some(producer), Some(anchor), hp)
        );
        assert_eq!(e.statuses[&condition::STASIS], (0, 1000));
    }
    world.now = 1000;
    for &(id, _, _, _) in &artifacts {
        let flags = world.entities[&id].conditions(world.now);
        assert_eq!(flags & condition::bit(condition::STASIS), 0);
        assert_ne!(flags & condition::bit(condition::STASIS_IMMUNE), 0);
    }
    let lost = bosses[2];
    let position = world.entities[&lost].pos;
    world
        .damage_enemy(reentered, lost, 1_000_000, true, &[], None)
        .unwrap();
    let replacement = world.spawn_entity(d::BES, position, vec![]).unwrap();
    assert_ne!(lost, replacement);
    for &(id, producer, anchor, _) in &artifacts {
        assert!(world.entities.contains_key(&id));
        world.tomb_mechanics(id, Some(position)).unwrap();
        assert_eq!(
            world.entities[&id].tomb.as_ref().unwrap().producer,
            Some(producer)
        );
        if anchor == lost {
            assert!(matches!(
                world.tomb_movement(id, Some(position)).unwrap().0,
                Motion::Chase { .. }
            ));
            assert_eq!(
                world.entities[&id].tomb.as_ref().unwrap().anchor,
                Some(lost)
            );
        }
    }
    assert_eq!(world.players[&reentered].character.tombs, 0);
    assert_eq!(
        world
            .actors()
            .filter(|e| (0xd1c..=0xd24).contains(&e.kind))
            .count(),
        9
    );
}

#[test]
fn equipped_orb_stasises_indestructible_edge_turrets_but_not_center_turrets() {
    for kind in 0xd31..=0xd36 {
        let (mut world, player) = fixture();
        let mut character = world.leave(player).unwrap();
        character.class = 0x323;
        character.base = [670, 385, 60, 25, 60, 50, 40, 75];
        character.hp = 670;
        character.mp = 440;
        character.items[..4].copy_from_slice(&[-1, 0xb2d, 0xb05, 0xac8]);
        let player = world.enter(character, None).unwrap();
        let target = world.actors().find(|e| e.kind == kind).unwrap().id;
        let pos = world.entities[&target].pos;
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: pos.x,
            y: pos.y + 1.,
        };
        assert!(d::immortal(kind));
        assert_eq!(world.entities[&target].hp, 0);
        assert!(
            world
                .use_item(
                    player,
                    Slot {
                        object: player,
                        slot: 1,
                        kind: 0xb2d
                    },
                    pos
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(world.players[&player].character.mp, 355);
        let flags = world.entities[&target].conditions(world.now);
        assert_eq!(
            flags & condition::bit(condition::STASIS) != 0,
            kind <= 0xd33,
            "turret {kind:#x}"
        );
        assert_eq!(world.entities[&target].hp, 0);
    }
}

#[test]
fn mummy_proximity_fires_before_suicide_and_keeps_both_scarab_waves() {
    let (mut world, player) = fixture();
    let mummy = world.actors().find(|e| e.kind == 0xd1a).unwrap().id;
    let pos = world.entities[&mummy].pos;
    world.players.get_mut(&player).unwrap().pos = Pos {
        x: pos.x - 1.5,
        y: pos.y,
    };
    world.npc_step();
    assert_eq!(world.entities[&mummy].stage, Some(1));
    assert_eq!(world.entities[&mummy].hp, 3000);
    assert_eq!(
        world.bullets.iter().filter(|b| b.owner == mummy).count(),
        36
    );
    assert_eq!(world.tomb_owned(mummy, 0xd1b).len(), 5);
    world.step();
    assert!(!world.entities.contains_key(&mummy));
    assert!((10..=13).contains(&world.tomb_owned(mummy, 0xd1b).len()));
    assert_eq!(
        world.bullets.iter().filter(|b| b.owner == mummy).count(),
        36
    );
    assert_eq!(world.players[&player].character.tombs, 0);
}
