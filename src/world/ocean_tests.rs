use super::*;
use crate::{condition, dungeon::trench};
use rand::{Rng, SeedableRng, rngs::StdRng};

fn arena(kind: u16) -> (World, i32, i32) {
    let content = tests::world().content;
    let map = Map {
        width: 80,
        height: 80,
        cells: vec![
            crate::content::Cell {
                ground: 0x2e,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            6400
        ],
        spawns: vec![(34.5, 32.5)],
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.name = "Ocean Trench".into();
    w.encounter = true;
    let p = w.enter(tests::ch(&w), None).unwrap();
    let e = w
        .spawn_entity(kind, Pos { x: 32.5, y: 32.5 }, vec![])
        .unwrap();
    (w, p, e)
}

#[test]
fn oxygen_is_private_bounded_server_time_and_suffocation_uses_permanent_death() {
    let (mut w, p, _) = arena(trench::VENT);
    assert_eq!(w.players[&p].oxygen, 100);
    for _ in 0..50 {
        w.now += 150;
        w.ocean_step();
    }
    assert_eq!(w.players[&p].oxygen, 0);
    assert_eq!(w.players[&p].character.hp, 100);
    w.now += 150;
    w.ocean_step();
    assert_eq!(w.players[&p].character.hp, 90);
    let pos = w.players[&p].pos;
    w.spawn_entity(trench::VENT, pos, vec![]).unwrap();
    for _ in 0..20 {
        w.now += 150;
        w.ocean_step();
    }
    assert_eq!(w.players[&p].oxygen, 100);
    let decode = |data: Vec<u8>| {
        let mut r = wire::Reader::new(&data);
        r.i32().unwrap();
        r.f32().unwrap();
        r.f32().unwrap();
        let n = r.i16().unwrap();
        let mut oxygen = None;
        for _ in 0..n {
            let k = r.u8().unwrap();
            if matches!(k, 31 | 62) {
                r.utf().unwrap();
            } else {
                let value = r.i32().unwrap();
                if k == 64 {
                    oxygen = Some(value);
                }
            }
        }
        r.done().unwrap();
        oxygen
    };
    assert_eq!(decode(w.stats(p, p).unwrap()), Some(100));
    assert_eq!(decode(w.stats(p, -1).unwrap()), None);
    w.players.get_mut(&p).unwrap().pos.x += 5.;
    {
        let player = w.players.get_mut(&p).unwrap();
        player.oxygen = 0;
        player.character.hp = 10;
        player.paused = true;
    }
    w.now += 150;
    w.ocean_step();
    assert_eq!(w.players[&p].character.hp, 10);
    w.players.get_mut(&p).unwrap().paused = false;
    w.now += 150;
    w.ocean_step();
    assert!(w.players[&p].death_pending);
    assert_eq!(w.players[&p].killer, "suffocation");
    assert_eq!(w.players[&p].character.hp, 0);
    w.now += 150;
    w.ocean_step();
    assert_eq!(w.players[&p].character.hp, 0);
}

#[test]
fn hermit_shield_depends_only_on_owned_tentacles_and_returns_after_attack_windows() {
    let (mut w, p, root) = arena(trench::HERMIT);
    {
        let e = w.entities.get_mut(&root).unwrap();
        e.quest = Some(root);
        e.stage = Some(0);
    }
    for n in 0..8 {
        let a = n as f32 * std::f32::consts::TAU / 8.;
        let id = w
            .spawn_entity(
                0xd65,
                Pos {
                    x: 32.5 + 6. * a.cos(),
                    y: 32.5 + 6. * a.sin(),
                },
                vec![],
            )
            .unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(Some(root), id, &mut w.timed_ids);
    }
    let target = w.players[&p].pos;
    w.ocean_mechanics(root, target, 0).unwrap();
    assert_eq!(w.entities.values().filter(|e| e.kind == 0xd64).count(), 8);
    assert_ne!(
        w.entities[&root].condition & condition::bit(condition::INVINCIBLE),
        0
    );
    w.now = 999;
    w.ocean_mechanics(root, target, 999).unwrap();
    assert_eq!(w.entities[&root].stage, Some(1));
    w.now = 1000;
    w.ocean_mechanics(root, target, 1000).unwrap();
    w.ocean_mechanics(root, target, 0).unwrap();
    assert_eq!(w.entities[&root].stage, Some(2));
    let own: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.parent == Some(root) && e.kind == 0xd64)
        .map(|e| e.id)
        .collect();
    for id in own {
        w.remove_entity(id);
    }
    let other = w
        .spawn_entity(0xd64, Pos { x: 20.5, y: 20.5 }, vec![])
        .unwrap();
    w.ocean_mechanics(root, target, 0).unwrap();
    assert_eq!(w.entities[&root].stage, Some(3));
    assert!(w.entities.contains_key(&other));
    w.now += 100;
    w.ocean_mechanics(root, target, 100).unwrap();
    assert_eq!(w.entities[&root].stage, Some(4));
    assert_eq!(
        w.entities[&root].condition & condition::bit(condition::INVINCIBLE),
        0
    );
    assert_eq!(w.ocean_shots(root, target).unwrap().len(), 3);
    w.now += 6000;
    w.ocean_mechanics(root, target, 6000).unwrap();
    assert_eq!(w.ocean_shots(root, target).unwrap().len(), 2);
    w.now += 6000;
    w.ocean_mechanics(root, target, 6000).unwrap();
    assert_eq!(w.entities[&root].stage, Some(0));
    assert_ne!(
        w.entities[&root].condition & condition::bit(condition::INVINCIBLE),
        0
    );
}

#[test]
fn thessal_uses_selected_patterns_and_native_stun_suppresses_shooting() {
    let (mut w, p, boss) = arena(trench::THESSAL);
    let target = w.players[&p].pos;
    w.ocean_mechanics(boss, target, 0).unwrap();
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x1707).count(), 4);
    w.entities.get_mut(&boss).unwrap().hp = 34000;
    w.ocean_mechanics(boss, target, 0).unwrap();
    assert_eq!(w.entities[&boss].next_aux, w.now + 500);
    for (stage, count) in [(6, 16), (7, 8), (8, 30)] {
        let e = w.entities.get_mut(&boss).unwrap();
        e.stage = Some(stage);
        e.activated = Some(w.now);
        e.next_shot = w.now;
        let shots = w.ocean_shots(boss, target).unwrap();
        assert_eq!(shots.len(), count);
        for (index, _) in shots {
            assert!(
                w.content.objects[&trench::THESSAL]
                    .projectiles
                    .contains_key(&index)
            );
        }
    }
    w.entities.get_mut(&boss).unwrap().stage = Some(2);
    assert_eq!(w.ocean_shots(boss, target).unwrap().len(), 8);
    let e = w.entities.get_mut(&boss).unwrap();
    e.stage = Some(6);
    e.next_shot = w.now;
    w.apply_condition(boss, condition::STUNNED, 1000).unwrap();
    assert!(w.npc_step().into_iter().all(|(owner, _)| owner != boss));
    w.entities.get_mut(&boss).unwrap().statuses.clear();
    w.players
        .get_mut(&p)
        .unwrap()
        .statuses
        .insert(condition::INVISIBLE, (0, 1000));
    let position = w.entities[&boss].pos;
    assert!(w.npc_step().into_iter().all(|(owner, _)| owner != boss));
    assert_eq!(w.entities[&boss].pos.distance(position), 0.);
}

#[test]
fn wounded_response_is_nearby_known_once_and_not_a_second_boss_clear() {
    let (mut w, p, wounded) = arena(0x1704);
    let target = w.players[&p].pos;
    w.ocean_mechanics(wounded, target, 0).unwrap();
    w.ocean_response(p, "Yes, he lives.").unwrap();
    assert!(w.entities.contains_key(&wounded));
    w.players.get_mut(&p).unwrap().known.insert(wounded);
    w.ocean_response(p, "Yes, he lives.").unwrap();
    assert!(!w.entities.contains_key(&wounded));
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x1705).count(), 3);
    w.ocean_response(p, "Yes, he lives.").unwrap();
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x1705).count(), 3);
    w.mechanic_expiry();
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x1705).count(), 3);
    assert!(!w.cleared);
}

#[test]
fn coastal_landmark_preserves_water_and_credits_one_original_not_its_children() {
    let mut d = Directory::test_prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        12351,
    )
    .unwrap();
    let raw = crate::terrain::generate(&d.content, 12351, 512).unwrap();
    let w = d.worlds.get_mut(&-3).unwrap();
    assert_eq!(w.hero_identities().len(), 14);
    let roots: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == trench::HERMIT)
        .map(|e| e.id)
        .collect();
    assert_eq!(roots.len(), 1);
    let root = roots[0];
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.kind == 0xd65 && e.parent == Some(root))
            .count(),
        8
    );
    for (a, b) in raw.cells.iter().zip(&w.map.cells) {
        if matches!(a.ground, 0x72 | 0x73) {
            assert_eq!(a.ground, b.ground);
        }
    }
    let player = w.enter(tests::ch(w), None).unwrap();
    assert!(!w.walkable(w.entities[&root].pos));
    w.entities
        .get_mut(&root)
        .unwrap()
        .damage
        .insert(w.players[&player].character.id, 55000);
    let seed = (0..100)
        .find(|seed| {
            let mut rng = StdRng::seed_from_u64(*seed);
            rng.gen_bool(1.);
            !crate::loot::rewards(&w.content, trench::HERMIT, &mut rng).is_empty()
        })
        .expect("selected Hermit loot fixture seed");
    w.rng = StdRng::seed_from_u64(seed);
    w.defeat_entity(root).unwrap();
    let bags: Vec<_> = w
        .entities
        .values()
        .filter(|e| !e.drop_slots.is_empty())
        .collect();
    assert!(!bags.is_empty());
    assert!(bags.iter().all(|bag| w.walkable(bag.pos)));
    assert_eq!(w.critical_deaths, vec![root]);
    assert_eq!(w.portal_requests.len(), 1);
    assert_eq!(w.portal_requests[0].0, 0x730);
    assert!(w.walkable(w.portal_requests[0].1));
    assert!(
        !w.entities
            .values()
            .any(|e| e.kind == 0xd65 && e.parent == Some(root))
    );
}

#[test]
fn thessal_mana_threshold_and_clear_commit_are_character_owned_and_once_only() {
    let (mut w, first, boss) = arena(trench::THESSAL);
    let mut other = tests::ch(&w);
    other.id = 2;
    other.account = 2;
    let second = w.enter(other, None).unwrap();
    w.entities.get_mut(&boss).unwrap().damage = BTreeMap::from([(1, 22080), (2, 22079)]);
    let defeated = w.remove_entity(boss);
    w.drop_loot(&defeated).unwrap();
    let mana: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.items.contains(&0xaea))
        .collect();
    assert_eq!(mana.len(), 1);
    assert!(mana[0].accessible(1));
    assert!(!mana[0].accessible(2));
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    let plans = w.pending_progress();
    assert_eq!(plans.len(), 2);
    for (id, snapshot) in plans {
        assert_eq!(snapshot.trenches, 1);
        w.finish_progress(id, snapshot).unwrap();
    }
    assert_eq!(w.players[&first].character.trenches, 1);
    assert_eq!(w.players[&second].character.trenches, 1);
    assert!(w.entities[&exit].published);
    assert!(w.drop_loot(&defeated).is_err());
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.items.contains(&0xaea))
            .count(),
        1
    );
}

#[test]
fn a_refused_hermit_wave_cannot_leave_partial_children_to_duplicate_on_retry() {
    let (mut w, player, root) = arena(trench::HERMIT);
    {
        let e = w.entities.get_mut(&root).unwrap();
        e.quest = Some(root);
        e.stage = Some(0);
    }
    for n in 0..8 {
        let a = n as f32 * std::f32::consts::TAU / 8.;
        let id = w
            .spawn_entity(
                0xd65,
                Pos {
                    x: 32.5 + 6. * a.cos(),
                    y: 32.5 + 6. * a.sin(),
                },
                vec![],
            )
            .unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(Some(root), id, &mut w.timed_ids);
    }
    for _ in 0..240 {
        let id = w
            .spawn_entity(0x170b, Pos { x: 20.5, y: 20.5 }, vec![])
            .unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(Some(root), id, &mut w.timed_ids);
    }
    for reserved in [false, true] {
        if reserved {
            let id = w.entities.values().find(|e| e.kind == 0x170b).unwrap().id;
            w.remove_entity(id);
            assert_eq!(
                w.named_toss(root, 0x170b, 1, &[Pos { x: 20.5, y: 20.5 }])
                    .unwrap(),
                1
            );
            assert_eq!(w.named_births.len(), 1);
        }
        for _ in 0..2 {
            let error = w
                .ocean_mechanics(root, w.players[&player].pos, 0)
                .unwrap_err();
            assert!(error.to_string().contains("budget"));
            assert_eq!(w.entities[&root].stage, Some(0));
            assert!(!w.entities.values().any(|e| e.kind == 0xd64));
        }
    }
}
