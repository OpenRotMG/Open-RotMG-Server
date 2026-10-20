use super::*;
use crate::{
    condition,
    encounter::{Anchor, Motion, nr_speed},
};

fn actor(kind: u16) -> (World, i32, i32) {
    let mut w = tests::arena(0);
    w.rng = rand::rngs::StdRng::seed_from_u64(42);
    let home = Pos { x: 20.5, y: 20.5 };
    let id = w.spawn_entity(kind, home, vec![]).unwrap();
    let p = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 24.5, ..home };
    (w, id, p)
}
fn tick(w: &mut World, id: i32, ms: u64) -> Pos {
    let before = w.entities[&id].pos;
    w.now = ms;
    w.npc_step();
    let e = &w.entities[&id];
    let step = Pos {
        x: e.pos.x - before.x,
        y: e.pos.y - before.y,
    };
    assert!(e.pos.x.is_finite() && e.pos.y.is_finite());
    assert_eq!(
        (0..w.cells.len())
            .filter(|&c| w.cells.ids(c).contains(&id))
            .count(),
        1
    );
    if step.x != 0. || step.y != 0. {
        assert!((e.last_step.x - step.x).abs() < 0.00001);
        assert!((e.last_step.y - step.y).abs() < 0.00001);
    }
    assert!(!w.cleared);
    step
}
fn ghost() -> (World, i32, i32) {
    let (mut w, id, p) = actor(0x928);
    let e = w.entities.get_mut(&id).unwrap();
    e.quest = Some(id);
    e.stage = Some(7);
    e.activated = Some(0);
    (w, id, p)
}
fn follow_tick(w: &mut World, id: i32, p: i32, ms: u64, distance: f32) -> f32 {
    let home = w.entities[&id].home;
    w.position_entity(id, home).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: home.x + distance,
        ..home
    };
    tick(w, id, ms).x
}

#[test]
fn follow_acquisition_duration_and_cooldown_are_invocation_relative() {
    let (mut w, id, p) = ghost();
    w.last_npc_step = 3100;
    for n in 1..=40 {
        assert!(follow_tick(&mut w, id, p, 3100 + n * 50, 6.) > 0.1);
    }
    for n in 41..=81 {
        assert_eq!(follow_tick(&mut w, id, p, 3100 + n * 50, 6.), 0.);
    }
    assert!(follow_tick(&mut w, id, p, 7200, 6.) > 0.1);
}

#[test]
fn follow_rest_hysteresis_and_target_loss_have_distinct_reacquisition_waits() {
    let (mut w, id, p) = ghost();
    for (ms, distance, moving) in [
        (50, 2., false),
        (100, 2.001, false),
        (150, 3., false),
        (200, 3.001, true),
        (250, 10., false),
        (300, 6., true),
        (350, 2., false),
        (400, 10., false),
    ] {
        assert_eq!(follow_tick(&mut w, id, p, ms, distance) > 0., moving);
    }
    for n in 9..=48 {
        assert_eq!(follow_tick(&mut w, id, p, n * 50, 6.), 0.);
    }
    assert!(follow_tick(&mut w, id, p, 2450, 6.) > 0.1);
}

#[test]
fn follow_status_dormancy_and_cloak_keep_local_clock_semantics() {
    let (mut w, id, p) = ghost();
    follow_tick(&mut w, id, p, 50, 6.);
    for (ms, status) in [(1050, condition::PARALYZED), (2050, condition::STASIS)] {
        w.apply_condition(id, status, 2000).unwrap();
        assert_eq!(follow_tick(&mut w, id, p, ms, 6.), 0.);
        assert_eq!(w.entities[&id].follow_remaining, 1950);
        w.entities.get_mut(&id).unwrap().statuses.clear();
    }
    assert_eq!(follow_tick(&mut w, id, p, 3050, 30.), 0.);
    assert_eq!(w.entities[&id].follow_remaining, 1950);
    w.apply_condition(id, condition::SLOWED, 1000).unwrap();
    assert!((follow_tick(&mut w, id, p, 3100, 6.) - nr_speed(0.6) / 1.5 * 0.025).abs() < 0.00001);
    w.apply_condition(p, condition::INVISIBLE, 1000).unwrap();
    assert_eq!(follow_tick(&mut w, id, p, 3150, 6.), 0.);
    assert_eq!(w.entities[&id].following, Following::Unknown);
    w.players.get_mut(&p).unwrap().statuses.clear();
    assert!(follow_tick(&mut w, id, p, 3200, 6.) > 0.);
}

#[test]
fn strict_shield_initializes_without_moving_then_ignores_slow_and_loses_owned_anchor() {
    let mut w = tests::arena(3);
    w.rng = rand::rngs::StdRng::seed_from_u64(42);
    let home = Pos { x: 20.5, y: 20.5 };
    let family = population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x900)
        .unwrap();
    let positions = w.family_layout(home, family, &[]).unwrap();
    w.place_camp(family, positions).unwrap();
    let key = w.population[w.camps[0].start].entity.unwrap();
    let id = w.camp_member(key, 0x902).unwrap().unwrap();
    let flood = w.camp_member(key, 0x901).unwrap().unwrap();
    let p = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = home;
    w.apply_condition(flood, condition::PARALYZED, 10000)
        .unwrap();
    w.apply_condition(id, condition::PARALYZED, 1000).unwrap();
    assert_eq!(tick(&mut w, id, 50).x, 0.);
    assert!(!w.entities[&id].strict_initialized);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    assert_eq!(tick(&mut w, id, 100).x, 0.);
    let phase = w.entities[&id].phase;
    let center = w.entities[&flood].pos;
    w.position_entity(
        flood,
        Pos {
            x: center.x + 0.25,
            ..center
        },
    )
    .unwrap();
    w.apply_condition(id, condition::SLOWED, 1000).unwrap();
    tick(&mut w, id, 150);
    assert!(
        ((w.entities[&id].phase - phase).rem_euclid(std::f32::consts::TAU) - 0.15).abs() < 0.00001
    );
    assert!((w.entities[&id].pos.distance(w.entities[&flood].pos) - 2.).abs() < 0.001);
    w.remove_entity(flood);
    tick(&mut w, id, 200);
    assert_eq!(w.entities[&id].motion, Some(Motion::Wander));
    assert!(!w.entities[&id].strict_initialized);
}

#[test]
fn all_five_cyclops_roles_keep_strict_sight_stop_and_retreat_boundaries() {
    for kind in 0x67e..=0x682 {
        let (mut w, id, p) = actor(kind);
        let home = w.entities[&id].home;
        for (distance, sign) in [(1.999, -1.), (2., -1.), (2.001, 1.), (9.999, 1.)] {
            let e = w.entities.get_mut(&id).unwrap();
            e.stage = Some(0);
            e.reaction_until = 0;
            w.position_entity(id, home).unwrap();
            w.players.get_mut(&p).unwrap().pos.x = home.x + distance;
            let ms = w.now + 50;
            assert_eq!(tick(&mut w, id, ms).x.signum(), sign);
        }
        w.entities.get_mut(&id).unwrap().stage = Some(1);
        w.entities.get_mut(&id).unwrap().reaction_until = 5000;
        for (distance, moving) in [(7.999, true), (8., false), (8.001, false)] {
            w.position_entity(id, home).unwrap();
            w.players.get_mut(&p).unwrap().pos.x = home.x + distance;
            let ms = w.now + 50;
            assert_eq!(tick(&mut w, id, ms).x < 0., moving);
        }
        w.position_entity(id, home).unwrap();
        w.players.get_mut(&p).unwrap().pos.x = home.x + 10.;
        let ms = w.now + 50;
        let step = tick(&mut w, id, ms);
        assert!((step.x.hypot(step.y) - 10. / 1.5 * 0.05).abs() < 0.0001);
    }
}

#[test]
fn cyclops_entry_and_expiry_publish_both_legs_reselect_target_and_attack_once() {
    let (mut w, id, p) = actor(0x67f);
    let home = w.entities[&id].home;
    w.players.get_mut(&p).unwrap().pos.x = home.x + 2.;
    assert!((tick(&mut w, id, 50).x + 8. / 1.5 * 0.05).abs() < 0.0001);
    assert_eq!(w.entities[&id].reaction_until, 1000);
    let mut ch = tests::ch(&w);
    ch.id = 2;
    ch.account = 2;
    let q = w.enter(ch, None).unwrap();
    w.players.get_mut(&q).unwrap().pos = Pos {
        x: home.x - 4.1,
        ..home
    };
    w.players.get_mut(&p).unwrap().pos.x = home.x + 4.;
    w.position_entity(id, home).unwrap();
    w.last_npc_step = 950;
    w.entities.get_mut(&id).unwrap().next_shot = 1000;
    let bullet = w.entities[&id].bullet;
    assert!((tick(&mut w, id, 1000).x + (8. + 10.) / 1.5 * 0.05).abs() < 0.0001);
    assert_eq!(w.entities[&id].bullet.wrapping_sub(bullet), 1);
    assert_eq!(w.players[&p].character.kills, 0);
    assert_eq!(w.players[&p].experience_queued, 0);
}

fn wall(w: &mut World, pos: Pos) {
    let kind = w
        .content
        .objects
        .values()
        .find(|o| o.blocks && o.class == "Wall")
        .unwrap()
        .kind;
    w.spawn_entity(kind, pos, vec![]).unwrap();
}

#[test]
fn queued_legs_preserve_upstream_wall_slide_and_publish_actual_net_step() {
    let (mut w, id, p) = actor(0x67e);
    let start = Pos { x: 20.01, y: 20.5 };
    w.position_entity(id, start).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 24.01, y: 24.5 };
    wall(&mut w, Pos { x: 19.5, ..start });
    let e = w.entities.get_mut(&id).unwrap();
    e.stage = Some(1);
    e.reaction_until = 1000;
    w.last_npc_step = 950;
    let step = tick(&mut w, id, 1000);
    assert!(
        step.x > 0.2,
        "chase must survive blocked retreat x component: {step:?}"
    );
    assert!(
        step.y > 0. && step.y < step.x,
        "retreat must first slide down the wall: {step:?}"
    );
    assert_eq!(w.entities[&id].stage, Some(0));
}

#[test]
fn cyclops_paralysis_does_not_start_retreat_but_existing_clock_and_stasis_differ() {
    let (mut w, id, p) = actor(0x680);
    w.players.get_mut(&p).unwrap().pos.x = 22.5;
    w.apply_condition(id, condition::PARALYZED, 1000).unwrap();
    assert_eq!(tick(&mut w, id, 50).x, 0.);
    assert_eq!(w.entities[&id].reaction_until, 0);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    assert!(tick(&mut w, id, 100).x < 0.);
    assert_eq!(w.entities[&id].reaction_until, 1050);
    w.apply_condition(id, condition::STASIS, 1000).unwrap();
    assert_eq!(tick(&mut w, id, 600).x, 0.);
    assert_eq!(w.entities[&id].reaction_until, 1550);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    w.apply_condition(id, condition::PARALYZED, 2000).unwrap();
    assert_eq!(tick(&mut w, id, 1550).x, 0.);
    assert_eq!(w.entities[&id].stage, Some(0));
}

#[test]
fn red_demon_return_uses_both_axes_and_pursuit_expiry_keeps_last_motor() {
    let (mut w, id, p) = actor(0x668);
    let home = w.entities[&id].home;
    for (x, y, stage) in [(1., 1., 0), (1.001, 1., 2), (1., -1.001, 2)] {
        let e = w.entities.get_mut(&id).unwrap();
        e.stage = Some(2);
        e.reaction_until = 0;
        w.position_entity(
            id,
            Pos {
                x: home.x + x,
                y: home.y + y,
            },
        )
        .unwrap();
        let ms = w.now + 50;
        tick(&mut w, id, ms);
        assert_eq!(w.entities[&id].stage, Some(stage));
    }
    w.position_entity(
        id,
        Pos {
            x: home.x + 4.,
            ..home
        },
    )
    .unwrap();
    w.players.get_mut(&p).unwrap().pos.x = home.x + 8.;
    let e = w.entities.get_mut(&id).unwrap();
    e.stage = Some(1);
    e.reaction_until = 5000;
    assert!(tick(&mut w, id, 4999).x > 0.);
    assert!(tick(&mut w, id, 5000).x.abs() < 0.0001);
    assert_eq!(w.entities[&id].stage, Some(2));
    assert!(tick(&mut w, id, 5050).x < 0.);
}

#[test]
fn red_demon_queue_never_invokes_a_third_child_on_idle_expiry() {
    let (mut w, id, p) = actor(0x668);
    let home = w.entities[&id].home;
    w.position_entity(
        id,
        Pos {
            x: home.x + 4.,
            ..home
        },
    )
    .unwrap();
    w.players.get_mut(&p).unwrap().pos.x = home.x + 5.;
    let e = w.entities.get_mut(&id).unwrap();
    e.stage = Some(0);
    e.reaction_until = 5000;
    e.motion = Some(Motion::WanderHome { radius: 1. });
    e.goal = Some(Pos {
        x: home.x + 0.5,
        ..home
    });
    let step = tick(&mut w, id, 5000);
    assert!(step.x < 0. && step.x.hypot(step.y) <= 0.1001);
    assert_eq!(w.entities[&id].stage, Some(2));
    w.apply_condition(id, condition::PARALYZED, 1000).unwrap();
    w.position_entity(id, home).unwrap();
    assert_eq!(tick(&mut w, id, 5050).x, 0.);
    assert_eq!(w.entities[&id].stage, Some(2));
}

fn escort(kind: u16, boss_distance: f32, player_distance: f32) -> (World, i32, i32, i32) {
    let (mut w, id, p) = actor(kind);
    let pos = w.entities[&id].pos;
    let boss = w
        .spawn_entity(
            0xd90,
            Pos {
                x: pos.x - boss_distance,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&boss).unwrap().stage = Some(0);
    w.entities.get_mut(&boss).unwrap().activated = Some(0);
    w.apply_condition(boss, condition::PARALYZED, 100000)
        .unwrap();
    w.entities
        .get_mut(&id)
        .unwrap()
        .parent
        .set(Some(boss), id, &mut w.timed_ids);
    w.players.get_mut(&p).unwrap().pos.x = pos.x + player_distance;
    (w, id, boss, p)
}

#[test]
fn lair_sibling_wander_runs_during_owned_protect_and_rest_for_both_escorts() {
    for kind in [0xdaa, 0xdac] {
        let (mut w, id, _, _) = escort(kind, 4., 1.);
        let wrong = w
            .spawn_entity(0xd90, Pos { x: 21., y: 20.5 }, vec![])
            .unwrap();
        w.apply_condition(wrong, condition::PARALYZED, 100000)
            .unwrap();
        let step = tick(&mut w, id, 50);
        assert!(step.x < 0.);
        assert!((step.y.abs() - nr_speed(0.25) / 1.5 * 0.05 / 2f32.sqrt()).abs() < 0.00001);
        assert_eq!(
            w.entities[&id].motion,
            Some(Motion::Protect {
                anchor: Anchor::Parent,
                distance: 1.
            })
        );
        let (mut w, id, _, _) = escort(kind, 0.5, 7.);
        let step = tick(&mut w, id, 50);
        assert_eq!(w.entities[&id].following, Following::Resting);
        assert!((step.x.hypot(step.y) - nr_speed(0.25) / 1.5 * 0.05).abs() < 0.00001);
    }
}

#[test]
fn lair_follow_latches_until_loss_then_reprotects_without_foreign_substitution() {
    let (mut w, id, boss, p) = escort(0xdaa, 0.5, 9.);
    tick(&mut w, id, 50);
    let pos = w.entities[&id].pos;
    w.position_entity(
        boss,
        Pos {
            x: pos.x - 7.,
            ..pos
        },
    )
    .unwrap();
    w.players.get_mut(&p).unwrap().pos.x = pos.x + 9.;
    assert!(tick(&mut w, id, 100).x > 0.);
    let pos = w.entities[&id].pos;
    w.players.get_mut(&p).unwrap().pos.x = pos.x + 10.;
    tick(&mut w, id, 150);
    assert_eq!(w.entities[&id].following, Following::Unknown);
    assert_eq!(w.entities[&id].motion, Some(Motion::Stationary));
    assert!(tick(&mut w, id, 200).x < 0.);
    w.remove_entity(boss);
    let pos = w.entities[&id].pos;
    w.spawn_entity(
        0xd90,
        Pos {
            x: pos.x + 1.,
            ..pos
        },
        vec![],
    )
    .unwrap();
    assert_eq!(tick(&mut w, id, 250).x, 0.);
}

#[test]
fn lair_blocked_primary_cannot_suppress_sibling_and_status_freezes_both() {
    let (mut w, id, _, _) = escort(0xdac, 4., 1.);
    let start = Pos { x: 20.01, y: 20.5 };
    w.position_entity(id, start).unwrap();
    wall(&mut w, Pos { x: 19.5, ..start });
    let step = tick(&mut w, id, 50);
    assert!(step.y.abs() > 0.);
    assert_eq!(
        w.entities[&id].motion,
        Some(Motion::Protect {
            anchor: Anchor::Parent,
            distance: 1.
        })
    );
    w.apply_condition(id, condition::PARALYZED, 1000).unwrap();
    assert_eq!(tick(&mut w, id, 100).x, 0.);
    assert_eq!(w.entities[&id].last_step.y, 0.);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    w.apply_condition(id, condition::STASIS, 1000).unwrap();
    tick(&mut w, id, 150);
    assert_eq!(
        w.entities[&id].pos.distance(Pos {
            x: start.x + step.x,
            y: start.y + step.y
        }),
        0.
    );
}

#[test]
fn lair_follow_rest_hysteresis_keeps_sibling_and_zero_duration_never_expires() {
    let (mut w, id, _, p) = escort(0xdac, 0.5, 7.);
    for (ms, distance) in [(50, 7.), (100, 7.001), (150, 8.)] {
        let pos = w.entities[&id].pos;
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: pos.x + distance,
            ..pos
        };
        let step = tick(&mut w, id, ms);
        assert_eq!(w.entities[&id].following, Following::Resting);
        assert!((step.x.hypot(step.y) - nr_speed(0.25) / 1.5 * 0.05).abs() < 0.00001);
    }
    // Completing a selected priority child defers a fresh selection to the next tick.
    for ms in [200, 250] {
        let pos = w.entities[&id].pos;
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: pos.x + 8.001,
            ..pos
        };
        tick(&mut w, id, ms);
    }
    assert_eq!(w.entities[&id].following, Following::Acquired);
    let home = w.entities[&id].home;
    for ms in (300..=2800).step_by(50) {
        w.position_entity(id, home).unwrap();
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: home.x + 9.,
            ..home
        };
        assert!(tick(&mut w, id, ms).x > 0.);
        assert_eq!(w.entities[&id].following, Following::Acquired);
    }
}

#[test]
fn white_demon_upstream_sight_speed_and_stop_remain_authoritative() {
    let (mut w, id, p) = actor(0x651);
    let home = w.entities[&id].home;
    for (distance, moving) in [(4., false), (4.001, true), (8.999, true)] {
        w.position_entity(id, home).unwrap();
        w.players.get_mut(&p).unwrap().pos.x = home.x + distance;
        let ms = w.now + 50;
        let step = tick(&mut w, id, ms);
        assert_eq!(step.x > 0., moving);
        if moving {
            assert!((step.x - 6. / 1.5 * 0.05).abs() < 0.00001);
        }
    }
    assert_eq!(
        w.entities[&id].motion.unwrap(),
        Motion::Chase {
            sight: 9.,
            stop: 4.
        }
    );
}
