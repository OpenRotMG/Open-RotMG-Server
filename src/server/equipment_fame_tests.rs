use super::*;
use crate::store::InventoryWrite;

#[tokio::test]
async fn equipment_death_waits_for_final_inventory_and_boost_excluded_queued_progress() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let account = a.store.list("equipment-queued".into()).await.unwrap();
    let _owner = a.claim(account.id).unwrap();
    let bind = lifecycle::binding(&a, account.id).unwrap();
    let mut ch = a
        .store
        .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(&a.content, 17999, &mut rand::thread_rng())
        .unwrap();
    ch.xp_boost.active = true;
    ch.items = vec![0xaa2, -1, -1, -1, 0xabf, 0xc01, -1, -1, -1, -1, -1, -1];
    ch.revision = a
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
    a.store.progress(ch.clone()).await.unwrap();
    let character = ch.id;
    let (packets, mut rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    let id = a
        .world
        .call(move |w| {
            let id = w.enter(ch, Some(crate::world::Output { packets, lost }))?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    let plan = a
        .world
        .call(move |w| {
            let plan = w.swap(
                id,
                Slot {
                    object: id,
                    slot: 4,
                    kind: 0xabf,
                },
                Slot {
                    object: id,
                    slot: 3,
                    kind: -1,
                },
            )?;
            let p = w.players.get_mut(&id).unwrap();
            p.queue_experience(501, None, false).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            assert!(w.pending_progress().is_empty());
            assert!(w.pending_deaths().is_empty());
            Ok(plan)
        })
        .await
        .unwrap();
    assert_eq!(
        a.store.load(account.id, character).await.unwrap().items[3],
        -1
    );
    persist(&a, plan).await.unwrap();
    let snapshot = a
        .world
        .call(move |w| {
            assert!(w.pending_deaths().is_empty());
            assert_eq!(w.players[&id].character.items[3], 0xabf);
            Ok(w.pending_progress().pop().unwrap().1)
        })
        .await
        .unwrap();
    assert_eq!(
        (snapshot.exp, snapshot.xp_boost.bonus_exp, snapshot.fame),
        (19001, 501, 18)
    );
    a.store.progress(snapshot.clone()).await.unwrap();
    let fatal = a
        .world
        .call(move |w| {
            w.finish_progress(id, snapshot)?;
            assert_eq!(w.players[&id].character.hp, 0);
            Ok(w.pending_deaths().pop().unwrap().1)
        })
        .await
        .unwrap();
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|packet| packet.id == 41));
    persist_death(&a, id, fatal, "Queued equipment fixture".into())
        .await
        .unwrap();
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|packet| packet.id == 41));
    assert_eq!(a.store.presentation(account.id).await.unwrap().fame, 160);
    let xml = a.store.fame(account.id, character).await.unwrap();
    assert!(xml.contains("<BaseFame>18</BaseFame>"));
    assert!(xml.contains("<TotalFame>160</TotalFame>"));
    assert!(xml.contains("<Bonus id=\"Well Equipped\" desc=\"Bonus for equipment\">8</Bonus>"));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM deaths WHERE character=?1",
            [character],
            |r| r.get(0),
        )
        .unwrap();
    let archived: crate::store::Character = serde_json::from_str(&raw).unwrap();
    assert_eq!(archived.items[3], 0xabf);
    assert_eq!(archived.items[4], -1);
    assert_eq!(archived.fame, 18);
    assert!(!archived.xp_boost.active);
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    assert_eq!(reopened.fame(account.id, character).await.unwrap(), xml);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn equipment_death_storage_failure_never_publishes_death_or_reward() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let account = a
        .store
        .list("equipment-death-failure".into())
        .await
        .unwrap();
    let _owner = a.claim(account.id).unwrap();
    let bind = lifecycle::binding(&a, account.id).unwrap();
    let ch = a
        .store
        .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let (packets, mut rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    let (id, fatal) = a
        .world
        .call(move |w| {
            let id = w.enter(ch, Some(crate::world::Output { packets, lost }))?;
            bind(id)?;
            let p = w.players.get_mut(&id).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            Ok((id, w.pending_deaths().pop().unwrap().1))
        })
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_death BEFORE INSERT ON deaths BEGIN SELECT RAISE(ABORT,'forced death storage failure'); END").unwrap();
    assert!(
        persist_death(&a, id, fatal, "rollback enemy".into())
            .await
            .is_err()
    );
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|p| p.id == 41));
    assert!(a.store.load(account.id, character).await.unwrap().alive);
    assert_eq!(a.store.presentation(account.id).await.unwrap().fame, 0);
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        0
    );
    a.world
        .call(move |w| {
            assert!(w.begin_departure(id).is_err());
            assert!(w.pending_deaths().is_empty());
            Ok(())
        })
        .await
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn all_death_fame_phase2_progress_credits_atomic() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let mut characters = Vec::new();
    for guid in ["phase2-recipient", "phase2-giver-a", "phase2-giver-b"] {
        let account = a.store.list(guid.into()).await.unwrap();
        characters.push(
            a.store
                .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap(),
        );
    }
    let mut recipient = characters.remove(0);
    let (giver_a, giver_b) = (characters[0].id, characters[1].id);
    recipient.exp = 49;
    a.store.progress(recipient.clone()).await.unwrap();
    let character = recipient.id;
    let id = a
        .world
        .call(move |w| w.enter(recipient, None))
        .await
        .unwrap();
    let snapshot = a
        .world
        .call(move |w| {
            let p = w.players.get_mut(&id).unwrap();
            p.queue_experience(1, Some(giver_a), true)?;
            p.queue_experience(149, None, false)?;
            p.queue_experience(1, Some(giver_b), true)?;
            Ok(w.pending_progress().pop().unwrap().1)
        })
        .await
        .unwrap();
    let reservation = snapshot.clone();
    let credits = a
        .world
        .for_player(id)
        .call(move |w| w.progress_assists(id, &reservation))
        .await;
    let saved = a
        .world
        .for_player(id)
        .call(move |w| Ok(w.players[&id].character.clone()))
        .await
        .unwrap();
    assert!(credits.is_ok());
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_level_credit BEFORE INSERT ON level_credits BEGIN SELECT RAISE(ABORT,'injected level credit failure'); END").unwrap();
    let prepared = snapshot;
    let assists = credits.unwrap();
    assert!(
        a.store
            .progress_with_assists(prepared.clone(), assists.clone())
            .await
            .is_err()
    );
    assert_eq!(
        a.store.load(saved.account, character).await.unwrap().exp,
        49
    );
    assert_eq!(a.store.assist_count(giver_a).await.unwrap(), 0);
    db.execute_batch("DROP TRIGGER fail_level_credit").unwrap();
    a.store
        .progress_with_assists(prepared.clone(), assists.clone())
        .await
        .unwrap();
    a.store
        .progress_with_assists(prepared.clone(), assists)
        .await
        .unwrap();
    assert_eq!(a.store.assist_count(giver_a).await.unwrap(), 1);
    assert_eq!(a.store.assist_count(giver_b).await.unwrap(), 1);
    assert!(
        a.store.progress(saved).await.is_err(),
        "stale snapshot must not overwrite recipient progress or credits"
    );
    let mut orphan = prepared.clone();
    orphan
        .gain_experience(&a.content, 250, &mut rand::thread_rng())
        .unwrap();
    assert!(
        a.store
            .progress_with_assists(
                orphan,
                vec![crate::store::LevelAssist {
                    reached_level: 4,
                    giver: 999999
                }]
            )
            .await
            .is_err()
    );
    assert_eq!(
        a.store.load(prepared.account, character).await.unwrap().exp,
        200
    );
    let mut foreign = prepared.clone();
    foreign.account += 1;
    assert!(
        a.store
            .progress_with_assists(foreign, vec![])
            .await
            .is_err()
    );
    assert!(
        a.store
            .progress_with_assists(
                prepared.clone(),
                vec![crate::store::LevelAssist {
                    reached_level: 2,
                    giver: giver_b
                }]
            )
            .await
            .is_err()
    );
    a.world
        .for_player(id)
        .call(move |w| w.finish_progress(id, prepared))
        .await
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM level_credits WHERE recipient=?1",
            [character],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        2
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn all_death_fame_phase2_progress_credits_reply_gates() {
    for lost in [false, true] {
        let (dir, a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let mut chars = Vec::new();
        for guid in ["phase2-reply-recipient", "phase2-reply-a", "phase2-reply-b"] {
            let owner = a.store.list(guid.into()).await.unwrap();
            chars.push(
                a.store
                    .create(owner.id, a.content.get(crate::content::WIZARD).unwrap())
                    .await
                    .unwrap(),
            );
        }
        let mut ch = chars.remove(0);
        ch.exp = 49;
        let (a_giver, b_giver) = (chars[0].id, chars[1].id);
        a.store.progress(ch.clone()).await.unwrap();
        let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
        let snapshot = a
            .world
            .for_player(id)
            .call(move |w| {
                let p = w.players.get_mut(&id).unwrap();
                p.queue_experience(1, Some(a_giver), true)?;
                p.queue_experience(149, None, false)?;
                p.queue_experience(1, Some(b_giver), true)?;
                Ok(w.pending_progress().pop().unwrap().1)
            })
            .await
            .unwrap();
        let character = snapshot.id;
        let reservation = snapshot.clone();
        a.world
            .for_player(id)
            .call(move |w| {
                let mut wrong = reservation.clone();
                wrong.fame += 1;
                assert!(
                    w.progress_assists(id, &wrong).is_err(),
                    "same exp/level is not exact reservation identity"
                );
                assert!(w.finish_progress(id, wrong).is_err());
                Ok(())
            })
            .await
            .unwrap();
        if lost {
            a.store.lifecycle_test_lose_reply();
        }
        let (entered, release) = a.store.lifecycle_test_pause_reply();
        let app = a.clone();
        let prepared = snapshot.clone();
        let task = tokio::spawn(async move { persist_progress_inner(&app, id, prepared).await });
        entered.await.unwrap();
        let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT json_extract(stats,'$.exp') FROM characters WHERE id=?1",
                [character],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            200
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM level_credits WHERE recipient=?1",
                [character],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            2
        );
        a.world
            .for_player(id)
            .call(move |w| {
                assert_eq!(w.players[&id].character.level, 1);
                assert!(w.players[&id].pending);
                assert_eq!(w.players[&id].level_assists_prepared.len(), 2);
                assert!(w.pending_progress().is_empty());
                Ok(())
            })
            .await
            .unwrap();
        release.send(()).unwrap();
        assert_eq!(task.await.unwrap().is_err(), lost);
        a.world
            .for_player(id)
            .call(move |w| {
                assert_eq!(w.players[&id].character.level, if lost { 1 } else { 3 });
                assert_eq!(w.players[&id].pending, lost);
                Ok(())
            })
            .await
            .unwrap();
        if lost {
            a.store
                .progress_with_assists(
                    snapshot,
                    vec![
                        crate::store::LevelAssist {
                            reached_level: 2,
                            giver: a_giver,
                        },
                        crate::store::LevelAssist {
                            reached_level: 3,
                            giver: b_giver,
                        },
                    ],
                )
                .await
                .unwrap();
        }
        assert_eq!(a.store.assist_count(a_giver).await.unwrap(), 1);
        assert_eq!(a.store.assist_count(b_giver).await.unwrap(), 1);
        let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
        assert_eq!(reopened.assist_count(a_giver).await.unwrap(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn all_death_fame_phase3_atomic_inventory_potions() {
    for kind in [0xa22, 0xa23, 0xa1f, 0xae9, 0xb0c, 0xb10, 0xc12] {
        for fail in [false, true] {
            let (dir, mut a) = account_actions::tests::fixture();
            a.world.clock.set_paused(true);
            a.world = a.world.in_world(-3);
            let account = a.store.list("phase3-potion".into()).await.unwrap().id;
            let mut ch = a
                .store
                .create(account, a.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            ch.items[5] = kind;
            ch.revision = a
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap()[0];
            let character = ch.id;
            let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
            let peer = a.store.list("phase3-beneficiary".into()).await.unwrap().id;
            let other = a
                .store
                .create(peer, a.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            let beneficiary = a.world.call(move |w| w.enter(other, None)).await.unwrap();
            let plan = a
                .world
                .call(move |w| {
                    let pos = w.players[&id].pos;
                    w.players.get_mut(&beneficiary).unwrap().pos = pos;
                    w.use_item(
                        id,
                        Slot {
                            object: id,
                            slot: 5,
                            kind,
                        },
                        pos,
                    )?
                    .context("potion plan missing")
                })
                .await
                .unwrap();
            if fail {
                rusqlite::Connection::open(dir.join("game.db")).unwrap().execute_batch("CREATE TRIGGER fail_potion BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'injected potion failure'); END").unwrap();
                assert!(persist(&a, plan).await.is_err());
            } else {
                let (entered, release) = a.store.lifecycle_test_pause_reply();
                let app = a.clone();
                let task = tokio::spawn(async move { persist(&app, plan).await });
                entered.await.unwrap();
                let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
                let saved = reopened.load(account, character).await.unwrap();
                assert!(saved.lifetime.inventory_potion_used);
                assert_eq!(saved.items[5], -1);
                a.world
                    .call(move |w| {
                        assert!(
                            !w.players[&id].character.lifetime.inventory_potion_used,
                            "no pre-ack publication"
                        );
                        assert_eq!(w.players[&id].character.items[5], kind);
                        let pos = w.players[&id].pos;
                        let next = [(0.6, 0.), (-0.6, 0.), (0., 0.6), (0., -0.6)]
                            .into_iter()
                            .map(|(x, y)| Pos {
                                x: pos.x + x,
                                y: pos.y + y,
                            })
                            .find(|pos| w.walkable(*pos))
                            .context("fixture has no adjacent walkable tile")?;
                        w.move_to(id, next, w.now as i32)?;
                        w.updates(id, true)?;
                        Ok(())
                    })
                    .await
                    .unwrap();
                release.send(()).unwrap();
                task.await.unwrap().unwrap();
                let snapshot = a
                    .world
                    .call(move |w| {
                        assert!(w.players[&id].character.lifetime.inventory_potion_used);
                        assert!(
                            !w.players[&beneficiary]
                                .character
                                .lifetime
                                .inventory_potion_used
                        );
                        w.pending_progress_where(|player, _| player == id)
                            .pop()
                            .map(|(_, ch)| ch)
                            .context("late fog delta must remain dirty")
                    })
                    .await
                    .unwrap();
                persist_progress_inner(&a, id, snapshot).await.unwrap();
            }
            let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
            let saved = reopened.load(account, character).await.unwrap();
            assert_eq!(saved.items[5], if fail { kind } else { -1 });
            assert_eq!(saved.lifetime.inventory_potion_used, !fail);
            assert!(
                !reopened
                    .load(peer, character + 1)
                    .await
                    .unwrap()
                    .lifetime
                    .inventory_potion_used
            );
        }
    }
}

#[tokio::test]
async fn all_death_fame_phase3_stale_ack_merge() {
    for legacy in [false, true] {
        let (dir, mut a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let account = a.store.list("phase3-telemetry".into()).await.unwrap().id;
        let mut ch = a
            .store
            .create(account, a.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        // Prepared noncanonical progression proves telemetry cannot recompute Fame/levels/boost or growth.
        ch.exp = 2000;
        ch.fame = 7777;
        ch.xp_boost = crate::store::XpBoost {
            active: false,
            bonus_exp: 1000,
        };
        a.store.progress(ch.clone()).await.unwrap();
        if legacy {
            let mut raw = serde_json::to_value(&ch).unwrap();
            raw.as_object_mut().unwrap().remove("lifetime");
            raw["tiles"] = serde_json::json!(4_000_001_u64);
            raw["hits"] = serde_json::json!(u64::MAX);
            raw["quests"] = serde_json::json!(u64::MAX);
            let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
            db.execute("UPDATE characters SET stats=?1", [raw.to_string()])
                .unwrap();
            db.execute_batch("DROP TABLE legacy_claims; DROP TABLE level_credits; ALTER TABLE deaths DROP COLUMN receipt; PRAGMA user_version=15").unwrap();
            a.store = Store::open(&dir.join("game.db"), &a.content).unwrap();
            ch = a.store.load(account, ch.id).await.unwrap();
            assert_eq!(
                ch.lifetime.completeness,
                crate::store::LifetimeCompleteness::KnownOnly
            );
            assert_eq!(ch.lifetime.tiles, 4_000_001);
            assert_eq!(ch.lifetime.projectile_hits, 0);
        }
        ch.items[1] = 0xb24;
        ch.revision = a
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        let character = ch.id;
        let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
        let snapshot = a
            .world
            .call(move |w| {
                w.updates(id, true)?;
                w.pending_progress()
                    .pop()
                    .map(|(_, ch)| ch)
                    .context("telemetry alone must save")
            })
            .await
            .unwrap();
        let (entered, release) = a.store.lifecycle_test_pause_reply();
        let app = a.clone();
        let task = tokio::spawn(async move { persist_progress_inner(&app, id, snapshot).await });
        entered.await.unwrap();
        a.world
            .call(move |w| {
                let pos = w.players[&id].pos;
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 1,
                        kind: 0xb24,
                    },
                    pos,
                )?;
                Ok(())
            })
            .await
            .unwrap();
        release.send(()).unwrap();
        task.await.unwrap().unwrap();
        let later = a
            .world
            .call(move |w| {
                w.pending_progress()
                    .pop()
                    .map(|(_, c)| c)
                    .context("late fact lost dirtiness")
            })
            .await
            .unwrap();
        assert!(later.lifetime.ability_used);
        persist_progress_inner(&a, id, later).await.unwrap();
        let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
        let saved = reopened.load(account, character).await.unwrap();
        assert!(saved.lifetime.ability_used);
        assert!(saved.lifetime.tiles > 0);
        assert_eq!(
            (saved.exp, saved.level, saved.fame, saved.xp_boost.bonus_exp),
            (2000, 1, 7777, 1000)
        );
        assert!(!saved.xp_boost.active);
        assert_eq!(
            saved.lifetime.completeness,
            if legacy {
                crate::store::LifetimeCompleteness::KnownOnly
            } else {
                crate::store::LifetimeCompleteness::Complete
            }
        );
        assert_eq!(saved.lifetime.projectile_hits, 0);
        assert_eq!(saved.lifetime.qualified_quests, 0);
    }
}

#[tokio::test]
async fn all_death_fame_phase3_potion_sources_and_replays() {
    for kind in [0xa22, 0xa23, 0xa1f, 0xb0c, 0xb10, 0xc12] {
        for moved in [false, true] {
            let (_dir, a) = account_actions::tests::fixture();
            a.world.clock.set_paused(true);
            let account = a
                .store
                .list("phase3-potion-source".into())
                .await
                .unwrap()
                .id;
            let ch = a
                .store
                .create(account, a.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            let character = ch.id;
            let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
            let bag = a
                .world
                .call(move |w| {
                    let pos = w.players[&id].pos;
                    let mut items = vec![-1; 8];
                    items[0] = kind;
                    w.spawn_entity(0x500, pos, items)
                })
                .await
                .unwrap();
            let source = if moved {
                let plan = a
                    .world
                    .call(move |w| {
                        w.swap(
                            id,
                            Slot {
                                object: bag,
                                slot: 0,
                                kind,
                            },
                            Slot {
                                object: id,
                                slot: 8,
                                kind: -1,
                            },
                        )
                    })
                    .await
                    .unwrap();
                persist(&a, plan).await.unwrap();
                assert!(
                    !a.store
                        .load(account, character)
                        .await
                        .unwrap()
                        .lifetime
                        .inventory_potion_used
                );
                Slot {
                    object: id,
                    slot: 8,
                    kind,
                }
            } else {
                Slot {
                    object: bag,
                    slot: 0,
                    kind,
                }
            };
            let plan = a
                .world
                .call(move |w| {
                    let pos = w.players[&id].pos;
                    w.use_item(id, source, pos)?
                        .context("potion source plan missing")
                })
                .await
                .unwrap();
            let old = InventoryWrite {
                id: character,
                account,
                revision: plan.revision,
                items: plan.items.clone(),
            };
            persist(&a, plan).await.unwrap();
            let saved = a.store.load(account, character).await.unwrap();
            assert_eq!(saved.lifetime.inventory_potion_used, moved);
            if kind != 0xa1f {
                assert!(
                    a.store
                        .consume_inventory_potion(old, if moved { 8 } else { 0 }, kind as u16)
                        .await
                        .is_err()
                );
            }
            assert_eq!(
                a.store
                    .load(account, character)
                    .await
                    .unwrap()
                    .lifetime
                    .inventory_potion_used,
                moved
            );
        }
    }
    for kind in [0xc42, 0x1175] {
        let (_dir, a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let account = a.store.list("phase3-unrelated".into()).await.unwrap().id;
        let mut ch = a
            .store
            .create(account, a.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.items[5] = kind;
        ch.revision = a
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        let character = ch.id;
        let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
        let plan = a
            .world
            .call(move |w| {
                let pos = w.players[&id].pos;
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 5,
                        kind,
                    },
                    pos,
                )?
                .context("unrelated use plan missing")
            })
            .await
            .unwrap();
        persist(&a, plan).await.unwrap();
        assert!(
            !a.store
                .load(account, character)
                .await
                .unwrap()
                .lifetime
                .inventory_potion_used
        );
        let snapshot = a
            .world
            .call(move |w| {
                w.pending_progress_where(|p, _| p == id)
                    .pop()
                    .map(|(_, ch)| ch)
                    .context("unrelated acknowledgement erased dirty fog")
            })
            .await
            .unwrap();
        persist_progress_inner(&a, id, snapshot).await.unwrap();
    }
    let (_dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let account = a
        .store
        .list("phase3-prepared-stat-cap".into())
        .await
        .unwrap()
        .id;
    let mut ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.base[2] = a.content.classes[&ch.class].caps[2];
    a.store.progress(ch.clone()).await.unwrap();
    ch.items[5] = 0xa1f;
    ch.revision = a
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    let character = ch.id;
    let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
    a.world
        .call(move |w| {
            let pos = w.players[&id].pos;
            assert!(
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 5,
                        kind: 0xa1f
                    },
                    pos
                )
                .is_err()
            );
            assert!(!w.players[&id].character.lifetime.inventory_potion_used);
            assert_eq!(w.players[&id].character.items[5], 0xa1f);
            Ok(())
        })
        .await
        .unwrap();
    let saved = a.store.load(account, character).await.unwrap();
    assert!(!saved.lifetime.inventory_potion_used);
    assert_eq!(saved.items[5], 0xa1f);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_death_fame_phase5_no_publication_before_commit() {
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(reply) = self.0.take() {
                let _ = reply.send(());
            }
        }
    }
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let account = a.store.list("phase5-publish".into()).await.unwrap().id;
    let _owner = a.claim(account).unwrap();
    let mut ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = 100;
    a.store.progress(ch.clone()).await.unwrap();
    let character = ch.id;
    let bind = lifecycle::binding(&a, account).unwrap();
    let (tx, mut rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    let (id, snapshot) = a
        .world
        .call(move |w| {
            let id = w.enter(ch, Some(crate::world::Output { packets: tx, lost }))?;
            bind(id)?;
            let p = w.players.get_mut(&id).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            let snapshot = w.pending_deaths().pop().unwrap().1;
            Ok((id, snapshot))
        })
        .await
        .unwrap();
    let (entered, release) = a.store.lifecycle_test_pause_reply();
    let release = Release(Some(release));
    let app = a.clone();
    let work = tokio::spawn(async move {
        persist_death(&app, id, snapshot, "phase5 actual held reply".into()).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|p| p.id == 41));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT receipt FROM deaths WHERE character=?",
            [character],
            |r| r.get(0),
        )
        .unwrap();
    let paid = crate::store::death_awards::decode_receipt(&raw).unwrap();
    assert_eq!(paid.total, 143);
    assert_eq!(
        paid.policy,
        crate::store::death_awards::ReceiptPolicy::Historical
    );
    assert!(a.claim(account).is_err());
    drop(release);
    tokio::time::timeout(std::time::Duration::from_secs(5), work)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|p| p.id == 41)
            .count(),
        1
    );
    assert_eq!(a.store.presentation(account).await.unwrap().fame, 143);
}
