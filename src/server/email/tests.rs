use super::*;
use axum::http::header::ORIGIN;
use serde_json::Value;

async fn body(response: Response) -> String {
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn account(app: &App, operation: &str, password: &str) -> String {
    body(
        account_request(
            State(app.clone()),
            ConnectInfo("127.0.0.1:1234".parse().unwrap()),
            HeaderMap::new(),
            axum::extract::Path(operation.into()),
            Form(HashMap::from([
                ("guid".into(), "http@example.invalid".into()),
                ("password".into(), password.into()),
            ])),
        )
        .await,
    )
    .await
}
async fn provider(
    app: &mut App,
    status: StatusCode,
    receipt: Value,
) -> (Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let capture = sent.clone();
    let router = Router::new().route(
        "/emails",
        post(move |headers: HeaderMap, Json(request): Json<Value>| {
            let sent = capture.clone();
            let receipt = receipt.clone();
            async move {
                assert!(headers["idempotency-key"].to_str().unwrap().starts_with(
                    if request["subject"] == "Reset your Open RotMG password" {
                        "password-reset-"
                    } else {
                        "email-verification-"
                    }
                ));
                sent.lock().unwrap().push(request);
                (status, Json(receipt))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/emails", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    app.email = Some(Arc::new(Service {
        client: reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap(),
        endpoint,
        from: "Open RotMG <accounts@example.invalid>".into(),
        slots: Arc::new(tokio::sync::Semaphore::new(4)),
    }));
    (sent, task)
}
#[tokio::test]
async fn native_forgot_password_is_non_enumerating_and_received_link_resets_without_discord() {
    let (_, mut app) = super::super::test_app();
    let owner = app
        .store
        .register(
            "reset-http".into(),
            "http@example.invalid".into(),
            "fixture-password".into(),
        )
        .await
        .unwrap();
    let (sent, task) = provider(
        &mut app,
        StatusCode::OK,
        serde_json::json!({"id":"reset-fixture-1"}),
    )
    .await;
    for guid in ["absent@example.invalid", "http@example.invalid"] {
        let response = tokio::time::timeout(
            Duration::from_millis(100),
            account_request(
                State(app.clone()),
                ConnectInfo("127.0.0.1:1234".parse().unwrap()),
                HeaderMap::new(),
                axum::extract::Path("forgotPassword".into()),
                Form(HashMap::from([("guid".into(), guid.into())])),
            ),
        )
        .await
        .unwrap();
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(body(response).await, "<Success/>");
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if sent.lock().unwrap().len() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mail = sent.lock().unwrap()[0].clone();
    let token = mail["text"]
        .as_str()
        .unwrap()
        .split("/recover/#")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    assert_eq!(mail["subject"], "Reset your Open RotMG password");
    assert!(!mail.to_string().contains("fixture-password"));
    assert_eq!(account(&app, "forgotPassword", "").await, "<Success/>");
    let response = super::super::account_actions::reset_password(
        State(app.clone()),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        HeaderMap::from_iter([(ORIGIN, app.origin.parse().unwrap())]),
        Json(
            serde_json::from_value(
                serde_json::json!({"token":token,"password":"replacement-password"}),
            )
            .unwrap(),
        ),
    )
    .await;
    assert!(body(response).await.contains("\"reset\":true"));
    assert!(
        app.store
            .authenticate("http@example.invalid".into(), "fixture-password".into())
            .await
            .is_err()
    );
    assert_eq!(
        app.store
            .authenticate("http@example.invalid".into(), "replacement-password".into())
            .await
            .unwrap(),
        owner
    );
    assert!(app.store.reset_account(token.into()).await.is_err());
    tokio::time::timeout(Duration::from_secs(3), async {
        while app.email.as_ref().unwrap().slots.available_permits() != 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(sent.lock().unwrap().len(), 1);
    task.abort();
}

#[tokio::test]
async fn authenticated_native_resend_and_explicit_browser_confirmation_share_durable_state() {
    let (_, mut app) = super::super::test_app();
    let account_id = app
        .store
        .register(
            "http-owner".into(),
            "http@example.invalid".into(),
            "fixture-password".into(),
        )
        .await
        .unwrap();
    let (sent, task) = provider(
        &mut app,
        StatusCode::OK,
        serde_json::json!({"id":"fixture-message-1"}),
    )
    .await;
    assert_eq!(
        account(&app, "verify", "fixture-password").await,
        "<Account/>"
    );
    assert!(
        account(&app, "sendVerifyEmail", "wrong-password")
            .await
            .starts_with("<Error>")
    );
    assert!(sent.lock().unwrap().is_empty());
    assert_eq!(
        account(&app, "sendVerifyEmail", "fixture-password").await,
        "<Success/>"
    );
    let request = sent.lock().unwrap()[0].clone();
    assert_eq!(request["to"], serde_json::json!(["http@example.invalid"]));
    assert!(!request.to_string().contains("fixture-password"));
    let token = request["text"]
        .as_str()
        .unwrap()
        .split("/verify-email/#")
        .nth(1)
        .unwrap()[..64]
        .to_owned();
    let peer = ConnectInfo("127.0.0.1:1234".parse().unwrap());
    assert_eq!(
        confirm(
            State(app.clone()),
            peer,
            HeaderMap::new(),
            Json(Confirmation {
                token: token.clone()
            })
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert!(!app.store.email_verified(account_id).await.unwrap());
    let headers = HeaderMap::from_iter([(ORIGIN, app.origin.parse().unwrap())]);
    let response = confirm(
        State(app.clone()),
        peer,
        headers.clone(),
        Json(Confirmation {
            token: token.clone(),
        }),
    )
    .await;
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(body(response).await, "{\"verified\":true}");
    assert_eq!(
        account(&app, "verify", "fixture-password").await,
        "<Account><VerifiedEmail/></Account>"
    );
    assert_eq!(
        confirm(
            State(app.clone()),
            peer,
            headers,
            Json(Confirmation { token })
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(
        account(&app, "sendVerifyEmail", "fixture-password")
            .await
            .contains("already verified")
    );
    assert_eq!(sent.lock().unwrap().len(), 1);
    task.abort();
    drop(app);
}
#[tokio::test]
async fn reset_delivery_failure_and_unknown_account_have_the_same_ack_and_never_release_quota() {
    let (dir, mut app) = super::super::test_app();
    app.store
        .register(
            "reset-failure".into(),
            "http@example.invalid".into(),
            "fixture-password".into(),
        )
        .await
        .unwrap();
    assert!(
        account(&app, "forgotPassword", "")
            .await
            .contains("not configured")
    );
    let (sent, task) = provider(
        &mut app,
        StatusCode::TOO_MANY_REQUESTS,
        serde_json::json!({"error":"private-provider-detail"}),
    )
    .await;
    assert_eq!(account(&app, "forgotPassword", "").await, "<Success/>");
    tokio::time::timeout(Duration::from_secs(3), async {
        while app.email.as_ref().unwrap().slots.available_permits() != 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(account(&app, "forgotPassword", "").await, "<Success/>");
    tokio::time::timeout(Duration::from_secs(3), async {
        while app.email.as_ref().unwrap().slots.available_permits() != 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(sent.lock().unwrap().len(), 1);
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM email_send_attempts", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        1
    );
    assert!(!app.store.email_verified(1).await.unwrap());
    task.abort();
}

#[tokio::test]
async fn delivery_failures_never_report_success_or_release_the_reserved_budget() {
    for (status, receipt) in [
        (
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({"error":"provider-quota"}),
        ),
        (StatusCode::OK, serde_json::json!({})),
    ] {
        let (_, mut app) = super::super::test_app();
        app.store
            .register(
                "failed-owner".into(),
                "http@example.invalid".into(),
                "fixture-password".into(),
            )
            .await
            .unwrap();
        assert!(
            account(&app, "sendVerifyEmail", "fixture-password")
                .await
                .contains("not configured")
        );
        let (sent, task) = provider(&mut app, status, receipt).await;
        let result = account(&app, "sendVerifyEmail", "fixture-password").await;
        assert!(result.starts_with("<Error>"), "{result}");
        assert!(
            account(&app, "sendVerifyEmail", "fixture-password")
                .await
                .contains("one minute")
        );
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert_eq!(
            app.store
                .authenticate("http@example.invalid".into(), "fixture-password".into())
                .await
                .unwrap(),
            1
        );
        task.abort();
        drop(app);
    }
}
