use super::*;
use rand::SeedableRng;

fn texts(rx: &mut tokio::sync::mpsc::Receiver<Packet>) -> Vec<String> {
    let mut result = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        if packet.id != wire::TEXT {
            continue;
        }
        let mut r = crate::wire::Reader::new(&packet.body);
        assert_eq!(r.utf().unwrap(), "#Oryx the Mad God");
        assert_eq!(r.i32().unwrap(), -1);
        r.i32().unwrap();
        r.u8().unwrap();
        r.utf().unwrap();
        result.push(r.utf().unwrap());
    }
    result
}

#[test]
fn realm_welcome_finishing_player_and_last_root_are_once_only_and_local() {
    fn listener(
        d: &mut Directory,
        world: i32,
        account: i32,
        name: &str,
    ) -> (i32, tokio::sync::mpsc::Receiver<Packet>) {
        let w = d.worlds.get_mut(&world).unwrap();
        let mut ch = tests::ch(w);
        ch.account = account;
        ch.id = account;
        ch.name = name.into();
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        let (lost, _) = tokio::sync::watch::channel(false);
        (w.enter(ch, Some(Output { packets: tx, lost })).unwrap(), rx)
    }
    let mut d = tests::directory();
    let other = *d.endgame.realms.keys().find(|id| **id != -3).unwrap();
    let (top, mut a) = listener(&mut d, -3, 1, "TopDamage");
    assert_eq!(texts(&mut a), vec!["You are food for my minions!"]);
    let (finish, mut peer) = listener(&mut d, -3, 2, "Finisher");
    assert_eq!(texts(&mut peer), vec!["You are food for my minions!"]);
    assert!(texts(&mut a).is_empty());
    let (_, mut b) = listener(&mut d, other, 3, "OtherRealm");
    texts(&mut b);
    let (_, mut nexus) = listener(&mut d, -2, 4, "Nexus");
    assert!(texts(&mut nexus).is_empty());
    let w = d.worlds.get_mut(&-3).unwrap();
    w.event_reserves.clear();
    let pos = w.players[&top].pos;
    let first = w.spawn_entity(0x668, pos, vec![]).unwrap();
    let last = w.spawn_entity(0x668, pos, vec![]).unwrap();
    d.endgame.realms.get_mut(&-3).unwrap().cycle.remaining = [first, last].into();
    let w = d.worlds.get_mut(&-3).unwrap();
    let hp = w.entities[&first].hp;
    w.damage_enemy(top, first, hp - 1, true, &[], None).unwrap();
    w.damage_enemy(finish, first, 1, true, &[], None).unwrap();
    d.endgame_step();
    let output = texts(&mut a);
    assert_eq!(
        output,
        vec![
            "Finisher, you will pay for defeating my Red Demon!",
            "A Red Demon still guards me!"
        ]
    );
    assert_eq!(texts(&mut peer), output);
    assert!(texts(&mut b).is_empty());
    assert!(texts(&mut nexus).is_empty());
    d.worlds.get_mut(&-3).unwrap().critical_deaths.push(first);
    d.endgame_step();
    assert!(texts(&mut a).is_empty());
    let w = d.worlds.get_mut(&-3).unwrap();
    w.finish_timed_defeat(last).unwrap();
    d.endgame_step();
    let output = texts(&mut a);
    assert_eq!(
        output,
        vec!["I HAVE CLOSED THIS REALM! YOU WILL NOT LIVE TO SEE THE LIGHT OF DAY!"]
    );
    assert!(d.endgame.realms[&other].cycle.admits());
    assert!(texts(&mut b).is_empty());
    assert!(texts(&mut nexus).is_empty());
}

#[test]
fn realm_event_policy_has_ten_reserved_roots_and_uniform_quarter_chance_without_reward_rng() {
    let mut d = tests::directory();
    let w = d.worlds.get_mut(&-3).unwrap();
    let mut counts = BTreeMap::new();
    for event in &w.event_reserves {
        *counts.entry(event.root.kind).or_insert(0) += 1;
        assert!(!w.entities.contains_key(&event.root.id));
        if event.root.kind == 0xd50 {
            assert!(
                w.walkable(event.root.pos),
                "Lost Lands clearing must be accessible"
            );
        }
    }
    assert_eq!(
        counts,
        [
            (0xd50, 1),
            (0xd54, 1),
            (0xd56, 2),
            (0xd59, 2),
            (0xd5f, 2),
            (0xd61, 1),
            (0xe37, 1)
        ]
        .into()
    );
    assert_eq!(d.endgame.realms[&-3].cycle.remaining.len(), 11);
    let pos = Pos {
        x: w.map.spawns[0].0,
        y: w.map.spawns[0].1,
    };
    while w.summon_occupancy() < 256 {
        let id = w.spawn_entity(0xd57, pos, vec![]).unwrap();
        w.entities.get_mut(&id).unwrap().quest = Some(id);
    }
    let reward_next = w.rng.clone().r#gen::<u64>();
    let mut selected = BTreeMap::<u16, usize>::new();
    for seed in 0..4096 {
        w.event_rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.event_waiting = None;
        w.event_choice = None;
        match w.earn_realm_event(1).unwrap() {
            realm_events::EventAdmission::Miss => {}
            realm_events::EventAdmission::Waiting => {
                *selected.entry(w.event_choice.unwrap()).or_default() += 1;
                let next = w.event_rng.clone().r#gen::<u64>();
                let choice = w.event_choice;
                for _ in 0..3 {
                    assert!(matches!(
                        w.earn_realm_event(1).unwrap(),
                        realm_events::EventAdmission::Waiting
                    ));
                }
                assert_eq!(w.event_rng.clone().r#gen::<u64>(), next);
                assert_eq!(w.event_choice, choice);
            }
            realm_events::EventAdmission::Spawned(_) => panic!("full capacity admitted an event"),
        }
    }
    let successes: usize = selected.values().sum();
    assert!(
        (900..1150).contains(&successes),
        "quarter chance: {successes}"
    );
    assert_eq!(selected.len(), 7);
    for count in selected.values() {
        assert!(
            (100..200).contains(count),
            "type selection is not reserve-weighted: {selected:?}"
        );
    }
    assert_eq!(w.rng.clone().r#gen::<u64>(), reward_next);
}

#[test]
fn lost_lands_minions_roll_both_potions_independently() {
    let w = tests::arena(0);
    for kind in [0xd52, 0xd53] {
        let mut rng = rand::rngs::mock::StepRng::new(0, 0);
        assert_eq!(
            crate::loot::rewards(&w.content, kind, &mut rng),
            vec![0xa22, 0xa23]
        );
    }
}

#[tokio::test]
async fn lost_lands_and_ship_real_deaths_have_private_one_percent_rewards_once() {
    for kind in [0xd50, 0xe37] {
        let mut w = tests::arena(0);
        w.name = "Realm".into();
        let path =
            std::env::temp_dir().join(format!("realm-reward-{kind:x}-{}", rand::random::<u64>()));
        std::fs::create_dir(&path).unwrap();
        let store = crate::store::Store::open(&path.join("game.db"), &w.content).unwrap();
        let mut players = Vec::new();
        for account in 1..=3 {
            let owner = store.list(format!("realm-reward-{account}")).await.unwrap();
            let ch = store
                .create(owner.id, w.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            players.push(w.enter(ch, None).unwrap());
        }
        let pos = w.players[&players[0]].pos;
        let root = w.spawn_entity(kind, pos, vec![]).unwrap();
        w.landmark_bind(root).unwrap();
        if kind == 0xe37 {
            let anchor = w.spawn_entity(0xe38, pos, vec![]).unwrap();
            w.landmark_bind(anchor).unwrap();
        }
        let hp = w.entities[&root].hp;
        let seed = (0..1000)
            .find(|seed| {
                let mut rng = rand::rngs::StdRng::seed_from_u64(*seed);
                if kind == 0xe37 {
                    rng.gen_bool(0.5);
                    rng.gen_bool(0.2);
                }
                !crate::loot::rewards(&w.content, kind, &mut rng).is_empty()
            })
            .unwrap();
        w.damage_enemy(players[0], root, hp / 100 + 1, true, &[], None)
            .unwrap();
        w.damage_enemy(players[1], root, hp / 100, true, &[], None)
            .unwrap();
        w.damage_enemy(players[2], root, hp - hp / 100 * 2 - 2, true, &[], None)
            .unwrap();
        w.leave(players[2]).unwrap();
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.damage_enemy(players[0], root, 1, true, &[], None)
            .unwrap();
        assert!(!w.entities.contains_key(&root));
        let bags: Vec<_> = w
            .bag_expiry
            .keys()
            .map(|id| w.entities[id].clone())
            .collect();
        let private: Vec<_> = bags
            .iter()
            .filter(|b| b.extra_stats.iter().any(|(stat, _)| *stat == 54))
            .collect();
        assert!(!private.is_empty());
        assert!(private.iter().all(|b| b.extra_stats.contains(&(54, 1))));
        assert!(
            bags.iter()
                .filter(|b| !b.extra_stats.iter().any(|(stat, _)| *stat == 54))
                .all(|b| kind == 0xe37 && b.items.iter().all(|item| matches!(*item, -1 | 0xc25)))
        );
        assert_eq!(w.critical_deaths, vec![root]);
        assert_eq!(
            w.damage_enemy(players[0], root, hp, true, &[], None)
                .unwrap(),
            0
        );
        assert_eq!(w.bag_expiry.len(), bags.len());
        let bag = private[0];
        let slot = bag.items.iter().position(|item| *item >= 0).unwrap();
        let item = bag.items[slot];
        w.players.get_mut(&players[1]).unwrap().pos = bag.pos;
        let from = Slot {
            object: bag.id,
            slot,
            kind: item,
        };
        let destination = (4..12)
            .find(|slot| w.players[&players[0]].character.items[*slot] == -1)
            .unwrap();
        let to = |object| Slot {
            object,
            slot: destination,
            kind: -1,
        };
        assert!(w.swap(players[1], from.clone(), to(players[1])).is_err());
        w.players.get_mut(&players[0]).unwrap().pos = bag.pos;
        let plan = w.swap(players[0], from, to(players[0])).unwrap();
        let account = plan.account;
        let character = plan.character;
        let revision = plan.revision;
        assert_eq!(
            store.load(account, character).await.unwrap().items[destination],
            -1
        );
        let revisions = store
            .exchange_collected(
                vec![crate::store::InventoryWrite {
                    id: character,
                    account,
                    revision,
                    items: plan.items.clone(),
                }],
                plan.vaults.clone(),
                plan.collected,
            )
            .await
            .unwrap();
        w.finish_inventory(plan, Ok(revisions[0])).unwrap();
        w.leave(players[0]).unwrap();
        let loaded = store.load(account, character).await.unwrap();
        assert!(loaded.alive && loaded.hp > 0);
        assert_eq!(
            (loaded.items[destination], loaded.revision),
            (item, revision + 1)
        );
        let reentered = w.enter(loaded, None).unwrap();
        assert_eq!(w.players[&reentered].character.items[destination], item);
        eprintln!(
            "durable private {kind:#x} reward evidence: {}",
            path.display()
        );
    }
}
