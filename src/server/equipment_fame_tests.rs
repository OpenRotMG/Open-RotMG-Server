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
    assert_eq!(a.store.presentation(account.id).await.unwrap().fame, 19);
    let xml = a.store.fame(account.id, character).await.unwrap();
    assert!(xml.contains("<BaseFame>18</BaseFame>"));
    assert!(xml.contains("<TotalFame>19</TotalFame>"));
    assert!(xml.contains("<Bonus id=\"Well Equipped\" desc=\"Bonus for equipment\">1</Bonus>"));
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
