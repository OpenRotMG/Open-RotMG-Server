use super::*;
use crate::{server::Owner, store::Character, world::Slot};

struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}
impl Release {
    fn now(&mut self) {
        if let Some(tx) = self.0.take() {
            tx.send(()).unwrap();
        }
    }
}

async fn fixture(dead: bool) -> (std::path::PathBuf, App, Owner, Profile, Character, i32) {
    let (dir, app) = crate::server::lifecycle_test_fixture();
    app.world.clock.set_paused(true);
    let profile = *PROFILES.iter().find(|p| p.healer).unwrap();
    let mut ch = app
        .store
        .shore_character(
            profile.identity.into(),
            profile.name.into(),
            &app.content,
            profile.healer,
        )
        .await
        .unwrap();
    ch.level = 5;
    ch.exp = crate::store::level_start(5).unwrap();
    app.store.progress(ch.clone()).await.unwrap();
    let owner = app.claim(ch.account).unwrap();
    let bind = server::lifecycle::binding(&app, ch.account).unwrap();
    let presentation = app.store.presentation(ch.account).await.unwrap();
    let entering = ch.clone();
    let id = app
        .world
        .call(move |w| {
            let id = w.enter_at(entering, None, None, presentation)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    if dead {
        let fatal = app
            .world
            .for_player(id)
            .call(move |w| {
                let p = w.players.get_mut(&id).unwrap();
                p.character.hp = 0;
                p.death_pending = true;
                Ok(w.pending_deaths().pop().unwrap().1)
            })
            .await
            .unwrap();
        let result = app
            .store
            .death(fatal, "owned replacement fixture".into())
            .await;
        app.world
            .for_player(id)
            .call(move |w| {
                w.committed(id, result)?;
                w.finish_death(id)
            })
            .await
            .unwrap();
    }
    server::depart(&app, id).await.unwrap();
    (dir, app, owner, profile, ch, id)
}

async fn cancelled_reentry(kind: &str) {
    let (dir, app, owner, profile, old, old_player) = fixture(kind == "replacement").await;
    let account = old.account;
    let request = if kind == "replacement" {
        Reentry::Replacement { previous: old.id }
    } else {
        let (time, key, _) = app
            .world
            .directory(move |d| d.issue(account, old.id, -2))
            .await
            .unwrap();
        if kind == "collective" {
            Reentry::Collective {
                destination: -2,
                time,
                key,
                character: old.id,
            }
        } else {
            Reentry::Portal {
                destination: -2,
                time,
                key,
                session: crate::world::view::Session {
                    world: -1,
                    player: old_player,
                    account,
                    character: old.id,
                },
                selected: (kind == "selected").then_some(old.id),
            }
        }
    };
    let (ready, mut release) = if kind == "replacement" {
        let (gate_tx, gate_rx) = tokio::sync::oneshot::channel();
        let store = app.store.clone();
        app.store
            .lifecycle_test_job(move |db| {
                let mut gate_tx = Some(gate_tx);
                db.update_hook(Some(
                    move |action: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
                        if table == "characters" && action == rusqlite::hooks::Action::SQLITE_INSERT
                        {
                            gate_tx
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
        (Some(gate_rx), None)
    } else {
        (None, None)
    };
    let world_gate = if kind != "replacement" {
        Some(app.world.lifecycle_test_pause_job(3, true))
    } else {
        None
    };
    let running = app.clone();
    let waiter = tokio::spawn(async move {
        let mut owner = Some(owner);
        reenter(&running, profile, account, &mut owner, request).await
    });
    let (gate, sender) = if let Some(ready) = ready {
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap()
    } else {
        world_gate.unwrap()
    };
    release.replace(Release(Some(sender)));
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let live: Vec<(i32, u16)> = db
        .prepare(
            "SELECT id,json_extract(stats,'$.class') FROM characters WHERE account=?1 AND alive=1",
        )
        .unwrap()
        .query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        live.len(),
        1,
        "checkpoint must follow the actual replacement commit"
    );
    if kind == "replacement" {
        assert_ne!(live[0].0, old.id);
        assert_eq!(live[0].1, crate::content::PRIEST);
    } else {
        assert_eq!(live[0].0, old.id);
    }
    waiter.abort();
    assert!(waiter.await.err().unwrap().is_cancelled());
    let held = app.owners.lock().unwrap().contains_key(&account);
    release.as_mut().unwrap().now();
    if kind == "replacement" {
        app.store
            .lifecycle_test_job(|db| {
                db.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
                Ok(())
            })
            .await
            .unwrap();
    }
    assert!(
        held,
        "accepted reentry lost account custody before publication"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while app.owners.lock().unwrap().contains_key(&account) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled reentry left an orphan avatar/account instead of canonical retirement");
    let players = app
        .world
        .directory(move |d| {
            Ok(d.worlds
                .values()
                .flat_map(|w| w.players.values())
                .filter(|p| p.character.account == account)
                .count())
        })
        .await
        .unwrap();
    assert_eq!(
        players, 0,
        "cancelled controller must retire its published avatar"
    );
    let saved = app.store.load(account, live[0].0).await.unwrap();
    assert_eq!(saved.class, live[0].1);
    if kind == "replacement" {
        assert!(app.store.load(account, old.id).await.is_err());
        assert_eq!(
            saved.items,
            app.content.get(crate::content::PRIEST).unwrap().equipment
        );
    } else {
        assert_eq!(
            (saved.items, saved.revision),
            (old.items.clone(), old.revision)
        );
    }
    let fresh = app.claim(account).unwrap();
    assert_eq!(app.owners.lock().unwrap().len(), 1);
    drop(fresh);
    assert!(app.owners.lock().unwrap().is_empty());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1 AND alive=1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(app.store.lifecycle_test_jobs(), 0);
    // A separate account still uses the normal inventory transaction/publication path.
    let sibling = app.store.list(format!("healthy-{kind}")).await.unwrap();
    let ch = app
        .store
        .create(sibling.id, app.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let sibling_owner = app.claim(sibling.id).unwrap();
    let bind = server::lifecycle::binding(&app, sibling.id).unwrap();
    let presentation = app.store.presentation(sibling.id).await.unwrap();
    let expected = ch.clone();
    let sibling = app
        .world
        .call(move |w| {
            let id = w.enter_at(ch, None, None, presentation)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    server::inventory(&app, sibling, move |w| {
        w.swap(
            sibling,
            Slot {
                object: sibling,
                slot: 4,
                kind: expected.items[4],
            },
            Slot {
                object: sibling,
                slot: 5,
                kind: expected.items[5],
            },
        )
        .map(Some)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        app.store
            .load(expected.account, expected.id)
            .await
            .unwrap()
            .revision,
        expected.revision + 1
    );
    server::depart(&app, sibling).await.unwrap();
    drop(sibling_owner);
    assert!(app.owners.lock().unwrap().is_empty());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_reentry_collective_queued_world_survives_controller_drop() {
    cancelled_reentry("collective").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_reentry_portal_queued_world_survives_controller_drop() {
    cancelled_reentry("portal").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_reentry_selected_queued_world_survives_controller_drop() {
    cancelled_reentry("selected").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_reentry_replacement_commit_survives_controller_drop() {
    cancelled_reentry("replacement").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_bot_portal_and_collective_reentry_retain_owner_and_replace_avatar() {
    for collective in [false, true] {
        let (dir, app, owner, profile, ch, old) = fixture(false).await;
        let mut owner = Some(owner);
        let account = ch.account;
        let character = ch.id;
        let (time, key, _) = app
            .world
            .directory(move |d| d.issue(account, character, -2))
            .await
            .unwrap();
        let request = if collective {
            Reentry::Collective {
                destination: -2,
                time,
                key,
                character,
            }
        } else {
            Reentry::Portal {
                destination: -2,
                time,
                key,
                session: crate::world::view::Session {
                    world: -1,
                    player: old,
                    account,
                    character,
                },
                selected: None,
            }
        };
        let (new, _output, _sender, brain) = reenter(&app, profile, account, &mut owner, request)
            .await
            .unwrap();
        assert_ne!(new, old);
        assert_eq!(brain.session.1, new);
        assert_eq!(brain.session.2, character);
        assert!(
            app.claim(account).is_err(),
            "bot transfer must retain the account lease"
        );
        assert!(
            server::lifecycle::finalize(&app, old, server::lifecycle::CloseReason::Controller)
                .is_err(),
            "old completion cannot retire transferred avatar"
        );
        let view = app
            .world
            .directory(move |d| d.player_view(new))
            .await
            .unwrap();
        assert_eq!(
            (
                view.session.world,
                view.session.account,
                view.session.character
            ),
            (-2, account, character)
        );
        assert_eq!(
            (view.character.items, view.character.inventory_revision),
            (ch.items.clone(), ch.revision)
        );
        close_controller(
            &app,
            owner.take().unwrap(),
            None,
            server::lifecycle::CloseReason::Controller,
        )
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
        assert!(app.owners.lock().unwrap().is_empty());
        assert!(
            app.world
                .directory(move |d| d.player_view(new))
                .await
                .is_err()
        );
        let saved = app.store.load(account, character).await.unwrap();
        assert_eq!((saved.items, saved.revision), (ch.items, ch.revision));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_selected_death_during_portal_reentry_rejects_without_replacement_or_fence()
 {
    let (dir, app, owner, profile, ch, old) = fixture(true).await;
    let mut owner = Some(owner);
    let account = ch.account;
    let character = ch.id;
    let (time, key, _) = app
        .world
        .directory(move |d| d.issue(account, character, -2))
        .await
        .unwrap();
    let error = reenter(
        &app,
        profile,
        account,
        &mut owner,
        Reentry::Portal {
            destination: -2,
            time,
            key,
            session: crate::world::view::Session {
                world: -1,
                player: old,
                account,
                character,
            },
            selected: Some(character),
        },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(
        error.downcast_ref::<server::lifecycle::CloseReason>(),
        Some(&server::lifecycle::CloseReason::Controller)
    );
    close_controller(
        &app,
        owner.take().unwrap(),
        Some(&error),
        server::lifecycle::CloseReason::Controller,
    )
    .unwrap()
    .unwrap()
    .wait()
    .await
    .unwrap();
    assert!(app.owners.lock().unwrap().is_empty());
    assert!(app.store.load(account, character).await.is_err());
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1 AND alive=1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_freeze_rejects_actual_autonomous_death_replacement_before_store_or_controller_entry()
 {
    let (dir, app, owner, profile, old, _) = fixture(true).await;
    let account = old.account;
    let before = app.store.list(profile.identity.into()).await.unwrap();
    let mut owner = Some(owner);
    let draining = app.clone();
    let joined = tokio::spawn(async move { server::lifecycle::drain::terminate(&draining).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while server::lifecycle::drain::open(&app) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let error = reenter(
        &app,
        profile,
        account,
        &mut owner,
        Reentry::Replacement { previous: old.id },
    )
    .await
    .err()
    .unwrap();
    assert_eq!(
        error.downcast_ref::<CloseReason>(),
        Some(&CloseReason::Controller)
    );
    assert!(owner.is_some(), "rejected replacement lost custody");
    assert_eq!(
        server::lifecycle::state(&app, account),
        Some((false, None, false))
    );
    assert_eq!(
        app.store
            .list(profile.identity.into())
            .await
            .unwrap()
            .characters
            .len(),
        before.characters.len()
    );
    drop(owner);
    tokio::time::timeout(Duration::from_secs(2), joined)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(app.owners.lock().unwrap().is_empty());
    assert_eq!(
        app.world
            .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
            .await
            .unwrap(),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}
