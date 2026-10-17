use super::{tests, *};

fn arena() -> World {
    let mut w = tests::world();
    w.encounter = true;
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut w.map).cells {
        cell.ground = 0x61;
    }
    w
}
fn center(w: &World) -> Pos {
    Pos {
        x: w.map.width as f32 / 2.,
        y: w.map.height as f32 / 2.,
    }
}

#[test]
fn red_demon_cannot_be_repeatedly_lured_away_from_its_original_fissure() {
    let mut w = arena();
    let home = center(&w);
    let id = w.spawn_entity(0x668, home, vec![]).unwrap();
    let p = w.enter(tests::ch(&w), None).unwrap();
    let mut previous = None;
    let mut returns = 0;
    for ms in (0..45000).step_by(50) {
        let pos = w.entities[&id].pos;
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: pos.x,
            y: pos.y + 9.,
        };
        w.now = ms;
        w.npc_step();
        let e = &w.entities[&id];
        assert_eq!((e.home.x, e.home.y), (home.x, home.y));
        // Five chase seconds at two tiles/second must not become a renewable pursuit.
        assert!(
            e.pos.distance(home) <= 11.1,
            "moving lure escaped the fissure at {ms}ms: {:?}",
            e.pos
        );
        if previous == Some(2) && e.stage == Some(0) {
            returns += 1;
            assert!((e.pos.x - home.x).abs() <= 1.);
            assert!((e.pos.y - home.y).abs() <= 1.);
        }
        previous = e.stage;
    }
    assert!(
        returns >= 2,
        "continued aggro prevented repeated home returns"
    );
}

#[test]
fn selected_boss_shields_summons_corners_and_cleanup_are_bounded() {
    let mut w = arena();
    let pos = center(&w);
    let stheno = w.spawn_entity(0x917, pos, vec![]).unwrap();
    w.encounter_mechanics(stheno, pos, 2000).unwrap();
    assert_ne!(
        w.entities[&stheno].conditions(w.now)
            & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    w.encounter_mechanics(stheno, pos, 6000).unwrap();
    assert_eq!(
        w.entities[&stheno].conditions(w.now)
            & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    w.entities.get_mut(&stheno).unwrap().hp = 5000;
    w.encounter_mechanics(stheno, pos, 6100).unwrap();
    w.encounter_mechanics(stheno, pos, 6150).unwrap();
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.kind == 0x919 && e.parent == Some(stheno))
            .count(),
        8
    );
    w.remove_entity(stheno);
    w.mechanic_expiry();
    assert!(w.entities.is_empty());

    let limon = w.spawn_entity(0xd06, pos, vec![]).unwrap();
    w.encounter_mechanics(limon, pos, 12300).unwrap();
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.parent == Some(limon))
            .count(),
        8
    );
    w.encounter_mechanics(limon, pos, 14000).unwrap();
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.parent == Some(limon))
            .count(),
        8
    );
    assert_ne!(
        w.entities[&limon].condition & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    for e in w.entities.values().filter(|e| e.parent == Some(limon)) {
        assert!((0xd07..=0xd0a).contains(&e.kind));
        assert_eq!(e.expires, Some(20000));
        assert_ne!(
            e.condition & crate::condition::bit(crate::condition::INVULNERABLE),
            0
        );
    }
    w.now = 20000;
    w.mechanic_expiry();
    assert_eq!(w.entities.len(), 1);
    w.remove_entity(limon);

    let septavius = w.spawn_entity(0xd90, pos, vec![]).unwrap();
    w.encounter_mechanics(septavius, pos, 0).unwrap();
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.parent == Some(septavius))
            .count(),
        9
    );
    w.encounter_mechanics(septavius, pos, 42000).unwrap();
    w.now += 1000;
    w.encounter_mechanics(septavius, pos, 43000).unwrap();
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.parent == Some(septavius))
            .count(),
        15
    );
    w.remove_entity(septavius);
    w.mechanic_expiry();
    assert!(w.entities.is_empty());
}

#[test]
fn grates_have_bounded_selected_children_and_existing_portal_sources_are_unchanged() {
    let mut w = arena();
    let pos = center(&w);
    let parent = w.spawn_entity(0x173, pos, vec![]).unwrap();
    for _ in 0..5 {
        w.encounter_mechanics(parent, pos, 0).unwrap();
        w.now += 5000;
    }
    let children: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.parent == Some(parent))
        .collect();
    assert_eq!(children.len(), 4);
    for child in children {
        crate::encounter::validate(&w.content, child.kind).unwrap();
        crate::loot::validate(&w.content, child.kind).unwrap();
    }
    w.remove_entity(parent);
    w.mechanic_expiry();
    assert!(w.entities.is_empty());
    for (enemy, portal, chance) in [
        (0x610, 0x717, 0.01),
        (0x654, 0x718, 0.17),
        (0x653, 0x70c, 0.11),
        (0x659, 0x71a, 0.17),
    ] {
        assert_eq!(crate::dungeon::portal_drop(enemy), Some((portal, chance)));
        assert!(
            crate::dungeon::DUNGEONS
                .iter()
                .any(|(kind, ..)| *kind == portal)
        );
    }
    assert_eq!(crate::dungeon::portal_drop(0x600), None);
}

#[test]
fn admitted_grenades_are_telegraphed_delayed_and_survive_owner_death() {
    let mut w = arena();
    let (tx, mut rx) = mpsc::channel(128);
    let (lost, _) = watch::channel(false);
    let player = w
        .enter(tests::ch(&w), Some(Output { packets: tx, lost }))
        .unwrap();
    let pos = w.players[&player].pos;
    let medusa = w.spawn_entity(0x654, pos, vec![]).unwrap();
    w.updates(player, false).unwrap();
    while rx.try_recv().is_ok() {}
    w.encounter_mechanics(medusa, pos, 0).unwrap();
    assert_eq!(rx.try_recv().unwrap().id, wire::SHOW_EFFECT);
    let hp = w.players[&player].character.hp;
    w.now = 1499;
    w.grenade_step();
    assert_eq!(w.players[&player].character.hp, hp);
    w.now = 1500;
    w.grenade_step();
    assert_eq!(rx.try_recv().unwrap().id, 68);
    assert!(w.players[&player].death_pending);
    assert_eq!(w.players[&player].character.hp, 0);
    assert_eq!(w.players[&player].killer, w.content.objects[&0x654].name);
    assert!(w.grenades.is_empty());

    let mut character = tests::ch(&w);
    character.id = 2;
    character.account = 2;
    let survivor = w.enter(character, None).unwrap();
    w.players.get_mut(&survivor).unwrap().pos = pos;
    w.now = 3000;
    w.encounter_mechanics(medusa, pos, 3000).unwrap();
    assert_eq!(w.grenades.len(), 1);
    w.remove_entity(medusa);
    w.now = 4500;
    w.grenade_step();
    assert!(w.grenades.is_empty());
    assert_eq!(w.players[&survivor].character.hp, 0);
    assert!(w.players[&survivor].death_pending);
    assert_eq!(w.players[&survivor].killer, w.content.objects[&0x654].name);
}

#[test]
fn slime_splits_are_selected_and_traps_burst_once_then_expire() {
    let mut w = arena();
    let pos = center(&w);
    let slime = w.spawn_entity(0xd9e, pos, vec![]).unwrap();
    let dead = w.remove_entity(slime);
    w.encounter_death(&dead).unwrap();
    assert_eq!(w.entities.values().filter(|e| e.kind == 0xd9f).count(), 6);
    let player = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&player).unwrap().pos = pos;
    let trap = w.spawn_entity(0xda2, pos, vec![]).unwrap();
    let shots = w.npc_step();
    let trap_shots: Vec<_> = shots.iter().filter(|(owner, _)| *owner == trap).collect();
    assert_eq!(trap_shots.len(), 1);
    let mut r = wire::Reader::new(&trap_shots[0].1);
    r.take(20).unwrap();
    assert_eq!(r.u8().unwrap(), 12);
    r.f32().unwrap();
    r.done().unwrap();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == trap).count(), 12);
    w.now += 50;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&trap));
}

#[test]
fn snake_oil_consumes_after_commit_heals_nearby_living_players_and_respects_sick_and_death() {
    let mut w = arena();
    let mut ch = tests::ch(&w);
    ch.items[4] = 0xa3f;
    ch.hp = 20;
    let a = w.enter(ch.clone(), None).unwrap();
    ch.account += 1;
    ch.id += 1;
    let b = w.enter(ch, None).unwrap();
    let slot = Slot {
        object: a,
        slot: 4,
        kind: 0xa3f,
    };
    let plan = w.use_item(a, slot, w.players[&a].pos).unwrap().unwrap();
    assert_eq!(w.players[&a].character.hp, 20);
    assert_eq!(w.players[&b].character.hp, 20);
    assert!(w.use_item(a, slot, w.players[&a].pos).is_err());
    w.apply_condition(b, crate::condition::SICK, 5000).unwrap();
    w.finish_inventory(plan, Ok(1)).unwrap();
    assert_eq!(w.players[&a].character.items[4], -1);
    assert_eq!(w.players[&a].character.hp, 100);
    assert_eq!(w.players[&b].character.hp, 20);
    w.players.get_mut(&a).unwrap().character.items[4] = 0xa3f;
    let plan = w.use_item(a, slot, w.players[&a].pos).unwrap().unwrap();
    w.players.get_mut(&a).unwrap().death_pending = true;
    w.players.get_mut(&a).unwrap().character.hp = 0;
    w.finish_inventory(plan, Ok(2)).unwrap();
    assert_eq!(w.players[&a].character.hp, 0);
}

#[test]
fn orbit_anchors_are_explicit_and_missing_parent_or_kind_is_an_error() {
    use crate::encounter::Anchor;
    let mut w = arena();
    let before = w.entities.len();
    assert!(w.loot_bag(center(&w), None, vec![0xa22; 9]).is_err());
    assert_eq!(w.entities.len(), before);
    let pos = center(&w);
    let queen = w.spawn_entity(0x917, pos, vec![]).unwrap();
    w.spawn_children(queen, 0x919, 1, 2.).unwrap();
    let child = w
        .entities
        .values()
        .find(|e| e.parent == Some(queen))
        .unwrap()
        .id;
    let player = Pos {
        x: pos.x + 5.,
        y: pos.y,
    };
    assert_eq!(
        w.orbit_anchor(child, Anchor::Parent, player).unwrap().x,
        pos.x
    );
    assert_eq!(
        w.orbit_anchor(child, Anchor::Kind(0x917), player)
            .unwrap()
            .x,
        pos.x
    );
    assert_eq!(
        w.orbit_anchor(child, Anchor::Player, player).unwrap().x,
        player.x
    );
    assert_eq!(
        w.orbit_anchor(child, Anchor::Home, player).unwrap().x,
        w.entities[&child].home.x
    );
    assert!(w.orbit_anchor(queen, Anchor::Parent, player).is_err());
    w.remove_entity(queen);
    assert!(w.orbit_anchor(child, Anchor::Parent, player).is_err());
    assert!(w.orbit_anchor(child, Anchor::Kind(0x917), player).is_err());
}
