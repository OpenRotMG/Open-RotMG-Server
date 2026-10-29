#[tokio::test]
async fn full_admission_is_retryable_without_fencing_or_stealing_a_human_slot() {
    let (_dir, mut app) = super::super::account_actions::tests::fixture();
    app.max_players = 1;
    let human = app.store.list("admission-human".into()).await.unwrap().id;
    let waiting = app.store.list("admission-waiting".into()).await.unwrap().id;
    let owner = app.claim(human).unwrap();
    assert!(app.try_claim(waiting).unwrap().is_none());
    assert_eq!(
        app.claim(waiting).err().unwrap().to_string(),
        "development world full"
    );
    assert_eq!(app.owners.lock().unwrap().len(), 1);
    drop(owner);
    assert!(app.try_claim(waiting).unwrap().is_some());
}

#[tokio::test]
async fn failed_session_releases_the_account_when_it_closes() {
    let (_dir, app) = super::super::account_actions::tests::fixture();
    let account = app.store.list("failed-session".into()).await.unwrap().id;
    let owner = app.claim(account).unwrap();
    owner.failed("injected session failure".into());
    assert!(app.try_claim(account).is_err());
    drop(owner);
    assert!(app.try_claim(account).unwrap().is_some());
}
