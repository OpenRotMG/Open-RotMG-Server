use super::*;

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
