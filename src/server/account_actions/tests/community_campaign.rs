use super::*;
use std::{collections::BTreeSet, io::Write, time::Duration};

async fn persist_campaign(app: &App) -> Result<()> {
    let progress = app
        .world
        .collect(crate::world::World::pending_progress)
        .await?;
    if !progress.is_empty() {
        let result = app
            .store
            .progress_batch(progress.iter().map(|(_, ch)| ch.clone()).collect())
            .await
            .context("progress persistence failed");
        for (id, snapshot) in progress {
            let committed = result
                .as_ref()
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("{e:#}"));
            app.world
                .for_player(id)
                .call(move |w| {
                    w.committed(id, committed)?;
                    w.finish_progress(id, snapshot)
                })
                .await?;
        }
        result?;
    }
    for (id, ch, killer) in app
        .world
        .collect(crate::world::World::pending_deaths)
        .await?
    {
        super::super::super::persist_death(app, id, ch, killer).await?;
    }
    Ok(())
}

async fn snapshot(app: &App) -> Result<serde_json::Value> {
    let started = app.started;
    let mut value = app.world.directory(move |d| {
        let observed_wall_uptime_ns = started.elapsed().as_nanos();
        let observed_clock_uptime_ns = d.clock.now().duration_since(started).as_nanos();
        let mut players = Vec::new();
        let mut worlds = Vec::new();
        for (world, w) in &d.worlds {
            worlds.push(serde_json::json!({"id":world,"name":w.name,"tick":w.tick,
                "layout":w.realm_layout,"width":w.map.width,"height":w.map.height,
                "prepared_seed":w.prepared_seed,"entities":w.entities.len()}));
            for (id, p) in &w.players {
                let view = d.player_view(*id)?;
                players.push(serde_json::json!({"session":view.session,"position":view.position,
                    "tick":view.tick,"time_ms":view.time_ms,"elapsed_ms":view.elapsed_ms,
                    "character":p.character,"lifecycle":view.lifecycle,"controller":format!("{:?}",p.controller),
                    "quest":view.quest,"objects":view.objects,"player_markers":view.player_markers,
                    "projectiles":view.projectiles,"known_tiles":view.tiles.len()}));
            }
        }
        Ok(serde_json::json!({"observed_wall_uptime_ns":observed_wall_uptime_ns,
            "observed_clock_uptime_ns":observed_clock_uptime_ns,
            "health":d.health(started),"worlds":worlds,"players":players}))
    }).await?;
    value["owners"] = serde_json::json!(
        app.owners
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect::<Vec<_>>()
    );
    value["quarantined"] = serde_json::json!(app.quarantined.lock().unwrap().len());
    Ok(value)
}

fn append(file: &mut impl Write, value: serde_json::Value) -> Result<()> {
    let mut line = serde_json::to_vec(&value)?;
    line.push(b'\n');
    file.write_all(&line)?;
    file.flush()?;
    Ok(())
}

struct TickHealth {
    tick: u64,
    changed_ns: u128,
}
impl TickHealth {
    fn observe(&mut self, tick: u64, observed_ns: u128) -> Result<()> {
        ensure!(
            tick >= self.tick && observed_ns >= self.changed_ns,
            "Directory health regressed"
        );
        if tick != self.tick {
            self.tick = tick;
            self.changed_ns = observed_ns;
        }
        ensure!(
            observed_ns - self.changed_ns < 1000000000,
            "Directory tick stalled"
        );
        Ok(())
    }
}
fn within_window_wall(earliest_ns: u128, latest_ns: u128) -> bool {
    latest_ns
        .checked_sub(earliest_ns)
        .is_some_and(|elapsed| elapsed <= 630000000000)
}

#[test]
fn community_campaign_tick_health_survives_window_checkpoints_and_rejects_regression() {
    let mut health = TickHealth {
        tick: 4,
        changed_ns: 0,
    };
    health.observe(4, 999999999).unwrap();
    assert!(health.observe(4, 1000000000).is_err());
    health.observe(5, 1000000000).unwrap();
    assert!(health.observe(4, 1000000001).is_err());
}
#[test]
fn community_campaign_record_buffer_preserves_values_without_token_sized_codec_writes() {
    #[derive(Default)]
    struct Sink {
        bytes: Vec<u8>,
        writes: usize,
        flushes: usize,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
    let value = serde_json::json!({"position":{"x":187.123456789_f64,"y":512.5},"alive":true,"quiet":[],"budget_ns":3600000000000_u64});
    let mut expected = serde_json::to_vec(&value).unwrap();
    expected.push(b'\n');
    let mut sink = Sink::default();
    append(&mut sink, value).unwrap();
    assert_eq!(sink.bytes, expected);
    assert_eq!((sink.writes, sink.flushes), (1, 1));
}
#[test]
fn community_campaign_wall_deadline_uses_latest_end_without_rounding() {
    assert!(within_window_wall(100, 630000000100));
    assert!(!within_window_wall(100, 630000000101));
    assert!(!within_window_wall(100, 99));
    assert!(!within_window_wall(0, 630000100000));
}

// This is an explicit acceptance campaign, not an hour added to every unit-test invocation.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "three separately retained 60-minute ordinary campaigns; requires declared seed and evidence root"]
async fn community_campaign_ordinary_account_lifetimes() -> Result<()> {
    community_pursuit_logging();
    let seed = std::env::var("COMMUNITY_CAMPAIGN_SEED")?.parse::<u64>()?;
    ensure!([7, 42, 101].contains(&seed), "undeclared campaign seed");
    let root = PathBuf::from(std::env::var("COMMUNITY_CAMPAIGN_ROOT")?);
    std::fs::create_dir(&root)?;
    let (dir, app) = fixture_with_realm_seed(Some(seed));
    app.world.clock.set_paused(true);
    let mut evidence = flate2::write::GzEncoder::new(
        std::fs::File::create(root.join("observations.jsonl.gz"))?,
        flate2::Compression::default(),
    );
    let mut writer_metrics = std::fs::File::create(root.join("writer-metrics.jsonl"))?;
    append(
        &mut evidence,
        serde_json::json!({"kind":"setup","seed":seed,"database":dir.join("game.db"),
        "wall_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis(),
        "structural_zero_human":"no server::serve, listener, testing registration or other entry producer; bots::start once",
        "seed_limit":"production realm_rng selection/name only; not World/dungeon/account Brain/async total determinism",
        "windows":6,"window_game_ms":600000,"window_wall_ms":630000,"observation_period_ms":500,
        "tick_stall_ns":1000000000_u64,"missing_lifecycle_grace_ms":5000,
        "tokio_runtime_flavor":format!("{:?}",tokio::runtime::Handle::current().runtime_flavor()),
        "tokio_runtime_workers":tokio::runtime::Handle::current().metrics().num_workers(),
        "available_parallelism":std::thread::available_parallelism()?.get(),
        "tokio_worker_threads_override":std::env::var_os("TOKIO_WORKER_THREADS").map(|s|format!("{s:?}")),
        "proc_status":std::fs::read_to_string("/proc/self/status")?,

        "initial":snapshot(&app).await?}),
    )?;
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let persistence_app = app.clone();
    let coordinator = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(50)) => persist_campaign(&persistence_app).await?,
                _ = stopped.changed() => return Ok::<(), anyhow::Error>(()),
            }
        }
    });
    let mut final_pause = None;
    let result: Result<()> = async {
        crate::bots::start(&app).await?;
        let accounts: BTreeSet<i32> = app.owners.lock().unwrap().keys().copied().collect();
        ensure!(accounts.len()==6, "ordinary start did not own exactly six accounts");
        let initial = snapshot(&app).await?;
        append(&mut evidence, serde_json::json!({"kind":"enrolled_paused","accounts":accounts,"snapshot":initial}))?;
        let mut characters = BTreeSet::new();
        let mut missing = std::collections::BTreeMap::new();
        let begin = app.world.clock.now();
        let wall_begin = std::time::Instant::now();
        let clock_origin_ns = begin.duration_since(app.started).as_nanos();
        let wall_origin_ns = wall_begin.duration_since(app.started).as_nanos();
        app.world.clock.set_paused(false);
        let remaining = Duration::from_millis(3600000).checked_sub(app.world.clock.now().duration_since(begin))
            .context("campaign horizon elapsed before scheduling final pause")?;
        let deadline = tokio::time::Instant::now() + remaining;
        let clock = app.world.clock.clone();
        final_pause = Some(tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            clock.set_paused(true);
            (clock.now().duration_since(begin).as_nanos(), wall_begin.elapsed().as_nanos())
        }));
        append(&mut evidence, serde_json::json!({"kind":"resume","active_ms":0,"wall_ms":0,
            "clock_origin_debug":format!("{begin:?}"),"wall_origin_debug":format!("{wall_begin:?}"),
            "clock_origin_uptime_ns":clock_origin_ns,"wall_origin_uptime_ns":wall_origin_ns,
            "time_precision":"ns fields are authoritative; integer ms fields are floor summaries; ticks*50 are separate simulation time",
            "wall_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis(),
            "logical_intervals_ms":[[0,600000],[600000,1200000],
            [1200000,1800000],[1800000,2400000],[2400000,3000000],[3000000,3600000]],
            "physical_stop_tail_limit_ms":1000,"tail_does_not_qualify_as_behavior":true}))?;
        let mut cadence = tokio::time::interval(Duration::from_millis(500));
        cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut prior_poll = std::time::Instant::now();
        let mut prior_observed_ns = wall_origin_ns;
        let mut tick_health = TickHealth { tick: 0, changed_ns: wall_origin_ns };
        let mut prior_active = 0;
        let mut prior_wall_ns = 0;
        let mut earliest_wall_start_ns = 0;
        for window in 0..6_u64 {
            let target = (window+1)*600000;
            let deadline = tokio::time::Instant::from_std(wall_begin + Duration::from_nanos(u64::try_from(earliest_wall_start_ns)?))
                + Duration::from_millis(630000);
            tokio::time::timeout_at(deadline, async {
            loop {
                cadence.tick().await;
                let poll = std::time::Instant::now();
                let gap = poll.duration_since(prior_poll).as_millis();
                prior_poll = poll;
                let view = tokio::time::timeout(Duration::from_millis(900),snapshot(&app)).await??;
                let observed_ns = view["observed_wall_uptime_ns"].as_u64().context("observation wall timestamp missing")? as u128;
                let observation_gap_ns = observed_ns.checked_sub(prior_observed_ns).context("observation wall timestamp regressed")?;
                prior_observed_ns=observed_ns;
                let active_ns = (view["observed_clock_uptime_ns"].as_u64().context("observation Clock timestamp missing")? as u128)
                    .checked_sub(clock_origin_ns).context("observation Clock timestamp regressed")?;
                let active = active_ns/1000000;
                let wall_ns = observed_ns.checked_sub(wall_origin_ns).context("observation precedes wall origin")?;
                let wall = wall_ns/1000000;
                let returned_ns=app.started.elapsed().as_nanos();
                let writing=std::time::Instant::now();
                append(&mut evidence, serde_json::json!({"kind":"observation","window":window+1,
                    "clock_debug":format!("{:?}",app.world.clock.now()),"active_ms":active,"active_ns":active_ns,
                    "qualifying_interval":active_ns<3600000000000,
                    "observation_end_active_ms":app.world.clock.now().duration_since(begin).as_millis(),
                    "wall_ms":wall,"request_gap_ms":gap,"observation_gap_ns":observation_gap_ns,"snapshot":view}))?;
                writeln!(writer_metrics,"{}",serde_json::json!({"active_ms":active,
                    "queue_to_capture_ns":observed_ns-poll.duration_since(app.started).as_nanos(),
                    "capture_to_return_ns":returned_ns-observed_ns,"record_write_ns":writing.elapsed().as_nanos(),
                    "compressed_bytes":evidence.get_ref().metadata()?.len()}))?;
                writer_metrics.flush()?;
                ensure!(observation_gap_ns<=1000000000,"observation coverage exceeded 1000ms: {observation_gap_ns}ns");
                ensure!(!coordinator.is_finished(),"persistence coordinator ended");
                ensure!(!app.world.clock.paused() || app.world.clock.now().duration_since(begin).as_millis()>=3600000,
                    "unexpected early campaign Clock pause");
                ensure!(view["quarantined"]==0,"controller quarantined");
                ensure!(view["health"]["players"]==0 && view["health"]["qa_actors"]==0,"Human/Qa invalidated campaign");
                let owners: BTreeSet<i32> = serde_json::from_value(view["owners"].clone())?;
                ensure!(owners==accounts,"lost or unrecognized account controller");
                let mut present = BTreeSet::new();
                for player in view["players"].as_array().context("players missing")? {
                    let account = i32::try_from(player["session"]["account"].as_i64().context("account missing")?)?;
                    ensure!(accounts.contains(&account) && present.insert(account),"unrecognized/duplicate account entry");
                    ensure!(player["controller"]=="Autonomous","unexpected controller");
                    ensure!(player["lifecycle"]["quarantined"]==false,"quarantined player");
                    ensure!(player["lifecycle"]["output"]=="live","lost controller output");
                    let character = i32::try_from(player["session"]["character"].as_i64().context("character missing")?)?;
                    characters.insert((account,character));
                    missing.remove(&account);
                }
                for account in accounts.difference(&present) {
                    let since = missing.entry(*account).or_insert_with(std::time::Instant::now);
                    ensure!(since.elapsed()<Duration::from_secs(5),"unexplained lifecycle gap for account {account}");
                }
                let tick = view["health"]["tick"].as_u64().context("tick missing")?;
                tick_health.observe(tick, observed_ns)?;
                ensure!(within_window_wall(earliest_wall_start_ns,wall_begin.elapsed().as_nanos()),"window wall deadline exceeded");
                if active>=target as u128 {
                    let mut durable = Vec::new();
                    for account in &accounts {
                        durable.push(serde_json::json!({"account":account,"mind":app.store.bot_mind(*account).await?,
                            "vault":app.store.vault(*account, 0).await?}));
                    }
                    append(&mut evidence, serde_json::json!({"kind":"window_checkpoint","window":window+1,
                        "logical_end_active_ms":target,"active_bracket_ms":[prior_active,active],
                        "wall_bracket_ns":[prior_wall_ns,wall_ns],"active_ms":active,
                        "earliest_wall_start_ns":earliest_wall_start_ns,"checkpoint_latest_wall_ns":wall_begin.elapsed().as_nanos(),
                        "tick":tick,"durable":durable}))?;
                    ensure!(within_window_wall(earliest_wall_start_ns,wall_begin.elapsed().as_nanos()),"window checkpoint wall deadline exceeded");
                    earliest_wall_start_ns=prior_wall_ns;
                    prior_active=active;
                    prior_wall_ns=wall_ns;
                    break;
                }
                prior_active=active;
                prior_wall_ns=wall_ns;
            }
            Ok::<(),anyhow::Error>(())
            }).await.context("window wall deadline exceeded")??;
        }
        let (active_ns, wall_ns) = final_pause.take().context("final pause timer missing")?.await?;
        append(&mut evidence, serde_json::json!({"kind":"pause","active_ns":active_ns,"active_ms":active_ns/1000000,
            "wall_ns":wall_ns,"wall_ms":wall_ns/1000000,"reason":"campaign complete; excluded evaluator reconciliation"}))?;
        ensure!((3600000000000..=3601000000000).contains(&active_ns),"physical final pause outside approved precision bound");
        for (account,character) in characters {
            match app.store.load(account,character).await {
                Ok(ch) => append(&mut evidence,serde_json::json!({"kind":"living_store","character":ch}))?,
                Err(error) => {
                    let fame=app.store.fame(account,character).await?;
                    append(&mut evidence,serde_json::json!({"kind":"dead_unloadable","account":account,
                        "character":character,"load_error":error.to_string(),"fame":fame}))?;
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = &result {
        app.world.clock.set_paused(true);
        eprintln!(
            "PRIMARY ordinary campaign seed={seed} failure; Clock paused before cleanup: {error:#}"
        );
    }
    if let Some(timer) = final_pause {
        timer.abort();
    }
    app.shutdown.send_replace(true);
    let shutdown_deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !app.owners.lock().unwrap().is_empty() && std::time::Instant::now() < shutdown_deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stop.send_replace(true);
    let coordinator_result = coordinator.await?;
    let final_state = snapshot(&app).await?;
    append(
        &mut evidence,
        serde_json::json!({"kind":"terminal","result":result.as_ref().err().map(|e|format!("{e:#}")),
        "coordinator_error":coordinator_result.as_ref().err().map(|e|format!("{e:#}")),"paused":app.world.clock.paused(),"snapshot":final_state}),
    )?;
    evidence.try_finish()?;
    evidence.get_ref().sync_all()?;
    writer_metrics.sync_all()?;
    println!(
        "ordinary campaign seed={seed} evidence={} database={}",
        root.display(),
        dir.join("game.db").display()
    );
    result?;
    coordinator_result?;
    ensure!(
        app.owners.lock().unwrap().is_empty(),
        "shutdown ownership not released"
    );
    ensure!(
        final_state["players"]
            .as_array()
            .context("final players missing")?
            .is_empty(),
        "shutdown players retained"
    );
    ensure!(
        app.quarantined.lock().unwrap().is_empty(),
        "shutdown quarantine retained"
    );
    Ok(())
}
