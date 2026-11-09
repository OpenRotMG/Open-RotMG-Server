use super::*;
use serde_json::json;

fn begin(a: &App, owner: char) -> Status {
    apply(
        a,
        Request::Begin {
            owner: owner.to_string().repeat(64),
            seconds: 90,
        },
    )
    .unwrap()
}
fn expire(a: &App) {
    a.admission
        .state
        .lock()
        .unwrap()
        .restart
        .as_mut()
        .unwrap()
        .deadline = tokio::time::Instant::now();
}
#[tokio::test]
async fn cancellation_and_cutover_are_receipt_bound_and_mutually_exclusive() {
    let (_dir, app) = crate::server::test_app();
    assert!(apply(&app, Request::Status { receipt: None }).is_err());
    let pending = begin(&app, 'a');
    assert!(
        apply(
            &app,
            Request::Begin {
                owner: "b".repeat(64),
                seconds: 90
            }
        )
        .is_err()
    );
    assert!(
        apply(
            &app,
            Request::Consume {
                receipt: pending.receipt.clone()
            }
        )
        .is_err()
    );
    let mut stale = pending.receipt.clone();
    stale.owner = "b".repeat(64);
    assert!(
        apply(
            &app,
            Request::Cancel {
                receipt: stale,
                actor_id: "175037041837473797".into()
            }
        )
        .is_err()
    );
    let cancelled = apply(
        &app,
        Request::Cancel {
            receipt: pending.receipt.clone(),
            actor_id: "175037041837473797".into(),
        },
    )
    .unwrap();
    assert_eq!(cancelled.state, "cancelled");
    assert_eq!(
        apply(
            &app,
            Request::Consume {
                receipt: pending.receipt.clone()
            }
        )
        .unwrap()
        .state,
        "cancelled"
    );
    assert!(open(&app));
    let next = begin(&app, 'b');
    assert!(
        apply(
            &app,
            Request::Consume {
                receipt: pending.receipt
            }
        )
        .is_err()
    );
    expire(&app);
    assert!(
        apply(
            &app,
            Request::Cancel {
                receipt: next.receipt.clone(),
                actor_id: "175037041837473797".into()
            }
        )
        .is_err()
    );
    assert_eq!(
        apply(
            &app,
            Request::Consume {
                receipt: next.receipt.clone()
            }
        )
        .unwrap()
        .state,
        "consumed"
    );
    assert!(
        apply(
            &app,
            Request::Consume {
                receipt: next.receipt.clone()
            }
        )
        .is_err()
    );
    assert!(
        apply(
            &app,
            Request::Cancel {
                receipt: next.receipt,
                actor_id: "175037041837473797".into()
            }
        )
        .is_err()
    );
    let (_dir2, other) = crate::server::test_app();
    let fresh = begin(&other, 'c');
    assert_ne!(
        app.admission.state.lock().unwrap().instance,
        fresh.receipt.instance
    );
    assert!(
        apply(
            &app,
            Request::Status {
                receipt: Some(fresh.receipt)
            }
        )
        .is_err()
    );
}
#[tokio::test]
async fn private_http_route_rejects_unauthorized_forwarded_and_malformed_requests() {
    let (_dir, app) = crate::server::test_app();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/internal/lifecycle/restart",
        listener.local_addr().unwrap()
    );
    let router = super::super::routes().with_state(app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let client = reqwest::Client::new();
    let body = json!({"operation":"begin","owner":"a".repeat(64),"seconds":90});
    for token in ["", "wrong"] {
        assert_eq!(
            client
                .post(&url)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("a".repeat(64))
            .header("x-forwarded-for", "127.0.0.1")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = client
        .post(&url)
        .bearer_auth("a".repeat(64))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let pending: serde_json::Value = response.json().await.unwrap();
    assert_eq!(pending["state"], "pending");
    for malformed in [
        json!({"operation":"cancel","receipt":pending["receipt"],"actor_id":"x"}),
        json!({"operation":"status","extra":true}),
    ] {
        assert!(
            client
                .post(&url)
                .bearer_auth("a".repeat(64))
                .json(&malformed)
                .send()
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
    }
    let cancelled = client.post(&url).bearer_auth("a".repeat(64)).json(&json!({"operation":"cancel","receipt":pending["receipt"],"actor_id":"175037041837473797"})).send().await.unwrap();
    assert_eq!(
        cancelled.json::<serde_json::Value>().await.unwrap()["state"],
        "cancelled"
    );
    server.abort();
    let _ = server.await;
    assert!(open(&app));
}
