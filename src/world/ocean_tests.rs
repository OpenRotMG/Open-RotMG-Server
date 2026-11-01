use super::*;
use crate::dungeon::trench;

fn wounded() -> (World, i32, i32) {
    let (mut w, player) = super::movement_tests::fixture(0x2e, false);
    w.name = "Ocean Trench".into();
    let pos = w.players[&player].pos;
    let boss = w.spawn_entity(trench::THESSAL, pos, vec![]).unwrap();
    let defeated = w.entities[&boss].clone();
    for seed in 0..100 {
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.ocean_death(&defeated).unwrap();
        if let Some(id) = w.entities.values().find(|e| e.kind == 0x1704).map(|e| e.id) {
            w.ocean_mechanics(id, Some(pos), 0).unwrap();
            w.players.get_mut(&player).unwrap().known.insert(id);
            return (w, player, id);
        }
    }
    panic!("No wounded Thessal in bounded ordinary death rolls");
}

#[test]
fn wounded_thessal_accepts_period_answer_once_and_rejects_other_answers() {
    for answer in [
        "He lives and reigns and conquers the world",
        " he LIVES and reigns and conquers the world. ",
    ] {
        let (mut w, player, id) = wounded();
        for wrong in ["No", "Yes, he lives", "He lives and reigns"] {
            w.ocean_response(player, wrong).unwrap();
            assert!(w.entities.contains_key(&id));
            assert!(!w.entities.values().any(|e| e.kind == 0x1705));
        }
        w.ocean_response(player, answer).unwrap();
        assert!(!w.entities.contains_key(&id));
        let gifts: Vec<_> = w.entities.values().filter(|e| e.kind == 0x1705).collect();
        assert_eq!(gifts.len(), 3);
        assert!(gifts.iter().all(|e| e.parent.is_none()));
        w.ocean_response(player, answer).unwrap();
        assert_eq!(w.entities.values().filter(|e| e.kind == 0x1705).count(), 3);
    }
}

#[test]
fn wounded_thessal_requires_known_nearby_live_window() {
    for gate in ["unknown", "far", "expired"] {
        let (mut w, player, id) = wounded();
        match gate {
            "unknown" => {
                w.players.get_mut(&player).unwrap().known.remove(&id);
            }
            "far" => {
                w.players.get_mut(&player).unwrap().pos.x += 10.;
            }
            "expired" => {
                w.now = 12000;
            }
            _ => unreachable!(),
        }
        w.ocean_response(player, "He lives and reigns and conquers the world")
            .unwrap();
        assert!(w.entities.contains_key(&id), "{gate}");
        assert!(!w.entities.values().any(|e| e.kind == 0x1705), "{gate}");
    }
}

#[test]
fn oxygen_pockets_do_not_stack_and_empty_air_attributes_lethal_damage() {
    let (mut w, player) = super::movement_tests::fixture(0x2e, false);
    w.name = "Ocean Trench".into();
    let pos = w.players[&player].pos;
    w.players.get_mut(&player).unwrap().oxygen = 0;
    for _ in 0..2 {
        w.spawn_entity(trench::VENT, pos, vec![]).unwrap();
    }
    w.now = 150;
    w.ocean_step();
    assert_eq!(w.players[&player].oxygen, 8);
    w.players.get_mut(&player).unwrap().pos.x += 1.;
    w.now = 300;
    w.ocean_step();
    assert_eq!(w.players[&player].oxygen, 6);
    let p = w.players.get_mut(&player).unwrap();
    p.oxygen = 0;
    p.character.hp = 10;
    w.now = 450;
    w.ocean_step();
    let p = &w.players[&player];
    assert_eq!(p.character.hp, 0);
    assert!(p.death_pending);
    assert_eq!(p.killer, "suffocation");
}

#[test]
fn ocean_children_are_the_live_owned_children_of_that_kind() {
    let (mut w, player) = super::movement_tests::fixture(0x2e, false);
    let pos = w.players[&player].pos;
    let boss = w.spawn_entity(trench::THESSAL, pos, vec![]).unwrap();
    let other = w.spawn_entity(trench::THESSAL, pos, vec![]).unwrap();
    let mut owned = Vec::new();
    for (parent, kind) in [
        (boss, 0x1707),
        (other, 0x1707),
        (boss, 0x1702),
        (boss, 0x1707),
    ] {
        let id = w.spawn_entity(kind, pos, vec![]).unwrap();
        let e = w.entities.get_mut(&id).unwrap();
        e.parent.set(Some(parent), id, &mut w.timed_ids);
        if (parent, kind) == (boss, 0x1707) {
            owned.push(id);
        }
    }
    w.spawn_entity(0x1707, pos, vec![]).unwrap();
    assert_eq!(w.ocean_children(boss, 0x1707), owned);
    w.remove_entity(owned[0]);
    assert_eq!(w.ocean_children(boss, 0x1707), owned[1..]);
}
