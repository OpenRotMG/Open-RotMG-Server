use super::*;

async fn reward_world() -> (World, crate::store::Store, Vec<(i32, i32)>) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let mut w = World::new(content, map).unwrap();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    w.encounter = true;
    let map = Arc::make_mut(&mut w.map);
    map.spawns = vec![(1.5, 1.5)];
    for cell in &mut map.cells {
        cell.ground = 0x61;
        cell.object = None;
    }
    for i in 0..6 {
        let account = store.list(format!("reward-accuracy-{i}")).await.unwrap();
        let character = store
            .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        w.enter(character, None).unwrap();
    }
    let owners = w
        .players
        .values()
        .map(|p| (p.character.account, p.character.id))
        .collect();
    (w, store, owners)
}

#[tokio::test]
async fn ent_tincture_death_preserves_rolls_and_has_one_eligible_owner() {
    let (mut w, _store, owners) = reward_world().await;
    assert!(w.content.objects[&0xb0c].soulbound);
    assert!(
        w.loot_bag(Pos { x: 3.5, y: 3.5 }, None, vec![0xb0c])
            .is_err()
    );
    let mut successes = 0;
    let mut misses = 0;
    for kind in [0x921, 0x922] {
        for contributors in [0usize, 1, 3, 5] {
            for seed in 0..128 {
                clear_bags(&mut w);
                let mut expected_rng = rand::rngs::StdRng::seed_from_u64(seed);
                let mut expected = false;
                for _ in 0..(contributors * 2).min(8) {
                    let items = crate::loot::rewards(&w.content, kind, &mut expected_rng);
                    assert!(items.iter().all(|item| *item == 0xb0c));
                    expected |= !items.is_empty();
                }
                let id = w
                    .spawn_entity(kind, Pos { x: 3.5, y: 3.5 }, vec![])
                    .unwrap();
                w.entities.get_mut(&id).unwrap().damage = owners[..contributors]
                    .iter()
                    .map(|(_, character)| (*character, 1))
                    .collect();
                w.entities
                    .get_mut(&id)
                    .unwrap()
                    .damage
                    .insert(owners[5].1, 0);
                w.rng = rand::rngs::StdRng::seed_from_u64(seed);
                w.defeat_entity(id).unwrap();
                assert!(!w.entities.contains_key(&id));
                assert_eq!(
                    w.bag_expiry.len(),
                    usize::from(expected),
                    "seed={seed}, contributors={contributors}"
                );
                if expected {
                    successes += 1;
                    let bag = &w.entities[w.bag_expiry.keys().next().unwrap()];
                    assert_eq!(
                        bag.items
                            .iter()
                            .copied()
                            .filter(|item| *item >= 0)
                            .collect::<Vec<_>>(),
                        [0xb0c]
                    );
                    assert_eq!(
                        owners[..contributors]
                            .iter()
                            .filter(|(owner, _)| bag.accessible(*owner))
                            .count(),
                        1
                    );
                    assert!(
                        owners[contributors..]
                            .iter()
                            .all(|(owner, _)| !bag.accessible(*owner))
                    );
                    assert_eq!(
                        bag.extra_stats
                            .iter()
                            .filter(|(stat, _)| *stat == 54)
                            .count(),
                        1
                    );
                } else {
                    misses += 1;
                }
            }
        }
    }
    assert!(successes > 0 && misses > 0);
}

#[tokio::test]
async fn private_loot_reentry_deadline_and_committed_pickup() {
    let (mut w, store, owners) = reward_world().await;
    let (account, character) = owners[0];
    let player = w
        .players
        .iter()
        .find(|(_, p)| p.character.id == character)
        .unwrap()
        .0
        .to_owned();
    let outsider = *w
        .players
        .iter()
        .find(|(_, p)| p.character.id == owners[1].1)
        .unwrap()
        .0;
    let pos = w.players[&player].pos;
    let bag = w.loot_bag(pos, Some(account), vec![0xb0c, 0xb0c]).unwrap();
    let deadline = w.bag_expiry[&bag];
    let source = Slot {
        object: bag,
        slot: 0,
        kind: 0xb0c,
    };
    let destination = |id| Slot {
        object: id,
        slot: 11,
        kind: -1,
    };
    assert_eq!(w.players[&outsider].character.items[11], -1);
    assert!(w.swap(outsider, source, destination(outsider)).is_err());
    assert_eq!(w.entities[&bag].items[0], 0xb0c);
    assert!(!w.entities[&bag].reserved);
    w.leave(player).unwrap();
    let loaded = store.load(account, character).await.unwrap();
    let reentered = w.enter(loaded, None).unwrap();
    assert_ne!(reentered, player);
    assert_eq!(w.bag_expiry[&bag], deadline);
    assert!(w.entities[&bag].accessible(account));
    assert!(!w.entities[&bag].accessible(owners[1].0));
    w.now = deadline - 50;
    assert_eq!(w.players[&reentered].character.items[11], -1);
    let plan = w.swap(reentered, source, destination(reentered)).unwrap();
    w.step();
    assert_eq!(w.now, deadline);
    assert!(w.entities[&bag].reserved);
    assert_eq!(w.players[&reentered].character.items[11], -1);
    let revisions = store
        .exchange_collected(
            vec![crate::store::InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision,
                items: plan.items.clone(),
            }],
            plan.vaults.clone(),
            plan.collected,
        )
        .await
        .unwrap();
    w.finish_inventory(plan, Ok(revisions[0])).unwrap();
    assert!(
        w.swap(
            reentered,
            source,
            Slot {
                object: reentered,
                slot: 10,
                kind: -1
            }
        )
        .is_err()
    );
    w.step();
    assert!(!w.entities.contains_key(&bag));
    assert!(!w.bag_expiry.contains_key(&bag));
    assert_eq!(
        store.load(account, character).await.unwrap().items[11],
        0xb0c
    );
    let saved = w.leave(reentered).unwrap();
    let mut restarted = World::new(w.content.clone(), w.map.clone()).unwrap();
    let id = restarted.enter(saved, None).unwrap();
    assert_eq!(restarted.players[&id].character.items[11], 0xb0c);
    assert!(restarted.bag_expiry.is_empty());
}

#[tokio::test]
async fn surface_follower_death_rolls_public_equipment_once_not_per_contributor() {
    let (mut w, _store, owners) = reward_world().await;
    let mut successes = 0;
    for kind in (0x669..=0x66b)
        .chain(0x605..=0x607)
        .chain(0x612..=0x613)
        .chain(0x60e..=0x610)
        .chain(0x65f..=0x661)
        .chain(0x91d..=0x91e)
        .chain([0x6c6])
    {
        for seed in 0..64 {
            let expected = crate::loot::rewards(
                &w.content,
                kind,
                &mut rand::rngs::StdRng::seed_from_u64(seed),
            );
            successes += expected.len();
            for contributors in [1, 3] {
                clear_bags(&mut w);
                let id = w
                    .spawn_entity(kind, Pos { x: 3.5, y: 3.5 }, vec![])
                    .unwrap();
                w.entities.get_mut(&id).unwrap().damage = owners[..contributors]
                    .iter()
                    .map(|(_, character)| (*character, 1))
                    .collect();
                w.rng = rand::rngs::StdRng::seed_from_u64(seed);
                w.defeat_entity(id).unwrap();
                let mut actual = Vec::new();
                for bag in w.bag_expiry.keys().map(|id| &w.entities[id]) {
                    let follower = matches!(kind, 0x91d | 0x91e);
                    assert_eq!(
                        bag.bag_owners.len(),
                        if follower && bag.kind == 0x500 {
                            contributors
                        } else {
                            0
                        }
                    );
                    for item in bag.items.iter().copied().filter(|item| *item >= 0) {
                        let bound = w.content.objects[&(item as u16)].soulbound;
                        let owned = bound || (follower && bag.kind != 0x500);
                        assert_eq!(!bag.extra_stats.is_empty(), owned);
                        if owned {
                            assert!(
                                owners[..contributors]
                                    .iter()
                                    .any(|(owner, _)| bag.extra_stats.contains(&(54, *owner)))
                            );
                            assert!(!bag.accessible(owners[5].0));
                        } else if follower {
                            assert!(
                                owners[..contributors]
                                    .iter()
                                    .all(|(owner, _)| bag.accessible(*owner))
                            );
                            assert!(!bag.accessible(owners[5].0));
                        }
                        actual.push(item);
                    }
                }
                actual.sort_unstable();
                let mut expected: Vec<_> = expected.iter().map(|id| i32::from(*id)).collect();
                expected.sort_unstable();
                assert_eq!(
                    actual, expected,
                    "kind={kind:#x}, seed={seed}, contributors={contributors}"
                );
                assert!(!w.entities.contains_key(&id));
            }
        }
    }
    assert!(successes > 0);
}

#[tokio::test]
async fn lair_slime_splits_leave_potion_rolls_to_terminal_forms() {
    let (mut world, _store, owners) = reward_world().await;
    for (parent, child, count) in [(0xd9b, 0xd9c, 4), (0xd9c, 0xd9d, 4), (0xd9e, 0xd9f, 6)] {
        for seed in 0..64 {
            for id in world.entities.keys().copied().collect::<Vec<_>>() {
                world.remove_entity(id);
            }
            let id = world
                .spawn_entity(parent, Pos { x: 3.5, y: 3.5 }, vec![])
                .unwrap();
            world
                .entities
                .get_mut(&id)
                .unwrap()
                .damage
                .insert(owners[0].1, 1);
            world.rng = rand::rngs::StdRng::seed_from_u64(seed);
            world.defeat_entity(id).unwrap();
            assert!(!world.entities.contains_key(&id));
            assert_eq!(world.actors().filter(|e| e.kind == child).count(), count);
            assert!(
                world.bag_expiry.is_empty(),
                "intermediate {parent:#x}, seed {seed} rolled loot"
            );
        }
    }
    for kind in [0xd9d, 0xd9f] {
        let outcomes: BTreeSet<_> = (0..256)
            .map(|seed| {
                crate::loot::rewards(
                    &world.content,
                    kind,
                    &mut rand::rngs::StdRng::seed_from_u64(seed),
                )
            })
            .collect();
        assert_eq!(outcomes, BTreeSet::from([vec![], vec![0xa22], vec![0xa23]]));
    }
}

#[test]
fn septavius_stages_a_realm_exit_until_clear_commit() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = crate::dungeon::undead::generate(&content, 12351).unwrap();
    let mut world = World::new(content, Arc::new(map)).unwrap();
    world.name = "Undead Lair".into();
    world.encounter = true;
    let boss = world.actors().find(|e| e.kind == 0xd90).unwrap().id;
    world.defeat_entity(boss).unwrap();
    assert!(world.cleared);
    assert!(!world.entities.contains_key(&boss));
    let exit = &world.entities[&world.clear_exit.unwrap()];
    assert_eq!(exit.kind, 0x704);
    assert!(!exit.published);
    assert_eq!(world.actors().filter(|e| e.kind == 0x704).count(), 1);
    assert!(!world.actors().any(|e| e.kind == 0x71d));
}

#[tokio::test]
async fn flying_brain_death_requests_only_the_realm_lab_portal() {
    let (mut w, _store, _owners) = reward_world().await;
    assert_eq!(crate::dungeon::portal_drop(0x657), Some((0x890, 0.17)));
    let mut successes = 0;
    for name in ["Realm", "Tutorial"] {
        w.name = name.into();
        for kind in [0x657, 0x658] {
            for seed in 0..32 {
                clear_bags(&mut w);
                w.portal_requests.clear();
                let pos = Pos { x: 3.5, y: 3.5 };
                let id = w.spawn_entity(kind, pos, vec![]).unwrap();
                w.rng = rand::rngs::StdRng::seed_from_u64(seed);
                let expected = name == "Realm"
                    && kind == 0x657
                    && rand::rngs::StdRng::seed_from_u64(seed).gen_bool(0.17);
                w.defeat_entity(id).unwrap();
                assert!(!w.entities.contains_key(&id));
                assert_eq!(w.portal_requests.len(), usize::from(expected));
                if expected {
                    successes += 1;
                    assert_eq!(w.portal_requests[0].0, 0x890);
                    assert!(w.portal_requests[0].1.distance(pos) < 0.01);
                }
            }
        }
    }
    assert!((1..32).contains(&successes));
}

fn clear_bags(w: &mut World) {
    for id in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
}

fn white_demon_gear(content: &Content, seed: u64) -> Vec<i32> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut items = Vec::new();
    for (chance, tier, slots) in [
        (0.04, 6, crate::loot::WEAPONS),
        (0.02, 7, crate::loot::WEAPONS),
        (0.01, 8, crate::loot::WEAPONS),
        (0.08, 6, crate::loot::ARMOR),
        (0.04, 7, crate::loot::ARMOR),
        (0.02, 8, crate::loot::ARMOR),
        (0.01, 9, crate::loot::ARMOR),
        (0.015, 3, crate::loot::RINGS),
        (0.005, 4, crate::loot::RINGS),
        (0.02, 4, crate::loot::ABILITIES),
    ] {
        let candidates: Vec<_> = content
            .objects
            .values()
            .filter(|item| item.tier == Some(tier) && slots.contains(&item.slot_type))
            .collect();
        for item in &candidates {
            if rng.gen_bool(chance / candidates.len() as f64) {
                items.push(i32::from(item.kind));
            }
        }
    }
    items.sort_unstable();
    items
}

#[tokio::test]
async fn god_gear_death_roll_is_once_per_candidate_with_private_colored_bags() {
    let (mut w, _store, owners) = reward_world().await;
    let mut seeds = vec![0, 1, 2, 3];
    let multiple = (0..32768)
        .find(|seed| {
            white_demon_gear(&w.content, *seed)
                .iter()
                .filter(|kind| {
                    let item = &w.content.objects[&(**kind as u16)];
                    item.tier == Some(6) && crate::loot::WEAPONS.contains(&item.slot_type)
                })
                .count()
                > 1
        })
        .expect("terminating seeded search must exercise multiple successes in one tier");
    seeds.push(multiple);
    for contributors in [1, 2, 5] {
        for seed in &seeds {
            clear_bags(&mut w);
            let root = w
                .spawn_entity(0x651, Pos { x: 3.5, y: 3.5 }, vec![])
                .unwrap();
            w.entities.get_mut(&root).unwrap().damage = owners[..contributors]
                .iter()
                .map(|(_, character)| (*character, 1))
                .collect();
            w.rng = rand::rngs::StdRng::seed_from_u64(*seed);
            w.defeat_entity(root).unwrap();
            assert!(!w.entities.contains_key(&root));
            let mut actual = Vec::new();
            for bag in w.bag_expiry.keys().map(|id| &w.entities[id]) {
                assert_eq!(bag.items.len(), 8);
                assert_eq!(
                    owners[..contributors]
                        .iter()
                        .filter(|(account, _)| bag.accessible(*account))
                        .count(),
                    if bag.kind == 0x500 { contributors } else { 1 }
                );
                assert!(!bag.accessible(owners[5].0));
                actual.extend(bag.items.iter().copied().filter(|item| *item >= 0));
            }
            actual.sort_unstable();
            assert_eq!(
                actual,
                white_demon_gear(&w.content, *seed),
                "contributors={contributors}, seed={seed}"
            );
            assert_eq!(actual.iter().collect::<BTreeSet<_>>().len(), actual.len());
        }
    }
}
