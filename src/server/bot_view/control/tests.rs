use super::*;
use crate::bots::cctv::Cctv;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

fn fixture() -> (Arc<Cctv>, Owner, watch::Receiver<Option<Value>>) {
    let cctv = Cctv::test_catalogue();
    let ip = IpAddr::from([127, 0, 0, 1]);
    let viewer = Viewer::join(&cctv, "A", ip, true).unwrap();
    let (view, replies) = View::new(viewer, "A".into(), ip);
    let token = cctv.register_view(&view).unwrap();
    (cctv.clone(), Owner { view, token, cctv }, replies)
}

#[test]
fn command_schema_is_strict_and_generation_is_unsigned() {
    for valid in [
        r#"{"generation":1,"target":{"kind":"bot","bot":"A"}}"#,
        r#"{"generation":1,"target":{"kind":"player","source":1,"target":2}}"#,
    ] {
        assert!(serde_json::from_str::<Request>(valid).is_ok());
    }
    for invalid in [
        r#"{"generation":-1,"target":{"kind":"bot","bot":"A"}}"#,
        r#"{"generation":4294967296,"target":{"kind":"bot","bot":"A"}}"#,
        r#"{"generation":1,"target":{"kind":"bot","bot":"A","source":1}}"#,
        r#"{"generation":1,"target":{"kind":"human","source":1,"target":2}}"#,
        r#"{"generation":1,"target":{"kind":"player","source":1,"target":2,"bot":"B"}}"#,
        r#"{"generation":1,"target":{"kind":"bot","bot":"A"},"view":"stolen"}"#,
        r#"{"generation":1,"generation":2,"target":{"kind":"bot","bot":"A"}}"#,
    ] {
        assert!(
            serde_json::from_str::<Request>(invalid).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn capability_is_ip_bound_exclusive_closed_and_weakly_registered() {
    let (cctv, owner, _replies) = fixture();
    assert_eq!(owner.token.len(), 32);
    assert!(
        owner
            .token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(cctv.view("unknown").is_none());
    assert!(cctv.view(&format!("G{}", &owner.token[1..])).is_none());
    assert!(owner.view.acquire(IpAddr::from([127, 0, 0, 2])).is_err());
    let lease = owner.view.acquire(owner.view.ip).unwrap();
    assert!(owner.view.acquire(owner.view.ip).is_err());
    let weak = Arc::downgrade(&owner.view);
    let token = owner.token.clone();
    drop(owner);
    assert!(cctv.view(&token).is_none());
    assert!(lease.view.acquire(lease.view.ip).is_err());
    let ip = lease.view.ip;
    let _second = Viewer::join(&cctv, "A", ip, true).unwrap();
    assert!(
        Viewer::join(&cctv, "A", ip, true).is_err(),
        "draining native data must still own its admission"
    );
    drop(lease);
    assert!(weak.upgrade().is_none());
    let _replacement = Viewer::join(&cctv, "A", ip, true).unwrap();
}

#[test]
fn weak_registry_is_bounded_even_while_closed_socket_tasks_still_hold_views() {
    let cctv = Cctv::test_catalogue();
    let mut held = Vec::new();
    for ip in 1..=32 {
        let ip = IpAddr::from([127, 0, 0, ip]);
        let viewer = Viewer::join(&cctv, "A", ip, true).unwrap();
        let (view, _) = View::new(viewer, "A".into(), ip);
        cctv.register_view(&view).unwrap();
        view.close();
        held.push(view);
    }
    let ip = IpAddr::from([127, 0, 0, 33]);
    assert!(Viewer::join(&cctv, "A", ip, true).is_err());
    drop(held);
    let viewer = Viewer::join(&cctv, "A", ip, true).unwrap();
    let (next, _) = View::new(viewer, "A".into(), ip);
    assert!(cctv.register_view(&next).is_ok());
}

#[test]
fn reconnect_reuses_admission_and_late_lease_cannot_remove_replacement() {
    let (cctv, owner, _replies) = fixture();
    let mut first = owner.view.acquire(owner.view.ip).unwrap();
    assert_eq!(
        owner.view.commit(1, "B", None).unwrap(),
        json!({"type":"selected","bot":"B","generation":1})
    );
    assert!(first.changed.has_changed().unwrap());
    assert!(owner.view.acquire(owner.view.ip).is_err());
    first.release();
    owner.view.close_socket(Some(first.id));
    assert!(
        !first.closed(),
        "a released socket cannot close the view during its reconnect gap"
    );
    let replacement = owner.view.acquire(owner.view.ip).unwrap();
    assert_eq!((replacement.bot.as_str(), replacement.generation), ("B", 1));
    owner.view.close_socket(Some(first.id));
    assert!(!replacement.closed());
    drop(first);
    assert_eq!(owner.view.lock().active, Some(replacement.id));
    assert!(owner.view.acquire(owner.view.ip).is_err());
    // Reconnecting did not consume the second per-IP slot.
    let _second = Viewer::join(&cctv, "A", owner.view.ip, true).unwrap();
    drop(replacement);
    assert!(owner.view.acquire(owner.view.ip).is_ok());
}

#[test]
fn selection_fences_generation_source_socket_and_preserves_rejections() {
    let (_cctv, owner, _replies) = fixture();
    let mut lease = owner.view.acquire(owner.view.ip).unwrap();
    owner.view.lock().attached = Some((1, 101));
    for (generation, target, source) in [
        (0, "B", None),
        (2, "B", None),
        (1, "missing", None),
        (1, "B", Some((lease.id, (2, 101)))),
        (1, "B", Some((lease.id + 1, (1, 101)))),
    ] {
        assert!(owner.view.commit(generation, target, source).is_err());
        assert_eq!(owner.view.selection(), ("A".into(), 0));
    }
    let old_id = lease.id;
    lease.release();
    let new_lease = owner.view.acquire(owner.view.ip).unwrap();
    owner.view.lock().attached = Some((1, 101));
    assert!(owner.view.commit(1, "B", Some((old_id, (1, 101)))).is_err());
    owner
        .view
        .commit(1, "B", Some((new_lease.id, (1, 101))))
        .unwrap();
    assert!(owner.view.lock().attached.is_none());
    assert!(owner.view.commit(1, "A", None).is_err());
    assert_eq!(owner.view.selection(), ("B".into(), 1));
}

struct Socket(TcpStream);
impl Socket {
    async fn open(address: SocketAddr, path: &str, origin: &str, ip: &str) -> (Self, String) {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nOrigin: {origin}\r\nX-Forwarded-For: {ip}\r\n\r\n").as_bytes()).await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(socket.read_u8().await.unwrap());
        }
        (Self(socket), String::from_utf8(header).unwrap())
    }
    async fn send(&mut self, opcode: u8, body: &[u8]) {
        assert!(body.len() < 126);
        let mut frame = vec![0x80 | opcode, 0x80 | body.len() as u8, 0, 0, 0, 0];
        frame.extend(body);
        self.0.write_all(&frame).await.unwrap();
    }
    async fn next(&mut self) -> (u8, Vec<u8>) {
        loop {
            let opcode = self.0.read_u8().await.unwrap() & 15;
            let length = self.0.read_u8().await.unwrap();
            assert!(length < 128);
            let length = match length {
                126 => self.0.read_u16().await.unwrap() as usize,
                127 => panic!("unexpected large test frame"),
                other => other as usize,
            };
            let mut body = vec![0; length];
            self.0.read_exact(&mut body).await.unwrap();
            if opcode == 9 {
                self.send(10, &body).await;
            } else {
                return (opcode, body);
            }
        }
    }
    async fn json(&mut self) -> Value {
        let (opcode, body) = self.next().await;
        assert_eq!(opcode, 1);
        serde_json::from_slice(&body).unwrap()
    }
}

#[tokio::test]
async fn protocol_ready_switch_reconnect_and_control_close_cancel_bootstrap() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (_, mut app) = crate::server::test_app();
        app.cctv = Cctv::test_catalogue();
        app.behind_proxy = true;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new()
            .route("/api/bots/{name}/control", axum::routing::get(socket))
            .route("/api/bots/views/{view}/socket", axum::routing::get(data))
            .with_state(app.clone());
        let mut shutdown = app.shutdown.subscribe();
        let server = tokio::spawn(async move { axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async move { let _ = shutdown.wait_for(|stop| *stop).await; }).await.unwrap(); });
        let origin = app.bot_view_origin.as_deref().unwrap();
        let (_, denied) = Socket::open(address, "/api/bots/A/control", "http://unapproved", "127.0.0.1").await;
        assert!(denied.starts_with("HTTP/1.1 403"));
        let (mut control, header) = Socket::open(address, "/api/bots/A/control", origin, "127.0.0.1").await;
        assert!(header.starts_with("HTTP/1.1 101"));
        let ready = control.json().await;
        let token = ready["view"].as_str().unwrap().to_owned();
        assert_eq!(ready, json!({"type":"ready","view":token,"bot":"A","generation":0,"parent_origin":app.origin}));
        let path = format!("/api/bots/views/{token}/socket?cursor=2");
        let (_, missing) = Socket::open(address, "/api/bots/views/invalid/socket?cursor=2", origin, "127.0.0.1").await;
        assert!(missing.starts_with("HTTP/1.1 404"));
        let (_, denied) = Socket::open(address, &path, origin, "127.0.0.2").await;
        assert!(denied.starts_with("HTTP/1.1 403"));
        let (mut old, header) = Socket::open(address, &path, origin, "127.0.0.1").await;
        assert!(header.starts_with("HTTP/1.1 101"));
        let (_, denied) = Socket::open(address, &path, origin, "127.0.0.1").await;
        assert!(denied.starts_with("HTTP/1.1 403"));
        let mut hello = Writer::new();
        hello.utf("123.5.1");
        old.send(2, &Codec::client().frame(wire::HELLO, hello.0)).await;
        assert_eq!(control.json().await, json!({"type":"unavailable","bot":"A","generation":0}));
        control.send(1, br#"{"generation":1,"target":{"kind":"bot","bot":"B"}}"#).await;
        assert_eq!(control.json().await, json!({"type":"selected","bot":"B","generation":1}));
        let (opcode, reconnect) = old.next().await;
        assert_eq!(opcode, 2);
        let mut codec = Codec::client();
        assert_eq!(codec.feed(&reconnect).unwrap()[0].id, 39);
        // Receiving RECONNECT proves the old lease was released, even with its socket still held here.
        let (replacement, header) = Socket::open(address, &path, origin, "127.0.0.1").await;
        assert!(header.starts_with("HTTP/1.1 101"));
        control.send(1, br#"{"generation":1,"target":{"kind":"bot","bot":"A"}}"#).await;
        assert_eq!(control.json().await, json!({"type":"rejected","bot":"B","generation":1,"requested":1,"error":"stale switch generation"}));
        let retained = app.cctv.view(&token).unwrap();
        retained.lock().attached = Some((2, 202));
        control.send(1, br#"{"generation":2,"target":{"kind":"player","source":2,"target":1}}"#).await;
        assert_eq!(control.json().await, json!({"type":"rejected","bot":"B","generation":1,"requested":2,"error":"player is not a visible active bot in this perspective"}));
        let mut closed = retained.changed.subscribe();
        control.send(8, &[]).await;
        closed.changed().await.unwrap();
        assert!(retained.acquire(IpAddr::from([127,0,0,1])).is_err());
        assert!(app.cctv.view(&token).is_none());
        let (_, missing) = Socket::open(address, &path, origin, "127.0.0.1").await;
        assert!(missing.starts_with("HTTP/1.1 404"));
        drop((old, replacement, control, retained));
        app.shutdown.send(true).unwrap();
        server.await.unwrap();
        drop(app);
        }).await.unwrap();
}

#[tokio::test]
async fn attachment_notifications_coalesce_during_a_fast_switch() {
    let (_, mut app) = crate::server::test_app();
    let (cctv, owner, mut replies) = fixture();
    app.cctv = cctv;
    let mut first = owner.view.acquire(owner.view.ip).unwrap();
    let (_, receiver) = tokio::sync::broadcast::channel(1);
    let mut attached = Attachment {
        world: 1,
        account: 11,
        player: 1,
        character: 101,
        map: vec![],
        snapshot: vec![],
        receiver,
    };
    assert!(first.attached(&app, &attached));
    owner.view.commit(1, "B", None).unwrap();
    first.release();
    let replacement = owner.view.acquire(owner.view.ip).unwrap();
    attached.account = 22;
    attached.player = 2;
    attached.character = 202;
    assert!(
        replacement.attached(&app, &attached),
        "latest attachment must replace the unconsumed older one"
    );
    assert_eq!(
        replies.borrow_and_update().clone(),
        Some(json!({"type":"attached","bot":"B","generation":1,"player":2,"character":202}))
    );
    drop((first, replacement, owner, app));
}

#[test]
fn initial_and_reconnect_deadlines_fail_closed() {
    let (_cctv, owner, _replies) = fixture();
    let initial = owner.view.lock().deadline;
    assert!(initial > Instant::now() + Duration::from_secs(69));
    let mut lease = owner.view.acquire(owner.view.ip).unwrap();
    lease.release();
    let gap = owner.view.lock().deadline;
    assert!(gap > Instant::now() + Duration::from_secs(14));
    assert!(gap < Instant::now() + Duration::from_secs(16));
    owner.view.lock().deadline = Instant::now() - Duration::from_secs(1);
    assert!(owner.view.acquire(owner.view.ip).is_err());
    owner.view.close();
    assert!(lease.closed());
}
