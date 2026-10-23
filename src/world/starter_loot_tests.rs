use super::*;
use rand::{RngCore, seq::SliceRandom};

fn expected_gray(seed: u64, contributors: &[(i32, i32)]) -> (BTreeMap<i32, Vec<i32>>, u64) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut drops = BTreeMap::<i32, Vec<i32>>::new();
    for _ in 0..(contributors.len() * 2).min(8) {
        for (chance, item) in [(0.03, 0xa22), (0.02, 0xa23)] {
            if rng.gen_bool(chance) {
                drops
                    .entry(-1)
                    .or_default()
                    .push(*[item].choose(&mut rng).unwrap());
                break;
            }
        }
    }
    let eligible: Vec<_> = contributors
        .iter()
        .filter(|(_, damage)| *damage > 1)
        .collect();
    let mut successes = 0;
    for _ in 0..eligible.len() {
        if rng.gen_bool(0.005) {
            let item = *[0xa51].choose(&mut rng).unwrap();
            drops.entry(eligible[successes].0).or_default().push(item);
            successes += 1;
            if successes == 8 {
                break;
            }
        }
    }
    (drops, rng.next_u64())
}

fn gray_world(count: i32) -> (World, Vec<i32>, i32) {
    let mut w = tests::arena(6);
    let mut players = Vec::new();
    for account in 1..=count {
        let mut ch = tests::ch(&w);
        ch.id = account;
        ch.account = account;
        players.push(w.enter(ch, None).unwrap());
    }
    let enemy = w
        .spawn_entity(0x20d, Pos { x: 1.5, y: 1.5 }, vec![])
        .unwrap();
    (w, players, enemy)
}

fn actual_drops(w: &World) -> BTreeMap<i32, Vec<i32>> {
    let mut result = BTreeMap::<i32, Vec<i32>>::new();
    for bag in w
        .entities
        .values()
        .filter(|e| w.bag_expiry.contains_key(&e.id))
    {
        let owner = bag
            .extra_stats
            .iter()
            .find(|(stat, _)| *stat == 54)
            .map_or(-1, |(_, account)| *account);
        result
            .entry(owner)
            .or_default()
            .extend(bag.items.iter().copied().filter(|item| *item >= 0));
    }
    result
}

#[test]
fn starter_gray_world_preserves_public_attempts_and_filters_private_contributors() {
    for count in [0, 1, 3, 5] {
        let (mut w, players, enemy) = gray_world(count + 4);
        let mut contributors = Vec::new();
        for (n, player) in players.iter().enumerate() {
            let damage = if n < count as usize {
                count + 1 - n as i32
            } else {
                100
            };
            w.entities
                .get_mut(&enemy)
                .unwrap()
                .damage
                .insert(w.players[player].character.id, damage);
            if n < count as usize {
                contributors.push((w.players[player].character.account, damage));
            }
        }
        w.players
            .get_mut(&players[count as usize])
            .unwrap()
            .character
            .alive = false;
        w.players
            .get_mut(&players[count as usize + 1])
            .unwrap()
            .death_pending = true;
        w.players
            .get_mut(&players[count as usize + 2])
            .unwrap()
            .departing = true;
        w.entities
            .get_mut(&enemy)
            .unwrap()
            .damage
            .remove(&w.players[&players[count as usize + 3]].character.id);
        for seed in 0..32 {
            for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
                w.remove_entity(bag);
            }
            w.rng = rand::rngs::StdRng::seed_from_u64(seed);
            let (expected, next) = expected_gray(seed, &contributors);
            w.drop_loot(&w.entities[&enemy].clone()).unwrap();
            assert_eq!(actual_drops(&w), expected, "count {count}, seed {seed}");
            assert_eq!(
                w.rng.next_u64(),
                next,
                "exact public 2N/cap8 and private N attempts"
            );
        }
    }
}

#[test]
fn starter_gray_successes_belong_to_highest_damage_owner_and_publish_once_after_commit() {
    let contributors = [(1, 3), (2, 2), (3, 1)];
    let seed = (0..100_000)
        .find(|seed| {
            let (drops, _) = expected_gray(*seed, &contributors);
            if !drops.contains_key(&-1)
                || drops.get(&1) != Some(&vec![0xa51])
                || drops.contains_key(&2)
            {
                return false;
            }
            let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
            for _ in 0..6 {
                for chance in [0.03, 0.02] {
                    if rng.gen_bool(chance) {
                        let _ = [0xa22].choose(&mut rng);
                        break;
                    }
                }
            }
            !rng.gen_bool(0.005) && rng.gen_bool(0.005)
        })
        .expect("bounded deterministic seed with public potions and second private roll success");
    let (mut w, players, enemy) = gray_world(3);
    w.rng = rand::rngs::StdRng::seed_from_u64(seed);
    w.entities.get_mut(&enemy).unwrap().damage = BTreeMap::from([(1, 3), (2, 2), (3, 1)]);
    w.drop_loot(&w.entities[&enemy].clone()).unwrap();
    let (expected, next) = expected_gray(seed, &contributors);
    assert_eq!(actual_drops(&w), expected);
    assert_eq!(w.rng.next_u64(), next);
    let private = w
        .entities
        .values()
        .find(|e| e.items.contains(&0xa51))
        .unwrap()
        .id;
    let public = w
        .entities
        .values()
        .find(|e| w.bag_expiry.contains_key(&e.id) && e.id != private)
        .unwrap()
        .id;
    let owner = players[0];
    let spectator = players[2];
    assert!(w.visible(owner).contains(&private));
    assert!(!w.visible(spectator).contains(&private));
    assert!(w.visible(owner).contains(&public) && w.visible(spectator).contains(&public));
    assert!(
        w.swap(
            spectator,
            Slot {
                object: private,
                slot: 0,
                kind: 0xa51
            },
            Slot {
                object: spectator,
                slot: 6,
                kind: -1
            }
        )
        .is_err()
    );
    // Collection records are native-only; this fixture tests publication, not a native session.
    w.players.get_mut(&owner).unwrap().controller = Controller::Human;
    let plan = w
        .swap(
            owner,
            Slot {
                object: private,
                slot: 0,
                kind: 0xa51,
            },
            Slot {
                object: owner,
                slot: 6,
                kind: -1,
            },
        )
        .unwrap();
    assert_eq!(plan.collected, Some(0xa51));
    assert_eq!(w.entities[&private].items[0], 0xa51);
    assert_eq!(w.players[&owner].character.items[6], -1);
    w.finish_inventory(plan, Ok(1)).unwrap();
    assert_eq!(w.entities[&private].items[0], -1);
    assert_eq!(w.players[&owner].character.items[6], 0xa51);
    assert_eq!(w.players[&owner].character.revision, 1);
    assert!(!w.entities[&private].drop_slots.contains(&0));
    assert!(
        w.swap(
            owner,
            Slot {
                object: private,
                slot: 0,
                kind: 0xa51
            },
            Slot {
                object: owner,
                slot: 7,
                kind: -1
            }
        )
        .is_err()
    );
}

fn expected_midland(
    seed: u64,
    kind: u16,
    contributors: &[(i32, i32)],
) -> (BTreeMap<i32, Vec<i32>>, u64) {
    let public: &[(f64, &[i32])] = if kind == 0x952 {
        &[
            (0.05, &[0xa06, 0xa16, 0xa1c, 0xa3b, 0xa9c]),
            (0.05, &[0xa10, 0xa80, 0xad0]),
        ]
    } else {
        &[
            (0.10, &[0xa01, 0xa05, 0xa73, 0xa76, 0xa9a]),
            (0.05, &[0xa6e, 0xa71, 0xa74, 0xa77, 0xa9b]),
            (0.10, &[0xa5d, 0xa79, 0xa7c]),
            (0.05, &[0xa7a, 0xa7d, 0xa7f]),
        ]
    };
    let private: &[(f64, &[i32])] = if kind == 0x952 {
        &[(
            0.03,
            &[0xa29, 0xa2a, 0xa2b, 0xa2c, 0xa2d, 0xa38, 0xa39, 0xa4e],
        )]
    } else {
        &[
            (0.03, &[0xa17, 0xa1d, 0xa3c, 0xa3d, 0xa9d]),
            (0.03, &[0xa11, 0xa5e, 0xad1]),
        ]
    };
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut drops = BTreeMap::<i32, Vec<i32>>::new();
    for _ in 0..(contributors.len() * 2).min(8) {
        for (chance, pool) in public {
            if rng.gen_bool(*chance) {
                let item = *pool.choose(&mut rng).unwrap();
                let items = drops.entry(-1).or_default();
                if !items.contains(&item) {
                    items.push(item);
                }
                break;
            }
        }
    }
    let owners: Vec<_> = contributors
        .iter()
        .filter(|(_, damage)| *damage > 1)
        .map(|(owner, _)| *owner)
        .collect();
    let mut successes = 0;
    for _ in 0..owners.len() * if kind == 0x949 { 2 } else { 1 } {
        for (chance, pool) in private {
            if rng.gen_bool(*chance) {
                let item = *pool.choose(&mut rng).unwrap();
                let items = drops.entry(owners[successes % owners.len()]).or_default();
                if !items.contains(&item) {
                    items.push(item);
                }
                successes += 1;
                break;
            }
        }
        if successes == 8 {
            break;
        }
    }
    (drops, rng.next_u64())
}

#[test]
fn midland_world_dual_rewards_preserve_living_credit_thresholds_and_exact_attempt_counts() {
    for kind in [0x952, 0x949] {
        for count in [0, 1, 3, 5, 10] {
            let (mut w, players, old) = gray_world(count + 4);
            w.remove_entity(old);
            let enemy = w
                .spawn_entity(kind, Pos { x: 1.5, y: 1.5 }, vec![])
                .unwrap();
            let contributors: Vec<_> = (1..=count)
                .map(|account| (account, count + 1 - account))
                .collect();
            for (index, player) in players.iter().enumerate() {
                w.entities.get_mut(&enemy).unwrap().damage.insert(
                    w.players[player].character.id,
                    if index < count as usize {
                        count - index as i32
                    } else {
                        100
                    },
                );
            }
            w.players
                .get_mut(&players[count as usize])
                .unwrap()
                .character
                .alive = false;
            w.players
                .get_mut(&players[count as usize + 1])
                .unwrap()
                .death_pending = true;
            w.players
                .get_mut(&players[count as usize + 2])
                .unwrap()
                .departing = true;
            w.entities
                .get_mut(&enemy)
                .unwrap()
                .damage
                .remove(&w.players[&players[count as usize + 3]].character.id);
            for seed in 0..64 {
                for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
                    w.remove_entity(bag);
                }
                w.rng = rand::rngs::StdRng::seed_from_u64(seed);
                let (expected, next) = expected_midland(seed, kind, &contributors);
                w.drop_loot(&w.entities[&enemy].clone()).unwrap();
                assert_eq!(
                    actual_drops(&w),
                    expected,
                    "kind {kind:x} count {count} seed {seed}"
                );
                assert_eq!(
                    w.rng.next_u64(),
                    next,
                    "public2Ncap8; privateN/2N; strict1vs2; no contributor no draws"
                );
            }
        }
    }
}

#[test]
fn midland_world_simultaneous_public_private_rewards_enforce_owner_access_and_committed_pickup() {
    let contributors = [(1, 3), (2, 2), (3, 1)];
    for kind in [0x952, 0x949] {
        let seed = (0..100_000)
            .find(|seed| {
                let (drops, _) = expected_midland(*seed, kind, &contributors);
                drops.contains_key(&-1) && drops.contains_key(&1)
            })
            .expect("bounded deterministic dual-channel reward fixture");
        let (mut w, players, old) = gray_world(3);
        w.remove_entity(old);
        let enemy = w
            .spawn_entity(kind, Pos { x: 1.5, y: 1.5 }, vec![])
            .unwrap();
        w.entities.get_mut(&enemy).unwrap().damage = BTreeMap::from(contributors);
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.drop_loot(&w.entities[&enemy].clone()).unwrap();
        assert_eq!(
            actual_drops(&w),
            expected_midland(seed, kind, &contributors).0
        );
        let private = w
            .entities
            .values()
            .find(|e| e.extra_stats.contains(&(54, 1)))
            .unwrap()
            .id;
        let public = w
            .entities
            .values()
            .find(|e| {
                w.bag_expiry.contains_key(&e.id)
                    && !e.extra_stats.iter().any(|(stat, _)| *stat == 54)
            })
            .unwrap()
            .id;
        let (owner, spectator) = (players[0], players[2]);
        assert!(w.visible(owner).contains(&private) && !w.visible(spectator).contains(&private));
        assert!(w.visible(owner).contains(&public) && w.visible(spectator).contains(&public));
        let item = w.entities[&private].items[0];
        let from = Slot {
            object: private,
            slot: 0,
            kind: item,
        };
        assert!(
            w.swap(
                spectator,
                from,
                Slot {
                    object: spectator,
                    slot: 6,
                    kind: -1
                }
            )
            .is_err()
        );
        w.players.get_mut(&owner).unwrap().controller = Controller::Human;
        let plan = w
            .swap(
                owner,
                Slot {
                    object: private,
                    slot: 0,
                    kind: item,
                },
                Slot {
                    object: owner,
                    slot: 6,
                    kind: -1,
                },
            )
            .unwrap();
        assert_eq!(plan.collected, Some(item as u16));
        assert_eq!(w.entities[&private].items[0], item);
        assert_eq!(w.players[&owner].character.items[6], -1);
        w.finish_inventory(plan, Ok(1)).unwrap();
        assert_eq!(w.entities[&private].items[0], -1);
        assert_eq!(w.players[&owner].character.items[6], item);
        assert_eq!(w.players[&owner].character.revision, 1);
        assert!(!w.entities[&private].drop_slots.contains(&0));
    }
}
