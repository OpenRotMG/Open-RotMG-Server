use super::{tests::*, *};
use rand::SeedableRng;

#[test]
fn minimap_visible_mobs_tick_without_expanding_attack_range_or_ignoring_conditions() {
    for (distance, condition, moves) in [
        (17., 0, true),
        (21., 0, false),
        (17., crate::condition::bit(crate::condition::STASIS), false),
        (
            17.,
            crate::condition::bit(crate::condition::PARALYZED),
            false,
        ),
    ] {
        let mut w = arena(0);
        w.rng = rand::rngs::StdRng::seed_from_u64(51403);
        let home = Pos { x: 12.5, y: 100.5 };
        let id = w.spawn_entity(0x603, home, vec![]).unwrap();
        w.entities.get_mut(&id).unwrap().condition = condition;
        let player = w.enter(ch(&w), None).unwrap();
        w.players.get_mut(&player).unwrap().pos = Pos {
            x: home.x,
            y: home.y - distance,
        };
        w.updates(player, true).unwrap();
        assert_eq!(w.players[&player].known.contains(&id), distance < 20.);
        let mut excursion = 0_f32;
        for _ in 0..10 {
            w.step();
            excursion = excursion.max(w.entities[&id].pos.distance(home));
        }
        assert_eq!(
            excursion > 0.1,
            moves,
            "distance={distance} condition={condition}: {:?}",
            w.entities[&id].pos
        );
        assert!(
            w.bullets.is_empty(),
            "simulation range must not become attack range"
        );
    }
}

#[test]
fn minimap_simulation_preserves_quest_wake_then_advances_an_engaged_encounter() {
    let mut w = arena(0);
    let home = Pos { x: 12.5, y: 100.5 };
    let id = w.spawn_entity(0xd06, home, vec![]).unwrap();
    assert!(w.content.objects[&0xd06].quest);
    let player = w.enter(ch(&w), None).unwrap();
    w.players.get_mut(&player).unwrap().pos = Pos {
        x: home.x,
        y: home.y - 17.,
    };
    for _ in 0..10 {
        w.step();
    }
    assert!(w.entities[&id].activated.is_none());
    assert!(w.entities[&id].pos.distance(home) < 0.001);
    w.players.get_mut(&player).unwrap().pos.y = home.y - 10.;
    w.step();
    let activated = w.entities[&id]
        .activated
        .expect("quest must wake inside its own range");
    w.players.get_mut(&player).unwrap().pos.y = home.y - 17.;
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(
        w.entities[&id].activated,
        Some(activated),
        "visible encounter clock was suspended"
    );
}
