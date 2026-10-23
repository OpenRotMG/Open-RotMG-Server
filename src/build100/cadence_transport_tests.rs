use super::*;

#[tokio::test]
async fn cadence_pacing_waiting_reply_drains_bounded_output_with_one_rc4_writer() {
    let (out, mut packets) = mpsc::channel(lifecycle::OUTPUT_CAPACITY);
    let (wire, mut received) = mpsc::channel(8);
    let mut socket = Box::pin(futures_util::sink::unfold(
        wire,
        |wire, message| async move {
            wire.send(message)
                .await
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            Ok::<_, std::io::Error>(wire)
        },
    ));
    let (reply, receive) = oneshot::channel();
    let task = tokio::spawn(async move {
        input_reply(
            &mut socket,
            &mut protocol::server_codec(),
            &mut packets,
            receive,
            7,
        )
        .await
    });
    let producer = tokio::spawn(async move {
        for serial in 0..24_i32 {
            let mut body = Writer::new();
            body.i32(serial);
            out.send(vec![Packet {
                id: 22,
                body: body.0,
            }])
            .await
            .unwrap();
        }
        out
    });
    let mut peer = Codec::server_with_keys(b"hPvJ(R}Mp$-cg", b"zCV2tsY0]s;|]");
    for serial in 0..24_i32 {
        let frame = tokio::time::timeout(Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = frame else {
            panic!("binary")
        };
        let decoded = peer.feed(&bytes).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, 22);
        assert_eq!(wire::Reader::new(&decoded[0].body).i32().unwrap(), serial);
        assert!(!task.is_finished());
    }
    let output = producer.await.unwrap();
    assert_eq!(output.capacity(), lifecycle::OUTPUT_CAPACITY);
    reply.send(Ok(())).unwrap();
    assert!(task.await.unwrap().unwrap().is_ok());
}

#[tokio::test]
async fn cadence_pacing_waiting_reply_reports_owner_output_and_socket_cancellation() {
    for which in 0..3 {
        let (out, mut packets) = mpsc::channel(lifecycle::OUTPUT_CAPACITY);
        let (reply, receive) = oneshot::channel();
        let mut socket = Box::pin(futures_util::sink::unfold((), |(), _| async {
            Err::<(), _>(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }));
        match which {
            0 => drop(reply),
            1 => drop(out),
            _ => {
                out.send(vec![Packet {
                    id: 20,
                    body: vec![],
                }])
                .await
                .unwrap();
            }
        }
        let error = input_reply(
            &mut socket,
            &mut protocol::server_codec(),
            &mut packets,
            receive,
            7,
        )
        .await
        .unwrap_err();
        assert!(
            format!("{error:#}")
                .contains(["owner stopped", "output channel closed", "broken pipe"][which])
        );
    }
}
