use super::*;

fn rewards(w: &World) -> BTreeMap<i32, Vec<i32>> {
    let mut rewards = BTreeMap::<i32, Vec<i32>>::new();
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
        rewards
            .entry(owner)
            .or_default()
            .extend(bag.items.iter().copied().filter(|item| *item >= 0));
    }
    rewards
}

#[test]
fn hermit_solo_death_preserves_private_required_potions_for_every_roll() {
    let mut w = tests::arena(2);
    let player = w.enter(tests::ch(&w), None).unwrap();
    let root = w
        .spawn_entity(
            crate::dungeon::trench::HERMIT,
            Pos { x: 10.5, y: 20.5 },
            vec![],
        )
        .unwrap();
    let hp = w.content.objects[&crate::dungeon::trench::HERMIT].hp;
    w.entities
        .get_mut(&root)
        .unwrap()
        .damage
        .insert(w.players[&player].character.id, hp);
    for seed in 0..128 {
        for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
            w.remove_entity(bag);
        }
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.drop_loot(&w.entities[&root].clone()).unwrap();
        let actual = rewards(&w);
        assert_eq!(
            actual.keys().copied().collect::<Vec<_>>(),
            vec![1],
            "private Hermit rewards, seed {seed}: {actual:?}"
        );
        assert!(
            actual[&1].contains(&0xa34) && actual[&1].contains(&0xa4c),
            "required Vit/Dex, seed {seed}: {actual:?}"
        );
        assert!(
            actual[&1]
                .iter()
                .all(|item| [0xa34, 0xa4c, 0xc08].contains(item))
        );
    }
}
