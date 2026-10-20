use super::output::encode_ready;
use super::*;

#[test]
fn output_batches_preserve_cipher_stream_fifo_and_retirement_boundaries() {
    let (tx, mut rx) = wire::outbox::channel(64);
    let mut expected = Codec::server();
    let mut bytes = Vec::new();
    for index in 0_i32..40 {
        let body = index.to_be_bytes().to_vec();
        if index < 32 {
            bytes.extend(expected.frame(wire::PING, body.clone()));
        }
        tx.try_send(Packet {
            id: wire::PING,
            body,
        })
        .unwrap();
    }
    let mut codec = Codec::server();
    let first = rx.try_recv().unwrap();
    let batch = encode_ready(&mut codec, first, &mut rx, &mut None, None);
    assert_eq!(batch.packets, 32);
    assert!(!batch.reconnect);
    assert_eq!(batch.bytes, bytes);
    assert_eq!(rx.len(), 8);
    let decoded = Codec::client().feed(&batch.bytes).unwrap();
    assert_eq!(decoded.len(), 32);
    assert_eq!(decoded[31].body, 31_i32.to_be_bytes());

    while rx.try_recv().is_ok() {}
    tx.try_send(Packet {
        id: 39,
        body: vec![],
    })
    .unwrap();
    tx.try_send(Packet {
        id: wire::NEW_TICK,
        body: vec![],
    })
    .unwrap();
    let first = rx.try_recv().unwrap();
    let batch = encode_ready(&mut codec, first, &mut rx, &mut None, None);
    assert!(batch.reconnect);
    assert_eq!(batch.packets, 1);
    assert_eq!(rx.try_recv().unwrap().id, wire::NEW_TICK);
}

#[tokio::test]
async fn output_drains_without_waiting_for_an_input_command() {
    let (tx, rx) = wire::outbox::channel(64);
    let (sent, mut observed) = mpsc::unbounded_channel();
    let sink = Box::pin(futures_util::sink::unfold(
        sent,
        |sent, message| async move {
            sent.send(message).unwrap();
            Ok::<_, std::convert::Infallible>(sent)
        },
    ));
    let (lost, _) = tokio::sync::watch::channel(false);
    let (_shutdown, shutdown) = tokio::sync::watch::channel(false);
    let (_drain, drain) = tokio::sync::watch::channel(0);
    let mut writer = output::Writer::start(sink, rx, lost.clone(), shutdown, drain, None);
    for index in 0_i32..200 {
        tx.send(Packet {
            id: wire::PING,
            body: index.to_be_bytes().to_vec(),
        })
        .await
        .unwrap();
    }
    writer.stop().await;
    let mut codec = Codec::client();
    let mut packets = Vec::new();
    while let Ok(Message::Binary(bytes)) = observed.try_recv() {
        packets.extend(codec.feed(&bytes).unwrap());
    }
    assert_eq!(packets.len(), 200);
    assert_eq!(writer.stats().packets, 200);
    assert!(!*lost.borrow());
    assert_eq!(
        packets
            .iter()
            .map(|p| i32::from_be_bytes(p.body[..].try_into().unwrap()))
            .collect::<Vec<_>>(),
        (0..200).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_blocked_socket_keeps_its_configured_packet_limit() {
    let (tx, rx) = wire::outbox::channel(64);
    let (entered, ready) = tokio::sync::oneshot::channel();
    let sink = Box::pin(futures_util::sink::unfold(
        entered,
        |entered: tokio::sync::oneshot::Sender<()>, _: Message| async move {
            entered.send(()).unwrap();
            std::future::pending::<Result<tokio::sync::oneshot::Sender<()>, std::convert::Infallible>>().await
        },
    ));
    let (lost, _) = tokio::sync::watch::channel(false);
    let (_shutdown, shutdown) = tokio::sync::watch::channel(false);
    let (_drain, drain) = tokio::sync::watch::channel(0);
    let mut writer = output::Writer::start(sink, rx, lost.clone(), shutdown, drain, None);
    tx.send(Packet {
        id: wire::PING,
        body: vec![0; 4],
    })
    .await
    .unwrap();
    ready.await.unwrap();
    for _ in 0..64 {
        tx.try_send(Packet {
            id: wire::PING,
            body: vec![0; 4],
        })
        .unwrap();
    }
    assert!(matches!(
        tx.try_send(Packet {
            id: wire::PING,
            body: vec![0; 4]
        }),
        Err(mpsc::error::TrySendError::Full(_))
    ));
    lost.send_replace(true);
    writer.stop().await;
    assert_eq!(writer.stats().queued_at_stop, 64);
}

#[test]
fn output_byte_budget_does_not_split_a_native_packet() {
    let (tx, mut rx) = wire::outbox::channel(64);
    let body = vec![7; 70_000];
    tx.try_send(Packet {
        id: wire::UPDATE,
        body: body.clone(),
    })
    .unwrap();
    tx.try_send(Packet {
        id: wire::PING,
        body: vec![0; 4],
    })
    .unwrap();
    let first = rx.try_recv().unwrap();
    let batch = encode_ready(&mut Codec::server(), first, &mut rx, &mut None, None);
    assert_eq!(batch.packets, 1);
    assert_eq!(Codec::client().feed(&batch.bytes).unwrap()[0].body, body);
    assert_eq!(rx.len(), 1);
}
