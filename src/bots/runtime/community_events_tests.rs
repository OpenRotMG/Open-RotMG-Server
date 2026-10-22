use super::*;
use crate::{content::WIZARD, store::InventoryWrite, world::World};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::{fs, time::Instant};

#[path = "community_events_aftermath_tests.rs"]
mod aftermath;
#[path = "community_events_castle_tests.rs"]
mod castle;

fn guarded_exit(events: &[Value], account: i32) -> bool {
    events.iter().enumerate().rev().any(|(i, f)| {
        f["message"] == "autonomous event action selected"
            && f["account"] == account
            && f["journey"] == "Some(Nexus)"
            && f["held_health"] == 0
            && f["hp"].as_f64().is_some_and(|hp| {
                hp < f["recovery"].as_f64().unwrap() * f["max_hp"].as_f64().unwrap()
            })
            && events[i + 1..].iter().any(|after| {
                after["message"] == "autonomous durable transfer completed"
                    && after["account"] == account
                    && after["character"] == f["character"]
                    && after["destination"] == -2
                    && after["portal"] == "None"
            })
    })
}
fn readiness_exit(events: &[Value], account: i32) -> bool {
    events.iter().enumerate().any(|(i, g)| {
        if g["message"] != "autonomous next event declined; own readiness spent"
            || g["account"] != account
            || g["usable"] != true
            || ![json!(0xd7b), json!(0x700)].contains(&g["kind"])
            || g["ready"] != false
            || g["pending"] != false
            || g["opening_active"] != false
            || g["ability_pending"] != false
            || g["remedy_available"] != false
        {
            return false;
        }
        let hp_short = g["hp"].as_f64().unwrap() * 4. < g["max_hp"].as_f64().unwrap() * 3.;
        let mp_short = g["mp"].as_f64().unwrap() * 3. < g["max_mp"].as_f64().unwrap();
        let health = g["held_health"].as_u64().unwrap();
        let flags = g["conditions"].as_i64().unwrap() as i32;
        (hp_short || mp_short && health < 2)
            && (!hp_short
                || health == 0
                || flags & crate::condition::bit(crate::condition::SICK) != 0)
            && (!mp_short
                || g["held_magic"] == 0
                || flags & crate::condition::bit(crate::condition::QUIET) != 0)
            && events[i + 1..].iter().any(|a| {
                a["message"] == "autonomous event action selected"
                    && a["account"] == account
                    && a["character"] == g["character"]
                    && a["world"] == g["world"]
                    && a["tick"] == g["tick"]
                    && a["hp"] == g["hp"]
                    && a["mp"] == g["mp"]
                    && a["held_health"] == g["held_health"]
                    && a["goal"] == "Recover"
                    && a["journey"] == "Some(Nexus)"
            })
            && events[i + 1..].iter().any(|a| {
                a["message"] == "autonomous durable transfer completed"
                    && a["account"] == account
                    && a["character"] == g["character"]
                    && a["destination"] == -2
                    && a["portal"] == "None"
            })
    })
}
fn remedy_attempts(events: &[Value], account: i32) -> bool {
    let observed: Vec<_> = events
        .iter()
        .filter(|f| {
            f["message"] == "autonomous event safety observation"
                && f["account"] == account
                && f["blue"].is_number()
        })
        .collect();
    let progressed = observed.windows(2).any(|p| {
        p[0]["blue"] == p[1]["blue"]
            && p[1]["blue_distance"].as_f64().unwrap() + 0.2
                < p[0]["blue_distance"].as_f64().unwrap()
    });
    progressed
        && !observed.is_empty()
        && observed.iter().all(|f| {
            f["retreat"] == true
                || f["conditions"]
                    .as_i64()
                    .is_some_and(|flags| flags & i64::from(crate::content::PARALYZED) != 0)
                || events.iter().any(|a| {
                    a["message"] == "autonomous event action selected"
                        && a["account"] == account
                        && a["tick"] == f["tick"]
                        && a["goal"].as_str().is_some_and(|g| g.starts_with("Remedy("))
                        && a["movement"] != "None"
                })
        })
}
fn permanent_death(dir: &std::path::Path, character: i32) -> bool {
    let db = rusqlite::Connection::open_with_flags(
        dir.join("game.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let state:Option<(i32,String)>=db.query_row("SELECT alive,items FROM characters WHERE id=? AND EXISTS(SELECT 1 FROM deaths WHERE character=?)",[character,character],|r|Ok((r.get(0)?,r.get(1)?))).optional().unwrap();
    state.is_some_and(|(alive, items)| {
        alive == 0
            && serde_json::from_str::<Vec<i32>>(&items)
                .unwrap()
                .iter()
                .all(|k| *k == -1)
    })
}
#[test]
fn community_events_readiness_decline_observer_rejects_unused_stock_wrong_view_and_pending() {
    let guard = json!({"message":"autonomous next event declined; own readiness spent","account":1,"character":2,"world":"Oryx's Castle","tick":8,"hp":543,"max_hp":810,"mp":68,"max_mp":440,"held_health":0,"held_magic":0,"conditions":0,"kind":0xd7b,"usable":true,"ready":false,"pending":false,"opening_active":false,"ability_pending":false,"remedy_available":false});
    let selected = json!({"message":"autonomous event action selected","account":1,"character":2,"world":"Oryx's Castle","tick":8,"hp":543,"mp":68,"held_health":0,"goal":"Recover","journey":"Some(Nexus)"});
    let departed = json!({"message":"autonomous durable transfer completed","account":1,"character":2,"destination":-2,"portal":"None"});
    assert!(readiness_exit(
        &[guard.clone(), selected.clone(), departed.clone()],
        1
    ));
    let mut g = guard.clone();
    let mut a = selected.clone();
    g["held_health"] = json!(4);
    a["held_health"] = json!(4);
    assert!(!readiness_exit(&[g, a, departed.clone()], 1));
    for field in [
        "pending",
        "ability_pending",
        "opening_active",
        "remedy_available",
    ] {
        let mut g = guard.clone();
        g[field] = json!(true);
        assert!(!readiness_exit(&[g, selected.clone(), departed.clone()], 1));
    }
    let mut wrong = selected.clone();
    wrong["hp"] = json!(540);
    assert!(!readiness_exit(
        &[guard.clone(), wrong, departed.clone()],
        1
    ));
    let mut wrong = guard.clone();
    wrong["message"] = json!("a different abort reason");
    assert!(!readiness_exit(&[wrong, selected.clone(), departed], 1));
    assert!(!readiness_exit(&[guard, selected], 1));
}
#[test]
fn community_events_terminal_observer_rejects_healthy_noop_cleanup_and_transfer_gaps() {
    let selected = json!({"message":"autonomous event action selected","account":1,"character":2,"tick":8,"hp":10,"max_hp":100,"recovery":0.5,"held_health":0,"journey":"Some(Nexus)","goal":"Remedy(7)","movement":"Some(Pos)"});
    let departed = json!({"message":"autonomous durable transfer completed","account":1,"character":2,"destination":-2,"portal":"None"});
    assert!(guarded_exit(&[selected.clone(), departed.clone()], 1));
    assert!(!guarded_exit(std::slice::from_ref(&selected), 1));
    assert!(!guarded_exit(
        &[
            json!({"message":"player left","account":1}),
            departed.clone()
        ],
        1
    ));
    let mut healthy = selected.clone();
    healthy["hp"] = json!(90);
    assert!(!guarded_exit(&[healthy, departed.clone()], 1));
    let mut supplied = selected.clone();
    supplied["held_health"] = json!(1);
    assert!(!guarded_exit(&[supplied, departed], 1));
    let safety = |tick, distance| json!({"message":"autonomous event safety observation","account":1,"tick":tick,"blue":7,"blue_distance":distance,"retreat":false});
    let mut a = selected.clone();
    a["tick"] = json!(9);
    assert!(remedy_attempts(
        &[safety(8, 5.), selected.clone(), safety(9, 4.), a.clone()],
        1
    ));
    a["goal"] = json!("Dodge");
    assert!(!remedy_attempts(
        &[safety(8, 5.), selected.clone(), safety(9, 4.), a],
        1
    ));
    assert!(!remedy_attempts(
        &[safety(8, 5.), selected.clone(), safety(9, 5.), selected],
        1
    ));
}

async fn commits(app: &App) {
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
    for (id, ch, killer) in app.world.collect(World::pending_deaths).await.unwrap() {
        assert!(
            !ch.items
                .iter()
                .take(4)
                .any(|k| *k >= 0 && app.content.objects[&(*k as u16)].resurrects)
        );
        let result = app.store.death(ch, killer).await;
        app.world
            .for_player(id)
            .call(move |w| {
                w.committed(id, result)?;
                w.finish_death(id)
            })
            .await
            .unwrap();
    }
}
async fn cleanup(app: &App) {
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !app.owners.lock().unwrap().is_empty() {
        commits(app).await;
        assert!(Instant::now() < deadline, "event fixture cleanup deadline");
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
}

#[tokio::test]
async fn community_events_actual_lab_controllers_remedy_or_guarded_escape_with_independent_rewards()
{
    let mut results = vec![];
    for last_ready in [true, false] {
        let (dir, mut app) = crate::server::fixture();
        app.world = crate::world::Handle::spawn(
            crate::world::Directory::test_prepared(
                std::path::Path::new("data"),
                app.content.clone(),
                16,
                12351,
            )
            .unwrap(),
        );
        app.world.clock.set_paused(true);
        let log = fs::File::create(dir.join("events-runtime.jsonl")).unwrap();
        let sub = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_writer(Mutex::new(log))
            .finish();
        let _logging = tracing::subscriber::set_default(sub);
        let world = app
            .world
            .directory(|d| {
                let world = d.instance("mad-lab", None)?;
                let w = d.worlds.get_mut(&world).unwrap();
                let boss = w
                    .entities
                    .values_mut()
                    .find(|e| e.kind == crate::dungeon::lab::BOSS)
                    .unwrap();
                boss.hp = 5000;
                Ok(world)
            })
            .await
            .unwrap();
        let mut actors = vec![];
        let mut initial = vec![];
        for (i, p) in [PROFILES[0], PROFILES[1], PROFILES[2]]
            .into_iter()
            .enumerate()
        {
            let account = app.store.list(format!("event-lab-{i}")).await.unwrap().id;
            let mut ch = app
                .store
                .create(account, app.content.get(WIZARD).unwrap())
                .await
                .unwrap();
            ch.level = if i == 2 && !last_ready { 1 } else { 20 };
            ch.exp = crate::store::level_start(ch.level).unwrap();
            ch.base = [670, 385, 75, 25, 50, 75, 40, 60];
            ch.hp = 670;
            ch.mp = 385;
            ch.items = vec![
                2824, 2852, 2821, 2765, 0xa22, 0xa22, 0xa22, 0xa22, 0xa23, 0xa23, -1, -1,
            ];
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
            app.store.progress(ch.clone()).await.unwrap();
            let mut mind = pursuits::Mind::new(account);
            mind.events.knows_blue = i == 0;
            app.store.save_bot_mind(mind).await.unwrap();
            let pos = [
                Pos { x: 52.5, y: 30.5 },
                Pos { x: 56.5, y: 24.5 },
                Pos { x: 58.5, y: 24.5 },
            ][i];
            initial.push(json!({"character":ch,"position":pos,"knowledge_blue":i==0}));
            actors.push((account, ch.id));
            let selected = ch.id;
            launch(&app, p, ch, world, Some(pos), Some(selected))
                .await
                .unwrap();
        }
        fs::write(dir.join("events-initial.json"),serde_json::to_vec_pretty(&json!({"initial":initial,"context":"PREPARED production generated Lab, all real actors and initially injured Dr Terrible5000HP; own selected max kit. No prewritten outcomes/after-start intervention. Actual controllers and normal authority afterward.","last_ready":last_ready,"intended_stop_simulation_step_ms":45000,"max_simulation_step_ms":46000,"max_active_wall_ms":90000,"humans":0,"bots":3})).unwrap()).unwrap();
        let start_tick = app.world.directory(|d| Ok(d.tick)).await.unwrap();
        let start_clock = app.world.clock.now();
        app.world.clock.set_paused(false);
        let start = Instant::now();
        let mut samples = vec![];
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            commits(&app).await;
            let sample=app.world.directory(|d| {
                let views=d.worlds.values().flat_map(|w|w.players.iter()).map(|(id,p)|Ok(json!({"view":d.player_view(*id)?,"shots":p.character.shots,"kills":p.character.kills,"mad_labs":p.character.mad_labs}))).collect::<Result<Vec<_>>>()?;
                Ok(json!({"game_ms":d.tick*50,"actors":views}))
            }).await.unwrap();
            let ms = sample["game_ms"].as_u64().unwrap();
            samples.push(sample);
            if ms >= 45000 || start.elapsed() > Duration::from_secs(90) {
                break;
            }
        }
        app.world.clock.set_paused(true);
        let end_tick = app.world.directory(|d| Ok(d.tick)).await.unwrap();
        let step_ms = (end_tick - start_tick) * 50;
        let wall_ms = start.elapsed().as_millis();
        let active_clock_ms = app
            .world
            .clock
            .now()
            .saturating_duration_since(start_clock)
            .as_millis();
        let prefix = fs::read_to_string(dir.join("events-runtime.jsonl")).unwrap();
        fs::write(dir.join("events-runtime-before-cleanup.jsonl"), &prefix).unwrap();
        let fields: Vec<Value> = prefix
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap()["fields"].clone())
            .collect();
        fs::write(dir.join("events-observation-boundary.json"),serde_json::to_vec_pretty(&json!({"start_tick":start_tick,"end_tick":end_tick,"simulation_step_ms":step_ms,"shared_clock_active_ms":active_clock_ms,"active_wall_ms":wall_ms,"before_cleanup":true})).unwrap()).unwrap();
        fs::write(
            dir.join("events-sequence.json"),
            serde_json::to_vec(&samples).unwrap(),
        )
        .unwrap();
        let mut minds = vec![];
        for (account, _) in &actors {
            minds.push(app.store.bot_mind(*account).await.unwrap());
        }
        fs::write(
            dir.join("events-minds-before-cleanup.json"),
            serde_json::to_vec_pretty(&minds).unwrap(),
        )
        .unwrap();
        cleanup(&app).await;
        let player = fields
            .iter()
            .find(|f| f["message"] == "autonomous character entered" && f["account"] == actors[0].0)
            .unwrap()["player"]
            .clone();
        let applied = fields.iter().any(|f| {
            f["message"] == "Lab pool source conditions changed"
                && f["player"] == player
                && f["after"].as_i64().unwrap() != 0
        });
        let cured = fields.iter().any(|f| {
            f["message"] == "Lab pool source conditions changed"
                && f["player"] == player
                && f["remedy"] == true
                && f["after"] == 0
        });
        let danger_abandoned = remedy_attempts(&fields, actors[0].0)
            && (guarded_exit(&fields, actors[0].0) || permanent_death(&dir, actors[0].1));
        let clear = samples.iter().any(|s| {
            s["actors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["mad_labs"].as_u64().unwrap() > 0)
        });
        let unready_exited = samples.iter().any(|s| {
            s["actors"].as_array().unwrap().iter().any(|p| {
                p["view"]["session"]["account"] == actors[2].0
                    && p["view"]["map"]["name"] == "Nexus"
            })
        });
        let independent_shots = samples.iter().any(|s| {
            s["actors"].as_array().unwrap().iter().any(|p| {
                p["view"]["session"]["account"] == actors[1].0 && p["shots"].as_u64().unwrap() > 0
            }) && s["actors"].as_array().unwrap().iter().any(|p| {
                p["view"]["session"]["account"] == actors[0].0
                    && p["view"]["character"]["conditions"].as_i64().unwrap()
                        & i64::from(crate::condition::bit(crate::condition::HEXED))
                        != 0
            })
        });
        println!("event Lab original evidence: {}", dir.display());
        assert!(
            step_ms <= 46000 && wall_ms <= 90000,
            "declared active budgets exceeded: {}",
            dir.display()
        );
        assert!(
            applied,
            "no actual green pool source applied: {}",
            dir.display()
        );
        assert!(
            cured || danger_abandoned,
            "known blue activity neither remedied nor causally abandoned: {}",
            dir.display()
        );
        assert!(
            fields
                .iter()
                .any(|f| f["message"] == "autonomous inventory outcome"
                    && f["actions"]
                        .as_str()
                        .is_some_and(|a| a.contains("Use { slot:"))
                    && f["revision"].as_i64().unwrap() > f["before_revision"].as_i64().unwrap()),
            "no real committed item use: {}",
            dir.display()
        );
        assert!(
            independent_shots,
            "other actor synchronized with hexed participant: {}",
            dir.display()
        );
        assert!(clear, "no damage-earned Lab completion: {}", dir.display());
        if !last_ready {
            assert!(
                unready_exited
                    && samples
                        .iter()
                        .any(|s| s["game_ms"].as_u64().unwrap() <= 5000
                            && s["actors"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|p| p["view"]["session"]["account"] == actors[2].0
                                    && p["view"]["map"]["name"] == "Nexus"))
                    && samples.iter().all(|s| s["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|p| p["view"]["session"]["account"] != actors[2].0
                            || p["mad_labs"] == 0)),
                "unready participant did not decline before personal clear: {}",
                dir.display()
            );
        }
        let result = json!({"last_ready":last_ready,"directory":dir,"cured":cured,"danger_abandoned":danger_abandoned,"positive_remedy_observed":cured,"clear":clear,"unready_exited":unready_exited,"independent_shots":independent_shots});
        fs::write(
            dir.join("events-result.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        results.push(result);
    }
    println!("event counterfactuals: {}", json!(results));
}
