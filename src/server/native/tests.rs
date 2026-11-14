use super::*;
use crate::{
    server::{self, lifecycle_finalize_tests::socket_hello},
    wire::{self, Codec, Packet, Reader},
};

async fn start(
    a: &App,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<io::Result<()>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = a.clone();
    (address, tokio::spawn(serve(app, listener)))
}

async fn receive(socket: &mut TcpStream, codec: &mut Codec, expected: u8) -> Packet {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let n = socket.read_u32().await.unwrap() as usize;
            assert!((5..=wire::MAX_FRAME).contains(&n));
            let mut frame = vec![0; n];
            frame[..4].copy_from_slice(&(n as u32).to_be_bytes());
            socket.read_exact(&mut frame[4..]).await.unwrap();
            for packet in codec.feed(&frame).unwrap() {
                if packet.id == expected {
                    return packet;
                }
            }
        }
    })
    .await
    .expect("native game response deadline")
}

#[tokio::test]
async fn native_account_list_requires_enabled_transport_and_keeps_browser_dns_unchanged() {
    use axum::{
        Form,
        extract::{ConnectInfo, Query, State},
        http::{HeaderMap, StatusCode},
    };
    use std::collections::HashMap;
    let (_, mut a) = server::test_app();
    let peer = ConnectInfo("127.0.0.1:1234".parse().unwrap());
    let form = HashMap::from([
        ("guid".into(), "native-list".into()),
        ("password".into(), "".into()),
    ]);
    let response = server::native_chars(
        State(a.clone()),
        peer,
        HeaderMap::new(),
        Query(HashMap::new()),
        Form(form.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    a.native = Some(crate::config::Native {
        bind: "127.0.0.1:2050".parse().unwrap(),
        host: "game.example.test".into(),
    });
    for native in [false, true] {
        let query = Query(HashMap::from([(
            "native".into(),
            if native { "1" } else { "0" }.into(),
        )]));
        let response = server::chars(
            State(a.clone()),
            peer,
            HeaderMap::new(),
            query,
            Form(form.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let xml = std::str::from_utf8(&body).unwrap();
        assert!(xml.contains(if native {
            "<DNS>game.example.test</DNS>"
        } else {
            "<DNS>127.0.0.1</DNS>"
        }));
    }
}

#[tokio::test]
async fn policy_is_nul_terminated_and_shares_connection_admission() {
    let (_, a) = server::test_app();
    let (address, task) = start(&a).await;
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.write_all(POLICY_REQUEST).await.unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, POLICY);
    let permit = a.connections.clone().acquire_many_owned(32).await.unwrap();
    let mut denied = TcpStream::connect(address).await.unwrap();
    assert_eq!(
        denied.read_u8().await.unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
    drop(permit);
    a.shutdown.send_replace(true);
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_and_browser_use_the_same_character_and_release_ownership() {
    let (dir, a) = server::test_app();
    let account = a
        .store
        .characters("native-transport".into(), "".into())
        .await
        .unwrap();
    let character = a
        .store
        .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character_id = character.id;
    let (address, task) = start(&a).await;
    let mut socket = TcpStream::connect(address).await.unwrap();
    let mut codec = Codec::client();
    let hello = socket_hello(&dir, "native-transport", -2, None);
    // TCP does not preserve packet boundaries, including the initial length header.
    let bytes = codec.frame(hello.id, hello.body);
    for chunk in bytes.chunks(3) {
        socket.write_all(chunk).await.unwrap();
    }
    receive(&mut socket, &mut codec, wire::MAP_INFO).await;
    socket
        .write_all(&codec.frame(wire::LOAD, character_id.to_be_bytes().to_vec()))
        .await
        .unwrap();
    let success = receive(&mut socket, &mut codec, wire::CREATE_SUCCESS).await;
    let player = Reader::new(&success.body).i32().unwrap();
    let marked = a
        .world
        .for_player(player)
        .call(move |w| {
            let character = &mut w.players.get_mut(&player).unwrap().character;
            character.lifetime.tiles += 1;
            Ok(character.lifetime.tiles)
        })
        .await
        .unwrap();
    drop(socket);
    tokio::time::timeout(Duration::from_secs(5), async {
        while a.owners.lock().unwrap().contains_key(&account.id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let saved = a.store.load(account.id, character_id).await.unwrap();
    assert!(saved.lifetime.tiles >= marked);
    let (port, browser) = server::lifecycle_finalize_tests::socket_server(&a).await;
    let mut browser_socket =
        server::lifecycle_finalize_tests::NativeSocket::connect(port, &a.origin).await;
    server::lifecycle_finalize_tests::socket_load(
        &mut browser_socket,
        &dir,
        "native-transport",
        character_id,
    )
    .await;
    a.shutdown.send_replace(true);
    task.await.unwrap().unwrap();
    browser.abort();
}

#[tokio::test]
async fn malformed_frames_release_permits_and_shutdown_interrupts_idle_handshakes() {
    let (_, a) = server::test_app();
    let (address, task) = start(&a).await;
    let mut invalid = TcpStream::connect(address).await.unwrap();
    invalid.write_all(&[0, 0, 0, 4, 0]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), invalid.read_u8())
        .await
        .unwrap()
        .unwrap_err();
    let mut idle = TcpStream::connect(address).await.unwrap();
    idle.write_all(b"<pol").await.unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    a.shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(a.connections.available_permits(), 32);
}
