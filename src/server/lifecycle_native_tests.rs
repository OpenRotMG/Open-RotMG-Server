use super::*;

struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Release {
    fn now(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}

async fn native_cancel(checkpoint: &str) {
    let (dir, a) = account_actions::tests::fixture();
    let account = a
        .store
        .list(format!("native-{checkpoint}"))
        .await
        .unwrap()
        .id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let initial = ch.clone();
    let presentation = a.store.presentation(account).await.unwrap();
    let id = a
        .world
        .call(move |w| w.enter_at(ch, None, None, presentation))
        .await
        .unwrap();
    let owner = a.claim_session(account).unwrap();
    let (tx, _rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    let (ready, release, blocker) = match checkpoint {
        "after_enqueue" => {
            let (entered, ready) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let store = a.store.clone();
            let blocker = tokio::spawn(async move {
                store
                    .lifecycle_test_job(move |_| {
                        entered
                            .send(())
                            .map_err(|_| anyhow::anyhow!("Store barrier waiter disappeared"))?;
                        wait.recv().context("Store barrier abandoned")?;
                        Ok(())
                    })
                    .await
            });
            ready.await.unwrap();
            (None, release, Some(blocker))
        }
        "after_commit" | "panic_after_commit" => {
            let (ready, release) = a.store.lifecycle_test_pause_reply();
            (Some(ready), release, None)
        }
        "before_reservation" => {
            let (ready, release) = a.world.lifecycle_test_pause_job(2, true);
            (Some(ready), release, None)
        }
        "after_reservation" => {
            let (ready, release) = a.world.lifecycle_test_pause_job(2, false);
            (Some(ready), release, None)
        }
        "before_publication" => {
            let (ready, release) = a.world.lifecycle_test_pause_job(4, true);
            (Some(ready), release, None)
        }
        "before_release" => {
            let (ready, release) = a.world.lifecycle_test_pause_job(4, false);
            (Some(ready), release, None)
        }
        _ => panic!("unknown lifecycle checkpoint"),
    };
    let mut release = Release(Some(release));
    let mut body = Writer::new();
    body.i32(0).f32(0.0).f32(0.0);
    for slot in [4, 5] {
        body.i32(id)
            .u8(slot)
            .i16(initial.items[slot as usize] as i16);
    }
    let mut app = a.clone();
    let (panic_sender, panic_receiver) = tokio::sync::oneshot::channel::<()>();
    let caller = tokio::spawn(async move {
        tokio::select! {
        result = handle(
            &mut app,
            Packet {
                id: 65,
                body: body.0,
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        => result,
        signal = panic_receiver => { signal.expect("caller panic signal lost"); panic!("lifecycle injected native caller panic"); }
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        if let Some(ready) = ready {
            ready
                .await
                .expect("World/Store checkpoint sender disappeared");
        } else {
            while a.store.lifecycle_test_jobs() != 2 {
                assert!(
                    !caller.is_finished(),
                    "native command rejected before Store enqueue"
                );
                tokio::task::yield_now().await;
            }
        }
    })
    .await
    .expect("required native checkpoint was not reached");
    if matches!(
        checkpoint,
        "after_commit" | "panic_after_commit" | "before_publication" | "before_release"
    ) {
        let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT revision FROM characters WHERE id=?1",
                [initial.id],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            initial.revision + 1,
            "checkpoint must follow a real SQL commit"
        );
    }
    if checkpoint == "panic_after_commit" {
        panic_sender.send(()).unwrap();
        assert!(caller.await.unwrap_err().is_panic());
    } else {
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
    }
    let admission_blocked = a.owners.lock().unwrap().contains_key(&account);
    release.now();
    if let Some(blocker) = blocker {
        blocker.await.unwrap().unwrap();
    }
    assert!(
        admission_blocked,
        "{checkpoint}: cancelled native waiter released the loaded account before acknowledged retirement"
    );
    let mut expected = initial.items.clone();
    expected.swap(4, 5);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if a.world
                .for_player(id)
                .call(
                    move |w| Ok(!w.players[&id].pending && w.players[&id].character.revision == 1),
                )
                .await
                .unwrap()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("accepted native write was not published");
    let saved = a.store.load(account, initial.id).await.unwrap();
    assert_eq!(
        (saved.revision, saved.items),
        (initial.revision + 1, expected)
    );
    assert!(
        a.owners.lock().unwrap().contains_key(&account),
        "publication is not avatar retirement"
    );
    depart(&a, id).await.unwrap();
    assert!(a.owners.lock().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_cancel_before_reservation() {
    native_cancel("before_reservation").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_cancel_after_reservation() {
    native_cancel("after_reservation").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_cancel_after_enqueue() {
    native_cancel("after_enqueue").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_cancel_after_commit() {
    native_cancel("after_commit").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_cancel_before_publication() {
    native_cancel("before_publication").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_cancel_before_release() {
    native_cancel("before_release").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_http_cancel_after_accepted_delete_keeps_short_claim() {
    let (dir, a) = account_actions::tests::fixture();
    let account = a
        .store
        .register(
            "http-cancel-red".into(),
            "http-cancel@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let mut release = Release(Some(release));
    a.store
        .lifecycle_test_job(move |db| {
            let mut entered = Some(entered);
            db.update_hook(Some(
                move |_: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
                    if table == "characters" {
                        entered
                            .take()
                            .expect("delete must mutate exactly once")
                            .send(())
                            .expect("HTTP test waiter disappeared");
                        wait.recv()
                            .expect("HTTP gate must be released by its owner");
                    }
                },
            ));
            Ok(())
        })
        .await
        .unwrap();
    let app = a.clone();
    let caller = tokio::spawn(async move {
        delete_character(
            State(app),
            ConnectInfo("127.0.0.1:1234".parse().unwrap()),
            HeaderMap::new(),
            Form(HashMap::from([
                ("guid".into(), "http-cancel@example.test".into()),
                ("password".into(), "test-password".into()),
                ("charId".into(), ch.id.to_string()),
            ])),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let blocked = a.owners.lock().unwrap().contains_key(&account);
    release.now();
    a.store.presentation(account).await.unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT alive,revision FROM characters WHERE id=?1",
            [ch.id],
            |r| Ok((r.get::<_, bool>(0)?, r.get::<_, i32>(1)?))
        )
        .unwrap(),
        (false, ch.revision + 1)
    );
    assert!(
        blocked,
        "cancelled HTTP delete released its short account claim while the accepted SQL mutation was still executing"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while a.owners.lock().unwrap().contains_key(&account) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("successful HTTP completion leaked a short claim");
    std::fs::remove_dir_all(dir).unwrap();
}

pub(super) fn native_swap_packet(id: i32, ch: &crate::store::Character) -> Packet {
    let mut body = Writer::new();
    body.i32(0).f32(0.0).f32(0.0);
    for slot in [4, 5] {
        body.i32(id).u8(slot).i16(ch.items[slot as usize] as i16);
    }
    Packet {
        id: 65,
        body: body.0,
    }
}
async fn native_fault(fault: &str) {
    let (dir, a) = account_actions::tests::fixture();
    let mut sessions = Vec::new();
    for name in [format!("fault-{fault}"), format!("healthy-{fault}")] {
        let account = a.store.list(name).await.unwrap().id;
        let ch = a
            .store
            .create(account, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let saved = ch.clone();
        let presentation = a.store.presentation(account).await.unwrap();
        let id = a
            .world
            .call(move |w| w.enter_at(ch, None, None, presentation))
            .await
            .unwrap();
        sessions.push((
            Session {
                account: Some(a.claim_session(account).unwrap()),
                player: Some(id),
                ..Default::default()
            },
            id,
            saved,
        ));
    }
    let (mut healthy_session, healthy, healthy_ch) = sessions.pop().unwrap();
    let (mut failed_session, failed, failed_ch) = sessions.pop().unwrap();
    if fault == "sql" {
        rusqlite::Connection::open(dir.join("game.db")).unwrap().execute_batch(&format!("CREATE TRIGGER lifecycle_native_failure BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'lifecycle native SQL failure'); END;",failed_ch.account)).unwrap();
    }
    if fault == "store_reply" {
        a.store.lifecycle_test_lose_reply();
    }
    let gate = if fault == "world_reply" {
        Some(a.world.lifecycle_test_pause_job(2, true))
    } else {
        None
    };
    let packet = native_swap_packet(failed, &failed_ch);
    let app = a.clone();
    let caller = tokio::spawn(async move {
        let (tx, _rx) = mpsc::channel(64);
        let (lost, _) = tokio::sync::watch::channel(false);
        let mut app = app;
        handle(
            &mut app,
            packet,
            &mut failed_session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await
    });
    if let Some((ready, release)) = gate {
        let mut release = Release(Some(release));
        tokio::time::timeout(std::time::Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        a.world.lifecycle_test_lose_reply();
        release.now();
    }
    let error = caller.await.unwrap().unwrap_err();
    assert!(
        format!("{error:#}").contains(if fault == "sql" {
            "lifecycle native SQL failure"
        } else {
            "worker failed"
        }),
        "{fault}: {error:#}"
    );
    let retained = a.owners.lock().unwrap().contains_key(&failed_ch.account);
    let (tx, _rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let mut app = a.clone();
    handle(
        &mut app,
        native_swap_packet(healthy, &healthy_ch),
        &mut healthy_session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    for (ch, committed) in [(&healthy_ch, true), (&failed_ch, fault == "store_reply")] {
        assert_eq!(
            db.query_row(
                "SELECT revision FROM characters WHERE id=?1",
                [ch.id],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            ch.revision + i32::from(committed),
            "{fault}: exact once-only durable revision"
        );
    }
    assert_eq!(
        a.world
            .for_player(healthy)
            .call(move |w| Ok(w.players[&healthy].character.revision))
            .await
            .unwrap(),
        healthy_ch.revision + 1
    );
    depart(&a, healthy).await.unwrap();
    healthy_session.account.take().unwrap().release();
    assert!(
        retained,
        "{fault}: failed native completion released its uncertain account while a healthy sibling persisted"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_sql_failure_retains_uncertain_owner_and_healthy_sibling() {
    native_fault("sql").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_lost_store_reply_retains_uncertain_owner_and_healthy_sibling() {
    native_fault("store_reply").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_lost_world_reply_retains_uncertain_owner_and_healthy_sibling() {
    native_fault("world_reply").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_owned_native_caller_panic_after_commit() {
    native_cancel("panic_after_commit").await;
}

#[tokio::test]
async fn lifecycle_owned_native_preserves_request_world_calls_and_completions() {
    let (dir, mut a) = account_actions::tests::fixture();
    let account = a.store.list("owned-request".into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let initial = ch.clone();
    let owner = a.claim_session(account).unwrap();
    let id = a.world.call(move |w| w.enter(ch, None)).await.unwrap();
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    let (tx, _rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let requests = Arc::new(Mutex::new(crate::performance::Requests::default()));
    crate::performance::REQUEST
        .scope(
            requests.clone(),
            handle(
                &mut a,
                native_swap_packet(id, &initial),
                &mut session,
                &tx,
                &lost,
                std::time::Instant::now(),
            ),
        )
        .await
        .unwrap();
    let stats = requests.lock().unwrap().clone();
    // Native transfer check, swap preparation, unlock staging, and inventory publication.
    assert_eq!((stats.calls, stats.completed), (4, 4));
    assert!(stats.work > std::time::Duration::ZERO);
    assert_eq!(
        a.store.load(account, initial.id).await.unwrap().revision,
        initial.revision + 1
    );
    depart(&a, id).await.unwrap();
    drop(session);
    std::fs::remove_dir_all(dir).unwrap();
}
