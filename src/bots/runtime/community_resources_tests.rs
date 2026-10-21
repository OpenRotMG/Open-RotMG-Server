use super::*;
use crate::{content::WIZARD, store::InventoryWrite, world::World};
use serde_json::{Value, json};
use std::{fs, time::Instant};

fn transfer_destination(logs: &[Value], account: i32, character: i32) -> Result<i32> {
    ensure!(
        !logs.iter().any(|l| l["level"] == "ERROR"),
        "resource controller error during transfer"
    );
    let mut destination = None;
    for log in logs {
        let f = &log["fields"];
        if f["account"] != account || f["character"] != character {
            continue;
        }
        match f["message"].as_str() {
            Some("transfer grant issued") => {
                destination = f["destination"].as_i64().map(|v| v as i32)
            }
            Some("transfer grant consumed") => ensure!(
                destination == f["destination"].as_i64().map(|v| v as i32),
                "resource consumed grant has no matching issue"
            ),
            Some("autonomous durable transfer completed") => destination = None,
            _ => {}
        }
    }
    destination.context("resource actor absent without an authoritative in-flight transfer")
}
fn gap_guard(owned: bool, alive: bool, quarantined: bool, age: Duration) -> Result<()> {
    ensure!(
        owned && alive && !quarantined,
        "resource transfer controller lost or quarantined"
    );
    ensure!(
        age <= Duration::from_secs(5),
        "resource transfer gap exceeded declared five seconds"
    );
    Ok(())
}
fn read_logs(dir: &std::path::Path) -> Vec<Value> {
    fs::read_to_string(dir.join("resources-runtime.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn community_resources_transfer_gap_observer_rejects_loss_quarantine_timeout_and_unproven_absence()
 {
    let issued = json!({"level":"INFO","fields":{"message":"transfer grant issued","account":1,"character":7,"destination":-2}});
    let consumed = json!({"level":"INFO","fields":{"message":"transfer grant consumed","account":1,"character":7,"destination":-2}});
    assert_eq!(
        transfer_destination(&[issued.clone(), consumed.clone()], 1, 7).unwrap(),
        -2
    );
    assert!(transfer_destination(&[], 1, 7).is_err());
    assert!(transfer_destination(std::slice::from_ref(&issued), 1, 8).is_err());
    assert!(transfer_destination(&[consumed], 1, 7).is_err());
    let stopped = json!({"level":"ERROR","fields":{"message":"autonomous controller stopped; account retained for diagnosis"}});
    assert!(transfer_destination(&[issued, stopped], 1, 7).is_err());
    for (owned, alive, quarantined, ms) in [
        (false, true, false, 0),
        (true, false, false, 0),
        (true, true, true, 0),
        (true, true, false, 5001),
    ] {
        assert!(gap_guard(owned, alive, quarantined, Duration::from_millis(ms)).is_err());
    }
    let (dir, app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    let log = fs::File::create(dir.join("resources-runtime.jsonl")).unwrap();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(Mutex::new(log))
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let mut ch = app
        .store
        .shore_character(
            PROFILES[0].identity.into(),
            PROFILES[0].name.into(),
            &app.content,
            false,
        )
        .await
        .unwrap();
    ch.items[0] = -1;
    ch.revision = app
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            None,
        )
        .await
        .unwrap()[0];
    app.store.progress(ch.clone()).await.unwrap();
    let (account, character) = (ch.account, ch.id);
    launch(&app, PROFILES[0], ch, -2, None, Some(character))
        .await
        .unwrap();
    app.world.clock.set_paused(false);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.quarantined.lock().unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "empty-weapon controller did not fail within declared bound"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let owned = app.owners.lock().unwrap().contains_key(&account);
    let alive = app.store.load(account, character).await.is_ok();
    let quarantined = !app.quarantined.lock().unwrap().is_empty();
    assert!(
        owned && alive && quarantined,
        "real failed controller retains its diagnostic owner, not a legal transfer gap"
    );
    assert!(gap_guard(owned, alive, quarantined, Duration::ZERO).is_err());
    assert!(transfer_destination(&read_logs(&dir), account, character).is_err());
    fs::write(dir.join("resource-controller-loss-result.json"), serde_json::to_vec_pretty(&json!({"initial_context":"ordinary selected runtime in Nexus with no equipped weapon, no after-start intervention","account":account,"character":character,"owned":owned,"alive":alive,"quarantined":quarantined,"classification":"rejected_not_transfer_gap","max_wall_ms":5000})).unwrap()).unwrap();
    println!(
        "resource genuine controller-loss evidence: {}",
        dir.display()
    );
    let owners = app.owners.clone();
    drop(app);
    assert!(
        owners.lock().unwrap().is_empty(),
        "negative fixture release after complete diagnostic capture"
    );
}

#[tokio::test]
async fn community_resources_actual_controllers_withdraw_store_and_continue_or_keep_unsatisfied_stock()
 {
    for label in ["available", "unavailable", "full"] {
        let (dir, app) = crate::server::fixture();
        app.world.clock.set_paused(true);
        let log = fs::File::create(dir.join("resources-runtime.jsonl")).unwrap();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_writer(Mutex::new(log))
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        let account = app
            .store
            .list(format!("resources-{label}"))
            .await
            .unwrap()
            .id;
        let mut ch = app
            .store
            .create(account, app.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.base[4] = app.content.classes[&WIZARD].caps[4];
        ch.items[4..].copy_from_slice(&[0xa3d, 0xa21, 0xa23, 0xa23, 0xa23, 0xa23, 0xa23, 0xa23]);
        ch.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                None,
            )
            .await
            .unwrap()[0];
        app.store.progress(ch.clone()).await.unwrap();
        let mut vault = app.store.vault(account).await.unwrap();
        match label {
            "available" => vault.items[..3].copy_from_slice(&[0xa22, 0xa22, 0xa9a]),
            "unavailable" => {}
            "full" => vault.items.fill(0xa21),
            _ => unreachable!(),
        }
        ch.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                Some(vault.clone()),
            )
            .await
            .unwrap()[0];
        let initial = json!({"character":ch,"vault":app.store.vault(account).await.unwrap()});
        let loaded = app.store.vault(account).await.unwrap();
        let (scope, position) = app
            .world
            .directory(move |d| {
                let realm = d.worlds.get_mut(&-3).unwrap();
                for (x, y) in realm.map.spawns.clone() {
                    realm.spawn_entity(0x205, Pos { x, y }, vec![])?;
                }
                let scope = d.instance("vault", Some(account))?;
                let w = d.worlds.get_mut(&scope).unwrap();
                w.load_vault(loaded)?;
                Ok((
                    scope,
                    w.entities.values().find(|e| e.kind == 0x504).unwrap().pos,
                ))
            })
            .await
            .unwrap();
        let character = ch.id;
        launch(
            &app,
            PROFILES[0],
            ch,
            scope,
            Some(position),
            Some(character),
        )
        .await
        .unwrap();
        let declaration = json!({"label":label,"initial":initial,"kind":"initial context only, actual selected runtime controller, ordinary private Vault/World/Store; zero Humans/Qa", "max_game_ms":60000,"minimum_game_ms":32000,"max_wall_ms":120000,"max_transfer_gap_wall_ms":5000,"encounter":"ordinary Pink Blob at each existing Realm spawn before controller start","future_class":crate::content::PRIEST,"locked_future_class":true});
        fs::write(
            dir.join("resources-declaration.json"),
            serde_json::to_vec_pretty(&declaration).unwrap(),
        )
        .unwrap();
        app.world.clock.set_paused(false);
        let start = Instant::now();
        let mut sequence = Vec::new();
        let mut gap: Option<(Instant, i32)> = None;
        let mut reentries = Vec::new();
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            finish_progress(&app).await;
            let sample = app
                .world
                .directory(move |d| {
                    let id = d
                        .worlds
                        .values()
                        .flat_map(|w| w.players.iter())
                        .find(|(_, p)| p.character.id == character)
                        .map(|(id, _)| *id);
                    if let Some(id) = id {
                        let view = d.player_view(id)?;
                        let own = &d.worlds[&view.session.world].players[&id].character;
                        Ok(json!({"game_ms":d.tick*50,"shots":own.shots,"kills":own.kills,"view":view}))
                    } else {
                        Ok(json!({"game_ms":d.tick*50,"event":"GAP","account":account,"character":character,"view":null}))
                    }
                })
                .await
                .unwrap();
            let elapsed = sample["game_ms"].as_u64().unwrap();
            if sample["view"].is_null() {
                let destination =
                    transfer_destination(&read_logs(&dir), account, character).unwrap();
                let (since, expected) = *gap.get_or_insert((Instant::now(), destination));
                assert_eq!(destination, expected);
                let owned = app.owners.lock().unwrap().contains_key(&account);
                let alive = app.store.load(account, character).await.is_ok();
                gap_guard(
                    owned,
                    alive,
                    !app.quarantined.lock().unwrap().is_empty(),
                    since.elapsed(),
                )
                .unwrap();
            } else if let Some((since, destination)) = gap.take() {
                let owned = app.owners.lock().unwrap().contains_key(&account);
                let alive = app.store.load(account, character).await.is_ok();
                gap_guard(
                    owned,
                    alive,
                    !app.quarantined.lock().unwrap().is_empty(),
                    since.elapsed(),
                )
                .unwrap();
                assert_eq!(sample["view"]["session"]["account"], account);
                assert_eq!(sample["view"]["session"]["character"], character);
                assert_eq!(sample["view"]["session"]["world"], destination);
                assert_eq!(sample["view"]["lifecycle"]["alive"], true);
                assert_eq!(sample["view"]["lifecycle"]["quarantined"], false);
                reentries.push(destination);
            }
            let used = !sample["view"].is_null()
                && sample["shots"].as_u64().unwrap() > 0
                && sample["kills"].as_u64().unwrap() > 0;
            sequence.push(sample);
            if (elapsed >= 32000 && used)
                || elapsed >= 60000
                || start.elapsed() > Duration::from_secs(120)
            {
                break;
            }
        }
        assert!(
            gap.is_none(),
            "resource controller never reentered within original budgets"
        );
        let transfer_logs = read_logs(&dir);
        assert!(!transfer_logs.iter().any(|l| l["level"] == "ERROR"));
        for destination in reentries {
            for message in [
                "transfer grant consumed",
                "autonomous durable transfer completed",
            ] {
                assert!(
                    transfer_logs
                        .iter()
                        .any(|l| l["fields"]["message"] == message
                            && l["fields"]["account"] == account
                            && l["fields"]["character"] == character
                            && l["fields"]["destination"] == destination)
                );
            }
        }
        app.world.clock.set_paused(true);
        app.shutdown.send_replace(true);
        let deadline = Instant::now() + Duration::from_secs(15);
        while !app.owners.lock().unwrap().is_empty() {
            finish_progress(&app).await;
            assert!(
                Instant::now() < deadline,
                "resource cleanup timed out: {}",
                dir.display()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(app.quarantined.lock().unwrap().is_empty());
        assert_eq!(
            app.world
                .collect(|w| vec![w.players.len()])
                .await
                .unwrap()
                .into_iter()
                .sum::<usize>(),
            0
        );
        let durable = app.store.load(account, character).await.unwrap();
        let banked = app.store.vault(account).await.unwrap();
        fs::write(
            dir.join("resources-sequence.json"),
            serde_json::to_vec_pretty(&sequence).unwrap(),
        )
        .unwrap();
        fs::write(dir.join("resources-result.json"),serde_json::to_vec_pretty(&json!({"label":label,"durable":durable,"vault":banked,"cleanup":{"owners":0,"players":0,"quarantined":0}})).unwrap()).unwrap();
        println!("resource controller evidence: {}", dir.display());
        assert!(sequence.last().unwrap()["game_ms"].as_u64().unwrap() >= 32000);
        assert!(sequence.iter().any(|s| s["view"]["map"]["name"] == "Nexus"));
        assert!(
            sequence.iter().any(|s| s["view"]["map"]["name"] == "Realm"),
            "no next outing: {}",
            dir.display()
        );
        assert!(
            durable.tiles > 0,
            "no ordinary movement after organization: {}",
            dir.display()
        );
        assert!(
            !app.content
                .unlocked(
                    crate::content::PRIEST,
                    &app.store
                        .list(format!("resources-{label}"))
                        .await
                        .unwrap()
                        .best_levels
                )
                .unwrap()
        );
        assert!(
            app.store
                .create(account, app.content.get(crate::content::PRIEST).unwrap())
                .await
                .is_err()
        );
        let departure_items = &sequence
            .iter()
            .find(|s| s["view"]["map"]["name"] == "Nexus")
            .unwrap()["view"]["character"]["items"];
        let stock = |items: &Value| -> Vec<i32> {
            items
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_i64().unwrap() as i32)
                .filter(|k| *k >= 0)
                .collect()
        };
        let mut before = stock(&initial["character"]["items"]);
        before.extend(stock(&initial["vault"]["items"]));
        before.sort();
        let mut after = stock(departure_items);
        after.extend(banked.items.iter().copied().filter(|k| *k >= 0));
        after.sort();
        assert_eq!(
            before,
            after,
            "private transfers must conserve every item: {}",
            dir.display()
        );
        assert!(
            durable.shots > 0 && durable.kills > 0,
            "changed holdings were not subsequently used: {}",
            dir.display()
        );
        if label == "available" {
            assert_eq!(
                departure_items[0],
                0xa9a,
                "upgrade not used: {}",
                dir.display()
            );
            assert!(departure_items.as_array().unwrap().contains(&json!(0xa22)));
            assert!(banked.items.contains(&0xa3d));
            assert!(banked.items.contains(&0xa21));
            assert!(durable.revision > initial["character"]["revision"].as_i64().unwrap() as i32);
        } else {
            assert_eq!(departure_items[0], 0xa97);
            assert!(
                !departure_items.as_array().unwrap().contains(&json!(0xa22)),
                "unavailable private stock must not be fabricated; later earned Realm drops are separate"
            );
            assert_eq!(
                departure_items.as_array().unwrap().contains(&json!(0xa3d)),
                label == "full"
            );
            assert_eq!(banked.items.contains(&0xa3d), label == "unavailable");
        }
        let logs: Vec<Value> = fs::read_to_string(dir.join("resources-runtime.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(logs.iter().any(|l|l["fields"]["message"]=="autonomous private organization finished; continuing outing with retained holdings"));
    }
}

async fn finish_progress(app: &App) {
    for (id, pending) in app.world.collect(World::pending_progress).await.unwrap() {
        app.store.progress(pending.clone()).await.unwrap();
        app.world
            .for_player(id)
            .call(move |w| {
                w.committed(id, Ok(()))?;
                w.finish_progress(id, pending)
            })
            .await
            .unwrap();
    }
}
