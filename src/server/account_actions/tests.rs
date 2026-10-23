use super::*;
mod community_campaign;
pub(crate) fn fixture() -> (PathBuf, App) {
    fixture_with_realm_seed(None)
}
fn fixture_with_realm_seed(production_seed: Option<u64>) -> (PathBuf, App) {
    let dir = std::env::temp_dir().join(format!("session-actions-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let app = App {
        diagnostics: Arc::new(
            super::super::diagnostics::Diagnostics::open(dir.join("bug-reports")).unwrap(),
        ),
        started: std::time::Instant::now(),
        testing: None,
        society: Arc::default(),
        store: Store::open(&dir.join("game.db"), &content).unwrap(),
        world: Handle::spawn(
            match production_seed {
                Some(seed) => crate::world::Directory::with_seed(
                    std::path::Path::new("data"),
                    content.clone(),
                    16,
                    seed,
                    2,
                ),
                None => crate::world::Directory::test_with_seed(
                    std::path::Path::new("data"),
                    content.clone(),
                    16,
                    12351,
                ),
            }
            .unwrap(),
        ),
        keys: Arc::new(Keys::load(&dir).unwrap()),
        content,
        swf: Arc::default(),
        origin: "http://127.0.0.1:1".into(),
        sfx: "data/sfx".into(),
        max_players: 16,
        behind_proxy: false,
        shore_bots: false,
        dialogue: None,
        requests: Arc::default(),
        picture_reads: Arc::default(),
        connections: Arc::new(tokio::sync::Semaphore::new(32)),
        control: None,
        discord_link_token: Some("a".repeat(64)),
        browser_links: Arc::default(),
        owners: Arc::default(),
        admission_started: Arc::default(),
        admission: Arc::default(),
        authentication: Arc::default(),
        guild_actions: Arc::default(),
        gold_updates: Arc::default(),
        bots: Arc::default(),
        quarantined: Arc::default(),
        shutdown: tokio::sync::watch::channel(false).0,
    };
    (dir, app)
}

fn community_pursuit_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("openrotmg=info,openrotmg::world::native_hits=debug")
        .with_test_writer()
        .try_init();
}
async fn community_pursuit_stop(app: &App) {
    app.shutdown.send(true).unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.owners.lock().unwrap().is_empty() {
        crate::server::fixture_persistence_tick(app).await.unwrap();
        assert!(
            tokio::time::Instant::now() < deadline,
            "controller ownership did not release; quarantined={}",
            app.quarantined.lock().unwrap().len()
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(app.quarantined.lock().unwrap().is_empty());
}

async fn community_pursuit_prepare_supply(
    app: &App,
    character: crate::store::Character,
) -> (crate::store::Character, Pos) {
    let account = character.account;
    let character_id = character.id;
    let presentation = app.store.presentation(account).await.unwrap();
    let owner = app.claim(account).unwrap();
    let (id, owner) = super::super::lifecycle::account(
        app,
        vec![account],
        "community_supply_entry",
        move |a| async move {
            let mut owner = owner;
            owner.arm_cleanup(&a, tokio::runtime::Handle::current());
            let bind = super::super::lifecycle::binding(&a, account)?;
            super::super::lifecycle::stage(super::super::lifecycle::Stage::Publication);
            let id = a
                .world
                .in_world(-3)
                .call(move |w| {
                    let id = w.enter_at(character, None, None, presentation)?;
                    bind(id)?;
                    Ok(id)
                })
                .await?;
            Ok((id, owner))
        },
    )
    .await
    .unwrap();
    let pos = app
        .world
        .directory(move |d| Ok(d.player_view(id)?.position))
        .await
        .unwrap();
    super::super::lifecycle::player(app, id, "community_supply_drop", move |a| async move {
        super::super::lifecycle::stage(super::super::lifecycle::Stage::Reservation);
        let plan = a
            .world
            .for_player(id)
            .call(move |w| {
                w.drop_item(
                    id,
                    crate::world::Slot {
                        object: id,
                        slot: 4,
                        kind: 0xa22,
                    },
                )
            })
            .await?;
        super::super::persist(&a, plan).await
    })
    .await
    .unwrap();
    app.world
        .for_player(id)
        .call(move |w| {
            let bag = w
                .entities
                .matching_mut(|e| e.pos.distance(pos) < 1. && e.items.contains(&0xa22))
                .next()
                .unwrap();
            bag.items = vec![0xa22, 0xa22, -1, -1, -1, -1, -1, -1];
            bag.extra_stats.push((54, account));
            Arc::make_mut(&mut w.map).spawns = vec![(pos.x, pos.y)];
            Ok(())
        })
        .await
        .unwrap();
    owner
        .close(app, super::super::lifecycle::CloseReason::Controller)
        .unwrap()
        .expect("prepared supply owner lost its bound avatar")
        .wait()
        .await
        .unwrap();
    assert!(
        super::super::lifecycle::state(app, account).is_none(),
        "prepared supply custody must release before controller enrollment"
    );
    let mut ch = app.store.load(account, character_id).await.unwrap();
    let spare = app
        .content
        .objects
        .values()
        .find(|o| o.slot_type == 1 && o.tier == Some(3))
        .unwrap()
        .kind as i32;
    ch.items[4..].copy_from_slice(&[spare, 0xa23, 0xa23, 0xa23, 0xa23, 0xa23, 0xa23, -1]);
    ch.revision = app
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    (ch, pos)
}

#[tokio::test]
async fn community_pursuit_selected_supply_commits_partial_banks_and_resumes_same_activity() {
    community_pursuit_logging();
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let account = app
        .store
        .list("community-supply-selected".into())
        .await
        .unwrap();
    let ch = app
        .store
        .create(account.id, app.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let (ch, pos) = community_pursuit_prepare_supply(&app, ch).await;
    let character = ch.id;
    // The real dropped bag expires after 60s; no scene mutation occurs after enrollment.
    let claim = app.claim(ch.account).unwrap();
    crate::bots::start_test(&app, ch, -3, pos, "Biceps", claim)
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut observations = Vec::new();
    let result = loop {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let views = app
            .world
            .directory(move |d| {
                d.worlds
                    .values()
                    .flat_map(|w| w.players.iter())
                    .filter(|(_, p)| p.character.account == account.id)
                    .map(|(id, _)| d.player_view(*id).map(|v| serde_json::to_value(v).unwrap()))
                    .collect::<Result<Vec<_>>>()
            })
            .await
            .unwrap();
        observations.extend(views);
        let mind = app.store.bot_mind(account.id).await.unwrap().unwrap();
        if mind
            .history
            .iter()
            .any(|h| h.reason == "reserve_carried_satisfied")
        {
            break true;
        }
        if tokio::time::Instant::now() >= deadline {
            break false;
        }
    };
    community_pursuit_stop(&app).await;
    let mind = app.store.bot_mind(account.id).await.unwrap().unwrap();
    std::fs::write(
        dir.join("community-supply-observations.json"),
        serde_json::to_vec(&observations).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("community-supply-mind.json"),
        serde_json::to_vec_pretty(&mind).unwrap(),
    )
    .unwrap();
    println!("community supply evidence: {}", dir.display());
    assert!(result, "supply errand did not finish: {:?}", mind.history);
    let ids: std::collections::BTreeSet<_> = mind
        .history
        .iter()
        .filter(|h| h.activity > 0)
        .map(|h| h.activity)
        .collect();
    assert_eq!(ids.len(), 1);
    assert!(
        mind.history
            .iter()
            .any(|h| h.carried == 1 && h.reason == "capacity_interruption")
    );
    assert!(
        mind.history
            .iter()
            .any(|h| h.reason == "space_committed_retaining_supply" && h.carried == 1)
    );
    let reopened = Store::open(&dir.join("game.db"), &app.content).unwrap();
    let saved = reopened.load(account.id, character).await.unwrap();
    assert_eq!(
        saved.items.iter().skip(4).filter(|k| **k == 0xa22).count(),
        2
    );
    assert!(
        reopened
            .vault(account.id, 0)
            .await
            .unwrap()
            .items
            .iter()
            .any(|k| *k >= 0)
    );
    assert_eq!(reopened.bot_mind(account.id).await.unwrap().unwrap(), mind);
}

#[tokio::test]
async fn community_pursuit_ordinary_death_and_restart_preserve_account_not_dead_holdings() {
    community_pursuit_logging();
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let target = app
        .store
        .shore_character("Bayleaf".into(), "Triceps".into(), &app.content, true)
        .await
        .unwrap();
    let account = target.account;
    let character = target.id;
    community_pursuit_prepare_supply(&app, target).await;
    crate::bots::start(&app).await.unwrap();
    assert_eq!(app.owners.lock().unwrap().len(), 6);
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if app
            .store
            .bot_mind(account)
            .await
            .unwrap()
            .unwrap()
            .history
            .iter()
            .any(|h| h.carried == 1)
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            let debug = app
                .world
                .directory(move |d| {
                    Ok(d.worlds
                        .values()
                        .flat_map(|w| w.players.iter())
                        .filter(|(_, p)| p.character.account == account)
                        .map(|(id, _)| serde_json::to_value(d.player_view(*id).unwrap()).unwrap())
                        .collect::<Vec<_>>())
                })
                .await
                .unwrap();
            std::fs::write(
                dir.join("community-lifetime-failed-view.json"),
                serde_json::to_vec_pretty(&debug).unwrap(),
            )
            .unwrap();
            panic!(
                "ordinary controller did not partially collect; {}",
                dir.display()
            );
        }
    }
    app.world.clock.set_paused(true);
    community_pursuit_stop(&app).await;
    let before = app.store.bot_mind(account).await.unwrap().unwrap();
    assert!(before.history.iter().any(|h| h.carried == 1));
    assert_eq!(
        app.store
            .load(account, character)
            .await
            .unwrap()
            .items
            .iter()
            .filter(|k| **k == 0xa22)
            .count(),
        1
    );
    drop(app);

    let (_, mut app) = fixture();
    app.world.clock.set_paused(true);
    app.store = Store::open(&dir.join("game.db"), &app.content).unwrap();
    let durable = app.store.bot_mind(account).await.unwrap().unwrap();
    assert_eq!(durable, before);
    crate::bots::start(&app).await.unwrap();
    let rebound = app.store.bot_mind(account).await.unwrap().unwrap();
    let before_state = serde_json::to_value(&before).unwrap();
    let rebound_state = serde_json::to_value(&rebound).unwrap();
    std::fs::write(
        dir.join("community-restart-before.json"),
        serde_json::to_vec_pretty(&before_state).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("community-restart-rebound.json"),
        serde_json::to_vec_pretty(&rebound_state).unwrap(),
    )
    .unwrap();
    println!("community living restart evidence: {}", dir.display());
    assert!(before_state["activity"].is_object());
    assert!(
        rebound_state["activity"].is_object(),
        "living restart erased the live project and retained only history"
    );
    assert_eq!(
        rebound_state["activity"]["id"],
        before_state["activity"]["id"]
    );
    assert_eq!(rebound_state["activity"]["character"], character);
    assert_eq!(rebound_state["activity"]["carried"], 1);
    assert_eq!(rebound_state["activity"]["step"], "Seeking");
    assert!(
        rebound_state["activity"]["opportunity"].is_null(),
        "restarted project retained a stale world/bag handle"
    );
    let binding = rebound.history.back().unwrap();
    assert_eq!(binding.activity, before.history.back().unwrap().activity);
    assert_eq!(binding.carried, 1);
    assert_ne!(binding.session, before.history.back().unwrap().session);
    let stale = before.clone();
    assert!(app.store.save_bot_mind(stale).await.is_err());
    assert!(
        rebound
            .history
            .iter()
            .any(|h| h.reason == "original_world_not_restored_after_restart"),
        "restart invented opportunity continuity"
    );
    let player = app
        .world
        .directory(move |d| {
            Ok(d.worlds
                .values()
                .flat_map(|w| w.players.iter())
                .find(|(_, p)| p.character.account == account)
                .unwrap()
                .0
                .to_owned())
        })
        .await
        .unwrap();
    app.world
        .for_player(player)
        .call(move |w| {
            assert!(
                !w.safe,
                "declared lethal hazard must be in the Realm, not a safe hub"
            );
            let pos = w.players[&player].pos;
            let definition = w.content.get(0x917)?;
            let spec = definition
                .projectiles
                .values()
                .find(|p| p.multi_hit && p.max == 100)
                .unwrap()
                .clone();
            w.bullets.push(crate::world::Bullet {
                owner: -99,
                source_kind: definition.kind,
                id: 0,
                friendly: false,
                start: pos,
                pos,
                angle: 0.,
                born: w.now,
                damage: spec.max,
                spec,
                hit: Default::default(),
            });
            w.step();
            assert!(
                w.players[&player].death_pending,
                "declared XML projectile did not register lethal damage"
            );
            Ok(())
        })
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let replacement = loop {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        crate::server::fixture_persistence_tick(&app).await.unwrap();
        let living = app
            .world
            .directory(move |d| {
                Ok(d.worlds
                    .values()
                    .flat_map(|w| w.players.values())
                    .find(|p| p.character.account == account && p.character.id != character)
                    .map(|p| p.character.clone()))
            })
            .await
            .unwrap();
        if let Some(ch) = living {
            break ch;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "ordinary runtime did not enter its own replacement"
        );
    };
    assert_eq!(replacement.account, account);
    assert_ne!(replacement.id, character);
    assert_eq!(
        replacement.items,
        app.content.get(replacement.class).unwrap().equipment
    );
    assert!(app.store.load(account, character).await.is_err());
    assert!(app.store.fame(account, character).await.is_ok());
    app.world.clock.set_paused(true);
    community_pursuit_stop(&app).await;
    let mind = app.store.bot_mind(account).await.unwrap().unwrap();
    assert!(
        mind.history
            .iter()
            .any(|h| h.reason == "original_world_not_restored_after_restart")
    );
    assert!(
        mind.history
            .iter()
            .any(|h| h.reason == "character_died_steps_invalidated"
                && h.activity == before_state["activity"]["id"].as_u64().unwrap()
                && h.binding[1] == character)
    );
    let replacement_state = serde_json::to_value(&mind).unwrap();
    if let Some(activity) = replacement_state["activity"].as_object() {
        assert_ne!(activity["id"], before_state["activity"]["id"]);
        assert_eq!(activity["character"], replacement.id);
    }
    std::fs::write(
        dir.join("community-lifetime-mind.json"),
        serde_json::to_vec_pretty(&mind).unwrap(),
    )
    .unwrap();
    println!("community ordinary lifetime evidence: {}", dir.display());
    let replacement_id = replacement.id;
    drop(app);
    let (_, mut app) = fixture();
    app.world.clock.set_paused(true);
    app.store = Store::open(&dir.join("game.db"), &app.content).unwrap();
    crate::bots::start(&app).await.unwrap();
    let living = app
        .world
        .directory(move |d| {
            Ok(d.worlds
                .values()
                .flat_map(|w| w.players.values())
                .filter(|p| p.character.account == account)
                .map(|p| p.character.id)
                .collect::<Vec<_>>())
        })
        .await
        .unwrap();
    assert_eq!(living, vec![replacement_id]);
    assert!(app.store.load(account, character).await.is_err());
    community_pursuit_stop(&app).await;
}

#[tokio::test]
async fn community_pursuit_equal_resource_selected_controllers_learn_before_committed_equip() {
    community_pursuit_logging();
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let first = app.store.list("community-familiar".into()).await.unwrap();
    for n in 0..3 {
        app.store
            .list(format!("community-cadence-padding-{n}"))
            .await
            .unwrap();
    }
    let second = app.store.list("community-unfamiliar".into()).await.unwrap();
    assert_eq!(first.id % 4, second.id % 4);
    let map = Arc::new(crate::content::Map {
        width: 48,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            48 * 32
        ],
        spawns: vec![(10.5, 10.5)],
    });
    let content = app.content.clone();
    app.world
        .directory(move |d| {
            let mut w = crate::world::World::new(content, map)?;
            w.name = "Realm".into();
            d.worlds.insert(-3, w);
            Ok(())
        })
        .await
        .unwrap();
    for (account, x, familiar) in [(first.id, 10.5, true), (second.id, 35.5, false)] {
        let ch = app
            .store
            .create(account, app.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let mut mind = crate::bots::pursuits::Mind::new(account);
        if familiar {
            mind.facts.push(crate::bots::pursuits::ItemFact {
                kind: 0xa9a,
                assessments: 2,
            });
        }
        app.store.save_bot_mind(mind).await.unwrap();
        app.world
            .in_world(-3)
            .call(move |w| {
                let bag = w.spawn_entity(
                    0x500,
                    Pos { x, y: 11.5 },
                    vec![0xa9a, -1, -1, -1, -1, -1, -1, -1],
                )?;
                w.entities
                    .get_mut(&bag)
                    .unwrap()
                    .extra_stats
                    .push((54, account));
                Ok(())
            })
            .await
            .unwrap();
        let claim = app.claim(ch.account).unwrap();
        crate::bots::start_test(&app, ch, -3, Pos { x, y: 10.5 }, "Biceps", claim)
            .await
            .unwrap();
    }
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let tick = app.world.in_world(-3).call(|w| Ok(w.tick)).await.unwrap();
        if tick >= 8 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "knowledge scene did not reach first outcome tick; tick={tick}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    app.world.clock.set_paused(true);
    let equipped = app
        .world
        .in_world(-3)
        .call(|w| {
            Ok(w.players
                .values()
                .map(|p| {
                    (
                        p.character.account,
                        p.character.items[0],
                        p.character.revision,
                    )
                })
                .collect::<Vec<_>>())
        })
        .await
        .unwrap();
    assert_eq!(equipped.iter().find(|p| p.0 == first.id).unwrap().1, 0xa9a);
    assert_eq!(equipped.iter().find(|p| p.0 == second.id).unwrap().1, 0xa97);
    let unknown = app.store.bot_mind(second.id).await.unwrap().unwrap();
    assert!(
        unknown
            .history
            .iter()
            .any(|h| h.reason.starts_with("inspect_item_"))
    );
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let tick = app.world.in_world(-3).call(|w| Ok(w.tick)).await.unwrap();
        if tick >= 24 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "knowledge inspection did not finish; tick={tick}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    app.world.clock.set_paused(true);
    community_pursuit_stop(&app).await;
    let saved = Store::open(&dir.join("game.db"), &app.content).unwrap();
    for account in [first.id, second.id] {
        let mind = saved.bot_mind(account).await.unwrap().unwrap();
        assert!(
            mind.facts
                .iter()
                .any(|f| f.kind == 0xa9a && f.assessments == 2)
        );
        std::fs::write(
            dir.join(format!("community-knowledge-{account}.json")),
            serde_json::to_vec_pretty(&mind).unwrap(),
        )
        .unwrap();
    }
    println!("community selected knowledge evidence: {}", dir.display());
}

async fn community_pursuit_crossing_pair(risk: bool, loaded: bool) {
    community_pursuit_logging();
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let first = app
        .store
        .list(format!("community-crossing-{risk}-{loaded}-a"))
        .await
        .unwrap();
    for n in 0..3 {
        app.store
            .list(format!("community-crossing-padding-{n}"))
            .await
            .unwrap();
    }
    let second = app
        .store
        .list(format!("community-crossing-{risk}-{loaded}-b"))
        .await
        .unwrap();
    assert_eq!(first.id % 4, second.id % 4);
    let map = Arc::new(crate::content::Map {
        width: 56,
        height: 36,
        cells: vec![
            crate::content::Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            56 * 36
        ],
        spawns: vec![(10.5, 10.5)],
    });
    let content = app.content.clone();
    app.world
        .directory(move |d| {
            let mut w = crate::world::World::new(content, map)?;
            w.name = "Realm".into();
            d.worlds.insert(-3, w);
            Ok(())
        })
        .await
        .unwrap();
    let mut emitters = Vec::new();
    for (account, x, contrast) in [(first.id, 10.5, false), (second.id, 38.5, true)] {
        let mut ch = app
            .store
            .create(account, app.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        if loaded {
            ch.items[5] = 0xa9a;
            ch.revision = app
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap()[0];
        }
        let mut mind = crate::bots::pursuits::Mind::new(account);
        if risk {
            mind.cognition.pickup_clearance = if contrast { 0.1 } else { 1.2 };
        } else {
            mind.cognition.attention = if contrast { 2 } else { 8 };
        }
        app.store.save_bot_mind(mind).await.unwrap();
        let owner = app
            .world
            .in_world(-3)
            .call(move |w| {
                let owner = w.spawn_entity(
                    0x205,
                    Pos {
                        x: x + 10.,
                        y: 18.5,
                    },
                    vec![],
                )?;
                let bag = w.spawn_entity(
                    0x500,
                    Pos {
                        x: x + if risk { 1. } else { 5. },
                        y: 10.5,
                    },
                    vec![if risk { 0xa22 } else { 0xa9b }, -1, -1, -1, -1, -1, -1, -1],
                )?;
                w.entities
                    .get_mut(&bag)
                    .unwrap()
                    .extra_stats
                    .push((54, account));
                Ok(owner)
            })
            .await
            .unwrap();
        emitters.push((owner, x));
        let claim = app.claim(ch.account).unwrap();
        crate::bots::start_test(&app, ch, -3, Pos { x, y: 10.5 }, "Biceps", claim)
            .await
            .unwrap();
    }
    app.world
        .in_world(-3)
        .call(move |w| {
            w.step();
            let spec = w.content.get(0x205)?.projectiles[&0].clone();
            assert_eq!((spec.speed, spec.max), (50., 10));
            for (owner, x) in emitters {
                for id in 0..2 {
                    let pos = Pos {
                        x: x + 2.5,
                        y: 10.5
                            + if id == 0 {
                                6.
                            } else if risk {
                                0.4
                            } else {
                                0.
                            },
                    };
                    w.bullets.push(crate::world::Bullet {
                        owner,
                        source_kind: 0x205,
                        id,
                        friendly: false,
                        start: pos,
                        pos,
                        angle: std::f32::consts::PI,
                        born: w.now,
                        damage: spec.max,
                        spec: spec.clone(),
                        hit: Default::default(),
                    });
                }
            }
            Ok(())
        })
        .await
        .unwrap();
    let mut views = Vec::new();
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let (tick, observed) = app
            .world
            .directory(|d| {
                Ok((
                    d.worlds[&-3].tick,
                    d.worlds
                        .values()
                        .flat_map(|w| w.players.keys())
                        .map(|id| serde_json::to_value(d.player_view(*id).unwrap()).unwrap())
                        .collect::<Vec<_>>(),
                ))
            })
            .await
            .unwrap();
        views.extend(observed);
        if tick >= 22 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "crossing scene did not advance; tick={tick}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    app.world.clock.set_paused(true);
    let outcomes = app
        .world
        .directory(|d| {
            Ok(d.worlds
                .iter()
                .flat_map(|(world, w)| {
                    w.players
                        .values()
                        .map(move |p| (*world, p.character.account, p.character.hp, p.pos))
                })
                .collect::<Vec<_>>())
        })
        .await
        .unwrap();
    community_pursuit_stop(&app).await;
    std::fs::write(
        dir.join("community-crossing-views.json"),
        serde_json::to_vec_pretty(&views).unwrap(),
    )
    .unwrap();
    println!(
        "community crossing risk={risk} loaded={loaded} evidence: {}",
        dir.display()
    );
    let a = outcomes.iter().find(|p| p.1 == first.id).unwrap();
    let b = outcomes.iter().find(|p| p.1 == second.id).unwrap();
    if risk {
        assert_eq!(
            a.0, -2,
            "cautious account did not commit earlier Nexus escape"
        );
        assert_eq!(a.2, 100);
        assert!(
            b.2 < 100,
            "exposed accepted pickup did not incur the declared damage"
        );
    } else if loaded {
        assert_eq!(a.2, 100, "broad attention did not take a safe path");
        assert!(
            b.2 < 100,
            "occupied narrow attention did not incur the declared hit"
        );
    } else {
        assert_eq!(
            (a.2, b.2),
            (100, 100),
            "narrow attention should handle the unloaded cue set"
        );
    }
}
#[tokio::test]
async fn community_pursuit_attention_actual_controllers_register_different_paths_and_damage() {
    community_pursuit_crossing_pair(false, true).await;
}
#[tokio::test]
async fn community_pursuit_risk_actual_controllers_commit_early_escape_or_exposed_pickup_damage() {
    community_pursuit_crossing_pair(true, false).await;
}
#[tokio::test]
async fn community_pursuit_narrow_attention_without_inspection_load_handles_crossing_cues() {
    community_pursuit_crossing_pair(false, false).await;
}

async fn community_pursuit_execution_pair(simple: bool) {
    community_pursuit_logging();
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let first = app
        .store
        .list(format!("community-execution-{simple}-a"))
        .await
        .unwrap();
    for n in 0..3 {
        app.store
            .list(format!("community-execution-padding-{n}"))
            .await
            .unwrap();
    }
    let second = app
        .store
        .list(format!("community-execution-{simple}-b"))
        .await
        .unwrap();
    assert_eq!(first.id % 4, second.id % 4);
    let map = Arc::new(crate::content::Map {
        width: 60,
        height: 36,
        cells: vec![
            crate::content::Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            60 * 36
        ],
        spawns: vec![(10.5, 10.5)],
    });
    let content = app.content.clone();
    app.world
        .directory(move |d| {
            let mut w = crate::world::World::new(content, map)?;
            w.name = "Realm".into();
            d.worlds.insert(-3, w);
            Ok(())
        })
        .await
        .unwrap();
    let mut targets = Vec::new();
    for (account, x, coarse) in [(first.id, 10.5, false), (second.id, 40.5, true)] {
        let mut ch = app
            .store
            .create(account, app.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.items[1] = -1;
        ch.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        let mut mind = crate::bots::pursuits::Mind::new(account);
        if coarse {
            mind.cognition.velocity_step = 4.;
            mind.cognition.directions = 4;
        }
        app.store.save_bot_mind(mind).await.unwrap();
        let owner = app
            .world
            .in_world(-3)
            .call(move |w| {
                w.spawn_entity(
                    0x205,
                    Pos {
                        x: x + if simple { 4. } else { 5. },
                        y: 10.5 + if simple { 0. } else { 1.5 },
                    },
                    vec![],
                )
            })
            .await
            .unwrap();
        targets.push((owner, x));
        let claim = app.claim(ch.account).unwrap();
        crate::bots::start_test(&app, ch, -3, Pos { x, y: 10.5 }, "Biceps", claim)
            .await
            .unwrap();
    }
    app.world
        .in_world(-3)
        .call(move |w| {
            w.step();
            let spec = w.content.get(0x205)?.projectiles[&0].clone();
            if !simple {
                for (owner, x) in targets {
                    for id in 0..4 {
                        let angle = id as f32 * std::f32::consts::FRAC_PI_2;
                        let pos = Pos {
                            x: x + 2.75 * angle.cos(),
                            y: 10.5 + 2.75 * angle.sin(),
                        };
                        w.bullets.push(crate::world::Bullet {
                            owner,
                            source_kind: 0x205,
                            id,
                            friendly: false,
                            start: pos,
                            pos,
                            angle: angle + std::f32::consts::PI,
                            born: w.now,
                            damage: spec.max,
                            spec: spec.clone(),
                            hit: Default::default(),
                        });
                    }
                }
            }
            Ok(())
        })
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let mut views = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let (tick, observed) = app
            .world
            .directory(|d| {
                Ok((
                    d.worlds[&-3].tick,
                    d.worlds[&-3]
                        .players
                        .keys()
                        .map(|id| serde_json::to_value(d.player_view(*id).unwrap()).unwrap())
                        .collect::<Vec<_>>(),
                ))
            })
            .await
            .unwrap();
        views.extend(observed);
        if tick >= 15 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "execution scene did not advance; tick={tick}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    app.world.clock.set_paused(true);
    let outcomes = app
        .world
        .in_world(-3)
        .call(|w| {
            Ok(w.players
                .values()
                .map(|p| p.character.clone())
                .collect::<Vec<_>>())
        })
        .await
        .unwrap();
    community_pursuit_stop(&app).await;
    std::fs::write(
        dir.join("community-execution-views.json"),
        serde_json::to_vec_pretty(&views).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("community-execution-outcomes.json"),
        serde_json::to_vec_pretty(&outcomes).unwrap(),
    )
    .unwrap();
    println!(
        "community execution simple={simple} evidence: {}",
        dir.display()
    );
    let a = outcomes.iter().find(|p| p.account == first.id).unwrap();
    let b = outcomes.iter().find(|p| p.account == second.id).unwrap();
    assert!(
        a.hits > 0 && a.hp == 100,
        "practiced execution did not hit and pass safely: {a:?}"
    );
    if simple {
        assert!(
            b.hits > 0 && b.hp == 100,
            "coarser execution failed the simple shot/path: {b:?}"
        );
    } else {
        assert!(
            b.shots > 0 && b.hits == 0 && b.hp < 100,
            "coarser aim/path did not produce the declared miss and hit: {b:?}"
        );
    }
}
#[tokio::test]
async fn community_pursuit_execution_actual_controllers_hit_and_pass_or_miss_and_take_damage() {
    community_pursuit_execution_pair(false).await;
}
#[tokio::test]
async fn community_pursuit_weaker_execution_still_completes_a_simple_shot_and_path() {
    community_pursuit_execution_pair(true).await;
}

#[tokio::test]
async fn community_pursuit_actual_return_reobserves_expired_bag_without_inventing_supply() {
    community_pursuit_logging();
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let map = Arc::new(crate::content::Map {
        width: 320,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            320 * 32
        ],
        spawns: vec![(250.5, 10.5)],
    });
    let content = app.content.clone();
    app.world
        .directory(move |d| {
            let mut w = crate::world::World::new(content, map)?;
            w.name = "Realm".into();
            d.worlds.insert(-3, w);
            Ok(())
        })
        .await
        .unwrap();
    let account = app
        .store
        .list("community-expired-supply".into())
        .await
        .unwrap();
    let ch = app
        .store
        .create(account.id, app.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let (ch, pos) = community_pursuit_prepare_supply(&app, ch).await;
    let character = ch.id;
    app.world
        .in_world(-3)
        .call(|w| {
            Arc::make_mut(&mut w.map).spawns = vec![(10.5, 10.5)];
            Ok(())
        })
        .await
        .unwrap();
    // Initial pickup is local; the ordinary return needs over 50s walking plus the Vault trip, beyond the real 60s bag lifetime.
    let claim = app.claim(ch.account).unwrap();
    crate::bots::start_test(&app, ch, -3, pos, "Biceps", claim)
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(160);
    let mut views = Vec::new();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        views.extend(
            app.world
                .directory(|d| {
                    Ok(d.worlds
                        .values()
                        .flat_map(|w| w.players.keys())
                        .map(|id| serde_json::to_value(d.player_view(*id).unwrap()).unwrap())
                        .collect::<Vec<_>>())
                })
                .await
                .unwrap(),
        );
        let mind = app.store.bot_mind(account.id).await.unwrap().unwrap();
        if mind
            .history
            .iter()
            .any(|h| h.reason == "observed_opportunity_missing_or_expired")
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expired-bag return did not terminate; evidence {}",
            dir.display()
        );
    }
    community_pursuit_stop(&app).await;
    let mind = app.store.bot_mind(account.id).await.unwrap().unwrap();
    assert!(
        !mind
            .history
            .iter()
            .any(|h| h.reason == "reserve_carried_satisfied")
    );
    assert_eq!(mind.history.back().unwrap().deficit, 1);
    let reopened = Store::open(&dir.join("game.db"), &app.content).unwrap();
    assert_eq!(
        reopened
            .load(account.id, character)
            .await
            .unwrap()
            .items
            .iter()
            .filter(|k| **k == 0xa22)
            .count(),
        1
    );
    assert!(
        reopened
            .vault(account.id, 0)
            .await
            .unwrap()
            .items
            .iter()
            .any(|k| *k >= 0)
    );
    std::fs::write(
        dir.join("community-expired-views.json"),
        serde_json::to_vec(&views).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("community-expired-mind.json"),
        serde_json::to_vec_pretty(&mind).unwrap(),
    )
    .unwrap();
    println!("community expired opportunity evidence: {}", dir.display());
}
