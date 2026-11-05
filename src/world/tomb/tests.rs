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

fn awakened_fixture() -> (World, i32, [i32; 3]) {
    let (mut world, player) = fixture();
    world.rng = rand::SeedableRng::seed_from_u64(1362);
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
    }
    (world, player, bosses)
}

#[test]
fn circle_phases_resume_around_generated_room_pillars() {
    for kind in d::BOSSES {
        for phase in [weapons::SPECIAL_TWO, weapons::SPECIAL_THREE] {
            for (dx, dy) in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)] {
                let (mut world, player, bosses) = awakened_fixture();
                let id = bosses[d::BOSSES.iter().position(|k| *k == kind).unwrap()];
                world.tomb_enter(id, phase, false).unwrap();
                let anchor = world.entities[&id].tomb.as_ref().unwrap().anchor.unwrap();
                let center = world.entities[&anchor].pos;
                let hp = weapons::thresholds(kind).unwrap()[(phase - 6) as usize] - 1;
                let e = world.entities.get_mut(&id).unwrap();
                e.hp = hp;
                let orbit = e.orbit.as_mut().unwrap();
                orbit.radius = 9.9;
                orbit.direction = -dx * dy;
                let orbit = *orbit;
                world
                    .position_entity(
                        id,
                        Pos {
                            x: center.x,
                            y: center.y + dy * orbit.radius,
                        },
                    )
                    .unwrap();
                world.players.get_mut(&player).unwrap().pos = center;
                let pillar = Pos {
                    x: center.x + dx * 7.,
                    y: center.y + dy * 7.,
                };
                assert!(!world.walkable(pillar));
                let mut contact = false;
                let mut beyond = false;
                let mut stalled = 0;
                let mut longest_stall = 0;
                for _ in 0..120 {
                    let old = world.entities[&id].pos;
                    world.now += 50;
                    world.npc_step();
                    let e = &world.entities[&id];
                    assert!(world.walkable(e.pos), "{kind:#x}/{phase}: entered a pillar");
                    assert_eq!(e.stage, Some(phase));
                    assert!((hp..=world.content.objects[&kind].hp).contains(&e.hp));
                    assert_eq!(e.orbit.unwrap().speed, orbit.speed);
                    assert_eq!(e.orbit.unwrap().radius, orbit.radius);
                    assert_eq!(e.orbit.unwrap().direction, orbit.direction);
                    assert!(old.distance(e.pos) <= orbit.speed / 1.5 * 0.05 + 0.001);
                    contact |= e.pos.distance(pillar) < 1.;
                    beyond |= dx * (e.pos.x - center.x) > 8. && dy * (e.pos.y - center.y) < 7.;
                    stalled = if e.pos.distance(old) < 0.001 {
                        stalled + 50
                    } else {
                        0
                    };
                    longest_stall = longest_stall.max(stalled);
                }
                assert!(contact, "{kind:#x}/{phase}: fixture missed the pillar");
                assert!(
                    beyond,
                    "{kind:#x}/{phase}: circle never passed pillar {pillar:?}; stalled {longest_stall}ms at {:?}",
                    world.entities[&id].pos
                );
                assert!(
                    longest_stall < 1000,
                    "{kind:#x}/{phase}: stalled {longest_stall}ms"
                );
            }
        }
    }
}

#[test]
fn circle_entry_reanchors_after_each_phase_and_guard_duty() {
    for (slot, caller_slot, caller_phase) in [(0, 2, 3), (1, 0, 4), (2, 0, 3)] {
        let (mut world, _, bosses) = awakened_fixture();
        let id = bosses[slot];
        let anchor = world.entities[&id].tomb.as_ref().unwrap().anchor.unwrap();
        let center = world.entities[&anchor].pos;
        for phase in (0..=6).chain([weapons::SPECIAL_TWO, weapons::SPECIAL_THREE]) {
            world.tomb_enter(id, phase, false).unwrap();
            assert!(!world.entities[&id].tomb.as_ref().unwrap().circling);
            let pos = Pos {
                x: center.x + 2.,
                y: center.y - 3.,
            };
            world.position_entity(id, pos).unwrap();
            world.entities.get_mut(&id).unwrap().phase = 99.;
            assert!(matches!(
                world.tomb_movement(id, None).unwrap().0,
                Motion::Orbit { .. }
            ));
            assert_eq!(world.entities[&id].phase, (-3_f32).atan2(2.));
            world.entities.get_mut(&id).unwrap().phase = 1.;
            world.tomb_movement(id, None).unwrap();
            assert_eq!(world.entities[&id].phase, 1.);
        }
        world
            .tomb_enter(bosses[caller_slot], caller_phase, false)
            .unwrap();
        assert_eq!(world.tomb_guard(id), Some(bosses[caller_slot]));
        assert!(matches!(
            world.tomb_movement(id, None).unwrap().0,
            Motion::Protect { .. }
        ));
        assert!(!world.entities[&id].tomb.as_ref().unwrap().circling);
        world
            .position_entity(
                id,
                Pos {
                    x: center.x - 4.,
                    y: center.y + 2.,
                },
            )
            .unwrap();
        world.tomb_enter(bosses[caller_slot], 2, false).unwrap();
        assert!(matches!(
            world.tomb_movement(id, None).unwrap().0,
            Motion::Orbit { .. }
        ));
        assert_eq!(world.entities[&id].phase, 2_f32.atan2(-4.));
    }
}

#[test]
fn circle_clock_keeps_slow_and_paralysis_rules() {
    for kind in d::BOSSES {
        let (mut world, player, bosses) = awakened_fixture();
        let id = bosses[d::BOSSES.iter().position(|k| *k == kind).unwrap()];
        let anchor = world.entities[&id].tomb.as_ref().unwrap().anchor.unwrap();
        let center = world.entities[&anchor].pos;
        world
            .position_entity(
                id,
                Pos {
                    x: center.x + 5.,
                    y: center.y,
                },
            )
            .unwrap();
        world.players.get_mut(&player).unwrap().pos = center;
        world.tomb_movement(id, None).unwrap();
        for effect in [None, Some(condition::SLOWED), Some(condition::PARALYZED)] {
            world.entities.get_mut(&id).unwrap().statuses.clear();
            if let Some(effect) = effect {
                world.apply_condition(id, effect, 1000).unwrap();
            }
            let old = world.entities[&id].pos;
            let angle = world.entities[&id].phase;
            let orbit = world.entities[&id].orbit.unwrap();
            world.now += 50;
            world.npc_step();
            let e = &world.entities[&id];
            let fraction = match effect {
                Some(condition::PARALYZED) => 0.,
                Some(condition::SLOWED) => 0.5,
                _ => 1.,
            };
            let expected = orbit.speed / 1.5 * 0.05 * fraction;
            assert!((e.phase - angle - orbit.direction * expected / orbit.radius).abs() < 0.00001);
            assert!((old.distance(e.pos) - expected).abs() < 0.001);
            assert_eq!(
                e.orbit.unwrap().direction,
                if fraction == 0. {
                    -orbit.direction
                } else {
                    orbit.direction
                }
            );
        }
    }
}

#[test]
fn jackal_healers_keep_target_families_and_sick_guards_separate() {
    for (kind, targets) in [
        (0xd19, &[0xd16, 0xd17, 0xd18][..]),
        (0xd3b, &[0xd3a, 0xd3c][..]),
    ] {
        let (mut world, player) = fixture();
        let pos = world.players[&player].pos;
        let healer = world.spawn_entity(kind, pos, vec![]).unwrap();
        world.entities.get_mut(&healer).unwrap().activated = Some(world.now);
        let mut candidates = vec![(healer, false)];
        for candidate in [
            0xd16, 0xd17, 0xd18, 0xd19, 0xd2a, 0xd2b, 0xd3a, 0xd3b, 0xd3c,
        ] {
            let id = world.spawn_entity(candidate, pos, vec![]).unwrap();
            candidates.push((id, targets.contains(&candidate)));
        }
        let sick = world.spawn_entity(targets[0], pos, vec![]).unwrap();
        world.apply_condition(sick, condition::SICK, 1000).unwrap();
        candidates.push((sick, false));
        for &(id, _) in &candidates {
            world.entities.get_mut(&id).unwrap().hp -= 1;
        }
        world.tomb_mechanics(healer, None).unwrap();
        for (id, healed) in candidates {
            let entity = &world.entities[&id];
            assert_eq!(
                entity.hp,
                world.content.objects[&entity.kind].hp - i32::from(!healed),
                "healer {kind:#x}, recipient {:#x}, entity {id}",
                entity.kind
            );
        }
        assert_eq!(world.players[&player].character.tombs, 0);
    }
}

#[test]
fn each_generated_sarcophagus_owns_eight_worshippers() {
    let (world, _) = fixture();
    for sarcophagus in world.actors().filter(|e| e.kind == d::SARCOPHAGUS) {
        let guards: Vec<_> = world
            .actors()
            .filter(|e| {
                matches!(e.kind, 0xd2a | 0xd2b)
                    && e.tomb.as_ref().unwrap().anchor == Some(sarcophagus.id)
            })
            .collect();
        assert_eq!(guards.len(), 8, "sarcophagus at {:?}", sarcophagus.pos);
        assert_eq!(guards.iter().filter(|e| e.kind == 0xd2a).count(), 4);
        assert_eq!(guards.iter().filter(|e| e.kind == 0xd2b).count(), 4);
    }
}

#[test]
fn swarm_replacements_and_orphans_keep_their_original_master() {
    for kind in [0xd11, 0xd13, 0xd15] {
        let (mut world, player) = fixture();
        let pos = world.players[&player].pos;
        let masters = [0; 2].map(|_| world.spawn_entity(kind, pos, vec![]).unwrap());
        for master in masters {
            world.entities.get_mut(&master).unwrap().activated = Some(world.now);
            world.tomb_mechanics(master, None).unwrap();
        }
        let owned = |world: &World, master| -> std::collections::BTreeSet<_> {
            world.tomb_owned(master, kind - 1).into_iter().collect()
        };
        let original = owned(&world, masters[0]);
        let neighbors = owned(&world, masters[1]);
        assert!(!original.is_empty() && !neighbors.is_empty());
        assert!(original.is_disjoint(&neighbors));
        for &child in &original {
            world.entities.get_mut(&child).unwrap().activated = Some(world.now);
            world.tomb_mechanics(child, None).unwrap();
            assert!(matches!(
                world.tomb_movement(child, None).unwrap().0,
                Motion::Orbit { anchor: Anchor::BoundEntity(id), .. } if id == masters[0]
            ));
        }
        let victim = *original.first().unwrap();
        world
            .damage_enemy(player, victim, 1_000_000, true, &[], None)
            .unwrap();
        world.now = world.entities[&masters[0]]
            .tomb
            .as_ref()
            .unwrap()
            .next_spawn;
        world.tomb_mechanics(masters[0], None).unwrap();
        let replenished = owned(&world, masters[0]);
        assert_eq!(replenished.len(), original.len());
        assert!(!replenished.contains(&victim));
        assert_eq!(
            replenished.intersection(&original).count(),
            original.len() - 1
        );
        assert_eq!(owned(&world, masters[1]), neighbors);
        world
            .damage_enemy(player, masters[0], 1_000_000, true, &[], None)
            .unwrap();
        assert!(!world.entities.contains_key(&masters[0]));
        let replacement = world.spawn_entity(kind, pos, vec![]).unwrap();
        world.entities.get_mut(&replacement).unwrap().activated = Some(world.now);
        world.tomb_mechanics(replacement, None).unwrap();
        assert!(!owned(&world, replacement).is_empty());
        assert!(owned(&world, replacement).is_disjoint(&replenished));
        assert_eq!(owned(&world, masters[0]), replenished);
        assert_eq!(owned(&world, masters[1]), neighbors);
        for child in replenished {
            world
                .entities
                .get_mut(&child)
                .unwrap()
                .activated
                .get_or_insert(world.now);
            world.tomb_mechanics(child, Some(pos)).unwrap();
            let state = world.entities[&child].tomb.as_ref().unwrap();
            assert_eq!(
                (state.producer, state.anchor),
                (Some(masters[0]), Some(masters[0]))
            );
            assert!(matches!(
                world.tomb_movement(child, Some(pos)).unwrap().0,
                Motion::Chase { .. }
            ));
        }
        assert_eq!(world.players[&player].character.tombs, 0);
    }
}

#[test]
fn jackal_replacements_keep_their_original_producer() {
    let (mut world, player) = fixture();
    let lords: Vec<_> = world
        .actors()
        .filter(|e| e.kind == 0xd19)
        .map(|e| e.id)
        .take(2)
        .collect();
    assert_eq!(lords.len(), 2);
    for &lord in &lords {
        world.entities.get_mut(&lord).unwrap().activated = Some(world.now);
        world.tomb_mechanics(lord, None).unwrap();
    }
    let owned = |world: &World, lord| -> std::collections::BTreeSet<_> {
        (0xd16..=0xd18)
            .flat_map(|kind| world.tomb_owned(lord, kind))
            .collect()
    };
    let original = owned(&world, lords[0]);
    let neighbors = owned(&world, lords[1]);
    assert!(!original.is_empty() && !neighbors.is_empty());
    assert!(original.is_disjoint(&neighbors));
    let victim = *original.first().unwrap();
    world
        .damage_enemy(player, victim, 1_000_000, true, &[], None)
        .unwrap();
    world.now += 4000;
    world.tomb_mechanics(lords[0], None).unwrap();
    let replacements = owned(&world, lords[0]);
    assert_eq!(replacements.len(), original.len());
    assert!(!replacements.contains(&victim));
    assert_eq!(
        replacements.intersection(&original).count(),
        original.len() - 1
    );
    assert_eq!(owned(&world, lords[1]), neighbors);
    world
        .damage_enemy(player, lords[0], 1_000_000, true, &[], None)
        .unwrap();
    assert!(!world.entities.contains_key(&lords[0]));
    assert_eq!(owned(&world, lords[0]), replacements);
    assert_eq!(owned(&world, lords[1]), neighbors);
    assert_eq!(world.players[&player].character.tombs, 0);
}

#[test]
fn returning_worshippers_heal_only_their_own_room() {
    for kind in [0xd2a, 0xd2b] {
        let (mut world, player) = fixture();
        let sarcs: Vec<_> = world
            .actors()
            .filter(|e| e.kind == d::SARCOPHAGUS)
            .map(|e| e.id)
            .collect();
        let guards = |anchor| {
            world
                .actors()
                .filter(|e| {
                    e.tomb.as_ref().is_some_and(|b| b.anchor == Some(anchor))
                        && matches!(e.kind, 0xd2a | 0xd2b)
                })
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        let own = guards(sarcs[0]);
        let foreign = guards(sarcs[1])[0];
        let healer = *own
            .iter()
            .find(|&&id| world.entities[&id].kind == kind)
            .unwrap();
        let peer = *own
            .iter()
            .find(|&&id| world.entities[&id].kind != kind)
            .unwrap();
        let sick = *own.iter().find(|&&id| id != healer && id != peer).unwrap();
        let pos = world.entities[&sarcs[0]].pos;
        for id in [healer, peer, sick, foreign] {
            let e = world.entities.get_mut(&id).unwrap();
            e.pos = pos;
            e.hp = world.content.objects[&e.kind].hp - 100;
        }
        world.apply_condition(sick, condition::SICK, 1000).unwrap();
        let e = world.entities.get_mut(&healer).unwrap();
        e.stage = Some(2);
        e.activated = Some(world.now);
        world.tomb_mechanics(healer, None).unwrap();
        for (id, missing) in [(healer, 0), (peer, 0), (sick, 100), (foreign, 100)] {
            let e = &world.entities[&id];
            assert_eq!(e.hp, world.content.objects[&e.kind].hp - missing);
        }
        assert_eq!(
            world.entities[&foreign].tomb.as_ref().unwrap().anchor,
            Some(sarcs[1])
        );
        assert_eq!(world.players[&player].character.tombs, 0);
    }
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
        world.npc_step();
        let fired = world.bullets.iter().filter(|b| b.owner == target).count();
        assert!(fired > 0, "turret {kind:#x} must fire before the orb");
        world.now += 350;
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
        world.npc_step();
        assert_eq!(
            world.bullets.iter().filter(|b| b.owner == target).count() > fired,
            kind > 0xd33,
            "only center turret {kind:#x} may continue firing during Stasis"
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
