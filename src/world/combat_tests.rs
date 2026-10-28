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
fn ruthven_leech_requires_actual_damage_an_ack_and_a_living_owner() {
    for (flags, living, ack) in [
        (0, true, 1000),
        (bit(condition::INVULNERABLE), true, 1000),
        (0, false, 1000),
        (0, true, -1),
    ] {
        let (mut w, player) = fixture(0x18, false);
        w.now = 1000;
        let kind = crate::dungeon::manor::BOSS;
        let boss = w
            .spawn_entity(kind, w.players[&player].pos, vec![])
            .unwrap();
        w.entities.get_mut(&boss).unwrap().hp = 14000;
        let p = w.players.get_mut(&player).unwrap();
        p.character.base[3] = 25;
        p.lab_conditions = flags;
        let b = shot(&w, boss, kind, 0);
        assert!(native_hits::expect_shoot_ack(
            w.players.get_mut(&player).unwrap(),
            vec![b.clone()],
            &w.clock
        ));
        w.native_player_hit(player, boss, b.id).unwrap();
        assert_eq!(w.players[&player].character.hp, 200);
        assert_eq!(w.entities[&boss].hp, 14000);
        w.acknowledge_shoot(player, ack).unwrap();
        if !living {
            w.remove_entity(boss);
        }
        w.native_player_hit(player, boss, b.id).unwrap();
        let hp = w.players[&player].character.hp;
        assert_eq!(hp, if flags == 0 && ack >= 0 { 125 } else { 200 });
        // The period guide specifies damage received, not announced projectile damage: https://web.archive.org/web/20120509113243id_/http://forums.wildshadow.com/wiki/realm-mad-god/lord-ruthven
        if living {
            assert_eq!(w.entities[&boss].hp, 14000 + 200 - hp);
        }
        w.native_player_hit(player, boss, b.id).unwrap();
        assert_eq!(w.players[&player].character.hp, hp);
        if living {
            assert_eq!(w.entities[&boss].hp, 14000 + 200 - hp);
        }
    }
}

#[test]
fn sprite_quiet_hit_drains_mp_without_damage_and_cannot_replay() {
    let (mut w, id) = fixture(0x18, false);
    w.now = 1000;
    let b = shot(&w, 999, 0x652, 1);
    assert_eq!(b.damage, 0);
    assert_eq!(b.spec.effects, [(condition::QUIET, 10000)]);
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&id).unwrap(),
        vec![b.clone()],
        &w.clock
    ));
    w.acknowledge_shoot(id, 1000).unwrap();
    w.native_player_hit(id, b.owner, b.id).unwrap();
    let p = &w.players[&id];
    assert_eq!((p.character.hp, p.character.mp), (200, 0));
    assert_eq!(p.statuses[&condition::QUIET], (1000, 11000));
    w.now = 1050;
    w.native_player_hit(id, b.owner, b.id).unwrap();
    assert_eq!(w.players[&id].statuses[&condition::QUIET], (1000, 11000));
    w.now = 11000;
    assert_eq!(w.players[&id].conditions(w.now) & bit(condition::QUIET), 0);
}

#[test]
fn beholder_zero_damage_star_blinds_once_for_five_seconds() {
    let (mut w, id) = fixture(0x18, false);
    w.now = 1000;
    let b = shot(&w, 999, 0x656, 1);
    assert_eq!((b.damage, &b.spec.effects), (0, &vec![(8, 5000)]));
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&id).unwrap(),
        vec![b.clone()],
        &w.clock
    ));
    w.native_player_hit(id, b.owner, b.id).unwrap();
    assert!(w.players[&id].statuses.is_empty());
    w.acknowledge_shoot(id, 1000).unwrap();
    w.native_player_hit(id, b.owner, b.id).unwrap();
    assert_eq!(w.players[&id].character.hp, 200);
    assert_eq!(w.players[&id].statuses[&8], (1000, 6000));
    w.now = 2000;
    w.native_player_hit(id, b.owner, b.id).unwrap();
    assert_eq!(w.players[&id].statuses[&8], (1000, 6000));
    assert_ne!(w.players[&id].conditions(5999) & bit(8), 0);
    assert_eq!(w.players[&id].conditions(6000) & bit(8), 0);
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
fn selected_swords_keep_damage_spread_cadence_and_first_target_collision() {
    for kind in [0xa00, 0xb0b, 0xa03, 0xc01, 0xc05, 0xaf4, 0xaf5, 0xb40] {
        let (mut w, id) = fixture(0x18, false);
        w.now = 1000;
        let targets = [9.5, 10.5].map(|x| {
            let target = w.spawn_entity(0x21b, Pos { x, y: 8.5 }, vec![]).unwrap();
            w.entities.get_mut(&target).unwrap().hp = 10000;
            target
        });
        let p = w.players.get_mut(&id).unwrap();
        p.character.class = 0x31e;
        p.character.items[0] = kind as i32;
        p.character.base = [770, 252, 50, 40, 50, 50, 75, 50];
        let item = w.content.get(kind).unwrap().clone();
        let count = if kind == 0xc01 { 2 } else { 1 };
        assert_eq!(item.shots, count);
        assert_eq!(item.arc, if count == 2 { 40. } else { 11.25 });
        assert_eq!(item.rate, if kind == 0xc05 { 0.6 } else { 1. });
        let start = Pos { x: 8.8, y: 8.5 };
        for bullet in 0..count as u8 {
            let angle = if count == 2 {
                (f32::from(bullet) * 40. - 20.).to_radians()
            } else {
                0.
            };
            w.shoot(id, 1000, bullet + 1, kind, start, angle).unwrap();
        }
        let damage: i32 = w.bullets.iter().map(|b| b.damage).sum();
        for b in &w.bullets {
            assert!(
                (item.projectiles[&0].min * 3 / 2..=item.projectiles[&0].max * 3 / 2)
                    .contains(&b.damage)
            );
            assert!(!b.spec.multi_hit && !b.spec.passes_cover && !b.spec.armor_piercing);
            assert!(!b.spec.wavy && !b.spec.parametric && !b.spec.boomerang);
        }
        let admitted = w.players[&id].character.shots;
        let period = if kind == 0xc05 { 285 } else { 171 };
        let angle = -item.arc.to_radians() * (count - 1) as f32 / 2.;
        assert!(
            w.shoot(id, 1000 + period - 3, count as u8 + 1, kind, start, angle)
                .is_err()
        );
        assert_eq!(w.players[&id].character.shots, admitted);
        for _ in 0..16 {
            w.step();
        }
        assert_eq!(w.entities[&targets[0]].hp, 10000 - damage);
        assert_eq!(w.entities[&targets[1]].hp, 10000);
        assert_eq!(
            w.players[&id].character.lifetime.projectile_hits,
            count as u64
        );
        assert!(w.bullets.iter().all(|b| b.owner != id));
        w.shoot(id, 1000 + period, count as u8 + 1, kind, start, angle)
            .unwrap();
        assert_eq!(w.players[&id].character.shots, admitted + 1);
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
fn night_elf_selected_multihit_crosses_players_and_native_reports_commit_once() {
    for (kind, damage, multi) in [
        (0x64a, 36, false),
        (0x64b, 40, false),
        (0x64c, 40, true),
        (0x64d, 52, true),
        (0x64f, 60, false),
        (0x650, 30, false),
    ] {
        let (mut w, first) = fixture(0x18, false);
        let mut character = w.players[&first].character.clone();
        character.account = 2;
        character.id = 2;
        let second = w.enter(character, None).unwrap();
        for (id, x) in [(first, 10.), (second, 12.)] {
            let p = w.players.get_mut(&id).unwrap();
            p.pos = Pos { x, y: 8.5 };
            p.controller = Controller::Qa;
            p.lab_conditions = bit(condition::SICK);
        }
        let b = shot(&w, 999, kind, 0);
        assert_eq!(
            (b.damage, b.spec.multi_hit, b.spec.effects.is_empty()),
            (damage, multi, true)
        );
        w.bullets.push(b.clone());
        for _ in 0..30 {
            w.step();
        }
        assert_eq!(w.players[&first].character.hp, 200 - damage);
        assert_eq!(
            w.players[&second].character.hp,
            200 - if multi { damage } else { 0 }
        );
        for id in [first, second] {
            let p = w.players.get_mut(&id).unwrap();
            p.controller = Controller::Human;
            p.character.hp = 200;
            assert!(native_hits::expect_shoot_ack(p, vec![b.clone()], &w.clock));
            w.acknowledge_shoot(id, 1500).unwrap();
            w.native_player_hit(id, b.owner, b.id).unwrap();
            w.native_player_hit(id, b.owner, b.id).unwrap();
            assert_eq!(w.players[&id].character.hp, 200 - damage);
        }
    }
}

#[test]
fn nomad_primary_cycle_and_selected_blind_are_independent() {
    let (mut w, player) = fixture(0x18, false);
    let mut clocks = crate::encounter::attacks::Clocks::default();
    for age in (0..10600).step_by(50) {
        let shots = crate::encounter::phased_attacks(0x6c6, 140, 1., age);
        let phase = age % 5300;
        let magic = phase < 3100;
        let elapsed = if magic { phase } else { phase - 3100 };
        let interval = if magic { 500 } else { 700 };
        let due = elapsed >= interval && elapsed % interval == 0;
        assert_eq!(
            shots.len(),
            if due { if magic { 3 } else { 1 } } else { 0 },
            "age={age}"
        );
        let schedule = crate::encounter::attacks::schedule(0x6c6, 140, age).unwrap();
        assert_eq!(
            clocks.due(&schedule, age).len(),
            usize::from(due),
            "age={age}"
        );
        for (i, (index, angle)) in shots.iter().enumerate() {
            assert_eq!(*index, u8::from(!magic));
            let expected = 1.
                + if magic {
                    (i as f32 - 1.) * 11f32.to_radians()
                } else {
                    0.
                };
            assert!((angle - expected).abs() < 0.0001);
        }
    }
    w.now = 1000;
    let b = shot(&w, 999, 0x6c6, 1);
    assert_eq!((b.damage, b.spec.effects.clone()), (30, vec![(8, 1500)]));
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&player).unwrap(),
        vec![b],
        &w.clock
    ));
    w.acknowledge_shoot(player, 1000).unwrap();
    w.native_player_hit(player, 999, 255).unwrap();
    assert_eq!(w.players[&player].character.hp, 170);
    assert_ne!(w.players[&player].conditions(2499) & bit(8), 0);
    assert_eq!(w.players[&player].conditions(2500) & bit(8), 0);
    w.native_player_hit(player, 999, 255).unwrap();
    assert_eq!(w.players[&player].character.hp, 170);
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
