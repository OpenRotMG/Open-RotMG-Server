use super::*;

#[tokio::test]
async fn community_social_replacement_arrives_in_nexus_when_dead_realms_slot_is_full() {
    let (dir, app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    let portal = app
        .world
        .directory(|d| {
            d.prepare_realm_capacity(4)?;
            let n = d.worlds.get_mut(&-2).context("Nexus missing")?;
            let portal = n
                .entities
                .values()
                .find(|e| e.kind == 0x712)
                .context("Realm portal missing")?
                .clone();
            n.map = Arc::new(Map {
                width: n.map.width,
                height: n.map.height,
                cells: n.map.cells.clone(),
                spawns: vec![(portal.pos.x, portal.pos.y)],
            });
            Ok(portal.id)
        })
        .await
        .unwrap();
    let p = PROFILES[0];
    let mut old = app
        .store
        .shore_character(p.identity.into(), p.name.into(), &app.content, p.healer)
        .await
        .unwrap();
    old.level = 5;
    old.exp = crate::store::level_start(5).unwrap();
    app.store.progress(old.clone()).await.unwrap();
    let mut owner = Some(app.claim(old.account).unwrap());
    let mut owners = vec![];
    let (old_id, old_rx, old_sender, _) = enter(&app, p, old.clone(), -3, None).await.unwrap();
    let session = app
        .world
        .directory(move |d| Ok(d.player_view(old_id)?.session))
        .await
        .unwrap();
    let vault_before = app.store.vault(old.account, 0).await.unwrap();
    old.hp = 0;
    old.alive = false;
    app.world
        .for_player(old_id)
        .call(move |w| {
            let player = w
                .players
                .get_mut(&old_id)
                .context("prepared old actor missing")?;
            player.character.hp = 0;
            player.death_pending = true;
            Ok(())
        })
        .await
        .unwrap();
    server::lifecycle::finalize(&app, old_id, server::lifecycle::CloseReason::Controller)
        .unwrap()
        .wait()
        .await
        .unwrap();
    drop((old_rx, old_sender));
    let mut others = vec![];
    for p in &PROFILES[1..5] {
        let ch = app
            .store
            .shore_character(p.identity.into(), p.name.into(), &app.content, p.healer)
            .await
            .unwrap();
        owners.push(app.claim(ch.account).unwrap());
        others.push(enter(&app, *p, ch, -3, None).await.unwrap());
    }
    assert_eq!(
        app.world
            .in_world(-3)
            .call(|w| Ok((w.players.len(), w.max_players)))
            .await
            .unwrap(),
        (4, 4)
    );
    assert!(app.store.load(old.account, old.id).await.is_err());
    let (id, rx, sender, brain) = reenter(
        &app,
        p,
        old.account,
        &mut owner,
        Reentry::Replacement {
            previous: session.character,
        },
    )
    .await
    .expect("ordinary replacement arrival must not compete for its dead Realm slot");
    let v = app
        .world
        .directory(move |d| d.player_view(id))
        .await
        .unwrap();
    assert_eq!(v.session.account, old.account);
    assert_ne!(v.session.character, old.id);
    assert_eq!(v.map.name, "Nexus");
    let current = app
        .store
        .load(old.account, v.session.character)
        .await
        .unwrap();
    assert_eq!(
        (current.class, current.level, current.exp),
        (crate::content::PRIEST, 1, 0)
    );
    assert_eq!(
        current.items,
        app.content.get(crate::content::PRIEST).unwrap().equipment
    );
    let vault_after = app.store.vault(old.account, 0).await.unwrap();
    assert_eq!(vault_before.revision, vault_after.revision);
    assert_eq!(vault_before.items, vault_after.items);
    let rejection = app
        .world
        .directory(move |d| {
            let (account, character, destination) = d.destination(id, Some(portal))?;
            d.issue(account, character, destination)
        })
        .await
        .expect_err("the replacement must not bypass the still-full ordinary portal");
    assert!(rejection.to_string().contains("full"), "{rejection:#}");
    assert_eq!(app.owners.lock().unwrap().len(), 5);
    assert!(app.quarantined.lock().unwrap().is_empty());
    assert!(!v.lifecycle.quarantined && v.lifecycle.alive);
    assert_eq!(
        app.world
            .in_world(-3)
            .call(|w| Ok((w.players.len(), w.max_players)))
            .await
            .unwrap(),
        (4, 4)
    );
    std::fs::write(dir.join("rebuild-admission-result.json"),serde_json::to_vec_pretty(&json!({"scope":"deterministic production-helper engineering regression; direct fixture requests committed death and replacement; NOT natural-controller proof","old":old,"new":current,"arrival":v.session,"portal_rejection":rejection.to_string(),"owners":5,"global_cap":app.max_players,"realm_cap":4,"realm_count":4,"mind":brain.mind})).unwrap()).unwrap();
    println!(
        "replacement admission engineering evidence: {}",
        dir.display()
    );
    server::lifecycle::finalize(&app, id, server::lifecycle::CloseReason::Controller)
        .unwrap()
        .wait()
        .await
        .unwrap();
    drop((rx, sender));
    for (id, rx, sender, _) in others {
        server::lifecycle::finalize(&app, id, server::lifecycle::CloseReason::Controller)
            .unwrap()
            .wait()
            .await
            .unwrap();
        drop((rx, sender));
    }
    drop(owner);
    drop(owners);
    app.shutdown.send_replace(true);
}
