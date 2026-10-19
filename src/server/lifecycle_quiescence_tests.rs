use super::*;

async fn motor(
    a: &App,
    name: &str,
) -> (Owner, i32, crate::store::Character, mpsc::Receiver<Packet>) {
    let (owner, id, ch) = avatar(a, name).await;
    let (packets, rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    a.world
        .for_player(id)
        .call(move |w| {
            let p = w.players.get_mut(&id).unwrap();
            p.controller = crate::world::Controller::Autonomous;
            p.bot_goal = Some(Pos {
                x: p.pos.x + 0.25,
                y: p.pos.y,
            });
            p.bot_aim = Some(0.);
            p.tx = Some(crate::world::Output { packets, lost });
            Ok(())
        })
        .await
        .unwrap();
    (owner, id, ch, rx)
}
async fn stopped(a: &App, id: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if a.world
                .for_player(id)
                .call(move |w| Ok(w.players[&id].departing))
                .await
                .unwrap()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.departing && p.bot_goal.is_none() && p.bot_aim.is_none() && p.tx.is_none());
            let pos = p.pos;
            for _ in 0..5 {
                w.step();
            }
            assert_eq!((w.players[&id].pos.x, w.players[&id].pos.y), (pos.x, pos.y));
            Ok(())
        })
        .await
        .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_fenced_close_cancelled_waiter_stops_autonomous_controller_once() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (owner, id, ch, _rx) = motor(&a, "fenced-motor").await;
    owner.failed("original autonomous controller invariant".into());
    let original = lifecycle::failure(&a, ch.account).unwrap();
    let (ready, release) = a.world.lifecycle_test_pause_job(1, true);
    let first = owner.close(&a, CloseReason::Internal).unwrap().unwrap();
    let caller = tokio::spawn(first.wait());
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let second = lifecycle::finalize(&a, id, CloseReason::Shutdown).unwrap();
    let third = lifecycle::finalize(&a, id, CloseReason::Controller).unwrap();
    caller.abort();
    assert!(
        caller
            .await
            .expect_err("cancelled caller unexpectedly returned")
            .is_cancelled()
    );
    assert!(a.claim(ch.account).is_err());
    release.send(()).unwrap();
    let error = second.wait().await.unwrap_err().to_string();
    assert_eq!(third.wait().await.unwrap_err().to_string(), error);
    assert!(error.starts_with(&original));
    assert!(error.contains("controller_stop=acknowledged"));
    stopped(&a, id).await;
    assert_eq!(lifecycle::failure(&a, ch.account).unwrap(), original);
    assert!(a.claim(ch.account).is_err());
    let view = a.world.directory(move |d| d.player_view(id)).await.unwrap();
    assert!(!view.lifecycle.quarantined);
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    let (peer_owner, peer, peer_ch) = avatar(&a, "fenced-motor-peer").await;
    depart(&a, peer).await.unwrap();
    peer_owner.release();
    assert_eq!(
        a.store
            .load(peer_ch.account, peer_ch.id)
            .await
            .unwrap()
            .items,
        peer_ch.items
    );
    std::fs::remove_dir_all(dir).unwrap();
}
async fn pending_commit(stall: bool) {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (owner, id, ch, _rx) = motor(&a, "fenced-pending").await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TABLE fenced_audit(n INTEGER); CREATE TRIGGER fenced_updates AFTER UPDATE ON characters BEGIN INSERT INTO fenced_audit VALUES(1); END;").unwrap();
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let app = a.clone();
    let original_ch = ch.clone();
    let caller = tokio::spawn(async move {
        inventory(&app, id, move |w| {
            w.swap(
                id,
                Slot {
                    object: id,
                    slot: 4,
                    kind: original_ch.items[4],
                },
                Slot {
                    object: id,
                    slot: 5,
                    kind: original_ch.items[5],
                },
            )
            .map(Some)
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM characters WHERE id=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        ch.revision + 1
    );
    owner.failed("original unknown terminal controller error".into());
    let original = lifecycle::failure(&a, ch.account).unwrap();
    let close = owner.close(&a, CloseReason::Internal).unwrap().unwrap();
    caller.abort();
    assert!(
        caller
            .await
            .expect_err("cancelled caller unexpectedly returned")
            .is_cancelled()
    );
    stopped(&a, id).await;
    assert_eq!(
        lifecycle::state(&a, ch.account),
        Some((true, Some(id), true))
    );
    a.world
        .for_player(id)
        .call(move |w| {
            assert!(w.players[&id].pending);
            assert_eq!(w.players[&id].character.revision, original_ch.revision);
            Ok(())
        })
        .await
        .unwrap();
    let error = if stall {
        let error = close.wait().await.unwrap_err().to_string();
        assert!(
            error.contains("controller_stop=acknowledged") && error.contains("pending_work=true")
        );
        assert!(error.contains("deadline exceeded"));
        release.send(()).unwrap();
        error
    } else {
        release.send(()).unwrap();
        close.wait().await.unwrap_err().to_string()
    };
    assert!(error.starts_with(&original));
    assert_eq!(lifecycle::failure(&a, ch.account).unwrap(), original);
    tokio::time::timeout(Duration::from_secs(5), async {
        while lifecycle::state(&a, ch.account).unwrap().0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut expected = ch.items.clone();
    expected.swap(4, 5);
    let saved = a.store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(
        (saved.revision, saved.items.clone()),
        (ch.revision + 1, expected.clone())
    );
    a.world
        .for_player(id)
        .call(move |w| {
            assert_eq!(w.players[&id].character.items, expected);
            assert!(!w.players[&id].pending && w.players[&id].departing);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM fenced_audit", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    assert!(a.claim(ch.account).is_err());
    let view = a.world.directory(move |d| d.player_view(id)).await.unwrap();
    assert!(!view.lifecycle.quarantined);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_fenced_stop_preserves_accepted_commit_publication() {
    pending_commit(false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_fenced_stalled_store_reports_stop_ack_and_unresolved_work() {
    pending_commit(true).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_fenced_queued_preparation_rejects_without_clearing_original_cause() {
    let (dir, a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let (owner, id, ch, _rx) = motor(&a, "fenced-preparation").await;
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = tokio::sync::oneshot::channel();
    let app = a.clone();
    let initial = ch.clone();
    let caller = tokio::spawn(async move {
        lifecycle::player(&app, id, "queued_before_fence", move |app| async move {
            lifecycle::stage(Stage::Reservation);
            entered.send(()).unwrap();
            wait.await.unwrap();
            app.world
                .for_player(id)
                .call(move |w| {
                    w.swap(
                        id,
                        Slot {
                            object: id,
                            slot: 4,
                            kind: initial.items[4],
                        },
                        Slot {
                            object: id,
                            slot: 5,
                            kind: initial.items[5],
                        },
                    )
                })
                .await
        })
        .await
    });
    ready.await.unwrap();
    owner.failed("queued controller invariant".into());
    let original = lifecycle::failure(&a, ch.account).unwrap();
    let close = owner.close(&a, CloseReason::Internal).unwrap().unwrap();
    stopped(&a, id).await;
    release.send(()).unwrap();
    let rejected = caller
        .await
        .unwrap()
        .err()
        .expect("queued fenced preparation unexpectedly succeeded");
    assert!(lifecycle::rule_rejection(&rejected));
    assert!(
        close
            .wait()
            .await
            .unwrap_err()
            .to_string()
            .starts_with(&original)
    );
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().items,
        ch.items
    );
    assert_eq!(
        a.store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    assert_eq!(lifecycle::failure(&a, ch.account).unwrap(), original);
    assert!(a.claim(ch.account).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_owned_entry_custody_survives_cancelled_waiter_and_publication() {
    for checkpoint in ["commit", "queued_entry", "bound_entry", "lost_entry"] {
        let (dir, a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let account = a
            .store
            .list(format!("owned-entry-{checkpoint}"))
            .await
            .unwrap()
            .id;
        let mut owner = a.claim_session(account).unwrap();
        owner.arm_cleanup(&a, tokio::runtime::Handle::current());
        let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        db.execute_batch("CREATE TABLE entry_audit(n INTEGER); CREATE TRIGGER entry_updates AFTER UPDATE ON characters BEGIN INSERT INTO entry_audit VALUES(1); END;").unwrap();
        let gate = if checkpoint == "commit" {
            a.store.lifecycle_test_pause_reply()
        } else {
            a.world
                .lifecycle_test_pause_job(1, checkpoint != "bound_entry")
        };
        let (packets, _rx) = mpsc::channel(4096);
        let (lost, _) = tokio::sync::watch::channel(false);
        let app = a.clone();
        let caller = tokio::spawn(async move {
            lifecycle::account(
                &app,
                vec![account],
                "owned_entry_custody",
                move |app| async move {
                    let outcome: Result<i32> = async {
                        lifecycle::stage(Stage::Store);
                        let ch = app.store.create(account, app.content.get(WIZARD)?).await?;
                        let bind = lifecycle::binding(&app, account)?;
                        lifecycle::stage(Stage::Publication);
                        app.world
                            .call(move |w| {
                                let id =
                                    w.enter(ch, Some(crate::world::Output { packets, lost }))?;
                                bind(id)?;
                                let p = w.players.get_mut(&id).unwrap();
                                p.controller = crate::world::Controller::Autonomous;
                                p.bot_goal = Some(p.pos);
                                p.bot_aim = Some(0.);
                                Ok(id)
                            })
                            .await
                    }
                    .await;
                    if let Err(error) = &outcome
                        && !lifecycle::rule_rejection(error)
                    {
                        owner.failed(format!("{error:#}"));
                    }
                    Ok((owner, outcome))
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), gate.0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM characters WHERE account=?1 AND alive=1",
                [account],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            1
        );
        caller.abort();
        assert!(
            caller
                .await
                .err()
                .expect("cancelled caller unexpectedly returned")
                .is_cancelled()
        );
        assert!(a.claim(account).is_err());
        if checkpoint == "lost_entry" {
            a.world.lifecycle_test_lose_reply();
        }
        gate.1.send(()).unwrap();
        if checkpoint == "lost_entry" {
            tokio::time::timeout(Duration::from_secs(5), async {
                while lifecycle::failure(&a, account).is_none() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let id = lifecycle::state(&a, account).unwrap().1.unwrap();
            stopped(&a, id).await;
            assert!(a.claim(account).is_err());
            assert_eq!(
                db.query_row("SELECT count(*) FROM entry_audit", [], |r| r
                    .get::<_, i32>(0))
                    .unwrap(),
                0
            );
        } else {
            released(&a, account).await;
            assert_eq!(
                a.world
                    .directory(move |d| Ok(d
                        .worlds
                        .values()
                        .flat_map(|w| w.players.values())
                        .filter(|p| p.character.account == account)
                        .count()))
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM entry_audit", [], |r| r
                    .get::<_, i32>(0))
                    .unwrap(),
                1
            );
        }
        let id = db
            .query_row(
                "SELECT id FROM characters WHERE account=?1",
                [account],
                |r| r.get::<_, i32>(0),
            )
            .unwrap();
        let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
        assert_eq!(reopened.load(account, id).await.unwrap().revision, 0);
        let (peer_owner, peer, peer_ch) = avatar(&a, "owned-entry-healthy").await;
        depart(&a, peer).await.unwrap();
        peer_owner.release();
        assert_eq!(
            a.store
                .load(peer_ch.account, peer_ch.id)
                .await
                .unwrap()
                .items,
            peer_ch.items
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_close_rejects_old_success_while_new_entry_is_unpublished() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = avatar(&a, "old-success-close").await;
    depart(&a, id).await.unwrap();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = tokio::sync::oneshot::channel();
    let app = a.clone();
    let accepted = tokio::spawn(async move {
        lifecycle::account(
            &app,
            vec![ch.account],
            "unpublished_entry_precondition",
            move |_| async move {
                entered.send(()).unwrap();
                wait.await.unwrap();
                Ok(())
            },
        )
        .await
    });
    ready.await.unwrap();
    let error = owner
        .close(&a, CloseReason::Controller)
        .err()
        .expect("old retirement result incorrectly acknowledged a pending entry");
    assert!(error.to_string().contains("unpublished accepted entry"));
    assert!(a.claim(ch.account).is_err());
    release.send(()).unwrap();
    accepted.await.unwrap().unwrap();
    assert!(a.claim(ch.account).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_unknown_native_create_class_is_protocol_fault_not_storage_fence() {
    let (dir, a) = account_actions::tests::fixture();
    let guid = "bad-create-class";
    let account = a.store.list(guid.into()).await.unwrap().id;
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket.send(socket_hello(&dir, guid, -2, None)).await;
    socket.receive(wire::MAP_INFO).await;
    let mut body = Writer::new();
    body.i16(i16::MAX);
    socket
        .send(Packet {
            id: wire::CREATE,
            body: body.0,
        })
        .await;
    socket.receive(0).await;
    released(&a, account).await;
    assert_eq!(
        rusqlite::Connection::open(dir.join("game.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM characters WHERE account=?1",
                [account],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
        0
    );
    drop(socket);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_failed_unbound_entry_cannot_join_old_success() {
    for lost_reply in [true, false] {
        let (dir, a) = account_actions::tests::fixture();
        a.world.clock.set_paused(true);
        let (owner, old_id, old) = avatar(&a, "failed-unbound-entry").await;
        depart(&a, old_id).await.unwrap();
        a.store.delete_character(old.account, old.id).await.unwrap();
        let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        db.execute_batch("CREATE TABLE entry_audit(n INTEGER); CREATE TRIGGER entry_insert AFTER INSERT ON characters BEGIN INSERT INTO entry_audit VALUES(1); END;").unwrap();
        if !lost_reply {
            db.execute_batch(&format!("CREATE TRIGGER entry_failure BEFORE INSERT ON characters WHEN NEW.account={} BEGIN SELECT RAISE(FAIL,'injected entry SQL fault'); END;",old.account)).unwrap();
        }
        let wallet: (i32, i32) = db
            .query_row(
                "SELECT credits,character_slots FROM accounts WHERE id=?1",
                [old.account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let (ready, release) = a.store.lifecycle_test_pause_reply();
        if lost_reply {
            a.store.lifecycle_test_lose_reply();
        }
        let app = a.clone();
        let accepted = tokio::spawn(async move {
            lifecycle::account(
                &app,
                vec![old.account],
                "failed_unbound_entry",
                move |app| async move {
                    lifecycle::stage(Stage::Store);
                    let outcome = app
                        .store
                        .create(old.account, app.content.get(WIZARD)?)
                        .await;
                    let error = outcome.expect_err("injected entry failure unexpectedly succeeded");
                    assert!(!lifecycle::rule_rejection(&error));
                    if !lost_reply {
                        assert!(
                            error.is::<rusqlite::Error>(),
                            "entry fault must originate in SQLite"
                        );
                    }
                    owner.failed(format!("{error:#}"));
                    Ok(owner)
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        assert!(a.claim(old.account).is_err());
        assert_eq!(
            db.query_row("SELECT count(*) FROM entry_audit", [], |r| r
                .get::<_, i32>(0))
                .unwrap(),
            i32::from(lost_reply)
        );
        release.send(()).unwrap();
        let owner = accepted.await.unwrap().unwrap();
        assert_eq!(lifecycle::state(&a, old.account), Some((false, None, true)));
        let original = lifecycle::failure(&a, old.account).unwrap();
        let rows = |db: &rusqlite::Connection| {
            db.prepare(
                "SELECT id,alive,revision,items,stats FROM characters WHERE account=?1 ORDER BY id",
            )
            .unwrap()
            .query_map([old.account], |r| {
                Ok((
                    r.get::<_, i32>(0)?,
                    r.get::<_, i32>(1)?,
                    r.get::<_, i32>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
        };
        let committed = rows(&db);
        let closed = owner.close(&a, CloseReason::Internal);
        let close_error = closed
            .err()
            .expect("failed unbound entry returned the old successful retirement");
        assert!(close_error.to_string().contains(&original));
        assert_eq!(lifecycle::failure(&a, old.account).unwrap(), original);
        assert_eq!(lifecycle::state(&a, old.account), Some((false, None, true)));
        assert!(a.claim(old.account).is_err());
        assert_eq!(rows(&db), committed);
        assert_eq!(
            db.query_row(
                "SELECT credits,character_slots FROM accounts WHERE id=?1",
                [old.account],
                |r| Ok((r.get::<_, i32>(0)?, r.get::<_, i32>(1)?))
            )
            .unwrap(),
            wallet
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM entry_audit", [], |r| r
                .get::<_, i32>(0))
                .unwrap(),
            i32::from(lost_reply)
        );
        assert_eq!(
            a.world
                .directory(move |d| Ok(d
                    .worlds
                    .values()
                    .flat_map(|w| w.players.values())
                    .filter(|p| p.character.account == old.account)
                    .count()))
                .await
                .unwrap(),
            0
        );
        let reopened = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        assert_eq!(rows(&reopened), committed);
        let (peer_owner, peer, peer_ch) = avatar(&a, "failed-entry-healthy").await;
        depart(&a, peer).await.unwrap();
        peer_owner
            .close(&a, CloseReason::Controller)
            .unwrap()
            .unwrap()
            .wait()
            .await
            .unwrap();
        a.claim(peer_ch.account).unwrap().release();
        assert_eq!(
            a.store
                .load(peer_ch.account, peer_ch.id)
                .await
                .unwrap()
                .items,
            peer_ch.items
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
