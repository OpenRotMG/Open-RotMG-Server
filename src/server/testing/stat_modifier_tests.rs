use super::*;

#[tokio::test]
async fn prepared_signed_stat_modifier_is_bounded_ephemeral_and_idempotent() {
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
    let mut input = spec(pos);
    input["world"] = (-2).into();
    input["equipment"] = json!(vec![-1; 4]);
    input["backpack"] = json!(vec![-1; 8]);
    let mut base = app.content.classes[&WIZARD].base;
    base[4] = 20;
    input["stats"] = json!(base);
    let spawned = request(&app, "modifier-spawn", json!({"op":"spawn","spec":input})).await;
    assert_eq!(spawned["status"], "succeeded", "{spawned}");
    let actor = spawned["result"]["actor"].as_i64().unwrap();
    let before = request(
        &app,
        "modifier-before",
        json!({"op":"observe","actor":actor}),
    )
    .await;
    let change = json!({"kind":"stat_modifier","index":4,"amount":-5,"duration_ms":1000});
    let command = json!({"op":"cheat","actor":actor,"change":change});
    let applied = request(&app, "modifier-apply", command.clone()).await;
    assert_eq!(applied["status"], "succeeded", "{applied}");
    assert_eq!(request(&app, "modifier-apply", command).await, applied);
    let after = request(
        &app,
        "modifier-after",
        json!({"op":"observe","actor":actor}),
    )
    .await;
    assert_eq!(before["character"]["stats"]["speed"], 20);
    assert_eq!(after["character"]["stats"]["speed"], 15);
    for key in ["base_stats", "items", "inventory_revision", "hp", "mp"] {
        assert_eq!(after["character"][key], before["character"][key], "{key}");
    }
    assert_eq!(after["session"], before["session"]);
    let player = after["session"]["player"].as_i64().unwrap() as i32;
    let encoded = app
        .world
        .for_player(player)
        .call(move |w| w.stats(player, player))
        .await
        .unwrap();
    let mut wire = crate::wire::Reader::new(&encoded);
    assert_eq!(wire.i32().unwrap(), player);
    wire.f32().unwrap();
    wire.f32().unwrap();
    let count = wire.i16().unwrap();
    let mut values = std::collections::BTreeMap::new();
    for _ in 0..count {
        let key = wire.u8().unwrap();
        if matches!(key, 31 | 62) {
            wire.utf().unwrap();
        } else {
            values.insert(key, wire.i32().unwrap());
        }
    }
    wire.done().unwrap();
    assert_eq!(values[&22], 15);
    assert_eq!(values[&50], -5);
    let invalid = [
        json!({"index":0,"amount":-1,"duration_ms":1000}),
        json!({"index":8,"amount":-1,"duration_ms":1000}),
        json!({"index":4,"amount":0,"duration_ms":1000}),
        json!({"index":4,"amount":-101,"duration_ms":1000}),
        json!({"index":4,"amount":101,"duration_ms":1000}),
        json!({"index":4,"amount":-1,"duration_ms":0}),
        json!({"index":4,"amount":-1,"duration_ms":60001}),
        json!({"index":4,"amount":-20,"duration_ms":1000}),
    ];
    for (i, mut value) in invalid.into_iter().enumerate() {
        value["kind"] = "stat_modifier".into();
        let rejected = request(
            &app,
            &format!("modifier-invalid-{i}"),
            json!({"op":"cheat","actor":actor,"change":value}),
        )
        .await;
        assert_eq!(rejected["status"], "failed", "{rejected}");
        assert_eq!(
            request(
                &app,
                &format!("modifier-unchanged-{i}"),
                json!({"op":"observe","actor":actor})
            )
            .await,
            after
        );
    }
    let foreign = request(
        &app,
        "modifier-foreign",
        json!({"op":"cheat","actor":99999,"change":change}),
    )
    .await;
    assert_eq!(foreign["status"], "failed");
    let mut incompatible = base;
    incompatible[4] = 0;
    let rejected = request(
        &app,
        "modifier-invalid-base",
        json!({"op":"cheat","actor":actor,"change":{"kind":"stats","values":incompatible}}),
    )
    .await;
    assert_eq!(rejected["status"], "failed");
    assert!(
        rejected["error"]
            .as_str()
            .unwrap()
            .contains("active prepared modifier")
    );
    assert_eq!(
        request(
            &app,
            "modifier-base-unchanged",
            json!({"op":"observe","actor":actor})
        )
        .await,
        after
    );
    let saved = app
        .store
        .load(
            after["session"]["account"].as_i64().unwrap() as i32,
            actor as i32,
        )
        .await
        .unwrap();
    assert_eq!(saved.base, base);
    let mut capped = base;
    capped[4] = app.content.classes[&WIZARD].caps[4];
    let cap_result = request(
        &app,
        "modifier-cap",
        json!({"op":"cheat","actor":actor,"change":{"kind":"stats","values":capped}}),
    )
    .await;
    assert_eq!(cap_result["status"], "succeeded", "{cap_result}");
    let gold = request(&app, "modifier-gold", json!({"op":"observe","actor":actor})).await;
    assert_eq!(gold["character"]["base_stats"][4], capped[4]);
    assert_eq!(gold["character"]["stats"]["speed"], capped[4] - 5);
    app.world
        .for_player(player)
        .call(move |w| {
            w.now += 1000;
            w.step();
            Ok(())
        })
        .await
        .unwrap();
    let expired = request(
        &app,
        "modifier-expired",
        json!({"op":"observe","actor":actor}),
    )
    .await;
    assert_eq!(expired["character"]["stats"]["speed"], capped[4]);
    app.world.clock.set_paused(false);
    let running = request(
        &app,
        "modifier-valid-running",
        json!({"op":"cheat","actor":actor,"change":change}),
    )
    .await;
    assert_eq!(running["status"], "failed");
    assert!(
        running["error"]
            .as_str()
            .unwrap()
            .contains("explicitly paused")
    );
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn prepared_stat_modifier_cannot_mutate_normal_or_running_state() {
    let (dir, mut app) = fixture();
    let command = json!({"op":"cheat","actor":1,"change":{"kind":"stat_modifier","index":4,"amount":-5,"duration_ms":1000}});
    let testing = app.testing.take();
    let ordinary = request(&app, "modifier-normal-state", command.clone()).await;
    assert_eq!(ordinary["status"], "failed");
    assert!(
        ordinary["error"]
            .as_str()
            .unwrap()
            .contains("exclusively created --test-state")
    );
    app.testing = testing;
    app.world.clock.set_paused(false);
    let running = request(&app, "modifier-running", command).await;
    assert_eq!(running["status"], "failed");
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
