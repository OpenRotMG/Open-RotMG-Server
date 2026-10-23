use super::*;

pub(in crate::bots::runtime) async fn rebuilding_context(
    app: &mut App,
    dir: &std::path::Path,
) -> Value {
    app.world.clock.set_paused(true);
    fs::write(
        dir.join("testing.json"),
        br#"{"schema":1,"provenance":"prepared"}"#,
    )
    .unwrap();
    app.testing = crate::server::testing::Testing::open(dir).unwrap();
    app.control = Some("rebuilding-capacity-test".into());
    let global = app.max_players;
    let (portal, spawn, hazards) = app
        .world
        .directory(|d| {
            ensure!(
                d.tick == 0,
                "rebuilding context must precede all controller time"
            );
            let w = d
                .worlds
                .get_mut(&-3)
                .context("ordinary active fixture Realm missing")?;
            let (x, y) = w.map.spawns[0];
            let spawn = Pos { x, y };
            let mut hazards = vec![];
            let spawns=w.map.spawns.clone();ensure!(spawns.len()==4,"declared normal four-shore Realm topology changed");
            for (sx,sy) in spawns {
                let places=[(6.,0.),(0.,6.),(-6.,0.),(0.,-6.),(5.,5.),(-5.,-5.)].into_iter().map(|(dx,dy)|Pos{x:sx+dx,y:sy+dy}).filter(|p|w.walkable(*p)).take(3).collect::<Vec<_>>();
                ensure!(places.len()==3,"ordinary shore lacks declared reachable encounter positions");
                for (kind,p) in [0x654,0x651,0x652].into_iter().zip(places) {
                    let definition=w.content.get(kind)?;
                    let stats=json!({"name":definition.name,"hp":definition.hp,"enemy":definition.enemy,"stasis_immune":definition.stasis_immune,"projectiles":format!("{:?}",definition.projectiles)});
                    let id=w.spawn_entity(kind,p,vec![])?;
                    hazards.push(json!({"id":id,"kind":kind,"position":p,"shore":Pos{x:sx,y:sy},"definition":stats,"initial_entity":w.entities[&id],"semantics":"normal registered encounter AI/physics/damage; no status or timer alteration"}));
                }
            }
            ensure!(hazards.len()==12,"declared distributed encounter budget changed");
            let portal = d.worlds[&-2]
                .entities
                .values()
                .find(|e| e.kind == 0x712)
                .context("ordinary Realm entrance missing")?
                .pos;
            Ok((portal, spawn, hazards))
        })
        .await
        .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer rebuilding-capacity-test".parse().unwrap(),
    );
    let response = crate::server::testing::command(
        axum::extract::State(app.clone()),
        headers,
        axum::Json(
            json!({"id":"rebuild-capacity","command":{"op":"prepare_realm_capacity","limit":4}}),
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
    for (i, p) in [
        PROFILES[2],
        PROFILES[1],
        PROFILES[0],
        PROFILES[3],
        PROFILES[4],
        PROFILES[5],
    ]
    .into_iter()
    .enumerate()
    {
        let ch = prepare_character(app, p, [0xa22, 0xa22, -1, -1, -1, -1, -1, -1]).await;
        let position = if i < 4 {
            Pos {
                x: spawn.x + i as f32 * 0.5,
                y: spawn.y,
            }
        } else {
            portal
        };
        let world = if i < 4 { -3 } else { -2 };
        initial.push(json!({"character":ch,"world":world,"position":position}));
        let claim = app.claim(ch.account).unwrap();
        launch(app, p, ch, world, Some(position), None, claim)
            .await
            .unwrap();
    }
    let declaration = json!({"initial":initial,"capacity":receipt,"portal":portal,"hazards":hazards,"realm_spawn":spawn,"global_capacity":global,"bots":6,"humans":0,"max_game_ms":120000,"max_wall_ms":210000,"context":"Initial twelve normal registered Gods distributed three per each of four ordinary shore spawns, no altered NPC stats/status/timers; four free outings and two real Nexus waiters; normal selected=None repeated lifetimes only; no assigned deaths, actions, consent, friends or after-start mutation. All six selected=None ordinary account lifetimes."});
    fs::write(
        dir.join("rebuild-declaration.json"),
        serde_json::to_vec_pretty(&declaration).unwrap(),
    )
    .unwrap();
    declaration
}

#[tokio::test]
async fn community_social_actual_lifetimes_rebuild_independently_after_natural_deaths() {
    let (dir, mut app) = crate::server::fixture();
    let log = fs::File::create(dir.join("rebuild-runtime.jsonl")).unwrap();
    let sub = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(log))
        .finish();
    let _logging = tracing::subscriber::set_default(sub);
    let declaration = rebuilding_context(&mut app, &dir).await;
    let serving = tokio::spawn(crate::server::serve(
        app.clone(),
        "127.0.0.1:0".parse().unwrap(),
        std::path::PathBuf::from(std::env::var("CLIENT_DIR").unwrap()),
    ));
    let initial = declaration["initial"].as_array().unwrap();
    let originals: Vec<_> = initial[..4]
        .iter()
        .map(|c| {
            (
                c["character"]["account"].as_i64().unwrap() as i32,
                c["character"]["id"].as_i64().unwrap() as i32,
            )
        })
        .collect();
    let waiters = [
        initial[4]["character"]["account"].as_i64().unwrap(),
        initial[5]["character"]["account"].as_i64().unwrap(),
    ];
    app.world.clock.set_paused(false);
    let began = Instant::now();
    let mut sequence = vec![];
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let sample=app.world.directory(|d| {
            let views=d.worlds.values().flat_map(|w|w.players.iter()).map(|(id,_)| {let v=d.player_view(*id)?;Ok(json!({"session":v.session,"map":v.map.name,"character":v.character,"lifecycle":v.lifecycle,"position":v.position}))}).collect::<Result<Vec<_>>>()?;
            Ok(json!({"game_ms":d.tick*50,"realm_count":d.worlds[&-3].players.len(),"views":views,"hazards":d.worlds[&-3].entities.values().filter(|e|matches!(e.kind,0x651|0x654|0x652)).collect::<Vec<_>>()}))
        }).await.unwrap();
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("rebuild-sequence.jsonl"))
                .unwrap(),
            "{sample}"
        )
        .unwrap();
        let game = sample["game_ms"].as_u64().unwrap();
        sequence.push(sample);
        let replaced = sequence.iter().any(|s| {
            s["views"].as_array().unwrap().iter().any(|v| {
                originals.iter().any(|(account, old)| {
                    v["session"]["account"] == *account && v["session"]["character"] != *old
                })
            })
        });
        let all_live = sequence.last().unwrap()["views"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["lifecycle"]["alive"] == true && v["lifecycle"]["quarantined"] == false)
            .count()
            == 6;
        if !app.quarantined.lock().unwrap().is_empty()
            || (replaced && all_live && game >= 45000)
            || game >= 120000
            || began.elapsed() > Duration::from_secs(210)
        {
            break;
        }
    }
    app.world.clock.set_paused(true);
    let log: Vec<Value> = fs::read_to_string(dir.join("rebuild-runtime.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let events: Vec<_> = log
        .iter()
        .filter(|e| {
            matches!(
                e["fields"]["message"].as_str(),
                Some(
                    "permanent death committed"
                        | "autonomous character entered"
                        | "autonomous durable transfer completed"
                        | "autonomous controller stopped; account retained for diagnosis"
                        | "Realm portal updated"
                        | "autonomous decision"
                        | "autonomous replacement rebuilding arrival selected"
                        | "player entered development world"
                )
            )
        })
        .cloned()
        .collect();
    let quarantine = app.quarantined.lock().unwrap().len();
    let owners = app.owners.lock().unwrap().len();
    fs::write(dir.join("rebuild-result.json"),serde_json::to_vec_pretty(&json!({"events":events,"owners":owners,"quarantined":quarantine,"originals":originals,"sequence_samples":sequence.len()})).unwrap()).unwrap();
    println!("rebuild intersection evidence: {}", dir.display());
    app.shutdown.send_replace(true);
    if quarantine == 0 {
        stop(&app).await;
    }
    serving.abort();
    let _ = serving.await;
    let death = events
        .iter()
        .find(|e| {
            e["fields"]["message"] == "permanent death committed"
                && originals
                    .iter()
                    .any(|(_, old)| e["fields"]["character"] == *old)
        })
        .expect("an original Realm controller must actually die to qualify this context");
    let (account, old) = *originals
        .iter()
        .find(|(_, old)| death["fields"]["character"] == *old)
        .unwrap();
    let entry = events.iter().find(|e| {
        e["fields"]["message"] == "autonomous character entered"
            && e["fields"]["account"] == account
            && e["fields"]["character"] != old
    });
    assert!(
        sequence
            .iter()
            .all(|s| s["realm_count"].as_u64().unwrap() <= 4
                && s["views"].as_array().unwrap().len() <= 6)
    );
    assert!(
        events.iter().any(
            |e| e["fields"]["message"] == "autonomous durable transfer completed"
                && e["fields"]["destination"] == -3
                && waiters.contains(&e["fields"]["account"].as_i64().unwrap())
                && e["timestamp"].as_str().unwrap() > death["timestamp"].as_str().unwrap()
        ),
        "a genuine Nexus waiter must take the death vacancy"
    );
    assert_eq!(
        quarantine, 0,
        "ordinary replacement lifetimes must not quarantine"
    );
    assert_eq!(
        owners, 6,
        "all ordinary account lifetimes retain their owners"
    );
    let entry = entry.expect("same-account ordinary replacement must enter");
    let new = entry["fields"]["character"].as_i64().unwrap() as i32;
    assert!(app.store.load(account, old).await.is_err());
    assert_eq!(entry["fields"]["level"], 1);
    assert_eq!(entry["fields"]["experience"], 0);
    let class = entry["fields"]["class"].as_u64().unwrap() as u16;
    assert_eq!(
        serde_json::from_str::<Vec<i32>>(entry["fields"]["items"].as_str().unwrap()).unwrap(),
        app.content.get(class).unwrap().equipment
    );
    assert!(
        events.iter().any(
            |e| e["fields"]["message"] == "player entered development world"
                && e["fields"]["character"] == new
                && e["fields"]["world"] == "Nexus"
        ),
        "first new life must deliberately arrive in Nexus, not try a lost Realm slot"
    );
    fn seconds(e: &Value) -> f64 {
        let s = e["timestamp"].as_str().unwrap();
        let t = s.split('T').nth(1).unwrap().trim_end_matches('Z');
        let p = t.split(':').collect::<Vec<_>>();
        p[0].parse::<f64>().unwrap() * 3600.
            + p[1].parse::<f64>().unwrap() * 60.
            + p[2].parse::<f64>().unwrap()
    }
    let gap = (seconds(entry) - seconds(death) + 86400.) % 86400.;
    assert!(gap >= 3., "ordinary replacement delay shortened: {gap}");
    let mut current = vec![];
    for v in sequence.last().unwrap()["views"].as_array().unwrap() {
        let a = v["session"]["account"].as_i64().unwrap() as i32;
        let ch = v["session"]["character"].as_i64().unwrap() as i32;
        let living = app.store.load(a, ch).await.unwrap();
        assert!(living.alive);
        current.push(living);
    }
    assert_eq!(current.len(), 6);
    fs::write(dir.join("rebuild-lifetime-proof.json"),serde_json::to_vec_pretty(&json!({"scope":"actual six selected=None lifetimes; natural death and independent rebuilding, NOT deterministic full-race proof","full_race_conjunction":"not claimed; separately covered by direct production-helper engineering regression","first_death":death,"first_new_life":entry,"actual_wall_gap_seconds":gap,"current_characters_by_account":current,"owners_before_shutdown":owners,"quarantined":quarantine})).unwrap()).unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["fields"]["message"] == "autonomous decision"
                && e["fields"]["account"] == account
                && e["fields"]["character"] == new),
        "replacement must make an actual independent decision"
    );
}
