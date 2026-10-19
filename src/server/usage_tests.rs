use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, pkcs8::DecodePrivateKey};

async fn endpoint(a: App) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/legacy-api/char/list", post(chars))
        .route("/dev/join", post(bot_create))
        .route("/dev/leave", post(bot_leave))
        .with_state(a);
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (url, task)
}

async fn list(client: &reqwest::Client, url: &str) -> (reqwest::StatusCode, String) {
    let r = client
        .post(format!("{url}/legacy-api/char/list"))
        .form(&[("guid", "usage-observer"), ("password", "")])
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["content-type"], "application/xml");
    (r.status(), r.text().await.unwrap())
}

async fn snapshot(client: &reqwest::Client, url: &str, expected: f64, original: &str) {
    let (status, xml) = list(client, url).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let node = doc.descendants().find(|n| n.has_tag_name("Usage")).unwrap();
    assert_eq!(node.text().unwrap().parse::<f64>().unwrap(), expected);
    let mut normalized = xml.clone();
    normalized.replace_range(node.range(), "<Usage>0</Usage>");
    assert_eq!(normalized, original, "only Usage may change");
}

#[tokio::test]
async fn advertised_usage_tracks_existing_ownership_over_real_http() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.max_players = 3;
    a.control = Some("usage-test-control".into());
    a.world.clock.set_paused(true);
    let mut accounts = Vec::new();
    for guid in [
        "usage-observer",
        "usage-pending",
        "usage-session",
        "usage-bot",
    ] {
        let account = a.store.list(guid.into()).await.unwrap();
        let ch = a
            .store
            .create(account.id, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        accounts.push((account.id, ch.id));
    }
    let (url, server) = endpoint(a.clone()).await;
    let client = reqwest::Client::new();
    let (status, original) = list(&client, &url).await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert!(original.contains("<Usage>0</Usage>"));
    let pending = a.claim(accounts[1].0).unwrap();
    snapshot(&client, &url, 1.0 / 3.0, &original).await;

    let key =
        RsaPrivateKey::from_pkcs8_pem(&std::fs::read_to_string(dir.join("server.pem")).unwrap())
            .unwrap();
    let encrypted = STANDARD.encode(
        key.to_public_key()
            .encrypt(&mut rand::rngs::OsRng, Pkcs1v15Encrypt, b"usage-session")
            .unwrap(),
    );
    let mut hello = Writer::new();
    hello
        .utf("123.5.1")
        .i32(-2)
        .utf(&encrypted)
        .utf("")
        .utf("")
        .i32(0)
        .i16(0)
        .i32(0);
    for _ in 0..4 {
        hello.utf("");
    }
    let (tx, mut rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let mut session = Session::default();
    handle(
        &mut a,
        Packet {
            id: wire::HELLO,
            body: hello.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    assert_eq!(rx.recv().await.unwrap().id, wire::MAP_INFO);
    assert!(session.account.is_some() && session.player.is_none());
    assert_eq!(
        a.world
            .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
            .await
            .unwrap(),
        0
    );
    assert!(a.claim(accounts[2].0).is_err());
    snapshot(&client, &url, 2.0 / 3.0, &original).await;

    let joined = client
        .post(format!("{url}/dev/join"))
        .bearer_auth("usage-test-control")
        .json(&serde_json::json!({"guid":"usage-bot","world":-2}))
        .send()
        .await
        .unwrap();
    assert_eq!(joined.status(), reqwest::StatusCode::OK);
    let bot = joined.json::<serde_json::Value>().await.unwrap()["player"]
        .as_i64()
        .unwrap();
    assert_eq!(a.bots.lock().unwrap().len(), 1);
    assert!(
        a.claim(accounts[0].0)
            .err()
            .unwrap()
            .to_string()
            .contains("world full")
    );
    assert_eq!(a.owners.lock().unwrap().len(), 3);
    snapshot(&client, &url, 1.0, &original).await;

    handle(
        &mut a,
        Packet {
            id: wire::LOAD,
            body: accounts[2].1.to_be_bytes().to_vec(),
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    snapshot(&client, &url, 1.0, &original).await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER fail_usage_save BEFORE UPDATE OF stats ON characters WHEN OLD.id={} BEGIN SELECT RAISE(FAIL,'usage test save failure'); END;", accounts[2].1)).unwrap();
    let failure = depart(&a, session.player.unwrap()).await.unwrap_err();
    let owner = session.account.take().unwrap();
    owner.failed(failure.to_string());
    a.quarantined.lock().unwrap().push(owner);
    snapshot(&client, &url, 1.0, &original).await;
    pending.release();
    snapshot(&client, &url, 2.0 / 3.0, &original).await;
    a.claim(accounts[1].0).unwrap().release();
    let left = client
        .post(format!("{url}/dev/leave"))
        .bearer_auth("usage-test-control")
        .json(&serde_json::json!({"player":bot}))
        .send()
        .await
        .unwrap();
    assert_eq!(left.status(), reqwest::StatusCode::OK);
    assert!(a.bots.lock().unwrap().is_empty());
    snapshot(&client, &url, 1.0 / 3.0, &original).await;
    a.quarantined.lock().unwrap().clear();
    snapshot(&client, &url, 1.0 / 3.0, &original).await;
    assert!(
        a.claim(accounts[2].0).is_err(),
        "dropping an Owner must not clear a real SQL fence"
    );

    let bad = client
        .post(format!("{url}/legacy-api/char/list"))
        .form(&[
            ("guid", "missing@example.invalid"),
            ("password", "wrong-password"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), reqwest::StatusCode::OK);
    assert_eq!(
        bad.text().await.unwrap(),
        "<Error>Account credentials not valid</Error>"
    );
    let malformed = client
        .post(format!("{url}/legacy-api/char/list"))
        .form(&[("guid", "usage-observer")])
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        malformed.text().await.unwrap(),
        "<Error>password required</Error>"
    );
    assert_eq!(a.owners.lock().unwrap().len(), 1);
    assert!(a.owners.lock().unwrap().contains_key(&accounts[2].0));
    server.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn advertised_usage_rejects_invalid_capacity_and_overfull_registry() {
    let (dir, mut a) = account_actions::tests::fixture();
    let client = reqwest::Client::new();
    for capacity in [0, 86, 3] {
        a.max_players = capacity;
        if capacity == 3 {
            a.max_players = 4;
            let claims: Vec<_> = (1..=4).map(|id| a.claim(id).unwrap()).collect();
            for claim in &claims {
                claim.failed("overfull registry fixture".into());
            }
            drop(claims);
            a.max_players = 3;
        }
        let (url, server) = endpoint(a.clone()).await;
        let (status, xml) = list(&client, &url).await;
        assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
        assert!(xml.starts_with("<Error>"));
        assert!(xml.contains(if capacity == 3 {
            "ownership exceeds capacity"
        } else {
            "invalid server capacity"
        }));
        assert!(!xml.contains("<Usage>"));
        server.abort();
    }
    a.owners.lock().unwrap().clear();
    let registry = a.owners.clone();
    assert!(
        std::thread::spawn(move || {
            let _held = registry.lock().unwrap();
            panic!("intentional ownership poison fixture");
        })
        .join()
        .is_err()
    );
    let (url, server) = endpoint(a).await;
    let (status, xml) = list(&client, &url).await;
    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(xml, "<Error>ownership registry poisoned</Error>");
    server.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
