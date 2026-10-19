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
                p.experience_queued = 50;
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
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TABLE departure_updates(alive INTEGER, stats TEXT); CREATE TRIGGER departure_audit AFTER UPDATE ON characters WHEN OLD.account={account} BEGIN INSERT INTO departure_updates VALUES(NEW.alive,NEW.stats); END;")).unwrap();

    // The first departure owns admission before either coordinator can prepare its prerequisites.
    let (entered, release) = a.world.lifecycle_test_pause_job(1, false);
    let release = Release(Some(release));
    let (stored, store_release) = a.store.lifecycle_test_pause_reply();
    let store_release = Release(Some(store_release));
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
