use super::{tests, *};

#[test]
fn ground_potions_reserve_the_source_and_leave_full_backpacks_unchanged() {
    for kind in [0xa22, 0xa23, 0xa1f, 0xa3f, 0xc12, 0xae9, 0xaea, 0xb16] {
        let mut w = tests::world();
        let mut ch = tests::ch(&w);
        ch.items[4..].fill(0xa22);
        ch.hp = 20;
        ch.mp = 0;
        ch.base[2] = 10;
        let id = w.enter(ch, None).unwrap();
        let before = w.players[&id].character.clone();
        let next = match &w.content.get(kind as u16).unwrap().successor {
            Some(name) => i32::from(
                w.content
                    .objects
                    .values()
                    .find(|item| &item.name == name)
                    .unwrap()
                    .kind,
            ),
            None => -1,
        };
        let owner = w
            .content
            .get(kind as u16)
            .unwrap()
            .soulbound
            .then_some(before.account);
        let bag = w.loot_bag(w.players[&id].pos, owner, vec![kind]).unwrap();
        let slot = Slot {
            object: bag,
            slot: 0,
            kind,
        };
        let plan = w.use_item(id, slot, w.players[&id].pos).unwrap().unwrap();
        assert!(w.entities[&bag].reserved && w.players[&id].pending);
        assert_eq!(plan.items, before.items);
        assert_eq!(plan.bags.first().unwrap().1[0], next);
        assert_eq!(w.entities[&bag].items[0], kind);
        assert!(w.use_item(id, slot, w.players[&id].pos).is_err());
        let mut other = tests::ch(&w);
        other.account = 2;
        let other = w.enter(other, None).unwrap();
        assert!(w.use_item(other, slot, w.players[&id].pos).is_err());
        w.finish_inventory(plan, Ok(before.revision + 1)).unwrap();
        assert_eq!(w.players[&id].character.items, before.items);
        assert_eq!(w.entities[&bag].items[0], next);
        assert!(!w.entities[&bag].reserved);
        match kind {
            0xa22 | 0xa3f | 0xb16 => assert!(w.players[&id].character.hp > before.hp),
            0xa23 => assert!(w.players[&id].character.mp > before.mp),
            0xa1f => assert_eq!(w.players[&id].character.base[2], before.base[2] + 1),
            0xc12 => assert!(
                w.players[&id].character.hp > before.hp && w.players[&id].character.mp > before.mp
            ),
            0xae9 => assert_eq!(w.players[&id].character.base[0], before.base[0] + 5),
            0xaea => assert_eq!(w.players[&id].character.base[1], before.base[1] + 5),
            _ => unreachable!(),
        }
    }
}

#[test]
fn failed_ground_consumption_keeps_source_reserved_and_quarantines_owner() {
    for committed in [false, true] {
        let mut w = tests::world();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let bag = w.loot_bag(w.players[&id].pos, None, vec![0xa22]).unwrap();
        let plan = w
            .use_item(
                id,
                Slot {
                    object: bag,
                    slot: 0,
                    kind: 0xa22,
                },
                w.players[&id].pos,
            )
            .unwrap()
            .unwrap();
        let result = if committed {
            w.players.get_mut(&id).unwrap().character.base[0] = -1;
            Ok(1)
        } else {
            Err(anyhow::anyhow!("ground potion storage failure"))
        };
        assert!(w.finish_inventory(plan, result).is_err());
        assert_eq!(w.entities[&bag].items[0], 0xa22);
        assert!(w.entities[&bag].reserved);
        assert!(w.players[&id].pending && w.players[&id].persistence_failure.is_some());
    }
}

#[test]
fn inventory_only_shop_activations_reject_ground_sources_without_mutation() {
    for kind in [0x1175, 0xaa9, 0x1513, 0x705] {
        let mut w = tests::world();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let before = w.players[&id].character.items.clone();
        let owner = w
            .content
            .get(kind as u16)
            .unwrap()
            .soulbound
            .then_some(w.players[&id].character.account);
        let bag = w.loot_bag(w.players[&id].pos, owner, vec![kind]).unwrap();
        let source = w.entities[&bag].items.clone();
        assert!(
            w.use_item(
                id,
                Slot {
                    object: bag,
                    slot: 0,
                    kind
                },
                w.players[&id].pos
            )
            .is_err()
        );
        assert_eq!(w.players[&id].character.items, before);
        assert_eq!(w.entities[&bag].items, source);
        assert!(!w.entities[&bag].reserved && !w.players[&id].pending);
    }
}

#[test]
fn ground_use_rejects_inaccessible_or_stale_sources_without_reserving_items() {
    for case in 0..8 {
        let mut w = tests::world();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let bag = w.loot_bag(w.players[&id].pos, None, vec![0xa22]).unwrap();
        let mut slot = Slot {
            object: bag,
            slot: 0,
            kind: 0xa22,
        };
        match case {
            0 => w.entities.get_mut(&bag).unwrap().pos.x += 3.,
            1 => w
                .entities
                .get_mut(&bag)
                .unwrap()
                .extra_stats
                .push((54, 999)),
            2 => w.entities.get_mut(&bag).unwrap().reserved = true,
            3 => w.entities.get_mut(&bag).unwrap().published = false,
            4 => slot.slot = 8,
            5 => slot.kind = 0xa23,
            6 => {
                slot.slot = 1;
                slot.kind = -1;
            }
            7 => w.entities.get_mut(&bag).unwrap().kind = 0x704,
            _ => unreachable!(),
        }
        let before = w.players[&id].character.items.clone();
        assert!(
            w.use_item(id, slot, w.players[&id].pos).is_err(),
            "case {case}"
        );
        assert_eq!(w.players[&id].character.items, before);
        assert_eq!(w.entities[&bag].items[0], 0xa22);
        assert!(!w.players[&id].pending);
        assert_eq!(w.entities[&bag].reserved, case == 2);
    }
}

#[test]
fn occupied_ground_swaps_drop_incompatible_replacements_without_exposing_soulbound_items() {
    for (incoming, outgoing, private) in [(0xaeb, 0xa97, true), (0xa22, 0xaeb, false)] {
        for reverse in [false, true] {
            let mut w = tests::world();
            let mut ch = tests::ch(&w);
            ch.items[4] = outgoing;
            let id = w.enter(ch, None).unwrap();
            let account = w.players[&id].character.account;
            let bag = w
                .loot_bag(
                    w.players[&id].pos,
                    private.then_some(account),
                    vec![incoming],
                )
                .unwrap();
            if private {
                w.entities.get_mut(&bag).unwrap().kind = 0x503;
            }
            let source = Slot {
                object: bag,
                slot: 0,
                kind: incoming,
            };
            let target = Slot {
                object: id,
                slot: 4,
                kind: outgoing,
            };
            let plan = if reverse {
                w.swap(id, target, source)
            } else {
                w.swap(id, source, target)
            }
            .unwrap();
            let dropped = plan.dropped.unwrap();
            assert_eq!(plan.items[4], incoming);
            assert_eq!(plan.bags.first().unwrap().1[0], -1);
            assert_eq!(w.entities[&dropped].items[0], outgoing);
            assert!(!w.entities[&dropped].published && w.entities[&dropped].reserved);
            assert!(w.entities[&bag].reserved);
            let soulbound = w.content.get(outgoing as u16).unwrap().soulbound;
            assert_eq!(
                w.entities[&dropped].kind,
                if soulbound { 0x503 } else { 0x500 }
            );
            assert_eq!(
                w.entities[&dropped].extra_stats.contains(&(54, account)),
                soulbound || private
            );
            w.finish_inventory(plan, Ok(1)).unwrap();
            assert_eq!(w.players[&id].character.items[4], incoming);
            assert_eq!(w.entities[&bag].items[0], -1);
            assert!(!w.entities[&bag].reserved);
            assert!(w.entities[&dropped].published && !w.entities[&dropped].reserved);
        }
    }
}

#[tokio::test]
async fn desert_werewolf_combat_drops_collect_once_and_survive_inventory_cas_reopen() {
    ordinary_combat_drops_collect_once_and_survive_inventory_cas_reopen(0x623, 5, 320, 52).await;
}

#[tokio::test]
async fn swarm_combat_drops_collect_once_and_survive_inventory_cas_reopen() {
    ordinary_combat_drops_collect_once_and_survive_inventory_cas_reopen(0x955, 6, 360, 65).await;
}

#[tokio::test]
async fn horned_drake_combat_drops_collect_once_and_survive_inventory_cas_reopen() {
    ordinary_combat_drops_collect_once_and_survive_inventory_cas_reopen(0x945, 7, 1270, 229).await;
}

async fn ordinary_combat_drops_collect_once_and_survive_inventory_cas_reopen(
    kind: u16,
    terrain: u8,
    hp: i32,
    xp: i32,
) {
    use crate::store::{InventoryWrite, Store};
    use rand::{SeedableRng, rngs::StdRng};
    let mut drops = 0;
    let mut nondrops = 0;
    for seed in 0..16 {
        let mut w = tests::arena(terrain);
        Arc::make_mut(&mut w.map).spawns = vec![(12.5, 12.5)];
        let path = std::env::temp_dir().join(format!(
            "ordinary-loot-{kind:x}-{}-{}-{seed}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        let db = path.join("game.db");
        let store = Store::open(&db, &w.content).unwrap();
        let account = store.list("ordinary-combat-fixture".into()).await.unwrap();
        let mut ch = store
            .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.gain_experience(
            &w.content,
            crate::store::level_start(20).unwrap(),
            &mut StdRng::seed_from_u64(12351),
        )
        .unwrap();
        ch.items = vec![0xb08, 0xa2e, 0xa96, -1, -1, -1, -1, -1, -1, -1, -1, -1];
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                Vec::new(),
            )
            .await
            .unwrap()[0];
        ch.hp = ch.effective(&w.content).unwrap()[0];
        store.progress(ch.clone()).await.unwrap();
        let player = w.enter(ch.clone(), None).unwrap();
        let origin = w.players[&player].pos;
        let enemy = w
            .spawn_entity(
                kind,
                Pos {
                    x: origin.x + 2.,
                    y: origin.y,
                },
                vec![],
            )
            .unwrap();
        assert_eq!(w.entities[&enemy].hp, hp);
        w.rng = StdRng::seed_from_u64(seed);
        for _ in 0..200 {
            let Some(e) = w.entities.get(&enemy) else {
                break;
            };
            let p = w.players[&player].pos;
            let angle = (e.pos.y - p.y).atan2(e.pos.x - p.x);
            w.action(player, Action::Shoot { angle }).unwrap();
            w.step();
        }
        w.action(player, Action::Stop).unwrap();
        assert!(
            !w.entities.contains_key(&enemy),
            "real projectile defeat seed {seed}"
        );
        let p = &w.players[&player];
        assert!(p.character.shots > 0 && p.character.hits > 0);
        assert_eq!((p.character.kills, p.character.quests), (1, 1));
        let earned = p.experience_queued;
        assert_eq!(earned, xp);
        assert!(w.portal_requests.is_empty() && w.critical_deaths.is_empty());
        assert_eq!(
            w.damage_enemy(player, enemy, 1000, false, &[], Some(0))
                .unwrap(),
            0
        );
        assert_eq!(w.players[&player].experience_queued, earned);
        for (id, snapshot) in w.pending_progress() {
            store.progress(snapshot.clone()).await.unwrap();
            w.finish_progress(id, snapshot).unwrap();
        }
        let bags: Vec<_> = w.bag_expiry.keys().copied().collect();
        assert!(bags.len() <= 1);
        if let Some(&bag) = bags.first() {
            drops += 1;
            assert!(!crate::loot::private(kind));
            assert_eq!(w.bag_expiry[&bag], w.now + 60_000);
            assert_eq!(w.entities[&bag].kind, 0x500);
            assert!(
                !w.entities[&bag]
                    .extra_stats
                    .iter()
                    .any(|(stat, _)| *stat == 54)
            );
            let item = w.entities[&bag].items[0];
            let object = w.content.get(item as u16).unwrap();
            if kind == 0x945 {
                assert!(
                    matches!(item, 0xa22 | 0xa23)
                        || (object.tier == Some(5)
                            && (crate::loot::WEAPONS.contains(&object.slot_type)
                                || crate::loot::ARMOR.contains(&object.slot_type)))
                        || (object.tier == Some(2)
                            && crate::loot::ABILITIES.contains(&object.slot_type))
                );
            } else {
                assert!(matches!(
                    (object.tier.unwrap(), object.slot_type),
                    (3 | 4, 1 | 2 | 3 | 8 | 17 | 6 | 7 | 14)
                        | (
                            1,
                            4 | 5 | 11 | 12 | 13 | 15 | 16 | 18 | 19 | 20 | 21 | 22 | 23 | 9
                        )
                ));
                assert!(
                    object.tier != Some(4) || kind == 0x623,
                    "only the Werewolf includes tier four"
                );
            }
            assert!(!object.soulbound && object.bag_type == 0);
            let from = Slot {
                object: bag,
                slot: 0,
                kind: item,
            };
            let to = Slot {
                object: player,
                slot: 4,
                kind: -1,
            };
            let destination = w.entities[&bag].pos;
            for _ in 0..200 {
                if w.players[&player].pos.distance(destination) < 1. {
                    break;
                }
                w.action(
                    player,
                    Action::Move {
                        x: destination.x,
                        y: destination.y,
                    },
                )
                .unwrap();
                w.step();
            }
            w.action(player, Action::Stop).unwrap();
            assert!(w.players[&player].pos.distance(destination) < 1.);
            // Collection accounting belongs to native sessions, not scripted combat actors.
            w.players.get_mut(&player).unwrap().controller = Controller::Human;
            assert!(
                w.swap(
                    player,
                    Slot {
                        kind: item + 1,
                        ..from
                    },
                    to
                )
                .is_err()
            );
            let before = store.load(account.id, ch.id).await.unwrap();
            let plan = w.swap(player, from, to).unwrap();
            assert_eq!(plan.collected, Some(item as u16));
            assert!(w.players[&player].pending && w.entities[&bag].reserved);
            assert!(w.swap(player, from, to).is_err());
            let mut write = InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision + 1,
                items: plan.items.clone(),
            };
            assert!(
                store
                    .exchange_collected(vec![write.clone()], Vec::new(), plan.collected)
                    .await
                    .is_err()
            );
            assert_eq!(
                store.load(account.id, ch.id).await.unwrap().items,
                before.items
            );
            assert_eq!(w.entities[&bag].items[0], item);
            write.revision = plan.revision;
            let revision = store
                .exchange_collected(vec![write], Vec::new(), plan.collected)
                .await
                .unwrap()[0];
            w.finish_inventory(plan, Ok(revision)).unwrap();
            assert_eq!(w.players[&player].character.items[4], item);
            assert_eq!(w.entities[&bag].items[0], -1);
            assert!(!w.entities[&bag].reserved && !w.players[&player].pending);
            assert!(w.swap(player, from, to).is_err());
            drop(store);
            let reopened = Store::open(&db, &w.content).unwrap();
            let saved = reopened.load(account.id, ch.id).await.unwrap();
            assert_eq!(saved.items[4], item);
            assert_eq!(saved.revision, revision);
            assert_eq!(
                (saved.exp, saved.fame, saved.kills, saved.quests),
                (before.exp, before.fame, before.kills, before.quests)
            );
            drop(reopened);
        } else {
            nondrops += 1;
            assert!(
                w.players[&player].character.items[4..]
                    .iter()
                    .all(|k| *k == -1)
            );
            drop(store);
        }
        std::fs::remove_dir_all(path).unwrap();
    }
    assert!(
        drops > 0,
        "defeated ordinary actor {kind:#x} never produced an item drop"
    );
    assert!(nondrops > 0, "loot must not have a guaranteed minimum");
}
