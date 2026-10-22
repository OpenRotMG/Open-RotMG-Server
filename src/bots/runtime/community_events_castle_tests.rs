use super::*;

pub(super) fn earned_chamber_prefix(fields: &[Value]) -> bool {
    let mut stage = 0;
    for f in fields {
        let message = f["message"].as_str().unwrap();
        let advances = match stage {
            0 => message == "actual bound Guardian sword defeat credited",
            1 => message == "actual Guardian pair defeated; Chamber binding requested",
            2 => message == "autonomous durable transfer completed" && f["portal_kind"] == 0xd7b,
            3 => {
                message == "actual Oryx defeat opened earned exits"
                    && f["boss"] == i32::from(crate::endgame::ORYX)
            }
            _ => false,
        };
        if advances {
            stage += 1;
        }
    }
    stage == 4
}
#[test]
fn community_events_castle_progression_observer_rejects_pending_or_reordered_authority() {
    let mut evidence = vec![
        json!({"message":"actual bound Guardian sword defeat credited"}),
        json!({"message":"actual Guardian pair defeated; Chamber binding requested"}),
        json!({"message":"autonomous durable transfer completed","portal_kind":0xd7b}),
        json!({"message":"actual Oryx defeat opened earned exits","boss":crate::endgame::ORYX}),
    ];
    assert!(earned_chamber_prefix(&evidence));
    evidence.swap(0, 1);
    assert!(!earned_chamber_prefix(&evidence));
    evidence.swap(0, 1);
    evidence[3] = json!({"message":"portal binding staged; consumable not committed or portal published yet"});
    assert!(!earned_chamber_prefix(&evidence));
}

#[tokio::test]
async fn community_events_actual_castle_controllers_progress_or_causally_abandon_with_guarded_outcomes()
 {
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
    let log = fs::File::create(dir.join("events-castle-runtime.jsonl")).unwrap();
    let sub = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .with_writer(Mutex::new(log))
        .finish();
    let _logging = tracing::subscriber::set_default(sub);
    let (world, position) = app
        .world
        .directory(|d| {
            let world = d.prepared_instance(crate::endgame::CASTLE)?;
            let w = d.worlds.get_mut(&world).unwrap();
            let mut guardians = vec![];
            for e in w
                .entities
                .values_mut()
                .filter(|e| crate::endgame::GUARDIANS.contains(&e.kind))
            {
                e.hp = 2000;
                guardians.push(e.pos);
            }
            let position = Pos {
                x: (guardians[0].x + guardians[1].x) * 0.5,
                y: (guardians[0].y + guardians[1].y) * 0.5 + 7.,
            };
            assert!(w.walkable(position));
            Ok((world, position))
        })
        .await
        .unwrap();
    let mut initial = vec![];
    let mut actors = vec![];
    for (i, p) in [
        PROFILES[3],
        PROFILES[1],
        PROFILES[2],
        PROFILES[0],
        PROFILES[4],
        PROFILES[5],
    ]
    .into_iter()
    .enumerate()
    {
        let account = app
            .store
            .list(format!("event-castle-{i}"))
            .await
            .unwrap()
            .id;
        let class = if i == 0 || i == 3 {
            crate::content::PRIEST
        } else {
            WIZARD
        };
        let mut ch = app
            .store
            .create_prepared(
                account,
                app.content.get(class).unwrap(),
                Some(p.name.into()),
            )
            .await
            .unwrap();
        ch.level = 20;
        ch.exp = crate::store::level_start(20).unwrap();
        ch.base = app.content.classes[&class].caps;
        ch.hp = ch.base[0];
        ch.mp = ch.base[1];
        ch.items = vec![
            2824, 2852, 2821, 2765, 0xa22, 0xa22, 0xa22, 0xa22, 0xa23, 0xa23, -1, -1,
        ];
        if i == 0 || i == 3 {
            ch.items[0] = 0xaf6;
            ch.items[1] = 0xc09;
        }
        if i == 1 {
            ch.items[10] = i32::from(crate::endgame::INCANTATION);
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
                vec![],
            )
            .await
            .unwrap()[0];
        app.store.progress(ch.clone()).await.unwrap();
        actors.push((account, ch.id));
        initial.push(json!({"character":ch,"position":position}));
        let selected = ch.id;
        launch(&app, p, ch, world, Some(position), Some(selected))
            .await
            .unwrap();
    }
    fs::write(dir.join("events-castle-initial.json"),serde_json::to_vec_pretty(&json!({"initial":initial,"context":"PREPARED production Castle near initially injured actual Guardian pair2000HP; SIX-ACTOR PREPARED existing selected cohort; protected-Spawn-equivalent initial prepared two Priests with supported Purification tomes+four Wizard classes/kits (not earned class unlocks); first three participants and resources unchanged from retained failed three-actor context, same ONE Incantation moved before start from Priest1slot10 to Biceps2slot10 under approved initial-owner counterfactual; prior negative owner permanently died/key lost, NOT repaired. No clear/portal/consent/outcome written. Chamber/Cellar subsequently created only by ordinary earned production lifecycle, with their unmodified full bosses.","max_game_ms":300000,"max_wall_ms":600000,"scope":"six existing identities, continuous actual-controller capability, NOT ordinary earned class/kit or culture", "bots":6,"humans":0})).unwrap()).unwrap();
    let start_tick = app.world.directory(|d| Ok(d.tick)).await.unwrap();
    let start_clock = app.world.clock.now();
    app.world.clock.set_paused(false);
    let start = Instant::now();
    let mut sequence = vec![];
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        commits(&app).await;
        let s=app.world.directory(|d| {
            let actors=d.worlds.values().flat_map(|w|w.players.iter()).map(|(id,p)|{
                let v=d.player_view(*id)?;
                Ok(json!({"session":v.session,"map":v.map.name,"position":v.position,"character":v.character,"lifecycle":v.lifecycle,"shots":p.character.shots,"kills":p.character.kills,"diagnostic":d.worlds[&v.session.world].observe(*id)?}))
            }).collect::<Result<Vec<_>>>()?;
            Ok(json!({"game_ms":d.tick*50,"actors":actors}))
        }).await.unwrap();
        let ms = s["game_ms"].as_u64().unwrap();
        sequence.push(s);
        let out = sequence.last().unwrap()["actors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["map"] == "Nexus" || p["map"] == "Realm");
        let ended = out
            && actors.iter().all(|(account, ch)| {
                sequence.last().unwrap()["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| {
                        p["session"]["account"] == *account
                            && p["session"]["character"] == *ch
                            && !events::shared(p["map"].as_str().unwrap())
                    })
                    || permanent_death(&dir, *ch)
            });
        if ms >= 300000 || start.elapsed() > Duration::from_secs(600) || ended && ms >= 5000 {
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
    let prefix = fs::read_to_string(dir.join("events-castle-runtime.jsonl")).unwrap();
    fs::write(
        dir.join("events-castle-runtime-before-cleanup.jsonl"),
        &prefix,
    )
    .unwrap();
    let fields: Vec<Value> = prefix
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["fields"].clone())
        .collect();
    fs::write(dir.join("events-castle-observation-boundary.json"),serde_json::to_vec_pretty(&json!({"start_tick":start_tick,"end_tick":end_tick,"simulation_step_ms":step_ms,"shared_clock_active_ms":active_clock_ms,"active_wall_ms":wall_ms,"before_cleanup":true})).unwrap()).unwrap();
    fs::write(
        dir.join("events-castle-sequence.json"),
        serde_json::to_vec(&sequence).unwrap(),
    )
    .unwrap();
    cleanup(&app).await;
    println!("event Castle original evidence: {}", dir.display());
    let entered = sequence.iter().any(|s| {
        s["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["map"] == "Wine Cellar")
    });
    let published: Vec<_> = fields
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f["message"] == "consumable committed; bound dungeon portal published"
                && f["kind"] == 0x700
        })
        .collect();
    let positive = entered
        && published.len() == 1
        && published[0].1["account"] == actors[1].0
        && published[0].1["character"] == actors[1].1
        && earned_chamber_prefix(&fields[..published[0].0])
        && fields[published[0].0 + 1..].iter().any(|f| {
            f["message"] == "autonomous durable transfer completed"
                && f["portal_kind"] == 0x700
                && f["destination"] == published[0].1["destination"]
                && actors
                    .iter()
                    .any(|(a, ch)| f["account"] == *a && f["character"] == *ch)
        });
    let final_views = sequence.last().unwrap()["actors"].as_array().unwrap();
    let abandoned = actors.iter().all(|(account, ch)| {
        permanent_death(&dir, *ch)
            || final_views.iter().any(|p| {
                p["session"]["account"] == *account
                    && p["session"]["character"] == *ch
                    && !events::shared(p["map"].as_str().unwrap())
            }) && (guarded_exit(&fields, *account) || readiness_exit(&fields, *account))
    });
    let engaged = actors
        .iter()
        .filter(|(account, _)| {
            sequence.iter().any(|s| {
                s["actors"].as_array().unwrap().iter().any(|p| {
                    p["session"]["account"] == *account
                        && p["shots"].as_u64().unwrap() > 0
                        && p["diagnostic"]["character"]["hits"].as_u64().unwrap() > 0
                })
            })
        })
        .count();
    let deaths = super::aftermath::canonical_death_revisions(&dir, &actors);
    let completed_oryx_key_lost_aftermath =
        super::aftermath::completed_oryx_key_lost_aftermath(&fields, &sequence, &initial, &deaths);
    let result = json!({"original":dir,"positive_continuous_chain_observed":positive,"causal_all_participant_abandon":abandoned,
        "completed_oryx_key_lost_aftermath_cellar_negative":completed_oryx_key_lost_aftermath,
        "engaged_accounts":engaged,"consumables_committed":published.len(),"simulation_step_ms":step_ms,"active_wall_ms":wall_ms});
    fs::write(
        dir.join("events-castle-result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("event Castle classified outcome: {result}");
    assert!(
        step_ms <= 300000 && wall_ms <= 600000,
        "declared active budgets exceeded: {}",
        dir.display()
    );
    assert!(
        published.len() <= 1 && engaged >= 3,
        "missing real participation or repeated key consumption: {}",
        dir.display()
    );
    assert!(
        positive || abandoned || completed_oryx_key_lost_aftermath.is_some(),
        "neither continuous progression, causal complete abandon nor verified completed-Oryx/key-lost aftermath: {}",
        dir.display()
    );
}
