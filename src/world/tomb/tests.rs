use super::*;

#[test]
fn numbered_artifacts_keep_stasis_and_original_boss_bindings_across_player_reentry() {
    let (mut world, player) = super::super::movement_tests::fixture(0xa0, false);
    let character = world.leave(player).unwrap();
    let map = Arc::new(d::generate(&world.content, 12).unwrap());
    let mut world = World::new(world.content.clone(), map).unwrap();
    world.name = "Tomb of the Ancients".into();
    world.encounter = true;
    let player = world.enter(character, None).unwrap();
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
        assert_eq!(
            world.entities[&id].tomb.as_ref().unwrap().producer,
            Some(producer)
        );
        if anchor == lost {
            assert_eq!(
                world.entities[&id].tomb.as_ref().unwrap().anchor,
                Some(lost)
            );
            assert!(matches!(
                world.tomb_movement(id, Some(position)).unwrap().0,
                Motion::Chase { .. }
            ));
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
