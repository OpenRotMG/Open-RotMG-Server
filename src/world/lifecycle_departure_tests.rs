use super::*;
use crate::content::WIZARD;
use crate::server::{self, lifecycle};

struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

async fn departure_first(progress: bool, dying: bool, amulet: bool) {
    let (dir, a) = server::lifecycle_test_fixture();
    a.world.clock.set_paused(true);
    let account = a.store.list("departure-first".into()).await.unwrap().id;
    let mut ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    if amulet {
        ch.items[3] = 0xb3e;
        ch.revision = a
            .store
            .exchange(
                vec![crate::store::InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
    }
    let initial = ch.clone();
    let presentation = a.store.presentation(account).await.unwrap();
    let owner = a.claim(account).unwrap();
    let bind = lifecycle::binding(&a, account).unwrap();
    let id = a
        .world
        .call(move |w| {
            let id = w.enter_at(ch, None, None, presentation)?;
            bind(id)?;
            let p = w.players.get_mut(&id).unwrap();
            if progress {
                p.queue_experience(50, None, false).unwrap();
                p.clears_queued[0] = 1;
            }
            if dying {
                p.character.hp = 0;
                p.death_pending = true;
                p.killer = "Pirate".into();
            }
            Ok(id)
        })
        .await
        .unwrap();
    let entered_lifetime = a
        .world
        .call(move |w| Ok(w.players[&id].character.lifetime.clone()))
        .await
        .unwrap();
    assert!(
        entered_lifetime.tiles > 0,
        "ordinary entry must create telemetry without XP"
    );
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TABLE departure_updates(alive INTEGER, stats TEXT); CREATE TRIGGER departure_audit AFTER UPDATE ON characters WHEN OLD.account={account} BEGIN INSERT INTO departure_updates VALUES(NEW.alive,NEW.stats); END;")).unwrap();

    // The first departure owns admission before either coordinator can prepare its prerequisites.
    let (entered, release) = a.world.lifecycle_test_pause_job(1, false);
    let release = Release(Some(release));
    let (written, write_gate) = tokio::sync::oneshot::channel();
    let mut hook = Some(a.store.lifecycle_test_write_reply_hook());
    let actor = initial.id;
    a.store
        .lifecycle_test_job(move |db| {
            let mut written = Some(written);
            db.update_hook(Some(
                move |action: rusqlite::hooks::Action, _: &str, table: &str, row: i64| {
                    if action == rusqlite::hooks::Action::SQLITE_UPDATE
                        && table == "characters"
                        && row == i64::from(actor)
                    {
                        if let Some(written) = written.take() {
                            written.send(hook.take().unwrap()()).unwrap();
                        }
                    }
                },
            ));
            Ok(())
        })
        .await
        .unwrap();
    let app = a.clone();
    let waiter = tokio::spawn(async move {
        let _owner = owner;
        server::depart(&app, id).await
    });
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    assert!(a.claim(account).is_err());
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(
        a.claim(account).is_err(),
        "departure lost custody with its waiter"
    );
    drop(release);
    // The real collector attempts admission after departure has custody, even on the RED schedule.
    server::persistence_tick(&a).await.unwrap();
    // Credit refresh reads precede the write; hold the actual SQL update's reply, not an incidental read.
    let (stored, store_release) = tokio::time::timeout(Duration::from_secs(2), write_gate)
        .await
        .unwrap()
        .unwrap();
    let store_release = Release(Some(store_release));
    tokio::time::timeout(Duration::from_secs(2), stored)
        .await
        .expect("departure admitted first deadlocked before persisting queued progress/death")
        .unwrap();
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.departing);
            if progress {
                assert!(p.pending && p.progress_started);
                assert_eq!((p.character.exp, p.character.pirate_caves), (0, 0));
                assert!(!p.death_started);
            } else {
                assert!(p.death_started && p.death_pending);
            }
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        a.claim(account).is_err(),
        "SQL commit released admission before World publication"
    );
    assert_eq!(
        a.store.lifecycle_test_jobs(),
        1,
        "must hold actual committed writer, not following read/job"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM departure_updates", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    drop(store_release);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(fresh) = a.claim(account) {
                drop(fresh);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("successful departure did not acknowledge retirement");
    assert!(
        !a.world
            .call(move |w| Ok(w.players.contains_key(&id)))
            .await
            .unwrap()
    );
    let (alive, revision, items, stats): (bool, i32, String, String) = db
        .query_row(
            "SELECT alive,revision,items,stats FROM characters WHERE id=?1",
            [initial.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    let saved: crate::store::Character = serde_json::from_str(&stats).unwrap();
    assert_eq!(
        (saved.exp, saved.level, saved.pirate_caves),
        if progress { (50, 2, 1) } else { (0, 1, 0) }
    );
    assert_eq!(alive, !dying || amulet);
    assert_eq!(revision, initial.revision + i32::from(dying));
    let death_count = db
        .query_row(
            "SELECT count(*) FROM deaths WHERE character=?1",
            [initial.id],
            |r| r.get::<_, i32>(0),
        )
        .unwrap();
    assert_eq!(death_count, i32::from(dying && !amulet));
    assert_eq!(
        saved.lifetime, entered_lifetime,
        "departure/death/resurrection lost real entry telemetry"
    );
    let expected_updates = i32::from(progress)
        + if dying && !amulet {
            1
        } else if amulet {
            2
        } else {
            1
        };
    assert_eq!(
        db.query_row("SELECT count(*) FROM departure_updates", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        expected_updates,
        "prerequisite/final save replayed"
    );
    let items: Vec<i32> = serde_json::from_str(&items).unwrap();
    if dying && !amulet {
        assert!(items.iter().all(|kind| *kind == -1));
    }
    if amulet {
        assert_eq!(items[3], -1);
        assert!(saved.hp > 0);
    }
    if dying && !amulet {
        let snapshot: String = db
            .query_row(
                "SELECT snapshot FROM deaths WHERE character=?1",
                [initial.id],
                |r| r.get(0),
            )
            .unwrap();
        let dead: crate::store::Character = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(
            (dead.exp, dead.pirate_caves, dead.hp),
            (saved.exp, saved.pirate_caves, 0)
        );
        assert!(
            a.world
                .call(|w| Ok(w
                    .entities
                    .values()
                    .any(|e| matches!(e.kind, 0x723..=0x72b | 0x734 | 0x735))))
                .await
                .unwrap()
        );
    }
    // Unrelated admission and real persistence/publication still work after the cancelled close.
    let sibling = a.store.list("departure-sibling".into()).await.unwrap().id;
    let sibling_ch = a
        .store
        .create(sibling, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let owner = a.claim(sibling).unwrap();
    let sibling_id = a
        .world
        .call(move |w| w.enter(sibling_ch, None))
        .await
        .unwrap();
    server::depart(&a, sibling_id).await.unwrap();
    drop(owner);
    assert!(a.claim(sibling).is_ok());
    a.store
        .lifecycle_test_job(|db| {
            db.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(a.store.lifecycle_test_jobs(), 0);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_departure_first_queued_xp_and_clear_cancelled_waiter() {
    departure_first(true, false, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_departure_first_queued_xp_clear_then_unprepared_death() {
    departure_first(true, true, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_departure_first_unprepared_death_cancelled_waiter() {
    departure_first(false, true, false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_departure_first_queued_progress_then_resurrection() {
    departure_first(true, true, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_death_fame_phase3_dirty_terminal_snapshots() {
    departure_first(false, true, false).await;
    departure_first(false, true, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_death_fame_phase4_real_lethal_sources_and_late_seal() {
    let (_dir, mut a) = server::lifecycle_test_fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let mut owners = vec![];
    let mut actors = vec![];
    for (index, guid) in [
        "real-credit-recipient",
        "real-credit-a",
        "real-credit-b",
        "real-credit-unrelated",
    ]
    .into_iter()
    .enumerate()
    {
        let account = a.store.list(guid.into()).await.unwrap().id;
        let mut ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        if index == 0 {
            ch.exp = 49;
            a.store.progress(ch.clone()).await.unwrap();
        }
        let character = ch.id;
        owners.push(a.claim(account).unwrap());
        let bind = lifecycle::binding(&a, account).unwrap();
        let id = a
            .world
            .call(move |w| {
                let id = w.enter(ch, None)?;
                bind(id)?;
                Ok(id)
            })
            .await
            .unwrap();
        actors.push((id, account, character));
    }
    server::persistence_tick(&a).await.unwrap();
    let (recipient, _, recipient_ch) = actors[0];
    let (giver, giver_account, giver_ch) = actors[1];
    let (b, _, b_ch) = actors[2];
    let (unrelated, _, _) = actors[3];
    a.world
        .call(move |w| {
            // Selected enemy with prepared high HP/quest reward exercises existing cap and aggregate XP.
            let definition = Arc::make_mut(&mut w.content)
                .objects
                .get_mut(&0x600)
                .unwrap();
            definition.hp = 100_000;
            definition.xp_multiplier = 1.;
            definition.quest = true;
            let pos = w.players[&recipient].pos;
            for id in [giver, b, unrelated] {
                w.players.get_mut(&id).unwrap().pos = pos;
            }
            for owner in std::iter::once(giver).chain(std::iter::repeat_n(b, 6)) {
                let enemy = w.spawn_entity(0x600, pos, vec![])?;
                w.damage_enemy(recipient, enemy, 1, true, &[], None)?;
                w.damage_enemy(owner, enemy, 200_000, true, &[], None)?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let (entered, release) = a.store.lifecycle_test_pause_reply();
    let release = Release(Some(release));
    let app = a.clone();
    let collector = tokio::spawn(async move { server::persistence_tick(&app).await });
    tokio::time::timeout(Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    let finalization = lifecycle::finalize(&a, giver, lifecycle::CloseReason::Controller).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if a.world
                .call(move |w| Ok(w.players[&giver].departing))
                .await
                .unwrap()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    a.world
        .call(move |w| {
            assert!(w.players[&giver].departure_snapshot.is_none());
            // Actual delayed kills after authority seal still grant shared XP, but no NEW giver credit.
            let pos = w.players[&recipient].pos;
            for _ in 0..10 {
                let enemy = w.spawn_entity(0x600, pos, vec![])?;
                w.damage_enemy(recipient, enemy, 1, true, &[], None)?;
                w.damage_enemy(giver, enemy, 200_000, true, &[], None)?;
            }
            let p = &w.players[&recipient];
            assert!(p.experience_queued > 0);
            assert!(
                p.experience_queue
                    .as_ref()
                    .unwrap()
                    .assists
                    .iter()
                    .flatten()
                    .all(|assist| assist.giver != giver_ch)
            );
            assert!(
                p.level_assists_prepared
                    .iter()
                    .any(|assist| assist.giver == giver_ch)
            );
            Ok(())
        })
        .await
        .unwrap();
    drop(release);
    tokio::time::timeout(Duration::from_secs(5), collector)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), finalization.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a.store.assist_count(giver_ch).await.unwrap(), 1);
    assert_eq!(a.store.assist_count(b_ch).await.unwrap(), 1);
    assert_eq!(
        a.store
            .load(giver_account, giver_ch)
            .await
            .unwrap()
            .lifetime
            .level_assists,
        1
    );
    server::persistence_tick(&a).await.unwrap();
    let account = actors[0].1;
    let saved = a.store.load(account, recipient_ch).await.unwrap();
    assert!(saved.level >= 4);
    assert_eq!(a.store.assist_count(giver_ch).await.unwrap(), 1);
    drop(owners);
}
