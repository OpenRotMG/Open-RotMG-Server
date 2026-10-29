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

fn saved_career(label: &str) -> (std::path::PathBuf, std::path::PathBuf, App) {
    let (scratch, mut app) = crate::server::test_app();
    let mut path = scratch.join("game.db");
    if let Some(results) = std::env::var_os("QA_RESULTS_DIR") {
        let dir = std::path::PathBuf::from(results).join(label);
        std::fs::create_dir(&dir).unwrap();
        path = dir.join("game.db");
        app.store = crate::store::Store::open(&path, &app.content).unwrap();
    }
    eprintln!("{label}: {}", path.display());
    (scratch, path, app)
}

#[tokio::test]
async fn ordinary_session_logout_and_reopen_preserve_career_and_selected_living_character() {
    let (scratch, path, mut app) = saved_career("ordinary-persona-session");
    app.bot_session_seconds = [0, 0];
    let profile = *persona::PERSONAS
        .iter()
        .find(|p| p.name == "Equrel")
        .unwrap();
    let mut character = runtime::persona_character(&app, profile).await.unwrap();
    character.items[4] = -1;
    let mut vault = app.store.vault(character.account, 0).await.unwrap();
    vault.items[0] = -1;
    app.store
        .exchange(
            vec![crate::store::InventoryWrite {
                id: character.id,
                account: character.account,
                revision: character.revision,
                items: character.items.clone(),
            }],
            vec![vault],
        )
        .await
        .unwrap();
    character = app
        .store
        .load(character.account, character.id)
        .await
        .unwrap();
    character
        .gain_experience(&app.content, 10, &mut rand::thread_rng())
        .unwrap();
    app.store.progress(character.clone()).await.unwrap();
    let vaults = serde_json::to_value(app.store.vaults(character.account).await.unwrap()).unwrap();
    let mut mind = pursuits::Mind::new(character.account);
    mind.future_class = Some(character.class);
    let (_, mut view) = quest_tests::fixture();
    view.session.account = character.account;
    view.session.character = character.id;
    view.character.class = character.class;
    view.character.level = character.level;
    view.character.experience = character.exp;
    mind.intentions
        .as_mut()
        .unwrap()
        .observe(&view, &app.content, profile.identity)
        .unwrap();
    mind.social = serde_json::from_value(serde_json::json!({"episodes":[{
        "account":character.account + 1000,"name":"Peer","own_character":character.id,
        "interaction":"ReceivedHealth","amount":1
    }]}))
    .unwrap();
    app.store.save_bot_mind(mind.clone()).await.unwrap();
    let (done, mut departed) = mpsc::channel(1);
    for _ in 0..2 {
        let owner = app.claim(character.account).unwrap();
        runtime::launch(
            &app,
            profile,
            character.clone(),
            (-2, None),
            None,
            owner,
            Some(done.clone()),
        )
        .await
        .unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(10), departed.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exit.account, character.account);
        assert!(!exit.retire, "named careers do not retire on timed logout");
        done.send(exit).await.unwrap();
        let mut online = BTreeMap::from([(profile.identity, character.account)]);
        let mut resting = BTreeMap::new();
        maintain(&app, &done, &mut departed, &mut online, &mut resting)
            .await
            .unwrap();
        assert!(online.is_empty());
        assert!(resting[profile.identity] > app.world.clock.now() + Duration::from_secs(19 * 60));
        let wallet = app.store.presentation(character.account).await.unwrap();
        app.store = crate::store::Store::open(&path, &app.content).unwrap();
        assert_eq!(
            app.store
                .shore_account(profile.identity.into())
                .await
                .unwrap(),
            Some(character.account)
        );
        let saved = app
            .store
            .bot_mind(character.account)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.intentions, mind.intentions);
        assert_eq!(saved.social.episodes, mind.social.episodes);
        assert_eq!(
            app.store.presentation(character.account).await.unwrap(),
            wallet
        );
        assert_eq!(
            serde_json::to_value(app.store.vaults(character.account).await.unwrap()).unwrap(),
            vaults
        );
        let guid = app.store.account_guid(character.account).await.unwrap();
        let living = app.store.list(guid).await.unwrap().characters;
        let competing = living.iter().position(|ch| ch.id != character.id).unwrap();
        assert_ne!(living[competing].id, character.id);
        let mut rng = rand::rngs::StdRng::seed_from_u64(persona::seed(profile.identity));
        let (roster, vault) = persona::life(&profile, &app.content, &mut rng).unwrap();
        let returned = app
            .store
            .persona_character(
                profile.identity.into(),
                roster,
                vault,
                competing,
                persona::starting_stock(&profile, &app.content).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            returned.id, character.id,
            "saved life wins over a different roster pick"
        );
        assert_eq!(returned.name, character.name);
        assert_eq!(returned.items, character.items);
        assert_eq!(returned.revision, character.revision);
        assert_eq!(returned.exp, character.exp);
        assert_eq!(returned.base, character.base);
        character = returned;
    }
    app.shutdown.send_replace(true);
    lifecycle::drain::prepare(&app);
    lifecycle::drain::terminate(&app).await.unwrap();
    drop(app);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[tokio::test]
async fn committed_persona_deaths_reopen_as_normal_starters_without_reseeding_wealth_or_history() {
    let (scratch, path, mut app) = saved_career("dead-persona-replacement");
    let profile = *persona::PERSONAS
        .iter()
        .find(|p| p.name == "Equrel")
        .unwrap();
    let original = runtime::persona_character(&app, profile).await.unwrap();
    let mut mind = pursuits::Mind::new(original.account);
    mind.future_class = Some(original.class);
    app.store.save_bot_mind(mind).await.unwrap();
    let mut vault = app.store.vault(original.account, 0).await.unwrap();
    vault.items.fill(-1);
    app.store
        .exchange(
            vec![crate::store::InventoryWrite {
                id: original.id,
                account: original.account,
                revision: original.revision,
                items: original.items.clone(),
            }],
            vec![vault],
        )
        .await
        .unwrap();
    let guid = app.store.account_guid(original.account).await.unwrap();
    for _ in 0..2 {
        let living = app.store.list(guid.clone()).await.unwrap().characters;
        let dead: Vec<_> = living.iter().map(|ch| ch.id).collect();
        for mut ch in living {
            ch.alive = false;
            ch.hp = 0;
            app.store
                .death(ch, "prepared lifecycle regression".into())
                .await
                .unwrap();
        }
        let wallet = app.store.presentation(original.account).await.unwrap();
        let vaults =
            serde_json::to_value(app.store.vaults(original.account).await.unwrap()).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let history = || {
            db.query_row("SELECT json_group_array(json_array(id,stats,items,revision)) FROM (SELECT * FROM characters WHERE alive=0 ORDER BY id)", [], |r| r.get::<_, String>(0)).unwrap()
        };
        let before = history();
        app.store = crate::store::Store::open(&path, &app.content).unwrap();
        let replacement = runtime::persona_character(&app, profile).await.unwrap();
        let fresh =
            crate::store::Character::fresh(app.content.get(original.class).unwrap()).unwrap();
        assert_eq!(replacement.account, original.account);
        assert_eq!(replacement.name, original.name);
        assert!(!dead.contains(&replacement.id));
        assert_eq!(
            (
                replacement.class,
                replacement.level,
                replacement.exp,
                replacement.fame
            ),
            (original.class, 1, 0, 0)
        );
        assert_eq!(replacement.items, fresh.items);
        assert_eq!(replacement.base, fresh.base);
        assert_eq!(replacement.lifetime, fresh.lifetime);
        assert_eq!(
            app.store.list(guid.clone()).await.unwrap().characters.len(),
            1
        );
        assert_eq!(history(), before, "dead history is immutable");
        assert_eq!(
            app.store.presentation(original.account).await.unwrap(),
            wallet
        );
        assert_eq!(
            serde_json::to_value(app.store.vaults(original.account).await.unwrap()).unwrap(),
            vaults
        );
        for id in dead {
            assert!(app.store.load(original.account, id).await.is_err());
            assert!(db.query_row("SELECT EXISTS(SELECT 1 FROM deaths WHERE character=?1 AND receipt IS NOT NULL)", [id], |r| r.get::<_, bool>(0)).unwrap());
        }
        assert_eq!(
            runtime::persona_character(&app, profile).await.unwrap(),
            replacement
        );
    }
    drop(app);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[tokio::test]
async fn all_dead_legacy_personas_initialize_missing_intent_from_real_death_not_synthetic_archive()
{
    let (scratch, path, mut app) = saved_career("legacy-persona-intent");
    let fresh =
        crate::store::Character::fresh(app.content.get(crate::content::WIZARD).unwrap()).unwrap();
    let mut archived =
        crate::store::Character::fresh(app.content.get(crate::content::PRIEST).unwrap()).unwrap();
    archived.alive = false;
    archived.hp = 0;
    archived.items.fill(-1);
    let roster = vec![fresh.clone(), archived];
    let identity = "persona:v1:LegacyCareer".to_owned();
    let mut character = app
        .store
        .persona_character(identity.clone(), roster.clone(), vec![[-1; 8]], 0, None)
        .await
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let archived_class: u16 = db.query_row("SELECT json_extract(stats,'$.class') FROM characters WHERE account=?1 ORDER BY id DESC LIMIT 1", [character.account], |r| r.get(0)).unwrap();
    assert_ne!(archived_class, character.class);
    assert!(
        app.store
            .bot_mind(character.account)
            .await
            .unwrap()
            .is_none()
    );
    for legacy_mind in [false, true] {
        if legacy_mind {
            let mut mind = app
                .store
                .bot_mind(character.account)
                .await
                .unwrap()
                .unwrap();
            mind.schema = 1;
            mind.intentions = None;
            mind.future_class = None;
            app.store.save_bot_mind(mind).await.unwrap();
        }
        character.alive = false;
        character.hp = 0;
        app.store
            .death(character.clone(), "prepared legacy lifecycle".into())
            .await
            .unwrap();
        app.store = crate::store::Store::open(&path, &app.content).unwrap();
        let replacement = app
            .store
            .persona_character(identity.clone(), roster.clone(), vec![[-1; 8]], 0, None)
            .await
            .unwrap();
        assert_eq!(replacement.account, character.account);
        assert_ne!(replacement.id, character.id);
        assert_eq!(
            (replacement.class, replacement.level, replacement.exp),
            (fresh.class, 1, 0)
        );
        assert_eq!(replacement.items, fresh.items);
        assert_eq!(
            app.store
                .bot_mind(character.account)
                .await
                .unwrap()
                .unwrap()
                .future_class,
            Some(fresh.class)
        );
        assert!(
            app.store
                .load(character.account, character.id)
                .await
                .is_err()
        );
        character = replacement;
    }
    drop(app);
    std::fs::remove_dir_all(scratch).unwrap();
}
