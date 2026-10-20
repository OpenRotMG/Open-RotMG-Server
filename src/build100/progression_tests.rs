use super::*;
use rand::{RngCore, SeedableRng, rngs::StdRng, rngs::mock::StepRng};

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
fn initial(class: &Object) -> Progress {
    Progress {
        level: 1,
        xp: 0,
        base: PERMANENT_STATS
            .into_iter()
            .map(|id| (id, class.stats[&id]))
            .collect(),
    }
}
fn seeded() -> StdRng {
    StdRng::seed_from_u64(100)
}
fn item(name: &str, probability: f32) -> Loot {
    Loot::Item {
        name: name.into(),
        probability,
    }
}
fn items(bags: &[LootBag]) -> Vec<i16> {
    bags.iter()
        .flat_map(|bag| bag.iter().flatten().copied())
        .collect()
}
fn error_contains<T: std::fmt::Debug>(result: Result<T>, text: &str) {
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains(text), "{error} does not contain {text}");
}

#[test]
fn progression_helpers_exact_goals_and_level_boundaries() {
    let data = content(false);
    let class = data.named("Wizard").unwrap();
    let start = initial(class);
    let thresholds = [
        0, 50, 200, 450, 800, 1250, 1800, 2450, 3200, 4050, 5000, 6050, 7200, 8450, 9800, 11250,
        12800, 14450, 16200, 18050,
    ];
    for (level, total) in (1..=20).zip(thresholds) {
        assert_eq!(xp_goal(level).unwrap(), 50 + 100 * (i32::from(level) - 1));
        assert_eq!(total, (1..level).map(|l| xp_goal(l).unwrap()).sum::<i32>());
        let result = award_xp(&start, class, total, &mut seeded()).unwrap();
        assert_eq!((result.progress.level, result.progress.xp), (level, 0));
        if level > 1 {
            let before = award_xp(&start, class, total - 1, &mut seeded()).unwrap();
            assert_eq!(before.progress.level, level - 1);
            assert_eq!(
                before.progress.xp,
                total - thresholds[usize::from(level) - 2] - 1
            );
        }
    }
    assert_eq!(xp_goal(20).unwrap(), 1950);
    for invalid in [0, 21, u8::MAX] {
        assert!(xp_goal(invalid).is_err());
    }
}

#[test]
fn progression_helpers_zero_large_awards_carry_no_kill_cap_and_level20_stop() {
    let data = content(false);
    let class = data.named("Wizard").unwrap();
    let start = initial(class);
    let mut rng = seeded();
    let mut untouched = rng.clone();
    let zero = award_xp(&start, class, 0, &mut rng).unwrap();
    assert_eq!(zero.progress, start);
    assert_eq!((zero.max_hp_delta, zero.max_mp_delta), (0, 0));
    assert_eq!(rng.next_u64(), untouched.next_u64());
    let result = award_xp(&start, class, 500, &mut seeded()).unwrap();
    assert_eq!((result.progress.level, result.progress.xp), (4, 50));
    let partial = Progress {
        xp: 49,
        ..start.clone()
    };
    let carried = award_xp(&partial, class, 151, &mut seeded()).unwrap();
    assert_eq!((carried.progress.level, carried.progress.xp), (3, 0));
    let final_level = award_xp(&start, class, i32::MAX, &mut seeded()).unwrap();
    assert_eq!(
        (final_level.progress.level, final_level.progress.xp),
        (20, 0)
    );
    let mut rng = seeded();
    let mut untouched = rng.clone();
    let stopped = award_xp(&final_level.progress, class, i32::MAX, &mut rng).unwrap();
    assert_eq!(stopped.progress, final_level.progress);
    assert_eq!((stopped.max_hp_delta, stopped.max_mp_delta), (0, 0));
    assert_eq!(rng.next_u64(), untouched.next_u64());
    assert_eq!(start, initial(class));
    assert_eq!(partial.xp, 49);
}

#[test]
fn progression_helpers_all_seven_original_repaired_xml_classes_caps_and_growth() {
    for repaired in [false, true] {
        let data = content(repaired);
        for name in [
            "Rogue", "Archer", "Wizard", "Priest", "Warrior", "Knight", "Paladin",
        ] {
            let class = data.named(name).unwrap();
            let start = initial(class);
            start.validate(class).unwrap();
            assert_eq!(class.growth.len(), 8);
            let mut oracle = seeded();
            let mut expected = start.clone();
            // One explicit XML-order oracle, with no award/validation helper involved.
            for _ in 1..20 {
                for &(id, min, max, cap) in &class.growth {
                    let roll = oracle.gen_range(min..=max);
                    let value =
                        (i64::from(expected.base[&id]) + i64::from(roll)).min(i64::from(cap));
                    expected.base.insert(id, value as i32);
                }
            }
            expected.level = 20;
            let mut rng = seeded();
            let awarded = award_xp(&start, class, 18050, &mut rng).unwrap();
            assert_eq!(awarded.progress, expected, "{name} repaired={repaired}");
            assert_eq!(awarded.max_hp_delta, expected.base[&0] - start.base[&0]);
            assert_eq!(awarded.max_mp_delta, expected.base[&3] - start.base[&3]);
            assert_eq!(rng.next_u64(), oracle.next_u64());
            for &(id, _, _, cap) in &class.growth {
                let mut capped = start.clone();
                capped.base.insert(id, cap);
                capped.validate(class).unwrap();
                capped.base.insert(id, cap + 1);
                error_contains(
                    capped.validate(class),
                    &format!("stat {id} outside class cap {cap}"),
                );
            }
        }
    }
}

#[test]
fn progression_helpers_growth_independent_inclusive_seeded_and_xml_order() {
    let data = content(false);
    let class = data.named("Wizard").unwrap();
    let start = initial(class);
    let mut seen: BTreeMap<u8, BTreeSet<i32>> = BTreeMap::new();
    for seed in 0..256 {
        let mut rng = StdRng::seed_from_u64(seed);
        let awarded = award_xp(&start, class, 50, &mut rng).unwrap();
        for &(id, min, max, _) in &class.growth {
            let delta = awarded.progress.base[&id] - start.base[&id];
            assert!((min..=max).contains(&delta));
            seen.entry(id).or_default().insert(delta);
        }
    }
    for &(id, min, max, _) in &class.growth {
        assert!(seen[&id].contains(&min), "stat {id} min not sampled");
        assert!(seen[&id].contains(&max), "stat {id} max not sampled");
    }
    let mut reversed = class.clone();
    reversed.growth.reverse();
    let normal = award_xp(&start, class, 500, &mut seeded()).unwrap();
    let reverse = award_xp(&start, &reversed, 500, &mut seeded()).unwrap();
    assert_ne!(normal.progress.base, reverse.progress.base);
    assert_eq!(normal, award_xp(&start, class, 500, &mut seeded()).unwrap());
}

#[test]
fn progression_helpers_at_cap_still_rolls_and_returns_only_actual_max_deltas() {
    let data = content(false);
    let mut class = data.named("Wizard").unwrap().clone();
    let mut start = initial(&class);
    for &(id, _, _, cap) in &class.growth {
        start.base.insert(id, cap);
    }
    let hp_cap = start.base[&0];
    let mp_cap = start.base[&3];
    start.base.insert(0, hp_cap - 1);
    start.base.insert(3, mp_cap - 2);
    let mut rng = seeded();
    let awarded = award_xp(&start, &class, 50, &mut rng).unwrap();
    assert_eq!((awarded.max_hp_delta, awarded.max_mp_delta), (1, 2));
    let mut oracle = seeded();
    for &(_, min, max, _) in &class.growth {
        let _ = oracle.gen_range(min..=max);
    }
    assert_eq!(rng.next_u64(), oracle.next_u64());
    for growth in &mut class.growth {
        growth.1 = 0;
        growth.2 = 0;
    }
    let zero = award_xp(&start, &class, 18050, &mut seeded()).unwrap();
    assert_eq!(zero.progress.base, start.base);
    assert_eq!((zero.max_hp_delta, zero.max_mp_delta), (0, 0));
    // Intermediate base+roll may exceed i32, but the capped result must not wrap.
    for growth in &mut class.growth {
        growth.1 = i32::MAX;
        growth.2 = i32::MAX;
        growth.3 = i32::MAX;
    }
    let huge = award_xp(&start, &class, 50, &mut seeded()).unwrap();
    assert!(huge.progress.base.values().all(|&value| value == i32::MAX));
}

#[test]
fn progression_helpers_invalid_progress_and_class_validate_before_any_draw() {
    let data = content(false);
    let class = data.named("Wizard").unwrap();
    let start = initial(class);
    let mut invalid = vec![
        Progress {
            level: 0,
            ..start.clone()
        },
        Progress {
            level: 21,
            ..start.clone()
        },
        Progress {
            xp: -1,
            ..start.clone()
        },
        Progress {
            xp: 50,
            ..start.clone()
        },
        Progress {
            level: 20,
            xp: 1,
            ..start.clone()
        },
    ];
    let mut missing = start.clone();
    missing.base.remove(&3);
    invalid.push(missing);
    let mut extra = start.clone();
    extra.base.insert(2, 100);
    invalid.push(extra);
    for (id, value) in [(0, 0), (3, -1), (20, i32::MAX)] {
        let mut bad = start.clone();
        bad.base.insert(id, value);
        invalid.push(bad);
    }
    for bad in invalid {
        let mut rng = seeded();
        let mut untouched = rng.clone();
        assert!(award_xp(&bad, class, 50, &mut rng).is_err());
        assert_eq!(rng.next_u64(), untouched.next_u64());
    }
    assert!(award_xp(&start, class, -1, &mut seeded()).is_err());
    let mut classes = vec![];
    let mut bad = class.clone();
    bad.class = "Equipment".into();
    classes.push(bad);
    let mut bad = class.clone();
    bad.growth.pop();
    classes.push(bad);
    let mut bad = class.clone();
    bad.growth.push(bad.growth[0]);
    classes.push(bad);
    let mut bad = class.clone();
    bad.growth[0].0 = 2;
    classes.push(bad);
    let mut bad = class.clone();
    bad.stats.remove(&0);
    classes.push(bad);
    for (min, max, cap) in [(-1, 1, 1000), (5, 4, 1000), (0, 0, -1), (0, 0, 1)] {
        let mut bad = class.clone();
        bad.growth[0] = (0, min, max, cap);
        classes.push(bad);
    }
    for bad in classes {
        let mut rng = seeded();
        let mut untouched = rng.clone();
        assert!(award_xp(&start, &bad, 0, &mut rng).is_err());
        assert_eq!(rng.next_u64(), untouched.next_u64());
    }
}

#[test]
fn progression_helpers_enemy_xp_floors_minimum_zero_promotion_and_wire_errors() {
    let data = content(false);
    assert_eq!(enemy_xp(data.named("Pirate").unwrap()).unwrap(), 1);
    assert_eq!(enemy_xp(data.named("Ent Ancient").unwrap()).unwrap(), 500);
    let mut enemy = data.named("Pirate").unwrap().clone();
    for (hp, multiplier, expected) in [
        (5, 0.0, 0),
        (5, 1.0, 1),
        (19, 1.0, 1),
        (20, 1.0, 2),
        (1000, 5.0, 500),
        (1, f32::MIN_POSITIVE, 1),
        (i32::MAX, 10.0, i32::MAX),
    ] {
        enemy.stats.insert(0, hp);
        enemy.xp_multiplier = multiplier;
        assert_eq!(enemy_xp(&enemy).unwrap(), expected);
    }
    // f32-first arithmetic would round to 2^31 and incorrectly fail this valid maximum.
    for multiplier in [-1.0, f32::NAN, f32::INFINITY, f32::MAX, 11.0] {
        enemy.xp_multiplier = multiplier;
        assert!(enemy_xp(&enemy).is_err());
    }
    enemy.xp_multiplier = 1.0;
    for hp in [0, -1] {
        enemy.stats.insert(0, hp);
        assert!(enemy_xp(&enemy).is_err());
    }
    enemy.stats.remove(&0);
    error_contains(enemy_xp(&enemy), "Pirate: missing XP MaxHitPoints");
}

#[test]
fn progression_helpers_credit_instance_living_paused_attribution_summon_and_strict_range() {
    let player = RewardRecipient {
        instance: 7,
        position: (0.0, 0.0),
        living: true,
        paused: false,
    };
    let death = EnemyDeath {
        instance: 7,
        position: (0.0, 0.0),
        ordinary_enemy: true,
        player_attributed: true,
        summoned: false,
    };
    assert!(reward_eligible(player, death).unwrap());
    for ineligible in [
        RewardRecipient {
            instance: 8,
            ..player
        },
        RewardRecipient {
            living: false,
            ..player
        },
        RewardRecipient {
            paused: true,
            ..player
        },
        RewardRecipient {
            position: (16.0, 0.0),
            ..player
        },
        RewardRecipient {
            position: (16.001, 0.0),
            ..player
        },
        RewardRecipient {
            position: (12.0, 12.0),
            ..player
        },
    ] {
        assert!(!reward_eligible(ineligible, death).unwrap());
    }
    for ineligible in [
        EnemyDeath {
            ordinary_enemy: false,
            ..death
        },
        EnemyDeath {
            player_attributed: false,
            ..death
        },
        EnemyDeath {
            summoned: true,
            ..death
        },
    ] {
        assert!(!reward_eligible(player, ineligible).unwrap());
    }
    let below = f32::from_bits(16.0f32.to_bits() - 1);
    for position in [(below, 0.0), (-below, 0.0), (0.0, below), (0.0, -below)] {
        assert!(reward_eligible(RewardRecipient { position, ..player }, death).unwrap());
    }
    // Each independent eligible recipient gets the full XP, not a shared fraction.
    let data = content(false);
    for _ in 0..3 {
        assert!(reward_eligible(player, death).unwrap());
        assert_eq!(enemy_xp(data.named("Ent Ancient").unwrap()).unwrap(), 500);
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for position in [(value, 0.0), (0.0, value)] {
            assert!(
                reward_eligible(
                    RewardRecipient {
                        position,
                        living: false,
                        ..player
                    },
                    death
                )
                .is_err()
            );
            assert!(
                reward_eligible(
                    player,
                    EnemyDeath {
                        position,
                        player_attributed: false,
                        ..death
                    }
                )
                .is_err()
            );
        }
    }
    assert!(
        !reward_eligible(
            RewardRecipient {
                position: (f32::MAX, f32::MAX),
                ..player
            },
            EnemyDeath {
                position: (-f32::MAX, -f32::MAX),
                ..death
            },
        )
        .unwrap()
    );
}

#[test]
fn progression_helpers_loot_threshold_f32_promotion_and_no_phantom_bags() {
    let data = content(false);
    let mut source = data.named("Pirate").unwrap().clone();
    let potion = data.named("Health Potion").unwrap().id;
    source.loot = vec![item("Health Potion", 0.5)];
    for (bits, expected) in [
        (0, vec![potion]),
        ((1u64 << 63) - 2048, vec![potion]),
        (1u64 << 63, vec![]),
        (u64::MAX, vec![]),
    ] {
        let bags = roll_loot(&source, &data, &mut StepRng::new(bits, 0)).unwrap();
        assert_eq!(items(&bags), expected);
        assert_eq!(bags.is_empty(), expected.is_empty());
    }
    let p = f64::from(0.1f32);
    assert!(p > 0.1f64);
    source.loot = vec![item("Health Potion", 0.1)];
    let midpoint = ((0.1f64 + p) / 2.0 * (1u64 << 53) as f64) as u64;
    assert_eq!(
        items(&roll_loot(&source, &data, &mut StepRng::new(midpoint << 11, 0)).unwrap()),
        vec![potion]
    );
    let exact = (p * (1u64 << 53) as f64) as u64;
    assert!(
        roll_loot(&source, &data, &mut StepRng::new(exact << 11, 0))
            .unwrap()
            .is_empty()
    );
    source.loot = vec![
        Loot::EmptyContainer,
        item("Health Potion", 0.0),
        Loot::EmptyContainer,
    ];
    let mut rng = StepRng::new(0, 17);
    assert!(roll_loot(&source, &data, &mut rng).unwrap().is_empty());
    assert_eq!(rng.next_u64(), 17);
    source.loot = vec![Loot::EmptyContainer];
    let mut rng = StepRng::new(42, 17);
    assert!(roll_loot(&source, &data, &mut rng).unwrap().is_empty());
    assert_eq!(rng.next_u64(), 42);
}

#[test]
fn progression_helpers_loot_duplicates_independent_order_p0_p1_seeded_draw_contract() {
    let data = content(false);
    let mut source = data.named("Pirate").unwrap().clone();
    let potion = data.named("Health Potion").unwrap().id;
    let staff = data.named("Energy Staff").unwrap().id;
    source.loot = vec![
        item("Health Potion", 0.5),
        Loot::EmptyContainer,
        item("Health Potion", 0.5),
        item("Energy Staff", 1.0),
        item("Health Potion", 0.0),
        item("Health Potion", 1.0),
    ];
    // Uniforms 0, .5, 0, .5, 0: duplicates get separate strict-threshold draws.
    let mut stepped = StepRng::new(0, 1u64 << 63);
    let bags = roll_loot(&source, &data, &mut stepped).unwrap();
    assert_eq!(items(&bags), vec![potion, staff, potion]);
    assert_eq!(
        bags[0],
        [
            Some(potion),
            Some(staff),
            Some(potion),
            None,
            None,
            None,
            None,
            None
        ]
    );
    assert_eq!(stepped.next_u64(), 1u64 << 63);
    let mut rng = seeded();
    let mut oracle = seeded();
    let draws: Vec<f64> = (0..5).map(|_| oracle.r#gen()).collect();
    let mut expected = vec![];
    if draws[0] < 0.5 {
        expected.push(potion);
    }
    if draws[1] < 0.5 {
        expected.push(potion);
    }
    expected.extend([staff, potion]);
    assert_eq!(
        items(&roll_loot(&source, &data, &mut rng).unwrap()),
        expected
    );
    assert_eq!(rng.next_u64(), oracle.next_u64());
    source.loot = vec![item("Health Potion", 1.0); 2];
    assert_eq!(
        items(&roll_loot(&source, &data, &mut seeded()).unwrap()),
        vec![potion, potion]
    );
}

#[test]
fn progression_helpers_loot_eight_nine_64_65_capacity_without_truncation() {
    let data = content(false);
    let mut source = data.named("Pirate").unwrap().clone();
    let potion = data.named("Health Potion").unwrap().id;
    for count in [0, 1, 8, 9, 64] {
        source.loot = vec![item("Health Potion", 1.0); count];
        source.loot.extend(vec![Loot::EmptyContainer; 70]);
        let bags = roll_loot(&source, &data, &mut seeded()).unwrap();
        assert_eq!(bags.len(), count.div_ceil(8));
        assert_eq!(items(&bags), vec![potion; count]);
        assert!(bags.iter().all(|bag| bag[0].is_some()));
        assert!(bags.len() <= 8);
    }
    for probability in [0.0, 1.0] {
        source.loot = vec![item("Health Potion", probability); 65];
        let mut rng = seeded();
        let mut untouched = rng.clone();
        error_contains(
            roll_loot(&source, &data, &mut rng),
            "65 declared loot items exceeds 64",
        );
        assert_eq!(rng.next_u64(), untouched.next_u64());
    }
}

#[test]
fn progression_helpers_loot_validates_all_entries_references_before_rng() {
    let data = content(false);
    let mut source = data.named("Pirate").unwrap().clone();
    for invalid in [
        item("not an item", 0.0),
        item("Pirate", 1.0),
        item("Health Potion", -0.1),
        item("Health Potion", 1.1),
        item("Health Potion", f32::NAN),
        item("Health Potion", f32::INFINITY),
    ] {
        source.loot = vec![item("Health Potion", 1.0), invalid];
        let mut rng = seeded();
        let mut untouched = rng.clone();
        error_contains(roll_loot(&source, &data, &mut rng), "Pirate: Loot 1");
        assert_eq!(rng.next_u64(), untouched.next_u64());
    }
}

#[test]
fn progression_helpers_actual_original_repaired_loot_ids_cardinality_and_resolution() {
    for repaired in [false, true] {
        let data = content(repaired);
        let mut largest = 0;
        for source in data.objects.values() {
            let declared: Vec<i16> = source
                .loot
                .iter()
                .filter_map(|entry| match entry {
                    Loot::EmptyContainer => None,
                    Loot::Item { name, .. } => Some(data.named(name).unwrap().id),
                })
                .collect();
            largest = largest.max(declared.len());
            let mut all = source.clone();
            for entry in &mut all.loot {
                if let Loot::Item { probability, .. } = entry {
                    *probability = 1.0;
                }
            }
            let bags = roll_loot(&all, &data, &mut seeded()).unwrap();
            assert_eq!(
                items(&bags),
                declared,
                "{} repaired={repaired}",
                source.name
            );
            for id in items(&bags) {
                assert_eq!(data.get(id).unwrap().class, "Equipment");
            }
            let actual = roll_loot(source, &data, &mut seeded()).unwrap();
            assert!(items(&actual).len() <= declared.len());
        }
        assert_eq!(largest, 39);
    }
}

#[test]
fn progression_helpers_split_awards_preserve_growth_rng_and_max_deltas() {
    let data = content(false);
    let class = data.named("Wizard").unwrap();
    let start = initial(class);
    let mut whole_rng = seeded();
    let whole = award_xp(&start, class, 18051, &mut whole_rng).unwrap();
    let mut split_rng = seeded();
    let mut progress = start.clone();
    let (mut hp_delta, mut mp_delta) = (0, 0);
    for amount in [49, 1, 149, 1, 250, 500, 1000, 10000, 6101] {
        let next = award_xp(&progress, class, amount, &mut split_rng).unwrap();
        hp_delta += next.max_hp_delta;
        mp_delta += next.max_mp_delta;
        progress = next.progress;
    }
    assert_eq!(progress, whole.progress);
    assert_eq!(
        (hp_delta, mp_delta),
        (whole.max_hp_delta, whole.max_mp_delta)
    );
    assert_eq!(split_rng.next_u64(), whole_rng.next_u64());
}

#[test]
fn progression_helpers_permanent_caps_do_not_cap_or_accept_equipment_effective_stats() {
    use crate::rules::combat::{Stats, equipment_stats};
    for repaired in [false, true] {
        let data = content(repaired);
        let class = data.named("Warrior").unwrap();
        let helm = data.named("Old Helm").unwrap();
        let cap = class.growth.iter().find(|g| g.0 == 21).unwrap().3;
        let mut progress = initial(class);
        progress.base.insert(21, cap);
        let award = award_xp(&progress, class, 50, &mut seeded()).unwrap();
        assert_eq!(award.progress.base[&21], cap);
        let equipped = equipment_stats(
            &Stats::from_base(&award.progress.base).unwrap(),
            &[class.slots[1]],
            &[Some(helm)],
        )
        .unwrap();
        let defense = equipped.get(21).unwrap();
        assert_eq!(
            defense,
            cap + helm.bonuses.iter().find(|b| b.0 == 21).unwrap().1
        );
        assert!(defense > cap);
        let mut wrong_base = award.progress;
        wrong_base.base.insert(21, defense);
        error_contains(
            wrong_base.validate(class),
            "permanent stat 21 outside class cap",
        );
    }
}
