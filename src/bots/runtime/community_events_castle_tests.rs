use super::*;
use sha2::Digest;

fn castle_log_prefix(dir: &std::path::Path, dispatch: &tracing::Dispatch) -> String {
    // Live completion checks must not parse a partially written JSON event.
    let _writer = dispatch
        .downcast_ref::<Mutex<fs::File>>()
        .expect("Castle subscriber must retain its declared file writer")
        .lock()
        .unwrap();
    fs::read_to_string(dir.join("events-castle-runtime.jsonl")).unwrap()
}

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

fn engaged_accounts(sequence: &[Value], actors: &[(i32, i32)]) -> usize {
    actors
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
        .count()
}

fn complete_positive_progression(
    fields: &[Value],
    sequence: &[Value],
    actors: &[(i32, i32)],
) -> bool {
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
    entered
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
        })
        && engaged_accounts(sequence, actors) >= 3
}

#[test]
fn community_events_castle_positive_stop_requires_complete_earned_owned_acknowledged_progression() {
    let actors: Vec<_> = (1..=6).map(|a| (a, a)).collect();
    let fields = vec![
        json!({"message":"actual bound Guardian sword defeat credited"}),
        json!({"message":"actual Guardian pair defeated; Chamber binding requested"}),
        json!({"message":"autonomous durable transfer completed","portal_kind":0xd7b}),
        json!({"message":"actual Oryx defeat opened earned exits","boss":crate::endgame::ORYX}),
        json!({"message":"consumable committed; bound dungeon portal published","kind":0x700,"account":2,"character":2,"destination":3}),
        json!({"message":"autonomous durable transfer completed","portal_kind":0x700,"account":2,"character":2,"destination":3}),
    ];
    let sequence = vec![
        json!({"actors":(1..=6).map(|a| json!({"session":{"account":a,"character":a},"map":if a==2 {"Wine Cellar"} else {"Oryx's Chamber"},"shots":1,"diagnostic":{"character":{"hits":1}}})).collect::<Vec<_>>()}),
    ];
    assert!(complete_positive_progression(&fields, &sequence, &actors));
    let boundary = fields.len() - 1;
    let mut late_ack = fields[..boundary].to_vec();
    late_ack.push(json!({"message":"prepared postclear terminal key audit"}));
    late_ack.push(fields.last().unwrap().clone());
    assert!(complete_positive_progression(&late_ack, &sequence, &actors));
    assert!(
        !complete_positive_progression(&late_ack[..boundary], &sequence, &actors),
        "an ACK after the captured pre-pause boundary cannot backfill positive completion"
    );
    for missing in 0..fields.len() {
        let mut f = fields.clone();
        f.remove(missing);
        assert!(
            !complete_positive_progression(&f, &sequence, &actors),
            "missing authority {missing}"
        );
    }
    let mut reordered = fields.clone();
    reordered.swap(0, 1);
    assert!(!complete_positive_progression(
        &reordered, &sequence, &actors
    ));
    reordered = fields.clone();
    reordered.swap(4, 5);
    assert!(!complete_positive_progression(
        &reordered, &sequence, &actors
    ));
    for (index, field, value) in [
        (4, "account", json!(9)),
        (4, "character", json!(9)),
        (5, "account", json!(9)),
        (5, "character", json!(9)),
        (5, "destination", json!(9)),
        (
            4,
            "message",
            json!("portal binding staged; consumable not committed or portal published yet"),
        ),
        (5, "message", json!("transfer grant issued")),
    ] {
        let mut f = fields.clone();
        f[index][field] = value;
        assert!(
            !complete_positive_progression(&f, &sequence, &actors),
            "invalid authority {index}/{field}"
        );
    }
    assert!(!complete_positive_progression(
        &fields[4..],
        &sequence,
        &actors
    ));
    let mut duplicate = fields.clone();
    duplicate.push(fields[4].clone());
    assert!(!complete_positive_progression(
        &duplicate, &sequence, &actors
    ));
    let mut absent = sequence.clone();
    for p in absent[0]["actors"].as_array_mut().unwrap() {
        p["map"] = json!("Oryx's Chamber");
    }
    assert!(!complete_positive_progression(&fields, &absent, &actors));
    let mut unengaged = sequence.clone();
    for p in unengaged[0]["actors"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .skip(2)
    {
        p["shots"] = json!(0);
    }
    assert!(!complete_positive_progression(&fields, &unengaged, &actors));
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
    let dispatch = tracing::Dispatch::new(sub);
    let _logging = tracing::dispatcher::set_default(&dispatch);
    let (world, position) = app
        .world
        .directory(|d| {
            let world = d.prepared_instance(crate::endgame::CASTLE)?;
            let w = d.worlds.get_mut(&world).unwrap();
            let mut guardians = vec![];
            for e in w
                .entities
                .matching_mut(|e| crate::endgame::GUARDIANS.contains(&e.kind))
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
        assert!(app.world.clock.paused());
        let vaults = app.store.vaults(account).await.unwrap();
        assert!(
            !vaults.is_empty()
                && vaults.iter().all(|v| v.account == account
                    && v.revision == 0
                    && v.items.iter().all(|k| *k == -1))
        );
        initial.push(json!({"character":ch,"position":position,"vaults":vaults}));
        let selected = ch.id;
        let claim = app.claim(ch.account).unwrap();
        launch(&app, p, ch, world, Some(position), Some(selected), claim)
            .await
            .unwrap();
    }
    fs::write(dir.join("events-castle-initial.json"),serde_json::to_vec_pretty(&json!({"initial":initial,"context":"PREPARED production Castle near initially injured actual Guardian pair2000HP; SIX-ACTOR PREPARED existing selected cohort; protected-Spawn-equivalent initial prepared two Priests with supported Purification tomes+four Wizard classes/kits (not earned class unlocks); first three participants and resources unchanged from retained failed three-actor context, same ONE Incantation moved before start from Priest1slot10 to Biceps2slot10 under approved initial-owner counterfactual; prior negative owner permanently died/key lost, NOT repaired. No clear/portal/consent/outcome written. Chamber/Cellar subsequently created only by ordinary earned production lifecycle, with their unmodified full bosses.","max_game_ms":300000,"max_wall_ms":600000,"scope":"six existing identities, continuous actual-controller capability, NOT ordinary earned class/kit or culture", "bots":6,"humans":0})).unwrap()).unwrap();
    let start_tick = app.world.directory(|d| Ok(d.tick)).await.unwrap();
    let start_clock = app.world.clock.now();
    app.world.clock.set_paused(false);
    let start = Instant::now();
    let mut sequence = vec![];
    let stop_cause = loop {
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
        if ms >= 300000 {
            break "simulation_budget";
        }
        if start.elapsed() > Duration::from_secs(600) {
            break "wall_budget";
        }
        if sequence.last().unwrap()["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["map"] == "Wine Cellar")
        {
            let prefix = castle_log_prefix(&dir, &dispatch);
            let observed = prefix
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap()["fields"].clone())
                .collect::<Vec<_>>();
            if complete_positive_progression(&observed, &sequence, &actors) {
                break "complete_positive_progression";
            }
        }
        if ended && ms >= 5000 {
            break "all_participants_outside_shared";
        }
    };
    let positive_prefix = castle_log_prefix(&dir, &dispatch);
    assert!(
        positive_prefix.ends_with('\n'),
        "positive boundary must contain complete JSON events"
    );
    app.world.clock.set_paused(true);
    let positive_fields = positive_prefix
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["fields"].clone())
        .collect::<Vec<_>>();
    let (end_tick, terminal_audit) = app
        .world
        .directory(|d| {
            Ok((
                d.tick,
                d.prepared_postclear_key_audit(None)?
                    .context("missing terminal key authority")?,
            ))
        })
        .await
        .unwrap();
    tracing::info!(audit=%terminal_audit,"prepared postclear terminal key audit");
    let step_ms = (end_tick - start_tick) * 50;
    let wall_ms = start.elapsed().as_millis();
    let active_clock_ms = app
        .world
        .clock
        .now()
        .saturating_duration_since(start_clock)
        .as_millis();
    let prefix = castle_log_prefix(&dir, &dispatch);
    assert!(
        prefix.starts_with(&positive_prefix),
        "full trace must retain exact pre-pause positive prefix"
    );
    fs::write(
        dir.join("events-castle-runtime-before-cleanup.jsonl"),
        &prefix,
    )
    .unwrap();
    let fields: Vec<Value> = prefix
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["fields"].clone())
        .collect();
    fs::write(dir.join("events-castle-observation-boundary.json"),serde_json::to_vec_pretty(&json!({"start_tick":start_tick,"end_tick":end_tick,"simulation_step_ms":step_ms,"shared_clock_active_ms":active_clock_ms,"active_wall_ms":wall_ms,"before_cleanup":true,"stop_cause":stop_cause,"positive_observation_boundary":"writer-locked complete prefix captured BEFORE pause; later events not positive proof","positive_prefix_bytes":positive_prefix.len(),"positive_prefix_events":positive_fields.len(),"positive_prefix_sha256":format!("{:x}",sha2::Sha256::digest(positive_prefix.as_bytes()))})).unwrap()).unwrap();
    fs::write(
        dir.join("events-castle-sequence.json"),
        serde_json::to_vec(&sequence).unwrap(),
    )
    .unwrap();
    cleanup(&app).await;
    println!("event Castle original evidence: {}", dir.display());
    let published: Vec<_> = fields
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f["message"] == "consumable committed; bound dungeon portal published"
                && f["kind"] == 0x700
        })
        .collect();
    let positive = complete_positive_progression(&positive_fields, &sequence, &actors);
    let final_views = sequence.last().unwrap()["actors"].as_array().unwrap();
    let abandoned = actors.iter().all(|(account, ch)| {
        permanent_death(&dir, *ch)
            || final_views.iter().any(|p| {
                p["session"]["account"] == *account
                    && p["session"]["character"] == *ch
                    && !events::shared(p["map"].as_str().unwrap())
            }) && (guarded_exit(&fields, *account) || readiness_exit(&fields, *account))
    });
    let engaged = engaged_accounts(&sequence, &actors);
    let deaths = super::aftermath::canonical_death_revisions(&dir, &actors);
    let completed_oryx_key_lost_aftermath =
        super::aftermath::completed_oryx_key_lost_aftermath(&fields, &sequence, &initial, &deaths);
    let result = json!({"original":dir,"positive_continuous_chain_observed":positive,"causal_all_participant_abandon":abandoned,
        "completed_oryx_key_lost_aftermath_cellar_negative":completed_oryx_key_lost_aftermath,
        "engaged_accounts":engaged,"consumables_committed":published.len(),"simulation_step_ms":step_ms,"active_wall_ms":wall_ms,"stop_cause":stop_cause});
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
