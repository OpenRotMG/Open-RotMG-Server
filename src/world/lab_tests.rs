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
        w.entities.get_mut(&id).unwrap().pos = Pos { x: 71.5, y: 12.5 };
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(2));
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
        assert_eq!(w.entities[&id].stage, Some(1));
    }
    assert_eq!(w.entities.len(), before);
    assert_eq!(w.entities[&id].hp, hp);
    assert_eq!(w.entities[&id].damage[&123], 500);
}
