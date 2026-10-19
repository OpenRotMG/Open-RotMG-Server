#[allow(dead_code)]
#[path = "../src/wire.rs"]
mod wire;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use wire::{Codec, Packet, Reader, Writer};

struct Server {
    child: Child,
    root: PathBuf,
    base: String,
    log: PathBuf,
}
impl Server {
    async fn start(root: &Path) -> Self {
        let config = root.join("config.json");
        let value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        let base = value["origin"].as_str().unwrap().to_owned();
        let log = root.join(format!("server-{}.log", rand::random::<u64>()));
        let output = std::fs::File::create(&log).unwrap();
        let binary = Path::new(env!("CARGO_BIN_EXE_openrotmg"));
        println!(
            "lifecycle_drain real binary={} sha256={:x} state={} log={}",
            binary.display(),
            Sha256::digest(std::fs::read(binary).unwrap()),
            root.display(),
            log.display()
        );
        let port = base.rsplit(':').next().unwrap().parse::<u16>().unwrap();
        assert!(
            listeners(port).is_empty(),
            "allocated child port is already listening on IPv4/IPv6"
        );
        let child = Command::new(binary)
            .args(["--config", config.to_str().unwrap()])
            .env("DISCORD_LINK_TOKEN_FILE", root.join("private.token"))
            .env(
                "RUST_LOG",
                "openrotmg=info,openrotmg::server::lifecycle=debug",
            )
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        println!("owned lifecycle_drain child pid={} base={base}", child.id());
        let mut server = Self {
            child,
            root: root.to_owned(),
            base,
            log,
        };
        let start = Instant::now();
        loop {
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "child failed startup: {}",
                std::fs::read_to_string(&server.log).unwrap()
            );
            if client()
                .get(format!("{}/health", server.base))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "child not healthy: {}",
                server.log.display()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let listening = listeners(port);
        assert_eq!(
            listening.len(),
            1,
            "unexpected IPv4/IPv6 listener on child port"
        );
        let inode = &listening[0].1;
        let owns_listener = std::fs::read_dir(format!("/proc/{}/fd", server.child.id()))
            .unwrap()
            .any(|fd| {
                std::fs::read_link(fd.unwrap().path())
                    .is_ok_and(|p| p.to_string_lossy() == format!("socket:[{inode}]"))
            });
        assert!(owns_listener, "child does not own its listener");
        println!(
            "owned IPv4/IPv6 listener receipt pid={} port={port} entries={listening:?}",
            server.child.id()
        );
        server
    }
    fn term(&mut self) {
        assert!(self.child.try_wait().unwrap().is_none());
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) },
            0
        );
    }
    async fn exited(&mut self, success: bool) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(
                    status.success(),
                    success,
                    "child status={status}: {}",
                    std::fs::read_to_string(&self.log).unwrap()
                );
                println!(
                    "owned child pid={} exit={status} elapsed_ms={}",
                    self.child.id(),
                    start.elapsed().as_millis()
                );
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(18),
                "SIGTERM exceeded end-to-end bound: {}",
                self.log.display()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    async fn drain(&self, op: &str, body: Value) -> reqwest::Response {
        client()
            .post(format!("{}/internal/lifecycle/{op}", self.base))
            .bearer_auth("a".repeat(64))
            .json(&body)
            .send()
            .await
            .unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
    }
}
fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}
fn root() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("lifecycle-drain-process-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = json!({"bind":address.to_string(),"origin":format!("http://{address}"),"data":repo.join("data"),"client":repo.parent().unwrap().join("client"),"state":root.join("state"),"max_players":16,"behind_proxy":false,"shore_bots":false,"bot_llm":false,"realm_seed":12351});
    std::fs::write(
        root.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("private.token"), "a".repeat(64)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        root.join("private.token"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    root
}
fn listeners(port: u16) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for family in ["tcp", "tcp6"] {
        for line in std::fs::read_to_string(format!("/proc/net/{family}"))
            .unwrap()
            .lines()
            .skip(1)
        {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields[3] == "0A"
                && u16::from_str_radix(fields[1].rsplit(':').next().unwrap(), 16).unwrap() == port
            {
                found.push((family.into(), fields[9].into()));
            }
        }
    }
    found
}
fn queued_bytes(pid: u32, server_port: u16, peer_port: u16) -> usize {
    let table = std::fs::read_to_string(format!("/proc/{pid}/net/tcp")).unwrap();
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let port =
                |field: &str| u16::from_str_radix(field.rsplit(':').next().unwrap(), 16).unwrap();
            (port(fields[1]) == server_port && port(fields[2]) == peer_port)
                .then(|| usize::from_str_radix(fields[4].split(':').next().unwrap(), 16).unwrap())
        })
        .sum()
}
fn rows(root: &Path) -> Value {
    let db = rusqlite::Connection::open(root.join("state/game.db")).unwrap();
    let mut statement = db
        .prepare("SELECT id,account,stats,items,revision,alive FROM characters ORDER BY id")
        .unwrap();
    json!(
        statement
            .query_map([], |r| Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, i32>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i32>(4)?,
                r.get::<_, bool>(5)?
            )))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    )
}
struct Native {
    io: TcpStream,
    codec: Codec,
    pending: std::collections::VecDeque<Packet>,
}
impl Native {
    async fn connect(server: &Server) -> Self {
        use std::os::fd::AsRawFd;
        let address = server.base.strip_prefix("http://").unwrap();
        let mut io = TcpStream::connect(address).await.unwrap();
        let n = 4096i32;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    io.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    (&n as *const i32).cast(),
                    std::mem::size_of_val(&n) as _,
                )
            },
            0
        );
        io.write_all(format!("GET /game HTTP/1.1\r\nHost: {address}\r\nOrigin: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",server.base).as_bytes()).await.unwrap();
        let mut response = vec![];
        while !response.ends_with(b"\r\n\r\n") {
            response.push(io.read_u8().await.unwrap());
            assert!(response.len() < 8192);
        }
        assert!(
            String::from_utf8(response)
                .unwrap()
                .starts_with("HTTP/1.1 101")
        );
        Self {
            io,
            codec: Codec::client(),
            pending: Default::default(),
        }
    }
    async fn send(&mut self, packet: Packet) {
        let bytes = self.codec.frame(packet.id, packet.body);
        let mut frame = vec![0x82];
        if bytes.len() < 126 {
            frame.push(0x80 | bytes.len() as u8);
        } else {
            frame.push(0xfe);
            frame.extend((bytes.len() as u16).to_be_bytes());
        }
        frame.extend([0; 4]);
        frame.extend(bytes);
        self.io.write_all(&frame).await.unwrap();
    }
    async fn receive(&mut self, expected: u8) -> Packet {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                while let Some(packet) = self.pending.pop_front() {
                    if packet.id == wire::UPDATE {
                        self.send(Packet {
                            id: wire::UPDATE_ACK,
                            body: vec![],
                        })
                        .await;
                    } else if packet.id == wire::PING {
                        let mut pong = packet.body;
                        pong.extend(0i32.to_be_bytes());
                        self.send(Packet {
                            id: wire::PONG,
                            body: pong,
                        })
                        .await;
                    } else if packet.id == expected {
                        return packet;
                    }
                }
                let op = self.io.read_u8().await.unwrap();
                assert_eq!(op & 15, 2, "unexpected frame opcode");
                let n = self.io.read_u8().await.unwrap();
                let n = match n & 127 {
                    126 => self.io.read_u16().await.unwrap() as usize,
                    127 => self.io.read_u64().await.unwrap() as usize,
                    n => n as usize,
                };
                assert!(n <= wire::MAX_FRAME);
                let mut bytes = vec![0; n];
                self.io.read_exact(&mut bytes).await.unwrap();
                self.pending.extend(self.codec.feed(&bytes).unwrap());
            }
        })
        .await
        .expect("native packet missing")
    }
    async fn enter(server: &Server, guid: &str, character: Option<i32>) -> (Self, i32, i32) {
        let response = client()
            .post(format!("{}/legacy-api/char/list", server.base))
            .form(&[("guid", guid), ("password", "")])
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let mut socket = Self::connect(server).await;
        use base64::{Engine, engine::general_purpose::STANDARD};
        use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, pkcs8::DecodePrivateKey};
        let key = RsaPrivateKey::from_pkcs8_pem(
            &std::fs::read_to_string(server.root.join("state/server.pem")).unwrap(),
        )
        .unwrap()
        .to_public_key();
        let encrypt = |s: &str| {
            STANDARD.encode(
                key.encrypt(&mut rand::rngs::OsRng, Pkcs1v15Encrypt, s.as_bytes())
                    .unwrap(),
            )
        };
        let mut body = Writer::new();
        body.utf("123.5.1")
            .i32(-2)
            .utf(&encrypt(guid))
            .utf(&encrypt(""))
            .utf("")
            .i32(0)
            .i16(0)
            .i32(0);
        for _ in 0..4 {
            body.utf("");
        }
        socket
            .send(Packet {
                id: wire::HELLO,
                body: body.0,
            })
            .await;
        socket.receive(wire::MAP_INFO).await;
        socket
            .send(match character {
                Some(id) => Packet {
                    id: wire::LOAD,
                    body: id.to_be_bytes().to_vec(),
                },
                None => Packet {
                    id: wire::CREATE,
                    body: (0x030ei16).to_be_bytes().to_vec(),
                },
            })
            .await;
        let success = socket.receive(wire::CREATE_SUCCESS).await;
        let mut r = Reader::new(&success.body);
        let player = r.i32().unwrap();
        let character = r.i32().unwrap();
        (socket, player, character)
    }
    async fn swap(&mut self, player: i32, items: &[i32]) {
        let mut body = Writer::new();
        body.i32(0).f32(0.).f32(0.);
        for slot in [4, 5] {
            body.i32(player).u8(slot).i16(items[slot as usize] as i16);
        }
        self.send(Packet {
            id: 65,
            body: body.0,
        })
        .await;
    }
}
fn snapshot(root: &Path, ch: i32) -> (Vec<i32>, i32) {
    let db = rusqlite::Connection::open(root.join("state/game.db")).unwrap();
    db.query_row(
        "SELECT items,revision FROM characters WHERE id=?1",
        [ch],
        |r| {
            Ok((
                serde_json::from_str(&r.get::<_, String>(0)?).unwrap(),
                r.get(1)?,
            ))
        },
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_drain_real_sigterm_inflight_sql_stalled_reader_hostile_peer_and_ordinary_restart()
 {
    let root = root();
    let mut server = Server::start(&root).await;
    let (mut normal, id, ch) = Native::enter(&server, "drain-process-normal", None).await;
    let (mut stalled, _, stalled_ch) = Native::enter(&server, "drain-process-stalled", None).await;
    let mut hostile = Native::connect(&server).await;
    hostile
        .send(Packet {
            id: 14,
            body: vec![],
        })
        .await;
    let (before, revision) = snapshot(&root, ch);
    let stalled_before = snapshot(&root, stalled_ch);
    let db = rusqlite::Connection::open(root.join("state/game.db")).unwrap();
    db.execute_batch("CREATE TABLE drain_audit(character INTEGER,items TEXT); CREATE TRIGGER drain_audit_update AFTER UPDATE ON characters BEGIN INSERT INTO drain_audit VALUES(NEW.id,NEW.items); END; BEGIN IMMEDIATE;").unwrap();
    let offset = std::fs::metadata(&server.log).unwrap().len() as usize;
    normal.swap(id, &before).await;
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(&server.log).unwrap();
        if text[offset..].contains("\"stage\":\"store\"") {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "inventory never reached Store stage: {}",
            server.log.display()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let stalled_port = stalled.io.local_addr().unwrap().port();
    let server_port = server.base.rsplit(':').next().unwrap().parse().unwrap();
    let flooding = tokio::spawn(async move {
        let mut frame = vec![0x89, 0xfd, 0, 0, 0, 0];
        frame.extend([0u8; 125]);
        for _ in 0..8192 {
            if stalled.io.write_all(&frame).await.is_err() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
        stalled
    });
    let pressure = Instant::now();
    let observed_queue = loop {
        let bytes = queued_bytes(server.child.id(), server_port, stalled_port);
        if bytes >= 8192 {
            break bytes;
        }
        assert!(
            pressure.elapsed() < Duration::from_secs(2),
            "stalled-reader fixture did not produce actual TCP backpressure"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    println!(
        "stalled reader observed pid={} server_port={server_port} peer_port={stalled_port} queued_bytes={observed_queue}",
        server.child.id()
    );
    server.term();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "server exited while accepted SQL was still blocked"
    );
    let stopping_status: Value = server
        .drain("prepare", json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        stopping_status["state"], "preparing",
        "SIGTERM readiness preceded accepted SQL completion"
    );
    let old_receipt = stopping_status["receipt"].clone();
    let cancellation = server.drain("cancel", old_receipt.clone()).await;
    assert_eq!(cancellation.status(), 409);
    assert_eq!(
        cancellation.json::<Value>().await.unwrap()["error"],
        "server termination cannot be cancelled"
    );
    db.execute_batch("COMMIT").unwrap();
    server.exited(true).await;
    flooding.abort();
    let _ = flooding.await;
    let mut expected = before;
    expected.swap(4, 5);
    assert_eq!(snapshot(&root, ch), (expected.clone(), revision + 1));
    assert_eq!(snapshot(&root, stalled_ch), stalled_before);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM drain_audit WHERE character=?1 AND items=?2",
            rusqlite::params![ch, serde_json::to_string(&expected).unwrap()],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        2,
        "inventory commit plus one ordinary final save, no replay"
    );
    let stopped_rows = rows(&root);
    std::fs::write(
        root.join("sqlite-before-restart.json"),
        serde_json::to_vec_pretty(&stopped_rows).unwrap(),
    )
    .unwrap();
    drop(normal);
    drop(hostile);
    drop(server);
    let mut restarted = Server::start(&root).await;
    assert_eq!(
        rows(&root),
        stopped_rows,
        "ordinary restart changed exact character rows"
    );
    let next: Value = restarted
        .drain("prepare", json!({}))
        .await
        .json()
        .await
        .unwrap();
    let next_receipt = next["receipt"].clone();
    assert_eq!(old_receipt["generation"], next_receipt["generation"]);
    assert_ne!(old_receipt["instance"], next_receipt["instance"]);
    assert_eq!(
        restarted
            .drain("status", old_receipt.clone())
            .await
            .status(),
        409
    );
    assert_eq!(restarted.drain("cancel", old_receipt).await.status(), 409);
    let checking = Instant::now();
    loop {
        let value: Value = restarted
            .drain("status", next_receipt.clone())
            .await
            .json()
            .await
            .unwrap();
        if value["state"] == "ready" {
            break;
        }
        assert_eq!(value["state"], "preparing");
        assert!(checking.elapsed() < Duration::from_secs(2));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(restarted.drain("cancel", next_receipt).await.status(), 200);
    let (reconnected, _, loaded) =
        Native::enter(&restarted, "drain-process-normal", Some(ch)).await;
    assert_eq!(loaded, ch);
    assert_eq!(snapshot(&root, ch), (expected, revision + 1));
    restarted.term();
    restarted.exited(true).await;
    drop(reconnected);
    println!("retained exact SQLite/restart evidence {}", root.display());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_drain_real_account_sql_failure_same_process_failed_receipt_cancel_and_healthy_reconnect()
 {
    let root = root();
    let mut server = Server::start(&root).await;
    let (bad, _, bad_ch) = Native::enter(&server, "drain-process-fenced", None).await;
    let (healthy, _, good_ch) = Native::enter(&server, "drain-process-healthy", None).await;
    let db = rusqlite::Connection::open(root.join("state/game.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER drain_failure BEFORE UPDATE ON characters WHEN OLD.id={bad_ch} BEGIN SELECT RAISE(ABORT,'process account-specific SQL failure'); END;")).unwrap();
    let p: Value = server
        .drain("prepare", json!({}))
        .await
        .json()
        .await
        .unwrap();
    let receipt = p["receipt"].clone();
    let start = Instant::now();
    let failed = loop {
        let value: Value = server
            .drain("status", receipt.clone())
            .await
            .json()
            .await
            .unwrap();
        if value["state"] != "preparing" {
            break value;
        }
        assert!(start.elapsed() < Duration::from_secs(16));
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(failed["state"], "failed");
    assert!(
        failed["errors"]
            .to_string()
            .contains("process account-specific SQL failure")
    );
    assert!(server.child.try_wait().unwrap().is_none());
    assert_eq!(
        client()
            .get(format!("{}/health", server.base))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(server.drain("cancel", receipt.clone()).await.status(), 200);
    let response = client()
        .post(format!("{}/legacy-api/char/delete", server.base))
        .form(&[
            ("guid", "drain-process-fenced"),
            ("password", ""),
            ("charId", &bad_ch.to_string()),
        ])
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        response.contains("active session"),
        "fenced account was admitted: {response}"
    );
    let (mut peer, id, loaded) =
        Native::enter(&server, "drain-process-healthy", Some(good_ch)).await;
    assert_eq!(loaded, good_ch);
    let (items, revision) = snapshot(&root, good_ch);
    peer.swap(id, &items).await;
    let start = Instant::now();
    loop {
        if snapshot(&root, good_ch).1 == revision + 1 {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        server.drain("status", receipt).await.status(),
        409,
        "cancelled readiness remained usable"
    );
    server.term();
    server.exited(false).await;
    drop(peer);
    drop(bad);
    drop(healthy);
    println!(
        "retained account-specific failure evidence {}",
        root.display()
    );
}
