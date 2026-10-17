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
async fn local_player_text_uses_live_distance_without_narrowing_other_audiences() {
    for world in [-2, -3] {
        let (dir, mut a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        a.world = a.world.in_world(if world == -2 { -3 } else { -2 });
        let remote = loaded(&a, "local-remote").await;
        let remote_id = remote.player.unwrap();
        let (remote_tx, mut remote_rx) = mpsc::channel(64);
        let (lost, _) = tokio::sync::watch::channel(false);
        let remote_output = crate::world::Output {
            packets: remote_tx,
            lost: lost.clone(),
        };
        a.world
            .call(move |w| {
                w.players.get_mut(&remote_id).unwrap().tx = Some(remote_output);
                Ok(())
            })
            .await
            .unwrap();
        a.world = a.world.in_world(world);
        let mut sender = loaded(&a, "local-sender").await;
        let near = loaded(&a, "local-near").await;
        let far = loaded(&a, "local-far").await;
        let id = sender.player.unwrap();
        let near_id = near.player.unwrap();
        let far_id = far.player.unwrap();
        let account = sender.account.as_ref().unwrap().id;
        let (tx, mut rx) = mpsc::channel(64);
        let (near_tx, mut near_rx) = mpsc::channel(64);
        let (far_tx, mut far_rx) = mpsc::channel(64);
        let outputs = [tx.clone(), near_tx, far_tx].map(|packets| crate::world::Output {
            packets,
            lost: lost.clone(),
        });
        let pos = a
            .world
            .call(move |w| {
                for (id, output) in [id, near_id, far_id].into_iter().zip(outputs) {
                    w.players.get_mut(&id).unwrap().tx = Some(output);
                }
                w.players.get_mut(&id).unwrap().character.name = "Speaker".into();
                let pos = w.players[&id].pos;
                let p = w.players.get_mut(&near_id).unwrap();
                p.pos = crate::world::Pos {
                    x: pos.x + 1.,
                    y: pos.y,
                };
                p.known.clear();
                w.players.get_mut(&far_id).unwrap().known.insert(id);
                Ok(pos)
            })
            .await
            .unwrap();
        a.world
            .for_player(remote_id)
            .call(move |w| {
                w.players.get_mut(&remote_id).unwrap().pos = pos;
                Ok(())
            })
            .await
            .unwrap();
        let packet = |text: &str| {
            let mut body = Writer::new();
            body.utf(text);
            Packet {
                id: wire::PLAYER_TEXT,
                body: body.0,
            }
        };
        let frame = |name: &str, object: i32, recipient: &str, text: &str| {
            let mut body = Writer::new();
            body.utf(name)
                .i32(object)
                .i32(0)
                .u8(if object < 0 { 0 } else { 5 })
                .utf(recipient)
                .utf(text)
                .utf("");
            Packet {
                id: wire::TEXT,
                body: body.0,
            }
        };
        let check = |actual: Packet, expected: &Packet| {
            assert_eq!(
                (actual.id, actual.body),
                (expected.id, expected.body.clone())
            );
        };
        let now = std::time::Instant::now();
        for (i, (dx, dy, reaches)) in [
            (14.999, 0., true),
            (15., 0., false),
            (15.001, 0., false),
            (-14.999, 0., true),
            (-15., 0., false),
            (0., 14.999, true),
            (0., 15., false),
            (12., 8., true),
            (9., 12., false),
            (11., 11., false),
            (20., 0., false),
            (1., 0., true),
        ]
        .into_iter()
        .enumerate()
        {
            a.world
                .call(move |w| {
                    w.players.get_mut(&far_id).unwrap().pos = crate::world::Pos {
                        x: pos.x + dx,
                        y: pos.y + dy,
                    };
                    Ok(())
                })
                .await
                .unwrap();
            let body = format!("  <Guest1>: /yell café {i}  ");
            handle(
                &mut a,
                packet(&body),
                &mut sender,
                &tx,
                &lost,
                now + std::time::Duration::from_secs(i as u64 * 2),
            )
            .await
            .unwrap();
            let expected = frame("Speaker", id, "", &body);
            check(rx.try_recv().unwrap(), &expected);
            check(near_rx.try_recv().unwrap(), &expected);
            if reaches {
                check(far_rx.try_recv().unwrap(), &expected);
            }
            assert!(
                rx.try_recv().is_err()
                    && near_rx.try_recv().is_err()
                    && far_rx.try_recv().is_err()
                    && remote_rx.try_recv().is_err(),
                "world {world}, offset ({dx}, {dy})"
            );
        }
        a.world
            .call(move |w| {
                w.players.get_mut(&id).unwrap().pos.x = pos.x + 20.;
                Ok(())
            })
            .await
            .unwrap();
        handle(
            &mut a,
            packet("moved speaker"),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(98),
        )
        .await
        .unwrap();
        check(
            rx.try_recv().unwrap(),
            &frame("Speaker", id, "", "moved speaker"),
        );
        assert!(
            rx.try_recv().is_err()
                && near_rx.try_recv().is_err()
                && far_rx.try_recv().is_err()
                && remote_rx.try_recv().is_err()
        );
        a.world
            .call(move |w| {
                w.players.get_mut(&id).unwrap().pos = pos;
                w.players.get_mut(&far_id).unwrap().pos.x = pos.x + 20.;
                Ok(())
            })
            .await
            .unwrap();
        if world == -2 {
            handle(
                &mut a,
                packet("/yell whole Nexus"),
                &mut sender,
                &tx,
                &lost,
                now + std::time::Duration::from_secs(100),
            )
            .await
            .unwrap();
            let expected = frame("Speaker", id, "", "whole Nexus");
            for actual in [
                rx.try_recv().unwrap(),
                near_rx.try_recv().unwrap(),
                far_rx.try_recv().unwrap(),
            ] {
                check(actual, &expected);
            }
            assert!(remote_rx.try_recv().is_err());
        }
        handle(
            &mut a,
            packet("/tell Guest4 still private"),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(102),
        )
        .await
        .unwrap();
        let expected = frame("Speaker", -1, "Guest4", "still private");
        check(rx.try_recv().unwrap(), &expected);
        check(far_rx.try_recv().unwrap(), &expected);
        assert!(near_rx.try_recv().is_err() && remote_rx.try_recv().is_err());
        for session in [&sender, &far, &remote] {
            let account = session.account.as_ref().unwrap().id;
            let mut presentation = a.store.presentation(account).await.unwrap();
            presentation.guild = Some(crate::store::GuildMembership {
                id: 7,
                name: "Local test guild".into(),
                rank: 0,
            });
            a.world
                .directory(move |d| d.sync_guild(account, presentation, false))
                .await
                .unwrap();
        }
        handle(
            &mut a,
            packet("/g still cross-world"),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(104),
        )
        .await
        .unwrap();
        let expected = frame("Speaker", -1, "*Guild*", "still cross-world");
        for actual in [
            rx.try_recv().unwrap(),
            far_rx.try_recv().unwrap(),
            remote_rx.try_recv().unwrap(),
        ] {
            check(actual, &expected);
        }
        assert!(near_rx.try_recv().is_err());
        assert_eq!(
            a.world
                .directory(|d| Ok(d.announce("Still an announcement")))
                .await
                .unwrap(),
            4
        );
        let expected = frame("@ANNOUNCEMENT", -1, "", "Still an announcement");
        for actual in [
            rx.try_recv().unwrap(),
            near_rx.try_recv().unwrap(),
            far_rx.try_recv().unwrap(),
            remote_rx.try_recv().unwrap(),
        ] {
            check(actual, &expected);
        }
        for mode in 0..5 {
            a.world
                .call(move |w| {
                    let ignored = if mode == 0 {
                        std::collections::BTreeSet::from([account])
                    } else {
                        std::collections::BTreeSet::new()
                    };
                    w.sync_account_lists(near_id, Some([Default::default(), ignored]))?;
                    let p = w.players.get_mut(&near_id).unwrap();
                    p.departing = mode == 1;
                    p.character.hp = if mode == 2 { 0 } else { 100 };
                    p.death_pending = mode == 3;
                    p.character.alive = mode != 4;
                    Ok(())
                })
                .await
                .unwrap();
            for _ in 0..2 {
                assert_eq!(near_rx.try_recv().unwrap().id, wire::ACCOUNT_LIST);
            }
            handle(
                &mut a,
                packet("filtered local"),
                &mut sender,
                &tx,
                &lost,
                now + std::time::Duration::from_secs(200 + mode * 2),
            )
            .await
            .unwrap();
            check(
                rx.try_recv().unwrap(),
                &frame("Speaker", id, "", "filtered local"),
            );
            assert!(
                rx.try_recv().is_err()
                    && near_rx.try_recv().is_err()
                    && far_rx.try_recv().is_err()
                    && remote_rx.try_recv().is_err()
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn nexus_yell_player_text_preserves_local_frames_audience_and_guards() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let mut remote = loaded(&a, "yell-remote").await;
    let (remote_tx, mut remote_rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let remote_output = crate::world::Output {
        packets: remote_tx.clone(),
        lost: lost.clone(),
    };
    let remote_id = remote.player.unwrap();
    a.world
        .call(move |w| {
            w.players.get_mut(&remote_id).unwrap().tx = Some(remote_output);
            Ok(())
        })
        .await
        .unwrap();
    a.world = a.world.in_world(-2);
    let mut sender = loaded(&a, "yell-sender").await;
    let far = loaded(&a, "yell-far").await;
    let id = sender.player.unwrap();
    let far_id = far.player.unwrap();
    let account = sender.account.as_ref().unwrap().id;
    let (tx, mut rx) = mpsc::channel(64);
    let (far_tx, mut far_rx) = mpsc::channel(64);
    let outputs = [tx.clone(), far_tx].map(|packets| crate::world::Output {
        packets,
        lost: lost.clone(),
    });
    a.world
        .call(move |w| {
            let pos = w.players[&id].pos;
            let far_pos = crate::world::Pos {
                x: pos.x + 20.,
                y: pos.y,
            };
            assert!(w.walkable(far_pos));
            for (id, output) in [id, far_id].into_iter().zip(outputs) {
                w.players.get_mut(&id).unwrap().tx = Some(output);
            }
            let p = w.players.get_mut(&far_id).unwrap();
            p.pos = far_pos;
            p.known.clear();
            Ok(())
        })
        .await
        .unwrap();
    let packet = |text: &str| {
        let mut body = Writer::new();
        body.utf(text);
        Packet {
            id: wire::PLAYER_TEXT,
            body: body.0,
        }
    };
    let check = |actual: Packet, expected: &Packet| {
        assert_eq!(
            (actual.id, actual.body),
            (expected.id, expected.body.clone())
        )
    };
    let now = std::time::Instant::now();
    for (i, (input, body, reaches_far)) in [
        (
            " /YeLl  /yell <Guest1>: café  ".to_string(),
            "/yell <Guest1>: café".to_string(),
            true,
        ),
        (
            "  normal /yell hi  ".to_string(),
            "  normal /yell hi  ".to_string(),
            false,
        ),
        (format!("/yell {}", "é".repeat(122)), "é".repeat(122), true),
    ]
    .into_iter()
    .enumerate()
    {
        handle(
            &mut a,
            packet(&input),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(i as u64 * 2),
        )
        .await
        .unwrap();
        let mut expected = Writer::new();
        expected
            .utf("Guest2")
            .i32(id)
            .i32(0)
            .u8(5)
            .utf("")
            .utf(&body)
            .utf("");
        let expected = Packet {
            id: wire::TEXT,
            body: expected.0,
        };
        check(rx.try_recv().unwrap(), &expected);
        if reaches_far {
            check(far_rx.try_recv().unwrap(), &expected);
        }
        assert!(
            rx.try_recv().is_err() && far_rx.try_recv().is_err() && remote_rx.try_recv().is_err()
        );
    }
    for (i, (input, error)) in [
        ("/yell".to_string(), "Usage: /yell <message>"),
        ("/yell   ".to_string(), "Usage: /yell <message>"),
        ("/yelling hi".to_string(), "Unknown chat command."),
        (
            "/yell hello\n".to_string(),
            "Chat message contains control characters.",
        ),
        (
            "/yell \u{202e}spoof".to_string(),
            "Chat message contains control characters.",
        ),
        (
            format!("/yell {}", "a".repeat(123)),
            "Chat messages are limited to 128 characters.",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        handle(
            &mut a,
            packet(&input),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(20 + i as u64 * 2),
        )
        .await
        .unwrap();
        check(rx.try_recv().unwrap(), &crate::world::chat_error(error));
        assert!(
            rx.try_recv().is_err() && far_rx.try_recv().is_err() && remote_rx.try_recv().is_err()
        );
    }
    a.world = a.world.for_player(remote_id);
    handle(
        &mut a,
        packet("/yell no relay"),
        &mut remote,
        &remote_tx,
        &lost,
        now,
    )
    .await
    .unwrap();
    check(
        remote_rx.try_recv().unwrap(),
        &crate::world::chat_error("You can only /yell in the Nexus."),
    );
    assert!(rx.try_recv().is_err() && far_rx.try_recv().is_err() && remote_rx.try_recv().is_err());
    a.world = a.world.for_player(id);
    for mode in 0..3 {
        a.world
            .call(move |w| {
                let ignored = if mode == 0 {
                    std::collections::BTreeSet::from([account])
                } else {
                    std::collections::BTreeSet::new()
                };
                w.sync_account_lists(far_id, Some([Default::default(), ignored]))?;
                w.players.get_mut(&far_id).unwrap().departing = mode == 1;
                w.players.get_mut(&id).unwrap().departing = mode == 2;
                Ok(())
            })
            .await
            .unwrap();
        for _ in 0..2 {
            assert_eq!(far_rx.try_recv().unwrap().id, wire::ACCOUNT_LIST);
        }
        handle(
            &mut a,
            packet("/yell filtered"),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(40 + mode * 2),
        )
        .await
        .unwrap();
        if mode < 2 {
            let reply = rx.try_recv().unwrap();
            assert_eq!(reply.id, wire::TEXT);
            let mut r = Reader::new(&reply.body);
            assert_eq!(r.utf().unwrap(), "Guest2");
        }
        assert!(
            rx.try_recv().is_err() && far_rx.try_recv().is_err() && remote_rx.try_recv().is_err()
        );
    }
    a.world
        .call(move |w| {
            w.players.get_mut(&id).unwrap().departing = false;
            Ok(())
        })
        .await
        .unwrap();
    for input in ["/who", "ordinary", "/yell allowed", "/yell throttled"] {
        handle(
            &mut a,
            packet(input),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(100),
        )
        .await
        .unwrap();
    }
    assert_eq!(std::iter::from_fn(|| far_rx.try_recv().ok()).count(), 1);
    for _ in 0..3 {
        assert_eq!(rx.try_recv().unwrap().id, wire::TEXT);
    }
    check(
        rx.try_recv().unwrap(),
        &crate::world::chat_error("Chat rate limit: wait one second before sending again."),
    );
    let mut malformed = packet("/yell bad packet");
    malformed.body.push(0);
    assert!(
        handle(&mut a, malformed, &mut sender, &tx, &lost, now)
            .await
            .is_err()
    );
    sender.player = None;
    assert!(
        handle(
            &mut a,
            packet("/yell unloaded"),
            &mut sender,
            &tx,
            &lost,
            now
        )
        .await
        .is_err()
    );
    assert!(rx.try_recv().is_err() && far_rx.try_recv().is_err() && remote_rx.try_recv().is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn named_trade_player_text_matches_native_requests_and_reciprocal_starts() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let _remote = loaded(&a, "trade-remote").await;
    a.world = a.world.in_world(-2);
    let mut sender = loaded(&a, "trade-sender").await;
    let mut peer = loaded(&a, "trade-peer").await;
    let id = sender.player.unwrap();
    let target = peer.player.unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let (peer_tx, mut peer_rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let outputs = [tx.clone(), peer_tx.clone()].map(|packets| crate::world::Output {
        packets,
        lost: lost.clone(),
    });
    let before = a
        .world
        .call(move |w| {
            let pos = w.players[&id].pos;
            w.players.get_mut(&target).unwrap().pos = crate::world::Pos {
                x: pos.x + 1.,
                y: pos.y,
            };
            for (player, output) in [id, target].into_iter().zip(outputs) {
                w.players.get_mut(&player).unwrap().tx = Some(output);
            }
            Ok([id, target].map(|id| w.players[&id].character.items.clone()))
        })
        .await
        .unwrap();
    let packet = |kind, text: &str| {
        let mut body = Writer::new();
        body.utf(text);
        Packet {
            id: kind,
            body: body.0,
        }
    };
    let now = std::time::Instant::now();
    let mut native_starts = Vec::new();
    for (round, (first, reciprocal)) in [
        (
            packet(21, "Guest3"),
            packet(wire::PLAYER_TEXT, " /TrAdE Guest2 "),
        ),
        (
            packet(wire::PLAYER_TEXT, " /trade Guest3 "),
            packet(21, "Guest2"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let received = now + std::time::Duration::from_secs(round as u64 * 4);
        handle(&mut a, first, &mut sender, &tx, &lost, received)
            .await
            .unwrap();
        let request = peer_rx.try_recv().unwrap();
        assert_eq!(request.id, 61);
        let mut body = Reader::new(&request.body);
        assert_eq!(body.utf().unwrap(), "Guest2");
        body.done().unwrap();
        assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
        handle(&mut a, reciprocal, &mut peer, &peer_tx, &lost, received)
            .await
            .unwrap();
        for (side, output) in [&mut rx, &mut peer_rx].into_iter().enumerate() {
            let start = output.try_recv().unwrap();
            assert_eq!(start.id, 67);
            if round == 0 {
                native_starts.push(start.body);
            } else {
                assert_eq!(start.body, native_starts[side]);
            }
            assert!(output.try_recv().is_err());
        }
        a.world
            .call(move |w| {
                assert!(w.cancel_trade(id, "test reset"));
                Ok(())
            })
            .await
            .unwrap();
        for output in [&mut rx, &mut peer_rx] {
            assert_eq!(output.try_recv().unwrap().id, 12);
            assert!(output.try_recv().is_err());
        }
    }
    for (i, (text, guard, expected)) in [
        ("/trade", 0, "Usage: /trade <player name>"),
        ("/trade Guest3 extra", 0, "Usage: /trade <player name>"),
        (
            "/trade Nobody",
            0,
            "trade target missing, ambiguous or self",
        ),
        (
            "/trade Guest1",
            0,
            "trade target missing, ambiguous or self",
        ),
        (
            "/trade Guest2",
            0,
            "trade target missing, ambiguous or self",
        ),
        (
            "/trade guest3",
            0,
            "trade target missing, ambiguous or self",
        ),
        ("/trade AAAAAAAAAAAAAAAAA", 0, "invalid trade target name"),
        ("/trade Guest3", 1, "trade target out of range"),
        (
            "/trade Guest3",
            2,
            "trade player busy, dead, departing or quarantined",
        ),
        (
            "/trade Guest3",
            3,
            "trade player busy, dead, departing or quarantined",
        ),
        (
            "/trade Guest3",
            4,
            "trade player busy, dead, departing or quarantined",
        ),
        (
            "/trade Guest3\n",
            0,
            "Chat message contains control characters.",
        ),
        ("/trades Guest3", 0, "Unknown chat command."),
    ]
    .into_iter()
    .enumerate()
    {
        a.world
            .call(move |w| {
                let pos = w.players[&id].pos;
                w.players.get_mut(&id).unwrap().pending = guard == 2;
                let p = w.players.get_mut(&target).unwrap();
                p.pos = crate::world::Pos {
                    x: pos.x + if guard == 1 { 2. } else { 1. },
                    y: pos.y,
                };
                p.pending = guard == 3;
                p.departing = guard == 4;
                Ok(())
            })
            .await
            .unwrap();
        handle(
            &mut a,
            packet(wire::PLAYER_TEXT, text),
            &mut sender,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(10 + i as u64 * 2),
        )
        .await
        .unwrap();
        let reply = rx.try_recv().unwrap();
        let expected = crate::world::chat_error(expected);
        assert_eq!(
            (reply.id, reply.body),
            (expected.id, expected.body),
            "{text}"
        );
        assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    }
    let received = now + std::time::Duration::from_secs(100);
    for text in ["/who", "/trade Guest3", "/who", "/trade Guest3"] {
        handle(
            &mut a,
            packet(wire::PLAYER_TEXT, text),
            &mut sender,
            &tx,
            &lost,
            received,
        )
        .await
        .unwrap();
    }
    for _ in 0..2 {
        assert_eq!(rx.try_recv().unwrap().id, wire::TEXT);
    }
    let reply = rx.try_recv().unwrap();
    let expected =
        crate::world::chat_error("Chat rate limit: wait one second before sending again.");
    assert_eq!((reply.id, reply.body), (expected.id, expected.body));
    assert_eq!(peer_rx.try_recv().unwrap().id, 61);
    assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    let mut malformed = packet(wire::PLAYER_TEXT, "/trade Guest3");
    malformed.body.push(0);
    assert!(
        handle(&mut a, malformed, &mut sender, &tx, &lost, received)
            .await
            .is_err()
    );
    sender.player = None;
    assert!(
        handle(
            &mut a,
            packet(wire::PLAYER_TEXT, "/trade Guest3"),
            &mut sender,
            &tx,
            &lost,
            received
        )
        .await
        .is_err()
    );
    assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    a.world
        .call(move |w| {
            assert_eq!(
                [id, target].map(|id| w.players[&id].character.items.clone()),
                before
            );
            Ok(())
        })
        .await
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn named_teleport_player_text_reuses_native_outputs_and_account_cooldown() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let _remote = loaded(&a, "teleport-remote").await;
    a.world = a.world.in_world(-2);
    let mut session = loaded(&a, "teleport-sender").await;
    let peer = loaded(&a, "teleport-peer").await;
    let id = session.player.unwrap();
    let target = peer.player.unwrap();
    let (tx, mut rx) = mpsc::channel(128);
    let (peer_tx, mut peer_rx) = mpsc::channel(128);
    let (lost, _) = tokio::sync::watch::channel(false);
    let outputs = [tx.clone(), peer_tx].map(|packets| crate::world::Output {
        packets,
        lost: lost.clone(),
    });
    let (before, destination) = a
        .world
        .call(move |w| {
            for (player, output) in [id, target].into_iter().zip(outputs) {
                let p = w.players.get_mut(&player).unwrap();
                p.tx = Some(output);
                p.known.extend([id, target]);
            }
            Ok((w.players[&id].pos, w.players[&target].pos))
        })
        .await
        .unwrap();
    assert!(before.distance(destination) > 0.);
    let chat = |text: &str| {
        let mut body = Writer::new();
        body.utf(text);
        Packet {
            id: wire::PLAYER_TEXT,
            body: body.0,
        }
    };
    let native = || Packet {
        id: wire::TELEPORT,
        body: target.to_be_bytes().to_vec(),
    };
    let now = std::time::Instant::now();
    for (packet, seconds, success) in [
        (chat(" /TeLePoRt gUeSt3 "), 0, true),
        (native(), 1, false),
        (native(), 10, true),
        (chat("/teleport Guest3"), 11, false),
    ] {
        handle(
            &mut a,
            packet,
            &mut session,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(seconds),
        )
        .await
        .unwrap();
        if success {
            for output in [&mut rx, &mut peer_rx] {
                let packets: Vec<_> = std::iter::from_fn(|| output.try_recv().ok()).collect();
                assert!(!packets.iter().any(|p| p.id == wire::TEXT));
                let goto = packets.iter().find(|p| p.id == wire::GOTO).unwrap();
                let mut body = Reader::new(&goto.body);
                assert_eq!(body.i32().unwrap(), id);
                assert_eq!(
                    (body.f32().unwrap(), body.f32().unwrap()),
                    (destination.x, destination.y)
                );
                body.done().unwrap();
                assert!(
                    packets
                        .iter()
                        .any(|p| p.id == wire::SHOW_EFFECT && p.body[0] == 2)
                );
            }
            a.world
                .call(move |w| w.acknowledge_goto(id, 0))
                .await
                .unwrap();
        } else {
            let reply = rx.try_recv().unwrap();
            let expected =
                crate::world::chat_error("Too soon to teleport again; wait ten seconds.");
            assert_eq!((reply.id, reply.body), (expected.id, expected.body));
            assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
        }
        a.world
            .call(move |w| {
                assert_eq!(w.players[&id].pos.distance(destination), 0.);
                assert_eq!(w.players[&target].pos.distance(destination), 0.);
                Ok(())
            })
            .await
            .unwrap();
    }
    for (i, (text, expected)) in [
        ("/teleport", "Usage: /teleport <player name>"),
        ("/teleport Guest3 extra", "Usage: /teleport <player name>"),
        ("/teleport Nobody", "Player not found in this world."),
        ("/teleport Guest1", "Player not found in this world."),
        ("/teleport Guest2", "You are already at yourself."),
        (
            "/teleport Guest3\n",
            "Chat message contains control characters.",
        ),
        ("/tp Guest3", "Unknown chat command."),
        ("/teleportation Guest3", "Unknown chat command."),
    ]
    .into_iter()
    .enumerate()
    {
        handle(
            &mut a,
            chat(text),
            &mut session,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(20 + i as u64 * 2),
        )
        .await
        .unwrap();
        let reply = rx.try_recv().unwrap();
        let expected = crate::world::chat_error(expected);
        assert_eq!((reply.id, reply.body), (expected.id, expected.body));
        assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    }
    let mut malformed = chat("/teleport Guest3");
    malformed.body.push(0);
    assert!(
        handle(&mut a, malformed, &mut session, &tx, &lost, now)
            .await
            .is_err()
    );
    session.player = None;
    assert!(
        handle(
            &mut a,
            chat("/teleport Guest3"),
            &mut session,
            &tx,
            &lost,
            now
        )
        .await
        .is_err()
    );
    assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn server_player_text_reports_current_world_privately_without_gameplay_changes() {
    use std::time::{Duration, Instant};
    fn packet(text: &str) -> Packet {
        let mut out = Writer::new();
        out.utf(text);
        Packet {
            id: wire::PLAYER_TEXT,
            body: out.0,
        }
    }
    fn reply(packet: Packet, name: &str, text: &str) {
        assert_eq!(packet.id, wire::TEXT);
        let mut r = Reader::new(&packet.body);
        let actual_name = r.utf().unwrap();
        assert_eq!(r.i32().unwrap(), -1);
        assert_eq!(r.i32().unwrap(), 0);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.utf().unwrap(), "");
        assert_eq!(r.utf().unwrap(), text);
        assert_eq!(actual_name, name);
        assert_eq!(r.utf().unwrap(), "");
        r.done().unwrap();
    }
    fn snapshot(w: &crate::world::World, id: i32) -> serde_json::Value {
        let p = &w.players[&id];
        serde_json::json!({"character": p.character, "position": p.pos,
            "pending": p.pending, "xp_queued": p.experience_queued,
            "world": w.name, "entities": w.entities.len(), "projectiles": w.bullets.len()})
    }
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    let mut session = loaded(&a, "server-info-sender").await;
    let peer = loaded(&a, "server-info-peer").await;
    let remote = loaded(
        &App {
            world: a.world.in_world(-3),
            ..a.clone()
        },
        "server-info-remote",
    )
    .await;
    let mut id = session.player.unwrap();
    let peer_id = peer.player.unwrap();
    let remote_id = remote.player.unwrap();
    let account = session.account.as_ref().unwrap().id;
    let (tx, mut rx) = mpsc::channel(16);
    let (peer_tx, mut peer_rx) = mpsc::channel(16);
    let (remote_tx, mut remote_rx) = mpsc::channel(16);
    let (lost, _) = tokio::sync::watch::channel(false);
    for (player, packets) in [(id, tx.clone()), (peer_id, peer_tx), (remote_id, remote_tx)] {
        let output = crate::world::Output {
            packets,
            lost: lost.clone(),
        };
        a.world
            .for_player(player)
            .call(move |w| {
                w.players.get_mut(&player).unwrap().tx = Some(output);
                Ok(())
            })
            .await
            .unwrap();
    }
    a.world
        .call(move |w| {
            w.players.get_mut(&peer_id).unwrap().pos.x += 100.;
            w.sync_account_lists(peer_id, Some([Default::default(), [account].into()]))
        })
        .await
        .unwrap();
    while peer_rx.try_recv().is_ok() {}
    a.world
        .directory(move |d| d.chat(id, "/pause".into(), Instant::now()))
        .await
        .unwrap();
    reply(rx.try_recv().unwrap(), "", "Game paused.");
    let mut now = Instant::now();
    for (text, name, expected) in [
        (" /SeRvEr ".into(), "", "Current world: Nexus"),
        ("/server    ".into(), "", "Current world: Nexus"),
        (
            format!("/server{}", " ".repeat(121)),
            "",
            "Current world: Nexus",
        ),
        ("/server extra".into(), "*Error*", "Usage: /server"),
        (
            "/server\n".into(),
            "*Error*",
            "Chat message contains control characters.",
        ),
        (
            format!("/server{}", " ".repeat(122)),
            "*Error*",
            "Chat messages are limited to 128 characters.",
        ),
        ("/servers".into(), "*Error*", "Unknown chat command."),
        // Exact native /help is intercepted locally, not a new backend command.
        ("/help".into(), "*Error*", "Unknown chat command."),
        (
            "/guild unchanged".into(),
            "*Error*",
            "You are not in a guild.",
        ),
        (
            "/tell Guest2 unchanged".into(),
            "*Error*",
            "Choose a name before using /tell.",
        ),
    ] {
        let before = a
            .world
            .for_player(id)
            .call(move |w| Ok(snapshot(w, id)))
            .await
            .unwrap();
        handle(&mut a, packet(&text), &mut session, &tx, &lost, now)
            .await
            .unwrap();
        reply(rx.try_recv().unwrap(), name, expected);
        assert!(
            rx.try_recv().is_err() && peer_rx.try_recv().is_err() && remote_rx.try_recv().is_err()
        );
        let after = a
            .world
            .for_player(id)
            .call(move |w| Ok(snapshot(w, id)))
            .await
            .unwrap();
        assert_eq!(after, before);
        now += Duration::from_secs(2);
    }
    let ch = a
        .world
        .for_player(id)
        .call(move |w| Ok(w.players[&id].character.clone()))
        .await
        .unwrap();
    depart(&a, id).await.unwrap();
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    id = a
        .world
        .in_world(-1)
        .call(move |w| w.enter(ch, Some(output)))
        .await
        .unwrap();
    session.player = Some(id);
    a.world = a.world.for_player(id);
    while rx.try_recv().is_ok() {}
    let (expected, map_name) = a
        .world
        .for_player(id)
        .call(move |w| {
            let body = w.map_info();
            let mut r = Reader::new(&body);
            r.i32()?;
            r.i32()?;
            Ok((format!("Current world: {}", w.name), r.utf()?))
        })
        .await
        .unwrap();
    assert_eq!(map_name, "Tutorial");
    for text in ["/server", "/who", "/server"] {
        let before = a
            .world
            .for_player(id)
            .call(move |w| Ok(snapshot(w, id)))
            .await
            .unwrap();
        handle(&mut a, packet(text), &mut session, &tx, &lost, now)
            .await
            .unwrap();
        reply(
            rx.try_recv().unwrap(),
            "",
            if text == "/who" {
                "Players online: Guest1"
            } else {
                &expected
            },
        );
        let after = a
            .world
            .for_player(id)
            .call(move |w| Ok(snapshot(w, id)))
            .await
            .unwrap();
        assert_eq!(after, before);
    }
    handle(&mut a, packet("/server"), &mut session, &tx, &lost, now)
        .await
        .unwrap();
    reply(
        rx.try_recv().unwrap(),
        "*Error*",
        "Chat rate limit: wait one second before sending again.",
    );
    now += Duration::from_secs(2);
    handle(&mut a, packet("normal chat"), &mut session, &tx, &lost, now)
        .await
        .unwrap();
    let p = rx.try_recv().unwrap();
    let mut r = Reader::new(&p.body);
    assert_eq!(p.id, wire::TEXT);
    assert_eq!(r.utf().unwrap(), "Guest1");
    assert_eq!(r.i32().unwrap(), id);
    assert!(peer_rx.try_recv().is_err() && remote_rx.try_recv().is_err());
    for mode in 0..5 {
        a.world
            .for_player(id)
            .call(move |w| {
                let p = w.players.get_mut(&id).unwrap();
                p.character.alive = mode != 0;
                p.character.hp = if mode == 1 { 0 } else { 100 };
                p.death_pending = mode == 2;
                p.departing = mode == 3;
                if mode == 4 {
                    w.poison(id, "test save failure".into())?;
                }
                Ok(())
            })
            .await
            .unwrap();
        now += Duration::from_secs(2);
        handle(&mut a, packet("/server"), &mut session, &tx, &lost, now)
            .await
            .unwrap();
        if mode == 3 {
            assert!(rx.try_recv().is_err());
        } else {
            reply(
                rx.try_recv().unwrap(),
                "*Error*",
                "Dead, departing or quarantined players cannot chat.",
            );
        }
    }
    let mut malformed = packet("/server");
    malformed.body.push(0);
    assert!(
        handle(&mut a, malformed, &mut session, &tx, &lost, now)
            .await
            .is_err()
    );
    session.player = None;
    assert!(
        handle(&mut a, packet("/server"), &mut session, &tx, &lost, now)
            .await
            .is_err()
    );
    assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err() && remote_rx.try_recv().is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn who_player_text_returns_private_roster_and_validation_errors() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    let mut session = loaded(&a, "who-sender").await;
    let peer = loaded(&a, "who-peer").await;
    let (tx, mut rx) = mpsc::channel(16);
    let (peer_tx, mut peer_rx) = mpsc::channel(16);
    let (lost, _) = tokio::sync::watch::channel(false);
    let sender_output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    let peer_output = crate::world::Output {
        packets: peer_tx,
        lost: lost.clone(),
    };
    let id = session.player.unwrap();
    let peer_id = peer.player.unwrap();
    a.world
        .call(move |w| {
            w.players.get_mut(&id).unwrap().tx = Some(sender_output);
            w.players.get_mut(&peer_id).unwrap().tx = Some(peer_output);
            Ok(())
        })
        .await
        .unwrap();
    let packet = |text: &str| {
        let mut body = Writer::new();
        body.utf(text);
        Packet {
            id: wire::PLAYER_TEXT,
            body: body.0,
        }
    };
    let now = std::time::Instant::now();
    for (i, (text, expected)) in [
        (
            " /WhO ",
            crate::world::chat_info("Players online: Guest1, Guest2"),
        ),
        ("/who extra", crate::world::chat_error("Usage: /who")),
        (
            "/who\n",
            crate::world::chat_error("Chat message contains control characters."),
        ),
        (
            "/whoever",
            crate::world::chat_error("Unknown chat command."),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        handle(
            &mut a,
            packet(text),
            &mut session,
            &tx,
            &lost,
            now + std::time::Duration::from_secs(i as u64 * 2),
        )
        .await
        .unwrap();
        let reply = rx.try_recv().unwrap();
        assert_eq!((reply.id, reply.body), (expected.id, expected.body));
        assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    }
    let mut malformed = packet("/who");
    malformed.body.push(0);
    assert!(
        handle(&mut a, malformed, &mut session, &tx, &lost, now)
            .await
            .is_err()
    );
    session.player = None;
    assert!(
        handle(&mut a, packet("/who"), &mut session, &tx, &lost, now)
            .await
            .is_err()
    );
    assert!(rx.try_recv().is_err() && peer_rx.try_recv().is_err());
    std::fs::remove_dir_all(dir).unwrap();
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
