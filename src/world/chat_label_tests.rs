use super::*;
use crate::{content::Review, store::Store};

fn texts(inbox: &mut mpsc::Receiver<Packet>) -> Vec<(String, String, String)> {
    std::iter::from_fn(|| inbox.try_recv().ok())
        .filter(|packet| packet.id == wire::TEXT)
        .map(|packet| {
            let mut reader = wire::Reader::new(&packet.body);
            let name = reader.utf().unwrap();
            reader.i32().unwrap();
            reader.i32().unwrap();
            reader.u8().unwrap();
            let recipient = reader.utf().unwrap();
            let text = reader.utf().unwrap();
            reader.utf().unwrap();
            reader.done().unwrap();
            (name, recipient, text)
        })
        .collect()
}

#[tokio::test]
async fn reviewed_chat_labels_follow_account_ids_without_changing_commands_or_names() {
    let mut content = Content::load(std::path::Path::new("data")).unwrap();
    let store = Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let marked = store.list("reviewed-chat-marked".into()).await.unwrap().id;
    let plain = store.list("reviewed-chat-plain".into()).await.unwrap().id;
    content.reviewed_accounts = vec![Review {
        account: marked,
        reviewed_on: "2026-11-05".into(),
    }];
    let content = Arc::new(content);
    let mut presentations = store
        .create_prepared_guild(vec![marked, plain], "Review Fixture".into())
        .await
        .unwrap();
    presentations.sort_by_key(|(account, _)| *account != marked);
    let mut directory =
        Directory::test_with_seed(std::path::Path::new("data"), content.clone(), 16, 1098).unwrap();
    let (lost, _) = watch::channel(None);
    let mut ids = Vec::new();
    let mut inboxes = Vec::new();
    for (account, presentation) in presentations {
        let mut ch = store
            .create(account, content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.name = if account == marked {
            "MarkedDemo"
        } else {
            "PlainDemo"
        }
        .into();
        let (packets, mut inbox) = mpsc::channel(64);
        ids.push(
            directory
                .worlds
                .get_mut(&-2)
                .unwrap()
                .enter_at(
                    ch,
                    Some(Output {
                        trace: None,
                        packets,
                        lost: lost.clone(),
                    }),
                    None,
                    presentation,
                    false,
                )
                .unwrap(),
        );
        while inbox.try_recv().is_ok() {}
        inboxes.push(inbox);
    }
    let [mut own, mut peer]: [_; 2] = inboxes.try_into().unwrap();
    let now = Instant::now();
    for (i, (input, recipient, body)) in [
        ("hello", "", "hello"),
        ("/g guild hello", "*Guild*", "guild hello"),
        (
            "/tell PlainDemo private hello",
            "PlainDemo",
            "private hello",
        ),
        ("/yell nexus hello", "", "nexus hello"),
    ]
    .into_iter()
    .enumerate()
    {
        directory
            .chat(ids[0], input.into(), now + Duration::from_secs(i as u64))
            .unwrap();
        for inbox in [&mut own, &mut peer] {
            assert_eq!(
                texts(inbox),
                vec![(
                    "MarkedDemo".into(),
                    recipient.into(),
                    format!("[cheater] {body}")
                )]
            );
        }
    }
    directory
        .worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&ids[0])
        .unwrap()
        .character
        .name = "RenamedDemo".into();
    directory
        .chat(ids[0], "still marked".into(), now + Duration::from_secs(4))
        .unwrap();
    assert_eq!(
        texts(&mut peer),
        vec![(
            "RenamedDemo".into(),
            "".into(),
            "[cheater] still marked".into()
        )]
    );
    texts(&mut own);
    directory
        .chat(ids[1], "ordinary".into(), now + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        texts(&mut own),
        vec![("PlainDemo".into(), "".into(), "ordinary".into())]
    );
    texts(&mut peer);
    directory
        .chat(ids[0], "/who".into(), now + Duration::from_secs(6))
        .unwrap();
    let roster = texts(&mut own);
    assert_eq!(roster.len(), 1);
    assert!(roster[0].2.contains("RenamedDemo"));
    assert!(!roster[0].2.contains("[cheater]"));
    assert!(texts(&mut peer).is_empty());
}
