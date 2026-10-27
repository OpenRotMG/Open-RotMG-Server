use super::*;

#[tokio::test]
async fn drain_blocks_population_writes_and_shutdown_is_not_a_critical_exit() {
    let (_dir, mut app) = crate::server::test_app();
    app.bot_population = 1;
    let profile = persona::PERSONAS[0];
    let character = runtime::persona_character(&app, profile).await.unwrap();
    let (done, mut departed) = mpsc::channel(1);
    done.send(Departure {
        profile,
        account: character.account,
        retire: true,
    })
    .await
    .unwrap();
    let mut online = BTreeSet::from([profile.identity]);
    let mut resting = BTreeMap::new();
    lifecycle::drain::prepare(&app);
    maintain(&app, &done, &mut departed, &mut online, &mut resting)
        .await
        .unwrap();
    assert_eq!(departed.len(), 1);
    assert!(online.contains(profile.identity));
    assert_eq!(
        app.store
            .shore_account(profile.identity.into())
            .await
            .unwrap(),
        Some(character.account)
    );
    let receivers = app.shutdown.receiver_count();
    start(&app).await.unwrap();
    assert_eq!(app.shutdown.receiver_count(), receivers + 1);
    app.shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_secs(2), async {
        while app.shutdown.receiver_count() != receivers {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(app.world.supervisor.latest().unwrap().is_none());
}
