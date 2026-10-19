use super::*;

async fn direct_spec(app: &App) -> Spawn {
    let pos = app
        .world
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    serde_json::from_value(spec(pos)).unwrap()
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
async fn lifecycle_finalize_qa_spawn_cancel_after_create_keeps_owned_publication_and_leave() {
    let (dir, app) = fixture();
    let spec = direct_spec(&app).await;
    let (ready, wait) = tokio::sync::oneshot::channel();
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
    let a = app.clone();
    let task = tokio::spawn(async move { spawn(&a, a.testing.as_ref().unwrap(), spec).await });
    let (gate, release) = tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .unwrap()
        .unwrap();
    let release = Release(Some(release));
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let (actor, account): (i32, i32) = db
        .query_row("SELECT id,account FROM characters", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(app.claim(account).is_err());
    drop(release);
    app.store
        .lifecycle_test_job(|db| {
            db.update_hook(None::<fn(rusqlite::hooks::Action, &str, &str, i64)>);
            Ok(())
        })
        .await
        .unwrap();
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(id) = app.bots.lock().unwrap().keys().next().copied() {
                break id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let observed = snapshot(&app, actor, false).await.unwrap();
    assert_eq!(observed["controller"], "direct");
    assert_eq!(observed["character"]["level"], 8);
    assert_eq!(observed["character"]["items"][4], 2599);
    let saved = app.store.load(account, actor).await.unwrap();
    assert_eq!((saved.items[4], saved.revision), (2599, 1));
    close_script(&app, id)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert!(app.owners.lock().unwrap().is_empty());
    assert!(resolve(&app, actor).await.is_err());
    let reopened = Store::open(&dir.join("game.db"), &app.content)
        .unwrap()
        .load(account, actor)
        .await
        .unwrap();
    assert_eq!(
        (reopened.items, reopened.revision),
        (saved.items, saved.revision)
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM characters", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_qa_lost_entry_publication_reply_is_stopped_and_fenced() {
    let (dir, app) = fixture();
    let spec = direct_spec(&app).await;
    let (gate, release) = app.world.lifecycle_test_pause_job(2, false);
    let release = Release(Some(release));
    let a = app.clone();
    let task = tokio::spawn(async move { spawn(&a, a.testing.as_ref().unwrap(), spec).await });
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let (actor, account): (i32, i32) = db
        .query_row("SELECT id,account FROM characters", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    let state = lifecycle::state(&app, account).unwrap();
    assert!(state.0);
    let id = state.1.unwrap();
    assert!(!state.2);
    app.world.lifecycle_test_lose_reply();
    drop(release);
    let error = task.await.unwrap().unwrap_err();
    assert!(format!("{error:#}").contains("world directory worker failed"));
    let completion = lifecycle::finalize(&app, id, CloseReason::Controller)
        .unwrap()
        .wait()
        .await
        .unwrap_err();
    assert!(format!("{completion:#}").contains("world directory worker failed"));
    assert_eq!(
        lifecycle::state(&app, account),
        Some((false, Some(id), true))
    );
    assert!(app.claim(account).is_err());
    assert!(app.bots.lock().unwrap().is_empty());
    app.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.departing);
            assert!(!p.pending);
            assert!(p.tx.is_none());
            Ok(())
        })
        .await
        .unwrap();
    let saved = app.store.load(account, actor).await.unwrap();
    assert_eq!((saved.items[4], saved.revision), (2599, 1));
    assert_eq!(
        db.query_row("SELECT count(*) FROM characters", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    let healthy = app
        .store
        .list("qa-after-fenced-entry".into())
        .await
        .unwrap();
    let claim = app.claim(healthy.id).unwrap();
    let character = app
        .store
        .create(healthy.id, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(app.store.load(healthy.id, character.id).await.is_ok());
    drop(claim);
    fs::remove_dir_all(dir).unwrap();
}

async fn script_leave(app: App, id: i32) -> (axum::http::StatusCode, String) {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer test-token".parse().unwrap());
    let response = super::super::bot_leave(
        State(app),
        headers,
        Json(super::super::BotPlayer { player: id }),
    )
    .await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn duplicate_script_leave(fault: &str) {
    let (dir, app) = fixture();
    let spec = direct_spec(&app).await;
    let created = spawn(&app, app.testing.as_ref().unwrap(), spec)
        .await
        .unwrap();
    let actor = created["actor"].as_i64().unwrap() as i32;
    let account = created["account"].as_i64().unwrap() as i32;
    let id = resolve(&app, actor).await.unwrap();
    let before = app.store.load(account, actor).await.unwrap();
    let expected = app
        .world
        .for_player(id)
        .call(move |w| Ok(w.players[&id].character.clone()))
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE qa_saves(account INTEGER); CREATE TRIGGER qa_save AFTER UPDATE ON characters BEGIN INSERT INTO qa_saves VALUES(NEW.account); END;").unwrap();
    if fault == "sql" {
        db.execute_batch(&format!("CREATE TRIGGER qa_fail BEFORE UPDATE ON characters WHEN OLD.account={account} BEGIN SELECT RAISE(ABORT,'script departure SQL failure'); END;")).unwrap();
    }
    let (gate, release) = app.store.lifecycle_test_pause_reply();
    let release = Release(Some(release));
    let leave = tokio::spawn(script_leave(app.clone(), id));
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    assert!(app.bots.lock().unwrap().is_empty());
    assert!(app.claim(account).is_err());
    let writes = || {
        db.query_row(
            "SELECT count(*) FROM qa_saves WHERE account=?1",
            [account],
            |r| r.get::<_, i32>(0),
        )
        .unwrap()
    };
    assert_eq!(writes(), if fault == "sql" { 0 } else { 1 });
    leave.abort();
    assert!(leave.await.unwrap_err().is_cancelled());
    let mut duplicate = tokio::spawn(script_leave(app.clone(), id));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut duplicate)
            .await
            .is_err(),
        "actual duplicate bot_leave returned before canonical Store publication"
    );
    // Shutdown snapshots capabilities, not ids that could disappear before the later join.
    let mut shutdown = super::super::close_scripts(&app).unwrap();
    assert_eq!(shutdown.len(), 1);
    if fault == "publication" {
        app.world.lifecycle_test_lose_reply();
    }
    drop(release);
    let (status, body) = duplicate.await.unwrap();
    if fault.is_empty() {
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"saved":true})
        );
        assert!(app.owners.lock().unwrap().is_empty());
        // The actual leave has completed and released its claim before shutdown joins its snapshot.
        shutdown.pop().unwrap().wait().await.unwrap();
        assert!(resolve(&app, actor).await.is_err());
        let replacement = app.claim(account).unwrap();
        assert_eq!(
            script_leave(app.clone(), id).await.0,
            axum::http::StatusCode::BAD_REQUEST
        );
        assert!(app.claim(account).is_err());
        drop(replacement);
        assert!(super::super::close_scripts(&app).unwrap().is_empty());
    } else {
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        let original = shutdown
            .pop()
            .unwrap()
            .wait()
            .await
            .unwrap_err()
            .to_string();
        assert!(
            original.contains(if fault == "sql" {
                "script departure SQL failure"
            } else {
                "worker failed"
            }),
            "{original}"
        );
        assert_eq!(
            body,
            format!("<Error>{}</Error>", super::super::xml_escape(&original))
        );
        assert_eq!(script_leave(app.clone(), id).await, (status, body));
        assert!(app.claim(account).is_err());
        assert_eq!(
            lifecycle::state(&app, account),
            Some((false, Some(id), true))
        );
        let present = resolve(&app, actor).await.is_ok();
        assert_eq!(present, fault == "sql");
        if present {
            app.world
                .for_player(id)
                .call(move |w| {
                    let p = &w.players[&id];
                    assert!(p.departing && p.pending && p.tx.is_none());
                    assert!(p.bot_goal.is_none() && p.bot_aim.is_none());
                    assert!(
                        w.begin_departure(id)
                            .unwrap_err()
                            .to_string()
                            .contains("script departure SQL failure")
                    );
                    Ok(())
                })
                .await
                .unwrap();
        }
    }
    assert_eq!(writes(), if fault == "sql" { 0 } else { 1 });
    let saved = Store::open(&dir.join("game.db"), &app.content)
        .unwrap()
        .load(account, actor)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(if fault == "sql" { &before } else { &expected }).unwrap()
    );
    assert_eq!((saved.items[4], saved.revision), (2599, 1));
    let mut peer = direct_spec(&app).await;
    peer.guid = "script-close-healthy-sibling".into();
    let peer = spawn(&app, app.testing.as_ref().unwrap(), peer)
        .await
        .unwrap();
    let peer_actor = peer["actor"].as_i64().unwrap() as i32;
    let peer_account = peer["account"].as_i64().unwrap() as i32;
    let peer_id = resolve(&app, peer_actor).await.unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer test-token".parse().unwrap());
    let action = super::super::bot_action(
        State(app.clone()),
        headers,
        Json(super::super::BotCommand {
            player: peer_id,
            action: Action::Swap {
                from: crate::world::Slot {
                    object: peer_id,
                    slot: 4,
                    kind: 2599,
                },
                to: crate::world::Slot {
                    object: peer_id,
                    slot: 5,
                    kind: 2594,
                },
            },
        }),
    )
    .await;
    assert_eq!(action.status(), axum::http::StatusCode::OK);
    assert_eq!(
        script_leave(app.clone(), peer_id).await.0,
        axum::http::StatusCode::OK
    );
    let peer_saved = Store::open(&dir.join("game.db"), &app.content)
        .unwrap()
        .load(peer_account, peer_actor)
        .await
        .unwrap();
    assert_eq!(
        (
            peer_saved.items[4],
            peer_saved.items[5],
            peer_saved.revision
        ),
        (2594, 2599, 2)
    );
    let claim = app.claim(peer_account).unwrap();
    drop(claim);
    assert_eq!(writes(), if fault == "sql" { 0 } else { 1 });
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_qa_leave_cancelled_waiter_duplicate_close_one_write() {
    duplicate_script_leave("").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_qa_duplicate_leave_retains_original_sql_failure() {
    duplicate_script_leave("sql").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_qa_duplicate_leave_retains_original_publication_failure() {
    duplicate_script_leave("publication").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_script_shutdown_snapshot_precedes_actual_leave_and_is_bounded() {
    let (dir, app) = fixture();
    let spec = direct_spec(&app).await;
    let created = spawn(&app, app.testing.as_ref().unwrap(), spec)
        .await
        .unwrap();
    let account = created["account"].as_i64().unwrap() as i32;
    let actor = created["actor"].as_i64().unwrap() as i32;
    let mut id = resolve(&app, actor).await.unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE qa_saves(n INTEGER); CREATE TRIGGER qa_save AFTER UPDATE ON characters BEGIN INSERT INTO qa_saves VALUES(1); END;").unwrap();
    for round in 0..8 {
        let (gate, release) = app.store.lifecycle_test_pause_reply();
        let release = Release(Some(release));
        let mut shutdown = super::super::close_scripts(&app).unwrap();
        assert_eq!(shutdown.len(), 1);
        tokio::time::timeout(Duration::from_secs(5), gate)
            .await
            .unwrap()
            .unwrap();
        let mut leave = tokio::spawn(script_leave(app.clone(), id));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut leave)
                .await
                .is_err()
        );
        assert!(app.claim(account).is_err());
        drop(release);
        assert_eq!(leave.await.unwrap().0, axum::http::StatusCode::OK);
        assert!(app.owners.lock().unwrap().is_empty());
        shutdown.pop().unwrap().wait().await.unwrap();
        assert!(super::super::close_scripts(&app).unwrap().is_empty());
        assert!(app.bots.lock().unwrap().is_empty());
        assert!(resolve(&app, actor).await.is_err());
        assert_eq!(
            script_leave(app.clone(), id).await.0,
            axum::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM qa_saves", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            round + 1
        );
        if round < 7 {
            let old = id;
            let mut headers = HeaderMap::new();
            headers.insert("authorization", "Bearer test-token".parse().unwrap());
            let response = super::super::bot_create(
                State(app.clone()),
                headers,
                Json(super::super::BotCreate {
                    guid: "qa-interface-unit".into(),
                    world: -1,
                }),
            )
            .await;
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let value: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 1_048_576)
                    .await
                    .unwrap(),
            )
            .unwrap();
            id = value["player"].as_i64().unwrap() as i32;
            assert_ne!(id, old);
            assert_eq!(
                script_leave(app.clone(), old).await.0,
                axum::http::StatusCode::BAD_REQUEST
            );
            assert_eq!(resolve(&app, actor).await.unwrap(), id);
            assert!(app.claim(account).is_err());
        }
    }
    let weak = Arc::downgrade(&app.bots);
    drop(app);
    tokio::time::timeout(Duration::from_secs(5), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_script_leave_rejects_unknown_native_and_autonomous_identity() {
    let (dir, app) = fixture();
    assert_eq!(
        script_leave(app.clone(), i32::MAX).await.0,
        axum::http::StatusCode::BAD_REQUEST
    );
    assert!(app.owners.lock().unwrap().is_empty());
    let account = app.store.list("non-script-native".into()).await.unwrap().id;
    let ch = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let owner = app.claim_session(account).unwrap();
    let bind = lifecycle::binding(&app, account).unwrap();
    let (packets, _receive) = tokio::sync::mpsc::channel(128);
    let (lost, _) = tokio::sync::watch::channel(false);
    let id = app
        .world
        .call(move |w| {
            let id = w.enter(ch, Some(crate::world::Output { packets, lost }))?;
            assert_eq!(w.players[&id].controller, crate::world::Controller::Human);
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    assert_eq!(
        script_leave(app.clone(), id).await.0,
        axum::http::StatusCode::BAD_REQUEST
    );
    assert!(super::super::close_scripts(&app).unwrap().is_empty());
    let (gate, release) = app.store.lifecycle_test_pause_reply();
    let release = Release(Some(release));
    let close = owner.close(&app, CloseReason::Transport).unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(5), gate)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        script_leave(app.clone(), id).await.0,
        axum::http::StatusCode::BAD_REQUEST
    );
    assert!(super::super::close_scripts(&app).unwrap().is_empty());
    drop(release);
    close.wait().await.unwrap();
    let pos = app
        .world
        .in_world(-2)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let mut spec = spec(pos);
    spec["guid"] = "non-script-autonomous".into();
    spec["controller"] = "game_bot".into();
    spec["profile"] = "Pixl".into();
    spec["world"] = (-2).into();
    let created = spawn(
        &app,
        app.testing.as_ref().unwrap(),
        serde_json::from_value(spec).unwrap(),
    )
    .await
    .unwrap();
    let actor = created["actor"].as_i64().unwrap() as i32;
    let account = created["account"].as_i64().unwrap() as i32;
    let id = resolve(&app, actor).await.unwrap();
    assert_eq!(
        script_leave(app.clone(), id).await.0,
        axum::http::StatusCode::BAD_REQUEST
    );
    assert!(super::super::close_scripts(&app).unwrap().is_empty());
    assert_eq!(resolve(&app, actor).await.unwrap(), id);
    assert!(app.claim(account).is_err());
    app.shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !app.owners.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn lifecycle_finalize_qa_map_owner_has_no_app_retention_cycle() {
    let (dir, app) = fixture();
    let spec = direct_spec(&app).await;
    spawn(&app, app.testing.as_ref().unwrap(), spec)
        .await
        .unwrap();
    assert_eq!(app.bots.lock().unwrap().len(), 1);
    let weak = Arc::downgrade(&app.bots);
    drop(app);
    assert!(
        weak.upgrade().is_none(),
        "map-held Owner retained App->Owner->App cycle"
    );
    fs::remove_dir_all(dir).unwrap();
}
