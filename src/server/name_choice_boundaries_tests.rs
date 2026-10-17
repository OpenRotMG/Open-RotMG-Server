use super::*;

#[tokio::test]
async fn free_http_and_preload_names_stay_free_only_and_storage_failure_retains_owner() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    let owner = a
        .store
        .register(
            "http-name".into(),
            "name@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    let form = [
        ("guid", "name@example.org"),
        ("password", "valid-password"),
        ("name", "HttpFirst"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    let response = account_request(
        State(a.clone()),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        HeaderMap::new(),
        axum::extract::Path("setName".into()),
        Form(form),
    )
    .await;
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), b"<Success/>");
    let (tx, mut rx) = mpsc::channel(128);
    let (lost, lost_rx) = tokio::sync::watch::channel(false);
    let mut session = Session {
        account: Some(a.claim_session(owner).unwrap()),
        ..Session::default()
    };
    let mut packet = Writer::new();
    packet.utf("PrePaid");
    handle(
        &mut a,
        Packet {
            id: 33,
            body: packet.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.id, 20);
    assert_eq!(Reader::new(&reply.body).u8().unwrap(), 0);
    session.account.take().unwrap().release();
    let free = a
        .store
        .register(
            "preload-free".into(),
            "free@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    session.account = Some(a.claim_session(free).unwrap());
    let mut packet = Writer::new();
    packet.utf("WireFree");
    handle(
        &mut a,
        Packet {
            id: 33,
            body: packet.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let reply = rx.recv().await.unwrap();
    assert_eq!(reply.id, 20);
    assert_eq!(Reader::new(&reply.body).u8().unwrap(), 1);
    assert_eq!(a.store.presentation(free).await.unwrap().credits, 100);
    session.account.take().unwrap().release();
    session.account = Some(a.claim_session(owner).unwrap());
    let ch = a
        .store
        .create(owner, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    let player = a
        .world
        .call(move |w| w.enter(ch, Some(output)))
        .await
        .unwrap();
    session.player = Some(player);
    let form = [
        ("guid", "name@example.org"),
        ("password", "valid-password"),
        ("name", "Blocked"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    let response = account_request(
        State(a.clone()),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        HeaderMap::new(),
        axum::extract::Path("setName".into()),
        Form(form),
    )
    .await;
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("active session"));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [owner])
        .unwrap();
    db.execute_batch("CREATE TRIGGER abort_name BEFORE UPDATE OF name ON accounts BEGIN SELECT RAISE(ABORT,'simulated name storage failure'); END;").unwrap();
    let mut packet = Writer::new();
    packet.utf("After");
    assert!(
        handle(
            &mut a,
            Packet {
                id: 33,
                body: packet.0
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now()
        )
        .await
        .is_err()
    );
    assert!(*lost_rx.borrow());
    while let Ok(packet) = rx.try_recv() {
        assert_ne!(
            packet.id, 20,
            "storage failure must not manufacture a normal result"
        );
    }
    assert!(depart(&a, player).await.is_err());
    assert!(a.claim_session(owner).is_err());
    assert_eq!(
        db.query_row(
            "SELECT name,credits FROM accounts WHERE id=?1",
            [owner],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i32>(1)?))
        )
        .unwrap(),
        ("HttpFirst".into(), 1000)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
