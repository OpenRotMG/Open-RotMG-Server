use super::*;

#[tokio::test]
async fn native_paid_name_choice_commits_gold_name_and_rejects_current_target() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world = a.world.in_world(-2);
    let owner = a
        .store
        .register(
            "rename-wire".into(),
            "rename@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    a.store.choose_name(owner, "Before".into()).await.unwrap();
    let ch = a
        .store
        .create(owner, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (tx, mut rx) = mpsc::channel(128);
    let (lost, _) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    let player = a
        .world
        .in_world(-2)
        .call(move |w| w.enter(ch, Some(output)))
        .await
        .unwrap();
    let mut session = Session {
        account: Some(a.claim_session(owner).unwrap()),
        player: Some(player),
        ..Session::default()
    };
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [owner])
        .unwrap();
    for expected in [true, false] {
        let mut out = Writer::new();
        out.utf("After");
        {
            let guard = a.gold_updates.clone().lock_owned().await;
            let command = handle(
                &mut a,
                Packet {
                    id: 33,
                    body: out.0,
                },
                &mut session,
                &tx,
                &lost,
                std::time::Instant::now(),
            );
            tokio::pin!(command);
            tokio::select! {
                _ = &mut command => panic!("rename bypassed Gold publication serialization"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
            }
            assert_eq!(
                db.query_row("SELECT name FROM accounts WHERE id=?1", [owner], |r| r
                    .get::<_, String>(
                    0
                ))
                .unwrap(),
                if expected { "Before" } else { "After" }
            );
            drop(guard);
            command.await.unwrap();
        }
        let result = loop {
            let packet = rx.recv().await.unwrap();
            if packet.id == 20 {
                break packet;
            }
        };
        let mut r = Reader::new(&result.body);
        assert_eq!(
            r.u8().unwrap(),
            u8::from(expected),
            "native paid name choice must commit before Success"
        );
        assert_eq!(r.utf().unwrap().is_empty(), expected);
        r.done().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT name,credits FROM accounts WHERE id=?1",
                [owner],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i32>(1)?))
            )
            .unwrap(),
            ("After".into(), 0)
        );
        a.world
            .for_player(player)
            .call(move |w| {
                let p = &w.players[&player];
                assert_eq!(p.character.name, "After");
                assert!(!p.pending);
                Ok(())
            })
            .await
            .unwrap();
    }
    assert!(a.claim_session(owner).is_err());
    drop(rx);
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [owner])
        .unwrap();
    let mut out = Writer::new();
    out.utf("LostAck");
    let error = handle(
        &mut a,
        Packet {
            id: 33,
            body: out.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap_err();
    assert!(error.is::<ReplyQueueError>());
    depart(&a, player).await.unwrap();
    session.account.take().unwrap().release();
    assert!(a.quarantined.lock().unwrap().is_empty());
    a.claim_session(owner).unwrap().release();
    let ch = a.store.load(owner, 1).await.unwrap();
    assert_eq!(ch.name, "LostAck");
    assert_eq!(a.store.presentation(owner).await.unwrap().credits, 0);
    assert!(a.store.choose_name(owner, "HttpPaid".into()).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
