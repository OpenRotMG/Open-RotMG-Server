use super::*;
pub(crate) fn fixture() -> (PathBuf, App) {
    let dir = std::env::temp_dir().join(format!("session-actions-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let app = App {
        diagnostics: Arc::new(
            super::super::diagnostics::Diagnostics::open(dir.join("bug-reports")).unwrap(),
        ),
        started: std::time::Instant::now(),
        testing: None,
        society: Arc::default(),
        store: Store::open(&dir.join("game.db"), &content).unwrap(),
        world: Handle::spawn(
            crate::world::Directory::test_with_seed(
                std::path::Path::new("data"),
                content.clone(),
                16,
                12351,
            )
            .unwrap(),
        ),
        keys: Arc::new(Keys::load(&dir).unwrap()),
        content,
        swf: Arc::default(),
        origin: "http://127.0.0.1:1".into(),
        sfx: "data/sfx".into(),
        max_players: 16,
        behind_proxy: false,
        shore_bots: false,
        dialogue: None,
        requests: Arc::default(),
        picture_reads: Arc::default(),
        connections: Arc::new(tokio::sync::Semaphore::new(32)),
        control: None,
        discord_link_token: Some("a".repeat(64)),
        owners: Arc::default(),
        admission: Arc::default(),
        authentication: Arc::default(),
        guild_actions: Arc::default(),
        gold_updates: Arc::default(),
        bots: Arc::default(),
        quarantined: Arc::default(),
        shutdown: tokio::sync::watch::channel(false).0,
    };
    (dir, app)
}
#[tokio::test]
async fn private_admission_rejects_forwarding_before_charging_the_shared_limit() {
    let (dir, a) = fixture();
    let peer = "127.0.0.1:1234".parse().unwrap();
    let mut headers = HeaderMap::new();
    assert_eq!(
        private_denial(&a, peer, &headers).unwrap().status(),
        StatusCode::FORBIDDEN
    );
    headers.insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    assert_eq!(
        private_denial(&a, "192.0.2.1:1234".parse().unwrap(), &headers)
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for name in [
        "origin",
        "forwarded",
        "x-forwarded-for",
        "x-real-ip",
        "x-forwarded-host",
        "x-forwarded-proto",
    ] {
        headers.insert(name, "untrusted".parse().unwrap());
        assert_eq!(
            private_denial(&a, peer, &headers).unwrap().status(),
            StatusCode::FORBIDDEN
        );
        headers.remove(name);
    }
    for _ in 0..20 {
        assert!(private_denial(&a, peer, &headers).is_none());
    }
    assert_eq!(
        private_denial(&a, peer, &headers).unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn announcement_requires_private_admission_and_bounded_text() {
    let (dir, a) = fixture();
    let peer = "127.0.0.1:1234".parse().unwrap();
    let announce = |headers: HeaderMap, text: String| {
        let app = a.clone();
        async move {
            discord_announce(
                State(app),
                ConnectInfo(peer),
                headers,
                Json(AnnounceRequest { text }),
            )
            .await
            .status()
        }
    };
    assert_eq!(
        announce(HeaderMap::new(), "Restarting soon".into()).await,
        StatusCode::FORBIDDEN
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    for text in ["   ", &"x".repeat(201), "two\nlines"] {
        assert_eq!(
            announce(headers.clone(), text.into()).await,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        announce(headers, " Server restarting in 20 seconds ".into()).await,
        StatusCode::OK
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn kick_waits_for_save_acknowledgement_and_failed_saves_keep_ownership() {
    let (dir, a) = fixture();
    let owner = a.claim_session(1).unwrap();
    let other = a.claim_session(2).unwrap();
    let mut cancellation = owner.cancel.clone().unwrap();
    let app = a.clone();
    let kick = tokio::spawn(async move { app.kick(1).await });
    cancellation.changed().await.unwrap();
    assert!(!*other.cancel.as_ref().unwrap().borrow());
    assert!(!kick.is_finished());
    assert!(a.claim(1).is_err());
    owner.release();
    assert!(kick.await.unwrap().unwrap());
    assert!(!a.owners.lock().unwrap().contains_key(&1));
    assert!(!a.kick(1).await.unwrap());
    let replacement = a.claim_session(1).unwrap();
    assert!(!*replacement.cancel.as_ref().unwrap().borrow());
    replacement.failed("fixture save failure".into());
    assert!(
        a.kick(1)
            .await
            .unwrap_err()
            .to_string()
            .contains("save failure")
    );
    assert!(a.claim(1).is_err());
    assert!(!*other.cancel.as_ref().unwrap().borrow());
    replacement.release();
    other.release();
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn reset_fences_inflight_old_password_logins_through_session_claim() {
    let (dir, a) = fixture();
    let account = a.store.list("owner".into()).await.unwrap().id;
    a.store
        .register(
            "owner".into(),
            "owner@example.org".into(),
            "old-password".into(),
        )
        .await
        .unwrap();
    let code = a.store.issue_discord_code(account).await.unwrap();
    a.store
        .redeem_discord_code(code, "1558662335076376578".into())
        .await
        .unwrap();
    let code = a.store.issue_recovery_code(account).await.unwrap();
    let token = a
        .store
        .issue_reset("1558662335076376578".into(), Some(code))
        .await
        .unwrap();
    let owner = a.claim_session(account).unwrap();
    let mut cancellation = owner.cancel.clone().unwrap();
    let app = a.clone();
    let reset = tokio::spawn(async move {
        let mut headers = HeaderMap::new();
        headers.insert("origin", app.origin.parse().unwrap());
        reset_password(
            State(app),
            ConnectInfo("127.0.0.1:1234".parse().unwrap()),
            headers,
            Json(ResetRequest {
                token,
                password: "new-password".into(),
            }),
        )
        .await
        .status()
    });
    cancellation.changed().await.unwrap();
    let app = a.clone();
    let mut stale_login = tokio::spawn(async move {
        let _authentication = app.authentication.read().await;
        let id = app
            .store
            .authenticate("owner@example.org".into(), "old-password".into())
            .await?;
        app.claim_session(id)
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), &mut stale_login)
            .await
            .is_err()
    );
    owner.release();
    assert_eq!(reset.await.unwrap(), StatusCode::OK);
    assert!(stale_login.await.unwrap().is_err());
    assert!(
        a.store
            .authenticate("owner@example.org".into(), "new-password".into())
            .await
            .unwrap()
            == account
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn account_actions_reject_target_account_password_and_unknown_fields() {
    assert!(
        serde_json::from_value::<DiscordRequest>(
            serde_json::json!({"discord_id":"1558662335076376578","account":2})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<RecoverRequest>(
            serde_json::json!({"discord_id":"1558662335076376578","password":"not-in-discord"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ResetRequest>(
            serde_json::json!({"token":"a".repeat(64),"password":"new-password","account":2})
        )
        .is_err()
    );
}
