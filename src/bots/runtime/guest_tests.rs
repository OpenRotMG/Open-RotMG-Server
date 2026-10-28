use super::*;
use crate::store::InventoryWrite;

#[tokio::test]
async fn guest_persona_uses_ordinary_account_starters_and_resumes_saved_progress() {
    let (dir, app) = crate::server::test_app();
    let profile = selected_profile("guest:v1:1").unwrap();
    assert_eq!(
        (profile.stars, profile.role, profile.directions),
        (0, persona::Role::Newbie, 4)
    );
    assert!(!profile.knows_blue && !profile.knows_minimap);
    let mut ch = persona_character(&app, profile).await.unwrap();
    let guid = app.store.account_guid(ch.account).await.unwrap();
    let account = app.store.list(guid.clone()).await.unwrap();
    assert_eq!(
        app.store
            .authenticate(guid.clone(), String::new())
            .await
            .unwrap(),
        ch.account
    );
    assert_eq!(ch.name, format!("Guest{}", ch.account));
    assert!(!crate::store::name_chosen(ch.account, &ch.name));
    assert!(!account.xml("", 0.).contains("<NameChosen/>"));
    assert_eq!((account.character_slots, account.best_fame.len()), (1, 1));
    assert_eq!(app.store.presentation(ch.account).await.unwrap().stars(), 0);
    let fresh = Character::fresh(app.content.get(crate::content::WIZARD).unwrap()).unwrap();
    assert_eq!(
        (ch.class, ch.level, ch.exp, ch.fame),
        (fresh.class, 1, 0, 0)
    );
    assert_eq!(
        (&ch.items, ch.base, ch.lifetime.clone()),
        (&fresh.items, fresh.base, fresh.lifetime)
    );
    let vaults = app.store.vaults(ch.account).await.unwrap();
    assert_eq!(vaults.len(), 1);
    assert_eq!(vaults[0].items, [-1; 8]);
    // A prepared committed save checks resumption, not earned progression.
    ch.level = 2;
    ch.exp = crate::store::level_start(2).unwrap();
    app.store.progress(ch.clone()).await.unwrap();
    assert_eq!(persona_character(&app, profile).await.unwrap(), ch);
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let counts: (i32, i32, i32) = db.query_row(
        "SELECT (SELECT count(*) FROM credentials WHERE account=?1),(SELECT count(*) FROM characters WHERE account=?1),(SELECT count(*) FROM deaths JOIN characters ON deaths.character=characters.id WHERE characters.account=?1)",
        [ch.account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(counts, (0, 1, 0));
    let owner = app.claim(ch.account).unwrap();
    let (id, _output, _sender, brain) = enter(&app, profile, ch.clone(), -2, None).await.unwrap();
    assert_eq!(brain.mind.market.name, ch.name);
    assert_eq!(app.cctv.binding(&ch.name), Some((id, ch.account)));
    assert_eq!(app.cctv.binding(profile.name), None);
    assert_eq!(
        app.cctv
            .account_session(ch.account)
            .map(|(name, id)| (name.to_string(), id)),
        Some((ch.name.clone(), id))
    );
    if let Some(closing) = close_controller(&app, owner, None, CloseReason::Shutdown).unwrap() {
        closing.wait().await.unwrap();
    }
    drop(db);
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn guest_persona_registration_is_an_error_not_a_new_identity_or_fake_guest() {
    let (dir, app) = crate::server::test_app();
    let profile = selected_profile("guest:v1:2").unwrap();
    let ch = persona_character(&app, profile).await.unwrap();
    let guid = app.store.account_guid(ch.account).await.unwrap();
    app.store
        .register(
            guid.clone(),
            "guest-guard@example.org".into(),
            "fixture-password".into(),
        )
        .await
        .unwrap();
    assert!(persona_character(&app, profile).await.is_err());
    assert!(app.store.authenticate(guid, String::new()).await.is_err());
    assert_eq!(
        app.store
            .shore_account(profile.identity.into())
            .await
            .unwrap(),
        Some(ch.account)
    );
    assert_eq!(app.store.load(ch.account, ch.id).await.unwrap(), ch);
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn guest_persona_generic_spawn_rejects_selector_before_creating_an_account() {
    let (dir, mut app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    std::fs::write(
        dir.join("testing.json"),
        r#"{"schema":1,"provenance":"prepared"}"#,
    )
    .unwrap();
    app.testing = crate::server::testing::Testing::open(&dir).unwrap();
    let (x, y) = app
        .world
        .in_world(-2)
        .call(|w| Ok(w.map.spawns[0]))
        .await
        .unwrap();
    let fresh = Character::fresh(app.content.get(crate::content::WIZARD).unwrap()).unwrap();
    let error = crate::server::testing::spawn(
        &app,
        app.testing.as_ref().unwrap(),
        crate::server::testing::Spawn {
            guid: "guest-selector-must-not-be-a-name".into(),
            class: fresh.class,
            level: 1,
            seed: 746,
            equipment: fresh.items[..4].try_into().unwrap(),
            backpack: [-1; 8],
            world: -2,
            position: Pos { x, y },
            controller: crate::server::testing::Controller::GameBot,
            profile: Some("guest:v1:1".into()),
            stats: None,
        },
    )
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("use Persona starting context"),
        "{error:#}"
    );
    assert!(validate_test_profile("BoxxyCC").is_ok());
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    for table in ["accounts", "characters", "shore_bots"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i32>(0))
                .unwrap(),
            0
        );
    }
    drop(db);
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn guest_persona_controller_resume_death_replacement_and_retirement_preserve_accounts() {
    let (dir, mut app) = crate::server::test_app();
    let retained = std::env::var_os("QA_RESULTS_DIR")
        .map(|root| std::path::PathBuf::from(root).join("guest-lifecycle"));
    // Managed QA keeps the ordinary Store fixture for the independent credential-free backup roundtrip.
    if let Some(path) = &retained {
        std::fs::create_dir(path).unwrap();
        app.store = crate::store::Store::open(&path.join("game.db"), &app.content).unwrap();
    }
    app.world.clock.set_paused(true);
    std::fs::write(
        dir.join("testing.json"),
        r#"{"schema":1,"provenance":"prepared"}"#,
    )
    .unwrap();
    app.testing = crate::server::testing::Testing::open(&dir).unwrap();
    let (x, y) = app
        .world
        .in_world(-2)
        .call(|w| Ok(w.map.spawns[0]))
        .await
        .unwrap();
    let position = Pos { x, y };
    let profile = selected_profile("guest:v1:3").unwrap();
    let first = start_test_persona(&app, profile, -2, position)
        .await
        .unwrap();
    let first_player = app.cctv.binding(&first.name).unwrap().0;
    app.stop_persona(first.account).await.unwrap();
    assert!(app.cctv.binding(&first.name).is_none());
    let saved = persona_character(&app, profile).await.unwrap();
    assert_eq!((saved.account, saved.id), (first.account, first.id));
    let owner = app.claim(saved.account).unwrap();
    launch(
        &app,
        profile,
        saved.clone(),
        (-2, Some(position)),
        None,
        owner,
        None,
    )
    .await
    .unwrap();
    let resumed_player = app.cctv.binding(&saved.name).unwrap().0;
    assert_ne!(resumed_player, first_player);
    app.stop_persona(saved.account).await.unwrap();
    assert!(app.cctv.binding(&saved.name).is_none());
    let settled = app.store.load(saved.account, saved.id).await.unwrap();
    assert_eq!(
        (
            settled.account,
            settled.id,
            &settled.name,
            settled.class,
            settled.level,
            settled.exp,
            settled.base,
            &settled.items,
            settled.revision
        ),
        (
            saved.account,
            saved.id,
            &saved.name,
            saved.class,
            saved.level,
            saved.exp,
            saved.base,
            &saved.items,
            saved.revision
        )
    );
    settled.lifetime.follows(&saved.lifetime).unwrap();
    assert!(!app.world.supervisor.account_fenced(saved.account).unwrap());

    // A prepared spare makes inherited dead inventory distinguishable from ordinary starter gear.
    let mut dead = settled;
    dead.items[5] = 0xa22;
    dead.revision = app
        .store
        .exchange(
            vec![InventoryWrite {
                id: dead.id,
                account: dead.account,
                revision: dead.revision,
                items: dead.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    dead.alive = false;
    dead.hp = 0;
    app.store
        .death(dead.clone(), "Guest lifecycle fixture".into())
        .await
        .unwrap();
    assert!(
        app.store
            .death(dead.clone(), "duplicate fixture death".into())
            .await
            .is_err()
    );
    assert!(app.store.load(dead.account, dead.id).await.is_err());
    let replacement = persona_character(&app, profile).await.unwrap();
    let fresh = Character::fresh(app.content.get(crate::content::WIZARD).unwrap()).unwrap();
    assert_eq!(replacement.account, dead.account);
    assert_ne!(replacement.id, dead.id);
    assert_eq!(
        (
            &replacement.items,
            replacement.class,
            replacement.level,
            replacement.exp
        ),
        (&fresh.items, fresh.class, 1, 0)
    );
    let owner = app.claim(replacement.account).unwrap();
    launch(
        &app,
        profile,
        replacement.clone(),
        (-2, Some(position)),
        None,
        owner,
        None,
    )
    .await
    .unwrap();
    let replacement_player = app.cctv.binding(&replacement.name).unwrap().0;
    assert_eq!(
        app.world
            .player_view(replacement_player)
            .await
            .unwrap()
            .session
            .character,
        replacement.id
    );
    app.stop_persona(replacement.account).await.unwrap();
    let replacement = app
        .store
        .load(replacement.account, replacement.id)
        .await
        .unwrap();
    app.store
        .retire_persona(profile.identity.into(), replacement.account)
        .await
        .unwrap();
    let retired_key = format!(
        "guest:v1:retired:{}:{}",
        replacement.account, profile.identity
    );
    assert_eq!(
        app.store.shore_account(retired_key.clone()).await.unwrap(),
        Some(replacement.account)
    );
    let next = persona_character(&app, profile).await.unwrap();
    assert_ne!(next.account, replacement.account);
    assert_eq!(next.name, format!("Guest{}", next.account));
    let owner = app.claim(next.account).unwrap();
    launch(
        &app,
        profile,
        next.clone(),
        (-2, Some(position)),
        None,
        owner,
        None,
    )
    .await
    .unwrap();
    let next_player = app.cctv.binding(&next.name).unwrap().0;
    assert_eq!(
        app.world
            .player_view(next_player)
            .await
            .unwrap()
            .session
            .account,
        next.account
    );
    app.stop_persona(next.account).await.unwrap();
    let next = app.store.load(next.account, next.id).await.unwrap();
    assert!(!crate::store::name_chosen(next.account, &next.name));
    assert_eq!(
        app.store
            .load(replacement.account, replacement.id)
            .await
            .unwrap(),
        replacement
    );
    assert_eq!((&next.items, next.level, next.exp), (&fresh.items, 1, 0));
    assert_eq!(
        app.store.vaults(replacement.account).await.unwrap()[0].items,
        [-1; 8]
    );
    assert_eq!(
        app.store.vaults(next.account).await.unwrap()[0].items,
        [-1; 8]
    );
    if let Some(path) = retained {
        let facts = serde_json::json!({"active_key":profile.identity,"retired_key":retired_key,"retired_account":replacement.account,"new_account":next.account,"dead_character":dead.id,"replacement_character":replacement.id,"new_character":next.id,"controller_players":[first_player,resumed_player,replacement_player,next_player],"controller_reconnect":true,"prepared_death":true,"prepared_spare_health_potion":true});
        std::fs::write(
            path.join("lifecycle.json"),
            serde_json::to_vec_pretty(&facts).unwrap(),
        )
        .unwrap();
        eprintln!("guest lifecycle fixture: {}", path.display());
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
