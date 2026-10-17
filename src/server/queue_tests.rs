use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, pkcs8::DecodePrivateKey};
use std::{path::Path, time::Duration};

fn hello(dir: &Path, guid: &str) -> Packet {
    let key =
        RsaPrivateKey::from_pkcs8_pem(&std::fs::read_to_string(dir.join("server.pem")).unwrap())
            .unwrap();
    let guid = STANDARD.encode(
        key.to_public_key()
            .encrypt(&mut rand::rngs::OsRng, Pkcs1v15Encrypt, guid.as_bytes())
            .unwrap(),
    );
    let mut out = Writer::new();
    out.utf("123.5.1")
        .i32(-2)
        .utf(&guid)
        .utf("")
        .utf("")
        .i32(0)
        .i16(0)
        .i32(0);
    for _ in 0..4 {
        out.utf("");
    }
    Packet {
        id: wire::HELLO,
        body: out.0,
    }
}

fn text(value: &str) -> Packet {
    let mut out = Writer::new();
    out.utf(value);
    Packet {
        id: wire::PLAYER_TEXT,
        body: out.0,
    }
}

async fn loaded(a: &App, guid: String) -> Session {
    let account = a.store.list(guid).await.unwrap().id;
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

async fn rejected_replies(closed: bool) {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    a.discord_link_token = None;
    for case in [
        "bootstrap",
        "discord",
        "bug",
        "ignore",
        "chat",
        "teleport",
        "item",
        "name",
        "reconnect",
        "tutorial",
    ] {
        let guid = format!("queue-{case}-{closed}");
        a.store.list(guid.clone()).await.unwrap();
        if case == "reconnect" {
            a.world = a.world.in_world(-1);
        }
        let mut session = if case == "bootstrap" {
            Session::default()
        } else {
            loaded(&a, guid.clone()).await
        };
        let packet = match case {
            "bootstrap" => hello(&dir, &guid),
            "discord" => text("/discord"),
            "bug" => text("/bug"),
            "ignore" => text("/ignore"),
            "chat" => text(""),
            "tutorial" => text("/tutorial"),
            "teleport" => Packet {
                id: wire::TELEPORT,
                body: i32::MAX.to_be_bytes().to_vec(),
            },
            "item" => {
                let mut out = Writer::new();
                out.i32(1).i32(i32::MAX).u8(4).i16(2594).f32(0.).f32(0.);
                Packet {
                    id: 30,
                    body: out.0,
                }
            }
            "name" => {
                let mut out = Writer::new();
                out.utf("bad name");
                Packet {
                    id: 33,
                    body: out.0,
                }
            }
            "reconnect" => Packet {
                id: 42,
                body: vec![],
            },
            _ => unreachable!(),
        };
        let (tx, rx) = mpsc::channel(1);
        let mut rx = if closed {
            drop(rx);
            None
        } else {
            tx.try_send(Packet {
                id: 255,
                body: vec![1, 2, 3],
            })
            .unwrap();
            Some(rx)
        };
        let (lost, _) = tokio::sync::watch::channel(false);
        let departing = session.player;
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            handle(
                &mut a,
                packet,
                &mut session,
                &tx,
                &lost,
                std::time::Instant::now(),
            ),
        )
        .await
        .unwrap_or_else(|_| panic!("{case}: handler waited for its own output consumer"));
        let error = result.unwrap_err();
        assert_eq!(
            error.downcast_ref::<ReplyQueueError>(),
            Some(&if closed {
                ReplyQueueError::Closed
            } else {
                ReplyQueueError::Full
            }),
            "{case}: {error:#}"
        );
        let reason = if closed { "closed" } else { "full" };
        assert!(
            error
                .to_string()
                .contains(&format!("connection reply queue is {reason}")),
            "{case}: {error:#}"
        );
        if let Some(rx) = rx.as_mut() {
            let retained = rx.try_recv().unwrap();
            assert_eq!((retained.id, retained.body), (255, vec![1, 2, 3]));
            assert!(rx.try_recv().is_err());
        }
        if matches!(case, "reconnect" | "tutorial") {
            assert!(session.player.is_none());
            assert!(!session.reconnecting);
            assert!(session.account.is_some());
            assert!(
                !a.world
                    .call(move |w| Ok(w.players.contains_key(&departing.unwrap())))
                    .await
                    .unwrap()
            );
        }
        if let Some(player) = session.player {
            depart(&a, player).await.unwrap();
        }
        if let Some(owner) = session.account.take() {
            owner.release();
        }
        assert!(a.owners.lock().unwrap().is_empty());
        a.world = a.world.in_world(-2);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn full_handler_reply_queue_is_fatal_without_waiting_for_capacity() {
    rejected_replies(false).await;
}

#[tokio::test]
async fn closed_handler_reply_queue_is_explicitly_fatal() {
    rejected_replies(true).await;
}

fn send_commands(
    input: &mpsc::UnboundedSender<Message>,
    codec: &mut Codec,
    packets: impl IntoIterator<Item = Packet>,
) {
    let mut bytes = Vec::new();
    for packet in packets {
        bytes.extend(codec.frame(packet.id, packet.body));
    }
    input.send(Message::Binary(bytes.into())).unwrap();
}

async fn next_packet(output: &mut mpsc::UnboundedReceiver<Message>, codec: &mut Codec) -> Packet {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match output
                .recv()
                .await
                .expect("connection ended before expected output")
            {
                Message::Binary(bytes) => {
                    let mut packets = codec.feed(&bytes).unwrap();
                    assert_eq!(packets.len(), 1);
                    return packets.remove(0);
                }
                Message::Pong(_) => {}
                other => panic!("unexpected output: {other:?}"),
            }
        }
    })
    .await
    .expect("connection stopped making progress")
}

enum Termination {
    Cancel,
    Shutdown,
    Full,
}

async fn connection_fifo_and_departure(termination: Termination) {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    a.world.clock.set_paused(true);
    a.discord_link_token = None;
    let guid = "connection-queue";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let revision = ch.revision;
    let hp = ch.hp - 1;
    let (input, incoming) = mpsc::unbounded_channel();
    let (outgoing, mut output) = mpsc::unbounded_channel();
    let stream = Box::pin(futures_util::stream::unfold(incoming, |mut rx| async {
        rx.recv()
            .await
            .map(|message| (Ok::<_, axum::Error>(message), rx))
    }));
    let saturation = Arc::new(Mutex::new(None::<mpsc::Sender<Packet>>));
    let refill = saturation.clone();
    let sink = Box::pin(futures_util::sink::unfold(outgoing, move |tx, message| {
        let refill = refill.clone();
        async move {
            if let Some(queue) = refill.lock().unwrap().as_ref() {
                // Keep saturation deterministic even if the loop drains output before handling input.
                match queue.try_send(Packet {
                    id: 255,
                    body: vec![],
                }) {
                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                    Err(error) => panic!("connection queue unexpectedly closed: {error}"),
                }
            } else {
                tx.send(message)?;
            }
            Ok::<_, mpsc::error::SendError<Message>>(tx)
        }
    }));
    let connection = tokio::spawn(connection_io(sink, stream, a.clone(), None, None));
    let mut codec = Codec::client();
    send_commands(
        &input,
        &mut codec,
        [
            hello(&dir, guid),
            Packet {
                id: wire::LOAD,
                body: character.to_be_bytes().to_vec(),
            },
        ],
    );
    assert_eq!(
        next_packet(&mut output, &mut codec).await.id,
        wire::MAP_INFO
    );
    let created = next_packet(&mut output, &mut codec).await;
    assert_eq!(created.id, wire::CREATE_SUCCESS);
    let mut r = Reader::new(&created.body);
    let player = r.i32().unwrap();
    assert_eq!(r.i32().unwrap(), character);
    r.done().unwrap();
    for id in [wire::UPDATE, 34, wire::ACCOUNT_LIST, wire::ACCOUNT_LIST] {
        assert_eq!(next_packet(&mut output, &mut codec).await.id, id);
    }
    a.world
        .for_player(player)
        .call(move |w| {
            w.players.get_mut(&player).unwrap().character.hp = hp;
            Ok(())
        })
        .await
        .unwrap();
    // Stay below the queue limit: the separate input reader makes the old 65-frame repro nondeterministic.
    send_commands(
        &input,
        &mut codec,
        (0..24).map(|index| text(if index % 2 == 0 { "/discord" } else { "" })),
    );
    for index in 0..24 {
        let packet = next_packet(&mut output, &mut codec).await;
        let expected = if index % 2 == 0 {
            crate::world::chat_private("Discord linking is not configured")
        } else {
            crate::world::chat_error("Chat message is empty.")
        };
        assert_eq!((packet.id, packet.body), (expected.id, expected.body));
    }
    let mut trade = Writer::new();
    trade.utf("NobodyHere");
    send_commands(
        &input,
        &mut codec,
        [
            Packet {
                id: 35,
                body: vec![],
            },
            Packet {
                id: 21,
                body: trade.0,
            },
            text("/discord"),
        ],
    );
    let inventory = next_packet(&mut output, &mut codec).await;
    assert_eq!(
        (inventory.id, inventory.body),
        (4, 1i32.to_be_bytes().to_vec())
    );
    let trade = next_packet(&mut output, &mut codec).await;
    assert_eq!(trade.id, 12);
    let mut r = Reader::new(&trade.body);
    assert_eq!(r.i32().unwrap(), 1);
    assert!(!r.utf().unwrap().is_empty());
    r.done().unwrap();
    assert_eq!(next_packet(&mut output, &mut codec).await.id, wire::TEXT);
    match termination {
        Termination::Cancel => assert!(a.kick(account).await.unwrap()),
        Termination::Shutdown => {
            a.shutdown.send_replace(true);
        }
        Termination::Full => {
            let queue = a
                .world
                .for_player(player)
                .call(move |w| Ok(w.players[&player].tx.as_ref().unwrap().packets.clone()))
                .await
                .unwrap();
            *saturation.lock().unwrap() = Some(queue.clone());
            for _ in 0..64 {
                queue
                    .try_send(Packet {
                        id: 255,
                        body: vec![],
                    })
                    .unwrap();
            }
            let mut item = Writer::new();
            item.i32(1).i32(i32::MAX).u8(4).i16(2594).f32(0.).f32(0.);
            send_commands(
                &input,
                &mut codec,
                [Packet {
                    id: 30,
                    body: item.0,
                }],
            );
        }
    }
    tokio::time::timeout(Duration::from_secs(3), connection)
        .await
        .unwrap()
        .unwrap();
    assert!(a.owners.lock().unwrap().is_empty());
    assert!(a.quarantined.lock().unwrap().is_empty());
    assert!(
        !a.world
            .call(move |w| Ok(w.players.contains_key(&player)))
            .await
            .unwrap()
    );
    let saved = a.store.load(account, character).await.unwrap();
    assert_eq!(saved.hp, hp);
    assert_eq!(saved.revision, revision);
    let owner = a.claim(account).unwrap();
    owner.release();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn connection_coalesced_fifo_cipher_and_cancel_commit_departure() {
    connection_fifo_and_departure(Termination::Cancel).await;
}

#[tokio::test]
async fn connection_coalesced_fifo_cipher_and_shutdown_commit_departure() {
    connection_fifo_and_departure(Termination::Shutdown).await;
}

#[tokio::test]
async fn connection_full_item_reply_is_fatal_before_inventory_recovery_and_commits_departure() {
    connection_fifo_and_departure(Termination::Full).await;
}
