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
    w.name = "Realm".into();
    let hp = w.content.objects[&crate::dungeon::trench::HERMIT].hp;
    for seed in 0..128 {
        for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
            w.remove_entity(bag);
        }
        let root = w
            .spawn_entity(
                crate::dungeon::trench::HERMIT,
                Pos { x: 10.5, y: 20.5 },
                vec![],
            )
            .unwrap();
        let defeated = w.entities.get_mut(&root).unwrap();
        defeated.hp = 0;
        defeated.damage.insert(w.players[&player].character.id, hp);
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.defeat_entity(root).unwrap();
        let (kind, pos) = w.portal_requests.last().unwrap();
        assert_eq!((*kind, pos.x, pos.y), (0x730, 10.5, 25.5));
        let actual = rewards(&w);
        assert_eq!(
            actual.keys().copied().collect::<Vec<_>>(),
            vec![1],
            "private Hermit rewards, seed {seed}: {actual:?}"
        );
        for item in [0xa34, 0xa4c] {
            assert_eq!(
                actual[&1].iter().filter(|i| **i == item).count(),
                1,
                "required potion without duplicating successful rolls, seed {seed}: {actual:?}"
            );
        }
        assert!(
            actual[&1]
                .iter()
                .all(|item| [0xa34, 0xa4c, 0xc08].contains(item))
        );
    }
}

#[test]
fn hermit_private_rolls_are_independent() {
    let w = tests::arena(2);
    let mut rng = rand::rngs::mock::StepRng::new(0, 0);
    assert!(crate::loot::private(crate::dungeon::trench::HERMIT));
    assert_eq!(
        crate::loot::rewards(&w.content, crate::dungeon::trench::HERMIT, &mut rng),
        vec![0xa34, 0xa4c, 0xc08]
    );
}

#[test]
fn hermit_group_minima_do_not_give_every_raider_a_pair() {
    let mut w = tests::arena(2);
    let mut participants = Vec::new();
    for id in 1..=3 {
        let mut ch = tests::ch(&w);
        ch.id = id;
        ch.account = id;
        participants.push(w.enter(ch, None).unwrap());
    }
    let root = w
        .spawn_entity(
            crate::dungeon::trench::HERMIT,
            Pos { x: 10.5, y: 20.5 },
            vec![],
        )
        .unwrap();
    let hp = w.content.objects[&crate::dungeon::trench::HERMIT].hp;
    w.entities.get_mut(&root).unwrap().damage = [(1, hp - 2), (2, 1), (3, 1)].into();
    let mut group_only_minimum = false;
    for seed in 0..64 {
        for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
            w.remove_entity(bag);
        }
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.drop_loot(&w.entities[&root].clone()).unwrap();
        let actual = rewards(&w);
        assert!(
            actual.keys().all(|owner| (1..=3).contains(owner)),
            "{actual:?}"
        );
        let items: Vec<_> = actual.values().flatten().copied().collect();
        assert!(
            items.contains(&0xa34) && items.contains(&0xa4c),
            "seed {seed}: {actual:?}"
        );
        group_only_minimum |= items.len() == 2;
    }
    assert!(
        group_only_minimum,
        "minimum is encounter-wide, not per raider"
    );
    w.entities.get_mut(&root).unwrap().damage = [(1, 1), (2, 0), (3, hp - 1)].into();
    w.players.get_mut(&participants[2]).unwrap().departing = true;
    for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(bag);
    }
    w.drop_loot(&w.entities[&root].clone()).unwrap();
    let actual = rewards(&w);
    assert_eq!(
        actual.keys().copied().collect::<Vec<_>>(),
        vec![1],
        "any positive living contributor, not zero-damage spectators/departing players: {actual:?}"
    );
    assert!(actual[&1].contains(&0xa34) && actual[&1].contains(&0xa4c));
    w.players.get_mut(&participants[0]).unwrap().departing = true;
    for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(bag);
    }
    w.drop_loot(&w.entities[&root].clone()).unwrap();
    assert!(
        rewards(&w).is_empty(),
        "no eligible contributor means no rewards"
    );
}
