use super::*;
use crate::encounter::{Anchor, Motion};

fn realm() -> World {
    Directory::test_prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        12351,
    )
    .unwrap()
    .worlds
    .remove(&-3)
    .unwrap()
}
fn enemy(w: &World, kind: u16) -> i32 {
    w.entities.values().find(|e| e.kind == kind).unwrap().id
}

#[test]
fn realm_orbits_use_the_reference_radius_speed_and_owned_anchor() {
    let mut w = population_tests::family_arena(1, &[0x905]);
    w.populate_solos(1, &[Pos { x: 50.5, y: 50.5 }], &[0x6d8])
        .unwrap();
    let rock = enemy(&w, 0x905);
    let target = w.entities[&rock].pos;
    assert!(matches!(
        w.god_movement(rock, target, 0).unwrap(),
        (
            Motion::Orbit {
                anchor: Anchor::Home,
                radius: 2.
            },
            5.
        )
    ));
    for kind in [0x906, 0x907] {
        let id = enemy(&w, kind);
        assert!(matches!(
            w.god_movement(id, target, 0).unwrap(),
            (
                Motion::Orbit {
                    anchor: Anchor::CampLeader,
                    radius: 4.
                },
                5.
            )
        ));
    }
    for (kind, radius, speed) in [(0x605, 6., 5.), (0x612, 2., 8.), (0x613, 2., 8.)] {
        let leader = if kind == 0x605 { 0x609 } else { 0x614 };
        let mut family = population_tests::family_arena(10, &[leader]);
        let id = enemy(&family, kind);
        let (motion, actual_speed) = family.family_movement(id, target, 2700).unwrap();
        assert!(
            matches!(motion, Motion::Orbit {anchor: Anchor::CampLeader, radius: r} if r == radius)
        );
        assert_eq!(actual_speed, speed);
    }
    let id = enemy(&w, 0x6d8);
    assert!(matches!(
        w.god_movement(id, target, 2500).unwrap(),
        (
            Motion::Orbit {
                anchor: Anchor::Player,
                radius: 5.
            },
            8.
        )
    ));
}

#[test]
fn realm_followers_do_not_replace_chase_or_wander_priorities_with_a_freeze() {
    for (kind, stop, speed, terrain, leader) in
        [(0x607, 6., 6., 10, 0x609), (0x954, 3., 8., 11, 0x953)]
    {
        let mut w = population_tests::family_arena(terrain, &[leader]);
        let id = enemy(&w, kind);
        let camp = w.entities[&id].camp.unwrap();
        let leader = w.population[w.camps[camp].start].entity.unwrap();
        let home = w.entities[&leader].pos;
        w.entities.get_mut(&id).unwrap().pos = Pos {
            x: home.x + stop + 1.,
            y: home.y,
        };
        assert!(matches!(w.family_movement(id, home, 0).unwrap(),
            (Motion::Protect {anchor: Anchor::CampLeader, distance}, s) if distance == stop && s == speed));
        w.entities.get_mut(&id).unwrap().pos = home;
        let (motion, _) = w.family_movement(id, home, 0).unwrap();
        assert!(matches!(motion, Motion::Wander | Motion::Chase { .. }));
    }
}

#[test]
fn realm_quest_orbits_are_player_relative_and_spirits_stop_when_vulnerable() {
    let mut w = realm();
    let root = enemy(&w, 0x91f);
    let pos = w.entities[&root].pos;
    for kind in [0x923, 0x924, 0x91d, 0x91e, 0x922] {
        let id = w.spawn_entity(kind, pos, vec![]).unwrap();
        w.entities.get_mut(&id).unwrap().quest = Some(root);
        let (motion, _) = w.quest_movement(id, pos, 0);
        match kind {
            0x923 | 0x924 | 0x91d => assert!(matches!(
                motion,
                Motion::Orbit {
                    anchor: Anchor::Player,
                    radius: 4.
                }
            )),
            0x91e => {
                let (motion, _) = w.quest_movement(id, pos, 8000);
                assert!(matches!(motion, Motion::Stationary));
            }
            0x922 => {
                w.entities.get_mut(&id).unwrap().pos = Pos {
                    x: pos.x + 6.,
                    y: pos.y,
                };
                assert!(matches!(
                    w.quest_movement(id, pos, 0).0,
                    Motion::Protect {
                        anchor: Anchor::QuestMember(0x91f),
                        distance: 4.
                    }
                ));
            }
            _ => unreachable!(),
        }
    }
}

fn participant(w: &mut World, pos: Pos) -> i32 {
    let p = w.enter(tests::ch(w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = pos;
    p
}

#[test]
fn realm_construct_path_stays_at_home_and_paralysis_freezes_the_orbit_clock() {
    let mut w = realm();
    let id = enemy(&w, 0x905);
    let home = w.entities[&id].home;
    let p = participant(
        &mut w,
        Pos {
            x: home.x + 7.,
            y: home.y,
        },
    );
    for tick in 1..=80 {
        w.now = tick * 50;
        w.players.get_mut(&p).unwrap().pos.y = home.y + if tick < 40 { 3. } else { -3. };
        w.npc_step();
        assert!(w.entities[&id].pos.distance(home) <= 2.01);
        assert_eq!(w.cells.iter().filter(|c| c.contains(&id)).count(), 1);
    }
    let (pos, phase) = (w.entities[&id].pos, w.entities[&id].phase);
    w.apply_condition(id, crate::condition::PARALYZED, 1000)
        .unwrap();
    for _ in 0..10 {
        w.now += 50;
        w.npc_step();
    }
    assert_eq!(w.entities[&id].pos.distance(pos), 0.);
    assert_eq!(w.entities[&id].phase, phase);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    w.apply_condition(id, crate::condition::SLOWED, 1000)
        .unwrap();
    w.now += 50;
    w.npc_step();
    assert!(w.entities[&id].pos.distance(pos) <= 5. / 1.5 * 0.05 * 0.5 + 0.001);
}

#[test]
fn realm_leviathan_continues_its_actual_heading_instead_of_another_player_orbit() {
    let mut w = realm();
    let id = enemy(&w, 0x6d8);
    let home = w.entities[&id].home;
    participant(
        &mut w,
        Pos {
            x: home.x + 7.,
            y: home.y,
        },
    );
    w.entities.get_mut(&id).unwrap().activated = Some(0);
    for tick in 40..80 {
        w.now = tick * 50;
        w.npc_step();
    }
    let direction = w.entities[&id].last_step;
    assert!(direction.x.hypot(direction.y) > 0.);
    for tick in 80..86 {
        w.now = tick * 50;
        w.npc_step();
        let step = w.entities[&id].last_step;
        assert!((step.x * direction.y - step.y * direction.x).abs() < 0.0001);
        assert!(step.x * direction.x + step.y * direction.y > 0.);
    }
}

#[test]
fn realm_wandering_takes_local_steps_not_a_two_tile_home_patrol() {
    let mut w = realm();
    let home = w.entities[&enemy(&w, 0x905)].home;
    participant(&mut w, home);
    let id = w.spawn_entity(0x60e, home, vec![]).unwrap();
    let mut farthest = 0f32;
    for tick in 1..=1200 {
        w.now = tick * 50;
        w.npc_step();
        farthest = farthest.max(w.entities[&id].pos.distance(home));
    }
    assert!(
        farthest > 2.1,
        "wander remained tethered to home: {farthest}"
    );
    assert!(farthest <= 24.2);
}

#[test]
fn realm_pacing_starts_at_the_current_position_and_traverses_the_full_distance() {
    let mut w = realm();
    let pos = w.entities[&enemy(&w, 0x905)].home;
    participant(
        &mut w,
        Pos {
            x: pos.x + 8.,
            y: pos.y,
        },
    );
    let id = w.spawn_entity(0x92d, pos, vec![]).unwrap();
    let e = w.entities.get_mut(&id).unwrap();
    e.quest = Some(id);
    e.home.x -= 4.;
    let mut farthest = 0f32;
    for tick in 1..=60 {
        w.now = tick * 50;
        w.npc_step();
        let e = &w.entities[&id];
        assert!(e.pos.x >= pos.x - 0.001 && e.pos.x <= pos.x + 3.001);
        assert!((e.pos.y - pos.y).abs() < 0.001);
        farthest = farthest.max(e.pos.distance(pos));
    }
    assert!(farthest > 2.9);
}

#[test]
fn realm_ogres_keep_distance_and_warriors_alternate_targets() {
    let w = realm();
    for kind in [0x642, 0x644] {
        let id = enemy(&w, kind);
        assert!(matches!(
            w.midland_movement(id, w.entities[&id].pos, 0).unwrap(),
            (
                Motion::Maintain {
                    sight: 10.,
                    distance: 8.
                },
                4.
            )
        ));
    }
    let id = enemy(&w, 0x643);
    let pos = w.entities[&id].pos;
    assert!(matches!(
        w.midland_movement(id, pos, 0).unwrap().0,
        Motion::Chase {
            sight: 15.,
            stop: 2.
        }
    ));
    assert!(matches!(
        w.midland_movement(id, pos, 1500).unwrap().0,
        Motion::Protect {
            anchor: Anchor::CampLeader,
            distance: 2.
        }
    ));
}

#[test]
fn realm_quest_motor_and_orbit_clock_use_nr_core_units_including_slowing() {
    let mut w = realm();
    let pos = w.entities[&enemy(&w, 0x905)].home;
    let mut sprite = None;
    for (kind, rate) in [
        (0x91f, 0.1),
        (0x920, 0.2),
        (0x92d, 0.5),
        (0x923, 1.5),
        (0x91d, 0.55),
        (0x65f, 0.4),
        (0x676, 0.5),
    ] {
        let id = w.spawn_entity(kind, pos, vec![]).unwrap();
        let e = w.entities.get_mut(&id).unwrap();
        e.quest = Some(id);
        e.stage = Some(0);
        let (motion, speed) = w.quest_movement(id, pos, 0);
        if matches!(kind, 0x65f | 0x676) {
            assert!(matches!(motion, Motion::Wander));
        }
        let actual = speed / 1.5;
        assert!((actual - (5.55 * rate + 0.74)).abs() < 0.00001);
        if kind == 0x923 {
            sprite = Some(id);
        }
    }
    let id = sprite.unwrap();
    participant(
        &mut w,
        Pos {
            x: pos.x + 3.,
            y: pos.y,
        },
    );
    w.now = 50;
    w.npc_step();
    let phase = w.entities[&id].phase;
    w.apply_condition(id, crate::condition::SLOWED, 1000)
        .unwrap();
    w.now += 50;
    w.npc_step();
    let advance = (w.entities[&id].phase - phase).rem_euclid(std::f32::consts::TAU);
    assert!((advance - (5.55 * 1.5 + 0.74) / 4. * 0.05 * 0.5).abs() < 0.00001);
}
