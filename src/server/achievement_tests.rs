use super::*;
use crate::world::Output;
use tokio::sync::watch;

#[tokio::test]
async fn achievement_dead_empty_escape_uses_real_dispatch_before_and_after_permanent_commit() {
    for before in [true, false] {
        let (_dir, mut a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let account = a
            .store
            .list(format!("achievement-dispatch-{before}"))
            .await
            .unwrap()
            .id;
        let mut ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.level = 17;
        a.store.progress(ch.clone()).await.unwrap();
        let character = ch.id;
        let owner = a.claim_session(account).unwrap();
        let (tx, mut rx) = mpsc::channel(256);
        let (lost, _) = watch::channel(None);
        let output = Output {
            packets: tx.clone(),
            lost: lost.clone(),
        };
        let id = a
            .world
            .directory(move |d| d.worlds.get_mut(&-2).unwrap().enter(ch, Some(output)))
            .await
            .unwrap();
        owner.bind(id).unwrap();
        a.world = a.world.for_player(id);
        let mut session = Session {
            account: Some(owner),
            player: Some(id),
            ..Default::default()
        };
        a.world
            .call(move |w| {
                let p = w.players.get_mut(&id).unwrap();
                p.character.hp = 0;
                p.death_pending = true;
                Ok(())
            })
            .await
            .unwrap();
        if before {
            handle(
                &mut a,
                vec![(
                    Packet {
                        id: 42,
                        body: vec![],
                    },
                    std::time::Instant::now(),
                )],
                &mut session,
                &tx,
                &lost,
            )
            .await
            .unwrap();
            assert!(
                a.store
                    .achievement_dead_escape(account, character)
                    .await
                    .is_err()
            );
        }
        persistence_tick(&a).await.unwrap();
        assert_eq!(
            a.store.presentation(account).await.unwrap().credits,
            if before { 120 } else { 110 }
        );
        for _ in 0..2 {
            handle(
                &mut a,
                vec![(
                    Packet {
                        id: 42,
                        body: vec![],
                    },
                    std::time::Instant::now(),
                )],
                &mut session,
                &tx,
                &lost,
            )
            .await
            .unwrap();
        }
        assert_eq!(a.store.presentation(account).await.unwrap().credits, 120);
        assert!(
            handle(
                &mut a,
                vec![(
                    Packet {
                        id: 42,
                        body: vec![1]
                    },
                    std::time::Instant::now()
                )],
                &mut session,
                &tx,
                &lost
            )
            .await
            .is_err()
        );
        assert!(session.transfer.is_none());
        while let Ok(packet) = rx.try_recv() {
            assert_ne!(packet.id, 39, "dead escape cannot reconnect");
        }
        depart(&a, id).await.unwrap();
        session.account.take().unwrap().release();
    }
}

#[tokio::test]
async fn achievement_guild_real_invite_queue_and_join_dispatch_award_but_prepared_founder_does_not()
{
    let (_dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let mut actors = Vec::new();
    for name in ["Achfounder", "Achjoiner"] {
        let account = a
            .store
            .register(
                name.into(),
                format!("{name}@example.invalid"),
                "achievement-guild-password".into(),
            )
            .await
            .unwrap();
        a.store.choose_name(account, name.into()).await.unwrap();
        a.store.compete(account).await.unwrap();
        let mut ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.fame = 20;
        a.store.progress(ch.clone()).await.unwrap();
        actors.push(ch);
    }
    let founder = actors[0].account;
    let joiner = actors[1].account;
    a.store
        .create_prepared_guild(vec![founder], "Achievement Guild".into())
        .await
        .unwrap();
    assert!(
        a.store.achievements(founder).await.unwrap()["earned"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut sessions = Vec::new();
    let mut outputs = Vec::new();
    let mut inboxes = Vec::new();
    for ch in actors {
        let owner = a.claim_session(ch.account).unwrap();
        let presentation = a.store.presentation(ch.account).await.unwrap();
        let (tx, rx) = mpsc::channel(256);
        let (lost, _) = watch::channel(None);
        let output = Output {
            packets: tx.clone(),
            lost: lost.clone(),
        };
        let id = a
            .world
            .directory(move |d| {
                let w = d.worlds.get_mut(&-2).unwrap();
                let id = w.enter(ch, Some(output))?;
                w.players.get_mut(&id).unwrap().presentation = presentation;
                Ok(id)
            })
            .await
            .unwrap();
        owner.bind(id).unwrap();
        sessions.push(Session {
            account: Some(owner),
            player: Some(id),
            ..Default::default()
        });
        outputs.push((tx, lost));
        inboxes.push(rx);
    }
    let (first, second) = (sessions[0].player.unwrap(), sessions[1].player.unwrap());
    a.world
        .directory(move |d| {
            d.worlds
                .get_mut(&-2)
                .unwrap()
                .players
                .get_mut(&first)
                .unwrap()
                .known
                .insert(second);
            Ok(())
        })
        .await
        .unwrap();
    let mut invite = Writer::new();
    invite.utf("Achjoiner");
    a.world = a.world.for_player(first);
    handle(
        &mut a,
        vec![(
            Packet {
                id: wire::GUILD_INVITE,
                body: invite.0,
            },
            std::time::Instant::now(),
        )],
        &mut sessions[0],
        &outputs[0].0,
        &outputs[0].1,
    )
    .await
    .unwrap();
    let mut observed = false;
    while let Ok(p) = inboxes[1].try_recv() {
        observed |= p.id == wire::INVITED_TO_GUILD;
    }
    assert!(observed, "actual target must receive the invitation");
    assert_eq!(
        a.store.achievements(founder).await.unwrap()["earned"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut join = Writer::new();
    join.utf("Achievement Guild");
    a.world = a.world.for_player(second);
    handle(
        &mut a,
        vec![(
            Packet {
                id: wire::JOIN_GUILD,
                body: join.0,
            },
            std::time::Instant::now(),
        )],
        &mut sessions[1],
        &outputs[1].0,
        &outputs[1].1,
    )
    .await
    .unwrap();
    assert_eq!(
        a.store.achievements(joiner).await.unwrap()["earned"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        a.store.guild(founder).await.unwrap().id,
        a.store.guild(joiner).await.unwrap().id
    );
    let mut invite = Writer::new();
    invite.utf("Achjoiner");
    a.world = a.world.for_player(first);
    handle(
        &mut a,
        vec![(
            Packet {
                id: wire::GUILD_INVITE,
                body: invite.0,
            },
            std::time::Instant::now(),
        )],
        &mut sessions[0],
        &outputs[0].0,
        &outputs[0].1,
    )
    .await
    .unwrap();
    assert_eq!(
        a.store.achievements(founder).await.unwrap()["earned"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for session in &mut sessions {
        depart(&a, session.player.unwrap()).await.unwrap();
        session.account.take().unwrap().release();
    }
}

#[tokio::test]
async fn achievement_http_profiles_preserve_competitor_privacy_and_never_award_on_get() {
    let (_dir, a) = account_actions::tests::fixture();
    let account = a.store.list("achievement-http".into()).await.unwrap().id;
    let mut ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.base = a.content.classes[&WIZARD].caps;
    a.store.progress(ch).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new()
        .route("/api/achievements/{account}", get(hiscores::achievements))
        .route("/api/hiscores", get(hiscores::hiscores))
        .with_state(a.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://127.0.0.1:{port}/api/achievements/{account}");
    assert_eq!(
        client.get(&url).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    a.store
        .register(
            "achievement-http".into(),
            "achievement-http@example.invalid".into(),
            "achievement-http-password".into(),
        )
        .await
        .unwrap();
    a.store.compete(account).await.unwrap();
    for _ in 0..2 {
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let value: serde_json::Value = response.json().await.unwrap();
        assert_eq!(value.as_object().unwrap().len(), 3);
        assert_eq!(value["earned"].as_array().unwrap().len(), 2);
        assert_eq!(a.store.presentation(account).await.unwrap().credits, 225);
    }
    let scores: serde_json::Value = client
        .get(format!("http://127.0.0.1:{port}/api/hiscores"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(scores["players"][0]["achievements"], 2);
    assert_eq!(scores["achievement_catalog"].as_array().unwrap().len(), 13);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn achievement_thirteen_actual_public_dtos_are_private_and_match_the_site_catalogue() {
    async fn capture(response: Response, expected: StatusCode) -> (serde_json::Value, Vec<u8>) {
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        (serde_json::from_slice(&body).unwrap(), body.to_vec())
    }
    let (_dir, a) = account_actions::tests::fixture();
    let mut accounts = Vec::new();
    for name in ["Synthetica", "Dtoempty"] {
        let account = a
            .store
            .register(
                format!("synthetic-thirteen-{name}"),
                format!("{name}@example.invalid"),
                "synthetic-dto-password".into(),
            )
            .await
            .unwrap();
        a.store.choose_name(account, name.into()).await.unwrap();
        a.store.compete(account).await.unwrap();
        let mut ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        if accounts.is_empty() {
            ch.base = a.content.classes[&WIZARD].caps;
            ch.level = 20;
            ch.exp = 18050;
            a.store.progress(ch.clone()).await.unwrap();
            // Seeded unit state exercises serialization, not a claim of normal gameplay earning.
            ch.achievements
                .insert(crate::achievements::Achievement::Ah7uck);
            ch.hp = 0;
            ch.alive = false;
            a.store
                .death(ch, "synthetic DTO fixture".into())
                .await
                .unwrap();
        }
        accounts.push(account);
    }
    let account = accounts[0];
    let private = a
        .store
        .list("synthetic-private-thirteen".into())
        .await
        .unwrap()
        .id;
    let before = a.store.presentation(account).await.unwrap().credits;
    let (profile, profile_bytes) = capture(
        hiscores::achievements(State(a.clone()), axum::extract::Path(account)).await,
        StatusCode::OK,
    )
    .await;
    let (scores, score_bytes) =
        capture(hiscores::hiscores(State(a.clone())).await, StatusCode::OK).await;
    let (_, graveyard_bytes) = capture(
        hiscores::graveyard(State(a.clone()), Query(HashMap::new())).await,
        StatusCode::OK,
    )
    .await;
    let (_, denial_bytes) = capture(
        hiscores::achievements(State(a.clone()), axum::extract::Path(private)).await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(profile.as_object().unwrap().len(), 3);
    assert_eq!(profile["earned"].as_array().unwrap().len(), 3);
    assert!(
        profile["earned"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "ah_7uck")
    );
    assert_eq!(scores["players"].as_array().unwrap().len(), 2);
    let catalogue = scores["achievement_catalog"].as_array().unwrap();
    assert_eq!(catalogue.len(), 13);
    assert_eq!(
        catalogue
            .iter()
            .map(|d| d["gold"].as_u64().unwrap())
            .sum::<u64>(),
        400
    );
    assert_eq!(a.store.presentation(account).await.unwrap().credits, before);
    if let Ok(out) = std::env::var("QA_RESULTS_DIR") {
        let out = PathBuf::from(out);
        for (name, bytes) in [
            ("actual-profile.json", profile_bytes),
            ("actual-hiscores.json", score_bytes),
            ("actual-graveyard.json", graveyard_bytes),
            ("actual-private-denial.json", denial_bytes),
        ] {
            std::fs::write(out.join(name), bytes).unwrap();
        }
        std::fs::write(out.join("actual-dto-scope.json"),serde_json::to_vec(&serde_json::json!({"synthetic_store_only":true,"actual_existing_public_handlers_and_json_response_bodies":true,"profile_account":account,"private_account":private,"no_earned400_claim":true})).unwrap()).unwrap();
    }
}
