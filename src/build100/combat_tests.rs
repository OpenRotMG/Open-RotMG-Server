use super::*;
use crate::content::Content;

fn mask(conditions: &[Condition]) -> ConditionMask {
    ConditionMask::from_bits(
        conditions
            .iter()
            .fold(0, |bits, c| bits | ConditionMask::bit(*c)),
    )
    .unwrap()
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
}
fn content(repaired: bool) -> Content {
    let variable = if repaired {
        "BUILD100_CONTENT_REPAIRED_DIR"
    } else {
        "BUILD100_CONTENT_ORIGINAL_DIR"
    };
    Content::load(std::path::Path::new(
        &std::env::var(variable).expect(variable),
    ))
    .unwrap()
}
fn stats(hp: i32, mp: i32, hp_regen: i32, mp_regen: i32) -> Stats {
    Stats::from_base(
        &[
            (0, hp),
            (3, mp),
            (20, 0),
            (21, 0),
            (22, 0),
            (26, hp_regen),
            (27, mp_regen),
            (28, 0),
        ]
        .into(),
    )
    .unwrap()
}
fn vitals(hp: i32, mp: i32) -> Vitals {
    Vitals {
        hp,
        mp,
        hp_remainder: 0.0,
        mp_remainder: 0.0,
        hp_roundoff: 0.0,
        mp_roundoff: 0.0,
    }
}
fn projectile() -> Projectile {
    Projectile {
        visual: "test".into(),
        min: 30,
        max: 50,
        speed: 10.0,
        lifetime: 2.0,
        amplitude: 0.0,
        frequency: 1.0,
        wavy: false,
        continues: false,
        effects: vec![],
        features: vec![],
    }
}

#[test]
fn combat_helpers_condition_masks_refresh_and_expiry() {
    use Condition::*;
    let all = [
        Nothing,
        Dead,
        Quiet,
        Weak,
        Slowed,
        Sick,
        Dazed,
        Stunned,
        Blind,
        Hallucinating,
        Drunk,
        Confused,
        Knockback,
        Invisible,
        Paralyzed,
        Jumpy,
        Bleeding,
        ArmorPierce,
        Healing,
        Damaging,
        Berserk,
        Paused,
    ];
    let mut timed = Conditions::default();
    for (i, c) in all.into_iter().enumerate() {
        assert_eq!(ConditionMask::bit(c), if i == 0 { 0 } else { 1 << (i - 1) });
        timed
            .refresh(
                &ConditionEffect {
                    condition: c,
                    duration_seconds: 2.0,
                },
                0.0,
            )
            .unwrap();
    }
    assert_eq!(timed.expire(0.0).unwrap().bits(), (1 << 21) - 1);
    assert_eq!(timed.next_expiry_seconds(), Some(2.0));
    assert_eq!(timed.expire(2.0).unwrap().bits(), 0);
    assert_eq!(timed.next_expiry_seconds(), None);
    let effect = |duration_seconds| ConditionEffect {
        condition: Weak,
        duration_seconds,
    };
    timed.refresh(&effect(3.0), 2.0).unwrap();
    timed.refresh(&effect(1.0), 3.0).unwrap();
    assert!(timed.expire(4.0).unwrap().contains(Weak));
    timed.refresh(&effect(4.0), 4.0).unwrap();
    assert!(timed.expire(7.999).unwrap().contains(Weak));
    assert!(!timed.expire(8.0).unwrap().contains(Weak));
    assert_eq!(timed.refresh(&effect(0.0), 8.0).unwrap().bits(), 0);
    assert!(timed.expire(7.0).is_err());
    assert!(timed.expire(f64::NAN).is_err());
    assert!(timed.expire(-1.0).is_err());
    assert!(timed.refresh(&effect(-1.0), 8.0).is_err());
    assert!(timed.refresh(&effect(f32::INFINITY), 8.0).is_err());
    assert!(timed.refresh(&effect(1.0), f64::MAX).is_err());
    assert!(ConditionMask::from_bits(1 << 21).is_err());
    assert!(ConditionMask::from_bits(u32::MAX).is_err());
}

#[test]
fn combat_helpers_player_early_return_precedence_no_modern_dazed_or_stat_cap() {
    use Condition::*;
    let empty = mask(&[]);
    for (spd, expected) in [(0, 4.0), (75, 9.6), (150, 15.2)] {
        close(movement_tiles_per_second(spd, empty).unwrap(), expected);
    }
    close(
        movement_tiles_per_second(75, mask(&[Berserk])).unwrap(),
        14.4,
    );
    close(
        movement_tiles_per_second(75, mask(&[Slowed, Berserk])).unwrap(),
        4.0,
    );
    for c in [Paralyzed, Paused] {
        assert_eq!(
            movement_tiles_per_second(75, mask(&[c, Berserk])).unwrap(),
            0.0
        );
    }
    for (atk, expected) in [(0, 0.5), (25, 1.0), (75, 2.0), (150, 3.5)] {
        close(attack_factor(atk, empty).unwrap(), expected);
    }
    close(attack_factor(75, mask(&[Damaging])).unwrap(), 4.0);
    close(attack_factor(75, mask(&[Weak, Damaging])).unwrap(), 0.5);
    assert!(movement_tiles_per_second(-1, empty).is_err());
    assert!(attack_factor(-1, empty).is_err());
}

#[test]
fn cadence_pacing_source_int_schedule() {
    let c = content(true);
    let mut item = c.named("Energy Staff").unwrap().clone();
    for (dex, period) in [(15, 357.), (16, 346.), (17, 336.)] {
        assert_eq!(weapon_interval_ms(&item, dex, mask(&[])).unwrap(), period);
        let interval = weapon_interval_ms(&item, dex, mask(&[])).unwrap();
        assert!(!cooldown_ready(period - 0.001, Some(0.), interval).unwrap());
        assert!(cooldown_ready(period, Some(0.), interval).unwrap());
    }
    item.rate_of_fire = Some(1.5);
    assert_eq!(
        weapon_interval_ms(&item, 17, mask(&[])).unwrap(),
        ((1.0_f64 / (0.0015 + 17.0 / 75.0 * (0.008 - 0.0015))) * (1.0 / 1.5)).trunc()
    );
    for rof in [f32::MIN_POSITIVE, f32::MAX] {
        item.rate_of_fire = Some(rof);
        assert!(weapon_interval_ms(&item, 17, mask(&[])).is_err());
    }
}

#[test]
fn combat_helpers_cadence_gates_and_exact_cooldown_boundaries() {
    use Condition::*;
    let c = content(true);
    let weapon = c.named("Blunt Sword").unwrap();
    close(weapon_interval_ms(weapon, 0, mask(&[])).unwrap(), 666.0);
    close(weapon_interval_ms(weapon, 75, mask(&[])).unwrap(), 125.0);
    close(weapon_interval_ms(weapon, 150, mask(&[])).unwrap(), 68.0);
    close(
        weapon_interval_ms(weapon, 75, mask(&[Berserk, Dazed])).unwrap(),
        83.0,
    );
    close(
        weapon_interval_ms(weapon, 75, mask(&[Dazed])).unwrap(),
        125.0,
    );
    let interval = weapon_interval_ms(weapon, 75, mask(&[])).unwrap();
    assert!(!cooldown_ready(124.999, Some(0.0), interval).unwrap());
    assert!(cooldown_ready(125.0, Some(0.0), interval).unwrap());
    assert!(cooldown_ready(0.0, None, interval).unwrap());
    let ability = c.named("Fire Spray Spell").unwrap();
    assert!(!ability_ready(ability, 40, 100, mask(&[]), 499.999, Some(0.0)).unwrap());
    assert!(ability_ready(ability, 40, 100, mask(&[Stunned]), 500.0, Some(0.0)).unwrap());
    assert!(!ability_ready(ability, 39, 100, mask(&[]), 500.0, Some(0.0)).unwrap());
    for condition in [Quiet, Paused] {
        assert!(!ability_ready(ability, 100, 100, mask(&[condition]), 500.0, None).unwrap());
    }
    assert!(ability_ready(ability, -1, 100, mask(&[]), 500.0, None).is_err());
    assert!(ability_ready(ability, 101, 100, mask(&[]), 500.0, None).is_err());
    assert!(ability_ready(ability, 0, -1, mask(&[]), 500.0, None).is_err());
    for (now, previous, interval) in [
        (-1.0, None, 1.0),
        (f64::NAN, None, 1.0),
        (0.0, Some(1.0), 1.0),
        (0.0, Some(-1.0), 1.0),
        (0.0, None, 0.0),
        (0.0, None, f64::INFINITY),
    ] {
        assert!(cooldown_ready(now, previous, interval).is_err());
    }
    let mut bad = weapon.clone();
    bad.rate_of_fire = None;
    assert!(
        weapon_interval_ms(&bad, 75, mask(&[]))
            .unwrap_err()
            .to_string()
            .contains("Blunt Sword missing root")
    );
    for rof in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        bad.rate_of_fire = Some(rof);
        assert!(weapon_interval_ms(&bad, 75, mask(&[])).is_err());
    }
    assert!(weapon_interval_ms(weapon, -1, mask(&[])).is_err());
    bad.mp_cost = None;
    assert!(ability_ready(&bad, 100, 100, mask(&[]), 0.0, None).is_err());
    bad.mp_cost = Some(-1);
    assert!(ability_ready(&bad, 100, 100, mask(&[]), 0.0, None).is_err());
}

#[test]
fn combat_helpers_continuous_damage_source_multiplier_and_quarter_floor() {
    let c = content(false);
    let bow = c.named("Poor Quality Bow").unwrap();
    let p = &bow.projectiles[0].1;
    let weapon = || AttackScaling::Weapon {
        attack: 75,
        conditions: mask(&[]),
    };
    assert_eq!(
        continuous_damage(DamageSource::Root(bow), weapon(), 0.0).unwrap(),
        900
    );
    assert_eq!(
        continuous_damage(DamageSource::Root(bow), weapon(), 0.999999999999).unwrap(),
        999
    );
    // Continuous base450.75 * .5=225.375, not an inclusive integer roll.
    assert_eq!(
        continuous_damage(
            DamageSource::Root(bow),
            AttackScaling::Weapon {
                attack: 0,
                conditions: mask(&[])
            },
            0.015
        )
        .unwrap(),
        225
    );
    assert_eq!(
        continuous_damage(DamageSource::Projectile(p), AttackScaling::Unscaled, 0.0).unwrap(),
        10
    );
    let mut tiny = p.clone();
    tiny.min = 3;
    tiny.max = 4;
    assert_eq!(
        continuous_damage(DamageSource::Projectile(&tiny), weapon(), 0.75).unwrap(),
        7
    );
    assert_eq!(
        continuous_damage(
            DamageSource::Projectile(&tiny),
            AttackScaling::Unscaled,
            0.999999
        )
        .unwrap(),
        3
    );
    for random in [-0.1, 1.0, f64::NAN, f64::INFINITY] {
        assert!(continuous_damage(DamageSource::Root(bow), weapon(), random).is_err());
    }
    assert!(
        continuous_damage(
            DamageSource::Root(bow),
            AttackScaling::Weapon {
                attack: i32::MAX,
                conditions: mask(&[])
            },
            0.0
        )
        .is_err()
    );
    let mut missing = bow.clone();
    missing.root_damage = None;
    assert!(
        continuous_damage(DamageSource::Root(&missing), weapon(), 0.0)
            .unwrap_err()
            .to_string()
            .contains("Poor Quality Bow missing root damage")
    );
    for range in [(-1, 1), (2, 1), (0, 32768)] {
        missing.root_damage = Some(range);
        assert!(continuous_damage(DamageSource::Root(&missing), weapon(), 0.0).is_err());
    }
    for (raw, def, expected) in [
        (100, 74, 26),
        (100, 75, 25),
        (7, 100, 1),
        (3, 100, 0),
        (0, 0, 0),
    ] {
        assert_eq!(projectile_defended_damage(raw, def, p).unwrap(), expected);
    }
    tiny.effects.push(ConditionEffect {
        condition: Condition::ArmorPierce,
        duration_seconds: 0.0,
    });
    assert_eq!(projectile_defended_damage(100, 1000, &tiny).unwrap(), 100);
    for (raw, def) in [(-1, 0), (32768, 0), (1, -1)] {
        assert!(projectile_defended_damage(raw, def, &tiny).is_err());
    }
    for (name, expected) in [("Tattered Quiver", 60), ("Wooden Shield", 50)] {
        let ability = c.named(name).unwrap();
        assert_eq!(
            ability.activations[0].action().unwrap(),
            crate::content::Action::Shoot
        );
        assert_eq!(
            continuous_damage(DamageSource::Root(ability), AttackScaling::Unscaled, 0.5).unwrap(),
            expected
        );
    }
    // Fire Spray's source must be explicitly selected, not globally normalized to root.
    let spell = c.named("Fire Spray Spell").unwrap();
    assert_eq!(
        continuous_damage(DamageSource::Root(spell), AttackScaling::Unscaled, 0.5).unwrap(),
        400
    );
    assert_eq!(
        continuous_damage(
            DamageSource::Projectile(&spell.projectiles[0].1),
            AttackScaling::Unscaled,
            0.5
        )
        .unwrap(),
        40
    );
}

#[test]
fn weapon_runtime_continuous_damage_bow_number_rounding_endpoint() {
    let random = f64::from_bits(0x3feffffffffffffe);
    assert!(random < 1.0);
    let factor = 0.5 + 12.0 / 75.0 * 1.5;
    assert_eq!(factor, 0.74);
    for repaired in [false, true] {
        let c = content(repaired);
        let bow = c.named("Poor Quality Bow").unwrap();
        let (min, max, lower, upper) = if repaired {
            (10, 40, 7, 29)
        } else {
            (450, 500, 333, 370)
        };
        assert_eq!(bow.root_damage, Some((min, max)));
        let source_base = f64::from(min) + f64::from(max - min) * random;
        if !repaired {
            assert_eq!(source_base, 500.0);
        }
        let source_damage = (source_base * factor) as i32;
        assert_eq!(source_damage, upper);
        let scaling = || AttackScaling::Weapon {
            attack: 12,
            conditions: mask(&[]),
        };
        assert_eq!(
            continuous_damage(DamageSource::Root(bow), scaling(), random).unwrap(),
            source_damage
        );
        assert_eq!(
            continuous_damage(DamageSource::Root(bow), scaling(), 0.0).unwrap(),
            lower
        );
        let bounds = if repaired { 7..=29 } else { 333..=370 };
        assert!(bounds.contains(&source_damage));
    }
}

#[test]
fn combat_helpers_multishot_radians_centering_and_defaults() {
    let c = content(false);
    let mut item = c.named("Energy Staff").unwrap().clone();
    assert_eq!(shot_angles(&item, 1.0).unwrap(), [1.0, 1.0]);
    for (count, gap, expected) in [
        (1, 0.5, vec![1.0]),
        (2, 0.5, vec![0.75, 1.25]),
        (3, 0.5, vec![0.5, 1.0, 1.5]),
    ] {
        item.num_projectiles = count;
        item.arc_gap_radians = gap;
        assert_eq!(shot_angles(&item, 1.0).unwrap(), expected);
    }
    let parsed = Content::parse(
        "<Objects><Object type='782' id='Wizard'><Class>Player</Class></Object><Object type='1536' id='Pirate'><Class>Character</Class></Object><Object type='1280' id='Loot Bag'><Class>Container</Class></Object><Object type='2594' id='Health Potion'><Class>Equipment</Class></Object><Object type='2711' id='Energy Staff'><Class>Equipment</Class></Object><Object type='1' id='default'><Class>Equipment</Class></Object></Objects>",
    )
    .unwrap();
    let default = parsed.named("default").unwrap();
    close(
        f64::from(default.arc_gap_radians),
        f64::from(std::f32::consts::PI / 16.0),
    );
    assert_eq!(shot_angles(default, 0.0).unwrap(), [0.0]);
    item.num_projectiles = 0;
    assert!(shot_angles(&item, 0.0).is_err());
    item.num_projectiles = 2;
    for gap in [-1.0, f32::NAN, f32::INFINITY] {
        item.arc_gap_radians = gap;
        assert!(shot_angles(&item, 0.0).is_err());
    }
    assert!(shot_angles(default, f64::NAN).is_err());
}

#[test]
fn combat_helpers_projectile_original_wave_wavy_precedence_parity_and_expiry() {
    let mut p = projectile();
    assert_eq!(
        projectile_position(&p, (1.0, 2.0), 0.0, 0, 0.0).unwrap(),
        Some((1.0, 2.0))
    );
    assert_eq!(
        projectile_position(&p, (1.0, 2.0), 0.0, 0, 0.5).unwrap(),
        Some((6.0, 2.0))
    );
    let pos = projectile_position(&p, (0.0, 0.0), PI / 2.0, 0, 1.0)
        .unwrap()
        .unwrap();
    close(pos.0, 0.0);
    close(pos.1, 10.0);
    p.amplitude = 2.0;
    for (id, y) in [(0, 2.0), (1, -2.0)] {
        let pos = projectile_position(&p, (0.0, 0.0), 0.0, id, 0.5)
            .unwrap()
            .unwrap();
        close(pos.0, 5.0);
        close(pos.1, y);
    }
    p.frequency = 2.0;
    close(
        projectile_position(&p, (0.0, 0.0), 0.0, 0, 0.25)
            .unwrap()
            .unwrap()
            .1,
        2.0,
    );
    p.wavy = true;
    for (id, sign) in [(0, 1.0), (1, -1.0)] {
        let pos = projectile_position(&p, (0.0, 0.0), 0.0, id, 1.0 / 12.0)
            .unwrap()
            .unwrap();
        close(pos.0, (10.0 / 12.0) * (PI / 64.0).cos());
        close(pos.1, sign * (10.0 / 12.0) * (PI / 64.0).sin());
    }
    p.amplitude = 0.0;
    p.continues = true;
    let straight_wavy = projectile_position(&p, (0.0, 0.0), 0.0, 0, 1.0 / 12.0).unwrap();
    p.amplitude = 1000.0;
    assert_eq!(
        projectile_position(&p, (0.0, 0.0), 0.0, 0, 1.0 / 12.0).unwrap(),
        straight_wavy
    );
    assert!(
        projectile_position(&p, (0.0, 0.0), 0.0, 0, 1.999)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        projectile_position(&p, (0.0, 0.0), 0.0, 0, 2.0).unwrap(),
        None
    );
    assert_eq!(
        projectile_position(&p, (0.0, 0.0), 0.0, 0, 3.0).unwrap(),
        None
    );
    p.lifetime = 0.0;
    assert_eq!(
        projectile_position(&p, (0.0, 0.0), 0.0, 0, 0.0).unwrap(),
        None
    );
    for age in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(projectile_position(&p, (0.0, 0.0), 0.0, 0, age).is_err());
    }
    p = projectile();
    p.speed = f32::NAN;
    assert!(projectile_position(&p, (0.0, 0.0), 0.0, 0, 0.0).is_err());
    p = projectile();
    p.lifetime = -1.0;
    assert!(projectile_position(&p, (0.0, 0.0), 0.0, 0, 0.0).is_err());
    p = projectile();
    p.frequency = f32::INFINITY;
    assert!(projectile_position(&p, (0.0, 0.0), 0.0, 0, 0.0).is_err());
    p = projectile();
    p.features.push("Unknown".into());
    assert!(projectile_position(&p, (0.0, 0.0), 0.0, 0, 0.0).is_err());
    p = projectile();
    assert!(projectile_position(&p, (f64::NAN, 0.0), 0.0, 0, 0.0).is_err());
    assert!(projectile_position(&p, (0.0, 0.0), f64::INFINITY, 0, 0.0).is_err());
}

#[test]
fn combat_helpers_qualified_projectile_units_wave_and_static_armor_pierce() {
    for repaired in [false, true] {
        let c = content(repaired);
        let staff = c.named("Energy Staff").unwrap();
        let p = staff.normal_projectile(0).unwrap();
        for (bullet_id, y) in [(0, 0.5), (1, -0.5)] {
            let position = projectile_position(p, (0.0, 0.0), 0.0, bullet_id, 0.125)
                .unwrap()
                .unwrap();
            close(position.0, 1.625);
            close(position.1, y);
        }
        assert_eq!(
            projectile_position(p, (0.0, 0.0), 0.0, 0, 1.0).unwrap(),
            None
        );
        let cube = c.named("Cube Defender").unwrap();
        let position = projectile_position(&cube.projectiles[0].1, (0.0, 0.0), 0.0, 0, 1.0 / 12.0)
            .unwrap()
            .unwrap();
        close(position.0, (PI / 64.0).cos());
        close(position.1, (PI / 64.0).sin());
        assert!(cube.normal_projectile(0).unwrap().wavy);
        let oryx = c.named("Oryx the Mad God").unwrap();
        let piercing = oryx
            .projectiles
            .iter()
            .find(|(_, p)| p.armor_piercing())
            .unwrap();
        assert!(
            piercing
                .1
                .effects
                .iter()
                .any(|effect| effect.condition == Condition::ArmorPierce
                    && effect.duration_seconds == 0.0)
        );
        assert_eq!(
            projectile_defended_damage(100, 1000, &piercing.1).unwrap(),
            100
        );
    }
}

#[test]
fn combat_helpers_qualified_all_seven_equipment_loadouts_and_root_only_bonuses() {
    for repaired in [false, true] {
        let c = content(repaired);
        for (name, base_values, def_bonus) in [
            (
                "Rogue",
                [150, 100, 10, 0, 15, 15, 10, 15],
                if repaired { 2 } else { 10 },
            ),
            ("Archer", [130, 100, 12, 0, 12, 50, 10, 12], 0),
            ("Wizard", [100, 100, 12, 0, 10, 12, 12, 15], 0),
            ("Priest", [100, 100, 10, 0, 12, 10, 15, 12], 0),
            (
                "Warrior",
                [200, 100, 15, 0, 7, 10, 10, 10],
                if repaired { 2 } else { 500 },
            ),
            ("Knight", [200, 100, 15, 0, 7, 10, 10, 10], 2),
            ("Paladin", [200, 100, 12, 0, 7, 10, 10, 10], 0),
        ] {
            let class = c.named(name).unwrap();
            let base = Stats::from_base(&class.stats).unwrap();
            assert_eq!(base.0, base_values, "{name}");
            let inventory: Vec<_> = class
                .equipment
                .iter()
                .map(|id| {
                    if *id == -1 {
                        None
                    } else {
                        Some(c.get(*id as i16).unwrap())
                    }
                })
                .collect();
            let effective = equipment_stats(&base, &class.slots, &inventory).unwrap();
            let mut expected = base_values;
            expected[3] += def_bonus;
            assert_eq!(effective.0, expected, "{name}");
            assert_eq!(base.0, base_values);
            let mut wrong = inventory.clone();
            wrong[0] = inventory[1];
            assert!(equipment_stats(&base, &class.slots, &wrong).is_err());
            let mut with_ring = inventory.clone();
            with_ring[3] = Some(c.named("Ring of Minor Defense").unwrap());
            let ring_stats = equipment_stats(&base, &class.slots, &with_ring).unwrap();
            assert_eq!(
                ring_stats.get(20).unwrap(),
                base_values[2] + if repaired { 0 } else { 5 }
            );
            assert_eq!(
                ring_stats.get(21).unwrap(),
                def_bonus + if repaired { 1 } else { 0 }
            );
            // Backpack is not equipped, even if its item has valid root bonuses.
            with_ring[3] = None;
            with_ring[5] = Some(c.named("Ring of Minor Defense").unwrap());
            assert_eq!(
                equipment_stats(&base, &class.slots, &with_ring).unwrap(),
                effective
            );
        }
    }
}

#[test]
fn combat_helpers_effective_stat_validation_overflow_and_no_equipment_cap() {
    let c = content(true);
    let base = stats(100, 100, 0, 0);
    let mut item = c.named("Ring of Minor Defense").unwrap().clone();
    item.bonuses = vec![(20, 100), (0, 100)];
    let effective = equipment_stats(&base, &[9], &[Some(&item)]).unwrap();
    assert_eq!(effective.get(20).unwrap(), 100);
    assert_eq!(effective.get(0).unwrap(), 200);
    assert!(equipment_stats(&base, &[18], &[None]).is_err());
    assert!(equipment_stats(&base, &[], &[]).is_err());
    assert!(equipment_stats(&base, &[9], &[]).is_err());
    item.slot_type = None;
    assert!(equipment_stats(&base, &[9], &[Some(&item)]).is_err());
    item.slot_type = Some(9);
    for bonuses in [vec![(1, 1)], vec![(20, -1)], vec![(0, i32::MAX)]] {
        item.bonuses = bonuses;
        assert!(equipment_stats(&base, &[9], &[Some(&item)]).is_err());
    }
    let mut map: BTreeMap<_, _> = STAT_IDS
        .into_iter()
        .zip([100, 100, 0, 0, 0, 0, 0, 0])
        .collect();
    map.remove(&26);
    assert!(
        Stats::from_base(&map)
            .unwrap_err()
            .to_string()
            .contains("missing permanent stat 26")
    );
    map.insert(26, 0);
    map.insert(99, 1);
    assert!(Stats::from_base(&map).is_err());
    map.remove(&99);
    map.insert(0, 0);
    assert!(Stats::from_base(&map).is_err());
    assert!(base.get(25).is_err());
    let old = stats(100, 100, 0, 0);
    let larger = stats(200, 200, 0, 0);
    let smaller = stats(50, 50, 0, 0);
    assert_eq!(
        vitals(80, 80).rebase(&old, &larger).unwrap(),
        vitals(80, 80)
    );
    assert_eq!(
        vitals(80, 80).rebase(&old, &smaller).unwrap(),
        vitals(50, 50)
    );
    let fractional = Vitals {
        hp_remainder: 0.5,
        mp_remainder: 0.5,
        ..vitals(50, 50)
    };
    assert_eq!(fractional.rebase(&old, &smaller).unwrap(), vitals(50, 50));
    let debt = Vitals {
        hp_remainder: -0.5,
        ..vitals(50, 50)
    };
    assert_eq!(debt.rebase(&old, &smaller).unwrap().hp_remainder, -0.5);
    assert!(vitals(101, 100).rebase(&old, &larger).is_err());
}

#[test]
fn combat_helpers_regen_fractional_caps_quiet_sick_healing_and_bleeding() {
    use Condition::*;
    let s = stats(100, 100, 8, 0);
    let first = vitals(50, 50).regenerate(0.25, &s, mask(&[])).unwrap();
    assert_eq!((first.hp, first.mp), (50, 50));
    close(first.hp_remainder, 0.5);
    close(first.mp_remainder, 0.2);
    let second = first.regenerate(0.25, &s, mask(&[])).unwrap();
    assert_eq!((second.hp, second.mp), (51, 50));
    close(second.hp_remainder, 0.0);
    close(second.mp_remainder, 0.4);
    let full = vitals(99, 99).regenerate(10.0, &s, mask(&[])).unwrap();
    assert_eq!(full, vitals(100, 100));
    assert_eq!(
        vitals(80, 80)
            .regenerate(1.0, &s, mask(&[Quiet]))
            .unwrap()
            .mp,
        0
    );
    assert_eq!(
        vitals(80, 80)
            .regenerate(0.0, &s, mask(&[Quiet]))
            .unwrap()
            .mp,
        0
    );
    assert_eq!(
        vitals(80, 80)
            .regenerate(1.0, &s, mask(&[Sick, Healing]))
            .unwrap()
            .hp,
        80
    );
    assert_eq!(
        vitals(50, 80)
            .regenerate(1.0, &s, mask(&[Healing]))
            .unwrap()
            .hp,
        72
    );
    assert_eq!(
        vitals(50, 80)
            .regenerate(1.0, &s, mask(&[Healing, Bleeding]))
            .unwrap()
            .hp,
        52
    );
    assert_eq!(
        vitals(50, 80)
            .regenerate(1.0, &s, mask(&[Sick, Bleeding]))
            .unwrap()
            .hp,
        30
    );
    let dead = vitals(10, 0)
        .regenerate(1.0, &s, mask(&[Sick, Bleeding]))
        .unwrap();
    assert_eq!(dead.hp, 0);
    assert_eq!(dead.hp_remainder, 0.0);
    assert_eq!(dead.regenerate(100.0, &s, mask(&[Healing])).unwrap().hp, 0);
    assert_eq!(dead.heal(1000, &s, mask(&[])).unwrap().hp, 0);
    assert_eq!(
        vitals(80, 80).heal(1000, &s, mask(&[Sick])).unwrap(),
        vitals(80, 80)
    );
    assert_eq!(
        vitals(100, 80).heal(1000, &s, mask(&[])).unwrap(),
        vitals(100, 80)
    );
    let s2 = stats(100, 100, 0, 167);
    let mp_regenerated = vitals(10, 10).regenerate(0.5, &s2, mask(&[])).unwrap();
    assert_eq!(mp_regenerated.mp, 15);
    close(mp_regenerated.mp_remainder, 0.4);
    // Paused alone neither freezes timers nor grants invulnerability.
    assert_eq!(
        vitals(50, 50)
            .regenerate(1.0, &s, mask(&[Paused, Bleeding]))
            .unwrap()
            .hp,
        32
    );
}

#[test]
fn combat_helpers_net_hp_partition_invariance_cap_bleeding_debt_and_instant_heal() {
    use Condition::*;
    let s = stats(100, 100, 0, 0);
    for (start, conditions) in [
        (vitals(50, 0), mask(&[])),
        (vitals(100, 0), mask(&[Bleeding])),
        (vitals(90, 0), mask(&[Healing, Bleeding])),
        (vitals(10, 0), mask(&[Sick, Bleeding])),
        (vitals(99, 0), mask(&[Healing])),
    ] {
        let once = start.regenerate(1.0, &s, conditions).unwrap();
        let mut split = start;
        for _ in 0..5 {
            split = split.regenerate(0.2, &s, conditions).unwrap();
        }
        assert_eq!((split.hp, split.mp), (once.hp, once.mp));
        close(split.hp_remainder, once.hp_remainder);
        close(split.mp_remainder, once.mp_remainder);
    }
    let debt = vitals(100, 50)
        .regenerate(0.01, &s, mask(&[Bleeding]))
        .unwrap();
    assert_eq!(debt.hp, 100);
    close(debt.hp_remainder, -0.19);
    let next = debt.regenerate(0.05, &s, mask(&[Bleeding])).unwrap();
    assert_eq!(next.hp, 99);
    close(next.hp_remainder, -0.14);
    assert_eq!(debt.heal(1, &s, mask(&[])).unwrap().hp_remainder, 0.0);
    assert_eq!(debt.heal(0, &s, mask(&[])).unwrap(), debt);
    assert_eq!(debt.rebase(&s, &s).unwrap(), debt);
    let sat = vitals(99, 50).regenerate(1.0, &s, mask(&[])).unwrap();
    assert_eq!(sat.hp, 100);
    assert_eq!(sat.hp_remainder, 0.0);
    let mut timed = Conditions::default();
    timed
        .refresh(
            &ConditionEffect {
                condition: Healing,
                duration_seconds: 0.5,
            },
            0.0,
        )
        .unwrap();
    assert_eq!(timed.next_expiry_seconds(), Some(0.5));
    let half = vitals(10, 0)
        .regenerate(0.5, &s, timed.expire(0.0).unwrap())
        .unwrap();
    let end = half
        .regenerate(0.5, &s, timed.expire(0.5).unwrap())
        .unwrap();
    assert_eq!(end.hp, 21);
}

#[test]
fn combat_helpers_vital_domains_nonfinite_overflow_and_invalid_state_not_repaired() {
    let s = stats(100, 100, 8, 0);
    let empty = mask(&[]);
    for invalid in [
        vitals(-1, 0),
        vitals(101, 0),
        vitals(1, -1),
        vitals(1, 101),
        Vitals {
            hp_remainder: f64::NAN,
            ..vitals(50, 50)
        },
        Vitals {
            hp_remainder: 1.0,
            ..vitals(50, 50)
        },
        Vitals {
            mp_remainder: -0.1,
            ..vitals(50, 50)
        },
        Vitals {
            mp_remainder: 0.1,
            ..vitals(50, 100)
        },
        Vitals {
            hp_remainder: 0.1,
            ..vitals(100, 50)
        },
        Vitals {
            hp_remainder: -0.1,
            ..vitals(0, 50)
        },
    ] {
        assert!(invalid.regenerate(1.0, &s, empty).is_err());
        assert!(invalid.heal(1000, &s, empty).is_err());
    }
    for elapsed in [-1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(vitals(50, 50).regenerate(elapsed, &s, empty).is_err());
    }
    assert_eq!(
        vitals(50, 50).regenerate(0.0, &s, empty).unwrap(),
        vitals(50, 50)
    );
    assert!(vitals(50, 50).heal(-1, &s, empty).is_err());
    assert_eq!(
        vitals(50, 50).heal(i32::MAX, &s, empty).unwrap(),
        vitals(100, 50)
    );
}

fn regenerate_steps(
    mut state: Vitals,
    elapsed: f64,
    count: usize,
    stats: &Stats,
    conditions: ConditionMask,
) -> Vitals {
    for _ in 0..count {
        state = state.regenerate(elapsed, stats, conditions).unwrap();
    }
    state
}

#[test]
fn combat_helpers_fractional_review_hp_tenths_partition_regression() {
    let s = stats(100, 100, 0, 0);
    let once = vitals(50, 0).regenerate(1.0, &s, mask(&[])).unwrap();
    let split = regenerate_steps(vitals(50, 0), 0.1, 10, &s, mask(&[]));
    eprintln!("HP review repro: once={once:?}, split={split:?}");
    assert_eq!(once.hp, 51);
    assert_eq!(split.hp, once.hp);
}

#[test]
fn combat_helpers_fractional_review_mp_eighths_partition_regression() {
    let s = stats(100, 100, 0, 0);
    let once = vitals(50, 0).regenerate(1.25, &s, mask(&[])).unwrap();
    let split = regenerate_steps(vitals(50, 0), 0.125, 10, &s, mask(&[]));
    eprintln!("MP review repro: once={once:?}, split={split:?}");
    assert_eq!(once.mp, 1);
    assert_eq!(split.mp, once.mp);
}

#[test]
fn combat_helpers_fractional_negative_lethal_and_cap_partition_boundaries() {
    let s = stats(100, 100, 0, 0);
    let bleed = mask(&[Condition::Sick, Condition::Bleeding]);
    for (hp, elapsed, step, count) in [(50, 0.05, 0.005, 10), (1, 0.05, 0.005, 10)] {
        let once = vitals(hp, 0).regenerate(elapsed, &s, bleed).unwrap();
        let split = regenerate_steps(vitals(hp, 0), step, count, &s, bleed);
        assert_eq!(split.hp, once.hp);
        assert_eq!(split.hp, hp - 1);
        split.validate(&s).unwrap();
        if hp == 1 {
            assert_eq!((split.hp_remainder, split.hp_roundoff), (0.0, 0.0));
            assert_eq!(
                split
                    .regenerate(1.0, &s, mask(&[Condition::Healing]))
                    .unwrap()
                    .hp,
                0
            );
        }
    }
    let once = vitals(99, 99).regenerate(1.25, &s, mask(&[])).unwrap();
    let split = regenerate_steps(vitals(99, 99), 0.125, 10, &s, mask(&[]));
    assert_eq!(split, once);
    assert_eq!(split, vitals(100, 100));
    let cap_debt = vitals(100, 50)
        .regenerate(2.0_f64.powi(-100), &s, bleed)
        .unwrap();
    assert_eq!(cap_debt.hp, 100);
    assert!(cap_debt.hp_remainder < 0.0);
    assert_eq!(cap_debt.rebase(&s, &s).unwrap(), cap_debt);
    assert_eq!(cap_debt.heal(1, &s, mask(&[])).unwrap().hp_roundoff, 0.0);
}

#[test]
fn combat_helpers_fractional_many_small_steps_keep_integer_vitals_equal() {
    let s = stats(100, 100, 0, 0);
    for count in [10, 1_000, 100_000] {
        for (start, total, conditions) in [
            (vitals(50, 0), 1.0, mask(&[])),
            (vitals(50, 0), 1.25, mask(&[])),
            (vitals(19, 0), 1.0, mask(&[Condition::Bleeding])),
            (vitals(99, 99), 1.25, mask(&[])),
        ] {
            let once = start.regenerate(total, &s, conditions).unwrap();
            let split = regenerate_steps(start, total / count as f64, count, &s, conditions);
            assert_eq!(
                (split.hp, split.mp),
                (once.hp, once.mp),
                "count={count}, start={start:?}"
            );
            split.validate(&s).unwrap();
        }
    }
}

#[test]
fn combat_helpers_fractional_legitimate_near_boundaries_are_not_snapped() {
    let s = stats(100, 100, 0, 0);
    let below_hp = vitals(50, 0)
        .regenerate(1.0_f64.next_down(), &s, mask(&[]))
        .unwrap();
    assert_eq!(below_hp.hp, 50);
    let below_mp = vitals(50, 0)
        .regenerate(1.25_f64.next_down(), &s, mask(&[]))
        .unwrap();
    assert_eq!(below_mp.mp, 0);
    let below_death = vitals(1, 0)
        .regenerate(
            0.05_f64.next_down(),
            &s,
            mask(&[Condition::Sick, Condition::Bleeding]),
        )
        .unwrap();
    assert_eq!(below_death.hp, 1);
    assert!(below_death.hp_remainder < 0.0);
    let below_cap = vitals(99, 99)
        .regenerate(1.0_f64.next_down(), &s, mask(&[]))
        .unwrap();
    assert_eq!(below_cap.hp, 99);
    let just_below = 1.0_f64.next_down();
    let mut hp = Vitals {
        hp_remainder: just_below,
        ..vitals(50, 0)
    };
    for _ in 0..3 {
        hp = hp.regenerate(2.0_f64.powi(-55), &s, mask(&[])).unwrap();
        assert_eq!(hp.hp, 50);
        hp.validate(&s).unwrap();
    }
    // Three quarter-ULP additions leave a real deficit even when the high word rounds to 1.
    assert!(hp.hp_roundoff > 0.0);
    hp = hp.regenerate(2.0_f64.powi(-55), &s, mask(&[])).unwrap();
    assert_eq!(hp.hp, 51);
    let mut mp = Vitals {
        mp_remainder: just_below,
        ..vitals(50, 0)
    };
    for _ in 0..4 {
        mp = mp.regenerate(2.0_f64.powi(-55), &s, mask(&[])).unwrap();
        assert_eq!(mp.mp, 0);
        mp.validate(&s).unwrap();
    }
    mp = mp.regenerate(2.0_f64.powi(-55), &s, mask(&[])).unwrap();
    assert_eq!(mp.mp, 1);
    let bleed = mask(&[Condition::Sick, Condition::Bleeding]);
    let mut lethal = Vitals {
        hp_remainder: -just_below,
        ..vitals(1, 0)
    };
    for _ in 0..3 {
        lethal = lethal.regenerate(2.0_f64.powi(-59), &s, bleed).unwrap();
        assert_eq!(lethal.hp, 1);
        lethal.validate(&s).unwrap();
    }
    lethal = lethal.regenerate(2.0_f64.powi(-59), &s, bleed).unwrap();
    assert_eq!(
        (lethal.hp, lethal.hp_remainder, lethal.hp_roundoff),
        (0, 0.0, 0.0)
    );
}

#[test]
fn combat_helpers_fractional_sub_ulp_carry_survives_and_resets_only_at_semantic_caps() {
    let s = stats(100, 100, 0, 0);
    let tiny = 2.0_f64.powi(-100);
    let start = Vitals {
        hp_remainder: 0.5,
        mp_remainder: 0.5,
        ..vitals(50, 0)
    };
    let carried = regenerate_steps(start, tiny, 100, &s, mask(&[]));
    assert_eq!(carried.hp_remainder, 0.5);
    assert_eq!(carried.hp_roundoff, 100.0 * tiny);
    assert!(carried.mp_roundoff > 0.0);
    assert_eq!(carried.regenerate(0.0, &s, mask(&[])).unwrap(), carried);
    assert_eq!(
        carried
            .regenerate(1.0, &s, mask(&[Condition::Sick]))
            .unwrap()
            .hp_roundoff,
        carried.hp_roundoff
    );
    let quiet = carried
        .regenerate(0.0, &s, mask(&[Condition::Quiet]))
        .unwrap();
    assert_eq!(
        (quiet.mp, quiet.mp_remainder, quiet.mp_roundoff),
        (0, 0.0, 0.0)
    );
    let healed = carried.heal(100, &s, mask(&[])).unwrap();
    assert_eq!(
        (healed.hp, healed.hp_remainder, healed.hp_roundoff),
        (100, 0.0, 0.0)
    );
    let lower = stats(50, 0, 0, 0);
    assert_eq!(carried.rebase(&s, &lower).unwrap(), vitals(50, 0));
    for invalid in [
        Vitals {
            hp_roundoff: f64::NAN,
            ..start
        },
        Vitals {
            mp_roundoff: f64::INFINITY,
            ..start
        },
        Vitals {
            hp_roundoff: 0.01,
            ..start
        },
        Vitals {
            hp_remainder: 1.0_f64.next_down(),
            hp_roundoff: 2.0_f64.powi(-53),
            ..start
        },
    ] {
        assert!(invalid.regenerate(0.0, &s, mask(&[])).is_err());
    }
}

#[test]
fn weapon_runtime_shot_angles_preserves_source_accumulating_addition_not_index_multiplication() {
    let c = content(false);
    let mut item = c.named("Energy Staff").unwrap().clone();
    item.num_projectiles = 3;
    item.arc_gap_radians = 0.2;
    let center = 1.0e10;
    let gap = f64::from(item.arc_gap_radians);
    let start = center - gap * 2.0 / 2.0;
    let actual = shot_angles(&item, center).unwrap();
    let mut next = start;
    let expected = (0..3)
        .map(|_| {
            let a = next;
            next += gap;
            a
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_ne!(
        actual,
        (0..3)
            .map(|i| start + f64::from(i) * gap)
            .collect::<Vec<_>>()
    );
}

#[test]
fn weapon_runtime_f32_cells_midpoint_ties_zero_subnormal_and_overflow_edges() {
    for value in [
        1.0_f32,
        1.0_f32.next_up(),
        -1.0,
        (-1.0_f32).next_down(),
        f32::from_bits(1),
        f32::MAX,
        -f32::MAX,
    ] {
        let (low, high) = f32_rounding_cell(value).unwrap();
        assert!(low.is_finite() && high.is_finite());
        assert_eq!(low as f32, value);
        assert_eq!(high as f32, value);
        assert_ne!(low.next_down() as f32, value);
        assert_ne!(high.next_up() as f32, value);
    }
    for zero in [0.0_f32, -0.0_f32] {
        let (low, high) = f32_rounding_cell(zero).unwrap();
        assert_eq!(low as f32, 0.0);
        assert_eq!(high as f32, 0.0);
        assert_ne!(low.next_down() as f32, 0.0);
        assert_ne!(high.next_up() as f32, 0.0);
    }
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(f32_rounding_cell(bad).is_err());
    }
}

#[test]
fn weapon_runtime_angle_cells_bound_lost_xml_precision_and_default_pi_over_16_at_all_steps() {
    let c = content(false);
    let mut item = c.named("Energy Staff").unwrap().clone();
    for gap in [
        0.1,
        std::f64::consts::PI / 16.0,
        f64::from(f32::from_bits(1)),
    ] {
        item.arc_gap_radians = gap as f32;
        item.num_projectiles = 255;
        let mut value = -0.3;
        let angles = (0..255)
            .map(|_| {
                let a = value as f32;
                value += gap;
                a
            })
            .collect::<Vec<_>>();
        weapon_burst_angles(&item, &angles).unwrap();
        let mut wrong = angles.clone();
        wrong[254] = (value + 0.001) as f32;
        assert!(weapon_burst_angles(&item, &wrong).is_err());
    }
    item.num_projectiles = 2;
    item.arc_gap_radians = 0.1;
    let first = 1.0_f32;
    let (lo, hi) = f32_rounding_cell(first).unwrap();
    let (gap_lo, gap_hi) = f32_rounding_cell(item.arc_gap_radians).unwrap();
    let min = ((lo + gap_lo).next_down()) as f32;
    let max = ((hi + gap_hi).next_up()) as f32;
    weapon_burst_angles(&item, &[first, min]).unwrap();
    weapon_burst_angles(&item, &[first, max]).unwrap();
    assert!(weapon_burst_angles(&item, &[first, min.next_down()]).is_err());
    assert!(weapon_burst_angles(&item, &[first, max.next_up()]).is_err());
    assert!(weapon_burst_angles(&item, &[first, f32::INFINITY]).is_err());
    assert!(weapon_burst_angles(&item, &[]).is_err());
    item.arc_gap_radians = 0.0;
    weapon_burst_angles(&item, &[-0.0, 0.0]).unwrap();
    assert!(weapon_burst_angles(&item, &[0.0, f32::from_bits(1)]).is_err());
    assert!(weapon_burst_angles(&item, &[0.0, 0.0, 0.0]).is_err());
}
