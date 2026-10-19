use super::*;
use crate::world::Slot;

async fn live(selected: bool, amulet: bool) -> (std::path::PathBuf, App, Profile, Character, i32) {
    let (dir, app) = crate::server::lifecycle_test_fixture();
    app.world.clock.set_paused(true);
    let profile = *PROFILES.iter().find(|p| p.healer).unwrap();
    let mut ch = app
        .store
        .shore_character(
            profile.identity.into(),
            profile.name.into(),
            &app.content,
            true,
        )
        .await
        .unwrap();
    ch.level = 5;
    ch.exp = crate::store::level_start(5).unwrap();
    ch.items[4] = 2599;
    if amulet {
        ch.items[3] = 0xb3e;
    }
    ch.revision = app
        .store
        .exchange(
            vec![crate::store::InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    app.store.progress(ch.clone()).await.unwrap();
    let owner = app.claim(ch.account).unwrap();
    launch(
        &app,
        profile,
        ch.clone(),
        -1,
        None,
        selected.then_some(ch.id),
        owner,
    )
    .await
    .unwrap();
    let character = ch.id;
    let id = app
        .world
        .directory(move |d| {
            Ok(d.worlds
                .values()
                .flat_map(|w| w.players.iter())
                .find(|(_, p)| p.character.id == character)
                .unwrap()
                .0
                .to_owned())
        })
        .await
        .unwrap();
    (dir, app, profile, ch, id)
}
async fn released(app: &App, account: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.owners.lock().unwrap().contains_key(&account) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual autonomous terminal path did not release its retired account");
    assert_eq!(
        app.world
            .directory(move |d| Ok(d
                .worlds
                .values()
                .flat_map(|w| w.players.values())
                .filter(|p| p.character.account == account)
                .count()))
            .await
            .unwrap(),
        0
    );
}
async fn swap(app: &App, id: i32) {
    server::inventory(app, id, move |w| {
        let ch = &w.players[&id].character;
        w.swap(
            id,
            Slot {
                object: id,
                slot: 4,
                kind: ch.items[4],
            },
            Slot {
                object: id,
                slot: 5,
                kind: ch.items[5],
            },
        )
        .map(Some)
    })
    .await
    .unwrap()
    .unwrap();
}
async fn healthy_peer(app: &App) {
    let account = app
        .store
        .list("actual-bot-healthy-sibling".into())
        .await
        .unwrap();
    let ch = app
        .store
        .create(account.id, app.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let expected = ch.clone();
    let owner = app.claim(account.id).unwrap();
    let bind = lifecycle::binding(app, account.id).unwrap();
    let id = app
        .world
        .call(move |w| {
            let id = w.enter(ch, None)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    swap(app, id).await;
    owner
        .close(app, CloseReason::Controller)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        app.store
            .load(expected.account, expected.id)
            .await
            .unwrap()
            .revision,
        expected.revision + 1
    );
}
async fn terminal(goto: bool) {
    let (dir, app, profile, ch, id) = live(true, false).await;
    swap(&app, id).await;
    let mut expected = ch.items.clone();
    expected.swap(4, 5);
    app.world
        .for_player(id)
        .call(move |w| {
            if goto {
                w.correct_position(id, 0, "actual controller expired-ACK regression")?;
            } else {
                w.players[&id].tx.as_ref().unwrap().lost.send_replace(true);
            }
            Ok(())
        })
        .await
        .unwrap();
    if goto {
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
    app.world.clock.set_paused(false);
    released(&app, ch.account).await;
    let saved = app.store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(
        (saved.revision, saved.items.clone()),
        (ch.revision + 1, expected.clone())
    );
    healthy_peer(&app).await;
    app.world.clock.set_paused(true);
    start_selected(
        &app,
        profile,
        saved,
        -1,
        app.world
            .call(|w| {
                let (x, y) = w.map.spawns[0];
                Ok(Pos { x, y })
            })
            .await
            .unwrap(),
        app.claim(ch.account).unwrap(),
    )
    .await
    .unwrap();
    let account = ch.account;
    assert_eq!(
        app.world
            .directory(move |d| Ok(d
                .worlds
                .values()
                .flat_map(|w| w.players.values())
                .filter(|p| p.character.account == account)
                .count()))
            .await
            .unwrap(),
        1
    );
    assert!(app.claim(account).is_err());
    app.shutdown.send_replace(true);
    released(&app, account).await;
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let reopened = crate::store::Store::open(&dir.join("game.db"), &content)
        .unwrap()
        .load(ch.account, ch.id)
        .await
        .unwrap();
    assert_eq!(
        (reopened.revision, reopened.items),
        (ch.revision + 1, expected)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_actual_bot_expired_goto_commit_peer_and_reconnect() {
    terminal(true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_actual_bot_output_loss_commit_peer_and_reconnect() {
    terminal(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_actual_bot_death_replacement_retains_lease_and_new_avatar() {
    let (dir, app, _, ch, old) = live(false, false).await;
    let account = ch.account;
    let fatal = app
        .world
        .for_player(old)
        .call(move |w| {
            let p = w.players.get_mut(&old).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            Ok(w.pending_deaths().pop().unwrap().1)
        })
        .await
        .unwrap();
    let result = app
        .store
        .death(fatal, "actual autonomous permanent death".into())
        .await;
    app.world
        .for_player(old)
        .call(move |w| {
            w.committed(old, result)?;
            w.finish_death(old)
        })
        .await
        .unwrap();
    let (ready, tx) = tokio::sync::oneshot::channel();
    let store = app.store.clone();
    app.store
        .lifecycle_test_job(move |db| {
            let mut ready = Some(ready);
            db.update_hook(Some(
                move |action: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
                    if table == "characters" && action == rusqlite::hooks::Action::SQLITE_INSERT {
                        ready
                            .take()
                            .unwrap()
                            .send(store.lifecycle_test_pause_reply())
                            .unwrap();
                    }
                },
            ));
            Ok(())
        })
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let (gate, release) = tokio::time::timeout(Duration::from_secs(8), tx)
        .await
        .unwrap()
        .unwrap();
    let release = Release(Some(release));
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    assert!(
        app.claim(account).is_err(),
        "replacement commit must retain exclusive lease"
    );
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let replacement: i32 = db
        .query_row(
            "SELECT id FROM characters WHERE account=?1 AND alive=1",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(replacement, ch.id);
    assert_eq!(
        app.world
            .directory(move |d| Ok(d
                .worlds
                .values()
                .flat_map(|w| w.players.values())
                .filter(|p| p.character.account == account)
                .count()))
            .await
            .unwrap(),
        0
    );
    drop(release);
    app.store
        .lifecycle_test_job(|db| {
            db.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
            Ok(())
        })
        .await
        .unwrap();
    let new = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = app
                .world
                .directory(move |d| {
                    Ok(d.worlds
                        .values()
                        .flat_map(|w| w.players.keys())
                        .find_map(|id| {
                            d.player_view(*id)
                                .ok()
                                .filter(|v| v.session.account == account)
                        }))
                })
                .await
                .unwrap();
            if let Some(view) = view {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    app.world.clock.set_paused(true);
    assert_ne!(new.session.player, old);
    assert_eq!(new.session.character, replacement);
    assert_eq!(new.character.class, crate::content::PRIEST);
    assert_eq!(
        new.character.items,
        app.content.get(crate::content::PRIEST).unwrap().equipment
    );
    assert!(lifecycle::finalize(&app, old, CloseReason::Controller).is_err());
    assert!(app.claim(account).is_err());
    assert!(app.store.load(account, ch.id).await.is_err());
    app.shutdown.send_replace(true);
    released(&app, account).await;
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1 AND alive=1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT alive FROM characters WHERE id=?1", [ch.id], |r| r
            .get::<_, i32>(
            0
        ))
        .unwrap(),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}
struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_actual_bot_terminal_sql_failure_stays_fenced() {
    let (dir, app, _, ch, id) = live(true, false).await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER bot_final_save BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'actual bot final-save failure'); END;",ch.account)).unwrap();
    app.world
        .for_player(id)
        .call(move |w| {
            w.players[&id].tx.as_ref().unwrap().lost.send_replace(true);
            Ok(())
        })
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let account = ch.account;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stopped = app
                .world
                .directory(move |d| {
                    let view = d.player_view(id)?;
                    Ok(view.lifecycle.departing && view.lifecycle.quarantined)
                })
                .await
                .unwrap();
            if stopped {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(app.claim(account).is_err());
    assert!(app.bots.lock().unwrap().is_empty());
    assert!(
        app.quarantined.lock().unwrap().is_empty(),
        "no second quarantine-Owner registry"
    );
    let saved = app.store.load(account, ch.id).await.unwrap();
    assert_eq!((saved.items, saved.revision), (ch.items, ch.revision));
    healthy_peer(&app).await;
    app.shutdown.send_replace(true);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_actual_bot_automatic_resurrection_transfer_retires_before_reentry() {
    let (dir, app, _, ch, old) = live(true, true).await;
    let account = ch.account;
    app.world
        .for_player(old)
        .call(move |w| {
            let p = w.players.get_mut(&old).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            Ok(())
        })
        .await
        .unwrap();
    server::persistence_tick(&app).await.unwrap();
    let saved = app.store.load(account, ch.id).await.unwrap();
    assert!(saved.alive);
    assert_eq!((saved.items[3], saved.revision), (-1, ch.revision + 1));
    let (gate, release) = app.world.lifecycle_test_pause_job(4, true);
    let release = Release(Some(release));
    app.world.clock.set_paused(false);
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    assert!(
        app.claim(account).is_err(),
        "automatic reentry must retain the bot's account lease after old-avatar retirement"
    );
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    drop(release);
    let new = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = app
                .world
                .directory(move |d| {
                    Ok(d.worlds
                        .values()
                        .flat_map(|w| w.players.keys())
                        .find_map(|id| {
                            d.player_view(*id)
                                .ok()
                                .filter(|v| v.session.account == account && v.session.player != old)
                        }))
                })
                .await
                .unwrap();
            if let Some(view) = view {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    app.world.clock.set_paused(true);
    assert_eq!(
        (
            new.session.world,
            new.session.account,
            new.session.character
        ),
        (-2, account, ch.id)
    );
    assert_ne!(new.session.player, old);
    assert_eq!(
        (new.character.items, new.character.inventory_revision),
        (saved.items.clone(), saved.revision)
    );
    assert!(lifecycle::finalize(&app, old, CloseReason::Controller).is_err());
    assert!(app.claim(account).is_err());
    app.shutdown.send_replace(true);
    released(&app, account).await;
    let reopened = crate::store::Store::open(&dir.join("game.db"), &app.content)
        .unwrap()
        .load(account, ch.id)
        .await
        .unwrap();
    assert_eq!(
        (reopened.items, reopened.revision),
        (saved.items, saved.revision)
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1 AND alive=1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}
async fn remembrance(fault: &str) {
    let (dir, app) = crate::server::lifecycle_test_fixture();
    app.world.clock.set_paused(true);
    let profile = PROFILES[0];
    let ch = app
        .store
        .shore_character(
            profile.identity.into(),
            profile.name.into(),
            &app.content,
            profile.healer,
        )
        .await
        .unwrap();
    let account = ch.account;
    let peer = app
        .store
        .list("remembered-clear-peer".into())
        .await
        .unwrap()
        .id;
    let mut owner = app.claim(account).unwrap();
    owner.arm_cleanup(&app, tokio::runtime::Handle::current());
    let (id, _output, _sender, _brain) = enter(&app, profile, ch.clone(), -1, None).await.unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TABLE remembrance_writes(n INTEGER); CREATE TRIGGER remembrance_audit AFTER INSERT ON account_lists WHEN NEW.account={account} BEGIN INSERT INTO remembrance_writes VALUES(1); END;")).unwrap();
    if fault == "sql" {
        db.execute_batch(&format!("CREATE TRIGGER remembrance_failure BEFORE INSERT ON account_lists WHEN NEW.account={account} BEGIN SELECT RAISE(ABORT,'remembrance SQL failure'); END;")).unwrap();
    }
    let (gate, release) = app.store.lifecycle_test_pause_reply();
    let release = Release(Some(release));
    if fault == "lost" {
        app.store.lifecycle_test_lose_reply();
    }
    let a = app.clone();
    let waiter = tokio::spawn(async move {
        let _owner = owner;
        remember_clear_peer(&a, id, account, peer).await
    });
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    let expected = usize::from(fault != "sql");
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM account_lists WHERE account=?1 AND list_id=0 AND target=?2",
            [account, peer],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        expected,
        "barrier must follow actual SQLite outcome"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM remembrance_writes", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        expected
    );
    assert!(
        app.world
            .for_player(id)
            .call(move |w| Ok(w.players[&id].lifecycle_test_account_lists()[0].is_empty()))
            .await
            .unwrap()
    );
    let final_save = if fault == "none" {
        let (gate, release) = app.store.lifecycle_test_pause_reply();
        Some((gate, Release(Some(release))))
    } else {
        None
    };
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    let completion = lifecycle::finalize(&app, id, CloseReason::Shutdown).unwrap();
    assert!(
        app.claim(account).is_err(),
        "accepted remembrance must keep admission through terminal close"
    );
    drop(release);
    if let Some((gate, release)) = final_save {
        tokio::time::timeout(Duration::from_secs(5), gate)
            .await
            .unwrap()
            .unwrap();
        let lists = app
            .world
            .for_player(id)
            .call(move |w| Ok(w.players[&id].lifecycle_test_account_lists()))
            .await
            .unwrap();
        assert_eq!(
            lists[0],
            std::collections::BTreeSet::from([peer]),
            "cancelled accepted remembrance did not publish before final save"
        );
        assert!(app.claim(account).is_err());
        drop(release);
        completion.wait().await.unwrap();
        released(&app, account).await;
    } else {
        let error = completion
            .wait()
            .await
            .expect_err("cancelled Store uncertainty was hidden by a successful final save");
        let error = format!("{error:#}");
        assert!(
            error.contains(if fault == "sql" {
                "remembrance SQL failure"
            } else {
                "account worker failed"
            }),
            "{error}"
        );
        assert!(app.claim(account).is_err());
        app.world
            .for_player(id)
            .call(move |w| {
                let p = &w.players[&id];
                assert!(p.departing && !p.pending && p.tx.is_none());
                assert!(
                    p.lifecycle_test_account_lists()[0].is_empty(),
                    "uncertain list must not fabricate World publication"
                );
                Ok(())
            })
            .await
            .unwrap();
    }
    healthy_peer(&app).await;
    let reopened = crate::store::Store::open(&dir.join("game.db"), &app.content).unwrap();
    let lists = reopened.account_lists(account).await.unwrap();
    assert_eq!(
        lists[0],
        if fault == "sql" {
            std::collections::BTreeSet::new()
        } else {
            std::collections::BTreeSet::from([peer])
        }
    );
    assert!(lists[1].is_empty());
    let saved = reopened.load(account, ch.id).await.unwrap();
    assert_eq!((saved.items, saved.revision), (ch.items, ch.revision));
    assert_eq!(
        db.query_row("SELECT count(*) FROM remembrance_writes", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        expected,
        "no mutation replay"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_bot_remembrance_committed_cancelled_controller_publishes_before_release()
 {
    remembrance("none").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_bot_remembrance_sql_failure_cancelled_controller_stays_fenced() {
    remembrance("sql").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_bot_remembrance_committed_lost_reply_cancelled_controller_stays_fenced()
{
    remembrance("lost").await;
}
