use super::*;
use crate::server::lifecycle_finalize_tests::{NativeSocket, socket_load, socket_server};
use crate::server::lock_command_tests::chat;

#[tokio::test]
async fn moderators_mark_online_players_and_detector_kicks_only_marked_accounts() {
    let (dir, mut app) = account_actions::tests::fixture();
    app.world.clock.set_paused(true);
    let posts = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let hook = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let webhook = format!(
        "http://127.0.0.1:{}/hook",
        hook.local_addr().unwrap().port()
    );
    let recorded = posts.clone();
    tokio::spawn(async move {
        let route = post(move |Json(body): Json<serde_json::Value>| async move {
            recorded.lock().unwrap().push(body);
        });
        axum::serve(hook, Router::new().route("/hook", route))
            .await
            .unwrap();
    });
    let mut accounts = Vec::new();
    for name in ["Modguy", "Zipper", "Bystander"] {
        let account = app.store.list(name.into()).await.unwrap().id;
        app.store.rename(account, name.into()).await.unwrap();
        let character = app
            .store
            .create(account, app.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        accounts.push((name, account, character.id));
    }
    let [
        (_, moderator, _),
        (_, cheater, cheater_character),
        (_, bystander, _),
    ] = accounts[..]
    else {
        unreachable!()
    };
    std::fs::write(dir.join("reviewed-accounts.json"), "[]").unwrap();
    app.moderation = Some(Arc::new(Moderation {
        moderators: vec![moderator],
        reviews: dir.join("reviewed-accounts.json"),
        webhook,
        http: reqwest::Client::new(),
    }));
    let (port, _server) = socket_server(&app).await;
    let mut sockets = Vec::new();
    let mut players = Vec::new();
    for (guid, _, character) in &accounts {
        let mut socket = NativeSocket::connect(port, &app.origin).await;
        players.push(socket_load(&mut socket, &dir, guid, *character).await);
        sockets.push(socket);
    }
    chat(
        &mut sockets[2],
        "/report Zipper",
        "Only moderators can use /report.",
    )
    .await;
    for (text, reply) in [
        ("/report Modguy", "You cannot report yourself."),
        ("/report Nobody", "Player not found."),
        (
            "/REPORT zipper",
            "Player Zipper was marked as a cheater and will receive fewer white and potion bags.",
        ),
        (
            "/report Zipper",
            "That player is already marked as a cheater.",
        ),
    ] {
        chat(&mut sockets[0], text, reply).await;
    }
    assert_eq!(
        *posts.lock().unwrap(),
        [
            serde_json::json!({"content":"Player Modguy reported player Zipper; they are now marked as a cheater.","allowed_mentions":{"parse":[]}})
        ]
    );
    assert!(app.content.reviewed(cheater) && !app.content.reviewed(bystander));
    let saved = std::fs::read_to_string(dir.join("reviewed-accounts.json")).unwrap();
    assert!(
        saved.contains(&format!("\"account\": {cheater}")),
        "{saved}"
    );

    let kick = |account| {
        let app = app.clone();
        async move {
            let mut headers = HeaderMap::new();
            headers.insert(
                "authorization",
                format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
            );
            let request = CheatKick {
                account,
                realm_lockout_seconds: 300,
            };
            let response = cheat_kick(
                State(app),
                ConnectInfo("127.0.0.1:9".parse().unwrap()),
                headers,
                Json(request),
            )
            .await;
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            (status, String::from_utf8(body.to_vec()).unwrap())
        }
    };
    let (status, body) = kick(bystander).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("is not marked"), "{body}");
    assert_eq!(
        kick(cheater).await,
        (StatusCode::OK, r#"{"disconnected":true}"#.into())
    );

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while app.owners.lock().unwrap().contains_key(&cheater) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut socket = NativeSocket::connect(port, &app.origin).await;
    let returned = socket_load(&mut socket, &dir, "Zipper", cheater_character).await;
    let bystander_player = players[2];
    let (refused, admitted) = app
        .world
        .directory(move |d| {
            let nexus = d.worlds.get_mut(&-2).unwrap();
            let portal = nexus.entities.values().find(|e| e.kind == 0x712).unwrap();
            let (portal, pos) = (portal.id, portal.pos);
            for player in [returned, bystander_player] {
                nexus.players.get_mut(&player).unwrap().pos = pos;
            }
            let refused = d
                .destination(returned, Some(portal))
                .unwrap_err()
                .to_string();
            Ok((
                refused,
                d.destination(bystander_player, Some(portal)).is_ok(),
            ))
        })
        .await
        .unwrap();
    let seconds: u64 = refused
        .strip_prefix("You cannot enter the Realm for another ")
        .and_then(|rest| rest.strip_suffix(" seconds."))
        .unwrap_or_else(|| panic!("{refused}"))
        .parse()
        .unwrap();
    assert!((290..=300).contains(&seconds), "{refused}");
    assert!(admitted);
}
