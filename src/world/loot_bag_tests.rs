use super::*;
use crate::{server, store::InventoryWrite};

fn players(w: &mut World) -> [i32; 3] {
    [1, 2, 3].map(|account| {
        let mut ch = tests::ch(w);
        ch.id = account;
        ch.account = account;
        ch.items = vec![-1; 12];
        let id = w.enter(ch, None).unwrap();
        let (x, y) = w.map.spawns[0];
        w.players.get_mut(&id).unwrap().pos = Pos { x, y };
        id
    })
}
fn slot(object: i32, slot: usize, kind: i32) -> Slot {
    Slot { object, slot, kind }
}
fn contents(w: &World) -> Vec<i32> {
    w.bag_expiry
        .keys()
        .flat_map(|id| w.entities[id].items.iter().copied())
        .filter(|item| *item >= 0)
        .collect()
}

#[test]
fn selected_rarity_and_nine_seventeen_item_chunks_conserve_exact_rewards_and_reset_maximum() {
    for (item, rarity, bag) in [
        (0xa22, 0, 0x500),
        (0xa3c, 1, 0x503),
        (0xa84, 2, 0x507),
        (0xa1f, 3, 0x508),
        (0xa03, 4, 0x509),
    ] {
        let mut w = tests::arena(6);
        assert_eq!(w.content.objects[&item].bag_type, rarity);
        assert!(!w.content.objects[&item].soulbound);
        let id = w
            .loot_bag(Pos { x: 1.5, y: 1.5 }, Some(1), vec![i32::from(item)])
            .unwrap();
        assert_eq!(w.entities[&id].kind, bag);
        assert_eq!(w.entities[&id].size, 80);
        assert_eq!(w.entities[&id].extra_stats, [(54, 1)]);
        assert_eq!(w.bag_expiry[&id], 60_000);
    }
    for items in [
        vec![
            0xa22, 0xa23, 0xa00, 0xa6c, 0xa6d, 0xa01, 0xa6e, 0xa03, 0xa22,
        ],
        [
            vec![0xa22; 7],
            vec![0xa03],
            vec![0xa22; 7],
            vec![0xa84],
            vec![0xa22],
        ]
        .concat(),
    ] {
        let mut w = tests::arena(6);
        w.loot_bags(Pos { x: 1.5, y: 1.5 }, Some(1), &items)
            .unwrap();
        assert_eq!(w.bag_expiry.len(), items.len().div_ceil(8));
        for ((id, _), chunk) in w.bag_expiry.iter().zip(items.chunks(8)) {
            let bag = &w.entities[id];
            let expected = if chunk.contains(&0xa03) {
                0x509
            } else if chunk.contains(&0xa84) {
                0x507
            } else {
                0x500
            };
            assert_eq!(bag.kind, expected);
            assert_eq!(&bag.items[..chunk.len()], chunk);
            assert!(bag.items[chunk.len()..].iter().all(|item| *item == -1));
            assert_eq!(bag.drop_slots, (0..chunk.len()).collect());
        }
        assert_eq!(contents(&w), items);
    }
    let mut w = tests::world();
    assert!(
        w.loot_bag(Pos { x: 1.5, y: 1.5 }, None, vec![0x701])
            .is_err()
    );
    assert!(w.bag_expiry.is_empty());
}

#[test]
fn three_accounts_filter_private_ids_and_public_slot_reservation_commits_one_copy() {
    let mut w = tests::world();
    let [owner, first, second] = players(&mut w);
    let pos = w.players[&owner].pos;
    w.loot_bags(pos, Some(1), &[0xa22]).unwrap();
    let private = *w.bag_expiry.keys().next().unwrap();
    w.loot_bags(pos, None, &[0xa23]).unwrap();
    let public = *w.bag_expiry.keys().last().unwrap();
    for peer in [first, second] {
        assert!(w.visible(peer).contains(&public));
        assert!(!w.visible(peer).contains(&private));
        assert!(
            w.swap(peer, slot(private, 0, 0xa22), slot(peer, 4, -1))
                .is_err()
        );
        assert_eq!(w.players[&peer].character.revision, 0);
        assert_eq!(w.players[&peer].character.items, vec![-1; 12]);
    }
    assert!(w.visible(owner).contains(&private));
    let plan = w
        .swap(first, slot(public, 0, 0xa23), slot(first, 4, -1))
        .unwrap();
    assert_eq!(w.entities[&public].items[0], 0xa23);
    assert!(
        w.swap(second, slot(public, 0, 0xa23), slot(second, 4, -1))
            .is_err()
    );
    w.finish_inventory(plan, Ok(1)).unwrap();
    let loser = w
        .swap(second, slot(public, 0, 0xa23), slot(second, 4, -1))
        .err()
        .unwrap();
    assert!(server::lifecycle::rule_rejection(&loser));
    assert!(loser.to_string().contains("stale inventory offer"));
    assert_eq!(w.players[&first].character.items[4], 0xa23);
    assert_eq!(w.players[&first].character.revision, 1);
    assert_eq!(w.players[&second].character.items, vec![-1; 12]);
    assert_eq!(w.players[&second].character.revision, 0);
    assert_eq!(w.entities[&public].items[0], -1);
    assert!(!w.entities[&public].drop_slots.contains(&0));
    assert!(!w.players[&second].pending);
    assert!(w.players[&second].active());
}

#[test]
fn empty_slots_obey_selected_put_flags_and_owner_without_inventory_mutation_on_denial() {
    for (kind, seed, normal, bound) in [
        (0x500, 0xa22, true, false),
        (0x503, 0xa3c, false, true),
        (0x507, 0xa84, true, true),
    ] {
        for owner in [None, Some(1), Some(2)] {
            for outgoing in [0xa23, 0x701] {
                let mut w = tests::world();
                let [player, _, _] = players(&mut w);
                w.players.get_mut(&player).unwrap().character.items[4] = outgoing;
                let bag = w
                    .loot_bag(w.players[&player].pos, owner, vec![seed])
                    .unwrap();
                assert_eq!(w.entities[&bag].kind, kind);
                let definition = &w.content.objects[&kind];
                assert_eq!(
                    (definition.put_normal, definition.put_soulbound),
                    (normal, bound)
                );
                let before = w.entities[&bag].items.clone();
                let accepts = owner != Some(2)
                    && if outgoing == 0x701 {
                        bound && owner == Some(1)
                    } else {
                        normal
                    };
                let result = w.swap(player, slot(player, 4, outgoing), slot(bag, 1, -1));
                assert_eq!(
                    result.is_ok(),
                    accepts,
                    "{kind:x} owner {owner:?} item {outgoing:x}"
                );
                if let Ok(plan) = result {
                    assert!(plan.dropped.is_none());
                    w.finish_inventory(plan, Ok(1)).unwrap();
                    assert_eq!(w.entities[&bag].items[1], outgoing);
                    assert_eq!(w.players[&player].character.items[4], -1);
                    assert_eq!(w.players[&player].character.revision, 1);
                } else {
                    assert_eq!(w.entities[&bag].items, before);
                    assert!(!w.entities[&bag].reserved);
                    assert_eq!(w.players[&player].character.items[4], outgoing);
                    assert_eq!(w.players[&player].character.revision, 0);
                    assert!(!w.players[&player].pending);
                }
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn occupied_replacement_publishes_only_after_durable_commit_and_fences_store_and_publication_failures()
 {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    for (incoming, outgoing, private, replacement) in
        [(0xa23, 0x701, false, 0x503), (0xa3c, 0xa23, true, 0x500)]
    {
        for fault in ["none", "store", "publication"] {
            let account = app
                .store
                .list(format!("bag-{incoming}-{fault}"))
                .await
                .unwrap();
            let mut ch = app
                .store
                .create(account.id, app.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            ch.items = vec![-1; 12];
            ch.items[4] = outgoing;
            ch.revision = app
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account: ch.account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap()[0];
            let before = ch.clone();
            let _owner = app.claim(account.id).unwrap();
            let (packets, mut received) = mpsc::channel(256);
            tokio::spawn(async move { while received.recv().await.is_some() {} });
            let (lost, _) = watch::channel(false);
            let (id, bag) = app
                .world
                .in_world(-2)
                .call(move |w| {
                    let id = w.enter(ch, Some(Output { packets, lost }))?;
                    w.loot_bags(
                        w.players[&id].pos,
                        private.then_some(account.id),
                        &[incoming],
                    )?;
                    let bag = *w.bag_expiry.keys().last().unwrap();
                    Ok((id, bag))
                })
                .await
                .unwrap();
            if fault == "store" {
                app.store.lifecycle_test_job(|db| {
                    db.execute_batch("CREATE TRIGGER reject_bag_exchange BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'bag exchange failure'); END;")?;
                    Ok(())
                }).await.unwrap();
            }
            let gate = (fault != "store").then(|| app.store.lifecycle_test_pause_reply());
            let (send, receive) = oneshot::channel();
            let a = app.clone();
            let task = tokio::spawn(async move {
                server::inventory(&a, id, move |w| {
                    let plan = w.swap(id, slot(bag, 0, incoming), slot(id, 4, outgoing))?;
                    send.send(plan.clone()).ok().unwrap();
                    Ok(Some(plan))
                })
                .await
            });
            let plan = receive.await.unwrap();
            let dropped = plan.dropped.unwrap();
            if let Some((ready, release)) = gate {
                tokio::time::timeout(std::time::Duration::from_secs(5), ready)
                    .await
                    .unwrap()
                    .unwrap();
                app.world
                    .for_player(id)
                    .call(move |w| {
                        assert_eq!(w.entities[&dropped].kind, replacement);
                        assert_eq!(w.entities[&dropped].extra_stats, [(54, account.id)]);
                        assert!(!w.entities[&dropped].published);
                        assert!(w.entities[&dropped].reserved);
                        assert!(!w.entities[&dropped].accessible(account.id));
                        assert!(!w.entities[&dropped].accessible(account.id + 1));
                        assert_eq!(w.entities[&bag].items[0], incoming);
                        assert!(w.entities[&bag].reserved);
                        assert_eq!(w.players[&id].character.items[4], outgoing);
                        if fault == "publication" {
                            w.entities.get_mut(&dropped).unwrap().reserved = false;
                        }
                        Ok(())
                    })
                    .await
                    .unwrap();
                release.send(()).unwrap();
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            if fault == "none" {
                result.unwrap().unwrap();
            } else {
                assert!(!server::lifecycle::rule_rejection(&result.unwrap_err()));
            }
            let saved = crate::store::Store::open(&dir.join("game.db"), &app.content)
                .unwrap()
                .load(before.account, before.id)
                .await
                .unwrap();
            assert_eq!(
                saved.revision,
                before.revision + i32::from(fault != "store")
            );
            assert_eq!(
                saved.items[4],
                if fault == "store" { outgoing } else { incoming }
            );
            let collected = app
                .store
                .lifecycle_test_job(move |db| {
                    Ok(db.query_row(
                        "SELECT COUNT(*) FROM collections WHERE account=?1 AND item=?2",
                        [before.account, incoming],
                        |r| r.get::<_, i32>(0),
                    )?)
                })
                .await
                .unwrap();
            assert_eq!(collected, i32::from(fault != "store"));
            app.world
                .for_player(id)
                .call(move |w| {
                    let p = &w.players[&id];
                    if fault == "none" {
                        assert_eq!(p.character.revision, saved.revision);
                        assert_eq!(p.character.items, saved.items);
                        assert_eq!(w.entities[&bag].items[0], -1);
                        assert!(!w.entities[&bag].reserved);
                        assert!(!w.entities[&bag].drop_slots.contains(&0));
                        assert!(w.entities[&dropped].published);
                        assert!(!w.entities[&dropped].reserved);
                        assert!(w.entities[&dropped].accessible(account.id));
                        assert!(!w.entities[&dropped].accessible(account.id + 1));
                        assert_eq!(w.entities[&dropped].items[0], outgoing);
                    } else {
                        assert!(p.pending && p.persistence_failure.is_some());
                        assert!(!p.active());
                        assert_eq!(p.character.revision, before.revision);
                        assert_eq!(p.character.items, before.items);
                        assert_eq!(w.entities[&bag].items[0], incoming);
                        assert!(w.entities[&bag].reserved);
                        assert!(!w.entities[&dropped].published);
                        assert!(!w.entities[&dropped].accessible(account.id));
                        assert!(!w.entities[&dropped].accessible(account.id + 1));
                        let retry = w
                            .swap(id, slot(bag, 0, incoming), slot(id, 4, outgoing))
                            .err()
                            .unwrap();
                        assert!(!server::lifecycle::rule_rejection(&retry));
                    }
                    Ok(())
                })
                .await
                .unwrap();
            if fault == "store" {
                app.store
                    .lifecycle_test_job(|db| {
                        db.execute_batch("DROP TRIGGER reject_bag_exchange;")?;
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
        }
    }
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}
