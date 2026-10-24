use super::*;

async fn request(app: &App, id: &str, command: Value) -> Value {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer guild-test".parse().unwrap());
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

fn emitted_tick_stats(
    receiver: &mut tokio::sync::mpsc::Receiver<crate::wire::Packet>,
) -> BTreeMap<(i32, u8), Value> {
    let packets = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter(|packet| packet.id == crate::wire::NEW_TICK)
        .collect::<Vec<_>>();
    assert_eq!(packets.len(), 1);
    let mut wire = crate::wire::Reader::new(&packets[0].body);
    wire.i32().unwrap();
    wire.i32().unwrap();
    let count = wire.i16().unwrap();
    let mut stats = BTreeMap::new();
    for _ in 0..count {
        let player = wire.i32().unwrap();
        wire.f32().unwrap();
        wire.f32().unwrap();
        for _ in 0..wire.i16().unwrap() {
            let key = wire.u8().unwrap();
            let value = if matches!(key, 31 | 62) {
                json!(wire.utf().unwrap())
            } else {
                json!(wire.i32().unwrap())
            };
            assert!(stats.insert((player, key), value).is_none());
        }
    }
    wire.done().unwrap();
    stats
}

// Backend stepping exercises wire delivery, not native timing, input or rendering.
async fn step_guild_world(app: &App) {
    let clock = app.world.clock.clone();
    app.world
        .in_world(-2)
        .call(move |w| {
            clock.set_paused(false);
            w.step();
            w.step();
            clock.set_paused(true);
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn prepared_guild_commits_real_relation_without_wallet_or_star_changes() {
    let (dir, mut app) = crate::server::account_actions::tests::fixture();
    write_json(
        &dir.join("testing.json"),
        &json!({"schema":1,"provenance":"prepared"}),
    )
    .unwrap();
    app.testing = Testing::open(&dir).unwrap();
    app.control = Some("guild-test".into());
    app.world.clock.set_paused(false);
    assert_eq!(request(&app,"running",json!({"op":"cheat","actor":999,"change":{"kind":"guild","name":"Prepared","members":[]}})).await["status"],"failed");
    app.world.clock.set_paused(true);
    let pos = app
        .world
        .in_world(-2)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let mut actors = Vec::new();
    let mut states = Vec::new();
    let mut presentations = Vec::new();
    for i in 0..3 {
        let result=request(&app,&format!("spawn-{i}"),json!({"op":"spawn","spec":{"guid":format!("qa-guild-{i}"),"class":WIZARD,"level":8,"seed":12351,"equipment":[-1,-1,-1,-1],"backpack":[-1,-1,-1,-1,-1,-1,-1,-1],"world":-2,"position":pos,"controller":"direct"}})).await;
        assert_eq!(result["status"], "succeeded", "{result}");
        let actor = result["result"]["actor"].as_i64().unwrap();
        actors.push(actor);
        let state = request(
            &app,
            &format!("before-{i}"),
            json!({"op":"observe","actor":actor}),
        )
        .await;
        presentations.push(
            app.store
                .presentation(state["session"]["account"].as_i64().unwrap() as i32)
                .await
                .unwrap(),
        );
        states.push(state);
    }
    let command = json!({"op":"cheat","actor":actors[0],"change":{"kind":"guild","name":"Prepared","members":[actors[1]]}});
    let testing = app.testing.take();
    assert_eq!(
        request(&app, "ordinary", command.clone()).await["status"],
        "failed"
    );
    app.testing = testing;
    for (i, change) in [
        json!({"kind":"guild","name":"!","members":[actors[1]]}),
        json!({"kind":"guild","name":"Prepared","members":[actors[0]]}),
        json!({"kind":"guild","name":"Prepared","members":[99999]}),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            request(
                &app,
                &format!("invalid-{i}"),
                json!({"op":"cheat","actor":actors[0],"change":change})
            )
            .await["status"],
            "failed"
        );
        assert_eq!(
            request(
                &app,
                &format!("invalid-unchanged-{i}"),
                json!({"op":"observe","actor":actors[0]})
            )
            .await,
            states[0]
        );
    }
    let players = states
        .iter()
        .map(|s| s["session"]["player"].as_i64().unwrap() as i32)
        .collect::<Vec<_>>();
    let selected = players.clone();
    let mut receivers = app
        .world
        .in_world(-2)
        .call(move |w| {
            let mut receivers = Vec::new();
            for player in &selected {
                let (packets, receiver) = tokio::sync::mpsc::channel(128);
                w.players.get_mut(player).unwrap().tx = Some(crate::world::Output {
                    packets,
                    lost: tokio::sync::watch::channel(false).0,
                });
                receivers.push(receiver);
            }
            Ok(receivers)
        })
        .await
        .unwrap();
    step_guild_world(&app).await;
    for (recipient, receiver) in receivers.iter_mut().enumerate() {
        let stats = emitted_tick_stats(receiver);
        for (i, player) in players.iter().enumerate() {
            assert_eq!(stats[&(*player, 62)], "");
            assert_eq!(stats[&(*player, 63)], 0);
            assert_eq!(stats[&(*player, 30)], presentations[i].stars());
        }
        assert_eq!(
            stats[&(players[recipient], 35)],
            presentations[recipient].credits
        );
        assert_eq!(
            stats[&(players[recipient], 39)],
            presentations[recipient].fame
        );
    }
    let selected = players.clone();
    app.world
        .in_world(-2)
        .call(move |w| {
            for player in &selected {
                assert!(
                    selected
                        .iter()
                        .all(|id| w.players[player].known.contains(id))
                );
            }
            Ok(())
        })
        .await
        .unwrap();
    step_guild_world(&app).await;
    for receiver in &mut receivers {
        let stats = emitted_tick_stats(receiver);
        assert!(!stats.keys().any(|(_, key)| matches!(key, 62 | 63)));
    }
    for (i, state) in states.iter_mut().enumerate() {
        *state = request(
            &app,
            &format!("primed-{i}"),
            json!({"op":"observe","actor":actors[i]}),
        )
        .await;
    }
    let result = request(&app, "create", command.clone()).await;
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(result["result"]["prepared_guild"]["unearned"], true);
    assert_eq!(
        result["result"]["prepared_guild"]["client_publication"],
        "next_ordinary_unpaused_tick"
    );
    assert!(app.world.clock.paused());
    for receiver in &mut receivers {
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }
    assert_eq!(request(&app, "create", command).await, result);
    let guild = result["result"]["prepared_guild"]["guild"]["id"]
        .as_i64()
        .unwrap() as i32;
    for i in 0..3 {
        let account = states[i]["session"]["account"].as_i64().unwrap() as i32;
        let actual = app.store.presentation(account).await.unwrap();
        let mut expected = presentations[i].clone();
        if i < 2 {
            assert_eq!(actual.guild.as_ref().unwrap().id, guild);
            assert_eq!(
                actual.guild.as_ref().unwrap().rank,
                if i == 0 { 40 } else { 0 }
            );
            expected.guild = actual.guild.clone();
        }
        assert_eq!(actual, expected);
        let after = request(
            &app,
            &format!("after-{i}"),
            json!({"op":"observe","actor":actors[i]}),
        )
        .await;
        assert_eq!(after["session"], states[i]["session"]);
        assert_eq!(after["character"], states[i]["character"]);
    }
    let rejected=request(&app,"taken",json!({"op":"cheat","actor":actors[2],"change":{"kind":"guild","name":"prepared","members":[]}})).await;
    assert_eq!(rejected["status"], "failed");
    assert_eq!(
        request(
            &app,
            "third-unchanged",
            json!({"op":"observe","actor":actors[2]})
        )
        .await,
        states[2]
    );
    let account = states[2]["session"]["account"].as_i64().unwrap() as i32;
    assert!(
        app.store
            .change_guild(
                account,
                crate::store::GuildAction::Create("Ordinary".into())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("Choose a name")
    );
    let founder = states[0]["session"]["account"].as_i64().unwrap() as i32;
    let info = app.store.guild(founder).await.unwrap();
    assert_eq!((info.fame, info.earned), (0, 0));
    step_guild_world(&app).await;
    for receiver in &mut receivers {
        let stats = emitted_tick_stats(receiver);
        assert_eq!(stats[&(players[0], 62)], "Prepared");
        assert_eq!(stats[&(players[0], 63)], 40);
        assert_eq!(stats[&(players[1], 62)], "Prepared");
        // Member rank remains zero, so the primed delta cache must elide it.
        assert!(!stats.contains_key(&(players[1], 63)));
        assert!(!stats.contains_key(&(players[2], 62)));
        assert!(!stats.contains_key(&(players[2], 63)));
        assert!(!stats.keys().any(|(_, key)| matches!(key, 30 | 35 | 39)));
    }
    app.shutdown.send_replace(true);
    fs::remove_dir_all(dir).unwrap();
}
