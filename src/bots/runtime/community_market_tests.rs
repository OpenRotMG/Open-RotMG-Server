use super::*;
use crate::{content::WIZARD, store::InventoryWrite, world::World};
use serde_json::json;
use std::{fs, time::Instant};

async fn progress(app: &App) {
    for (id, ch) in app.world.collect(World::pending_progress).await.unwrap() {
        app.store.progress(ch.clone()).await.unwrap();
        app.world
            .for_player(id)
            .call(move |w| {
                w.committed(id, Ok(()))?;
                w.finish_progress(id, ch)
            })
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn community_market_actual_controllers_negotiate_private_terms_bank_and_commit_installments()
{
    controller_market_case("private").await;
}
#[tokio::test]
async fn community_market_actual_guild_members_bargain_and_nonmember_cannot_uptake_guild_offer() {
    controller_market_case("guild").await;
}
#[tokio::test]
async fn community_market_actual_changed_interest_counteroffers_then_refuses_incompatible_goods() {
    controller_market_case("incompatible").await;
}
#[tokio::test]
async fn community_market_actual_partial_delivery_restart_and_partner_departure_resolves_remaining_balance_without_repaying()
 {
    controller_market_case("interrupted").await;
}
#[tokio::test]
async fn community_market_actual_guild_advertisements_from_unobserved_identity_cannot_bind_consent()
{
    controller_market_case("guild-unobserved").await;
}
async fn controller_market_case(mode: &str) {
    let interrupted = mode == "interrupted";
    let unobserved = mode == "guild-unobserved";
    let guild = mode.starts_with("guild");
    let count = if guild && !unobserved { 3 } else { 2 };
    let incompatible = mode == "incompatible";
    let (dir, app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    let log = fs::File::create(dir.join("market-runtime.jsonl")).unwrap();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(log))
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let mut initial = Vec::new();
    let mut selected = Vec::new();
    if guild {
        for (i, profile) in PROFILES[..count].iter().enumerate() {
            let mut prior = app
                .store
                .shore_character(
                    profile.identity.into(),
                    profile.name.into(),
                    &app.content,
                    false,
                )
                .await
                .unwrap();
            if i < 2 {
                prior.fame = if i == 0 { 1000 } else { 20 };
                prior.hp = 0;
                prior.alive = false;
                app.store
                    .death(prior, "initial prior-life guild eligibility fixture".into())
                    .await
                    .unwrap();
            }
        }
        let (id, _) = app
            .store
            .change_guild(1, crate::store::GuildAction::Create("Market Circle".into()))
            .await
            .unwrap();
        app.store
            .change_guild(
                2,
                crate::store::GuildAction::Join {
                    guild: id,
                    inviter: 1,
                },
            )
            .await
            .unwrap();
    }
    let initial_ad_deadline = incompatible.then(|| pursuits::market::now() + 30);
    let mut initial_cadence = Vec::new();
    for (i, profile) in PROFILES[..count].iter().enumerate() {
        let mut ch = app
            .store
            .shore_character(
                profile.identity.into(),
                profile.name.into(),
                &app.content,
                false,
            )
            .await
            .unwrap();
        ch.level = 20;
        ch.exp = crate::store::level_start(20).unwrap();
        ch.base = app.content.classes[&WIZARD].caps;
        if i < 2 {
            ch.base[if i == 0 {
                if incompatible { 2 } else { 5 }
            } else {
                4
            }] -= 10;
        }
        // The nonmember has no exchange spare; membership alone determines its guild audience.
        ch.hp = ch.base[0];
        ch.mp = ch.base[1];
        let spare = if i == 0 { 0xa21 } else { 0xa4c };
        ch.items[4..].copy_from_slice(&[0xa22, 0xa22, 0xa23, 0xa23, spare, spare, spare, spare]);
        let mut vault = app.store.vault(ch.account, 0).await.unwrap();
        if i < 2 {
            vault.items[..6].fill(spare);
        } else {
            ch.items[8..].fill(-1);
        }
        ch.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![vault],
            )
            .await
            .unwrap()[0];
        app.store.progress(ch.clone()).await.unwrap();
        // Both own Vaults are initially inspected by their actual controllers, then ordinary travel publishes them.
        let loaded = app.store.vault(ch.account, 0).await.unwrap();
        let (scope, pos) = app
            .world
            .directory(move |d| {
                if unobserved {
                    let w = &d.worlds[&-2];
                    let near = Pos {
                        x: w.map.spawns[0].0,
                        y: w.map.spawns[0].1,
                    };
                    let pos = if i == 0 {
                        near
                    } else {
                        (0..w.map.height)
                            .flat_map(|y| {
                                (0..w.map.width).map(move |x| Pos {
                                    x: x as f32 + 0.5,
                                    y: y as f32 + 0.5,
                                })
                            })
                            .find(|p| p.distance(near) > 40. && w.walkable(*p))
                            .context("initial distant walkable Nexus position unavailable")?
                    };
                    return Ok((-2, pos));
                }
                let scope = d.instance("vault", Some(loaded.account))?;
                let w = d.worlds.get_mut(&scope).unwrap();
                w.load_vaults(vec![loaded])?;
                Ok((
                    scope,
                    w.entities.values().find(|e| e.kind == 0x504).unwrap().pos,
                ))
            })
            .await
            .unwrap();
        if incompatible && i == 1 {
            // Concurrent advertisements are a different valid refusal path, not a counteroffer precondition.
            let mut mind = pursuits::Mind::new(ch.account);
            mind.cognition.pickup_clearance = 0.8;
            let mut raw = serde_json::to_value(mind).unwrap();
            raw["market"]["next_ad"] = json!(initial_ad_deadline.unwrap());
            let mind: pursuits::Mind = serde_json::from_value(raw).unwrap();
            mind.validate().unwrap();
            app.store.save_bot_mind(mind).await.unwrap();
        }
        initial.push(json!({"character":ch,"vault":app.store.vault(ch.account, 0).await.unwrap()}));
        selected.push((ch.account, ch.id));
        launch(&app, *profile, ch.clone(), scope, Some(pos), Some(ch.id))
            .await
            .unwrap();
        if incompatible {
            let account = ch.account;
            let view = app
                .world
                .directory(move |d| {
                    let id = d
                        .worlds
                        .values()
                        .flat_map(|w| w.players.iter())
                        .find(|(_, p)| p.character.account == account)
                        .context("initial market controller missing")?
                        .0;
                    d.player_view(*id)
                })
                .await
                .unwrap();
            let mind = app.store.bot_mind(account).await.unwrap().unwrap();
            assert_eq!(mind.cognition, Brain::new(*profile, &view).mind.cognition);
            let raw = serde_json::to_value(&mind).unwrap();
            if i == 1 {
                assert_eq!(raw["market"]["next_ad"], initial_ad_deadline.unwrap());
                assert!(initial_ad_deadline.unwrap() > pursuits::market::now());
            }
            assert!(mind.market.bargain.is_none() && mind.social.episodes.is_empty());
            assert_eq!(view.elapsed_ms, 0);
            initial_cadence.push(json!({"profile":profile.name,"reaction":profile.reaction,"post_enter_mind":mind,"view":view}));
        }
    }
    if incompatible {
        fs::write(dir.join("market-initial-cadence.json"), serde_json::to_vec_pretty(&json!({"clock":"existing persisted next_ad Unix seconds; initial own cadence only","second_account_ad_deadline":initial_ad_deadline,"offset_seconds":30,"post_enter":initial_cadence})).unwrap()).unwrap();
    }
    fs::write(dir.join("market-declaration.json"),serde_json::to_vec_pretty(&json!({"initial":initial,"max_game_ms":180000,"max_wall_ms":240000,"humans":0,"qa":0,"autonomous":count,"guild":guild,"mode":mode,"setup":"initial own earned-capable stats and held goods only; private Vault inspection, genuine selected runtime loops; no supplied agreement, action, text or outcome"})).unwrap()).unwrap();
    app.world.clock.set_paused(false);
    let start = Instant::now();
    let mut sequence = Vec::new();
    let settled;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        progress(&app).await;
        let accounts = selected.iter().map(|(a, _)| *a).collect::<Vec<_>>();
        let sample=app.world.directory(move|d|{let views=d.worlds.values().flat_map(|w|w.players.iter()).filter(|(_,p)|accounts.contains(&p.character.account)).map(|(id,_)|{let v=d.player_view(*id)?;Ok(json!({"session":v.session,"position":v.position,"map":v.map,"character":v.character,"lifecycle":v.lifecycle,"peers":v.objects.iter().filter(|p|p.account.is_some()).collect::<Vec<_>>()}))}).collect::<Result<Vec<_>>>()?;Ok(json!({"game_ms":d.tick*50,"views":views}))}).await.unwrap();
        let game = sample["game_ms"].as_u64().unwrap();
        use std::io::Write;
        let mut retained = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("market-sequence.jsonl"))
            .unwrap();
        writeln!(retained, "{}", sample).unwrap();
        sequence.push(sample);
        let mut minds = Vec::new();
        for (a, _) in &selected {
            minds.push(app.store.bot_mind(*a).await.unwrap().unwrap());
        }
        let complete = minds[..2].iter().all(|m| {
            if unobserved {
                return game >= 3000 && m.market.bargain.is_none() && m.market.outgoing.is_none();
            }
            m.market.bargain.as_ref().is_some_and(|b| {
                if interrupted {
                    b.confirmed >= 1
                } else {
                    b.resolution.as_ref().is_some_and(|s| {
                        if incompatible {
                            !s.starts_with("settled")
                        } else {
                            s.starts_with("settled")
                        }
                    })
                }
            }) && m.market.outgoing.is_none()
        });
        if complete
            || game >= 180000
            || start.elapsed() > Duration::from_secs(240)
            || !app.quarantined.lock().unwrap().is_empty()
        {
            settled = complete;
            break;
        }
    }
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !app.owners.lock().unwrap().is_empty() {
        progress(&app).await;
        assert!(
            Instant::now() < deadline,
            "cleanup timeout {}",
            dir.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut final_minds = Vec::new();
    for (a, _) in &selected {
        final_minds.push(app.store.bot_mind(*a).await.unwrap().unwrap());
    }
    fs::write(
        dir.join("market-sequence.json"),
        serde_json::to_vec_pretty(&sequence).unwrap(),
    )
    .unwrap();
    fs::write(dir.join("market-result.json"),serde_json::to_vec_pretty(&json!({"objective_met":settled,"settled":settled && !incompatible && !interrupted && !unobserved,"mode":mode,"minds":final_minds,"owners":app.owners.lock().unwrap().len(),"quarantined":app.quarantined.lock().unwrap().len()})).unwrap()).unwrap();
    println!("market actual controller evidence: {}", dir.display());
    assert!(
        app.quarantined.lock().unwrap().is_empty(),
        "quarantine {}",
        dir.display()
    );
    assert!(
        settled,
        "market controller objective absent {}",
        dir.display()
    );
    if unobserved {
        assert!(final_minds.iter().all(|m| m.market.bargain.is_none()));
        let logs = fs::read_to_string(dir.join("market-runtime.jsonl")).unwrap();
        let messages = logs
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .collect::<Vec<_>>();
        for account in [selected[0].0, selected[1].0] {
            assert!(messages.iter().any(|r| r["fields"]["account"] == account
                && r["fields"]["recipient"] == "*Guild*"
                && r["fields"]["observed"] == "None"));
        }
        return;
    }
    if guild {
        assert!(
            final_minds[2].market.bargain.is_none(),
            "nonmember must not receive the guild bargain"
        );
    }
    if interrupted {
        resume_without_partner(&app, &dir, selected[0]).await;
        return;
    }
    for m in final_minds.into_iter().take(2) {
        let b = m.market.bargain.unwrap();
        if incompatible {
            assert_eq!(b.confirmed, 0);
        } else {
            assert!(b.confirmed >= 2);
        }
        let receipts = app
            .store
            .market_receipts(m.account, b.terms.version)
            .await
            .unwrap();
        assert_eq!(receipts.len(), b.confirmed as usize);
    }
    let logs = fs::read_to_string(dir.join("market-runtime.jsonl")).unwrap();
    assert!(
        logs.lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .any(|r| r["fields"]["recipient"] == if guild { "*Guild*" } else { "Triceps" })
    );
    if incompatible {
        assert!(logs.contains(", counter ") && logs.contains("incompatible wants"));
        assert!(
            !logs
                .lines()
                .any(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["level"] == "ERROR")
        );
        let db = rusqlite::Connection::open_with_flags(
            dir.join("game.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM trade_transactions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert!(sequence.iter().any(|s| {
        s["views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["map"]["name"] == "Nexus")
    }));
}

async fn resume_without_partner(original: &App, dir: &std::path::Path, selected: (i32, i32)) {
    let initial = original.store.bot_mind(selected.0).await.unwrap().unwrap();
    let terms = initial.market.bargain.as_ref().unwrap().terms.clone();
    let receipts = original
        .store
        .market_receipts(selected.0, terms.version)
        .await
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert!(
        initial
            .market
            .bargain
            .as_ref()
            .unwrap()
            .resolution
            .is_none()
    );
    let (restart_dir, mut app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    app.store = crate::store::Store::open(&dir.join("game.db"), &app.content).unwrap();
    let ch = app.store.load(selected.0, selected.1).await.unwrap();
    fs::write(dir.join("restart-declaration.json"),serde_json::to_vec_pretty(&json!({"trigger":"prospectively declared outage after first durable installment; all previous owners released before restart","returning_account":selected.0,"unavailable_peer":terms.accounts.iter().find(|a|**a!=selected.0),"initial":initial,"unchanged_expiry":terms.expires,"max_game_ms":180000,"max_wall_ms":240000,"humans":0,"qa":0,"autonomous":1,"new_world_state":restart_dir})).unwrap()).unwrap();
    launch(&app, PROFILES[0], ch, -3, None, Some(selected.1))
        .await
        .unwrap();
    let rebound = app.store.bot_mind(selected.0).await.unwrap().unwrap();
    assert_eq!(rebound.market.bargain.as_ref().unwrap().confirmed, 1);
    assert!(!rebound.market.bargain.as_ref().unwrap().own_consent);
    assert!(
        rebound
            .market
            .bargain
            .as_ref()
            .unwrap()
            .peer_consent
            .is_none()
    );
    app.world.clock.set_paused(false);
    let start = Instant::now();
    let resolved;
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        progress(&app).await;
        let mind = app.store.bot_mind(selected.0).await.unwrap().unwrap();
        let game = app.world.directory(|d| Ok(d.tick * 50)).await.unwrap();
        let b = mind.market.bargain.as_ref().unwrap();
        fs::write(
            dir.join("restart-latest.json"),
            serde_json::to_vec_pretty(&json!({"game_ms":game,"mind":mind})).unwrap(),
        )
        .unwrap();
        if b.resolution.is_some() || game >= 180000 || start.elapsed() > Duration::from_secs(240) {
            resolved = b.resolution.clone();
            break;
        }
    }
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !app.owners.lock().unwrap().is_empty() {
        progress(&app).await;
        assert!(
            Instant::now() < deadline,
            "restart cleanup {}",
            dir.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let after = app
        .store
        .market_receipts(selected.0, terms.version)
        .await
        .unwrap();
    fs::write(dir.join("restart-result.json"),serde_json::to_vec_pretty(&json!({"before":receipts,"after":after,"resolution":resolved,"owners":0,"quarantined":app.quarantined.lock().unwrap().len()})).unwrap()).unwrap();
    assert_eq!(after, receipts);
    assert!(app.quarantined.lock().unwrap().is_empty());
    assert!(
        resolved
            .as_ref()
            .is_some_and(|r| r.contains("expired_remaining_balance")),
        "missing explicit default resolution {}",
        dir.display()
    );
}
