use super::*;

async fn login(
    socket: &mut NativeSocket,
    dir: &std::path::Path,
    guid: &str,
    password: &str,
    game: i32,
    character: i32,
) -> i32 {
    socket
        .send(socket_hello_password(dir, guid, password, game, None))
        .await;
    socket.receive(wire::MAP_INFO).await;
    socket
        .send(Packet {
            id: wire::LOAD,
            body: character.to_be_bytes().to_vec(),
        })
        .await;
    Reader::new(&socket.receive(wire::CREATE_SUCCESS).await.body)
        .i32()
        .unwrap()
}
fn text_packet(text: &str) -> Packet {
    let mut body = Writer::new();
    body.utf(text);
    Packet {
        id: wire::PLAYER_TEXT,
        body: body.0,
    }
}
async fn fenced(a: &App, account: i32, expected: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(error) = lifecycle::failure(a, account) {
                assert!(error.contains(expected), "wrong uncertainty: {error}");
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("unknown native failure was downgraded to a benign response");
}
async fn boundary(kind: &str, fault: &str) {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let registered = kind == "discord";
    let (guid, password, account) = if registered {
        let guid = "boundary-owner@example.test";
        let account = a
            .store
            .register("boundary-owner".into(), guid.into(), "test-password".into())
            .await
            .unwrap();
        (guid, "test-password", account)
    } else {
        (
            "boundary-owner",
            "",
            a.store.list("boundary-owner".into()).await.unwrap().id,
        )
    };
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let peer_account = a.store.list("boundary-peer".into()).await.unwrap().id;
    let peer_ch = a
        .store
        .create(peer_account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let before = a.store.presentation(account).await.unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    let id = login(&mut socket, &dir, guid, password, -2, ch.id).await;
    let mut peer_socket = NativeSocket::connect(port, &a.origin).await;
    let peer = login(
        &mut peer_socket,
        &dir,
        "boundary-peer",
        "",
        if kind == "move" { -1 } else { -2 },
        peer_ch.id,
    )
    .await;
    lifecycle::account(
        &a,
        vec![account, peer_account],
        "boundary_loads_settled",
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    for _ in 0..2 {
        socket.receive(wire::ACCOUNT_LIST).await;
    }
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    socket
        .send(super::super::lifecycle_native_tests::native_swap_packet(
            id, &ch,
        ))
        .await;
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM characters WHERE id=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        ch.revision + 1
    );
    release.send(()).unwrap();
    lifecycle::account(
        &a,
        vec![account],
        "boundary_inventory_published",
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    let moving = kind == "move";
    let (origin, destination) = a
        .world
        .for_player(id)
        .call(move |w| {
            let origin = w.players[&id].pos;
            if moving {
                for _ in 0..5 {
                    w.step();
                }
            }
            let destination = Pos {
                x: origin.x + 0.25,
                y: origin.y,
            };
            if !moving {
                assert!(w.walkable(destination));
                w.players.get_mut(&peer).unwrap().pos = destination;
                w.updates(id, false)?;
            }
            Ok((origin, destination))
        })
        .await
        .unwrap();
    if kind == "move" && fault == "unknown" {
        a.world
            .for_player(id)
            .call(move |w| {
                w.players.get_mut(&id).unwrap().character.xp_boost.bonus_exp = 1;
                Ok(())
            })
            .await
            .unwrap();
    }
    if fault == "sql" {
        let table = if kind == "discord" {
            "discord_codes"
        } else {
            "account_lists"
        };
        db.execute_batch(&format!("CREATE TRIGGER boundary_sql BEFORE INSERT ON {table} WHEN NEW.account={account} BEGIN SELECT RAISE(ABORT,'native boundary SQL failure'); END;")).unwrap();
    }
    let lookup_gate = if fault == "lost_store" && kind == "ignore" {
        Some(a.store.lifecycle_test_pause_reply())
    } else {
        if fault == "lost_store" {
            a.store.lifecycle_test_lose_reply();
        }
        None
    };
    if kind == "discord" && fault == "ordinary" {
        socket.send(text_packet("/discord")).await;
        let response = socket.receive(wire::TEXT).await;
        assert_eq!(Reader::new(&response.body).utf().unwrap(), "*Discord*");
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM discord_codes WHERE account=?1",
                [account],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            1
        );
    }
    let request = match kind {
        "direct" => Packet {
            id: wire::TELEPORT,
            body: if fault == "ordinary" { i32::MAX } else { peer }
                .to_be_bytes()
                .to_vec(),
        },
        "named" => text_packet(&format!(
            "/teleport {}",
            if fault == "ordinary" {
                "NobodyHere"
            } else {
                &peer_ch.name
            }
        )),
        "ignore" => text_packet(&format!(
            "/ignore {}",
            if fault == "ordinary" {
                "NobodyHere"
            } else {
                &peer_ch.name
            }
        )),
        "list" => {
            let mut body = Writer::new();
            body.i32(1)
                .u8(1)
                .i32(if fault == "ordinary" { i32::MAX } else { peer });
            Packet {
                id: wire::EDIT_ACCOUNT_LIST,
                body: body.0,
            }
        }
        "discord" => text_packet("/discord"),
        "move" => {
            let mut body = Writer::new();
            body.i32(0)
                .i32(1000)
                .f32(if fault == "ordinary" {
                    origin.x + 1000.
                } else {
                    origin.x
                })
                .f32(origin.y)
                .i16(0);
            Packet {
                id: wire::MOVE,
                body: body.0,
            }
        }
        _ => panic!("unknown boundary fixture"),
    };
    let (before_goto, before_lists) = a
        .world
        .for_player(id)
        .call(move |w| {
            Ok((
                w.players[&id].lifecycle_test_goto_acks(),
                w.players[&id].lifecycle_test_account_lists(),
            ))
        })
        .await
        .unwrap();
    let gate = if fault == "lost_world" {
        Some(a.world.lifecycle_test_pause_request_job(
            if matches!(kind, "ignore" | "list") {
                3
            } else {
                2
            },
            true,
        ))
    } else {
        None
    };
    socket.send(request).await;
    if let Some((ready, release)) = lookup_gate {
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM account_lists WHERE account=?1",
                [account],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            0
        );
        a.store.lifecycle_test_lose_reply();
        release.send(()).unwrap();
    }
    if fault == "lost_store" {
        let table = if kind == "discord" {
            "discord_codes"
        } else {
            "account_lists"
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if db
                    .query_row(
                        &format!("SELECT count(*) FROM {table} WHERE account=?1"),
                        [account],
                        |r| r.get::<_, i32>(0),
                    )
                    .unwrap()
                    == 1
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("lost-reply fixture did not commit the intended Store write");
    }
    if let Some((ready, release)) = gate {
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        if matches!(kind, "ignore" | "list") {
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM account_lists WHERE account=?1 AND target=?2",
                    [account, peer_account],
                    |r| r.get::<_, i32>(0)
                )
                .unwrap(),
                1
            );
        }
        a.world.lifecycle_test_lose_reply();
        release.send(()).unwrap();
        if matches!(kind, "direct" | "named") {
            a.world
                .for_player(id)
                .call(move |w| {
                    assert_eq!(
                        (w.players[&id].pos.x, w.players[&id].pos.y),
                        (destination.x, destination.y)
                    );
                    assert!(w.players[&id].known.contains(&peer));
                    assert_eq!(w.players[&id].character.account, account);
                    assert_eq!(w.players[&id].lifecycle_test_goto_acks(), before_goto + 1);
                    Ok(())
                })
                .await
                .unwrap();
        } else {
            a.world
                .for_player(id)
                .call(move |w| {
                    assert!(before_lists[1].is_empty());
                    let mut expected = before_lists;
                    expected[1].insert(peer_account);
                    assert_eq!(w.players[&id].character.account, account);
                    assert_eq!(w.players[&id].lifecycle_test_account_lists(), expected);
                    Ok(())
                })
                .await
                .unwrap();
        }
    }
    if fault == "ordinary" {
        let packet = socket
            .receive(if kind == "move" {
                wire::GOTO
            } else {
                wire::TEXT
            })
            .await;
        if kind != "move" {
            assert_eq!(
                Reader::new(&packet.body).utf().unwrap(),
                if kind == "discord" {
                    "*Discord*"
                } else {
                    "*Error*"
                }
            );
        }
        lifecycle::account(&a, vec![account], "ordinary_boundary_settled", |_| async {
            Ok(())
        })
        .await
        .unwrap();
        assert!(!lifecycle::state(&a, account).unwrap().2);
        assert!(a.claim(account).is_err());
        assert!(a.kick(account).await.unwrap());
        released(&a, account).await;
        let mut replacement = NativeSocket::connect(port, &a.origin).await;
        let replacement_id = login(&mut replacement, &dir, guid, password, -2, ch.id).await;
        assert_ne!(replacement_id, id);
        assert!(a.claim(account).is_err());
        assert_eq!(
            a.world
                .directory(move |d| Ok(d
                    .worlds
                    .values()
                    .flat_map(|w| w.players.values())
                    .filter(|p| p.character.account == account)
                    .count()))
                .await
                .unwrap(),
            1
        );
        drop(replacement);
        released(&a, account).await;
    } else {
        fenced(
            &a,
            account,
            match fault {
                "sql" => "native boundary SQL failure",
                "unknown" => "invalid XP boost state",
                _ => "worker failed",
            },
        )
        .await;
        socket.receive(0).await;
        let mut rejected = NativeSocket::connect(port, &a.origin).await;
        rejected
            .send(socket_hello_password(&dir, guid, password, -2, None))
            .await;
        rejected.receive(0).await;
        drop(rejected);
        assert!(a.claim(account).is_err());
        assert!(lifecycle::state(&a, account).unwrap().2);
        assert_eq!(
            a.world
                .directory(move |d| Ok(d
                    .worlds
                    .values()
                    .flat_map(|w| w.players.values())
                    .filter(|p| p.character.account == account)
                    .count()))
                .await
                .unwrap(),
            1
        );
        let written = i32::from(fault != "sql");
        if kind == "discord" {
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM discord_codes WHERE account=?1",
                    [account],
                    |r| r.get::<_, i32>(0)
                )
                .unwrap(),
                written
            );
        }
        if matches!(kind, "ignore" | "list") {
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM account_lists WHERE account=?1 AND target=?2",
                    [account, peer_account],
                    |r| r.get::<_, i32>(0)
                )
                .unwrap(),
                written
            );
        }
    }
    peer_socket
        .send(super::super::lifecycle_native_tests::native_swap_packet(
            peer, &peer_ch,
        ))
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if a.store
                .load(peer_account, peer_ch.id)
                .await
                .unwrap()
                .revision
                == peer_ch.revision + 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(peer_socket);
    released(&a, peer_account).await;
    let mut expected = ch.items.clone();
    expected.swap(4, 5);
    let saved = a.store.load(account, ch.id).await.unwrap();
    assert_eq!(
        (saved.revision, saved.items, saved.xp_boost.bonus_exp),
        (ch.revision + 1, expected, 0)
    );
    assert_eq!(a.store.presentation(account).await.unwrap(), before);
    drop(socket);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_teleport_ordinary_denial_continues_until_canonical_close() {
    for kind in ["direct", "named"] {
        boundary(kind, "ordinary").await;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_teleport_lost_reply_fences_direct_and_named_native_paths() {
    for kind in ["direct", "named"] {
        boundary(kind, "lost_world").await;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_social_ordinary_denials_preserve_native_session() {
    for kind in ["ignore", "list", "discord"] {
        boundary(kind, "ordinary").await;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_social_accepted_store_uncertainty_is_not_chat_success() {
    for (kind, fault) in [
        ("ignore", "lost_store"),
        ("list", "sql"),
        ("discord", "lost_store"),
        ("discord", "sql"),
    ] {
        boundary(kind, fault).await;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_social_lost_publication_preserves_committed_list_and_fence() {
    for kind in ["ignore", "list"] {
        boundary(kind, "lost_world").await;
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_move_corrects_only_rule_denials_not_unknown_invariants() {
    for fault in ["ordinary", "unknown"] {
        boundary("move", fault).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_request_gate_ignores_background_world_calls() {
    let (dir, a) = account_actions::tests::fixture();
    let (ready, release) = a.world.lifecycle_test_pause_request_job(1, true);
    a.world.call(|_| Ok(())).await.unwrap();
    let world = a.world.clone();
    let caller = tokio::spawn(async move {
        crate::performance::REQUEST
            .scope(
                Arc::new(Mutex::new(crate::performance::Requests::default())),
                world.call(|_| Ok(())),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    release.send(()).unwrap();
    caller.await.unwrap().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_full_character_capacity_is_ordinary_native_create_denial() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let guid = "boundary-capacity";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let before = a.store.presentation(account).await.unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket.send(socket_hello(&dir, guid, -2, None)).await;
    socket.receive(wire::MAP_INFO).await;
    let mut body = Writer::new();
    body.i16(WIZARD as i16);
    socket
        .send(Packet {
            id: wire::CREATE,
            body: body.0,
        })
        .await;
    socket.receive(0).await;
    released(&a, account).await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(db.query_row("SELECT character_slots,(SELECT count(*) FROM characters WHERE account=?1 AND alive=1) FROM accounts WHERE id=?1",[account],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,i32>(1)?))).unwrap(),(1,1));
    assert_eq!(a.store.load(account, ch.id).await.unwrap().items, ch.items);
    assert_eq!(a.store.presentation(account).await.unwrap(), before);
    let mut replacement = NativeSocket::connect(port, &a.origin).await;
    login(&mut replacement, &dir, guid, "", -2, ch.id).await;
    assert!(a.claim(account).is_err());
    drop(replacement);
    released(&a, account).await;
    drop(socket);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}
