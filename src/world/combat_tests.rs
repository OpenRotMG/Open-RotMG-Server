use super::movement_tests::fixture;
use super::*;
use crate::condition::{self, bit};

fn shot(w: &World, owner: i32, kind: u16, index: u8) -> Bullet {
    let spec = w.content.get(kind).unwrap().projectiles[&index].clone();
    Bullet {
        owner,
        source_kind: kind,
        id: 255,
        friendly: false,
        start: Pos { x: 8.5, y: 8.5 },
        pos: Pos { x: 8.5, y: 8.5 },
        angle: 0.,
        born: w.now,
        damage: spec.min,
        spec,
        hit: BTreeSet::new(),
        hit_enemy: false,
    }
}

#[test]
fn native_hits_keep_effects_separate_from_hp_protection_and_consume_once() {
    for (flags, paused, affected, damaged) in [
        (0, false, true, true),
        (bit(condition::INVULNERABLE), false, true, false),
        (bit(condition::INVINCIBLE), false, false, false),
        (bit(condition::STASIS), false, false, false),
        (0, true, false, false),
    ] {
        let (mut w, id) = fixture(0x18, false);
        w.now = 1000;
        let p = w.players.get_mut(&id).unwrap();
        p.lab_conditions = flags;
        p.paused = paused;
        let b = shot(&w, 999, 0xdf0, 0);
        w.native_player_hit(id, b.owner, b.id).unwrap();
        assert_eq!(w.players[&id].character.hp, 200);
        assert!(w.players[&id].statuses.is_empty());
        assert!(native_hits::expect_shoot_ack(
            w.players.get_mut(&id).unwrap(),
            vec![b.clone()],
            &w.clock
        ));
        w.acknowledge_shoot(id, 1000).unwrap();
        w.native_player_hit(id, b.owner, b.id).unwrap();
        let p = &w.players[&id];
        assert_eq!(p.character.hp < 200, damaged, "flags={flags}");
        assert_eq!(b.spec.effects.len(), 2);
        for (index, duration) in &b.spec.effects {
            assert_eq!(
                p.statuses.get(index).copied(),
                affected.then_some((1000, 1000 + duration)),
                "flags={flags}, effect={index}"
            );
        }
        let (hp, timers) = (p.character.hp, p.statuses.clone());
        w.now += 50;
        w.native_player_hit(id, b.owner, b.id).unwrap();
        assert_eq!(
            (w.players[&id].character.hp, &w.players[&id].statuses),
            (hp, &timers)
        );
        for ack in [-1, 1050] {
            assert!(native_hits::expect_shoot_ack(
                w.players.get_mut(&id).unwrap(),
                vec![b.clone()],
                &w.clock
            ));
            w.acknowledge_shoot(id, ack).unwrap();
            w.now += b.spec.lifetime as u64 + 5001;
            w.native_player_hit(id, b.owner, b.id).unwrap();
            assert_eq!(w.players[&id].character.hp, hp);
            assert_eq!(
                w.players[&id].conditions(w.now) & (bit(condition::SICK) | bit(condition::SLOWED)),
                0
            );
        }
    }
}

#[test]
fn area_damage_has_strict_radius_defense_floor_and_protection() {
    for (flags, amount, expected) in [
        (0, 101, 76),
        (0, 7, 1),
        (bit(condition::ARMORED), 101, 51),
        (bit(condition::ARMOR_BROKEN), 101, 101),
        (bit(condition::INVULNERABLE), 101, 0),
        (bit(condition::INVINCIBLE), 101, 0),
        (bit(condition::STASIS), 101, 0),
    ] {
        let (mut w, id) = fixture(0x18, false);
        let center = Pos { x: 5.5, y: 8.5 };
        for (distance, paused, inside) in [
            (2.999, false, true),
            (3., false, false),
            (3.001, false, false),
            (2.999, true, false),
        ] {
            let p = w.players.get_mut(&id).unwrap();
            p.pos = Pos {
                x: center.x + distance,
                y: center.y,
            };
            p.character.hp = 200;
            p.character.base[3] = 25;
            p.lab_conditions = flags;
            p.paused = paused;
            w.area_damage(0x936, center, 3., amount).unwrap();
            assert_eq!(
                w.players[&id].character.hp,
                200 - if inside { expected } else { 0 }
            );
        }
    }
    for (raw, expected) in [(1, 0), (6, 0), (7, 1), (99, 14), (101, 15)] {
        assert_eq!(condition::damage(raw, 1000, 0, false), expected);
        assert_eq!(condition::damage(raw, 1000, 0, true), raw);
    }
}

#[test]
fn selected_trajectory_modes_preserve_parity_rotation_and_return() {
    let (w, _) = fixture(0x18, false);
    let near = |actual: (f32, f32), expected: (f32, f32)| {
        assert!(
            (actual.0 - expected.0).abs() < 0.0001 && (actual.1 - expected.1).abs() < 0.0001,
            "{actual:?} != {expected:?}"
        );
    };
    let wave = &w.content.get(0xd5b).unwrap().projectiles[&0];
    for id in [0, 1] {
        let angle = std::f32::consts::PI / 64. * if id == 0 { -1. } else { 1. };
        near(
            wave.offset(id, 0., 250.),
            (2.5 * angle.cos(), 2.5 * angle.sin()),
        );
    }
    let parametric = &w.content.get(0x1740).unwrap().projectiles[&15];
    for id in 0..4 {
        near(
            parametric.offset(id, 0., 250.),
            (
                if id % 2 == 0 {
                    -0.5_f32.sqrt()
                } else {
                    0.5_f32.sqrt()
                },
                if id < 2 { 1. } else { -1. },
            ),
        );
    }
    let returning = &w.content.get(0x944).unwrap().projectiles[&0];
    for (time, distance) in [
        (0., 0.),
        (1000., 7.),
        (2000., 14.),
        (3000., 7.),
        (4000., 0.),
    ] {
        near(
            returning.offset(0, std::f32::consts::FRAC_PI_2, time),
            (0., distance),
        );
    }
    let staff = &w.content.get(0xa97).unwrap().projectiles[&0];
    for id in [0, 1] {
        near(
            staff.offset(id, 0., staff.lifetime / 8.),
            (
                staff.lifetime / 8. * staff.speed / 10000.,
                if id == 0 { 0.5 } else { -0.5 },
            ),
        );
    }
}

#[test]
fn multihit_and_cover_flags_preserve_distinct_targets_and_once_only_hits() {
    for (multi_hit, cover, passes_cover, expected) in [
        (false, None, false, [true, false]),
        (true, None, false, [true, true]),
        (true, Some(0x100), false, [false, false]),
        (true, Some(0x100), true, [true, true]),
        (true, Some(0x170), true, [false, false]),
    ] {
        let (mut w, id) = fixture(0x18, false);
        let targets =
            [10.5, 13.5].map(|x| w.spawn_entity(0x21b, Pos { x, y: 8.5 }, vec![]).unwrap());
        if let Some(kind) = cover {
            w.spawn_entity(kind, Pos { x: 9.5, y: 8.5 }, vec![])
                .unwrap();
        }
        for target in targets {
            w.entities.get_mut(&target).unwrap().hp = 2000;
        }
        let p = w.players.get_mut(&id).unwrap();
        p.character.shots = 1;
        p.character.lifetime.shots = 1;
        let hp = targets.map(|id| w.entities[&id].hp);
        let mut b = shot(&w, id, 0xb3f, 0);
        b.friendly = true;
        b.damage = 5;
        b.spec.multi_hit = multi_hit;
        b.spec.passes_cover = passes_cover;
        w.bullets.push(b);
        for _ in 0..12 {
            w.step();
        }
        for (i, target) in targets.into_iter().enumerate() {
            assert_eq!(
                w.entities[&target].hp,
                hp[i] - if expected[i] { 5 } else { 0 }
            );
        }
        assert_eq!(
            w.players[&id].character.lifetime.projectile_hits,
            u64::from(expected[0])
        );
    }
}

#[test]
fn selected_rng_keeps_equal_bounds_and_half_open_damage_draws() {
    let (mut w, id) = fixture(0x18, false);
    let p = w.players.get_mut(&id).unwrap();
    p.seed = 1;
    assert_eq!(p.roll_between(10, 10), 10);
    assert_eq!(p.seed, 1);
    for (seed, damage) in [
        (16807, 17),
        (282475249, 19),
        (1622650073, 13),
        (984943658, 18),
    ] {
        assert_eq!(p.roll_between(10, 20), damage);
        assert_eq!(p.seed, seed);
    }
}
