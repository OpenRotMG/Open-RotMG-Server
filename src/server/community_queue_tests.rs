use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, pkcs8::DecodePrivateKey};
use std::{path::Path, time::Duration};

#[path = "queue_tests/capacity_tests.rs"]
mod capacity_tests;

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
