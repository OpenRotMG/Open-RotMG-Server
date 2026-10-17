use super::*;

#[tokio::test]
async fn buy_packet_delivers_an_item_and_rejections_leave_the_session_usable() {
    let (dir, mut a) = account_actions::tests::fixture();
    let account = a
        .store
        .register(
            "packet-buyer".into(),
            "packet@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let presentation = a.store.presentation(account).await.unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    a.world = a.world.in_world(-2);
    let (id, merchant) = a
        .world
        .call(move |w| {
            let e = w
                .entities
                .values()
                .find(|e| e.kind == 0x1ca && e.extra_stats.contains(&(34, 0x705)))
                .unwrap();
            let merchant = e.id;
            let pos = Pos {
                x: e.pos.x,
                y: e.pos.y + 0.5,
            };
            let id = w.enter_at(ch, Some(output), Some(pos), presentation)?;
            w.players.get_mut(&id).unwrap().known.insert(merchant);
            Ok((id, merchant))
        })
        .await
        .unwrap();
    let owner = a.claim(account).unwrap();
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    for registered in [false, true] {
        session.account.as_mut().unwrap().registered = registered;
        let mut body = Writer::new();
        body.i32(if registered { -1 } else { merchant });
        handle(
            &mut a,
            Packet {
                id: 50,
                body: body.0,
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await
        .unwrap();
        let packet = std::iter::from_fn(|| rx.try_recv().ok())
            .find(|p| p.id == 27)
            .unwrap();
        assert_eq!(Reader::new(&packet.body).i32().unwrap(), 1);
        assert_eq!(a.store.presentation(account).await.unwrap().credits, 100);
    }
    let mut body = Writer::new();
    body.i32(merchant);
    handle(
        &mut a,
        Packet {
            id: 50,
            body: body.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let packet = std::iter::from_fn(|| rx.try_recv().ok())
        .find(|p| p.id == 27)
        .unwrap();
    let mut r = Reader::new(&packet.body);
    assert_eq!(r.i32().unwrap(), 0);
    assert_eq!(r.utf().unwrap(), "Item purchased!");
    r.done().unwrap();
    assert_eq!(a.store.presentation(account).await.unwrap().credits, 75);
    assert!(
        a.store
            .load(account, character)
            .await
            .unwrap()
            .items
            .contains(&0x705)
    );
    handle(
        &mut a,
        Packet {
            id: wire::UPDATE_ACK,
            body: vec![],
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    assert_eq!(session.player, Some(id));
    assert!(!*lost.borrow());
    assert!(
        handle(
            &mut a,
            Packet {
                id: 50,
                body: vec![0; 3]
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now()
        )
        .await
        .is_err()
    );
    assert!(
        handle(
            &mut a,
            Packet {
                id: 50,
                body: vec![0; 5]
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now()
        )
        .await
        .is_err()
    );
    session.account.take().unwrap().release();
    std::fs::remove_dir_all(dir).unwrap();
}
