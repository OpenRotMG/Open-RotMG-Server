use super::movement_tests::fixture;
use super::*;

fn facts(event: &str) -> Vec<serde_json::Value> {
    crate::audit::CAPTURED.with(|c| {
        c.borrow()
            .iter()
            .filter(|f| f["event"] == event)
            .map(|f| f["values"].clone())
            .collect()
    })
}

#[test]
fn shots_and_warned_blasts_record_aim_and_escape_evidence() {
    let (mut w, id) = fixture(0x18, false);
    w.now = 1000;
    let enemy = w
        .spawn_entity(0x21b, Pos { x: 12.5, y: 10.5 }, vec![])
        .unwrap();
    w.shoot(id, 1000, 1, 0xa14, Pos { x: 8.8, y: 8.5 }, 0.)
        .unwrap();
    let aim = facts("shot_aim");
    assert_eq!(aim.len(), 1);
    assert_eq!(aim[0]["self"], serde_json::json!([8.5, 8.5]));
    assert_eq!(aim[0]["target"][0], enemy);
    let error = aim[0]["target"][4].as_f64().unwrap();
    assert!((error - 2f64.atan2(3.7)).abs() < 1e-4, "{error}");

    w.throw_grenade(enemy, Pos { x: 8.5, y: 8.5 }, 2., 10)
        .unwrap();
    w.players.get_mut(&id).unwrap().pos = Pos { x: 12., y: 8.5 };
    for _ in 0..100 {
        w.step();
    }
    assert!(w.grenades.is_empty());
    let threat = facts("area_threat");
    assert_eq!(threat.len(), 1);
    assert_eq!(
        (
            &threat[0]["distance_at_throw"],
            &threat[0]["distance_at_impact"]
        ),
        (&serde_json::json!(0.), &serde_json::json!(3.5))
    );
    assert_eq!(w.players[&id].character.hp, 200);
}

#[test]
fn projectile_overlaps_are_recorded_once_without_damaging_humans() {
    let (mut w, id) = fixture(0x18, false);
    w.now = 1000;
    w.bullets.push(super::combat_tests::shot(&w, 999, 0xdf0, 0));
    for _ in 0..3 {
        w.step();
    }
    let overlaps = facts("overlap");
    assert_eq!(overlaps.len(), 1);
    assert!(overlaps[0]["damage"].as_i64().unwrap() > 0);
    assert_eq!(w.players[&id].character.hp, 200);
}
