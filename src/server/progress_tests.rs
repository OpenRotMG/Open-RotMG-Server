use super::*;

#[tokio::test]
async fn reserved_progress_accounts_start_before_the_first_store_reply() {
    let (dir, app) = account_actions::tests::fixture();
    app.world.clock.set_paused(true);
    let mut accounts = Vec::new();
    let mut owners = Vec::new();
    for n in 0..2 {
        let account = app
            .store
            .list(format!("progress-convoy-{n}"))
            .await
            .unwrap();
        let character = app
            .store
            .create(account.id, app.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let saved = (account.id, character.id);
        let owner = app.claim_session(account.id).unwrap();
        let player = app
            .world
            .call(move |w| {
                let id = w.enter(character, None)?;
                w.players.get_mut(&id).unwrap().experience_queued = 1;
                Ok(id)
            })
            .await
            .unwrap();
        owner.bind(player).unwrap();
        accounts.push((saved, player));
        owners.push(owner);
    }
    let before = app.store.lifecycle_test_job_count();
    let (entered, release) = app.store.lifecycle_test_pause_reply();
    let tick = tokio::spawn({
        let app = app.clone();
        async move { persistence_tick(&app).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered)
        .await
        .unwrap()
        .unwrap();
    let all_started = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while app.store.lifecycle_test_job_count() < before + 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .is_ok();
    release.send(()).unwrap();
    tick.await.unwrap().unwrap();
    assert!(
        all_started,
        "later reserved accounts waited for the first account's completion"
    );
    for ((account, character), player) in accounts {
        assert_eq!(app.store.load(account, character).await.unwrap().exp, 1);
        assert_eq!(
            app.world
                .call(move |w| Ok(w.players[&player].character.exp))
                .await
                .unwrap(),
            1
        );
    }
    for owner in owners {
        owner
            .close(&app, CloseReason::Controller)
            .unwrap()
            .unwrap()
            .wait()
            .await
            .unwrap();
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
