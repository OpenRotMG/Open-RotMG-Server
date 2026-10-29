use super::*;
use std::time::Duration;

pub(super) struct NativeSocket {
    io: reqwest::Upgraded,
    codec: Codec,
    pending: std::collections::VecDeque<Packet>,
    forbid_reconnect: bool,
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
            forbid_reconnect: false,
        }
    }
    pub(super) async fn send(&mut self, packet: Packet) {
        self.send_frame(vec![packet]).await;
    }
    // One WebSocket frame may carry several game packets, as Ruffle sends buffered writes.
    pub(super) async fn send_frame(&mut self, packets: Vec<Packet>) {
        use tokio::io::AsyncWriteExt;
        let bytes: Vec<u8> = packets
            .into_iter()
            .flat_map(|p| self.codec.frame(p.id, p.body))
            .collect();
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
        self.receive_one_of(&[expected]).await
    }
    pub(super) async fn receive_one_of(&mut self, expected: &[u8]) -> Packet {
        use tokio::io::AsyncReadExt;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                while let Some(packet) = self.pending.pop_front() {
                    if self.forbid_reconnect {
                        assert_ne!(packet.id, 39, "failed transfer published RECONNECT");
                    }
                    if expected.contains(&packet.id) {
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

pub(super) fn socket_hello_password(
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

pub(super) async fn socket_server(a: &App) -> (u16, tokio::task::JoinHandle<()>) {
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
            let mut mismatch = ch.clone();
            mismatch.alive = false;
            mismatch.hp = 0;
            mismatch.tombs += 1;
            assert!(
                a.store
                    .resurrect(mismatch, 3, a.content.clone())
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("resurrection reservation changed")
            );
            assert_eq!(a.store.load(account, ch.id).await.unwrap().tombs, ch.tombs);
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
async fn client_entry_rejects_debug_worlds_without_changing_saved_character() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (port, server) = socket_server(&a).await;
    let guid = "ordinary-client-entry";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let character = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    for mode in ["build", "editor-world", "editor-json"] {
        let mut hello = socket_hello(&dir, guid, -2, None);
        match mode {
            "build" => hello.body[8] = b'0',
            "editor-world" => hello.body[9..13].copy_from_slice(&(-6_i32).to_be_bytes()),
            "editor-json" => {
                let offset = hello.body.len() - 12;
                hello.body[offset..offset + 4].copy_from_slice(&2_i32.to_be_bytes());
                hello.body.splice(offset + 4..offset + 4, *b"{}");
            }
            _ => unreachable!(),
        }
        let mut socket = NativeSocket::connect(port, &a.origin).await;
        socket.send(hello).await;
        let refusal = socket.receive_one_of(&[0, wire::MAP_INFO]).await;
        assert_eq!(refusal.id, 0, "{mode} was admitted to a world");
        let mut reader = Reader::new(&refusal.body);
        assert_eq!(reader.i32().unwrap(), 0);
        assert!(!reader.utf().unwrap().is_empty());
        reader.done().unwrap();
        drop(socket);
        released(&a, account).await;
        assert_eq!(
            a.store.load(account, character.id).await.unwrap(),
            character
        );
    }
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket_load(&mut socket, &dir, guid, character.id).await;
    assert_eq!(
        a.store.load(account, character.id).await.unwrap(),
        character
    );
    drop(socket);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropped_connection_names_its_cause_across_restarts() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (port, server) = socket_server(&a).await;
    let account = a.store.list("drop-cause".into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    let id = socket_load(&mut socket, &dir, "drop-cause", ch.id).await;
    let hit = |_| {
        let mut hit = Writer::new();
        hit.u8(0).i32(id);
        Packet {
            id: 24,
            body: hit.0,
        }
    };
    socket.send_frame((0..513).map(hit).collect()).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while a.diagnostics.previous_drop(account).is_null() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "rate-limited drop was not recorded"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let reopened = diagnostics::Diagnostics::open(dir.join("bug-reports")).unwrap();
    let saved = reopened.previous_drop(account);
    assert_eq!(saved["transport"]["reason"], "Output");
    assert_eq!(saved["transport"]["cause"], "input_rate_exceeded");
    drop(socket);
    server.abort();
}
