use super::*;
use std::time::Duration;

async fn avatar(a: &App, name: &str) -> (Owner, i32, crate::store::Character) {
    let account = a.store.list(name.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let saved = ch.clone();
    let owner = a.claim_session(account).unwrap();
    let bind = lifecycle::binding(a, account).unwrap();
    let id = a
        .world
        .call(move |w| {
            let id = w.enter(ch, None)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    (owner, id, saved)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_duplicate_retirement_cancelled_waiter_one_sql_write() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = avatar(&a, "finalize-duplicate").await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE lifecycle_audit(n INTEGER); CREATE TRIGGER lifecycle_audit_update AFTER UPDATE ON characters BEGIN INSERT INTO lifecycle_audit VALUES(1); END;").unwrap();
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let first_app = a.clone();
    let first = tokio::spawn(async move { depart(&first_app, id).await });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM lifecycle_audit", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    let second_app = a.clone();
    let second = tokio::spawn(async move { depart(&second_app, id).await });
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(a.claim(ch.account).is_err());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    // Closing an already-retired avatar joins the same completion, not a second World lookup/write.
    depart(&a, id).await.unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM lifecycle_audit", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    assert!(
        !a.world
            .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
            .await
            .unwrap()
    );
    owner.release();
    let replacement = a.claim_session(ch.account).unwrap();
    assert!(
        depart(&a, id).await.is_err(),
        "stale avatar must not affect a new claim"
    );
    assert!(a.claim(ch.account).is_err());
    replacement.release();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_finalize_rejects_nested_entry_and_legacy_depart_keeps_custody() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = avatar(&a, "finalize-nested").await;
    lifecycle::player(&a, id, "legacy_adapter", move |app| async move {
        let error = lifecycle::finalize(&app, id, CloseReason::Controller)
            .err()
            .expect("nested entry accepted");
        assert!(error.is::<lifecycle::NestedFinalization>());
        depart(&app, id).await
    })
    .await
    .unwrap();
    assert!(
        a.claim(ch.account).is_err(),
        "retirement must not release a bot's account lease"
    );
    owner.release();
    assert!(a.owners.lock().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_native_drop_after_commit_joins_retirement_without_ambient_runtime() {
    let (dir, a) = account_actions::tests::fixture();
    let (mut owner, id, ch) = avatar(&a, "finalize-drop").await;
    owner.arm_cleanup(&a, tokio::runtime::Handle::current());
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let app = a.clone();
    let first = tokio::spawn(async move { depart(&app, id).await });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    // Drop occurs on a plain OS thread, not inside a Tokio executor.
    std::thread::spawn(move || drop(owner)).join().unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(a.claim(ch.account).is_err());
    let joined = lifecycle::finalize(&a, id, CloseReason::Shutdown).unwrap();
    release.send(()).unwrap();
    joined.wait().await.unwrap();
    assert!(a.owners.lock().unwrap().is_empty());
    let replacement = a.claim_session(ch.account).unwrap();
    replacement.release();
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_sql_failure_is_shared_and_fenced_healthy_sibling_saves() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = avatar(&a, "finalize-sql").await;
    let (healthy_owner, healthy, healthy_ch) = avatar(&a, "finalize-healthy").await;
    rusqlite::Connection::open(dir.join("game.db")).unwrap().execute_batch(&format!("CREATE TRIGGER lifecycle_finalize_sql BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'finalization SQL failure'); END;",ch.account)).unwrap();
    let first = lifecycle::finalize(&a, id, CloseReason::Transport).unwrap();
    let second = lifecycle::finalize(&a, id, CloseReason::Shutdown).unwrap();
    let a_error = first.wait().await.unwrap_err().to_string();
    let b_error = second.wait().await.unwrap_err().to_string();
    assert_eq!(a_error, b_error);
    assert!(a_error.contains("finalization SQL failure"), "{a_error}");
    owner.release();
    assert!(a.claim(ch.account).is_err());
    assert!(
        a.world
            .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
            .await
            .unwrap()
    );
    depart(&a, healthy).await.unwrap();
    healthy_owner.release();
    assert_eq!(
        a.store
            .load(healthy_ch.account, healthy_ch.id)
            .await
            .unwrap()
            .revision,
        healthy_ch.revision
    );
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    std::fs::remove_dir_all(dir).unwrap();
}

struct StalledSink;
impl futures_util::Sink<Message> for StalledSink {
    type Error = std::io::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
    fn start_send(self: std::pin::Pin<&mut Self>, _: Message) -> std::io::Result<()> {
        panic!("stalled sink unexpectedly ready")
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
}
#[tokio::test]
async fn lifecycle_finalize_stalled_send_is_interrupted_by_kick_shutdown_and_output_loss() {
    for reason in [
        CloseReason::Controller,
        CloseReason::Shutdown,
        CloseReason::Output,
    ] {
        let (lost, mut lost_rx) = tokio::sync::watch::channel(false);
        let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let (cancel, cancel_rx) = tokio::sync::watch::channel(false);
        let (_drain, mut drain_rx) = tokio::sync::watch::channel(0);
        let sender = tokio::spawn(async move {
            send_frame(
                &mut StalledSink,
                Message::Binary(vec![1].into()),
                &mut lost_rx,
                &mut shutdown_rx,
                Some(cancel_rx),
                &mut drain_rx,
            )
            .await
        });
        tokio::task::yield_now().await;
        match reason {
            CloseReason::Controller => {
                cancel.send_replace(true);
            }
            CloseReason::Shutdown => {
                shutdown.send_replace(true);
            }
            CloseReason::Output => {
                lost.send_replace(true);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), sender)
                .await
                .unwrap()
                .unwrap(),
            Err(reason)
        );
    }
}

pub(super) struct NativeSocket {
    io: reqwest::Upgraded,
    codec: Codec,
    pending: std::collections::VecDeque<Packet>,
}
impl NativeSocket {
    pub(super) async fn connect(port: u16, origin: &str) -> Self {
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{port}/game"))
            .header("origin", origin)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        Self {
            io: response.upgrade().await.unwrap(),
            codec: Codec::client(),
            pending: Default::default(),
        }
    }
    pub(super) async fn send(&mut self, packet: Packet) {
        use tokio::io::AsyncWriteExt;
        let bytes = self.codec.frame(packet.id, packet.body);
        let mut frame = vec![0x82];
        if bytes.len() < 126 {
            frame.push(0x80 | bytes.len() as u8);
        } else {
            frame.push(0x80 | 126);
            frame.extend((bytes.len() as u16).to_be_bytes());
        }
        frame.extend([0; 4]);
        frame.extend(bytes);
        self.io.write_all(&frame).await.unwrap();
    }
    pub(super) async fn receive(&mut self, expected: u8) -> Packet {
        use tokio::io::AsyncReadExt;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                while let Some(packet) = self.pending.pop_front() {
                    if packet.id == expected {
                        return packet;
                    }
                }
                let mut header = [0; 2];
                self.io.read_exact(&mut header).await.unwrap();
                assert_eq!(header[0] & 15, 2, "expected a binary WebSocket frame");
                assert_eq!(header[1] & 128, 0);
                let n = match header[1] & 127 {
                    126 => self.io.read_u16().await.unwrap() as usize,
                    127 => self.io.read_u64().await.unwrap() as usize,
                    n => n as usize,
                };
                assert!(n <= wire::MAX_FRAME);
                let mut body = vec![0; n];
                self.io.read_exact(&mut body).await.unwrap();
                self.pending.extend(self.codec.feed(&body).unwrap());
            }
        })
        .await
        .expect("native socket did not produce required packet")
    }
}
pub(super) fn socket_hello(
    dir: &std::path::Path,
    guid: &str,
    game: i32,
    transfer: Option<(i32, Vec<u8>)>,
) -> Packet {
    socket_hello_password(dir, guid, "", game, transfer)
}
fn socket_hello_password(
    dir: &std::path::Path,
    guid: &str,
    password: &str,
    game: i32,
    transfer: Option<(i32, Vec<u8>)>,
) -> Packet {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, pkcs8::DecodePrivateKey};
    let key =
        RsaPrivateKey::from_pkcs8_pem(&std::fs::read_to_string(dir.join("server.pem")).unwrap())
            .unwrap();
    let guid = STANDARD.encode(
        key.to_public_key()
            .encrypt(&mut rand::rngs::OsRng, Pkcs1v15Encrypt, guid.as_bytes())
            .unwrap(),
    );
    let password = STANDARD.encode(
        key.to_public_key()
            .encrypt(&mut rand::rngs::OsRng, Pkcs1v15Encrypt, password.as_bytes())
            .unwrap(),
    );
    let (time, key) = transfer.unwrap_or((0, vec![]));
    let mut body = Writer::new();
    body.utf("123.5.1")
        .i32(game)
        .utf(&guid)
        .utf(&password)
        .utf("")
        .i32(time)
        .i16(key.len() as i16);
    body.0.extend(key);
    body.i32(0);
    for _ in 0..4 {
        body.utf("");
    }
    Packet {
        id: wire::HELLO,
        body: body.0,
    }
}
pub(super) async fn socket_load(
    socket: &mut NativeSocket,
    dir: &std::path::Path,
    guid: &str,
    ch: i32,
) -> i32 {
    socket.send(socket_hello(dir, guid, -2, None)).await;
    socket.receive(wire::MAP_INFO).await;
    socket
        .send(Packet {
            id: wire::LOAD,
            body: ch.to_be_bytes().to_vec(),
        })
        .await;
    let success = socket.receive(wire::CREATE_SUCCESS).await;
    Reader::new(&success.body).i32().unwrap()
}
async fn socket_server(a: &App) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    println!(
        "owned native socket fixture pid={} port={port}",
        std::process::id()
    );
    let router = Router::new()
        .route("/game", get(websocket))
        .with_state(a.clone());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (port, task)
}
async fn released(a: &App, account: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while a.owners.lock().unwrap().contains_key(&account) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native account admission never released");
}

async fn socket_fault_after_commit(fault: &str) {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let mut characters = Vec::new();
    for name in ["finalize-socket-bad", "finalize-socket-healthy"] {
        let account = a.store.list(name.into()).await.unwrap().id;
        characters.push(
            a.store
                .create(account, a.content.get(WIZARD).unwrap())
                .await
                .unwrap(),
        );
    }
    let bad = &characters[0];
    let healthy = &characters[1];
    let (port, server) = socket_server(&a).await;
    let mut bad_socket = NativeSocket::connect(port, &a.origin).await;
    let bad_id = socket_load(&mut bad_socket, &dir, "finalize-socket-bad", bad.id).await;
    let mut healthy_socket = NativeSocket::connect(port, &a.origin).await;
    let healthy_id = socket_load(
        &mut healthy_socket,
        &dir,
        "finalize-socket-healthy",
        healthy.id,
    )
    .await;
    lifecycle::account(
        &a,
        vec![bad.account, healthy.account],
        "socket_load_settled",
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    bad_socket
        .send(super::lifecycle_native_tests::native_swap_packet(
            bad_id, bad,
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
            [bad.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        bad.revision + 1
    );
    if fault == "late" {
        release.send(()).unwrap();
        lifecycle::account(&a, vec![bad.account], "ack_inventory_settled", |_| async {
            Ok(())
        })
        .await
        .unwrap();
        let pos = a
            .world
            .for_player(bad_id)
            .call(move |w| Ok(w.players[&bad_id].pos))
            .await
            .unwrap();
        let mut use_spell = Writer::new();
        use_spell
            .i32(0)
            .i32(bad_id)
            .u8(1)
            .i16(bad.items[1] as i16)
            .f32(pos.x)
            .f32(pos.y);
        bad_socket
            .send(Packet {
                id: 30,
                body: use_spell.0,
            })
            .await;
        bad_socket.receive(13).await;
        a.world.clock.set_paused(false);
        tokio::time::sleep(Duration::from_secs(6)).await;
        a.world.clock.set_paused(true);
        bad_socket
            .send(Packet {
                id: 22,
                body: 0i32.to_be_bytes().to_vec(),
            })
            .await;
    } else {
        let pos = a
            .world
            .for_player(bad_id)
            .call(move |w| Ok(w.players[&bad_id].pos))
            .await
            .unwrap();
        let packet = match fault {
            "malformed" => Packet {
                id: 14,
                body: vec![],
            },
            "unsolicited" => Packet {
                id: 22,
                body: 0i32.to_be_bytes().to_vec(),
            },
            "invalid_time" => Packet {
                id: 22,
                body: (-2i32).to_be_bytes().to_vec(),
            },
            "ground_time" | "ground_range" | "ground_safe" => {
                let mut body = Writer::new();
                body.i32(if fault == "ground_time" { -1 } else { 0 })
                    .f32(if fault == "ground_range" {
                        pos.x + 1000.
                    } else {
                        pos.x
                    })
                    .f32(pos.y);
                Packet {
                    id: 64,
                    body: body.0,
                }
            }
            "shot_counter" => {
                let mut body = Writer::new();
                body.i32(1000)
                    .u8(127)
                    .i16(bad.items[0] as i16)
                    .f32(pos.x + 0.3)
                    .f32(pos.y)
                    .f32(0.);
                Packet {
                    id: wire::PLAYER_SHOOT,
                    body: body.0,
                }
            }
            _ => panic!("unknown native fault fixture"),
        };
        bad_socket.send(packet).await;
        release.send(()).unwrap();
    }
    released(&a, bad.account).await;
    assert!(
        !a.world
            .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&bad_id))))
            .await
            .unwrap()
    );
    healthy_socket
        .send(super::lifecycle_native_tests::native_swap_packet(
            healthy_id, healthy,
        ))
        .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if a.store
                .load(healthy.account, healthy.id)
                .await
                .unwrap()
                .revision
                == healthy.revision + 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(healthy_socket);
    released(&a, healthy.account).await;
    let mut saved_items = bad.items.clone();
    saved_items.swap(4, 5);
    let saved = a.store.load(bad.account, bad.id).await.unwrap();
    assert_eq!(
        (saved.revision, saved.items),
        (bad.revision + 1, saved_items)
    );
    let mut reconnect = NativeSocket::connect(port, &a.origin).await;
    let new_id = socket_load(&mut reconnect, &dir, "finalize-socket-bad", bad.id).await;
    assert_ne!(new_id, bad_id);
    assert!(a.claim(bad.account).is_err());
    drop(reconnect);
    released(&a, bad.account).await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_real_socket_manual_and_resurrection_transfer_retire_before_reconnect() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (port, server) = socket_server(&a).await;
    for forced in [false, true] {
        let guid = format!("finalize-transfer-{forced}");
        let account = a.store.list(guid.clone()).await.unwrap().id;
        let mut ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        if forced {
            ch.items[3] = 0xb3e;
            ch.revision = a
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap()[0];
        }
        let mut socket = NativeSocket::connect(port, &a.origin).await;
        let id = socket_load(&mut socket, &dir, &guid, ch.id).await;
        lifecycle::account(
            &a,
            vec![account],
            "socket_transfer_load_settled",
            |_| async { Ok(()) },
        )
        .await
        .unwrap();
        if forced {
            a.world
                .for_player(id)
                .call(move |w| {
                    let p = w.players.get_mut(&id).unwrap();
                    p.character.hp = 0;
                    p.death_pending = true;
                    Ok(())
                })
                .await
                .unwrap();
            persistence_tick(&a).await.unwrap();
        } else {
            let (ready, release) = a.store.lifecycle_test_pause_reply();
            let mut text = Writer::new();
            text.utf("/tutorial");
            socket
                .send(Packet {
                    id: wire::PLAYER_TEXT,
                    body: text.0,
                })
                .await;
            tokio::time::timeout(Duration::from_secs(5), ready)
                .await
                .unwrap()
                .unwrap();
            assert!(a.claim(account).is_err());
            assert!(
                a.world
                    .for_player(id)
                    .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
                    .await
                    .unwrap()
            );
            // Admission is still closed while the old save has committed but not published.
            assert_eq!(a.server_usage().unwrap(), 1. / a.max_players as f64);
            release.send(()).unwrap();
        }
        let packet = socket.receive(39).await;
        let mut r = Reader::new(&packet.body);
        r.utf().unwrap();
        r.utf().unwrap();
        r.i32().unwrap();
        let destination = r.i32().unwrap();
        let time = r.i32().unwrap();
        let n = r.i16().unwrap() as usize;
        let key = r.take(n).unwrap().to_vec();
        r.done().unwrap();
        assert_eq!(destination, if forced { -2 } else { -1 });
        assert!(
            !a.owners.lock().unwrap().contains_key(&account),
            "RECONNECT exposed before native admission release"
        );
        assert!(
            !a.world
                .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
                .await
                .unwrap()
        );
        let mut replacement = NativeSocket::connect(port, &a.origin).await;
        replacement
            .send(socket_hello(&dir, &guid, destination, Some((time, key))))
            .await;
        replacement.receive(wire::MAP_INFO).await;
        replacement
            .send(Packet {
                id: wire::LOAD,
                body: ch.id.to_be_bytes().to_vec(),
            })
            .await;
        let created = replacement.receive(wire::CREATE_SUCCESS).await;
        assert_ne!(Reader::new(&created.body).i32().unwrap(), id);
        assert!(a.claim(account).is_err());
        let saved = a.store.load(account, ch.id).await.unwrap();
        assert_eq!(saved.revision, ch.revision + if forced { 1 } else { 0 });
        assert!(saved.alive);
        if forced {
            assert_eq!(saved.items[3], -1);
        }
        drop(socket);
        drop(replacement);
        released(&a, account).await;
        assert_eq!(
            rusqlite::Connection::open(dir.join("game.db"))
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM deaths WHERE character=?1",
                    [ch.id],
                    |r| r.get::<_, i32>(0)
                )
                .unwrap(),
            0
        );
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_real_socket_disconnect_kick_shutdown_share_one_result() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let guid = "finalize-concurrent-socket";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    let id = socket_load(&mut socket, &dir, guid, ch.id).await;
    // CREATE_SUCCESS precedes final presentation publication in the accepted LOAD.
    lifecycle::account(&a, vec![account], "socket_setup_settled", |_| async {
        Ok(())
    })
    .await
    .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE lifecycle_close_audit(n INTEGER); CREATE TRIGGER lifecycle_close_update AFTER UPDATE ON characters BEGIN INSERT INTO lifecycle_close_audit VALUES(1); END;").unwrap();
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let app = a.clone();
    let cancelled = tokio::spawn(async move { app.kick(account).await });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let app = a.clone();
    let second = tokio::spawn(async move { app.kick(account).await });
    let duplicate = lifecycle::finalize(&a, id, CloseReason::Shutdown).unwrap();
    a.shutdown.send_replace(true);
    drop(socket);
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    assert!(a.claim(account).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM lifecycle_close_audit", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    release.send(()).unwrap();
    duplicate.wait().await.unwrap();
    assert!(second.await.unwrap().unwrap());
    released(&a, account).await;
    assert!(
        !a.world
            .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
            .await
            .unwrap()
    );
    assert_eq!(
        a.store.load(account, ch.id).await.unwrap().revision,
        ch.revision
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM lifecycle_close_audit", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_cancelled_script_leave_releases_after_publication() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.control = Some("test-script-control".into());
    let (owner, id, ch) = avatar(&a, "finalize-script-leave").await;
    a.bots.lock().unwrap().insert(id, owner);
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let app = a.clone();
    let caller = tokio::spawn(async move {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "Bearer test-script-control".parse().unwrap(),
        );
        bot_leave(State(app), headers, Json(BotPlayer { player: id })).await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(a.claim(ch.account).is_err());
    assert!(!a.bots.lock().unwrap().contains_key(&id));
    release.send(()).unwrap();
    released(&a, ch.account).await;
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_executor_loss_is_fenced_without_ambient_drop_executor() {
    let (dir, a) = account_actions::tests::fixture();
    let (mut owner, id, ch) = avatar(&a, "finalize-executor-loss").await;
    let runtime = std::thread::spawn(|| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        handle
    })
    .join()
    .unwrap();
    owner.arm_cleanup(&a, runtime);
    std::thread::spawn(move || drop(owner)).join().unwrap();
    assert_eq!(
        lifecycle::state(&a, ch.account),
        Some((false, Some(id), true))
    );
    assert!(a.claim(ch.account).is_err());
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_native_locked_class_is_rejected_without_uncertainty() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let guid = "finalize-locked-priest";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let before = a.store.presentation(account).await.unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket.send(socket_hello(&dir, guid, -2, None)).await;
    socket.receive(wire::MAP_INFO).await;
    let mut create = Writer::new();
    create.i16(crate::content::PRIEST as i16);
    socket
        .send(Packet {
            id: wire::CREATE,
            body: create.0,
        })
        .await;
    socket.receive(0).await;
    released(&a, account).await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    let after = a.store.presentation(account).await.unwrap();
    assert_eq!(before, after);
    let mut eligible = NativeSocket::connect(port, &a.origin).await;
    eligible.send(socket_hello(&dir, guid, -2, None)).await;
    eligible.receive(wire::MAP_INFO).await;
    let mut create = Writer::new();
    create.i16(WIZARD as i16);
    eligible
        .send(Packet {
            id: wire::CREATE,
            body: create.0,
        })
        .await;
    let created = eligible.receive(wire::CREATE_SUCCESS).await;
    let mut r = Reader::new(&created.body);
    r.i32().unwrap();
    let character = r.i32().unwrap();
    assert_eq!(
        a.store.load(account, character).await.unwrap().class,
        WIZARD
    );
    assert!(a.claim(account).is_err());
    drop(socket);
    drop(eligible);
    released(&a, account).await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_native_create_sql_fault_remains_fenced_healthy_peer_persists() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let guid = "finalize-create-sql";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER lifecycle_create_sql BEFORE INSERT ON characters WHEN NEW.account={account} BEGIN SELECT RAISE(ABORT,'native create SQL fault'); END;")).unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket.send(socket_hello(&dir, guid, -2, None)).await;
    socket.receive(wire::MAP_INFO).await;
    let mut create = Writer::new();
    create.i16(WIZARD as i16);
    socket
        .send(Packet {
            id: wire::CREATE,
            body: create.0,
        })
        .await;
    socket.receive(0).await;
    assert!(a.claim(account).is_err());
    assert!(lifecycle::state(&a, account).unwrap().2);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    let (owner, healthy, ch) = avatar(&a, "finalize-create-sql-peer").await;
    depart(&a, healthy).await.unwrap();
    owner.release();
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    assert!(a.claim(account).is_err());
    drop(socket);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

async fn trade_close(fault: &str) {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (one_owner, one, mut one_ch) = avatar(&a, "finalize-trade-one").await;
    let (two_owner, two, mut two_ch) = avatar(&a, "finalize-trade-two").await;
    for (id, ch, kind) in [(one, &mut one_ch, 0xa22), (two, &mut two_ch, 0xa23)] {
        ch.items[4] = kind;
        ch.revision = a
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        let setup = ch.clone();
        a.world
            .for_player(id)
            .call(move |w| {
                w.players.get_mut(&id).unwrap().character = setup;
                Ok(())
            })
            .await
            .unwrap();
    }
    a.world
        .call(move |w| {
            let pos = w.players[&one].pos;
            w.players.get_mut(&two).unwrap().pos = pos;
            w.players.get_mut(&one).unwrap().character.name = "FirstTrader".into();
            w.players.get_mut(&two).unwrap().character.name = "SecondTrader".into();
            w.request_trade(one, "SecondTrader".into())?;
            w.request_trade(two, "FirstTrader".into())?;
            let mut offer = [false; 12];
            offer[4] = true;
            w.change_trade(one, offer)?;
            w.change_trade(two, offer)?;
            w.now += 3000;
            assert!(w.accept_trade(one, offer, offer)?.is_none());
            Ok(())
        })
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE lifecycle_trade_audit(n INTEGER); CREATE TRIGGER lifecycle_trade_audit_update AFTER UPDATE ON characters BEGIN INSERT INTO lifecycle_trade_audit VALUES(1); END;").unwrap();
    if fault == "sql" {
        db.execute_batch(&format!("CREATE TRIGGER lifecycle_trade_sql BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'accepted trade SQL fault'); END;",one_ch.account)).unwrap();
    }
    let publication = if fault == "publication" {
        Some(a.world.lifecycle_test_pause_job(2, true))
    } else {
        None
    };
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let app = a.clone();
    let accounts = vec![one_ch.account, two_ch.account];
    let caller = tokio::spawn(async move {
        lifecycle::account(&app, accounts, "finalize_trade", move |app| async move {
            lifecycle::stage(Stage::Reservation);
            let plan = app
                .world
                .call(move |w| {
                    let mut offer = [false; 12];
                    offer[4] = true;
                    w.accept_trade(two, offer, offer)?
                        .context("trade must be accepted")
                })
                .await?;
            persist_trade(&app, plan).await
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let one_done = one_owner
        .close(&a, CloseReason::Transport)
        .unwrap()
        .unwrap();
    let two_done = two_owner.close(&a, CloseReason::Shutdown).unwrap().unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(a.claim(one_ch.account).is_err());
    assert!(a.claim(two_ch.account).is_err());
    release.send(()).unwrap();
    if let Some((ready, release)) = publication {
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        a.world.lifecycle_test_lose_reply();
        release.send(()).unwrap();
    }
    let first = one_done.wait().await;
    let second = two_done.wait().await;
    if fault == "none" {
        first.unwrap();
        second.unwrap();
    } else {
        assert!(first.is_err());
        assert!(second.is_err());
        for account in [one_ch.account, two_ch.account] {
            assert!(a.claim(account).is_err());
            assert!(
                lifecycle::failure(&a, account)
                    .unwrap()
                    .contains(if fault == "sql" {
                        "accepted trade SQL fault"
                    } else {
                        "worker failed"
                    })
            );
        }
    }
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    let saved_one = reopened.load(one_ch.account, one_ch.id).await.unwrap();
    let saved_two = reopened.load(two_ch.account, two_ch.id).await.unwrap();
    let committed = fault != "sql";
    if committed {
        one_ch.items[4] = 0xa23;
        two_ch.items[4] = 0xa22;
    }
    assert_eq!(
        (saved_one.revision, saved_one.items),
        (one_ch.revision + i32::from(committed), one_ch.items)
    );
    assert_eq!(
        (saved_two.revision, saved_two.items),
        (two_ch.revision + i32::from(committed), two_ch.items)
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM lifecycle_trade_audit", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        match fault {
            "none" => 4,
            "publication" => 2,
            "sql" => 0,
            _ => unreachable!(),
        }
    );
    assert_eq!(a.owners.lock().unwrap().is_empty(), fault == "none");
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_trade_commit_cancelled_waiter_exact_items_after_reopen() {
    trade_close("none").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_trade_sql_failure_fences_both_owners_without_mutation() {
    trade_close("sql").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_trade_lost_publication_reply_fences_both_committed_owners() {
    trade_close("publication").await;
}

async fn vault_close(boundary: &str) {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let account = a
        .store
        .register(
            format!("finalize-vault-{boundary}"),
            format!("finalize-{boundary}@example.test"),
            "test-password".into(),
        )
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [account])
        .unwrap();
    let mut ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut vault = a.store.vault(account, 0).await.unwrap();
    if boundary == "withdraw" {
        vault.items[0] = 0xa22;
        ch.revision = a
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![vault],
            )
            .await
            .unwrap()[0];
        vault = a.store.vault(account, 0).await.unwrap();
    }
    let initial = ch.clone();
    let old_vault = vault.clone();
    let destination = a
        .world
        .directory(move |d| d.instance("vault", Some(account)))
        .await
        .unwrap();
    a.world = a.world.in_world(destination);
    let presentation = a.store.presentation(account).await.unwrap();
    let mut owner = a.claim_session(account).unwrap();
    owner.registered = true;
    owner.arm_cleanup(&a, tokio::runtime::Handle::current());
    let bind = lifecycle::binding(&a, account).unwrap();
    let (tx, _rx) = mpsc::channel(4096);
    let (lost, _notice) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    let buying = boundary == "purchase";
    let (id, target) = a
        .world
        .call(move |w| {
            w.load_vaults(vec![vault])?;
            let target = w
                .entities
                .values()
                .find(|e| e.kind == if buying { 0x505 } else { 0x504 })
                .unwrap();
            let (target, pos) = (target.id, target.pos);
            let id = w.enter_at(ch, Some(output), Some(pos), presentation)?;
            bind(id)?;
            w.players.get_mut(&id).unwrap().known.insert(target);
            Ok((id, target))
        })
        .await
        .unwrap();
    let packet = if buying {
        Packet {
            id: 50,
            body: target.to_be_bytes().to_vec(),
        }
    } else {
        let mut body = Writer::new();
        body.i32(0).f32(0.).f32(0.);
        let (first, second) = if boundary == "deposit" {
            (
                Slot {
                    object: id,
                    slot: 4,
                    kind: initial.items[4],
                },
                Slot {
                    object: target,
                    slot: 0,
                    kind: -1,
                },
            )
        } else {
            (
                Slot {
                    object: target,
                    slot: 0,
                    kind: 0xa22,
                },
                Slot {
                    object: id,
                    slot: 5,
                    kind: initial.items[5],
                },
            )
        };
        for slot in [first, second] {
            body.i32(slot.object)
                .u8(slot.slot as u8)
                .i16(slot.kind as i16);
        }
        Packet {
            id: 65,
            body: body.0,
        }
    };
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let mut app = a.clone();
    let caller = tokio::spawn(async move {
        handle(
            &mut app,
            packet,
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM characters WHERE id=?1",
            [initial.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        initial.revision + 1
    );
    let finalization = lifecycle::finalize(&a, id, CloseReason::Transport).unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(a.claim(account).is_err());
    release.send(()).unwrap();
    finalization.wait().await.unwrap();
    released(&a, account).await;
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    let saved = reopened.load(account, initial.id).await.unwrap();
    let saved_vault = reopened.vault(account, 0).await.unwrap();
    let mut items = initial.items.clone();
    let mut vault_items = old_vault.items;
    match boundary {
        "deposit" => {
            vault_items[0] = items[4];
            items[4] = -1;
        }
        "withdraw" => {
            items[5] = vault_items[0];
            vault_items[0] = -1;
        }
        "purchase" => {}
        _ => unreachable!(),
    }
    assert_eq!((saved.revision, saved.items), (initial.revision + 1, items));
    assert_eq!(
        (saved_vault.revision, saved_vault.items),
        (old_vault.revision + i32::from(!buying), vault_items)
    );
    assert_eq!(
        reopened.presentation(account).await.unwrap().credits,
        if buying { 500 } else { 1000 }
    );
    assert_eq!(
        reopened.vaults(account).await.unwrap().len(),
        if buying { 2 } else { 1 }
    );
    assert!(a.owners.lock().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_vault_deposit_commit_cancelled_waiter_exact_items() {
    vault_close("deposit").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_vault_withdraw_commit_cancelled_waiter_exact_items() {
    vault_close("withdraw").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_vault_purchase_commit_cancelled_waiter_exact_gold() {
    vault_close("purchase").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_full_world_output_does_not_poison_committed_inventory() {
    let (dir, mut a) = account_actions::tests::fixture();
    let (mut owner, id, ch) = avatar(&a, "finalize-full-output").await;
    owner.arm_cleanup(&a, tokio::runtime::Handle::current());
    let (tx, _rx) = mpsc::channel(1);
    tx.try_send(Packet {
        id: 255,
        body: vec![],
    })
    .unwrap();
    let (lost, _notice) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    a.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().tx = Some(output);
            Ok(())
        })
        .await
        .unwrap();
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    handle(
        &mut a,
        super::lifecycle_native_tests::native_swap_packet(id, &ch),
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    assert!(
        *lost.borrow(),
        "publication must signal the actual saturated output"
    );
    session
        .account
        .take()
        .unwrap()
        .close(&a, CloseReason::Output)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    let mut items = ch.items.clone();
    items.swap(4, 5);
    let saved = a.store.load(ch.account, ch.id).await.unwrap();
    assert_eq!((saved.revision, saved.items), (ch.revision + 1, items));
    assert!(a.owners.lock().unwrap().is_empty());
    let (peer_owner, peer, peer_ch) = avatar(&a, "finalize-full-output-peer").await;
    depart(&a, peer).await.unwrap();
    peer_owner.release();
    assert_eq!(
        a.store
            .load(peer_ch.account, peer_ch.id)
            .await
            .unwrap()
            .revision,
        peer_ch.revision
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_real_socket_malformed_inventory_guild_and_shop_are_isolated() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (port, server) = socket_server(&a).await;
    for kind in [65, wire::CREATE_GUILD, wire::BUY] {
        let guid = format!("finalize-malformed-{kind}");
        let account = a.store.list(guid.clone()).await.unwrap().id;
        let ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let mut socket = NativeSocket::connect(port, &a.origin).await;
        socket_load(&mut socket, &dir, &guid, ch.id).await;
        lifecycle::account(&a, vec![account], "malformed_load_settled", |_| async {
            Ok(())
        })
        .await
        .unwrap();
        socket
            .send(Packet {
                id: kind,
                body: vec![],
            })
            .await;
        socket.receive(0).await;
        released(&a, account).await;
        let saved = a.store.load(account, ch.id).await.unwrap();
        assert_eq!((saved.revision, saved.items), (ch.revision, ch.items));
        let owner = a.claim(account).unwrap();
        owner.release();
        drop(socket);
    }
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_native_trade_rejections_continue_until_actual_close() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (port, server) = socket_server(&a).await;
    let mut actors = vec![];
    for guid in ["finalize-native-trade-one", "finalize-native-trade-two"] {
        let account = a.store.list(guid.into()).await.unwrap().id;
        let ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let mut socket = NativeSocket::connect(port, &a.origin).await;
        let id = socket_load(&mut socket, &dir, guid, ch.id).await;
        actors.push((socket, id, ch));
    }
    let (mut socket, one, ch) = actors.remove(0);
    let (other, two, other_ch) = actors.remove(0);
    lifecycle::account(
        &a,
        vec![ch.account, other_ch.account],
        "trade_loads_settled",
        |_| async { Ok(()) },
    )
    .await
    .unwrap();
    a.world
        .for_player(one)
        .call(move |w| {
            let pos = w.players[&one].pos;
            w.players.get_mut(&two).unwrap().pos = pos;
            w.players.get_mut(&one).unwrap().character.name = "NativeTraderOne".into();
            w.players.get_mut(&two).unwrap().character.name = "NativeTraderTwo".into();
            Ok(())
        })
        .await
        .unwrap();
    for case in ["missing", "settling", "stale"] {
        if case != "missing" {
            a.world
                .for_player(one)
                .call(move |w| {
                    w.request_trade(one, "NativeTraderTwo".into())?;
                    w.request_trade(two, "NativeTraderOne".into())?;
                    let mut offer = [false; 12];
                    offer[4] = true;
                    w.change_trade(one, offer)?;
                    w.change_trade(two, offer)?;
                    if case == "stale" {
                        w.now += 3000;
                    }
                    Ok(())
                })
                .await
                .unwrap();
        }
        let mut body = Writer::new();
        for _ in 0..2 {
            body.i16(12);
            for _ in 0..12 {
                body.u8(0);
            }
        }
        socket
            .send(Packet {
                id: 57,
                body: body.0,
            })
            .await;
        let reply = socket.receive(12).await;
        assert_eq!(Reader::new(&reply.body).i32().unwrap(), 1);
        assert!(
            a.world
                .for_player(one)
                .call(move |w| Ok(w.players.contains_key(&one) && w.players.contains_key(&two)))
                .await
                .unwrap()
        );
        assert!(!lifecycle::state(&a, ch.account).unwrap().2);
        assert!(!lifecycle::state(&a, other_ch.account).unwrap().2);
        assert!(a.claim(ch.account).is_err());
        assert_eq!(
            a.store.load(ch.account, ch.id).await.unwrap().revision,
            ch.revision
        );
    }
    assert!(a.kick(ch.account).await.unwrap());
    released(&a, ch.account).await;
    let mut replacement = NativeSocket::connect(port, &a.origin).await;
    assert_ne!(
        socket_load(&mut replacement, &dir, "finalize-native-trade-one", ch.id).await,
        one
    );
    drop(socket);
    drop(replacement);
    drop(other);
    released(&a, ch.account).await;
    released(&a, other_ch.account).await;
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().items,
        ch.items
    );
    assert_eq!(
        a.store
            .load(other_ch.account, other_ch.id)
            .await
            .unwrap()
            .items,
        other_ch.items
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_unknown_world_preparation_error_is_not_a_rule_rejection() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = avatar(&a, "finalize-unknown-preparation").await;
    let error = inventory(&a, id, |_| {
        anyhow::bail!("injected unknown preparation invariant")
    })
    .await
    .expect_err("unknown invariant was downgraded into an inner rule rejection");
    assert!(
        error
            .to_string()
            .contains("injected unknown preparation invariant")
    );
    owner.release();
    assert!(a.claim(ch.account).is_err());
    assert!(lifecycle::state(&a, ch.account).unwrap().2);
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    let (peer_owner, peer, peer_ch) = avatar(&a, "finalize-unknown-preparation-peer").await;
    depart(&a, peer).await.unwrap();
    peer_owner.release();
    assert_eq!(
        a.store
            .load(peer_ch.account, peer_ch.id)
            .await
            .unwrap()
            .items,
        peer_ch.items
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_native_ability_cooldown_and_nonconsumable_rejections_stay_active() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let guid = "finalize-native-cooldown";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    let id = socket_load(&mut socket, &dir, guid, ch.id).await;
    lifecycle::account(&a, vec![account], "ability_load_settled", |_| async {
        Ok(())
    })
    .await
    .unwrap();
    let pos = a
        .world
        .for_player(id)
        .call(move |w| Ok(w.players[&id].pos))
        .await
        .unwrap();
    for (attempt, slot) in [1, 1, 0].into_iter().enumerate() {
        let mut body = Writer::new();
        body.i32(0)
            .i32(id)
            .u8(slot)
            .i16(ch.items[slot as usize] as i16)
            .f32(pos.x)
            .f32(pos.y);
        socket
            .send(Packet {
                id: 30,
                body: body.0,
            })
            .await;
        if attempt > 0 {
            let reply = socket.receive(4).await;
            assert_eq!(Reader::new(&reply.body).i32().unwrap(), 1);
        }
    }
    lifecycle::account(&a, vec![account], "ability_rejections_settled", |_| async {
        Ok(())
    })
    .await
    .unwrap();
    let mana_cost = a.content.get(ch.items[1] as u16).unwrap().mana_cost;
    a.world
        .for_player(id)
        .call(move |w| {
            assert_eq!(w.players[&id].character.mp, ch.mp - mana_cost);
            assert!(!w.players[&id].pending);
            Ok(())
        })
        .await
        .unwrap();
    assert!(!lifecycle::state(&a, account).unwrap().2);
    assert!(a.kick(account).await.unwrap());
    released(&a, account).await;
    let saved = a.store.load(account, ch.id).await.unwrap();
    assert_eq!(saved.items, ch.items);
    assert_eq!(saved.mp, ch.mp - mana_cost);
    drop(socket);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_repeated_terminal_sessions_release_tasks_claims_and_avatars() {
    let (dir, a) = account_actions::tests::fixture();
    for n in 0..8 {
        let (owner, id, ch) = avatar(&a, &format!("finalize-repeated-{n}")).await;
        owner
            .close(&a, CloseReason::Controller)
            .unwrap()
            .unwrap()
            .wait()
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while lifecycle::live_tasks() != 0 || a.store.lifecycle_test_jobs() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(a.owners.lock().unwrap().is_empty());
        assert_eq!(lifecycle::outstanding(&a), 0);
        assert!(
            !a.world
                .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
                .await
                .unwrap()
        );
        assert_eq!(
            a.store.load(ch.account, ch.id).await.unwrap().revision,
            ch.revision
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_real_socket_malformed_after_commit_healthy_peer_and_reconnect() {
    socket_fault_after_commit("malformed").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_real_socket_semantic_shoot_ack_faults_preserve_commit_and_peer() {
    for fault in ["invalid_time", "unsolicited", "late"] {
        socket_fault_after_commit(fault).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_real_socket_semantic_ground_and_shot_faults_preserve_commit_and_peer() {
    for fault in ["ground_time", "ground_range", "ground_safe", "shot_counter"] {
        socket_fault_after_commit(fault).await;
    }
}

#[path = "lifecycle_fault_boundary_tests.rs"]
mod fault_boundary_tests;

#[path = "lifecycle_quiescence_tests.rs"]
mod quiescence_tests;

fn script_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer script-finalize-test".parse().unwrap(),
    );
    headers
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_script_entry_lost_bound_reply_stops_without_replay() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.control = Some("script-finalize-test".into());
    a.world.clock.set_paused(true);
    let (peer_owner, peer, peer_ch) = avatar(&a, "script-entry-healthy").await;
    let account = a.store.list("script-entry-lost".into()).await.unwrap().id;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TABLE script_writes(kind TEXT); CREATE TRIGGER script_insert AFTER INSERT ON characters WHEN NEW.account={account} BEGIN INSERT INTO script_writes VALUES('insert'); END; CREATE TRIGGER script_update AFTER UPDATE ON characters WHEN NEW.account={account} BEGIN INSERT INTO script_writes VALUES('update'); END;")).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    println!(
        "owned script socket fixture pid={} address={address}",
        std::process::id()
    );
    let router = Router::new()
        .route("/dev/join", post(bot_create))
        .with_state(a.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let (ready, release) = a.world.lifecycle_test_pause_job(2, false);
    let join = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("http://{address}/dev/join"))
            .bearer_auth("script-finalize-test")
            .json(&serde_json::json!({"guid":"script-entry-lost","world":-2}))
            .send()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let state = lifecycle::state(&a, account).unwrap();
    assert!(state.0);
    let id = state.1.unwrap();
    assert!(!state.2);
    assert!(a.claim(account).is_err());
    let actor = db
        .query_row(
            "SELECT id FROM characters WHERE account=?",
            [account],
            |r| r.get::<_, i32>(0),
        )
        .unwrap();
    let saved = a.store.load(account, actor).await.unwrap();
    assert!(saved.alive);
    assert_eq!(saved.revision, 0);
    let original = serde_json::to_value(&saved).unwrap();
    a.world.lifecycle_test_lose_reply();
    release.send(()).unwrap();
    let response = join.await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("world directory worker failed")
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if a.world
                .for_player(id)
                .call(move |w| {
                    let p = &w.players[&id];
                    Ok(p.departing && p.tx.is_none() && !p.pending)
                })
                .await
                .unwrap()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("lost script entry reply left its bound avatar active");
    let error = lifecycle::finalize(&a, id, CloseReason::Shutdown)
        .unwrap()
        .wait()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("operation=script_entry"), "{error}");
    assert!(error.contains("stage=publication"), "{error}");
    assert!(error.contains("world directory worker failed"), "{error}");
    assert!(error.contains("controller_stop=acknowledged"), "{error}");
    assert_eq!(lifecycle::state(&a, account), Some((false, Some(id), true)));
    assert!(a.claim(account).is_err());
    assert!(a.bots.lock().unwrap().is_empty());
    assert!(close_script(&a, id).is_err());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM script_writes WHERE kind='insert'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM script_writes WHERE kind='update'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.load(account, actor).await.unwrap()).unwrap(),
        original
    );
    let (from, to) = (peer_ch.items[4], peer_ch.items[5]);
    inventory(&a, peer, move |w| {
        w.action(
            peer,
            Action::Swap {
                from: crate::world::Slot {
                    object: peer,
                    slot: 4,
                    kind: from,
                },
                to: crate::world::Slot {
                    object: peer,
                    slot: 5,
                    kind: to,
                },
            },
        )
    })
    .await
    .unwrap()
    .unwrap();
    peer_owner
        .close(&a, CloseReason::Controller)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    let peer_saved = reopened.load(peer_ch.account, peer_ch.id).await.unwrap();
    let mut items = peer_ch.items;
    items.swap(4, 5);
    assert_eq!(
        (peer_saved.items, peer_saved.revision),
        (items, peer_ch.revision + 1)
    );
    a.claim(peer_ch.account).unwrap().release();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_script_entry_cancelled_waiter_publishes_map_custody_then_leave() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.control = Some("script-finalize-test".into());
    a.world.clock.set_paused(true);
    let account = a.store.list("script-entry-cancel".into()).await.unwrap().id;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE script_writes(kind TEXT); CREATE TRIGGER script_insert AFTER INSERT ON characters BEGIN INSERT INTO script_writes VALUES('insert'); END; CREATE TRIGGER script_update AFTER UPDATE ON characters BEGIN INSERT INTO script_writes VALUES('update'); END;").unwrap();
    let (ready, release) = a.world.lifecycle_test_pause_job(2, false);
    let app = a.clone();
    let join = tokio::spawn(async move {
        bot_create(
            State(app),
            script_headers(),
            Json(BotCreate {
                guid: "script-entry-cancel".into(),
                world: -2,
            }),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let state = lifecycle::state(&a, account).unwrap();
    assert!(state.0 && !state.2);
    let id = state.1.unwrap();
    assert!(a.bots.lock().unwrap().is_empty());
    assert!(a.claim(account).is_err());
    let actor = db
        .query_row(
            "SELECT id FROM characters WHERE account=?",
            [account],
            |r| r.get::<_, i32>(0),
        )
        .unwrap();
    let saved = a.store.load(account, actor).await.unwrap();
    join.abort();
    assert!(join.await.unwrap_err().is_cancelled());
    assert!(a.claim(account).is_err());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while lifecycle::state(&a, account).unwrap().0 || !a.bots.lock().unwrap().contains_key(&id)
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        lifecycle::state(&a, account),
        Some((false, Some(id), false))
    );
    let published = a
        .world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(!p.departing && !p.pending);
            assert_eq!(p.character.id, actor);
            Ok(p.character.clone())
        })
        .await
        .unwrap();
    let mut expected = saved;
    assert_eq!(expected.tiles, 0);
    assert!(published.tiles > 0);
    expected.tiles = published.tiles;
    assert_eq!(
        serde_json::to_value(published).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let response = bot_leave(
        State(a.clone()),
        script_headers(),
        Json(BotPlayer { player: id }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .as_ref(),
        br#"{"saved":true}"#
    );
    assert!(a.bots.lock().unwrap().is_empty());
    assert!(a.owners.lock().unwrap().is_empty());
    assert!(
        a.world
            .for_player(id)
            .call(move |w| w.observe(id))
            .await
            .is_err()
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM script_writes WHERE kind='insert'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM script_writes WHERE kind='update'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        serde_json::to_value(
            Store::open(&dir.join("game.db"), &a.content)
                .unwrap()
                .load(account, actor)
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    a.claim(account).unwrap().release();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_script_entry_map_owner_has_no_app_retention_cycle() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.control = Some("script-finalize-test".into());
    a.world.clock.set_paused(true);
    let response = bot_create(
        State(a.clone()),
        script_headers(),
        Json(BotCreate {
            guid: "script-entry-cycle".into(),
            world: -2,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(a.bots.lock().unwrap().len(), 1);
    let weak = Arc::downgrade(&a.bots);
    drop(a);
    assert!(
        weak.upgrade().is_none(),
        "script map Owner retained App->Owner->App cycle"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
