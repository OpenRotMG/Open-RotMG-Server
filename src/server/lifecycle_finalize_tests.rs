use super::*;
use std::time::Duration;

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
