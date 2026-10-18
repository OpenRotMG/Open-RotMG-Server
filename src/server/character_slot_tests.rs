use super::*;

async fn buy(client: &reqwest::Client, url: &str, fields: &[(&str, &str)]) -> String {
    let response = client
        .post(format!("{url}/legacy-api/account/purchaseCharSlot"))
        .form(fields)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/xml");
    assert_eq!(response.headers()["cache-control"], "no-store");
    response.text().await.unwrap()
}

#[tokio::test]
async fn native_character_slot_purchase_buys_real_capacity_and_rejects_replays() {
    let (dir, app) = account_actions::tests::fixture();
    let owner = app
        .store
        .register(
            "slot-http".into(),
            "slots@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    let first = app
        .store
        .create(owner, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/legacy-api/account/{operation}", post(account_request))
        .route("/legacy-api/char/list", post(chars))
        .with_state(app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let client = reqwest::Client::new();
    let form = [
        ("guid", "slots@example.org"),
        ("password", "valid-password"),
        ("ignore", "-2147483648"),
    ];
    assert!(buy(&client, &url, &form).await.contains("Not enough Gold"));
    assert!(
        buy(
            &client,
            &url,
            &[
                ("guid", "slots@example.org"),
                ("password", "wrong-password"),
                ("ignore", "1")
            ]
        )
        .await
        .starts_with("<Error>")
    );
    let guest = app.store.list("slot-guest".into()).await.unwrap().id;
    assert!(
        buy(
            &client,
            &url,
            &[("guid", "slot-guest"), ("password", ""), ("ignore", "1")]
        )
        .await
        .contains("Register")
    );
    assert!(app.store.purchase_character_slot(guest, 1).await.is_err());
    for timer in [None, Some("2147483648"), Some("-2147483649"), Some("1.5")] {
        let mut fields = form[..2].to_vec();
        if let Some(timer) = timer {
            fields.push(("ignore", timer));
        }
        let error = buy(&client, &url, &fields).await;
        assert!(
            error.starts_with("<Error>") && error.contains("timer"),
            "{error}"
        );
    }
    let active = app.claim_session(owner).unwrap();
    assert!(buy(&client, &url, &form).await.contains("active session"));
    assert!(app.claim_session(owner).is_err());
    drop(active);
    assert_eq!(
        db.query_row("SELECT credits FROM accounts WHERE id=?1", [owner], |r| r
            .get::<_, i32>(
            0
        ))
        .unwrap(),
        100
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM character_slot_purchases", [], |r| r
            .get::<_, i32>(
            0
        ))
        .unwrap(),
        0
    );
    db.execute("UPDATE accounts SET credits=600 WHERE id=?1", [owner])
        .unwrap();
    let foreign = app
        .store
        .register(
            "slot-other".into(),
            "other@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    assert!(
        buy(
            &client,
            &url,
            &[
                ("guid", "other@example.org"),
                ("password", "valid-password"),
                ("ignore", "1"),
                ("accountId", &owner.to_string())
            ]
        )
        .await
        .contains("Not enough Gold")
    );
    assert_eq!(
        buy(&client, &url, &form).await,
        "<Success/>",
        "native purchaseCharSlot must commit a real paid slot"
    );
    let replay = buy(&client, &url, &form).await;
    assert!(
        replay.starts_with("<FatalError>") && replay.contains("reload"),
        "{replay}"
    );
    let xml = client
        .post(format!("{url}/legacy-api/char/list"))
        .form(&form)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let xml = roxmltree::Document::parse(&xml).unwrap();
    assert_eq!(xml.root_element().attribute("maxNumChars"), Some("2"));
    for (tag, expected) in [("Credits", "0"), ("NextCharSlotPrice", "800")] {
        assert_eq!(
            xml.descendants()
                .find(|n| n.has_tag_name(tag))
                .unwrap()
                .text(),
            Some(expected)
        );
    }
    let second = app
        .store
        .create(owner, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert_ne!(first.id, second.id);
    assert!(
        app.store
            .create(owner, app.content.get(WIZARD).unwrap())
            .await
            .is_err()
    );
    for id in [first.id, second.id] {
        assert!(app.store.load(owner, id).await.is_ok());
    }
    assert!(app.store.load(foreign, second.id).await.is_err());
    assert_eq!(
        db.query_row("SELECT credits FROM accounts WHERE id=?1", [owner], |r| r
            .get::<_, i32>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM character_slot_purchases WHERE account=?1",
            [owner],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    server.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
