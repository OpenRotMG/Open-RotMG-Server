use super::*;
use crate::{server, store::InventoryWrite};
use std::path::PathBuf;

async fn fixture(kind: u16) -> (PathBuf, server::App, server::Owner, i32, i32) {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    let account = app
        .store
        .list("ground-dye-integration".into())
        .await
        .unwrap();
    let mut ch = app
        .store
        .create(account.id, app.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.items = vec![
        0xa97, 0xa2e, 0xa5c, 0xa4f, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22,
    ];
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
    let owner = app.claim(account.id).unwrap();
    let (packets, mut received) = mpsc::channel(256);
    tokio::spawn(async move { while received.recv().await.is_some() {} });
    let (lost, _) = watch::channel(false);
    let (id, bag) = app
        .world
        .in_world(-2)
        .call(move |w| {
            let id = w.enter(ch, Some(Output { packets, lost }))?;
            let p = &w.players[&id];
            assert_eq!(p.controller, Controller::Human);
            assert!(!p.native_update_acked);
            assert!(p.last_move <= 0);
            let bag = w.loot_bag(p.pos, Some(account.id), vec![i32::from(kind)])?;
            w.updates(id, false)?;
            Ok((id, bag))
        })
        .await
        .unwrap();
    (dir, app, owner, id, bag)
}

#[tokio::test]
async fn ground_dye_server_commits_both_channels_and_cloths_with_full_inventory() {
    for kind in [0x1000, 0x1100, 0x1200, 0x1300] {
        let (dir, app, _owner, id, bag) = fixture(kind).await;
        let before = app
            .world
            .for_player(id)
            .call(move |w| Ok(w.players[&id].character.clone()))
            .await
            .unwrap();
        server::inventory(&app, id, move |w| {
            w.native_use_item(
                id,
                1,
                Slot {
                    object: bag,
                    slot: 0,
                    kind: i32::from(kind),
                },
                w.players[&id].pos,
            )
        })
        .await
        .unwrap()
        .unwrap();
        let expected = app.content.get(kind).unwrap().textures;
        let after = app
            .world
            .for_player(id)
            .call(move |w| {
                assert_eq!(w.entities[&bag].items[0], -1);
                assert!(!w.entities[&bag].reserved);
                assert!(!w.players[&id].pending);
                Ok(w.players[&id].character.clone())
            })
            .await
            .unwrap();
        assert_eq!(after.items, before.items);
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(
            [after.tex1, after.tex2],
            [
                expected[0].unwrap_or(before.tex1),
                expected[1].unwrap_or(before.tex2)
            ]
        );
        let loaded = crate::store::Store::open(&dir.join("game.db"), &app.content)
            .unwrap()
            .load(after.account, after.id)
            .await
            .unwrap();
        assert_eq!(loaded.items, after.items);
        assert_eq!([loaded.tex1, loaded.tex2], [after.tex1, after.tex2]);
        assert_eq!(loaded.revision, after.revision);
        app.world
            .for_player(id)
            .call(move |w| {
                let appearance = packet_stats(&w.stats(id, id)?);
                let source = packet_stats(&w.stats(bag, id)?);
                assert_eq!(
                    [appearance[&32] as u32, appearance[&33] as u32],
                    [after.tex1, after.tex2]
                );
                assert_eq!(source[&8], -1);
                w.step();
                assert!(!w.entities.contains_key(&bag));
                Ok(())
            })
            .await
            .unwrap();
        app.shutdown.send_replace(true);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn ground_dye_preserves_a_nonzero_other_channel_across_successive_ground_uses() {
    let (dir, app, _owner, id, bag) = fixture(0x1000).await;
    use_ground(&app, id, bag, 0x1000).await.unwrap();
    let cloth = app
        .world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            let bag = w.loot_bag(p.pos, Some(p.character.account), vec![0x1300])?;
            w.updates(id, false)?;
            Ok(bag)
        })
        .await
        .unwrap();
    use_ground(&app, id, cloth, 0x1300).await.unwrap();
    let saved = app.store.load(1, 1).await.unwrap();
    assert_eq!([saved.tex1, saved.tex2], [0x01f0f8ff, 0x04000000]);
    let live = state(&app, id, cloth).await;
    assert_eq!(live["character"]["tex1"], saved.tex1);
    assert_eq!(live["character"]["tex2"], saved.tex2);
    assert_eq!(live["bag"][0], -1);
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}

fn packet_stats(bytes: &[u8]) -> BTreeMap<u8, i32> {
    let mut reader = crate::wire::Reader::new(bytes);
    reader.i32().unwrap();
    reader.f32().unwrap();
    reader.f32().unwrap();
    let count = reader.i16().unwrap();
    let mut stats = BTreeMap::new();
    for _ in 0..count {
        let kind = reader.u8().unwrap();
        if matches!(kind, 31 | 62) {
            reader.utf().unwrap();
        } else {
            stats.insert(kind, reader.i32().unwrap());
        }
    }
    reader.done().unwrap();
    stats
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ground_dye_pending_reservation_blocks_duplicate_use_and_pickup_and_stale_completion() {
    let (dir, app, _owner, id, bag) = fixture(0x1000).await;
    let before = state(&app, id, bag).await;
    let (ready, release) = app.store.lifecycle_test_pause_reply();
    let (send, receive) = oneshot::channel();
    let a = app.clone();
    let task = tokio::spawn(async move {
        server::inventory(&a, id, move |w| {
            let plan = w.native_use_item(
                id,
                1,
                Slot {
                    object: bag,
                    slot: 0,
                    kind: 0x1000,
                },
                w.players[&id].pos,
            )?;
            send.send(plan.clone().unwrap()).ok().unwrap();
            Ok(plan)
        })
        .await
        .unwrap()
        .unwrap();
    });
    let plan = receive.await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    app.world
        .for_player(id)
        .call(move |w| {
            let slot = Slot {
                object: bag,
                slot: 0,
                kind: 0x1000,
            };
            assert!(w.native_use_item(id, 1, slot, w.players[&id].pos).is_err());
            assert!(
                w.swap(
                    id,
                    slot,
                    Slot {
                        object: id,
                        slot: 4,
                        kind: 0xa22
                    }
                )
                .is_err()
            );
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(state(&app, id, bag).await["character"], before["character"]);
    let a = app.clone();
    let duplicate = tokio::spawn(async move { use_ground(&a, id, bag, 0x1000).await });
    tokio::task::yield_now().await;
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), duplicate)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let saved = app.store.load(1, 1).await.unwrap();
    assert_eq!(saved.tex1, 0x01f0f8ff);
    assert_eq!(saved.revision, plan.revision + 1);
    let committed = state(&app, id, bag).await;
    assert_eq!(committed["bag"][0], -1);
    assert_eq!(committed["reserved"], false);
    app.world
        .directory(move |d| {
            assert!(d.finish_inventory(plan, Ok(saved.revision)).is_err());
            Ok(())
        })
        .await
        .unwrap();
    let stale = state(&app, id, bag).await;
    assert_eq!(stale["character"], committed["character"]);
    assert_eq!(stale["bag"][0], -1);
    assert_eq!(stale["reserved"], true);
    assert_eq!(stale["quarantined"], true);
    assert_eq!(app.store.load(1, 1).await.unwrap().revision, saved.revision);
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ground_dye_lost_publication_reply_never_refunds_committed_source_or_appearance() {
    let (dir, app, _owner, id, bag) = fixture(0x1300).await;
    let (ready, release) = app.store.lifecycle_test_pause_reply();
    let a = app.clone();
    let task = tokio::spawn(async move { use_ground(&a, id, bag, 0x1300).await });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    app.world.lifecycle_test_lose_reply();
    release.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let after = state(&app, id, bag).await;
    assert_eq!(after["bag"][0], -1);
    assert_eq!(after["character"]["tex2"], 0x04000000);
    assert!(app.claim(1).is_err());
    assert!(use_ground(&app, id, bag, 0x1300).await.is_err());
    let saved = app.store.load(1, 1).await.unwrap();
    assert_eq!(saved.tex2, 0x04000000);
    assert_eq!(saved.tex1, 0);
    assert!(
        app.world
            .for_player(id)
            .call(move |w| Ok(w.entities[&bag].items[0] == -1))
            .await
            .unwrap()
    );
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn ground_dye_reconnect_reloads_committed_appearance_without_returning_the_item() {
    let (dir, app, owner, id, bag) = fixture(0x1200).await;
    use_ground(&app, id, bag, 0x1200).await.unwrap();
    let saved = app.store.load(1, 1).await.unwrap();
    owner
        .close(&app, CloseReason::Transport)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    let loaded = app.store.load(1, 1).await.unwrap();
    assert_eq!(loaded.items, saved.items);
    assert_eq!([loaded.tex1, loaded.tex2], [saved.tex1, saved.tex2]);
    let owner = app.claim(1).unwrap();
    let bind = server::lifecycle::binding(&app, 1).unwrap();
    let again = app
        .world
        .in_world(-2)
        .call(move |w| {
            let id = w.enter(loaded, None)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    assert_ne!(again, id);
    app.world
        .for_player(again)
        .call(move |w| {
            let stats = packet_stats(&w.stats(again, again)?);
            assert_eq!(stats[&32], 0x04000000);
            assert!(
                w.players[&again]
                    .character
                    .items
                    .iter()
                    .all(|item| *item != 0x1200)
            );
            assert_eq!(w.entities[&bag].items[0], -1);
            Ok(())
        })
        .await
        .unwrap();
    owner
        .close(&app, CloseReason::Transport)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn ground_dye_caller_preserves_legacy_inventory_dye_and_cloth_semantics() {
    for kind in [0x1000, 0x1100, 0x1200, 0x1300] {
        let (dir, app, _owner, id, bag) = fixture(kind).await;
        let mut before = app.store.load(1, 1).await.unwrap();
        before.items[4] = i32::from(kind);
        before.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: before.id,
                    account: before.account,
                    revision: before.revision,
                    items: before.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        let snapshot = before.clone();
        app.world
            .for_player(id)
            .call(move |w| {
                w.players.get_mut(&id).unwrap().character = snapshot;
                Ok(())
            })
            .await
            .unwrap();
        server::inventory(&app, id, move |w| {
            w.native_use_item(
                id,
                1,
                Slot {
                    object: id,
                    slot: 4,
                    kind: i32::from(kind),
                },
                w.players[&id].pos,
            )
        })
        .await
        .unwrap()
        .unwrap();
        let after = app.store.load(1, 1).await.unwrap();
        before.items[4] = -1;
        assert_eq!(after.items, before.items);
        assert_eq!(after.revision, before.revision + 1);
        let textures = app.content.get(kind).unwrap().textures;
        assert_eq!(
            [after.tex1, after.tex2],
            [
                textures[0].unwrap_or(before.tex1),
                textures[1].unwrap_or(before.tex2)
            ]
        );
        app.world
            .for_player(id)
            .call(move |w| {
                assert_eq!(w.entities[&bag].items[0], i32::from(kind));
                assert!(!w.entities[&bag].reserved);
                Ok(())
            })
            .await
            .unwrap();
        app.shutdown.send_replace(true);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

async fn use_ground(app: &server::App, id: i32, bag: i32, kind: u16) -> Result<()> {
    server::inventory(app, id, move |w| {
        w.native_use_item(
            id,
            1,
            Slot {
                object: bag,
                slot: 0,
                kind: i32::from(kind),
            },
            w.players[&id].pos,
        )
    })
    .await?
}

async fn state(app: &server::App, id: i32, bag: i32) -> serde_json::Value {
    app.world
        .for_player(id)
        .call(move |w| {
            Ok(serde_json::json!({
                "character": w.players[&id].character,
                "pending": w.players[&id].pending,
                "quarantined": w.players[&id].persistence_failure.is_some(),
                "bag": w.entities[&bag].items,
                "reserved": w.entities[&bag].reserved,
            }))
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn ground_dye_refusals_validate_actual_container_slot_access_range_owner_and_readiness() {
    for denial in [
        "foreign owner",
        "unknown",
        "unpublished",
        "reserved",
        "range",
        "slot",
        "kind",
        "class",
        "non ephemeral",
        "vault",
        "dead",
        "hp",
        "paused",
        "pending",
        "departing",
        "death pending",
        "not ready",
        "foreign world",
    ] {
        let (dir, app, _owner, id, bag) = fixture(0x1000).await;
        let foreign = app
            .world
            .in_world(-1)
            .call(|w| w.loot_bag(Pos { x: 9.5, y: 196.5 }, Some(1), vec![0x1000]))
            .await
            .unwrap();
        let slot = app
            .world
            .for_player(id)
            .call(move |w| {
                let mut slot = Slot {
                    object: bag,
                    slot: 0,
                    kind: 0x1000,
                };
                match denial {
                    "foreign owner" => {
                        w.entities.get_mut(&bag).unwrap().extra_stats = vec![(54, 9999)]
                    }
                    "unknown" => {
                        w.players.get_mut(&id).unwrap().known.remove(&bag);
                    }
                    "unpublished" => w.entities.get_mut(&bag).unwrap().published = false,
                    "reserved" => w.entities.get_mut(&bag).unwrap().reserved = true,
                    "range" => w.entities.get_mut(&bag).unwrap().pos.x += 1.5,
                    "slot" => slot.slot = 8,
                    "kind" => slot.kind = 0x1100,
                    "class" => w.entities.get_mut(&bag).unwrap().kind = 0x1000,
                    "non ephemeral" => {
                        w.bag_expiry.remove(&bag);
                    }
                    "vault" => w.entities.get_mut(&bag).unwrap().kind = 0x504,
                    "dead" => w.players.get_mut(&id).unwrap().character.alive = false,
                    "hp" => w.players.get_mut(&id).unwrap().character.hp = 0,
                    "paused" => w.players.get_mut(&id).unwrap().paused = true,
                    "pending" => w.players.get_mut(&id).unwrap().pending = true,
                    "departing" => w.players.get_mut(&id).unwrap().departing = true,
                    "death pending" => w.players.get_mut(&id).unwrap().death_pending = true,
                    "not ready" => {
                        w.players[&id].tx.as_ref().unwrap().lost.send_replace(true);
                    }
                    "foreign world" => slot.object = foreign,
                    _ => unreachable!(),
                }
                Ok(slot)
            })
            .await
            .unwrap();
        let before = state(&app, id, bag).await;
        let ch = &before["character"];
        let saved = app
            .store
            .load(
                ch["account"].as_i64().unwrap() as i32,
                ch["id"].as_i64().unwrap() as i32,
            )
            .await
            .unwrap();
        let error = server::inventory(&app, id, move |w| {
            w.native_use_item(id, 1, slot, w.players[&id].pos)
        })
        .await
        .unwrap()
        .unwrap_err();
        assert!(
            server::lifecycle::rule_rejection(&error),
            "{denial}: {error:#}"
        );
        assert_eq!(state(&app, id, bag).await, before, "{denial}");
        let after = app.store.load(saved.account, saved.id).await.unwrap();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            serde_json::to_value(saved).unwrap(),
            "{denial}"
        );
        app.shutdown.send_replace(true);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn ground_dye_revalidates_character_and_source_revisions_before_store_dispatch() {
    for mutation in ["character", "contents", "reservation"] {
        let (dir, app, _owner, id, bag) = fixture(0x1000).await;
        let before = state(&app, id, bag).await;
        let saved = app.store.load(1, 1).await.unwrap();
        let error = server::inventory(&app, id, move |w| {
            let plan = w.native_use_item(
                id,
                1,
                Slot {
                    object: bag,
                    slot: 0,
                    kind: 0x1000,
                },
                w.players[&id].pos,
            )?;
            match mutation {
                "character" => w.players.get_mut(&id).unwrap().character.revision += 1,
                "contents" => w.entities.get_mut(&bag).unwrap().items[1] = 0xa22,
                "reservation" => w.entities.get_mut(&bag).unwrap().reserved = false,
                _ => unreachable!(),
            }
            Ok(plan)
        })
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("reservation") || error.to_string().contains("source"),
            "{error:#}"
        );
        let failed = state(&app, id, bag).await;
        assert_eq!(failed["character"]["tex1"], before["character"]["tex1"]);
        assert_eq!(failed["character"]["tex2"], before["character"]["tex2"]);
        assert_eq!(failed["quarantined"], true);
        assert_eq!(failed["reserved"], true);
        assert_eq!(failed["bag"][0], 0x1000);
        assert_eq!(
            serde_json::to_value(app.store.load(1, 1).await.unwrap()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
        app.shutdown.send_replace(true);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn ground_dye_stale_durable_character_revision_does_not_consume_source() {
    let (dir, app, _owner, id, bag) = fixture(0x1100).await;
    let before = state(&app, id, bag).await;
    let ch = app.store.load(1, 1).await.unwrap();
    app.store
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
        .unwrap();
    let saved = app.store.load(1, 1).await.unwrap();
    let error = use_ground(&app, id, bag, 0x1100).await.unwrap_err();
    assert!(
        format!("{error:#}").contains("stale character"),
        "{error:#}"
    );
    let failed = state(&app, id, bag).await;
    assert_eq!(failed["character"], before["character"]);
    assert_eq!(failed["bag"], before["bag"]);
    assert_eq!(failed["reserved"], true);
    assert_eq!(failed["quarantined"], true);
    assert_eq!(
        serde_json::to_value(app.store.load(1, 1).await.unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn ground_dye_sql_abort_rolls_back_and_quarantines_without_refund_or_retry() {
    let (dir, app, _owner, id, bag) = fixture(0x1300).await;
    let before = state(&app, id, bag).await;
    let saved = app.store.load(1, 1).await.unwrap();
    app.store.lifecycle_test_job(|db| {
        db.execute_batch("CREATE TRIGGER abort_ground_dye AFTER UPDATE OF stats ON characters BEGIN SELECT RAISE(ABORT,'ground dye rollback probe'); END;")?;
        Ok(())
    }).await.unwrap();
    let error = use_ground(&app, id, bag, 0x1300).await.unwrap_err();
    assert!(
        format!("{error:#}").contains("ground dye rollback probe"),
        "{error:#}"
    );
    let failed = state(&app, id, bag).await;
    assert_eq!(failed["character"], before["character"]);
    assert_eq!(failed["bag"], before["bag"]);
    assert_eq!(failed["reserved"], true);
    assert_eq!(failed["quarantined"], true);
    assert_eq!(
        serde_json::to_value(app.store.load(1, 1).await.unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    assert!(use_ground(&app, id, bag, 0x1300).await.is_err());
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ground_dye_uncertain_store_result_and_postcommit_source_fault_keep_committed_item_quarantined()
 {
    for fault in [
        "lost Store result",
        "contents",
        "owner",
        "kind",
        "reservation",
        "expiry",
        "character",
        "textures",
    ] {
        let (dir, app, _owner, id, bag) = fixture(0x1200).await;
        let before = state(&app, id, bag).await;
        let (ready, release) = app.store.lifecycle_test_pause_reply();
        let a = app.clone();
        let task = tokio::spawn(async move { use_ground(&a, id, bag, 0x1200).await });
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT json_extract(stats,'$.tex1') FROM characters",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
            0x04000000
        );
        let pending = state(&app, id, bag).await;
        assert_eq!(pending["character"], before["character"]);
        assert_eq!(pending["bag"], before["bag"]);
        assert_eq!(pending["reserved"], true);
        app.world
            .for_player(id)
            .call(move |w| {
                match fault {
                    "lost Store result" => {}
                    "contents" => w.entities.get_mut(&bag).unwrap().items[1] = 0xa22,
                    "owner" => w.entities.get_mut(&bag).unwrap().extra_stats = vec![(54, 9999)],
                    "kind" => w.entities.get_mut(&bag).unwrap().kind ^= 3,
                    "reservation" => w.entities.get_mut(&bag).unwrap().reserved = false,
                    "expiry" => {
                        w.bag_expiry.insert(bag, u64::MAX);
                    }
                    "character" => w.players.get_mut(&id).unwrap().character.revision += 1,
                    "textures" => w.players.get_mut(&id).unwrap().character.tex2 = 99,
                    _ => unreachable!(),
                }
                Ok(())
            })
            .await
            .unwrap();
        if fault != "lost Store result" {
            release.send(()).unwrap();
        }
        drop(release);
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        if fault == "lost Store result" {
            assert!(
                format!("{error:#}").contains("Store gate abandoned"),
                "{error:#}"
            );
        }
        let failed = state(&app, id, bag).await;
        assert_eq!(failed["reserved"], true, "{fault}");
        assert_eq!(failed["quarantined"], true, "{fault}");
        assert_eq!(failed["pending"], true, "{fault}");
        assert_eq!(failed["bag"][0], 0x1200, "{fault}");
        let durable = app.store.load(1, 1).await.unwrap();
        assert_eq!(durable.tex1, 0x04000000);
        assert_eq!(durable.tex2, 0);
        assert_eq!(
            durable.revision,
            before["character"]["revision"].as_i64().unwrap() as i32 + 1
        );
        assert_eq!(
            durable.items,
            serde_json::from_value::<Vec<i32>>(before["character"]["items"].clone()).unwrap()
        );
        assert!(use_ground(&app, id, bag, 0x1200).await.is_err());
        app.shutdown.send_replace(true);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
