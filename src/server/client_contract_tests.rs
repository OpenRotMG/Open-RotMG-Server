use super::*;

const INVALID: &str = "<Error>Account credentials not valid</Error>";

async fn response(response: Response) -> (StatusCode, String) {
    assert_eq!(response.headers()["content-type"], "application/xml");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn list(a: &App, guid: &str, password: &str, extra: &[(&str, &str)]) -> (StatusCode, String) {
    let form = [("guid", guid), ("password", password)]
        .into_iter()
        .chain(extra.iter().copied())
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    response(
        chars(
            State(a.clone()),
            ConnectInfo("127.0.0.1:1234".parse().unwrap()),
            HeaderMap::new(),
            Form(form),
        )
        .await,
    )
    .await
}

#[tokio::test]
async fn chars_translates_only_typed_credential_rejections_to_legacy_xml() {
    let (dir, a) = account_actions::tests::fixture();
    let account = a
        .store
        .register(
            "converted".into(),
            "owner@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    for (guid, password) in [
        ("unknown@example.org", "valid-password"),
        ("owner@example.org", "wrong-password"),
        ("owner@example.org", ""),
        ("converted", ""),
        ("not an email", "valid-password"),
        ("owner@example.org", "x"),
    ] {
        assert_eq!(
            list(&a, guid, password, &[]).await,
            (StatusCode::OK, INVALID.into()),
            "{guid}"
        );
    }
    for (guid, password) in [("fresh-guest", ""), ("owner@example.org", "valid-password")] {
        let (status, xml) = list(&a, guid, password, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            roxmltree::Document::parse(&xml)
                .unwrap()
                .root_element()
                .tag_name()
                .name(),
            "Chars"
        );
    }
    let (status, xml) = list(&a, "fresh-guest", "", &[("secret", "external-token")]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_ne!(xml, INVALID);
    assert!(xml.contains("external platform authentication"));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute(
        "UPDATE credentials SET password_hash='corrupt' WHERE account=?1",
        [account],
    )
    .unwrap();
    let (status, xml) = list(&a, "owner@example.org", "valid-password", &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_ne!(xml, INVALID);
    assert!(xml.contains("invalid stored password verifier"));
    db.execute_batch("DROP TABLE credentials").unwrap();
    let (status, xml) = list(&a, "owner@example.org", "valid-password", &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_ne!(xml, INVALID);
    assert!(xml.contains("no such table"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn chars_origin_and_rate_denials_do_not_masquerade_as_credentials() {
    let (dir, a) = account_actions::tests::fixture();
    let mut headers = HeaderMap::new();
    headers.insert("origin", "https://unapproved.example".parse().unwrap());
    let (status, xml) = response(
        chars(
            State(a.clone()),
            ConnectInfo("127.0.0.1:1234".parse().unwrap()),
            headers,
            Form(HashMap::new()),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(xml.contains("unapproved request origin"));
    assert_ne!(xml, INVALID);
    assert!(a.requests.lock().unwrap().is_empty());
    for _ in 0..20 {
        assert_eq!(
            list(&a, "bad identity", "", &[]).await,
            (StatusCode::OK, INVALID.into())
        );
    }
    let (status, xml) = list(&a, "bad identity", "", &[]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_ne!(xml, INVALID);
    std::fs::remove_dir_all(dir).unwrap();
}

async fn delete(a: &App, fields: &[(&str, &str)]) -> (StatusCode, String) {
    let result = delete_character(
        State(a.clone()),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        HeaderMap::new(),
        Form(
            fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ),
    )
    .await;
    assert_eq!(result.headers()["cache-control"], "no-store");
    response(result).await
}

#[tokio::test]
async fn deletion_authenticates_owns_the_character_and_excludes_active_sessions() {
    let (dir, a) = account_actions::tests::fixture();
    let account = a.store.list("delete-owner".into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let id = ch.id.to_string();
    let form = [
        ("guid", "delete-owner"),
        ("password", ""),
        ("charId", id.as_str()),
    ];
    let active = a.claim_session(account).unwrap();
    let (status, xml) = delete(&a, &form).await;
    assert_eq!(status, StatusCode::OK);
    assert!(xml.contains("active session"));
    assert!(a.store.load(account, ch.id).await.is_ok());
    drop(active);
    let foreign = a.store.list("delete-other".into()).await.unwrap();
    assert!(
        delete(
            &a,
            &[("guid", "delete-other"), ("password", ""), ("charId", &id)]
        )
        .await
        .1
        .starts_with("<Error>")
    );
    assert!(
        a.store
            .list("delete-other".into())
            .await
            .unwrap()
            .characters
            .is_empty()
    );
    assert!(!a.owners.lock().unwrap().contains_key(&foreign.id));
    assert_eq!(
        delete(&a, &form).await,
        (StatusCode::OK, "<Success/>".into())
    );
    assert!(
        a.store
            .list("delete-owner".into())
            .await
            .unwrap()
            .characters
            .is_empty()
    );
    assert!(!a.owners.lock().unwrap().contains_key(&account));
    let replacement = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(delete(&a, &form).await.1.starts_with("<Error>"));
    assert!(a.store.load(account, replacement.id).await.is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn deletion_rejects_bad_credentials_and_malformed_native_forms_without_mutation() {
    let (dir, a) = account_actions::tests::fixture();
    let account = a
        .store
        .register(
            "delete-converted".into(),
            "owner@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let id = ch.id.to_string();
    for (guid, password) in [
        ("owner@example.org", "wrong-password"),
        ("owner@example.org", ""),
        ("delete-converted", ""),
        ("unknown-guest", ""),
    ] {
        assert_eq!(
            delete(
                &a,
                &[("guid", guid), ("password", password), ("charId", &id)]
            )
            .await,
            (StatusCode::OK, INVALID.into())
        );
    }
    for bad in ["", "0", "-1", "1.5", "2147483648"] {
        assert!(
            delete(
                &a,
                &[
                    ("guid", "owner@example.org"),
                    ("password", "valid-password"),
                    ("charId", bad)
                ]
            )
            .await
            .1
            .contains("invalid charId")
        );
    }
    for missing in ["guid", "password", "charId"] {
        let fields: Vec<_> = [
            ("guid", "owner@example.org"),
            ("password", "valid-password"),
            ("charId", id.as_str()),
        ]
        .into_iter()
        .filter(|(key, _)| *key != missing)
        .collect();
        assert!(delete(&a, &fields).await.1.contains("required"));
    }
    assert!(
        delete(
            &a,
            &[
                ("guid", "owner@example.org"),
                ("password", "valid-password"),
                ("charId", &id),
                ("secret", "external")
            ]
        )
        .await
        .1
        .contains("external platform authentication")
    );
    assert_eq!(a.store.load(account, ch.id).await.unwrap().items, ch.items);
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        delete(
            &a,
            &[
                ("guid", "OWNER@EXAMPLE.ORG"),
                ("password", "valid-password"),
                ("charId", &id),
                ("reason", "1")
            ]
        )
        .await,
        (StatusCode::OK, "<Success/>".into())
    );
    std::fs::remove_dir_all(dir).unwrap();
}

fn use_item(object: i32, slot: u8) -> Packet {
    let mut out = Writer::new();
    out.i32(1).i32(object).u8(slot).i16(0xa22).f32(0.).f32(0.);
    Packet {
        id: 30,
        body: out.0,
    }
}

async fn loaded(a: &App, guid: &str) -> Session {
    let account = a.store.list(guid.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let player = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
    Session {
        account: Some(a.claim_session(account).unwrap()),
        player: Some(player),
        ..Session::default()
    }
}

#[tokio::test]
async fn tutorial_chat_reconnect_preserves_the_character_and_rejects_full_entry_before_departure() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    let mut session = loaded(&a, "tutorial-command").await;
    let player = session.player.unwrap();
    let (tx, mut rx) = mpsc::channel(8);
    let (lost, _) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    let before = a
        .world
        .call(move |w| {
            let p = w.players.get_mut(&player).unwrap();
            p.character.hp -= 1;
            p.tx = Some(output);
            Ok(p.character.clone())
        })
        .await
        .unwrap();
    let mut body = Writer::new();
    body.utf(" /TuToRiAl ");
    a.world
        .in_world(-1)
        .call(|w| {
            w.max_players = 0;
            Ok(())
        })
        .await
        .unwrap();
    handle(
        &mut a,
        Packet {
            id: wire::PLAYER_TEXT,
            body: body.0.clone(),
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let reply = rx.try_recv().unwrap();
    assert_eq!((reply.id, reply.body), {
        let p = crate::world::chat_info("Destination is full.");
        (p.id, p.body)
    });
    assert_eq!(session.player, Some(player));
    assert!(session.account.is_some() && !session.reconnecting);
    assert!(
        a.world
            .call(move |w| Ok(w.players.contains_key(&player)))
            .await
            .unwrap()
    );
    a.world
        .in_world(-1)
        .call(|w| {
            w.max_players = 85;
            Ok(())
        })
        .await
        .unwrap();
    handle(
        &mut a,
        Packet {
            id: wire::PLAYER_TEXT,
            body: body.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let reply = rx.try_recv().unwrap();
    assert_eq!(reply.id, 39);
    let mut r = Reader::new(&reply.body);
    assert_eq!(r.utf().unwrap(), "Tutorial");
    assert_eq!(r.utf().unwrap(), "");
    assert_eq!(r.i32().unwrap(), 2050);
    assert_eq!(r.i32().unwrap(), -1);
    let time = r.i32().unwrap();
    let count = r.i16().unwrap();
    assert_eq!(count, 16);
    let key = r.take(count as usize).unwrap().to_vec();
    r.done().unwrap();
    assert!(session.player.is_none() && session.account.is_none() && session.reconnecting);
    assert!(rx.try_recv().is_err());
    assert!(
        !a.world
            .call(move |w| Ok(w.players.contains_key(&player)))
            .await
            .unwrap()
    );
    assert!(a.owners.lock().unwrap().is_empty());
    let saved = a.store.load(before.account, before.id).await.unwrap();
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    a.world
        .directory(move |d| {
            assert!(
                d.consume(before.account + 1, -1, time, key.clone())
                    .is_err()
            );
            assert_eq!(d.consume(before.account, -1, time, key.clone())?, before.id);
            assert!(d.consume(before.account, -1, time, key).is_err());
            Ok(())
        })
        .await
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn rejected_stale_and_foreign_useitem_then_ground_and_owned_potions_commit() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    let mut session = loaded(&a, "potion-owner").await;
    let mut foreign = loaded(&a, "potion-foreign").await;
    let player = session.player.unwrap();
    let other = foreign.player.unwrap();
    let (bag, before, foreign_before) = a
        .world
        .call(move |w| {
            let p = w.players.get_mut(&player).unwrap();
            p.character.hp = 20;
            let before = p.character.clone();
            assert_eq!(before.items[4], 0xa22);
            let pos = p.pos;
            let bag = w.spawn_entity(0x500, pos, vec![0xa22, -1, -1, -1, -1, -1, -1, -1])?;
            Ok((bag, before, w.players[&other].character.clone()))
        })
        .await
        .unwrap();
    let (tx, mut rx) = mpsc::channel(8);
    let (lost, _) = tokio::sync::watch::channel(false);
    for (object, slot) in [(other, 4), (bag, 1), (bag, 8)] {
        handle(
            &mut a,
            use_item(object, slot),
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await
        .unwrap();
        let chat = rx.try_recv().unwrap();
        let expected = crate::world::chat_error("item ownership mismatch");
        assert_eq!((chat.id, chat.body), (expected.id, expected.body));
        let result = rx.try_recv().unwrap();
        assert_eq!((result.id, result.body), (4, 1i32.to_be_bytes().to_vec()));
        assert!(rx.try_recv().is_err());
        let (current, other_current, items, reserved) = a
            .world
            .call(move |w| {
                Ok((
                    w.players[&player].character.clone(),
                    w.players[&other].character.clone(),
                    w.entities[&bag].items.clone(),
                    w.entities[&bag].reserved,
                ))
            })
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(current).unwrap(),
            serde_json::to_value(&before).unwrap()
        );
        assert_eq!(
            serde_json::to_value(other_current).unwrap(),
            serde_json::to_value(&foreign_before).unwrap()
        );
        assert_eq!(items, vec![0xa22, -1, -1, -1, -1, -1, -1, -1]);
        assert!(!reserved);
        let saved = a.store.load(before.account, before.id).await.unwrap();
        assert_eq!(
            (saved.items, saved.revision),
            (before.items.clone(), before.revision)
        );
    }
    handle(
        &mut a,
        use_item(bag, 0),
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let (ground, contents, reserved) = a
        .world
        .call(move |w| {
            Ok((
                w.players[&player].character.clone(),
                w.entities[&bag].items.clone(),
                w.entities[&bag].reserved,
            ))
        })
        .await
        .unwrap();
    assert_eq!(ground.items, before.items);
    assert_eq!(ground.revision, before.revision + 1);
    assert!(ground.hp > before.hp);
    assert_eq!(contents[0], -1);
    assert!(!reserved);
    let saved = a.store.load(before.account, before.id).await.unwrap();
    assert_eq!(
        (saved.items, saved.revision),
        (ground.items, ground.revision)
    );
    handle(
        &mut a,
        use_item(player, 4),
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let after = a
        .world
        .call(move |w| Ok(w.players[&player].character.clone()))
        .await
        .unwrap();
    assert_eq!(after.items[4], -1);
    assert!(after.hp > before.hp);
    assert_eq!(after.revision, before.revision + 2);
    let saved = a.store.load(before.account, before.id).await.unwrap();
    assert_eq!((saved.items, saved.revision), (after.items, after.revision));
    for s in [&mut session, &mut foreign] {
        depart(&a, s.player.take().unwrap()).await.unwrap();
        s.account.take().unwrap().release();
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn rejected_useitem_chat_and_invresult_queue_failures_stay_typed_fatal() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    let mut session = loaded(&a, "queue-potion").await;
    for (closed, prefilled) in [(false, true), (false, false), (true, false)] {
        let (tx, rx) = mpsc::channel(1);
        let mut rx = if closed {
            drop(rx);
            None
        } else {
            Some(rx)
        };
        if prefilled {
            tx.try_send(Packet {
                id: 255,
                body: vec![],
            })
            .unwrap();
        }
        let (lost, _) = tokio::sync::watch::channel(false);
        let error = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            handle(
                &mut a,
                use_item(i32::MAX, 4),
                &mut session,
                &tx,
                &lost,
                std::time::Instant::now(),
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<ReplyQueueError>(),
            Some(&if closed {
                ReplyQueueError::Closed
            } else {
                ReplyQueueError::Full
            })
        );
        if let Some(rx) = rx.as_mut() {
            assert_eq!(
                rx.try_recv().unwrap().id,
                if prefilled { 255 } else { wire::TEXT }
            );
            assert!(rx.try_recv().is_err());
        }
    }
    depart(&a, session.player.take().unwrap()).await.unwrap();
    session.account.take().unwrap().release();
    std::fs::remove_dir_all(dir).unwrap();
}
