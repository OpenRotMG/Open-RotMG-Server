use super::*;

async fn reward_world() -> (World, Vec<(i32, i32)>) {
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
    (w, owners)
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
async fn god_gear_death_roll_is_once_per_candidate_and_contributor_group_only() {
    let (mut w, owners) = reward_world().await;
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
                for (account, _) in &owners[..contributors] {
                    assert!(
                        bag.accessible(*account),
                        "shared gear available to each contributor"
                    );
                }
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

#[tokio::test]
async fn septavius_doom_fraction_qualifies_at_fifteen_percent_without_raising_other_gates() {
    let (mut w, owners) = reward_world().await;
    assert_eq!(w.content.objects[&0xd90].hp, 8000);
    let seed = (0..32768)
        .find(|seed| {
            crate::loot::rewards(
                &w.content,
                0xd90,
                &mut rand::rngs::StdRng::seed_from_u64(*seed),
            )
            .contains(&0xc02)
        })
        .expect("bounded seeded search for the unchanged .001 Doom roll");
    for damage in [800, 801, 1199, 1200, 1201] {
        clear_bags(&mut w);
        let root = w
            .spawn_entity(0xd90, Pos { x: 3.5, y: 3.5 }, vec![])
            .unwrap();
        w.entities
            .get_mut(&root)
            .unwrap()
            .damage
            .insert(owners[0].1, damage);
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.defeat_entity(root).unwrap();
        let mut items = Vec::new();
        for bag in w.bag_expiry.keys().map(|id| &w.entities[id]) {
            assert!(bag.accessible(owners[0].0));
            assert!(!bag.accessible(owners[1].0));
            items.extend(bag.items.iter().copied().filter(|item| *item >= 0));
        }
        assert_eq!(items.contains(&0xc02), damage >= 1200, "Doom at {damage}");
        assert_eq!(items.contains(&0xa35), damage > 800, "Wisdom at {damage}");
        let expected: Vec<_> = if damage > 800 {
            crate::loot::rewards(
                &w.content,
                0xd90,
                &mut rand::rngs::StdRng::seed_from_u64(seed),
            )
            .into_iter()
            .filter(|item| *item != 0xc02)
            .map(i32::from)
            .collect()
        } else {
            Vec::new()
        };
        assert_eq!(
            items
                .into_iter()
                .filter(|item| *item != 0xc02)
                .collect::<Vec<_>>(),
            expected
        );
    }
}
