use super::*;

#[test]
fn selected_lava_damage_and_safe_stone_obey_conditions_and_report_cadence() {
    let mut w = tests::world();
    let p = w.enter(tests::ch(&w), None).unwrap();
    let pos = w.players[&p].pos;
    let i = pos.y as usize * w.map.width as usize + pos.x as usize;
    Arc::make_mut(&mut w.map).cells[i].ground = 0xf4;
    assert_eq!(
        (w.content.hazards[&0xf4].min, w.content.hazards[&0xf4].max),
        (30, 60)
    );
    let hp = w.players[&p].character.hp;
    w.ground_damage(p, 1000, pos).unwrap();
    let hit = w.players[&p].character.hp;
    assert!((30..=60).contains(&(hp - hit)));
    w.ground_damage(p, 1400, pos).unwrap();
    assert_eq!(w.players[&p].character.hp, hit);
    let floor = w.spawn_entity(0x153, pos, vec![]).unwrap();
    w.ground_damage(p, 1600, pos).unwrap();
    assert_eq!(w.players[&p].character.hp, hit);
    w.remove_entity(floor);
    w.apply_condition(p, crate::condition::INVULNERABLE, 1000)
        .unwrap();
    w.ground_damage(p, 2200, pos).unwrap();
    assert_eq!(w.players[&p].character.hp, hit);
}
#[test]
fn ground_reports_fail_closed_and_lethal_ground_uses_the_normal_death_pipeline() {
    let mut w = tests::world();
    let p = w.enter(tests::ch(&w), None).unwrap();
    let pos = w.players[&p].pos;
    assert!(w.ground_damage(p, -1, pos).is_err());
    assert!(
        w.ground_damage(
            p,
            1000,
            Pos {
                x: pos.x + 5.,
                y: pos.y
            }
        )
        .is_err()
    );
    assert!(w.ground_damage(p, 1000, pos).is_err());
    let i = pos.y as usize * w.map.width as usize + pos.x as usize;
    Arc::make_mut(&mut w.map).cells[i].ground = 0xf4;
    w.players.get_mut(&p).unwrap().character.hp = 1;
    w.ground_damage(p, 1000, pos).unwrap();
    assert!(w.players[&p].death_pending);
    assert_eq!(w.players[&p].character.hp, 0);
    assert_eq!(w.players[&p].killer, "Lava Blend");
}
