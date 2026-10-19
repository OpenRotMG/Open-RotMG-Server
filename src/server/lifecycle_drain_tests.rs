use super::*;
use crate::server::{
    self,
    lifecycle_finalize_tests::{NativeSocket, socket_hello, socket_load},
};
use serde_json::{Value, json};

async fn frontdoor(a: &App) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    println!(
        "owned lifecycle_drain HTTP pid={} address={address}",
        std::process::id()
    );
    let router = server::routes(a, PathBuf::from("../client"));
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}"), server)
}
fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}
fn auth() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    headers
}
async fn http(base: &str, op: &str, body: Value) -> reqwest::Response {
    client()
        .post(format!("{base}/internal/lifecycle/{op}"))
        .headers(auth())
        .json(&body)
        .send()
        .await
        .unwrap()
}
async fn prepared(a: &App) -> DrainStatus {
    let first = prepare(a);
    tokio::time::timeout(DEADLINE + Duration::from_secs(1), wait(a, &first.receipt))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_authenticated_routes_reject_forwarding_origin_nonloopback_and_stale_receipts()
 {
    let (dir, a) = server::lifecycle_test_fixture();
    let (base, task) = frontdoor(&a).await;
    for op in ["prepare", "status", "cancel"] {
        let mut cases = vec![HeaderMap::new()];
        for key in [
            "origin",
            "forwarded",
            "x-forwarded-for",
            "x-real-ip",
            "x-forwarded-host",
            "x-forwarded-proto",
        ] {
            let mut headers = auth();
            headers.insert(key, "untrusted".parse().unwrap());
            cases.push(headers);
        }
        for headers in cases {
            let response = client()
                .post(format!("{base}/internal/lifecycle/{op}"))
                .headers(headers)
                .json(&json!({"instance":"wrong","generation":1}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "op={op}");
        }
        let response = if op == "prepare" {
            prepare_request(
                State(a.clone()),
                ConnectInfo("192.0.2.1:1".parse().unwrap()),
                auth(),
            )
            .await
        } else {
            receipt_request(
                &a,
                "192.0.2.1:1".parse().unwrap(),
                &auth(),
                &Receipt {
                    instance: "wrong".into(),
                    generation: 1,
                },
                op == "cancel",
            )
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let first: Value = http(&base, "prepare", json!({}))
        .await
        .json()
        .await
        .unwrap();
    let duplicate: Value = http(&base, "prepare", json!({}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(first["receipt"], duplicate["receipt"]);
    let receipt: Receipt = serde_json::from_value(first["receipt"].clone()).unwrap();
    assert_eq!(wait(&a, &receipt).await.unwrap().state, "ready");
    assert!(
        !*a.shutdown.borrow(),
        "preparation must not use the irreversible shutdown watch"
    );
    assert_eq!(
        http(
            &base,
            "status",
            json!({"instance":"old-process","generation":receipt.generation})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        http(
            &base,
            "cancel",
            json!({"instance":receipt.instance,"generation":receipt.generation+1})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        http(&base, "cancel", json!(receipt)).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        http(&base, "status", json!(receipt)).await.status(),
        StatusCode::CONFLICT
    );
    let second = prepare(&a);
    assert_eq!(second.receipt.generation, receipt.generation + 1);
    assert_eq!(
        http(&base, "cancel", json!(receipt)).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(wait(&a, &second.receipt).await.unwrap().state, "ready");
    cancel(&a, &second.receipt).unwrap();
    let (other_dir, other) = server::lifecycle_test_fixture();
    let other_status = prepared(&other).await;
    assert_ne!(other_status.receipt.instance, receipt.instance);
    assert!(status(&other, &receipt).is_err());
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(other_dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_cancelled_http_guest_write_remains_owned_and_freezes_all_http_entry_categories()
 {
    let (dir, mut a) = server::lifecycle_test_fixture();
    a.control = Some("control".into());
    let (base, task) = frontdoor(&a).await;
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let url = format!("{base}/legacy-api/char/list");
    let caller = tokio::spawn(async move {
        client()
            .post(url)
            .form(&[("guid", "drain-guest"), ("password", "")])
            .send()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let p = prepare(&a);
    assert_eq!(prepare(&a).receipt, p.receipt);
    assert_eq!(status(&a, &p.receipt).unwrap().state, "preparing");
    assert!(cancel(&a, &p.receipt).is_err());
    assert!(a.claim(999999).is_err());
    assert!(entry(&a).is_err());
    for path in [
        "/legacy-api/char/list",
        "/legacy-api/char/delete",
        "/legacy-api/account/register",
        "/legacy-api/account/changePassword",
        "/legacy-api/guild/create",
        "/api/account/recover",
        "/internal/discord/recover",
        "/dev/testing",
        "/dev/join",
        "/dev/action",
        "/dev/script",
    ] {
        let response = client()
            .post(format!("{base}{path}"))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
    }
    let response = client().get(format!("{base}/game")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        client()
            .get(format!("{base}/health"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        status(&a, &p.receipt).unwrap().state,
        "preparing",
        "readiness preceded accepted Store reply"
    );
    release.send(()).unwrap();
    assert_eq!(wait(&a, &p.receipt).await.unwrap().state, "ready");
    cancel(&a, &p.receipt).unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM accounts WHERE guid='drain-guest'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert!(a.owners.lock().unwrap().is_empty());
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_native_accepted_inventory_commit_waits_for_publication_and_lease_retirement()
 {
    let (dir, a) = server::lifecycle_test_fixture();
    a.world.clock.set_paused(true);
    let ch = a.store.list("drain-native".into()).await.unwrap();
    let ch = a
        .store
        .create(ch.id, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (base, task) = frontdoor(&a).await;
    let port = base.rsplit(':').next().unwrap().parse().unwrap();
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    let id = socket_load(&mut socket, &dir, "drain-native", ch.id).await;
    lifecycle::account(&a, vec![ch.account], "load_settled", |_| async { Ok(()) })
        .await
        .unwrap();
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    socket
        .send(server::lifecycle_native_tests::native_swap_packet(id, &ch))
        .await;
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM characters WHERE id=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        ch.revision + 1
    );
    let p = prepare(&a);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(status(&a, &p.receipt).unwrap().state, "preparing");
    assert!(a.claim(ch.account).is_err());
    release.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), wait(&a, &p.receipt))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.state, "ready", "{:?}", result.errors);
    assert!(a.owners.lock().unwrap().is_empty());
    assert!(
        !a.world
            .directory(move |d| Ok(d.worlds.values().any(|w| w.players.contains_key(&id))))
            .await
            .unwrap()
    );
    assert!(!*a.shutdown.borrow());
    cancel(&a, &p.receipt).unwrap();
    let saved = a.store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.items[4], ch.items[5]);
    assert_eq!(saved.items[5], ch.items[4]);
    assert_eq!(saved.revision, ch.revision + 1);
    let mut reconnect = NativeSocket::connect(port, &a.origin).await;
    assert_ne!(
        socket_load(&mut reconnect, &dir, "drain-native", ch.id).await,
        id
    );
    drop(reconnect);
    drop(socket);
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_inflight_hello_is_rejected_after_auth_freeze_and_create_load_transfer_cannot_enter()
 {
    let (dir, a) = server::lifecycle_test_fixture();
    a.world.clock.set_paused(true);
    let (base, task) = frontdoor(&a).await;
    let port = base.rsplit(':').next().unwrap().parse().unwrap();
    a.store.list("drain-hello".into()).await.unwrap();
    let mut peer = NativeSocket::connect(port, &a.origin).await;
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    peer.send(socket_hello(&dir, "drain-hello", -2, None)).await;
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let p = prepare(&a);
    assert_eq!(status(&a, &p.receipt).unwrap().state, "preparing");
    release.send(()).unwrap();
    assert_eq!(wait(&a, &p.receipt).await.unwrap().state, "ready");
    for id in [wire::HELLO, wire::CREATE, wire::LOAD] {
        let mut session = server::Session::default();
        let (tx, _) = mpsc::channel(64);
        let (lost, _) = tokio::sync::watch::channel(false);
        let result = server::handle(
            &mut a.clone(),
            Packet { id, body: vec![] },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await;
        assert!(result.unwrap_err().downcast_ref::<CloseReason>().is_some());
        assert!(session.account.is_none());
        assert!(session.player.is_none());
    }
    assert!(a.owners.lock().unwrap().is_empty());
    assert_eq!(
        a.world
            .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
            .await
            .unwrap(),
        0
    );
    cancel(&a, &p.receipt).unwrap();
    drop(peer);
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_sql_failure_keeps_process_contract_alive_cancel_preserves_fence_and_healthy_peer()
 {
    let (dir, mut a) = server::lifecycle_test_fixture();
    a.control = Some("control".into());
    a.world.clock.set_paused(true);
    let (base, task) = frontdoor(&a).await;
    let mut players = vec![];
    for guid in ["drain-sql-bad", "drain-sql-good"] {
        let response = client()
            .post(format!("{base}/dev/join"))
            .bearer_auth("control")
            .json(&json!({"guid":guid,"world":-1}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let v: Value = response.json().await.unwrap();
        players.push(v["player"].as_i64().unwrap() as i32);
    }
    let bad = a.store.list("drain-sql-bad".into()).await.unwrap();
    let good = a.store.list("drain-sql-good".into()).await.unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER drain_abort BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'drain account-specific SQL failure'); END;",bad.id)).unwrap();
    let p = prepared(&a).await;
    assert_eq!(p.state, "failed");
    assert!(
        p.errors
            .join(";")
            .contains("drain account-specific SQL failure")
    );
    let original = lifecycle::failure(&a, bad.id).unwrap();
    assert_eq!(
        client()
            .get(format!("{base}/health"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        http(&base, "prepare", json!({}))
            .await
            .json::<Value>()
            .await
            .unwrap()["state"],
        "failed"
    );
    assert_eq!(
        http(&base, "cancel", json!(p.receipt)).await.status(),
        StatusCode::OK
    );
    assert!(a.claim(bad.id).is_err());
    assert_eq!(lifecycle::failure(&a, bad.id).unwrap(), original);
    let response = client()
        .post(format!("{base}/dev/join"))
        .bearer_auth("control")
        .json(&json!({"guid":"drain-sql-good","world":-1}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let player = response.json::<Value>().await.unwrap()["player"]
        .as_i64()
        .unwrap();
    assert_ne!(player, players[1] as i64);
    let response = client()
        .post(format!("{base}/dev/leave"))
        .bearer_auth("control")
        .json(&json!({"player":player}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        a.store
            .load(good.id, good.characters[0].id)
            .await
            .unwrap()
            .items,
        good.characters[0].items
    );
    assert_eq!(
        a.store
            .load(bad.id, bad.characters[0].id)
            .await
            .unwrap()
            .revision,
        bad.characters[0].revision
    );
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_real_qa_direct_and_autonomous_controllers_depart_without_shutdown_watch_or_revival()
 {
    let (dir, mut a) = server::lifecycle_test_fixture();
    std::fs::write(
        dir.join("testing.json"),
        br#"{"schema":1,"provenance":"prepared"}"#,
    )
    .unwrap();
    a.testing = server::testing::Testing::open(&dir).unwrap();
    a.control = Some("control".into());
    a.world.clock.set_paused(true);
    let pos = a
        .world
        .in_world(-2)
        .call(|w| {
            let (x, y) = w.map.spawns[0];
            Ok(Pos { x, y })
        })
        .await
        .unwrap();
    let (base, task) = frontdoor(&a).await;
    let mut saved = vec![];
    for controller in ["direct", "game_bot"] {
        let spec = json!({"guid":format!("drain-{controller}"),"class":WIZARD,"level":8,"seed":12351,"equipment":[2712,-1,-1,-1],"backpack":[2599,2594,-1,-1,-1,-1,-1,-1],"world":-2,"position":pos,"controller":controller,"profile":if controller=="game_bot"{Some("Pixl")}else{None},"stats":null});
        let response=client().post(format!("{base}/dev/testing")).bearer_auth("control").json(&json!({"id":format!("drain-spawn-{controller}"),"command":{"op":"spawn","spec":spec}})).send().await.unwrap();
        let v: Value = response.json().await.unwrap();
        assert_eq!(v["status"], "succeeded", "{v}");
        saved.push((
            v["result"]["account"].as_i64().unwrap() as i32,
            v["result"]["actor"].as_i64().unwrap() as i32,
        ));
    }
    assert_eq!(a.owners.lock().unwrap().len(), 2);
    let p = prepared(&a).await;
    assert_eq!(p.state, "ready", "{:?}", p.errors);
    assert!(!*a.shutdown.borrow());
    assert!(a.owners.lock().unwrap().is_empty());
    assert!(a.bots.lock().unwrap().is_empty());
    assert_eq!(
        a.world
            .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
            .await
            .unwrap(),
        0
    );
    cancel(&a, &p.receipt).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        a.owners.lock().unwrap().is_empty(),
        "cancel revived an abandoned controller"
    );
    for (account, ch) in saved {
        assert!(a.store.load(account, ch).await.unwrap().alive);
    }
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_deadline_preserves_accepted_write_and_late_script_entry_cannot_revive_after_cancel()
 {
    let (dir, mut a) = server::lifecycle_test_fixture();
    a.control = Some("control".into());
    a.world.clock.set_paused(true);
    let (base, task) = frontdoor(&a).await;
    let (ready, release) = a.store.lifecycle_test_pause_reply();
    let url = format!("{base}/dev/join");
    let caller = tokio::spawn(async move {
        client()
            .post(url)
            .bearer_auth("control")
            .json(&json!({"guid":"drain-late-script","world":-1}))
            .send()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM accounts WHERE guid='drain-late-script'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(a.store.lifecycle_test_jobs(), 1);
    let start = tokio::time::Instant::now();
    let p = prepared(&a).await;
    assert_eq!(p.state, "failed");
    assert!(start.elapsed() >= DEADLINE);
    assert!(start.elapsed() < DEADLINE + Duration::from_secs(1));
    assert!(p.errors.join(";").contains("accepted work remains owned"));
    assert_eq!(
        a.store.lifecycle_test_jobs(),
        1,
        "deadline abandoned the committed Store job"
    );
    cancel(&a, &p.receipt).unwrap();
    assert_eq!(
        a.store.lifecycle_test_jobs(),
        1,
        "cancellation abandoned the committed Store job"
    );
    release.send(()).unwrap();
    let response = caller.await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "abandoned pre-claim entry was revived after cancel"
    );
    assert!(a.bots.lock().unwrap().is_empty());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM accounts WHERE guid='drain-late-script'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM characters", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        0
    );
    let next = prepared(&a).await;
    assert_eq!(next.state, "ready", "{:?}", next.errors);
    assert!(next.receipt.generation > p.receipt.generation);
    assert!(status(&a, &p.receipt).is_err());
    cancel(&a, &next.receipt).unwrap();
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_bound_script_entry_finishes_publication_then_retires_without_false_fence()
{
    let (dir, mut a) = server::lifecycle_test_fixture();
    a.control = Some("control".into());
    a.world.clock.set_paused(true);
    let account = a.store.list("drain-bound-script".into()).await.unwrap().id;
    let (base, task) = frontdoor(&a).await;
    let (ready, release) = a.world.lifecycle_test_pause_job(2, false);
    let url = format!("{base}/dev/join");
    let caller = tokio::spawn(async move {
        client()
            .post(url)
            .bearer_auth("control")
            .json(&json!({"guid":"drain-bound-script","world":-2}))
            .send()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    assert!(lifecycle::state(&a, account).unwrap().1.is_some());
    let p = prepare(&a);
    assert_eq!(status(&a, &p.receipt).unwrap().state, "preparing");
    release.send(()).unwrap();
    assert_eq!(caller.await.unwrap().status(), StatusCode::BAD_REQUEST);
    let result = tokio::time::timeout(Duration::from_secs(5), wait(&a, &p.receipt))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.state, "ready", "{:?}", result.errors);
    assert!(lifecycle::state(&a, account).is_none());
    assert!(a.bots.lock().unwrap().is_empty());
    assert_eq!(
        a.store
            .list("drain-bound-script".into())
            .await
            .unwrap()
            .characters
            .len(),
        1
    );
    assert_eq!(
        a.world
            .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
            .await
            .unwrap(),
        0
    );
    cancel(&a, &p.receipt).unwrap();
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_pending_transfer_is_not_consumed_by_frozen_hello_and_can_load_only_after_cancel()
 {
    let (dir, a) = server::lifecycle_test_fixture();
    a.world.clock.set_paused(true);
    let account = a.store.list("drain-transfer".into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let (time, key, _) = a
        .world
        .directory(move |d| d.issue(account, character, -2))
        .await
        .unwrap();
    let p = prepared(&a).await;
    assert_eq!(p.state, "ready");
    let packet = socket_hello(&dir, "drain-transfer", -2, Some((time, key.clone())));
    let mut session = server::Session::default();
    let (tx, _rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    assert!(
        server::handle(
            &mut a.clone(),
            packet,
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now()
        )
        .await
        .is_err()
    );
    assert!(session.account.is_none());
    cancel(&a, &p.receipt).unwrap();
    let packet = socket_hello(&dir, "drain-transfer", -2, Some((time, key.clone())));
    server::handle(
        &mut a.clone(),
        packet,
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    server::handle(
        &mut a.clone(),
        Packet {
            id: wire::LOAD,
            body: character.to_be_bytes().to_vec(),
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    assert!(session.player.is_some());
    assert!(
        a.world
            .directory(move |d| d.consume(account, -2, time, key))
            .await
            .is_err(),
        "transfer consumption must remain once-only"
    );
    session
        .account
        .take()
        .unwrap()
        .close(&a, CloseReason::Controller)
        .unwrap()
        .unwrap()
        .wait()
        .await
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_accepted_create_and_load_settle_before_ready_without_client_cooperation() {
    for creating in [true, false] {
        let (dir, a) = server::lifecycle_test_fixture();
        a.world.clock.set_paused(true);
        let account = a.store.list("drain-selection".into()).await.unwrap().id;
        let existing = if creating {
            None
        } else {
            Some(
                a.store
                    .create(account, a.content.get(WIZARD).unwrap())
                    .await
                    .unwrap()
                    .id,
            )
        };
        let (base, task) = frontdoor(&a).await;
        let port = base.rsplit(':').next().unwrap().parse().unwrap();
        let mut socket = NativeSocket::connect(port, &a.origin).await;
        socket
            .send(socket_hello(&dir, "drain-selection", -2, None))
            .await;
        socket.receive(wire::MAP_INFO).await;
        lifecycle::account(&a, vec![account], "selection_hello_settled", |_| async {
            Ok(())
        })
        .await
        .unwrap();
        let (ready, release) = a.store.lifecycle_test_pause_reply();
        socket
            .send(match existing {
                Some(character) => Packet {
                    id: wire::LOAD,
                    body: character.to_be_bytes().to_vec(),
                },
                None => Packet {
                    id: wire::CREATE,
                    body: (WIZARD as i16).to_be_bytes().to_vec(),
                },
            })
            .await;
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lifecycle::state(&a, account), Some((true, None, false)));
        let p = prepare(&a);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(status(&a, &p.receipt).unwrap().state, "preparing");
        release.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), wait(&a, &p.receipt))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.state, "ready", "{:?}", result.errors);
        assert!(a.owners.lock().unwrap().is_empty());
        assert_eq!(
            a.store
                .list("drain-selection".into())
                .await
                .unwrap()
                .characters
                .len(),
            1
        );
        assert_eq!(
            a.world
                .directory(|d| Ok(d.worlds.values().map(|w| w.players.len()).sum::<usize>()))
                .await
                .unwrap(),
            0
        );
        cancel(&a, &p.receipt).unwrap();
        drop(socket);
        task.abort();
        task.await.unwrap_err();
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_cancelled_accepted_http_delete_keeps_short_claim_through_real_sql_completion()
 {
    let (dir, a) = server::lifecycle_test_fixture();
    let account = a.store.list("drain-delete".into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (base, task) = frontdoor(&a).await;
    let (authenticated, release_auth) = a.store.lifecycle_test_pause_reply();
    let url = format!("{base}/legacy-api/char/delete");
    let character = ch.id.to_string();
    let caller = tokio::spawn(async move {
        client()
            .post(url)
            .form(&[
                ("guid", "drain-delete"),
                ("password", ""),
                ("charId", &character),
            ])
            .send()
            .await
            .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(5), authenticated)
        .await
        .unwrap()
        .unwrap();
    let (committed, release_commit) = a.store.lifecycle_test_pause_reply();
    release_auth.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), committed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lifecycle::state(&a, account), Some((true, None, false)));
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let p = prepare(&a);
    assert_eq!(status(&a, &p.receipt).unwrap().state, "preparing");
    assert!(a.claim(account).is_err());
    release_commit.send(()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), wait(&a, &p.receipt))
            .await
            .unwrap()
            .unwrap()
            .state,
        "ready"
    );
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
    cancel(&a, &p.receipt).unwrap();
    assert!(a.claim(account).is_ok());
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_drain_freeze_interrupts_stalled_output_without_irreversible_shutdown() {
    struct Blocked;
    impl futures_util::Sink<Message> for Blocked {
        type Error = std::io::Error;
        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Pending
        }
        fn start_send(self: std::pin::Pin<&mut Self>, _: Message) -> std::io::Result<()> {
            unreachable!()
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Pending
        }
        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Pending
        }
    }
    let (dir, a) = server::lifecycle_test_fixture();
    let mut drain = stopping(&a);
    let (_lost, mut lost) = tokio::sync::watch::channel(false);
    let mut shutdown = a.shutdown.subscribe();
    let sending = tokio::spawn(async move {
        server::send_frame(
            &mut Blocked,
            Message::Binary(vec![1].into()),
            &mut lost,
            &mut shutdown,
            None,
            &mut drain,
        )
        .await
    });
    tokio::task::yield_now().await;
    let p = prepare(&a);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), sending)
            .await
            .unwrap()
            .unwrap(),
        Err(CloseReason::Shutdown)
    );
    assert!(!*a.shutdown.borrow());
    assert_eq!(wait(&a, &p.receipt).await.unwrap().state, "ready");
    cancel(&a, &p.receipt).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_drain_sigterm_cannot_cancel_ready_generation_or_reopen_admission() {
    let (dir, a) = server::lifecycle_test_fixture();
    let (base, task) = frontdoor(&a).await;
    let ready = prepared(&a).await;
    assert_eq!(ready.state, "ready");
    a.shutdown.send_replace(true);
    let response = http(&base, "cancel", json!(ready.receipt)).await;
    assert_eq!(
        response.status(),
        StatusCode::CONFLICT,
        "SIGTERM reopened admission through matching ready cancel"
    );
    assert!(!open(&a));
    assert!(entry(&a).is_err());
    assert!(a.claim(999999).is_err());
    assert_eq!(status(&a, &ready.receipt).unwrap().state, "ready");
    task.abort();
    task.await.unwrap_err();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_drain_http_task_bound_does_not_reject_existing_controller_entry_or_leak_old_epochs()
 {
    let (dir, a) = server::lifecycle_test_fixture();
    let accepted = (0..a.max_players * 2)
        .map(|_| enter(&a, true).unwrap())
        .collect::<Vec<_>>();
    assert!(enter(&a, true).is_err());
    let controller = entry(&a).unwrap();
    assert_eq!(
        a.admission.state.lock().unwrap().http_entries,
        a.max_players * 2
    );
    drop(controller);
    drop(accepted);
    assert_eq!(a.admission.state.lock().unwrap().entries, 0);
    let old = entry(&a).unwrap();
    let p = prepare(&a);
    drop(old);
    assert_eq!(wait(&a, &p.receipt).await.unwrap().state, "ready");
    cancel(&a, &p.receipt).unwrap();
    assert!(enter(&a, true).is_ok());
    assert_eq!(a.admission.state.lock().unwrap().entries, 0);
    std::fs::remove_dir_all(dir).unwrap();
}
