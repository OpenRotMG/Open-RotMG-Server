use super::*;
use crate::{
    condition,
    dungeon::lab,
    encounter::{Anchor, Motion},
};

fn arena() -> (World, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = lab::generate(&content, 12351).unwrap();
    let world = World::new(content, Arc::new(map)).unwrap();
    let id = world.actors().find(|e| e.kind == lab::CREATION).unwrap().id;
    (world, id)
}

#[test]
fn creation_door_requires_boss_and_every_coil_in_either_order() {
    for boss_first in [false, true] {
        let (mut w, id) = arena();
        let home = w.entities[&id].home;
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, None);
        assert!(matches!(w.lab_movement(id).0, Motion::Stationary));
        let coils: Vec<_> = w
            .actors()
            .filter(|e| e.kind == lab::TESLA)
            .map(|e| e.id)
            .collect();
        assert!(!coils.is_empty());
        if boss_first {
            w.cleared = true;
        }
        for coil in coils {
            w.lab_mechanics(id, Some(home), 0).unwrap();
            assert_eq!(w.entities[&id].stage, None);
            assert!(w.entities.values().any(|e| e.kind == lab::EMERGENCY));
            w.remove_entity(coil);
        }
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, boss_first.then_some(1));
        w.cleared = true;
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(1));
        assert!(!w.entities.values().any(|e| e.kind == lab::EMERGENCY));
    }
}

#[test]
fn creation_attacks_and_pool_cycle_preserve_identity_and_damage() {
    let (mut w, id) = arena();
    let home = w.entities[&id].home;
    w.entities.get_mut(&id).unwrap().stage = Some(1);
    let before = w.entities.len();
    let hp = w.entities[&id].hp;
    w.entities.get_mut(&id).unwrap().damage.insert(123, 500);
    let shots = |w: &World, projectile, count| {
        let shots = w.lab_shots(id, home);
        assert_eq!(shots.len(), count);
        assert!(
            shots
                .iter()
                .all(|(index, angle)| *index == projectile && angle.is_finite())
        );
    };
    shots(&w, 1, 3);
    for (elapsed, stage, projectile, count) in [
        (10000, 3, 0, 0),
        (0, 4, 6, 1),
        (16000, 5, 0, 0),
        (0, 6, 5, 8),
        (1000, 7, 4, 8),
        (1000, 1, 1, 3),
    ] {
        w.lab_mechanics(id, Some(home), elapsed).unwrap();
        assert_eq!(w.entities[&id].stage, Some(stage));
        shots(&w, projectile, count);
        assert_ne!(
            w.entities[&id].condition & condition::bit(condition::INVINCIBLE),
            0
        );
    }
    for _ in 0..3 {
        let e = w.entities.get_mut(&id).unwrap();
        e.pos = Pos { x: 71.5, y: 12.5 };
        e.goal = Some(e.pos);
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(2));
        assert!(w.entities[&id].goal.is_none());
        assert_eq!(
            w.entities[&id].condition & condition::bit(condition::INVINCIBLE),
            0
        );
        shots(&w, 0, 0);
        w.entities.get_mut(&id).unwrap().pos = Pos { x: 75.5, y: 12.5 };
        w.lab_mechanics(id, Some(home), 9999).unwrap();
        assert_eq!(w.entities[&id].stage, Some(2));
        w.now += 10000;
        assert!(matches!(
            w.lab_movement(id).0,
            Motion::Protect {
                anchor: Anchor::Kind(lab::DEHEXXER),
                ..
            }
        ));
        w.entities.get_mut(&id).unwrap().pos = Pos { x: 75.5, y: 12.5 };
        w.lab_mechanics(id, Some(home), 10000).unwrap();
        assert_eq!(w.entities[&id].stage, Some(8));
        shots(&w, 1, 3);
        w.entities.get_mut(&id).unwrap().pos = home;
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(4));
        shots(&w, 6, 1);
        w.entities.get_mut(&id).unwrap().stage = Some(1);
    }
    assert_eq!(w.entities.len(), before);
    assert_eq!(w.entities[&id].hp, hp);
    assert_eq!(w.entities[&id].damage[&123], 500);
}

#[test]
fn lab_pool_remedy_is_actor_driven_source_aware_and_player_local() {
    let (mut w, first) = super::movement_tests::fixture(0xa7, false);
    let pool = condition::bit(condition::HEXED) | condition::bit(condition::SPEEDY) | STUNNED;
    let mut other = w.players[&first].character.clone();
    other.id = 2;
    other.account = 2;
    let second = w.enter(other, None).unwrap();
    w.players.get_mut(&second).unwrap().pos.x = 12.5;
    w.players.get_mut(&first).unwrap().lab_conditions = pool;
    w.lab_step();
    assert_eq!(w.players[&first].lab_conditions, pool);
    w.players.get_mut(&first).unwrap().lab_conditions = 0;
    let green = w.players[&first].pos;
    w.spawn_entity(lab::HEXXER, green, vec![]).unwrap();
    w.lab_step();
    assert_eq!(w.players[&first].conditions(w.now), pool);
    assert_eq!(w.players[&second].conditions(w.now), 0);
    w.players.get_mut(&first).unwrap().pos.x = 12.5;
    w.lab_step();
    assert_eq!(w.players[&first].lab_conditions, pool);
    let blue = w.players[&first].pos;
    w.spawn_entity(lab::DEHEXXER, blue, vec![]).unwrap();
    w.players.get_mut(&second).unwrap().pos = green;
    for effect in [
        condition::HEXED,
        condition::STUNNED,
        condition::SPEEDY,
        condition::QUIET,
    ] {
        w.players
            .get_mut(&first)
            .unwrap()
            .statuses
            .insert(effect, (0, 1000));
    }
    w.lab_step();
    let cured = &w.players[&first];
    assert_eq!(cured.lab_conditions, 0);
    assert!(!cured.statuses.contains_key(&condition::HEXED));
    assert_eq!(
        cured.conditions(w.now),
        STUNNED | condition::bit(condition::SPEEDY) | condition::bit(condition::QUIET)
    );
    assert_eq!(w.players[&second].lab_conditions, pool);
    w.now = 1000;
    w.lab_step();
    assert_eq!(w.players[&first].conditions(w.now), 0);
    assert_eq!(w.players[&second].conditions(w.now), pool);
}

#[test]
fn lab_pool_blue_precedence_pause_and_terminal_player_cleanup() {
    let (mut w, id) = super::movement_tests::fixture(0xa9, false);
    let pool = condition::bit(condition::HEXED) | condition::bit(condition::SPEEDY) | STUNNED;
    let position = w.players[&id].pos;
    w.lab_step();
    assert_eq!(w.players[&id].conditions(w.now), 0);
    w.spawn_entity(lab::HEXXER, position, vec![]).unwrap();
    let blue = w.spawn_entity(lab::DEHEXXER, position, vec![]).unwrap();
    w.lab_step();
    assert_eq!(w.players[&id].conditions(w.now), 0);
    w.remove_entity(blue);
    w.players.get_mut(&id).unwrap().paused = true;
    w.lab_step();
    assert_eq!(w.players[&id].lab_conditions, 0);
    w.players.get_mut(&id).unwrap().paused = false;
    w.lab_step();
    assert_eq!(w.players[&id].lab_conditions, pool);
    for terminal in 0..3 {
        let player = w.players.get_mut(&id).unwrap();
        player.lab_conditions = pool;
        player.character.alive = terminal != 0;
        player.death_pending = terminal == 1;
        player.departing = terminal == 2;
        player.statuses.insert(condition::QUIET, (0, 1000));
        w.lab_step();
        assert_eq!(w.players[&id].lab_conditions, 0);
        assert_eq!(
            w.players[&id].conditions(w.now),
            condition::bit(condition::QUIET)
        );
    }
}
