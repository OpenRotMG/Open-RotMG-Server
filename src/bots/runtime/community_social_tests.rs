use super::*;
use crate::{
    content::{Cell, Map},
    store::InventoryWrite,
    world::World,
};
use serde_json::{Value, json};
use std::{fs, time::Instant};

#[path = "community_social_help_tests.rs"]
mod help;
#[path = "community_social_rebuild_tests.rs"]
pub(super) mod rebuild;
#[path = "community_social_rebuild_admission_tests.rs"]
mod rebuild_admission;

async fn commits(app: &App) {
    crate::server::fixture_persistence_tick(app).await.unwrap();
}
async fn stop(app: &App) {
    app.world.clock.set_paused(true);
    app.shutdown.send_replace(true);
    let until = Instant::now() + Duration::from_secs(20);
    while !app.owners.lock().unwrap().is_empty() {
        commits(app).await;
        assert!(Instant::now() < until, "owned controller cleanup deadline");
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
        0
    );
}
async fn prepare_character(app: &App, p: Profile, items: [i32; 8]) -> Character {
    let mut ch = app
        .store
        .shore_character(p.identity.into(), p.name.into(), &app.content, false)
        .await
        .unwrap();
    ch.items[4..].copy_from_slice(&items);
    ch.revision = app
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    app.store.progress(ch.clone()).await.unwrap();
    ch
}
fn map() -> Arc<Map> {
    Arc::new(Map {
        width: 80,
        height: 40,
        cells: vec![
            Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            80 * 40
        ],
        spawns: vec![(10.5, 20.5)],
    })
}

fn observed_normal_vacancy(events: &[Value], portal: i32, waiters: &[(i32, i32)]) -> bool {
    let published = |event: &Value, name: &str| {
        event["fields"]["message"] == "Realm portal updated"
            && event["fields"]["realm"] == -3
            && event["fields"]["name"] == name
    };
    for (vacancy, event) in events.iter().enumerate() {
        let Some(name) = event["fields"]["name"].as_str() else {
            continue;
        };
        let Some(base) = name.strip_suffix("(3/4)") else {
            continue;
        };
        let full = format!("{base}(4/4)");
        if !published(event, name) || !events[..vacancy].iter().any(|e| published(e, &full)) {
            continue;
        }
        let Some(refill) = (vacancy + 1..events.len()).find(|i| published(&events[*i], &full))
        else {
            continue;
        };
        for consumer in vacancy + 1..refill {
            let fields = &events[consumer]["fields"];
            if fields["message"] != "autonomous vacancy observed; reconsidering Realm entry"
                || fields["portal"] != portal
                || fields["name"] != name
            {
                continue;
            }
            for &(account, character) in waiters {
                if fields["account"] != account {
                    continue;
                }
                let mut cursor = consumer;
                let mut admitted = true;
                for message in [
                    "transfer grant issued",
                    "transfer grant consumed",
                    "player entered development world",
                ] {
                    // Issuance reserves the slot, so full publication can precede consumption and entry.
                    let until = if message == "transfer grant issued" {
                        refill
                    } else {
                        events.len()
                    };
                    let next = (cursor + 1..until).find(|i| {
                        let f = &events[*i]["fields"];
                        f["message"] == message
                            && f["character"] == character
                            && if message == "player entered development world" {
                                f["world"] == "Realm"
                            } else {
                                f["account"] == account && f["destination"] == -3
                            }
                    });
                    if let Some(next) = next {
                        cursor = next;
                    } else {
                        admitted = false;
                        break;
                    }
                }
                if admitted
                    && events[cursor + 1..].iter().any(|e| {
                        let f = &e["fields"];
                        f["message"] == "autonomous durable transfer completed"
                            && f["account"] == account
                            && f["character"] == character
                            && f["destination"] == -3
                            && f["portal"] == format!("Some({portal})")
                            && f["portal_kind"] == 0x712
                    })
                {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn community_social_vacancy_observer_rejects_cleanup_wrong_waiter_and_out_of_order_events() {
    let publication =
        |name| json!({"fields":{"message":"Realm portal updated","realm":-3,"name":name}});
    let live = vec![
        publication("Djinn (4/4)"),
        publication("Djinn (3/4)"),
        json!({"fields":{"message":"autonomous vacancy observed; reconsidering Realm entry","account":5,"portal":77,"name":"Djinn (3/4)"}}),
        json!({"fields":{"message":"transfer grant issued","account":5,"character":15,"destination":-3}}),
        json!({"fields":{"message":"transfer grant consumed","account":5,"character":15,"destination":-3}}),
        json!({"fields":{"message":"player entered development world","character":15,"world":"Realm"}}),
        json!({"fields":{"message":"autonomous durable transfer completed","account":5,"character":15,"destination":-3,"portal":"Some(77)","portal_kind":0x712}}),
        publication("Djinn (4/4)"),
    ];
    let waiters = [(5, 15), (6, 16)];
    assert!(observed_normal_vacancy(&live, 77, &waiters));
    assert!(!observed_normal_vacancy(&live[..1], 77, &waiters));
    assert!(!observed_normal_vacancy(
        &[
            publication("Djinn (4/4)"),
            publication("Djinn (3/4)"),
            publication("Djinn (4/4)")
        ],
        77,
        &waiters
    ));
    let mut absent = live.clone();
    absent.remove(2);
    assert!(!observed_normal_vacancy(&absent, 77, &waiters));
    let mut occupant = live.clone();
    occupant[2]["fields"]["account"] = json!(4);
    assert!(!observed_normal_vacancy(&occupant, 77, &waiters));
    let mut foreign_portal = live.clone();
    foreign_portal[2]["fields"]["portal"] = json!(78);
    assert!(!observed_normal_vacancy(&foreign_portal, 77, &waiters));
    let mut reordered = live.clone();
    reordered.swap(1, 2);
    assert!(!observed_normal_vacancy(&reordered, 77, &waiters));
    let mut reserved = live.clone();
    let refill = reserved.remove(7);
    reserved.insert(4, refill);
    assert!(observed_normal_vacancy(&reserved, 77, &waiters));
    let mut late = live.clone();
    late.swap(4, 5);
    assert!(!observed_normal_vacancy(&late, 77, &waiters));
}

#[tokio::test]
async fn community_social_six_actual_controllers_wait_reconsider_and_enter_after_ordinary_departure()
 {
    let (dir, mut app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    fs::write(
        dir.join("testing.json"),
        br#"{"schema":1,"provenance":"prepared"}"#,
    )
    .unwrap();
    app.testing = crate::server::testing::Testing::open(&dir).unwrap();
    app.control = Some("social-test".into());
    let log = fs::File::create(dir.join("social-runtime.jsonl")).unwrap();
    let sub = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(log))
        .finish();
    let _logging = tracing::subscriber::set_default(sub);
    let content = app.content.clone();
    let global = app.max_players;
    let nexus = app
        .world
        .directory(move |d| {
            let mut w = World::new(content, map())?;
            w.bind_fixture_clock(&d.clock);
            w.name = "Realm".into();
            w.max_players = global;
            let mut goods = vec![-1; 8];
            goods[..2].fill(0xa22);
            w.spawn_entity(0x500, Pos { x: 28.5, y: 20.5 }, goods)?;
            for y in 12..30 {
                w.spawn_entity(
                    0x120,
                    Pos {
                        x: 19.5,
                        y: y as f32 + 0.5,
                    },
                    vec![],
                )?;
            }
            for y in [7.5, 33.5] {
                w.spawn_entity(0x205, Pos { x: 62.5, y }, vec![])?;
            }
            d.worlds.insert(-3, w);
            let nexus = &d.worlds[&-2];
            let portal = nexus.entities.values().find(|p| p.kind == 0x712).unwrap();
            let portal_id = portal.id;
            let portal = portal.pos;
            let vault = nexus
                .entities
                .values()
                .find(|p| p.kind == 0x720)
                .unwrap()
                .pos;
            Ok((portal, vault, portal_id))
        })
        .await
        .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization", "Bearer social-test".parse().unwrap());
    let response = crate::server::testing::command(
        axum::extract::State(app.clone()),
        headers,
        axum::Json(
            json!({"id":"social-capacity","command":{"op":"prepare_realm_capacity","limit":4}}),
        ),
    )
    .await;
    let receipt: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["status"], "succeeded");
    let mut initial = vec![];
    let mut accounts = vec![];
    let mut waiters = vec![];
    for (i, p) in PROFILES.into_iter().enumerate() {
        let items = if i == 0 {
            [0xa3d, 0xa3d, 0xa3d, 0xa3d, -1, -1, -1, -1]
        } else {
            [0xa22, 0xa22, -1, -1, -1, -1, -1, -1]
        };
        let ch = prepare_character(&app, p, items).await;
        accounts.push(ch.account);
        if i >= 4 {
            waiters.push((ch.account, ch.id));
        }
        let pos = if i == 0 {
            Pos { x: 10.5, y: 20.5 }
        } else if i < 4 {
            Pos {
                x: 55.5,
                y: 7.5 + (i - 1) as f32 * 12.,
            }
        } else {
            nexus.0
        };
        let world = if i < 4 { -3 } else { -2 };
        initial.push(json!({"character":ch,"world":world,"position":pos}));
        let selected = ch.id;
        let claim = app.claim(ch.account).unwrap();
        launch(&app, p, ch, world, Some(pos), Some(selected), claim)
            .await
            .unwrap();
    }
    fs::write(dir.join("social-declaration.json"),serde_json::to_vec_pretty(&json!({"initial":initial,"capacity":receipt,"nexus_entrance":nexus.0,"vault":nexus.1,"max_game_ms":90000,"max_wall_ms":150000,"humans":0,"autonomous":6,"setup":"initial own resources and ordinary visible health bag; one reserve-deficit outing, others feasible independent outings, no waiting/departure instructions or after-start mutation; protected capacity before enrollment"})).unwrap()).unwrap();
    app.world.clock.set_paused(false);
    let began = Instant::now();
    let mut sequence = vec![];
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        commits(&app).await;
        let sample=app.world.directory(|d|{
            let views=d.worlds.values().flat_map(|w|w.players.iter()).map(|(id,p)|{
                let v=d.player_view(*id)?;
                Ok(json!({"session":v.session,"map":v.map,"position":v.position,"character":v.character,"lifecycle":v.lifecycle,"objects":v.objects,"shots":p.character.shots,"controller":format!("{:?}",p.controller)}))
            }).collect::<Result<Vec<_>>>()?;
            let realm=&d.worlds[&-3];let nexus=&d.worlds[&-2];let entrance=nexus.entities.values().find(|e|e.kind==0x712).unwrap();
            Ok(json!({"game_ms":d.tick*50,"realm_count":realm.players.len(),"entrance":entrance,"views":views}))
        }).await.unwrap();
        let game = sample["game_ms"].as_u64().unwrap();
        use std::io::Write;
        let mut retained = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("social-sequence.jsonl"))
            .unwrap();
        writeln!(retained, "{sample}").unwrap();
        sequence.push(sample);
        let entered = sequence.iter().any(|s| {
            s["views"].as_array().unwrap().iter().any(|v| {
                (v["session"]["account"] == accounts[4] || v["session"]["account"] == accounts[5])
                    && v["map"]["name"] == "Realm"
            })
        });
        let sorted = sequence.iter().any(|s| {
            s["views"].as_array().unwrap().iter().any(|v| {
                v["session"]["account"] == accounts[5]
                    && v["map"]["name"] == "Nexus"
                    && s["realm_count"] == 4
                    && s["game_ms"].as_u64().unwrap() >= 5500
                    && ((v["position"]["x"].as_f64().unwrap() - nexus.0.x as f64)
                        .hypot(v["position"]["y"].as_f64().unwrap() - nexus.0.y as f64)
                        > 1.)
            })
        });
        if (entered && sorted && game >= 20000)
            || game >= 90000
            || began.elapsed() > Duration::from_secs(150)
        {
            break;
        }
    }
    fs::write(
        dir.join("social-sequence.json"),
        serde_json::to_vec_pretty(&sequence).unwrap(),
    )
    .unwrap();
    app.world.clock.set_paused(true);
    let end = app.world.directory(|d| {
        let views = d.worlds.values().flat_map(|w| w.players.keys()).map(|id| d.player_view(*id)).collect::<Result<Vec<_>>>()?;
        Ok(json!({"tick":d.tick,"clock_paused":d.clock.paused(),"players":views.len(),"quarantined":views.iter().filter(|v| v.lifecycle.quarantined).count(),"dead":views.iter().filter(|v| !v.lifecycle.alive).count()}))
    }).await.unwrap();
    let live_prefix = fs::read_to_string(dir.join("social-runtime.jsonl")).unwrap();
    let live_events = live_prefix
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    fs::write(
        dir.join("social-runtime-before-cleanup.jsonl"),
        &live_prefix,
    )
    .unwrap();
    use sha2::Digest;
    fs::write(dir.join("social-observation-boundary.json"), serde_json::to_vec_pretty(&json!({"end":end,"owners":app.owners.lock().unwrap().len(),"portal":nexus.2,"waiters":waiters,"prefix_bytes":live_prefix.len(),"prefix_sha256":format!("{:x}",sha2::Sha256::digest(live_prefix.as_bytes())),"scope":"immutable live observation prefix BEFORE any stop or cleanup departure"})).unwrap()).unwrap();
    stop(&app).await;
    let minds = app.store.bot_mind(accounts[0]).await.unwrap().unwrap();
    fs::write(
        dir.join("social-result.json"),
        serde_json::to_vec_pretty(
            &json!({"mind":minds,"cleanup":{"owners":0,"players":0,"quarantined":0}}),
        )
        .unwrap(),
    )
    .unwrap();
    println!("social capacity evidence: {}", dir.display());
    assert!(
        sequence
            .iter()
            .all(|s| s["realm_count"].as_u64().unwrap() <= 4
                && s["views"].as_array().unwrap().len() <= 6)
    );
    let full: Vec<_> = sequence
        .iter()
        .filter(|s| {
            s["realm_count"] == 4 && s["entrance"]["name"].as_str().unwrap().ends_with("(4/4)")
        })
        .collect();
    assert!(
        full.len() >= 20,
        "no sustained observed full interval: {}",
        dir.display()
    );
    assert!(sequence.iter().all(|s| s["entrance"]["id"] == nexus.2));
    assert!(
        observed_normal_vacancy(&live_events, nexus.2, &waiters),
        "no ordered normal vacancy publication, same waiter observation and legal admission before cleanup: {}",
        dir.display()
    );
    assert!(
        sequence.iter().any(|s| s["views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["session"]["account"] == accounts[0] && v["map"]["name"] == "Nexus")),
        "no ordinary occupant departure: {}",
        dir.display()
    );
    assert!(
        sequence.iter().any(|s| s["views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| (v["session"]["account"] == accounts[4]
                || v["session"]["account"] == accounts[5])
                && v["map"]["name"] == "Realm")),
        "no waiting controller entry: {}",
        dir.display()
    );
    assert!(
        sequence
            .iter()
            .any(|s| s["views"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["session"]["account"] == accounts[5]
                    && v["map"]["name"] == "Nexus"
                    && s["realm_count"] == 4
                    && s["game_ms"].as_u64().unwrap() >= 5500
                    && ((v["position"]["x"].as_f64().unwrap() - nexus.0.x as f64)
                        .hypot(v["position"]["y"].as_f64().unwrap() - nexus.0.y as f64)
                        > 1.))),
        "no independent reconsideration: {}",
        dir.display()
    );
}
