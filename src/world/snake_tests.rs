use super::{tests, *};
use std::path::Path;

#[test]
fn minotaur_defeats_can_earn_snake_pits_only_in_the_realm() {
    let mut w = tests::arena(6);
    let player = w.enter(tests::ch(&w), None).unwrap();
    let pos = w.players[&player].pos;
    for name in ["Realm", "Snake Pit"] {
        w.name = name.into();
        w.portal_requests.clear();
        w.rng = rand::SeedableRng::seed_from_u64(213);
        for _ in 0..64 {
            let enemy = w.spawn_entity(0x646, pos, vec![]).unwrap();
            w.damage_enemy(player, enemy, 10000, true, &[], None)
                .unwrap();
            assert!(!w.entities.contains_key(&enemy));
        }
        assert_eq!(!w.portal_requests.is_empty(), name == "Realm");
        assert!(
            w.portal_requests
                .iter()
                .all(|&(kind, at)| kind == 0x718 && at.distance(pos) == 0.)
        );
    }
}

#[test]
fn snake_pit_attack_and_shield_windows_share_the_timed_cycle() {
    use crate::encounter::{
        Anchor, Motion,
        attacks::{self, Action},
    };
    for hp in [7500, 5000, 1000] {
        for (time, phase, blades, blind, shield) in [
            (0, 0, 6, 0, true),
            (2000, 0, 6, 0, false),
            (8000, 1, 3, 0, true),
            (9000, 1, 3, 0, false),
            (24000, 1, 0, 0, true),
            (25000, 2, 0, 6, false),
            (26500, 2, 0, 6, true),
            (28500, 2, 0, 6, false),
            (30000, 2, 0, 6, true),
            (31000, 0, 6, 0, true),
        ] {
            let s = attacks::schedule(0x917, hp, time).unwrap();
            assert_eq!(s.state % 3, phase);
            assert_eq!(crate::dungeon::snake::shield(time), shield);
            for (projectile, count) in [(0, blades), (1, blind)] {
                assert_eq!(
                    s.attacks
                        .iter()
                        .filter(|a| a.enabled
                            && matches!(a.action, Action::Shoot{projectile:p,..} if p==projectile))
                        .map(|a| a.count)
                        .sum::<usize>(),
                    count
                );
            }
            assert_eq!(
                s.attacks
                    .iter()
                    .filter(|a| matches!(
                        a.action,
                        Action::Grenade {
                            radius: 2.5,
                            damage: 100
                        }
                    ))
                    .count(),
                usize::from(phase == 2)
            );
        }
    }
    let pet = crate::encounter::profile(0x918).unwrap();
    assert_eq!(
        pet.motion,
        Motion::Orbit {
            anchor: Anchor::Parent,
            radius: 10.
        }
    );
    assert_eq!(pet.speed, crate::encounter::nr_speed(7.5));
}

#[test]
fn snake_pit_attack_clock_survives_swarm_dismissal_and_two_complete_cycles() {
    let mut clocks = crate::encounter::attacks::Clocks::default();
    for time in (0..62000).step_by(50) {
        let schedule = crate::encounter::attacks::schedule(0x917, 7500, time).unwrap();
        let due = clocks.due(&schedule, time);
        if schedule.state % 3 == 1 && schedule.age >= 16000 {
            assert!(
                due.is_empty(),
                "dismissal must not fire or change channel count"
            );
        }
    }
}

#[test]
fn snake_pit_rooms_are_round_and_seeds_change_layout_not_only_enemies() {
    let content = Content::load(Path::new("data")).unwrap();
    let a = crate::dungeon::snake_pit(&content, 7).unwrap();
    let b = crate::dungeon::snake_pit(&content, 8).unwrap();
    let geometry = |m: &Map| {
        m.cells
            .iter()
            .map(|c| (c.ground, c.object.filter(|k| content.objects[k].blocks)))
            .collect::<Vec<_>>()
    };
    assert_ne!(
        geometry(&a),
        geometry(&b),
        "seeded enemy rolls are not procedural room generation"
    );
    let (sx, sy) = (a.spawns[0].0 as i32, a.spawns[0].1 as i32);
    let mut radius = 0;
    while {
        let c = &a.cells[(sy * a.width + sx + radius + 1) as usize];
        c.ground != 0xff && c.object.is_none_or(|k| !content.objects[&k].blocks)
    } {
        radius += 1;
    }
    let corner = (radius * 4 / 5).max(1);
    assert_eq!(
        a.cells[((sy + corner) * a.width + sx + corner) as usize].ground,
        0xff,
        "square room retained"
    );
}

#[test]
fn snake_pit_cycles_at_full_health_and_owns_its_guardians_and_swarm() {
    let mut w = tests::arena(0);
    let pos = Pos {
        x: w.map.width as f32 / 2.,
        y: w.map.height as f32 / 2.,
    };
    let queen = w.spawn_entity(0x917, pos, vec![]).unwrap();
    let other = w.spawn_entity(0x917, pos, vec![]).unwrap();
    w.encounter_mechanics(queen, pos, 0).unwrap();
    w.encounter_mechanics(other, pos, 0).unwrap();
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == 0x918 && e.parent == Some(queen))
            .count(),
        4,
        "missing original circling guardians"
    );
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == 0x918 && e.parent == Some(other))
            .count(),
        4
    );
    for (time, stage) in [(8000, 1), (25000, 2), (31000, 3), (39000, 4)] {
        w.now = time;
        w.encounter_mechanics(queen, pos, time).unwrap();
        assert_eq!(w.entities[&queen].hp, 7500);
        assert_eq!(
            w.entities[&queen].stage,
            Some(stage),
            "phase froze behind an HP threshold at {time}"
        );
    }
    for time in (39000..45000).step_by(750) {
        w.now = time;
        w.encounter_mechanics(queen, pos, time).unwrap();
    }
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == 0x919 && e.parent == Some(queen))
            .count(),
        8
    );
    w.now = 55000;
    w.encounter_mechanics(queen, pos, 55000).unwrap();
    assert!(
        !w.actors()
            .any(|e| e.kind == 0x919 && e.parent == Some(queen))
    );
    w.now = 56000;
    w.encounter_mechanics(queen, pos, 56000).unwrap();
    assert_eq!(w.entities[&queen].stage, Some(5));
    w.remove_entity(queen);
    w.mechanic_expiry();
    assert!(!w.actors().any(|e| e.parent == Some(queen)));
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == 0x918 && e.parent == Some(other))
            .count(),
        4
    );
}

#[test]
fn snake_pit_cowardice_and_durable_completion_return_to_realm_not_nexus() {
    let mut d = tests::directory();
    let pit = d.instance("snake-pit", None).unwrap();
    let (player, _rx) = tests::receive(&mut d, pit, 1, crate::content::WIZARD);
    let entrance = d.worlds[&pit]
        .entities
        .values()
        .find(|e| e.kind == 0x703)
        .unwrap()
        .id;
    assert_eq!(
        d.destination(player, Some(entrance)).unwrap().2,
        -3,
        "Cowardice incorrectly goes to Nexus"
    );
    assert_eq!(
        d.destination(player, None).unwrap().2,
        -2,
        "Nexus escape must remain distinct"
    );
    let exit;
    {
        let w = d.worlds.get_mut(&pit).unwrap();
        let boss = w.entities.values().find(|e| e.kind == 0x917).unwrap().id;
        let mut defeated = w.remove_entity(boss);
        let p = w.players.get_mut(&player).unwrap();
        p.pos = defeated.pos;
        defeated.damage.insert(p.character.id, 1000);
        w.drop_loot(&defeated).unwrap();
        exit = w.clear_exit.unwrap();
        assert_eq!(w.entities[&exit].kind, 0x704);
        assert!(!w.entities[&exit].published);
        assert!(w.drop_loot(&defeated).is_err());
    }
    assert!(
        d.destination(player, Some(exit)).is_err(),
        "uncommitted completion exit must not be usable"
    );
    let w = d.worlds.get_mut(&pit).unwrap();
    let plans = w.pending_progress();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].1.dungeons[0], 1);
    w.finish_progress(player, plans[0].1.clone()).unwrap();
    assert_eq!(d.destination(player, Some(exit)).unwrap().2, -3);
}
