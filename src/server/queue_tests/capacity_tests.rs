use super::*;
use serde_json::{Value, json};
use std::fs;

async fn capacity(a: &App) -> Value {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer native-capacity-test".parse().unwrap(),
    );
    let response = testing::command(
        State(a.clone()),
        headers,
        Json(json!({"id":"after-disconnect","command":{"op":"prepare_realm_capacity","limit":4}})),
    )
    .await;
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn community_social_capacity_rejects_normal_native_create_load_and_hello_history_without_spawn()
 {
    for selection in [Some(wire::CREATE), Some(wire::LOAD), None] {
        let (dir, mut a) = account_actions::tests::fixture();
        fs::write(
            dir.join("testing.json"),
            br#"{"schema":1,"provenance":"prepared"}"#,
        )
        .unwrap();
        a.testing = testing::Testing::open(&dir).unwrap();
        a.control = Some("native-capacity-test".into());
        a.world.clock.set_paused(true);
        let guid = "normal-native-no-spawn";
        let account = a.store.list(guid.into()).await.unwrap().id;
        let existing = if selection == Some(wire::LOAD) {
            Some(
                a.store
                    .create(account, a.content.get(WIZARD).unwrap())
                    .await
                    .unwrap()
                    .id,
            )
        } else {
            None
        };
        let (input, incoming) = mpsc::unbounded_channel();
        let (outgoing, mut output) = mpsc::unbounded_channel();
        let stream = Box::pin(futures_util::stream::unfold(incoming, |mut rx| async {
            rx.recv()
                .await
                .map(|message| (Ok::<_, axum::Error>(message), rx))
        }));
        let sink = Box::pin(futures_util::sink::unfold(outgoing, |tx, message| async {
            tx.send(message)?;
            Ok::<_, mpsc::error::SendError<Message>>(tx)
        }));
        let connection = tokio::spawn(connection_io(sink, stream, a.clone(), None, None));
        let mut codec = Codec::client();
        send_commands(&input, &mut codec, [hello(&dir, guid)]);
        assert_eq!(
            next_packet(&mut output, &mut codec).await.id,
            wire::MAP_INFO
        );
        let character = if let Some(id) = selection {
            let mut body = Writer::new();
            if id == wire::CREATE {
                body.i16(WIZARD as i16);
            } else {
                body.i32(existing.unwrap());
            }
            send_commands(&input, &mut codec, [Packet { id, body: body.0 }]);
            let created = next_packet(&mut output, &mut codec).await;
            assert_eq!(created.id, wire::CREATE_SUCCESS);
            let mut r = Reader::new(&created.body);
            let player = r.i32().unwrap();
            let character = r.i32().unwrap();
            r.done().unwrap();
            assert!(
                a.world
                    .for_player(player)
                    .call(move |w| Ok(w.players[&player].character.id == character))
                    .await
                    .unwrap()
            );
            Some(character)
        } else {
            None
        };
        assert_eq!(a.owners.lock().unwrap().len(), 1);
        drop(input);
        tokio::time::timeout(Duration::from_secs(10), connection)
            .await
            .unwrap()
            .unwrap();
        assert!(a.owners.lock().unwrap().is_empty());
        assert!(a.quarantined.lock().unwrap().is_empty());
        assert_eq!(
            a.world
                .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
                .await
                .unwrap(),
            0
        );
        if let Some(character) = character {
            assert!(a.store.load(account, character).await.unwrap().alive);
        }
        let testing = dir.join("testing");
        assert!(!fs::read_dir(&testing).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .as_encoded_bytes()
                .ends_with(b".spawn.json")
        }));
        let global = a.max_players;
        let before = a
            .world
            .directory(|d| {
                let realm = d.initial(-3)?;
                Ok(d.worlds[&realm].max_players)
            })
            .await
            .unwrap();
        let result = capacity(&a).await;
        println!(
            "normal native admission-history evidence: {} selection={selection:?} response={result}",
            dir.display()
        );
        assert_eq!(
            result["status"], "failed",
            "normal native enrollment must remain fenced after disconnect: {result}"
        );
        assert!(result["error"].as_str().unwrap().contains("admission"));
        assert_eq!(a.max_players, global);
        assert_eq!(
            a.world
                .directory(|d| {
                    let realm = d.initial(-3)?;
                    Ok(d.worlds[&realm].max_players)
                })
                .await
                .unwrap(),
            before
        );
        assert!(!testing.join("prepared-realm-capacity.json").exists());
        a.shutdown.send_replace(true);
    }
}
