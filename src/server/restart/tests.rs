use super::*;

#[test]
fn cancel_before_deadline_fences_the_exact_owner_and_cannot_be_replayed() {
    let mut service = Service::default();
    let now = Instant::now();
    assert!(service.status(now).is_err());
    let pending = service.begin("a".repeat(64), now).unwrap();
    assert!(service.begin("b".repeat(64), now).is_err());
    assert!(service.finish(&pending.receipt, false, now).is_err());
    let mut foreign = pending.receipt.clone();
    for field in [0, 1, 2] {
        match field {
            0 => foreign.instance = "b".repeat(64),
            1 => foreign.owner = "b".repeat(64),
            _ => foreign.deadline_ms += 1,
        }
        assert!(service.finish(&foreign, true, now).is_err());
        foreign = pending.receipt.clone();
    }
    let cancelled = service
        .finish(
            &pending.receipt,
            true,
            now + NOTICE - Duration::from_nanos(1),
        )
        .unwrap();
    assert_eq!(cancelled.state, "cancelled");
    assert!(service.finish(&pending.receipt, true, now).is_err());
    assert_eq!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .unwrap()
            .state,
        "cancelled"
    );
    assert!(service.begin("a".repeat(64), now + NOTICE).is_err());
    let next = service.begin("b".repeat(64), now + NOTICE).unwrap();
    assert!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .is_err()
    );
    assert_eq!(service.status(now + NOTICE).unwrap().receipt, next.receipt);
}

#[test]
fn deadline_and_consumption_are_irreversible_and_process_instances_are_distinct() {
    let now = Instant::now();
    let mut service = Service::default();
    let pending = service.begin("a".repeat(64), now).unwrap();
    assert!(service.status(now + NOTICE).is_err());
    assert!(
        service
            .finish(&pending.receipt, true, now + NOTICE)
            .is_err()
    );
    let mut replacement = Service::default();
    replacement.begin("a".repeat(64), now).unwrap();
    assert!(replacement.finish(&pending.receipt, true, now).is_err());
    assert_eq!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .unwrap()
            .state,
        "consumed"
    );
    assert!(
        service
            .finish(&pending.receipt, true, now + NOTICE)
            .is_err()
    );
    assert!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .is_err()
    );
    assert!(service.begin("b".repeat(64), now + NOTICE).is_err());
}

#[test]
fn expired_pending_owner_can_be_replaced_but_never_consumed_by_a_stale_request() {
    let now = Instant::now();
    let mut service = Service::default();
    for owner in ["", "a", &"A".repeat(64), &"g".repeat(64)] {
        assert!(service.begin(owner.into(), now).is_err());
    }
    let stale = service.begin("a".repeat(64), now).unwrap();
    service.begin("b".repeat(64), now + NOTICE).unwrap();
    assert!(service.finish(&stale.receipt, false, now + NOTICE).is_err());
}

#[test]
fn typed_update_intent_is_emitted_only_at_the_first_valid_stop_consumption() {
    let now = Instant::now();
    let mut service = Service::default();
    let pending = service.begin("a".repeat(64), now).unwrap();
    assert!(service.consume(&pending.receipt, now).is_err());
    let mut foreign = pending.receipt.clone();
    foreign.instance = "f".repeat(64);
    assert!(service.consume(&foreign, now + NOTICE).is_err());
    let (status, update) = service.consume(&pending.receipt, now + NOTICE).unwrap();
    assert_eq!(status.state, "consumed");
    assert_eq!(update.unwrap().operation, pending.receipt);
    assert!(service.consume(&pending.receipt, now + NOTICE).is_err());

    let mut service = Service::default();
    let cancelled = service.begin("b".repeat(64), now).unwrap();
    service.finish(&cancelled.receipt, true, now).unwrap();
    let (status, update) = service.consume(&cancelled.receipt, now + NOTICE).unwrap();
    assert_eq!(status.state, "cancelled");
    assert!(update.is_none());
    let expired = service.begin("c".repeat(64), now + NOTICE).unwrap();
    service.begin("d".repeat(64), now + NOTICE * 2).unwrap();
    assert!(service.consume(&expired.receipt, now + NOTICE * 2).is_err());
}

async fn call(
    app: &App,
    body: serde_json::Value,
    peer: &str,
    authorized: bool,
) -> (StatusCode, serde_json::Value) {
    let mut headers = HeaderMap::new();
    if authorized {
        headers.insert(
            "authorization",
            format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
        );
    }
    let response = request(
        State(app.clone()),
        ConnectInfo(peer.parse().unwrap()),
        headers,
        Json(serde_json::from_value(body).unwrap()),
    )
    .await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn production_handler_preserves_private_authorization_and_shutdown_and_drain_fences() {
    let (dir, app) = crate::server::test_app();
    let begin = serde_json::json!({"operation":"begin", "owner":"b".repeat(64)});
    for (peer, auth) in [("127.0.0.1:1", false), ("192.0.2.1:1", true)] {
        assert_eq!(
            call(&app, begin.clone(), peer, auth).await.0,
            StatusCode::FORBIDDEN
        );
        assert!(app.restart.lock().unwrap().current.is_none());
    }
    let pending = call(&app, begin, "127.0.0.1:1", true).await;
    assert_eq!(pending.0, StatusCode::OK);
    let receipt = pending.1["receipt"].clone();
    let invalid =
        serde_json::json!({"operation":"cancel", "receipt": receipt, "actor_id":"invalid"});
    assert_eq!(
        call(&app, invalid, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    let cancel = serde_json::json!({"operation":"cancel", "receipt": receipt, "actor_id":"1558662335076376580"});
    assert_eq!(
        call(&app, cancel.clone(), "127.0.0.1:1", true).await.1["state"],
        "cancelled"
    );
    assert!(!*app.shutdown.borrow());
    assert!(lifecycle::drain::open(&app));
    assert_eq!(
        call(&app, cancel.clone(), "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    app.shutdown.send_replace(true);
    assert_eq!(
        call(&app, cancel.clone(), "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    app.shutdown.send_replace(false);
    lifecycle::drain::prepare(&app);
    assert_eq!(
        call(&app, cancel, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
