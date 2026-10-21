use super::*;

#[tokio::test]
async fn community_social_prepared_realm_capacity_is_narrow_and_once_only() {
    let (dir, app) = fixture();
    let global = app.max_players;
    for (n, limit) in [0, global, global + 1].into_iter().enumerate() {
        let r = request(
            &app,
            &format!("bad-limit-{n}"),
            json!({"op":"prepare_realm_capacity","limit":limit}),
        )
        .await;
        assert_eq!(r["status"], "failed");
    }
    app.world.clock.set_paused(false);
    let r = request(
        &app,
        "running",
        json!({"op":"prepare_realm_capacity","limit":4}),
    )
    .await;
    assert_eq!(r["status"], "failed");
    app.world.clock.set_paused(true);
    let owner = app.claim(900).unwrap();
    let r = request(
        &app,
        "claimed",
        json!({"op":"prepare_realm_capacity","limit":4}),
    )
    .await;
    assert_eq!(r["status"], "failed");
    owner.release();
    let released = request(
        &app,
        "released",
        json!({"op":"prepare_realm_capacity","limit":4}),
    )
    .await;
    assert_eq!(released["status"], "failed");
    assert!(released["error"].as_str().unwrap().contains("admission"));
    println!("released claim capacity evidence: {}", dir.display());
    app.shutdown.send_replace(true);
    let (dir, app) = fixture();
    let global = app.max_players;
    let command = json!({"op":"prepare_realm_capacity","limit":4});
    let r = request(&app, "prepare", command.clone()).await;
    assert_eq!(r["status"], "succeeded", "{r}");
    assert_eq!(r["result"]["limit"], 4);
    assert_eq!(r["result"]["global_limit"], global);
    assert_eq!(request(&app, "prepare", command).await, r);
    let target = r["result"]["world"].as_i64().unwrap() as i32;
    assert_eq!(app.max_players, global);
    assert_eq!(
        app.world
            .in_world(target)
            .call(|w| Ok(w.max_players))
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        app.world
            .in_world(-2)
            .call(|w| Ok(w.max_players))
            .await
            .unwrap(),
        global
    );
    assert_eq!(
        request(
            &app,
            "again",
            json!({"op":"prepare_realm_capacity","limit":3})
        )
        .await["status"],
        "failed"
    );
    let caps = request(&app, "caps", json!({"op":"capabilities"})).await;
    assert_eq!(caps["prepared_realm_capacity"], r["result"]);
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn community_social_realm_capacity_rejects_unprotected_unprepared_and_late() {
    let (dir, app) = fixture();
    let raw = json!({"id":"unauth","command":{"op":"prepare_realm_capacity","limit":4}});
    let response = super::super::command(State(app.clone()), HeaderMap::new(), Json(raw)).await;
    let r: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(r["status"], "failed");
    let mut unprepared = app.clone();
    unprepared.testing = None;
    assert_eq!(
        request(
            &unprepared,
            "unprepared",
            json!({"op":"prepare_realm_capacity","limit":4})
        )
        .await["status"],
        "failed"
    );
    let pos = app
        .world
        .in_world(-1)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let born = request(&app, "enroll", json!({"op":"spawn","spec":spec(pos)})).await;
    assert_eq!(born["status"], "succeeded");
    assert_eq!(
        request(
            &app,
            "late",
            json!({"op":"prepare_realm_capacity","limit":4})
        )
        .await["status"],
        "failed"
    );
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn community_social_capacity_fence_survives_registered_routes_release_and_receipt_removal() {
    for controller in ["direct", "native", "game_bot"] {
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
        let mut invalid = spec(pos);
        invalid["class"] = json!(1);
        assert_eq!(
            request(&app, "invalid-spawn", json!({"op":"spawn","spec":invalid})).await["status"],
            "failed"
        );
        let prepared = request(
            &app,
            "initial-setup",
            json!({"op":"prepare_realm_capacity","limit":4}),
        )
        .await;
        assert_eq!(
            prepared["status"], "succeeded",
            "invalid static Spawn must not latch admission"
        );
        let mut value = spec(pos);
        value["controller"] = json!(controller);
        if controller == "game_bot" {
            value["profile"] = json!("Triceps");
        }
        let entered = request(&app, "registered", json!({"op":"spawn","spec":value})).await;
        assert_eq!(entered["status"], "succeeded", "{entered}");
        let actor = entered["result"]["actor"].as_i64().unwrap() as i32;
        if controller == "direct" {
            let player = resolve(&app, actor).await.unwrap();
            let owner = app.bots.lock().unwrap().remove(&player).unwrap();
            crate::server::depart(&app, player).await.unwrap();
            owner.release();
        } else if controller == "game_bot" {
            app.shutdown.send_replace(true);
            tokio::time::timeout(Duration::from_secs(15), async {
                while !app.owners.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
        }
        assert!(app.owners.lock().unwrap().is_empty());
        assert!(app.quarantined.lock().unwrap().is_empty());
        assert_eq!(
            app.world
                .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
                .await
                .unwrap(),
            0
        );
        let t = app.testing.as_ref().unwrap();
        for entry in fs::read_dir(&t.root).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_str().unwrap();
            if name.ends_with(".spawn.json")
                || name.ends_with(".native.json")
                || name == "prepared-realm-capacity.json"
            {
                fs::remove_file(entry.path()).unwrap();
            }
        }
        let mut clone = app.clone();
        clone.world = clone.world.in_world(-2);
        let late = if controller == "game_bot" {
            // Shutdown closes the HTTP interface before its admission guard can run.
            let error = prepare_realm_capacity(&clone, clone.testing.as_ref().unwrap(), 3)
                .await
                .expect_err("stopped controller history must remain fenced");
            json!({"status":"failed","error":error.to_string()})
        } else {
            request(
                &clone,
                "after-receipt-removal",
                json!({"op":"prepare_realm_capacity","limit":3}),
            )
            .await
        };
        println!(
            "registered admission-history evidence: {} route={controller} response={late}",
            dir.display()
        );
        assert_eq!(late["status"], "failed");
        assert!(late["error"].as_str().unwrap().contains("admission"));
        assert!(!t.root.join("prepared-realm-capacity.json").exists());
        assert_eq!(
            app.world
                .directory(|d| {
                    let realm = d.initial(-3)?;
                    Ok(d.worlds[&realm].max_players)
                })
                .await
                .unwrap(),
            4
        );
        app.shutdown.send_replace(true);
    }
}

#[tokio::test]
async fn community_social_capacity_fence_survives_validated_spawn_storage_rejection() {
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
    let account = app.store.list("qa-interface-unit".into()).await.unwrap().id;
    app.store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut value = spec(pos);
    value["controller"] = json!("native");
    let failed = request(&app, "already-living", json!({"op":"spawn","spec":value})).await;
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("living character")
    );
    assert!(app.owners.lock().unwrap().is_empty());
    let late = request(
        &app,
        "after-rejected-admission",
        json!({"op":"prepare_realm_capacity","limit":4}),
    )
    .await;
    assert_eq!(late["status"], "failed");
    assert!(late["error"].as_str().unwrap().contains("admission"));
    println!(
        "downstream rejection admission-history evidence: {} response={late}",
        dir.display()
    );
    app.shutdown.send_replace(true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn community_social_capacity_preparation_and_claim_are_linearized_by_shared_ownership_lock() {
    let (dir, app) = fixture();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let start = barrier.clone();
    let contender = app.clone();
    let claim = std::thread::spawn(move || {
        start.wait();
        contender.claim(701).unwrap().release();
    });
    barrier.wait();
    let result = request(
        &app,
        "concurrent-setup",
        json!({"op":"prepare_realm_capacity","limit":4}),
    )
    .await;
    claim.join().unwrap();
    let limit = app
        .world
        .directory(|d| {
            let realm = d.initial(-3)?;
            Ok(d.worlds[&realm].max_players)
        })
        .await
        .unwrap();
    if result["status"] == "succeeded" {
        assert_eq!(limit, 4);
    } else {
        assert!(
            result["error"].as_str().unwrap().contains("admission")
                || result["error"].as_str().unwrap().contains("ownership")
        );
        assert_eq!(limit, 16);
    }
    let late = request(
        &app,
        "after-race",
        json!({"op":"prepare_realm_capacity","limit":3}),
    )
    .await;
    assert_eq!(late["status"], "failed");
    println!(
        "concurrent admission-history evidence: {} first={result} later={late}",
        dir.display()
    );
    app.shutdown.send_replace(true);
}
