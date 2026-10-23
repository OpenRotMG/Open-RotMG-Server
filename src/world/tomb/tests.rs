use super::*;

fn tomb() -> (World, i32) {
    let content = crate::world::tests::world().content;
    let map = Arc::new(d::generate(&content, 12351).unwrap());
    let mut w = World::new(content, map).unwrap();
    w.name = "Tomb of the Ancients".into();
    w.encounter = true;
    w.rng = rand::rngs::StdRng::seed_from_u64(12351);
    let player = w.enter(crate::world::tests::ch(&w), None).unwrap();
    (w, player)
}
fn awaken(w: &mut World, player: i32) -> [i32; 3] {
    let sarcs: Vec<_> = w
        .actors()
        .filter(|e| e.kind == d::SARCOPHAGUS)
        .map(|e| e.id)
        .collect();
    for id in sarcs {
        w.damage_enemy(player, id, 1000000, true, &[], None)
            .unwrap();
    }
    d::BOSSES.map(|kind| w.actors().find(|e| e.kind == kind).unwrap().id)
}
fn activate(w: &mut World, id: i32) {
    w.entities.get_mut(&id).unwrap().activated = Some(w.now);
    w.tomb_mechanics(id, None).unwrap();
}

#[test]
fn generated_realm_earns_one_reserved_sphinx_and_credits_its_real_death() {
    let mut directory = crate::world::tests::directory();
    let realm = directory.endgame.active;
    let world = directory.worlds.get_mut(&realm).unwrap();
    let sphinx: Vec<_> = world
        .event_reserves
        .iter()
        .filter(|event| event.root.kind == d::SPHINX)
        .collect();
    assert_eq!(sphinx.len(), 1);
    let id = sphinx[0].root.id;
    assert_eq!(sphinx[0].root.hp, world.content.objects[&d::SPHINX].hp);
    assert_eq!(sphinx[0].root.quest, Some(id));
    assert!(!world.entities.contains_key(&id));
    assert!(!world.actor_ids.contains(&id));
    let walls: Vec<_> = world.actors().filter(|e| e.kind == d::WALL).collect();
    assert_eq!(walls.len(), 24);
    for wall in walls {
        let connection = wall
            .extra_stats
            .iter()
            .find(|(stat, _)| *stat == 41)
            .unwrap()
            .1;
        assert!(
            connection
                .to_be_bytes()
                .into_iter()
                .all(|side| matches!(side, 1 | 2))
        );
        assert_eq!(
            world.map.cell(wall.pos.x, wall.pos.y).unwrap().config,
            format!("conn:{connection}")
        );
    }
    assert!(crate::quest::root(d::SPHINX));
    let position = Pos {
        x: world.map.spawns[0].0,
        y: world.map.spawns[0].1,
    };
    let player = world.enter(crate::world::tests::ch(world), None).unwrap();
    let source = world
        .actors()
        .find(|e| e.kind == 0x668)
        .expect("generated Realm has a qualifying Red Demon")
        .id;
    world
        .damage_enemy(player, source, 100000, false, &[], None)
        .unwrap();
    let owned = world
        .actors()
        .filter(|e| e.parent.is_some() || e.quest.is_some())
        .count()
        + world.named_births.len();
    let clear: Vec<_> = world
        .event_reserves
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            owned + 1 + event.children.len() <= 256 && event.root.pos.distance(position) >= 32.
        })
        .map(|(index, _)| index)
        .collect();
    let selected = world
        .event_reserves
        .iter()
        .position(|event| event.root.id == id)
        .unwrap();
    let seed = (0..1024)
        .find(|seed| {
            let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
            rng.gen_bool(0.1) && clear.choose(&mut rng) == Some(&selected)
        })
        .expect("bounded fixture can earn its Sphinx through the shared director");
    world.rng = rand::rngs::StdRng::seed_from_u64(seed);
    directory.endgame_step();
    let world = directory.worlds.get_mut(&realm).unwrap();
    assert_eq!(world.actors().filter(|e| e.kind == d::SPHINX).count(), 1);
    assert_eq!(world.entities[&id].hp, world.content.objects[&d::SPHINX].hp);
    assert!(
        !world
            .event_reserves
            .iter()
            .any(|event| event.root.kind == d::SPHINX)
    );
    world.players.get_mut(&player).unwrap().pos = world.entities[&id].pos;
    world
        .damage_enemy(player, id, 1000000, true, &[], None)
        .unwrap();
    assert_eq!(
        world
            .damage_enemy(player, id, 1000000, true, &[], None)
            .unwrap(),
        0
    );
    assert_eq!(
        world
            .critical_deaths
            .iter()
            .filter(|root| **root == id)
            .count(),
        1
    );
    assert_eq!(world.portal_requests.len(), 1);
    directory.step();
    assert_eq!(
        directory.worlds[&realm]
            .actors()
            .filter(|e| e.kind == d::SPHINX)
            .count(),
        0
    );
    assert_eq!(
        directory
            .portals
            .iter()
            .filter(|(id, p)| p.origin == realm
                && directory.worlds[&realm].entities[id].kind == d::PORTAL)
            .count(),
        1
    );
}

#[test]
fn prepared_sphinx_death_opens_one_shared_portal_and_tomb_instances_are_independent() {
    let mut directory = crate::world::tests::directory();
    let realm = directory.endgame.active;
    let world = directory.worlds.get_mut(&realm).unwrap();
    // Realm progression owns natural Sphinx activation; this fixture tests only Tomb admission.
    let (x, y) = world.map.spawns[0];
    let position = Pos { x, y };
    let sphinx = world.spawn_entity(d::SPHINX, position, vec![]).unwrap();
    let mut character = crate::world::tests::ch(world);
    character.base[0] = 670;
    character.hp = 670;
    let player = world.enter(character.clone(), None).unwrap();
    world.players.get_mut(&player).unwrap().pos = position;
    world
        .damage_enemy(player, sphinx, 1000000, true, &[], None)
        .unwrap();
    assert_eq!(world.portal_requests.len(), 1);
    assert_eq!(
        world
            .damage_enemy(player, sphinx, 1000000, true, &[], None)
            .unwrap(),
        0
    );
    directory.step();
    let portals: Vec<_> = directory
        .portals
        .iter()
        .filter(|(id, portal)| {
            portal.origin == realm && directory.worlds[&realm].entities[id].kind == d::PORTAL
        })
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(portals.len(), 1);
    let (_, _, first) = directory.destination(player, Some(portals[0])).unwrap();
    character.id += 1;
    character.account += 1;
    let world = directory.worlds.get_mut(&realm).unwrap();
    let second_player = world.enter(character, None).unwrap();
    world.players.get_mut(&second_player).unwrap().pos = position;
    assert_eq!(
        directory
            .destination(second_player, Some(portals[0]))
            .unwrap()
            .2,
        first
    );
    let second = directory.instance("tomb-of-the-ancients", None).unwrap();
    assert_ne!(first, second);
    assert_eq!(directory.worlds[&first].name, "Tomb of the Ancients");
    for id in [first, second] {
        let world = &directory.worlds[&id];
        assert!(world.actors().any(|e| e.kind == d::SARCOPHAGUS));
        assert_eq!(
            world
                .actors()
                .filter(|e| d::STATUES.contains(&e.kind))
                .count(),
            3
        );
    }
    assert!(
        directory.worlds[&first]
            .entities
            .keys()
            .all(|id| !directory.worlds[&second].entities.contains_key(id))
    );
}

#[test]
fn objective_tincture_rewards_are_owned_and_created_after_actual_death() {
    let (mut w, player) = tomb();
    let seed = (0..256)
        .find(|seed| {
            crate::loot::rewards(
                &w.content,
                d::SARCOPHAGUS,
                &mut rand::rngs::StdRng::seed_from_u64(*seed),
            )
            .iter()
            .any(|kind| w.content.objects[kind].soulbound)
        })
        .expect("bounded fixture must select a soulbound tincture roll");
    w.rng = rand::rngs::StdRng::seed_from_u64(seed);
    let sarc = w.actors().find(|e| e.kind == d::SARCOPHAGUS).unwrap().id;
    let account = w.players[&player].character.account;
    w.damage_enemy(player, sarc, 1000000, true, &[], None)
        .unwrap();
    assert!(!w.entities.contains_key(&sarc));
    assert!(
        w.entities.values().any(|e| {
            e.extra_stats.contains(&(54, account))
                && e.items.iter().any(|item| matches!(item, 0xb0c..=0xb0e))
        }),
        "actual Sarcophagus death must create an accessible owned tincture bag"
    );
}

#[test]
fn tomb_private_loot_uses_explicit_boss_and_room_damage_boundaries() {
    let (mut w, player) = tomb();
    let sarc = w
        .actors()
        .find(|e| e.kind == d::SARCOPHAGUS)
        .unwrap()
        .clone();
    let bosses = awaken(&mut w, player);
    let treasure = w.actors().find(|e| e.kind == d::TREASURE).unwrap().id;
    let defeated: Vec<_> = bosses
        .into_iter()
        .chain([treasure])
        .map(|id| w.entities[&id].clone())
        .chain([sarc])
        .collect();
    for mut defeated in defeated {
        let threshold = if d::BOSSES.contains(&defeated.kind) {
            100
        } else {
            w.content.objects[&defeated.kind].hp / 100
        };
        let character = w.players[&player].character.id;
        let before = w.bag_expiry.len();
        defeated.damage.insert(character, threshold);
        w.drop_loot(&defeated).unwrap();
        assert_eq!(w.bag_expiry.len(), before);
        defeated.damage.insert(character, threshold + 1);
        for _ in 0..64 {
            w.drop_loot(&defeated).unwrap();
            if w.bag_expiry.len() > before {
                break;
            }
        }
        assert!(w.bag_expiry.len() > before, "kind {:#x}", defeated.kind);
    }
}

#[test]
fn tomb_boss_death_uses_two_shared_ordered_rolls_not_a_life_for_every_raider() {
    let (mut w, player) = tomb();
    let peers = awaken(&mut w, player);
    let mut players = vec![player];
    for n in 1..=2 {
        let mut character = crate::world::tests::ch(&w);
        character.id += n;
        character.account += n;
        players.push(w.enter(character, None).unwrap());
    }
    let before: BTreeSet<_> = w.bag_expiry.keys().copied().collect();
    w.rng = rand::rngs::StdRng::seed_from_u64(12351);
    w.damage_enemy(players[1], peers[0], 2000, true, &[], None)
        .unwrap();
    w.damage_enemy(players[2], peers[0], 3000, true, &[], None)
        .unwrap();
    w.damage_enemy(player, peers[0], 1000000, true, &[], None)
        .unwrap();
    let bags: Vec<_> = w
        .bag_expiry
        .keys()
        .filter(|id| !before.contains(id))
        .map(|id| &w.entities[id])
        .collect();
    let owners: BTreeSet<_> = bags
        .iter()
        .flat_map(|e| {
            e.extra_stats
                .iter()
                .filter(|(stat, _)| *stat == 54)
                .map(|(_, owner)| *owner)
        })
        .collect();
    assert_eq!(
        owners,
        BTreeSet::from([
            w.players[&player].character.account,
            w.players[&players[2]].character.account
        ])
    );
    assert_eq!(
        bags.iter()
            .flat_map(|e| &e.items)
            .filter(|item| **item >= 0)
            .count(),
        2
    );
}

#[test]
fn zero_hp_tomb_infrastructure_survives_hits_without_erasing_turret_status_susceptibility() {
    let (mut w, player) = tomb();
    let lifetime = w.players[&player].character.lifetime.clone();
    let mut ids: Vec<_> = w
        .actors()
        .filter(|e| d::immortal(e.kind))
        .map(|e| e.id)
        .collect();
    for kind in 0xd31..=0xd36 {
        ids.push(
            w.spawn_entity(kind, Pos { x: 82.5, y: 82.5 }, vec![])
                .unwrap(),
        );
    }
    assert!(ids.iter().any(|id| w.entities[id].hp == 0));
    for id in ids {
        let (kind, hp) = (w.entities[&id].kind, w.entities[&id].hp);
        assert_eq!(
            w.damage_enemy(
                player,
                id,
                1000000,
                true,
                &[(condition::STUNNED, 1000)],
                None
            )
            .unwrap(),
            0
        );
        assert!(
            w.entities.contains_key(&id),
            "indestructible {kind:#x} disappeared with selected HP {hp}"
        );
        assert_eq!(w.entities[&id].hp, hp);
        if (0xd31..=0xd36).contains(&kind) {
            assert_ne!(
                w.entities[&id].conditions(w.now) & condition::bit(condition::STUNNED),
                0
            );
        }
    }
    assert_eq!(w.players[&player].character.kills, 0);
    assert_eq!(w.players[&player].character.lifetime, lifetime);
    assert!(!w.cleared);
}

#[test]
fn objectives_not_treasure_awaken_all_three_correct_bosses() {
    let (mut w, player) = tomb();
    let sarcs: Vec<_> = w
        .actors()
        .filter(|e| e.kind == d::SARCOPHAGUS)
        .map(|e| e.id)
        .collect();
    assert!(!sarcs.is_empty());
    for (n, id) in sarcs.iter().enumerate() {
        w.damage_enemy(player, *id, 1000000, true, &[], None)
            .unwrap();
        if n + 1 < sarcs.len() {
            assert_eq!(
                w.actors().filter(|e| d::STATUES.contains(&e.kind)).count(),
                3
            );
            assert!(!w.actors().any(|e| d::BOSSES.contains(&e.kind)));
        }
    }
    assert!(!w.actors().any(|e| d::STATUES.contains(&e.kind)));
    let bosses: Vec<_> = w.actors().filter(|e| d::BOSSES.contains(&e.kind)).collect();
    assert_eq!(bosses.len(), 3);
    for e in bosses {
        assert_eq!(e.hp, w.content.objects[&e.kind].hp);
        let b = e.tomb.as_ref().unwrap();
        assert_eq!(b.peers.unwrap().map(|id| w.entities[&id].kind), d::BOSSES);
        assert_eq!(w.entities[&b.anchor.unwrap()].kind, d::ANCHOR);
    }
    assert_eq!(
        w.actors().find(|e| e.kind == d::TREASURE).unwrap().hp,
        50000
    );
    assert!(!w.cleared);
}

#[test]
fn shields_special_window_armor_break_and_artifact_skip_use_real_world_state() {
    let (mut w, player) = tomb();
    let peers = awaken(&mut w, player);
    for id in peers {
        activate(&mut w, id);
    }
    let nut = peers[0];
    w.entities.get_mut(&nut).unwrap().hp = 60000;
    w.tomb_enter(nut, 3, true).unwrap();
    w.now = 4999;
    w.tomb_mechanics(nut, None).unwrap();
    assert!(condition::protected(w.entities[&nut].conditions(w.now)));
    w.now = 5000;
    w.tomb_mechanics(nut, None).unwrap();
    assert!(!condition::protected(w.entities[&nut].conditions(w.now)));
    w.now = 19999;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&nut].stage, Some(3));
    w.now = 20000;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&nut].stage, Some(weapons::SPECIAL_TWO));
    assert_ne!(
        w.entities[&nut].conditions(w.now) & condition::bit(condition::ARMOR_BROKEN),
        0
    );
    w.now = 23000;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(
        w.entities[&nut].conditions(w.now) & condition::bit(condition::ARMOR_BROKEN),
        0
    );
    w.entities.get_mut(&nut).unwrap().hp = 4000;
    w.now = 23050;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&nut].stage, Some(weapons::RAGE));
    assert!(!w.entities[&nut].tomb.as_ref().unwrap().artifacts_unlocked);
    assert!(!w.actors().any(|e| (0xd1c..=0xd24).contains(&e.kind)));
    assert_eq!(w.tomb_guard(peers[2]), None);
}

#[test]
fn bomb_switch_requires_uninterrupted_occupancy_and_only_disables_its_room() {
    let (mut w, player) = tomb();
    let switches: Vec<_> = w
        .actors()
        .filter(|e| e.kind == d::SWITCH)
        .map(|e| e.id)
        .collect();
    assert!(switches.len() >= 2);
    let switch = switches[0];
    w.players.get_mut(&player).unwrap().pos = w.entities[&switch].pos;
    w.now = 100;
    activate(&mut w, switch);
    w.now = 2100;
    w.tomb_mechanics(switch, None).unwrap();
    assert_eq!(w.entities[&switch].stage, Some(0));
    w.suspend_npc(switch, 50);
    assert_eq!(w.entities[&switch].tomb.as_ref().unwrap().press_since, None);
    w.now = 3100;
    w.tomb_mechanics(switch, None).unwrap();
    w.now = 6099;
    w.tomb_mechanics(switch, None).unwrap();
    assert_eq!(w.entities[&switch].stage, Some(0));
    w.now = 6100;
    w.tomb_mechanics(switch, None).unwrap();
    assert_eq!(w.entities[&switch].stage, Some(1));
    assert!(
        !w.actors()
            .any(|e| e.kind == d::TRAP && e.tomb.as_ref().unwrap().anchor == Some(switch))
    );
    assert!(
        w.actors()
            .any(|e| e.kind == d::TRAP && e.tomb.as_ref().unwrap().anchor == Some(switches[1]))
    );
    assert!(w.entities[&switch].extra_stats.contains(&(61, 1)));
}

#[test]
fn orphan_artifacts_survive_and_never_rebind_to_another_boss() {
    let (mut w, player) = tomb();
    let peers = awaken(&mut w, player);
    let owner = peers[0];
    let child = w
        .tomb_spawn(
            Some(owner),
            w.entities[&owner].pos,
            0xd1c,
            1,
            0.,
            Some(owner),
        )
        .unwrap()[0];
    activate(&mut w, child);
    w.damage_enemy(player, owner, 1000000, true, &[], None)
        .unwrap();
    w.mechanic_expiry();
    assert!(w.entities.contains_key(&child));
    let replacement = w
        .insert_entity(d::NUT, w.entities[&child].pos, vec![])
        .unwrap();
    assert_ne!(replacement, owner);
    let target = w.entities[&child].pos;
    assert!(matches!(
        w.tomb_movement(child, Some(target)).unwrap().0,
        Motion::Chase { .. }
    ));
    assert_eq!(
        w.entities[&child].tomb.as_ref().unwrap().anchor,
        Some(owner)
    );
}

#[test]
fn worshipper_recovery_does_not_heal_its_sarcophagus_or_dying_peers() {
    let (mut w, _) = tomb();
    let priest = w.actors().find(|e| e.kind == 0xd2a).unwrap().id;
    let anchor = w.entities[&priest].tomb.as_ref().unwrap().anchor.unwrap();
    let position = w.entities[&anchor].pos;
    activate(&mut w, priest);
    let dying = w
        .actors()
        .find(|e| {
            e.id != priest && e.kind == 0xd2a && e.tomb.as_ref().unwrap().anchor == Some(anchor)
        })
        .unwrap()
        .id;
    w.entities.get_mut(&anchor).unwrap().hp = 14000;
    w.entities.get_mut(&priest).unwrap().hp = 2000;
    w.now = 1000;
    w.tomb_mechanics(priest, None).unwrap();
    assert_eq!(w.entities[&priest].stage, Some(1));
    assert_eq!(w.entities[&anchor].hp, 14000);
    w.position_entity(priest, position).unwrap();
    w.position_entity(dying, position).unwrap();
    w.entities.get_mut(&dying).unwrap().stage = Some(5);
    w.entities.get_mut(&dying).unwrap().hp = 1;
    w.now = 1050;
    w.tomb_mechanics(priest, None).unwrap();
    assert_eq!(w.entities[&priest].stage, Some(2));
    assert_eq!(w.entities[&priest].hp, 8000);
    assert_eq!(w.entities[&dying].hp, 1);
    assert_eq!(w.entities[&anchor].hp, 14000);
    w.now = 6050;
    w.tomb_mechanics(priest, None).unwrap();
    assert_eq!(w.entities[&priest].stage, Some(3));
    assert_eq!(w.entities[&priest].size, 120);
    assert_eq!(w.entities[&anchor].hp, 15000);
    w.position_entity(
        priest,
        Pos {
            x: position.x + 11.,
            y: position.y,
        },
    )
    .unwrap();
    w.entities.get_mut(&anchor).unwrap().hp = 14000;
    w.now = 7050;
    w.tomb_mechanics(priest, None).unwrap();
    assert_eq!(w.entities[&anchor].hp, 14000);
}

#[test]
fn worshipper_death_has_two_second_chase_then_one_burst_and_one_credit() {
    for (death_pending, departing) in [(false, false), (true, false), (false, true)] {
        let (mut w, player) = tomb();
        let priest = w.actors().find(|e| e.kind == 0xd2a).unwrap().id;
        let definition = w.content.objects[&0xd2a].clone();
        activate(&mut w, priest);
        w.damage_enemy(player, priest, 1000000, true, &[], None)
            .unwrap();
        assert_eq!(w.entities[&priest].stage, Some(4));
        assert_eq!(w.players[&player].character.kills, 0);
        assert_eq!(w.players[&player].character.lifetime.kills, 0);
        assert_eq!(
            w.damage_enemy(player, priest, 1000000, true, &[], None)
                .unwrap(),
            0
        );
        w.players.get_mut(&player).unwrap().death_pending = death_pending;
        w.players.get_mut(&player).unwrap().departing = departing;
        w.now = 1999;
        w.tomb_mechanics(priest, None).unwrap();
        assert!(w.entities.contains_key(&priest));
        w.now = 2000;
        w.tomb_mechanics(priest, None).unwrap();
        let burst = w.npc_attack_schedule(priest, 0).unwrap().unwrap();
        assert_eq!(burst.attacks.len(), 1);
        assert_eq!(burst.attacks[0].count, 9);
        w.now = 2050;
        w.tomb_mechanics(priest, None).unwrap();
        assert!(!w.entities.contains_key(&priest));
        assert_eq!(
            w.damage_enemy(player, priest, 1000000, true, &[], None)
                .unwrap(),
            0
        );
        let kills = u64::from(!death_pending && !departing);
        assert_eq!(w.players[&player].character.kills, kills);
        let lifetime = &w.players[&player].character.lifetime;
        assert_eq!(lifetime.kills, kills);
        assert_eq!(lifetime.god_kills, kills * u64::from(definition.god));
        assert_eq!(lifetime.cube_kills, kills * u64::from(definition.cube));
        assert_eq!(lifetime.oryx_kills, 0);
    }
}

#[test]
fn three_boss_clear_uses_final_death_range_and_waits_for_durable_progress() {
    let (mut w, player) = tomb();
    let slot = crate::dungeon::DUNGEONS
        .iter()
        .position(|(portal, _, _, _)| *portal == d::PORTAL)
        .unwrap();
    w.players.get_mut(&player).unwrap().character.jungles = 2;
    w.players.get_mut(&player).unwrap().character.manors = 3;
    w.players.get_mut(&player).unwrap().character.beaches = 4;
    w.players.get_mut(&player).unwrap().character.davy_lockers = 5;
    let peers = awaken(&mut w, player);
    let position = Pos { x: 16.5, y: 16.5 };
    w.players.get_mut(&player).unwrap().pos = position;
    for (n, id) in peers.into_iter().enumerate() {
        if n == 2 {
            w.position_entity(id, position).unwrap();
        }
        w.damage_enemy(player, id, 1000000, true, &[], None)
            .unwrap();
        assert_eq!(w.players[&player].clears_queued[slot], u64::from(n == 2));
        assert_eq!(w.cleared, n == 2);
    }
    assert!(!w.actors().any(|e| e.kind == d::ANCHOR));
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    assert_eq!(w.entities[&exit].kind, 0x704);
    assert_eq!(w.players[&player].character.tombs, 0);
    let plans = w.pending_progress_where(|_, _| true);
    assert_eq!(plans.len(), 1);
    let (owner, snapshot) = plans.into_iter().next().unwrap();
    assert_eq!(snapshot.tombs, 1);
    assert_eq!(snapshot.davy_lockers, 5);
    assert_eq!(snapshot.jungles, 2);
    assert_eq!(snapshot.manors, 3);
    assert_eq!(snapshot.beaches, 4);
    w.finish_progress(owner, snapshot).unwrap();
    assert!(w.entities[&exit].published);
    assert_eq!(w.players[&player].character.tombs, 1);
    assert!(!w.entities.values().any(|e| e.kind == d::EXIT));
    assert_eq!(
        w.damage_enemy(player, peers[2], 1000000, true, &[], None)
            .unwrap(),
        0
    );
    assert_eq!(w.players[&player].clears_queued[slot], 0);
}

#[test]
fn mummy_replenishment_and_death_leave_bounded_live_scarabs() {
    let (mut w, player) = tomb();
    let mummy = w
        .spawn_entity(0xd1a, Pos { x: 82.5, y: 82.5 }, vec![])
        .unwrap();
    activate(&mut w, mummy);
    let initial = w.tomb_owned(mummy, 0xd1b);
    assert_eq!(initial.len(), 5);
    w.now = 5000;
    w.tomb_mechanics(mummy, None).unwrap();
    assert_eq!(w.tomb_owned(mummy, 0xd1b), initial);
    w.damage_enemy(player, mummy, 1000000, true, &[], None)
        .unwrap();
    assert!(!w.entities.contains_key(&mummy));
    assert_eq!(w.players[&player].character.lifetime.kills, 1);
    let surviving = w.tomb_owned(mummy, 0xd1b);
    assert!((10..=13).contains(&surviving.len()));
    assert!(initial.iter().all(|id| surviving.contains(id)));
    assert!(surviving.iter().all(|id| w.entities[id].parent.is_none()));
    for child in surviving {
        activate(&mut w, child);
        assert!(w.entities.contains_key(&child));
    }
    let position = Pos { x: 84.5, y: 82.5 };
    let mummy = w.spawn_entity(0xd1a, position, vec![]).unwrap();
    activate(&mut w, mummy);
    w.damage_enemy(player, mummy, 1, true, &[], None).unwrap();
    w.tomb_mechanics(
        mummy,
        Some(Pos {
            x: position.x + 2.,
            y: position.y,
        }),
    )
    .unwrap();
    assert_eq!(w.entities[&mummy].stage, Some(0));
    w.tomb_mechanics(
        mummy,
        Some(Pos {
            x: position.x + 1.99,
            y: position.y,
        }),
    )
    .unwrap();
    assert_eq!(w.entities[&mummy].stage, Some(1));
    let burst = w.npc_attack_schedule(mummy, 0).unwrap().unwrap();
    assert_eq!(burst.attacks.len(), 1);
    assert_eq!(burst.attacks[0].count, 36);
    w.now = 5049;
    w.tomb_mechanics(mummy, None).unwrap();
    assert!(w.entities.contains_key(&mummy));
    assert_eq!(w.players[&player].character.lifetime.kills, 1);
    w.now = 5050;
    w.tomb_mechanics(mummy, None).unwrap();
    assert!(!w.entities.contains_key(&mummy));
    assert_eq!(w.players[&player].character.kills, 2);
    assert_eq!(
        w.damage_enemy(player, mummy, 1000000, true, &[], None)
            .unwrap(),
        0
    );
    let lifetime = &w.players[&player].character.lifetime;
    assert_eq!(lifetime.kills, 2);
    assert_eq!(
        (lifetime.god_kills, lifetime.cube_kills, lifetime.oryx_kills),
        (0, 0, 0)
    );
    assert!((10..=13).contains(&w.tomb_owned(mummy, 0xd1b).len()));
}

#[test]
fn treasure_spawns_eight_numbered_jars_once_and_each_death_leaves_a_scarab() {
    let (mut w, player) = tomb();
    let producer = w.actors().find(|e| e.kind == d::TREASURE).unwrap().id;
    activate(&mut w, producer);
    w.damage_enemy(player, producer, 600, true, &[], None)
        .unwrap();
    let origin = w.entities[&producer].pos;
    for n in 0..8 {
        w.now = 1000 + n * 1000;
        w.tomb_mechanics(producer, None).unwrap();
        assert_eq!(
            w.actors()
                .filter(|e| (0xd2d..=0xd30).contains(&e.kind))
                .count(),
            n as usize + 1
        );
    }
    for kind in 0xd2d..=0xd30 {
        let jars = w.tomb_owned(producer, kind);
        assert_eq!(jars.len(), 2);
        for jar in jars {
            assert!((w.entities[&jar].pos.distance(origin) - 5.).abs() < 0.001);
            w.damage_enemy(player, jar, 1000000, true, &[], None)
                .unwrap();
            let children = w.tomb_owned(jar, 0xd1b);
            assert_eq!(children.len(), 1);
            assert!(w.entities[&children[0]].parent.is_none());
            activate(&mut w, children[0]);
            assert!(w.entities.contains_key(&children[0]));
        }
    }
    w.now = 10000;
    w.tomb_mechanics(producer, None).unwrap();
    assert!(!w.actors().any(|e| (0xd2d..=0xd30).contains(&e.kind)));
    assert!(!w.cleared);
}

#[test]
fn nut_heals_only_its_exact_nearby_peers_once_per_second_and_respects_sick() {
    let (mut w, player) = tomb();
    let peers = awaken(&mut w, player);
    for id in peers {
        activate(&mut w, id);
    }
    let [nut, geb, bes] = peers;
    let origin = w.entities[&nut].pos;
    for id in peers {
        w.entities.get_mut(&id).unwrap().hp -= 1000;
    }
    w.position_entity(
        geb,
        Pos {
            x: origin.x + 1.,
            y: origin.y,
        },
    )
    .unwrap();
    w.position_entity(
        bes,
        Pos {
            x: origin.x + 2.,
            y: origin.y,
        },
    )
    .unwrap();
    w.entities
        .get_mut(&bes)
        .unwrap()
        .statuses
        .insert(condition::SICK, (0, 5000));
    let hp = peers.map(|id| w.entities[&id].hp);
    w.now = 999;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(peers.map(|id| w.entities[&id].hp), hp);
    w.now = 1000;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(
        peers.map(|id| w.entities[&id].hp),
        [hp[0], hp[1] + 100, hp[2]]
    );
    w.now = 1001;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&geb].hp, hp[1] + 100);
    w.position_entity(
        geb,
        Pos {
            x: origin.x + 6.,
            y: origin.y,
        },
    )
    .unwrap();
    w.now = 2000;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&geb].hp, hp[1] + 100);
    w.position_entity(
        geb,
        Pos {
            x: origin.x + 1.,
            y: origin.y,
        },
    )
    .unwrap();
    w.entities.get_mut(&nut).unwrap().hp = 5000;
    w.now = 3000;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&geb].hp, hp[1] + 200);
    w.entities.get_mut(&nut).unwrap().hp = 4999;
    w.now = 4000;
    w.tomb_mechanics(nut, None).unwrap();
    assert_eq!(w.entities[&geb].hp, hp[1] + 200);
}

#[test]
fn artifacts_keep_numbered_peer_anchors_and_only_replenish_missing_types_on_deadline() {
    let (mut w, player) = tomb();
    let peers = awaken(&mut w, player);
    for id in peers {
        activate(&mut w, id);
        w.entities.get_mut(&id).unwrap().hp = 35000;
        w.tomb_enter(id, 5, true).unwrap();
    }
    w.now = 5000;
    for id in peers {
        w.tomb_mechanics(id, None).unwrap();
    }
    for (owner, base) in peers.into_iter().zip([0xd1c, 0xd1f, 0xd22]) {
        for (number, anchor) in [peers[2], peers[1], peers[0]].into_iter().enumerate() {
            let children = w.tomb_owned(owner, base + number as u16);
            assert_eq!(children.len(), 1);
            assert_eq!(
                w.entities[&children[0]].tomb.as_ref().unwrap().anchor,
                Some(anchor)
            );
        }
    }
    let owner = peers[0];
    let removed = w.tomb_owned(owner, 0xd1c)[0];
    let retained = [w.tomb_owned(owner, 0xd1d)[0], w.tomb_owned(owner, 0xd1e)[0]];
    w.damage_enemy(player, removed, 1000000, true, &[], None)
        .unwrap();
    w.now = 8999;
    w.tomb_mechanics(owner, None).unwrap();
    assert!(w.tomb_owned(owner, 0xd1c).is_empty());
    w.now = 9000;
    w.tomb_mechanics(owner, None).unwrap();
    let replacements = w.tomb_owned(owner, 0xd1c);
    assert_eq!(replacements.len(), 1);
    assert_ne!(replacements[0], removed);
    assert!(retained.iter().all(|id| w.entities.contains_key(id)));
    assert_eq!(
        w.entities[&replacements[0]].tomb.as_ref().unwrap().anchor,
        Some(peers[2])
    );
}
