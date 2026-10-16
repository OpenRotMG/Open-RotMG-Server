use super::*;

fn fixture() -> (PathBuf, App) {
    let (dir, mut app) = crate::server::account_actions::tests::fixture();
    write_json(
        &dir.join("testing.json"),
        &json!({"schema":1,"provenance":"prepared"}),
    )
    .unwrap();
    app.testing = Testing::open(&dir).unwrap();
    app.control = Some("test-token".into());
    app.world.clock.set_paused(true);
    (dir, app)
}
async fn request(app: &App, id: &str, command: Value) -> Value {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer test-token".parse().unwrap());
    let response = super::command(
        State(app.clone()),
        headers,
        Json(json!({"id":id,"command":command})),
    )
    .await;
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap(),
    )
    .unwrap()
}
fn spec(pos: Pos) -> Value {
    json!({"guid":"qa-interface-unit","class":WIZARD,"level":8,"seed":12351,"equipment":[2712,-1,-1,-1],"backpack":[2599,2594,-1,-1,-1,-1,-1,-1],"world":-1,"position":pos,"controller":"direct","stats":null})
}
#[tokio::test]
async fn oversized_requests_are_rejected_before_acceptance() {
    let (dir, app) = fixture();
    let result = request(
        &app,
        "oversized",
        json!({"op":"capabilities","padding":"x".repeat(MAX_REQUEST_BYTES)}),
    )
    .await;
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("8192-byte limit")
    );
    assert_eq!(
        fs::read_dir(&app.testing.as_ref().unwrap().root)
            .unwrap()
            .count(),
        0
    );
    assert!(app.world.clock.paused());
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn mutation_budget_preserves_replay_and_does_not_resume() {
    let (dir, app) = fixture();
    let original = request(&app, "retained", json!({"op":"pause","native_paused":[]})).await;
    let testing = app.testing.as_ref().unwrap();
    for n in 1..MAX_MUTATIONS {
        fs::write(testing.path(&format!("budget-{n}"), "receipt"), b"").unwrap();
    }
    app.world.clock.set_paused(false);
    let denied = request(&app, "over-budget", json!({"op":"resume"})).await;
    assert!(
        denied["error"]
            .as_str()
            .unwrap()
            .contains("mutation budget")
    );
    assert!(app.world.clock.paused());
    assert!(!testing.path("over-budget", "receipt").exists());
    assert_eq!(
        request(&app, "retained", json!({"op":"pause","native_paused":[]})).await,
        original
    );
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn unknown_cancellation_cannot_grow_the_registry() {
    let (dir, app) = fixture();
    let result = request(
        &app,
        "cancel",
        json!({"op":"cancel","request":"not-accepted"}),
    )
    .await;
    assert_eq!(result["status"], "failed");
    assert!(
        app.testing
            .as_ref()
            .unwrap()
            .cancelled
            .lock()
            .unwrap()
            .is_empty()
    );
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn explicit_spawn_idempotency_and_compact_observations() {
    let (dir, app) = fixture();
    let pos = app
        .world
        .in_world(-1)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let mut incomplete = spec(pos);
    incomplete.as_object_mut().unwrap().remove("equipment");
    assert_eq!(
        request(&app, "missing", json!({"op":"spawn","spec":incomplete})).await["status"],
        "failed"
    );
    let mut incompatible = spec(pos);
    incompatible["equipment"][0] = 2599.into();
    assert_eq!(
        request(&app, "bad-gear", json!({"op":"spawn","spec":incompatible})).await["status"],
        "failed"
    );
    let cmd = json!({"op":"spawn","spec":spec(pos)});
    let created = request(&app, "create", cmd.clone()).await;
    assert_eq!(created["status"], "succeeded");
    assert_eq!(request(&app, "create", cmd).await, created);
    assert_eq!(
        request(&app, "create", json!({"op":"resume"})).await["status"],
        "failed"
    );
    let actor = created["result"]["actor"].as_i64().unwrap();
    let view = request(&app, "view", json!({"op":"observe","actor":actor})).await;
    assert_eq!(view["character"]["level"], 8);
    assert_eq!(view["controller"], "direct");
    assert_eq!(view["equipment"][0]["name"], "Firebrand Staff");
    assert!(view.get("tiles").is_none());
    assert!(serde_json::to_vec(&view).unwrap().len() < 6000);
    let trace = request(
        &app,
        "trace",
        json!({"op":"trace","request":"create","offset":0,"limit":1}),
    )
    .await;
    assert_eq!(trace["events"].as_array().unwrap().len(), 1);
    assert_eq!(
        request(
            &app,
            "trace-path",
            json!({"op":"trace","request":"../control.token","offset":0,"limit":1})
        )
        .await["status"],
        "failed"
    );
    assert_eq!(
        request(
            &app,
            "trace-limit",
            json!({"op":"trace","request":"create","offset":0,"limit":41})
        )
        .await["status"],
        "failed"
    );
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn ordinary_equipment_action_is_committed_and_pause_is_stable() {
    let (dir, app) = fixture();
    let pos = app
        .world
        .in_world(-1)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let created = request(&app, "create", json!({"op":"spawn","spec":spec(pos)})).await;
    let actor = created["result"]["actor"].as_i64().unwrap();
    let before = request(&app, "before", json!({"op":"observe","actor":actor})).await;
    let player = before["session"]["player"].as_i64().unwrap();
    let action = json!({"command":"swap","from":{"object":player,"slot":4,"kind":2599},"to":{"object":player,"slot":3,"kind":-1}});
    let result = request(
        &app,
        "equip",
        json!({"op":"act","actor":actor,"action":action,"duration_ms":0}),
    )
    .await;
    assert_eq!(result["status"], "succeeded");
    assert_eq!(result["paused"], true);
    let after = request(&app, "after", json!({"op":"observe","actor":actor})).await;
    assert!(
        after["character"]["stats"]["max_hp"].as_i64().unwrap()
            > before["character"]["stats"]["max_hp"].as_i64().unwrap()
    );
    let saved = app
        .store
        .load(
            after["session"]["account"].as_i64().unwrap() as i32,
            actor as i32,
        )
        .await
        .unwrap();
    assert_eq!(saved.items[3], 2599);
    assert_eq!(
        saved.revision,
        after["character"]["inventory_revision"].as_i64().unwrap() as i32
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        request(&app, "after-wait", json!({"op":"observe","actor":actor})).await,
        after
    );
    assert_eq!(
        request(
            &app,
            "not-bot",
            json!({"op":"run","actor":actor,"goal":{"kind":"level","level":9},"max_game_ms":1000})
        )
        .await["status"],
        "failed"
    );
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn prepared_priest_attaches_the_named_actual_controller() {
    let (dir, app) = fixture();
    let pos = app
        .world
        .in_world(-2)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let mut body = spec(pos);
    body["class"] = crate::content::PRIEST.into();
    body["controller"] = "game_bot".into();
    body["profile"] = "Kadabu".into();
    body["world"] = (-2).into();
    let equipment = &app.content.get(crate::content::PRIEST).unwrap().equipment;
    body["equipment"] = json!(&equipment[..4]);
    body["backpack"] = json!(&equipment[4..]);
    let created = request(&app, "priest-create", json!({"op":"spawn","spec":body})).await;
    assert_eq!(created["status"], "succeeded");
    let actor = created["result"]["actor"].as_i64().unwrap() as i32;
    let account = created["result"]["account"].as_i64().unwrap() as i32;
    let view = request(
        &app,
        "priest-observe",
        json!({"op":"observe","actor":actor}),
    )
    .await;
    assert_eq!(view["controller"], "game_bot");
    assert_eq!(view["registered_profile"], "Kadabu");
    assert_eq!(view["character"]["class"], crate::content::PRIEST);
    assert_eq!(app.store.load(account, actor).await.unwrap().name, "Kadabu");
    assert!(app.world.clock.paused());
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn actual_gameplay_controller_can_run_a_bounded_time_goal() {
    let (dir, app) = fixture();
    let pos = app
        .world
        .in_world(-2)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let mut spawn = spec(pos);
    spawn["controller"] = "game_bot".into();
    spawn["profile"] = "Pixl".into();
    spawn["world"] = (-2).into();
    let created = request(&app, "create", json!({"op":"spawn","spec":spawn})).await;
    assert_eq!(created["status"], "succeeded");
    let actor = created["result"]["actor"].as_i64().unwrap();
    let result = request(
        &app,
        "bounded",
        json!({"op":"run","actor":actor,"goal":{"kind":"time","ms":100},"max_game_ms":1000}),
    )
    .await;
    assert_eq!(result["result"]["goal_reached"], true);
    assert_eq!(result["result"]["state"]["controller"], "game_bot");
    assert_eq!(result["paused"], true);
    assert_eq!(result["result"]["state"]["lifecycle"]["output"], "live");
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn controls_do_not_enable_the_testing_interface_on_ordinary_state() {
    let (dir, mut app) = fixture();
    app.testing = None;
    let response = request(&app, "no-test-state", json!({"op":"capabilities"})).await;
    assert_eq!(response["status"], "failed");
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("exclusively created --test-state")
    );
    assert!(valid_id("../control.token").is_err());
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn native_input_acknowledgement_is_bound_to_the_exact_session() {
    let (dir, app) = fixture();
    let testing = app.testing.as_ref().unwrap();
    let (sender, mut receiver) = tokio::sync::oneshot::channel();
    testing
        .native_pings
        .lock()
        .unwrap()
        .insert(-1, (42, None, sender));
    testing.acknowledge_native(-1, 43);
    assert_eq!(
        receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    );
    testing.acknowledge_native(-1, 42);
    assert_eq!(
        receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    );
    testing.note_native_frame(-1, 43, b"wrong session");
    assert!(testing.native_pings.lock().unwrap()[&-1].1.is_none());
    testing.note_native_frame(-1, 42, b"actual wire frame");
    testing.acknowledge_native(-1, 42);
    assert_eq!(
        receiver.try_recv(),
        Ok(format!("{:x}", Sha256::digest(b"actual wire frame")))
    );
    testing.acknowledge_native(-1, 42);
    assert!(testing.native_pings.lock().unwrap().is_empty());
    assert!(app.world.clock.paused());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn synchronizing_an_already_paused_session_never_resumes_it() {
    let (dir, app) = fixture();
    let result = request(
        &app,
        "paused-native-sync",
        json!({"op":"native_sync","actors":[1]}),
    )
    .await;
    assert_eq!(result["status"], "failed");
    assert!(result["error"].as_str().unwrap().contains("not connected"));
    assert!(app.world.clock.paused());
    assert_eq!(
        request(
            &app,
            "duplicate-native-sync",
            json!({"op":"native_sync","actors":[1,1]})
        )
        .await["status"],
        "failed"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn native_lease_expiry_is_independent_terminal_and_cannot_be_renewed_by_another_owner() {
    let (dir, app) = fixture();
    let testing = app.testing.as_ref().unwrap();
    native_lease(&app, testing, "controller".into(), "begin".into(), vec![])
        .await
        .unwrap();
    assert!(
        native_lease(&app, testing, "other".into(), "renew".into(), vec![])
            .await
            .is_err()
    );
    app.world.clock.set_paused(false);
    assert!(
        native_lease(&app, testing, "controller".into(), "end".into(), vec![])
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(1700)).await;
    assert!(app.world.clock.paused());
    assert!(
        testing
            .stopped
            .borrow()
            .as_ref()
            .unwrap()
            .contains("controller")
    );
    assert!(
        native_lease(&app, testing, "controller".into(), "renew".into(), vec![])
            .await
            .is_err()
    );
    assert_eq!(
        request(&app, "cannot-resume", json!({"op":"resume"})).await["status"],
        "failed"
    );
    assert!(app.world.clock.paused());
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn released_native_lease_cannot_stop_a_later_operation() {
    let (dir, app) = fixture();
    let testing = app.testing.as_ref().unwrap();
    native_lease(&app, testing, "done".into(), "begin".into(), vec![])
        .await
        .unwrap();
    native_lease(&app, testing, "done".into(), "end".into(), vec![])
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1700)).await;
    assert!(testing.stopped.borrow().is_none());
    assert!(testing.lease.lock().unwrap().is_none());
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn native_pause_json_preserves_integer_session_identity() {
    let (dir, app) = fixture();
    let result = request(
        &app,
        "session-pairs",
        json!({"op":"pause","native_paused":[],"native_sessions":[[3,7233],[4,7234]]}),
    )
    .await;
    assert_eq!(result["status"], "succeeded");
    let bad = request(
        &app,
        "duplicate-pairs",
        json!({"op":"pause","native_paused":[],"native_sessions":[[3,7233],[3,7234]]}),
    )
    .await;
    assert_eq!(bad["status"], "failed");
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn committed_native_death_is_not_reconnect_and_halts_the_owned_cohort() {
    let (dir, app) = fixture();
    let pos = app
        .world
        .in_world(-1)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let mut spawn = spec(pos);
    spawn["controller"] = "native".into();
    let created = request(
        &app,
        "native-death-fixture",
        json!({"op":"spawn","spec":spawn}),
    )
    .await;
    let actor = created["result"]["actor"].as_i64().unwrap() as i32;
    let account = app.store.list("qa-interface-unit".into()).await.unwrap();
    let mut ch = app.store.load(account.id, actor).await.unwrap();
    let living = ch.clone();
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let lost = tokio::sync::watch::channel(false).0;
    let player = app
        .world
        .in_world(-1)
        .call(move |w| {
            let id = w.enter_at(
                living,
                Some(crate::world::Output { packets: tx, lost }),
                Some(pos),
                crate::store::AccountPresentation {
                    fame: 0,
                    best_fame: Default::default(),
                },
            )?;
            w.players.get_mut(&id).unwrap().native_update_acked = true;
            Ok(id)
        })
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let detached_output = app
        .world
        .for_player(player)
        .call(move |w| {
            let output = w.players[&player].tx.clone().unwrap();
            assert!(w.begin_departure(player)?.is_some());
            assert!(w.players[&player].tx.is_none());
            Ok(output)
        })
        .await
        .unwrap();
    let testing = app.testing.as_ref().unwrap();
    assert_eq!(
        native_state(&app, testing, vec![actor]).await.unwrap()["actors"][0]["stage"],
        "transferring"
    );
    assert!(pause(&app, &[actor], &[(actor, player)]).await.is_err());
    assert!(!app.world.clock.paused());
    app.world
        .for_player(player)
        .call(move |w| {
            let p = w.players.get_mut(&player).unwrap();
            p.departing = false;
            p.pending = false;
            p.tx = Some(detached_output);
            Ok(())
        })
        .await
        .unwrap();
    app.world.clock.set_paused(true);
    native_lease(
        &app,
        testing,
        "owned-death".into(),
        "begin".into(),
        vec![actor],
    )
    .await
    .unwrap();
    ch.alive = false;
    ch.hp = 0;
    app.store
        .death(ch, "native lifecycle regression".into())
        .await
        .unwrap();
    app.world
        .for_player(player)
        .call(move |w| {
            w.leave(player)?;
            Ok(())
        })
        .await
        .unwrap();
    let state = native_state(&app, testing, vec![actor]).await.unwrap();
    assert_eq!(state["actors"][0]["stage"], "dead");
    assert_eq!(state["actors"][0]["death_committed"], true);
    app.world.clock.set_paused(false);
    assert!(
        native_lease(&app, testing, "owned-death".into(), "renew".into(), vec![])
            .await
            .is_err()
    );
    assert!(app.world.clock.paused());
    assert!(
        testing
            .stopped
            .borrow()
            .as_ref()
            .unwrap()
            .contains("terminal")
    );
    assert!(
        !app.store
            .character_alive(actor, "qa-interface-unit".into())
            .await
            .unwrap()
    );
    assert!(
        app.store
            .character_alive(actor, "wrong-identity".into())
            .await
            .is_err()
    );
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepared_castle_is_full_strength_and_not_available_on_an_ordinary_directory() {
    let content = Arc::new(Content::load(Path::new("data")).unwrap());
    let mut ordinary =
        crate::world::Directory::with_seed(Path::new("data"), content.clone(), 16, 12351).unwrap();
    assert!(ordinary.prepared_instance(crate::endgame::CASTLE).is_err());
    let mut prepared =
        crate::world::Directory::prepared(Path::new("data"), content, 16, 12351).unwrap();
    let id = prepared.prepared_instance(crate::endgame::CASTLE).unwrap();
    let guards = prepared.worlds[&id]
        .entities
        .values()
        .filter(|e| crate::endgame::GUARDIANS.contains(&e.kind))
        .collect::<Vec<_>>();
    assert_eq!(guards.len(), 2);
    assert!(guards.iter().all(|g| g.hp == 60000));
    assert!(
        !prepared.worlds[&id]
            .entities
            .values()
            .any(|e| e.kind == 0xd7b)
    );
}
