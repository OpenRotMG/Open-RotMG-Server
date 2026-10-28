use super::{
    tests::{fixture, ip, register},
    *,
};

#[tokio::test]
async fn email_reset_proves_ownership_without_discord_and_is_atomic_single_use_after_restart() {
    let (dir, store) = fixture();
    let account = register(&store, "reset").await;
    assert!(
        store
            .issue_email_reset("absent@example.invalid".into(), ip())
            .await
            .unwrap()
            .is_none()
    );
    let (owner, email, token) = store
        .issue_email_reset("RESET@EXAMPLE.INVALID".into(), ip())
        .await
        .unwrap()
        .unwrap();
    assert_eq!((owner, email.as_str()), (account, "reset@example.invalid"));
    assert!(!store.email_verified(account).await.unwrap());
    assert!(store.confirm_email(token.clone()).await.is_err());
    assert!(store.issue_email_verification(account, ip()).await.is_err());
    let db = Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT hash FROM email_password_resets", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        token_hash(&token).unwrap()
    );
    assert_eq!(
        db.query_row(
            "SELECT expires_at-issued_at FROM email_password_resets",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        900
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM email_send_attempts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .authenticate(email.clone(), "fixture-password".into())
            .await
            .unwrap(),
        account
    );
    drop(store);
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    let verifier = store
        .prepare_password("replacement-password".into())
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store.finish_reset(token.clone(), verifier.clone()),
        store.finish_reset(token.clone(), verifier)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(store.reset_account(token).await.is_err());
    assert!(store.email_verified(account).await.unwrap());
    assert!(
        store
            .authenticate(email.clone(), "fixture-password".into())
            .await
            .is_err()
    );
    assert_eq!(
        store
            .authenticate(email, "replacement-password".into())
            .await
            .unwrap(),
        account
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM discord_recovery", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn reset_tokens_expire_are_email_bound_superseded_and_revoked_on_password_change() {
    let (dir, store) = fixture();
    let account = register(&store, "bound").await;
    let db = Connection::open(dir.join("game.db")).unwrap();
    let issue = || store.issue_email_reset("bound@example.invalid".into(), ip());
    let (_, _, old) = issue().await.unwrap().unwrap();
    assert!(issue().await.unwrap().is_none());
    db.execute("UPDATE email_send_attempts SET issued_at=issued_at-61", [])
        .unwrap();
    let (_, _, token) = issue().await.unwrap().unwrap();
    assert!(store.reset_account(old).await.is_err());
    db.execute("UPDATE email_password_resets SET expires_at=0", [])
        .unwrap();
    assert!(store.reset_account(token).await.is_err());
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    let (_, _, token) = issue().await.unwrap().unwrap();
    db.execute(
        "UPDATE credentials SET email='changed@example.invalid' WHERE account=?1",
        [account],
    )
    .unwrap();
    assert!(store.reset_account(token).await.is_err());
    db.execute(
        "UPDATE credentials SET email='bound@example.invalid' WHERE account=?1",
        [account],
    )
    .unwrap();
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    let (_, _, token) = issue().await.unwrap().unwrap();
    store
        .change_password(
            account,
            store
                .prepare_password("replacement-password".into())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(store.reset_account(token).await.is_err());
    assert!(!store.email_verified(account).await.unwrap());
    db.execute("DELETE FROM email_send_attempts", []).unwrap();
    let (_, verification) = store.issue_email_verification(account, ip()).await.unwrap();
    assert!(store.reset_account(verification).await.is_err());
    assert!(issue().await.unwrap().is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn migration_preserves_schema20_and_refuses_missing_schema21_reset_authority() {
    let (dir, store) = fixture();
    let account = register(&store, "migrate-reset").await;
    drop(store);
    let db = Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("DROP TABLE email_password_resets; PRAGMA user_version=20;")
        .unwrap();
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    assert_eq!(
        store
            .authenticate(
                "migrate-reset@example.invalid".into(),
                "fixture-password".into()
            )
            .await
            .unwrap(),
        account
    );
    drop(store);
    db.execute("DROP TABLE email_password_resets", []).unwrap();
    assert!(
        Store::open(&dir.join("game.db"), &content)
            .err()
            .unwrap()
            .to_string()
            .contains("schema-21")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
