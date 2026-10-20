use super::*;
#[path = "finalize_tests.rs"]
mod finalize_tests;

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
                    guild: None,
                    credits: 100,
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

#[tokio::test]
async fn dropped_native_renewal_keeps_deadline_and_traces_late_read() {
    let (dispatch, output) = crate::performance::tests::trace();
    let _logging = tracing::dispatcher::set_default(&dispatch);
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
    let created = request(&app, "renewal-fixture", json!({"op":"spawn","spec":spawn})).await;
    let actor = created["result"]["actor"].as_i64().unwrap() as i32;
    let account = app.store.list("qa-interface-unit".into()).await.unwrap();
    let ch = app.store.load(account.id, actor).await.unwrap();
    let (tx, _packets) = tokio::sync::mpsc::channel(64);
    let lost = tokio::sync::watch::channel(false).0;
    app.world
        .in_world(-1)
        .call(move |w| {
            let id = w.enter_at(
                ch,
                Some(crate::world::Output { packets: tx, lost }),
                Some(pos),
                crate::store::AccountPresentation {
                    guild: None,
                    credits: 100,
                    fame: 0,
                    best_fame: Default::default(),
                },
            )?;
            w.players.get_mut(&id).unwrap().native_update_acked = true;
            Ok(())
        })
        .await
        .unwrap();
    let begun = request(
        &app,
        "renewal-begin",
        json!({"op":"native_lease","owner":"owner","operation":"begin","actors":[actor]}),
    )
    .await;
    assert_eq!(begun["operation"], "begin");
    let renewed = request(
        &app,
        "renewal-control",
        json!({"op":"native_lease","owner":"owner","operation":"renew"}),
    )
    .await;
    assert_eq!(renewed["operation"], "renew");
    let testing = app.testing.as_ref().unwrap();
    let deadline = testing.lease.lock().unwrap().as_ref().unwrap().expires;
    let mut renewal = Box::pin(request(
        &app,
        "dropped-renew",
        json!({"op":"native_lease","owner":"owner","operation":"renew"}),
    ));
    assert!(futures_util::poll!(renewal.as_mut()).is_pending());
    drop(renewal);
    app.world.directory(|_| Ok(())).await.unwrap();
    assert_eq!(
        testing.lease.lock().unwrap().as_ref().unwrap().expires,
        deadline
    );
    assert!(testing.stopped.borrow().is_none());
    native_lease(&app, testing, "owner".into(), "end".into(), vec![])
        .await
        .unwrap();
    let events = crate::performance::tests::events(&output);
    let events = events
        .iter()
        .filter(|event| event["fields"]["request"] == "dropped-renew")
        .collect::<Vec<_>>();
    let dropped = events
        .iter()
        .filter(|event| event["fields"]["message"] == "native control request dropped")
        .collect::<Vec<_>>();
    assert_eq!(dropped.len(), 1, "{events:?}");
    assert_eq!(dropped[0]["fields"]["outcome"], "dropped");
    let snapshot: Value =
        serde_json::from_str(dropped[0]["fields"]["world_requests"].as_str().unwrap()).unwrap();
    assert_eq!(snapshot["calls"], 1);
    assert_eq!(snapshot["completed"], 0);
    assert_eq!(snapshot["round_trip_sum_ms"], 0.);
    assert!(
        !events
            .iter()
            .any(|event| event["fields"]["message"] == "native control request completed")
    );
    let stages = events
        .iter()
        .filter(|event| event["fields"]["message"] == "native control world request")
        .map(|event| event["fields"]["stage"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        stages,
        [
            "capacity_wait",
            "capacity_acquired",
            "enqueued",
            "started",
            "completed",
            "result_send"
        ]
    );
    for stage in ["started", "completed", "result_send"] {
        let event = events
            .iter()
            .find(|event| event["fields"]["stage"] == stage)
            .unwrap();
        assert_eq!(event["fields"]["call"], 1);
        assert_eq!(event["fields"]["receiver_closed"], true);
    }
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
