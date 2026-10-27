use super::*;

fn fixture() -> (std::path::PathBuf, Store) {
    let dir = std::env::temp_dir().join(format!("email-verification-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    (dir, store)
}
async fn register(store: &Store, name: &str) -> i32 {
    store
        .register(
            name.into(),
            format!("{name}@example.invalid"),
            "fixture-password".into(),
        )
        .await
        .unwrap()
}
fn ip() -> std::net::IpAddr {
    "127.0.0.1".parse().unwrap()
}

#[tokio::test]
async fn verification_is_single_use_email_bound_and_survives_restart() {
    let (dir, store) = fixture();
    let account = register(&store, "owner").await;
    let other = register(&store, "other").await;
    assert!(!store.email_verified(account).await.unwrap());
    let (_, token) = store.issue_email_verification(account, ip()).await.unwrap();
    let db = Connection::open(dir.join("game.db")).unwrap();
    let saved: String = db
        .query_row("SELECT hash FROM email_verifications", [], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, token_hash(&token).unwrap());
    assert_ne!(saved, token);
    drop(store);
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    let (first, second) = tokio::join!(
        store.confirm_email(token.clone()),
        store.confirm_email(token.clone())
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(store.email_verified(account).await.unwrap());
    assert!(!store.email_verified(other).await.unwrap());
    assert!(store.issue_email_verification(account, ip()).await.is_err());
    assert_eq!(
        store
            .authenticate("owner@example.invalid".into(), "fixture-password".into())
            .await
            .unwrap(),
        account
    );
    db.execute(
        "UPDATE credentials SET email='changed@example.invalid' WHERE account=?1",
        [account],
    )
    .unwrap();
    assert!(!store.email_verified(account).await.unwrap());
    assert!(store.confirm_email(token).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn expired_superseded_malformed_and_changed_email_tokens_are_rejected() {
    let (dir, store) = fixture();
    let account = register(&store, "expiry").await;
    for bad in ["", "bad", &"A".repeat(64), &"z".repeat(64)] {
        assert!(store.confirm_email(bad.into()).await.is_err());
    }
    let (_, old) = store.issue_email_verification(account, ip()).await.unwrap();
    let db = Connection::open(dir.join("game.db")).unwrap();
    db.execute("UPDATE email_send_attempts SET issued_at=issued_at-61", [])
        .unwrap();
    let (_, token) = store.issue_email_verification(account, ip()).await.unwrap();
    assert!(store.confirm_email(old).await.is_err());
    db.execute("UPDATE email_verifications SET expires_at=0", [])
        .unwrap();
    assert!(store.confirm_email(token).await.is_err());
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    let (_, token) = store.issue_email_verification(account, ip()).await.unwrap();
    db.execute(
        "UPDATE credentials SET email='new@example.invalid' WHERE account=?1",
        [account],
    )
    .unwrap();
    assert!(store.confirm_email(token).await.is_err());
    assert!(!store.email_verified(account).await.unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn durable_account_ip_and_global_budgets_fail_before_reserving_another_send() {
    let (dir, store) = fixture();
    let account = register(&store, "limits").await;
    let foreign = register(&store, "budget").await;
    let time = hiscores::now().unwrap();
    let db = Connection::open(dir.join("game.db")).unwrap();
    store.issue_email_verification(account, ip()).await.unwrap();
    assert!(
        store
            .issue_email_verification(account, ip())
            .await
            .unwrap_err()
            .to_string()
            .contains("one minute")
    );
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    for i in 0..5 {
        db.execute(
            "INSERT INTO email_send_attempts VALUES(?1,'different',?2)",
            params![account, time - 120 - i],
        )
        .unwrap();
    }
    assert!(
        store
            .issue_email_verification(account, ip())
            .await
            .unwrap_err()
            .to_string()
            .contains("tomorrow")
    );
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    let hash = format!("{:x}", Sha256::digest(ip().to_string().as_bytes()));
    for i in 0..10 {
        db.execute(
            "INSERT INTO email_send_attempts VALUES(?1,?2,?3)",
            params![foreign, hash, time - 120 - i],
        )
        .unwrap();
    }
    assert!(
        store
            .issue_email_verification(account, ip())
            .await
            .unwrap_err()
            .to_string()
            .contains("one hour")
    );
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    for i in 0..90 {
        db.execute(
            "INSERT INTO email_send_attempts VALUES(?1,'different',?2)",
            params![foreign, time - 120 - i],
        )
        .unwrap();
    }
    assert!(
        store
            .issue_email_verification(account, ip())
            .await
            .unwrap_err()
            .to_string()
            .contains("allowance")
    );
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    for i in 0..2500 {
        db.execute(
            "INSERT INTO email_send_attempts VALUES(?1,'different',?2)",
            params![foreign, time - 86400 - 120 - i],
        )
        .unwrap();
    }
    assert!(
        store
            .issue_email_verification(account, ip())
            .await
            .unwrap_err()
            .to_string()
            .contains("allowance")
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM email_send_attempts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2500
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn migration_does_not_grandfather_accounts_or_recreate_missing_verification_state() {
    let (dir, store) = fixture();
    let account = register(&store, "migration").await;
    drop(store);
    let db = Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("DROP TABLE verified_emails; DROP TABLE email_verifications; DROP TABLE email_send_attempts; PRAGMA user_version=19;").unwrap();
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    assert!(!store.email_verified(account).await.unwrap());
    assert_eq!(
        store
            .authenticate(
                "migration@example.invalid".into(),
                "fixture-password".into()
            )
            .await
            .unwrap(),
        account
    );
    drop(store);
    db.execute("DROP TABLE verified_emails", []).unwrap();
    assert!(
        Store::open(&dir.join("game.db"), &content)
            .err()
            .unwrap()
            .to_string()
            .contains("schema-20")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
