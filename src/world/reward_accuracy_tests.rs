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
async fn surface_follower_death_rolls_public_equipment_once_not_per_contributor() {
    let (mut w, _store, owners) = reward_world().await;
    let mut successes = 0;
    for kind in (0x669..=0x66b)
        .chain(0x605..=0x607)
        .chain(0x612..=0x613)
        .chain(0x60e..=0x610)
        .chain(0x65f..=0x661)
        .chain(0x91d..=0x91e)
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
