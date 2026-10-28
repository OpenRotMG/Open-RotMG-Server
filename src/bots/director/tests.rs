use super::*;

#[test]
fn blue_heavy_targets_keep_all_slots_and_class_relative_boundaries() {
    assert_eq!(targets(40), [4, 16, 16, 4]);
    assert_eq!(targets(1), [0, 1, 0, 0]);
    for population in 0..=85 {
        assert_eq!(targets(population).iter().sum::<usize>(), population);
    }
    for (stars, band) in [(0, 1), (12, 1), (13, 2), (25, 2), (26, 3), (65, 3)] {
        assert_eq!(rank_band("persona:v1:test", stars, 13), band);
    }
    assert_eq!(rank_band("guest:v1:1", 5, 13), 0);
}

#[tokio::test]
async fn population_uses_persisted_ranks_and_never_fills_missing_bands_with_veterans() {
    for exhausted in [false, true] {
        let (_dir, mut app) = crate::server::test_app();
        app.bot_population = 10;
        let classes = app.content.classes.len() as i32;
        if exhausted {
            for mut profile in persona::PERSONAS
                .iter()
                .copied()
                .filter(|p| p.stars < classes)
            {
                // Catalog labels must not override an account's preserved, later career.
                profile.stars += classes * 2;
                runtime::persona_character(&app, profile).await.unwrap();
            }
        }
        start(&app).await.unwrap();
        let expected = if exhausted {
            [1, 0, 4, 1]
        } else {
            [1, 4, 4, 1]
        };
        let mut accounts = Vec::new();
        for selector in crate::bots::profiles() {
            let guest = selector.starts_with("guest:v1:");
            let identity = if guest {
                selector.to_owned()
            } else {
                format!("persona:v1:{selector}")
            };
            if let Some(account) = app.store.shore_account(identity).await.unwrap() {
                accounts.push((guest, account));
            }
        }
        tokio::time::timeout(Duration::from_secs(15), async {
            while accounts
                .iter()
                .filter(|(_, account)| app.cctv.account_session(*account).is_some())
                .count()
                < expected.iter().sum::<usize>()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut counts = [0usize; 4];
        for &(guest, account) in &accounts {
            if app.cctv.account_session(account).is_some() {
                let stars = app.store.account_stars(account).await.unwrap();
                let band = if guest {
                    0
                } else if stars < classes {
                    1
                } else if stars < classes * 2 {
                    2
                } else {
                    3
                };
                counts[band] += 1;
            }
        }
        app.shutdown.send_replace(true);
        tokio::time::timeout(Duration::from_secs(15), async {
            while accounts
                .iter()
                .any(|(_, account)| app.cctv.account_session(*account).is_some())
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(counts, expected, "exhausted low-rank pool: {exhausted}");
        assert!(app.world.supervisor.latest().unwrap().is_none());
    }
}

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
    let mut online = BTreeMap::from([(profile.identity, character.account)]);
    let mut resting = BTreeMap::new();
    lifecycle::drain::prepare(&app);
    maintain(&app, &done, &mut departed, &mut online, &mut resting)
        .await
        .unwrap();
    assert_eq!(departed.len(), 1);
    assert!(online.contains_key(profile.identity));
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
