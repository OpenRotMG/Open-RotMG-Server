use super::*;
use crate::{
    content::{Cell, Map, WIZARD},
    world::World,
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::test]
async fn community_crowd_three_actual_controllers_recover_continue_and_clear_independently() {
    let (dir, app) = fixture();
    app.world.clock.set_paused(true);
    let positions = [
        Pos { x: 20.5, y: 18.5 },
        Pos { x: 27.5, y: 18.5 },
        Pos { x: 10.5, y: 18.5 },
    ];
    let prey_positions = [Pos { x: 31.5, y: 18.5 }, Pos { x: 6.5, y: 18.5 }];
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
        spawns: vec![(positions[0].x, positions[0].y)],
    });
    let content = app.content.clone();
    let opponents = app
        .world
        .directory(move |d| {
            assert_eq!(d.tick, 0);
            assert!(d.clock.paused());
            assert!(d.worlds[&-3].players.is_empty());
            assert!(d.worlds[&-3].players.is_empty());
            // Ambient loot can legitimately preempt combat, so this prepared opportunity starts without it.
            let mut w = World::new(content, map)?;
            w.bind_fixture_clock(&d.clock);
            w.name = "Realm".into();
            assert!(w.entities.is_empty());
            let mut opponents = vec![];
            for (actor, prey) in positions[1..].iter().zip(prey_positions) {
                assert!((3. ..5.).contains(&actor.distance(prey)));
                assert!((0..=32).all(|step| {
                    let f = step as f32 / 32.;
                    w.walkable(Pos {
                        x: actor.x + (prey.x - actor.x) * f,
                        y: actor.y + (prey.y - actor.y) * f,
                    })
                }));
                opponents.push((w.spawn_entity(0x205, prey, vec![])?, prey));
            }
            assert_eq!(w.entities.len(), 2);
            d.worlds.insert(-3, w);
            Ok(opponents)
        })
        .await
        .unwrap();
    let profiles = ["Biceps", "BattleCrus", "Pixl"];
    let mut bindings = vec![];
    let mut initial_characters = vec![];
    for (i, profile) in profiles.into_iter().enumerate() {
        let spawned = request(&app, &format!("crowd-spawn-{i}"), json!({"op":"spawn","spec":{"guid":format!("crowd-independent-{i}"),"class":WIZARD,"level":8,"seed":42+i,"equipment":[2714,2606,2653,-1],"backpack":[-1,-1,-1,-1,-1,-1,-1,-1],"world":-3,"position":positions[i],"controller":"game_bot","profile":profile,"stats":null}})).await;
        assert_eq!(spawned["status"], "succeeded", "{spawned}");
        let actor = spawned["result"]["actor"].as_i64().unwrap() as i32;
        let initial = app
            .world
            .directory(move |d| {
                let w = d.worlds.get_mut(&-3).unwrap();
                let p = w
                    .players
                    .values_mut()
                    .find(|p| p.character.id == actor)
                    .unwrap();
                if i == 0 {
                    p.character.hp = 1;
                }
                Ok(p.character.clone())
            })
            .await
            .unwrap();
        app.store.progress(initial.clone()).await.unwrap();
        bindings.push((initial.account, initial.id));
        initial_characters.push(initial);
    }
    let initial_bindings = bindings.clone();
    let initial_views = app
        .world
        .directory(move |d| {
            assert_eq!(d.tick, 0);
            assert!(d.clock.paused());
            let w = d.worlds.get_mut(&-3).unwrap();
            assert_eq!(w.players.len(), 3);
            assert_eq!(w.entities.len(), 2);
            let ids = initial_bindings
                .iter()
                .map(|(account, character)| {
                    *w.players
                        .iter()
                        .find(|(_, p)| {
                            p.character.account == *account && p.character.id == *character
                        })
                        .unwrap()
                        .0
                })
                .collect::<Vec<_>>();
            for id in &ids {
                assert!(matches!(
                    w.players[id].controller,
                    crate::world::Controller::Autonomous
                ));
                w.updates(*id, false)?;
            }
            ids.into_iter()
                .map(|id| d.player_view(id))
                .collect::<Result<Vec<_>>>()
        })
        .await
        .unwrap();
    let mut initial_minds = vec![];
    for (account, _) in &bindings {
        initial_minds.push(app.store.bot_mind(*account).await.unwrap().unwrap());
    }
    let mut initial_observation =
        std::fs::File::create_new(dir.join("initial-opportunity-observation.json")).unwrap();
    serde_json::to_writer_pretty(&mut initial_observation, &json!({"preconditions_verified":false,"stage":"actual available own observations before view/mind assertions and unpause","clock_paused":app.world.clock.paused(),"bindings":bindings,"profiles":profiles,"seeds":[42,43,44],"positions":positions,"opponents":opponents,"initial_characters":initial_characters,"initial_views":initial_views,"initial_minds":initial_minds})).unwrap();
    initial_observation.sync_all().unwrap();
    let expected_cognition = [(8, 0.25, 16, 0.8), (2, 2.0, 8, 1.05), (5, 1.0, 8, 0.95)];
    for (i, view) in initial_views.iter().enumerate() {
        assert_eq!((view.session.account, view.session.character), bindings[i]);
        assert_eq!(view.session.world, -3);
        assert_eq!((view.tick, view.time_ms, view.elapsed_ms), (0, 0, 0));
        assert_eq!(view.map.name, "Realm");
        assert_eq!(view.character.class, WIZARD);
        assert_eq!(view.character.level, 8);
        assert!(
            view.lifecycle.alive
                && !view.lifecycle.busy
                && !view.lifecycle.departing
                && !view.lifecycle.death_pending
                && !view.lifecycle.quarantined
        );
        assert_eq!(view.player_markers.len(), 2);
        assert_eq!(
            view.objects.iter().filter(|o| o.account.is_some()).count(),
            2
        );
        assert_eq!(view.character.base_stats, initial_characters[i].base);
        assert_eq!(
            view.character.hp,
            if i == 0 { 1 } else { view.character.stats[0] }
        );
        assert_eq!(
            view.character.items,
            vec![2714, 2606, 2653, -1, -1, -1, -1, -1, -1, -1, -1, -1]
        );
        assert!(view.projectiles.is_empty());
        assert!(
            view.objects
                .iter()
                .all(|o| !o.blocks && !o.contents.contains(&0xa22))
        );
        if i > 0 {
            let prey = view
                .objects
                .iter()
                .find(|o| o.id == opponents[i - 1].0)
                .expect("own live prey must be normally visible before unpause");
            assert_eq!(prey.kind, 0x205);
            assert_eq!(prey.hp, prey.max_hp);
            assert!(!crate::condition::protected(
                prey.condition_mask
                    .expect("normal prey conditions must be published")
            ));
            assert!(prey.hp > 0 && (3. ..5.).contains(&view.position.distance(prey.position)));
            assert!(
                !view.objects.iter().any(|o| o.id == opponents[2 - i].0),
                "peer prey must be outside ordinary object view"
            );
            assert_eq!(
                view.objects.iter().filter(|o| o.account.is_none()).count(),
                1
            );
        }
        let mind = &initial_minds[i];
        let cognition = &mind.cognition;
        assert_eq!(
            (
                cognition.attention,
                cognition.velocity_step,
                cognition.directions,
                cognition.pickup_clearance
            ),
            expected_cognition[i]
        );
        let mind = serde_json::to_value(mind).unwrap();
        assert_eq!(initial_characters[i].name, profiles[i]);
        assert!(mind["facts"].as_array().unwrap().is_empty());
        assert!(mind["social"]["episodes"].as_array().unwrap().is_empty());
        assert!(mind["activity"].is_null());
    }
    assert!(app.world.clock.paused());
    let declaration = json!({"kind":"prepared initial isolated positive opportunity, actual runtime controllers; not ordinary Realm/culture or old matched trajectory","preconditions_verified":true,"bindings":bindings,"profiles":profiles,"seeds":[42,43,44],"initial_characters":initial_characters,"initial_views":initial_views,"initial_minds":initial_minds,"positions":positions,"opponents":opponents,"native_humans":0,"controllers":3,"max_game_ms":15000,"max_wall_ms":120000,"interruption":"first account starts injured without a held remedy; each healthy actor has its own reachable live prey4tiles away, peer prey21tiles away/outside20tile ordinary view; no ambientNPC/healthbag; all initial preconditions before unpause"});
    std::fs::write(
        dir.join("crowd-declaration.json"),
        serde_json::to_vec_pretty(&declaration).unwrap(),
    )
    .unwrap();
    let run_app = app.clone();
    let actor = bindings[1].1;
    let running = tokio::spawn(async move {
        request(&run_app, "crowd-bounded", json!({"op":"run","actor":actor,"goal":{"kind":"time","ms":12000},"max_game_ms":15000,"pauseAfter":true})).await
    });
    let began = Instant::now();
    let mut trace = vec![];
    let mut separated = false;
    let mut shots = [0, 0];
    let mut kills = 0;
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let snapshot=app.world.directory(|d| {
            let actors=d.worlds.iter().flat_map(|(world,w)|w.players.iter().map(move |(id,p)|json!({"world":world,"map":w.name,"player":id,"account":p.character.account,"character":p.character.id,"hp":p.character.hp,"position":p.pos,"shots":p.character.shots,"kills":p.character.kills,"experience":p.character.exp,"controller":format!("{:?}",p.controller)}))).collect::<Vec<_>>();
            Ok(json!({"elapsed_ms":d.tick*50,"actors":actors}))
        }).await.unwrap();
        let actors = snapshot["actors"].as_array().unwrap();
        if let Some(a) = actors.iter().find(|a| a["account"] == bindings[0].0) {
            separated |= a["map"] == "Nexus";
        }
        for (j, (account, _)) in bindings[1..].iter().enumerate() {
            if let Some(a) = actors.iter().find(|a| a["account"] == *account) {
                shots[j] = shots[j].max(a["shots"].as_u64().unwrap());
            }
        }
        kills = kills.max(
            actors
                .iter()
                .filter(|a| {
                    bindings[1..]
                        .iter()
                        .any(|(account, _)| a["account"] == *account)
                })
                .map(|a| a["kills"].as_u64().unwrap())
                .sum(),
        );
        let elapsed = snapshot["elapsed_ms"].as_u64().unwrap();
        trace.push(snapshot);
        std::fs::write(
            dir.join("crowd-controller-sequence.json"),
            serde_json::to_vec_pretty(&trace).unwrap(),
        )
        .unwrap();
        if running.is_finished() || elapsed >= 15000 || began.elapsed() >= Duration::from_secs(120)
        {
            break;
        }
        drain(&app).await;
    }
    if !running.is_finished() {
        request(
            &app,
            "crowd-cancel",
            json!({"op":"cancel","request":"crowd-bounded"}),
        )
        .await;
    }
    let outcome = running.await.unwrap();
    std::fs::write(
        dir.join("crowd-run-outcome.json"),
        serde_json::to_vec_pretty(&outcome).unwrap(),
    )
    .unwrap();
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let cleanup_deadline = Instant::now() + Duration::from_secs(10);
    while !app.owners.lock().unwrap().is_empty() {
        drain(&app).await;
        assert!(
            Instant::now() < cleanup_deadline,
            "controller ownership failed to release; evidence {}",
            dir.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(app.quarantined.lock().unwrap().is_empty());
    assert_eq!(
        app.world
            .directory(|d| Ok(d
                .worlds
                .values()
                .map(|w| w.players.len())
                .collect::<Vec<_>>()))
            .await
            .unwrap()
            .into_iter()
            .sum::<usize>(),
        0,
        "actual departure must remove all players"
    );
    assert!(
        app.world
            .directory(|d| Ok(d
                .worlds
                .values()
                .flat_map(|w| w.players.keys().copied())
                .collect::<Vec<_>>()))
            .await
            .unwrap()
            .is_empty()
    );
    std::fs::write(
        dir.join("crowd-cleanup.json"),
        b"{\"owners\":0,\"players\":0,\"pending_progress\":0,\"quarantined\":0}",
    )
    .unwrap();
    assert_eq!(outcome["result"]["goal_reached"], true, "{outcome}");
    let summary = json!({"separated_recovery":separated,"continuing_shots":shots,"committed_kills":kills,"samples":trace.len(),"evidence":dir});
    std::fs::write(
        dir.join("crowd-summary.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .unwrap();
    println!("crowd controller evidence: {summary}");
    assert!(
        separated,
        "injured actor did not independently recover: {}",
        dir.display()
    );
    assert!(
        shots.iter().all(|n| *n > 0),
        "continuing accounts did not shoot in prepared opportunity (shots={shots:?}): {}",
        dir.display()
    );
    assert!(
        kills >= 2,
        "ordinary spawned enemies did not clear: {}",
        dir.display()
    );
    let mut durable = Vec::new();
    for (account, character) in bindings {
        durable.push(app.store.load(account, character).await.unwrap());
    }
    std::fs::write(
        dir.join("crowd-durable-characters.json"),
        serde_json::to_vec_pretty(&durable).unwrap(),
    )
    .unwrap();
    assert!(
        durable[1..]
            .iter()
            .zip(shots)
            .all(|(ch, shots)| ch.shots >= shots)
    );
    assert!(durable[1..].iter().map(|ch| ch.kills).sum::<u64>() >= kills);
}

async fn drain(app: &App) {
    crate::server::fixture_persistence_tick(app).await.unwrap();
}
