use super::*;
use crate::{
    content::{Cell, Map, WIZARD},
    store::InventoryWrite,
    world::World,
};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Instant};

#[tokio::test]
async fn community_crowd_actual_controllers_knowledge_attention_and_world_travel_are_counterfactuals()
 {
    let mut contexts = Vec::new();
    for (label, identity, attention, world) in [
        ("noticed", PROFILES[3].identity, 8, "Realm"),
        ("unknown_minimap", PROFILES[2].identity, 8, "Realm"),
        ("narrow_attention", PROFILES[3].identity, 1, "Realm"),
        ("prohibited_travel", PROFILES[3].identity, 8, "Spider Den"),
    ] {
        contexts.push(context(label, identity, attention, world).await);
    }
    for other in &contexts[1..] {
        assert_eq!(other["profiles"], contexts[0]["profiles"]);
        assert_eq!(other["bindings"], contexts[0]["bindings"]);
        assert_eq!(
            other["initial_characters"],
            contexts[0]["initial_characters"]
        );
        assert_eq!(other["positions"], contexts[0]["positions"]);
    }
    assert_eq!(
        contexts[0]["post_enter_policy_minds"],
        contexts[1]["post_enter_policy_minds"]
    );
    assert_eq!(
        contexts[0]["post_enter_policy_minds"],
        contexts[3]["post_enter_policy_minds"]
    );
    let mut narrow = contexts[2]["post_enter_policy_minds"].clone();
    narrow[0]["cognition"]["attention"] = json!(8);
    assert_eq!(narrow, contexts[0]["post_enter_policy_minds"]);
    println!("crowd counterfactual contexts: {}", json!(contexts));
}

async fn context(
    label: &str,
    identity: &'static str,
    attention: usize,
    world: &'static str,
) -> Value {
    let (dir, app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    let log = fs::File::create(dir.join("crowd-runtime.jsonl")).unwrap();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(log))
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let positions = [
        Pos { x: 10.5, y: 14.5 },
        Pos { x: 55.5, y: 8.5 },
        Pos { x: 55.5, y: 20.5 },
    ];
    let map = Arc::new(Map {
        width: 80,
        height: 36,
        cells: vec![
            Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            80 * 36
        ],
        spawns: vec![(10.5, 14.5)],
    });
    let content = app.content.clone();
    app.world
        .directory(move |d| {
            assert_eq!(d.tick, 0);
            let mut w = World::new(content, map)?;
            w.bind_fixture_clock(&d.clock);
            w.name = world.into();
            for y in [5.5, 23.5] {
                w.spawn_entity(0x205, Pos { x: 59.5, y }, vec![])?;
            }
            w.spawn_entity(0x654, Pos { x: 10.5, y: 33.5 }, vec![])?;
            d.worlds.insert(-3, w);
            Ok(())
        })
        .await
        .unwrap();
    let mut profiles = Vec::new();
    let mut characters = Vec::new();
    let mut bindings = Vec::new();
    let mut post_enter = Vec::new();
    for (i, base) in [PROFILES[3], PROFILES[1], PROFILES[5]]
        .into_iter()
        .enumerate()
    {
        let account = app
            .store
            .list(format!("crowd-factor-{i}"))
            .await
            .unwrap()
            .id;
        assert_eq!(account, i as i32 + 1);
        let mut ch = app
            .store
            .create(account, app.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.level = 8;
        ch.exp = crate::store::level_start(8).unwrap();
        ch.base = [280, 180, 22, 0, 14, 25, 16, 20];
        ch.hp = 280;
        ch.mp = 180;
        ch.items = vec![2714, 2606, 2653, -1, 0xa22, 0xa22, -1, -1, -1, -1, -1, -1];
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
        if i == 0 {
            mind.cognition.attention = attention;
        }
        let expected = mind.cognition.clone();
        app.store.save_bot_mind(mind).await.unwrap();
        let profile = Profile {
            identity: if i == 0 { identity } else { base.identity },
            ..base
        };
        // Identity selects minimap familiarity; persisted cognition replaces all identity-derived estimates on entry.
        profiles.push(
            json!({"name":profile.name,"reaction":profile.reaction,"recovery":profile.recovery,
            "resources":profile.resources,"companions":profile.companions,"healer":profile.healer}),
        );
        characters.push(
            json!({"account":account,"character":ch.id,"class":ch.class,"level":ch.level,
            "stats":ch.base,"hp":ch.hp,"mp":ch.mp,"items":ch.items,"revision":ch.revision}),
        );
        let character = ch.id;
        launch(&app, profile, ch, -3, Some(positions[i]), Some(character))
            .await
            .unwrap();
        let entered = app.store.bot_mind(account).await.unwrap().unwrap();
        assert_eq!(entered.cognition, expected);
        assert!(entered.facts.is_empty());
        assert_eq!(
            entered.history.back().unwrap().reason,
            "account_session_bound"
        );
        post_enter.push(entered);
        let view = app
            .world
            .directory(move |d| {
                let id = d.worlds[&-3]
                    .players
                    .iter()
                    .find(|(_, p)| p.character.id == character)
                    .unwrap()
                    .0;
                d.player_view(*id)
            })
            .await
            .unwrap();
        bindings.push(json!(view.session));
    }
    let ids: Vec<i32> = bindings
        .iter()
        .map(|b| b["player"].as_i64().unwrap() as i32)
        .collect();
    let initial_ids = ids.clone();
    let initial = app
        .world
        .directory(move |d| {
            let w = d.worlds.get_mut(&-3).unwrap();
            for id in &initial_ids {
                w.updates(*id, false)?;
            }
            initial_ids
                .into_iter()
                .map(|id| d.player_view(id))
                .collect::<Result<Vec<_>>>()
        })
        .await
        .unwrap();
    assert!(initial.iter().all(|v| v.tick == 0 && v.elapsed_ms == 0));
    assert_eq!(initial[0].player_markers.len(), 2);
    assert!(initial[0].objects.iter().all(|o| o.account.is_none()));
    assert_eq!(initial[0].objects.len(), 1);
    assert_eq!(initial[0].objects[0].kind, 0x654);
    let allowed = initial[0].map.allow_teleport;
    assert_eq!(allowed, world == "Realm");
    let mut policy_minds = json!(post_enter);
    for mind in policy_minds.as_array_mut().unwrap() {
        for event in mind["history"].as_array_mut().unwrap() {
            // Session nonces identify evidence, not cognitive state or decision timing.
            event["session"] = Value::Null;
        }
    }
    let mut declaration = json!({"label":label,"kind":"prepared initial context; three actual selected runtime controllers, no Humans",
        "max_game_ms":3000,"goal_game_ms":2000,"max_wall_ms":120000,"profiles":profiles,"minimap_identity":identity,
        "post_enter_cognition":post_enter.iter().map(|m| &m.cognition).collect::<Vec<_>>(),
        "post_enter_minds":post_enter,"post_enter_policy_minds":policy_minds,"initial_characters":characters,"bindings":bindings,"positions":positions,
        "world":world,"allow_teleport":allowed,"initial_views":initial,
        "competing_attention":"same ordinary visible Medusa at19 tiles in every context; narrow attention is occupied by the encounter, not a random idle delay",
        "limits":"context only; no post-start mutation, takeover, forced follow/teleport or Brain::decide calls"});
    save(&dir, "crowd-declaration.json", &declaration);
    app.world.clock.set_paused(false);
    let began = Instant::now();
    let mut sequence = Vec::new();
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snapshot_ids = ids.clone();
        let snapshot =
            app.world
                .directory(move |d| {
                    let actors = snapshot_ids.into_iter().map(|id| {
                let view = d.player_view(id)?;
                let ch = &d.worlds[&view.session.world].players[&id].character;
                Ok(json!({"view":view,"shots":ch.shots,"kills":ch.kills,"tiles":ch.tiles}))
            }).collect::<Result<Vec<_>>>()?;
                    Ok(json!({"game_ms":d.tick * 50,"actors":actors}))
                })
                .await
                .unwrap();
        let elapsed = snapshot["game_ms"].as_u64().unwrap();
        sequence.push(snapshot);
        save(&dir, "crowd-sequence.json", &json!(sequence));
        if elapsed >= 2000 || began.elapsed() >= Duration::from_secs(120) {
            break;
        }
        progress(&app).await;
    }
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let deadline = Instant::now() + Duration::from_secs(15);
    while !app.owners.lock().unwrap().is_empty() {
        progress(&app).await;
        assert!(
            Instant::now() < deadline,
            "counterfactual cleanup timeout: {}",
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
    assert!(
        app.world
            .collect(World::pending_progress)
            .await
            .unwrap()
            .is_empty()
    );
    save(
        &dir,
        "crowd-cleanup.json",
        &json!({"owners":0,"players":0,"pending_progress":0,"quarantined":0}),
    );
    let mut durable = Vec::new();
    for b in &bindings {
        durable.push(
            app.store
                .load(
                    b["account"].as_i64().unwrap() as i32,
                    b["character"].as_i64().unwrap() as i32,
                )
                .await
                .unwrap(),
        );
    }
    save(&dir, "crowd-durable.json", &json!(durable));
    let logs: Vec<Value> = fs::read_to_string(dir.join("crowd-runtime.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let noticed: BTreeSet<i32> = logs
        .iter()
        .filter(|e| {
            e["fields"]["message"] == "autonomous crowd cue noticed" && e["fields"]["account"] == 1
        })
        .map(|e| e["fields"]["peer"].as_i64().unwrap() as i32)
        .collect();
    let attempted = logs.iter().any(|e| {
        e["fields"]["message"] == "autonomous attended crowd travel" && e["fields"]["account"] == 1
    });
    let teleport_receipts: Vec<_> = logs
        .iter()
        .filter(|e| e["fields"]["message"] == "player teleported" && e["fields"]["account"] == 1)
        .collect();
    let decisions: Vec<_> = logs
        .iter()
        .filter(|e| e["fields"]["message"] == "autonomous decision" && e["fields"]["account"] == 1)
        .collect();
    let final_state = &sequence.last().unwrap()["actors"];
    let initial_pos = positions[0];
    let traveled = sequence.iter().any(|s| {
        let p = &s["actors"][0]["view"]["position"];
        initial_pos.distance(Pos {
            x: p["x"].as_f64().unwrap() as f32,
            y: p["y"].as_f64().unwrap() as f32,
        }) > 30.
    });
    let summary = json!({"label":label,"evidence":dir,"noticed_peers":noticed,"attempted_travel":attempted,"actual_remote_travel":traveled,
        "decisions":decisions,"teleport_receipts":teleport_receipts,"game_ms":sequence.last().unwrap()["game_ms"],"samples":sequence.len(),"final_actors":final_state});
    save(&dir, "crowd-summary.json", &summary);
    println!(
        "crowd counterfactual evidence: {}",
        json!({"label":label,"evidence":dir,"noticed":noticed,"attempted":attempted,"traveled":traveled})
    );
    assert!(
        sequence.last().unwrap()["game_ms"].as_u64().unwrap() >= 2000
            && sequence.last().unwrap()["game_ms"].as_u64().unwrap() <= 3000
    );
    assert!(
        !decisions.is_empty(),
        "no actual seeker decisions: {}",
        dir.display()
    );
    assert_eq!(
        noticed.len(),
        if label == "unknown_minimap" || attention == 1 {
            0
        } else {
            2
        },
        "{}",
        dir.display()
    );
    assert_eq!(attempted, label == "noticed", "{}", dir.display());
    assert_eq!(traveled, label == "noticed", "{}", dir.display());
    assert_eq!(
        teleport_receipts.len(),
        usize::from(label == "noticed"),
        "{}",
        dir.display()
    );
    assert!(
        durable[1..].iter().all(|c| c.shots > 0 && c.kills > 0),
        "continuing peers did not clear independently: {}",
        dir.display()
    );
    for (i, ch) in durable.iter().enumerate() {
        assert!(ch.alive);
        assert!(ch.shots >= final_state[i]["shots"].as_u64().unwrap());
        assert!(ch.tiles >= final_state[i]["tiles"].as_u64().unwrap());
    }
    declaration["evidence"] = json!(dir);
    declaration.as_object_mut().unwrap().remove("initial_views");
    declaration
        .as_object_mut()
        .unwrap()
        .remove("post_enter_minds");
    declaration
}

fn save(dir: &Path, name: &str, value: &Value) {
    fs::write(dir.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

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
