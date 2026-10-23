use super::*;

struct NativePauseFixture {
    dir: PathBuf,
    app: App,
    account: i32,
    actor: i32,
    player: i32,
    owner: Option<lifecycle::Owner>,
    _output: tokio::sync::mpsc::Receiver<Packet>,
}
impl NativePauseFixture {
    async fn new() -> Self {
        let (dir, app) = fixture();
        let pos = app
            .world
            .in_world(-1)
            .call(|w| {
                let (x, y) = w.map.spawns[0];
                Ok(Pos { x, y })
            })
            .await
            .unwrap();
        let mut spawn = spec(pos);
        spawn["controller"] = "native".into();
        let created = request(
            &app,
            "native-pause-fixture",
            json!({"op":"spawn","spec":spawn}),
        )
        .await;
        assert_eq!(created["status"], "succeeded");
        let actor = created["result"]["actor"].as_i64().unwrap() as i32;
        let account = created["result"]["account"].as_i64().unwrap() as i32;
        let character = app.store.load(account, actor).await.unwrap();
        let owner = app.claim_session(account).unwrap();
        let (packets, output) = tokio::sync::mpsc::channel(256);
        let lost = tokio::sync::watch::channel(false).0;
        let player = app
            .world
            .in_world(-1)
            .call(move |w| {
                let id = w.enter_at(
                    character,
                    Some(crate::world::Output { packets, lost }),
                    Some(pos),
                    crate::store::AccountPresentation {
                        guild: None,
                        credits: 100,
                        fame: 0,
                        best_fame: Default::default(),
                    },
                )?;
                w.players.get_mut(&id).unwrap().native_update_acked = true;
                Ok(id)
            })
            .await
            .unwrap();
        owner.bind(player).unwrap();
        super::super::super::persistence_tick(&app).await.unwrap();
        Self {
            dir,
            app,
            account,
            actor,
            player,
            owner: Some(owner),
            _output: output,
        }
    }
    async fn ready(&self) -> Value {
        native_state(
            &self.app,
            self.app.testing.as_ref().unwrap(),
            vec![self.actor],
        )
        .await
        .unwrap()
    }
    async fn dirty(&self) {
        let player = self.player;
        self.app
            .world
            .for_player(player)
            .call(move |w| {
                w.players.get_mut(&player).unwrap().character.lifetime.tiles += 1;
                Ok(())
            })
            .await
            .unwrap();
    }
    async fn wait_progress(&self) {
        let player = self.player;
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if self
                    .app
                    .world
                    .for_player(player)
                    .call(move |w| {
                        let p = &w.players[&player];
                        Ok(p.pending && p.progress_started)
                    })
                    .await
                    .unwrap()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    async fn retire(mut self) {
        self.owner
            .take()
            .unwrap()
            .close(&self.app, lifecycle::CloseReason::Controller)
            .unwrap()
            .unwrap()
            .wait()
            .await
            .unwrap();
        self.app.shutdown.send_replace(true);
        fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn persistence_collector(
    app: App,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
) {
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stopped => return Ok(()),
                _ = tokio::time::sleep(Duration::from_millis(50)) => super::super::super::persistence_tick(&app).await?,
            }
        }
    });
    (stop, task)
}

#[tokio::test]
async fn native_pause_drains_real_progress_started_after_ready_and_newer_facts_while_paused() {
    for fail in [false, true] {
        let f = NativePauseFixture::new().await;
        assert_eq!(f.ready().await["actors"][0]["stage"], "ready");
        let before = f.app.store.load(f.account, f.actor).await.unwrap().lifetime;
        let db = rusqlite::Connection::open(f.dir.join("game.db")).unwrap();
        if fail {
            db.execute_batch("CREATE TRIGGER fail_native_pause_progress BEFORE UPDATE OF stats ON characters BEGIN SELECT RAISE(ABORT,'injected native pause progress failure'); END;").unwrap();
        }
        let (sql_entered, sql_ready) = tokio::sync::oneshot::channel();
        let (sql_release, sql_wait) = std::sync::mpsc::channel();
        let app = f.app.clone();
        // A worker barrier avoids SQLite's immediate failure when a deferred transaction upgrades a locked read.
        let sql_blocker = tokio::spawn(async move {
            app.store
                .lifecycle_test_job(move |_| {
                    let _ = sql_entered.send(());
                    sql_wait
                        .recv()
                        .context("native pause SQL barrier abandoned")?;
                    Ok(())
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), sql_ready)
            .await
            .unwrap()
            .unwrap();
        let (stored, release) = f.app.store.lifecycle_test_pause_reply();
        f.dirty().await;
        let (stop, collector) = persistence_collector(f.app.clone());
        f.wait_progress().await;
        assert_eq!(f.ready().await["actors"][0]["stage"], "busy");
        let player = f.player;
        f.app
            .world
            .for_player(player)
            .call(move |w| {
                // A newer monotone fact must survive publication of the already prepared snapshot.
                w.players
                    .get_mut(&player)
                    .unwrap()
                    .character
                    .lifetime
                    .ability_used = true;
                Ok(())
            })
            .await
            .unwrap();
        let paused = {
            f.app.world.clock.set_paused(false);
            let app = f.app.clone();
            let actor = f.actor;
            let player = f.player;
            let mut waiting =
                tokio::spawn(async move { pause(&app, &[actor], &[(actor, player)]).await });
            tokio::time::timeout(Duration::from_secs(2), async {
                while !f.app.world.clock.paused() {
                    assert!(
                        !waiting.is_finished(),
                        "native pause rejected ordinary progress before freezing"
                    );
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(
                !waiting.is_finished(),
                "native pause succeeded while SQL was blocked"
            );
            assert!(
                f.app.world.clock.paused(),
                "persistence must drain with simulation stopped"
            );
            let saved: String = db
                .query_row(
                    "SELECT stats FROM characters WHERE id=?",
                    [f.actor],
                    |row| row.get(0),
                )
                .unwrap();
            let saved: Character = serde_json::from_str(&saved).unwrap();
            assert_eq!(saved.lifetime, before);
            sql_release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), sql_blocker)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), stored)
                .await
                .unwrap()
                .unwrap();
            let saved: String = db
                .query_row(
                    "SELECT stats FROM characters WHERE id=?",
                    [f.actor],
                    |row| row.get(0),
                )
                .unwrap();
            let saved: Character = serde_json::from_str(&saved).unwrap();
            assert_eq!(saved.lifetime.tiles, before.tiles + u64::from(!fail));
            assert!(
                !saved.lifetime.ability_used,
                "the first immutable snapshot included a later fact"
            );
            assert!(
                !waiting.is_finished(),
                "native pause succeeded before publication acknowledged the committed snapshot"
            );
            assert!(f.app.world.clock.paused());
            release.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), &mut waiting)
                .await
                .unwrap()
                .unwrap()
        };
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), collector)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(f.app.world.clock.paused());
        let durable = f.app.store.load(f.account, f.actor).await.unwrap().lifetime;
        if fail {
            assert!(format!("{:#}", paused.unwrap_err()).contains("quarantined"));
            assert_eq!(durable, before);
            assert!(lifecycle::state(&f.app, f.account).unwrap().2);
            f.app.shutdown.send_replace(true);
            fs::remove_dir_all(&f.dir).unwrap();
        } else {
            paused.unwrap();
            assert_eq!(durable.tiles, before.tiles + 1);
            assert!(durable.ability_used);
            let current = f
                .app
                .world
                .for_player(player)
                .call(move |w| {
                    assert!(w.settled()?);
                    Ok(w.players[&player].character.lifetime.clone())
                })
                .await
                .unwrap();
            assert_eq!(current, durable);
            assert_eq!(f.ready().await["actors"][0]["stage"], "ready");
            f.retire().await;
        }
    }
}

#[tokio::test]
async fn native_pause_keeps_same_session_and_rejects_nonprogress_malformed_departure_death_and_quarantine()
 {
    for case in [
        "cohort",
        "session",
        "departing",
        "death",
        "nonprogress",
        "missing_snapshot",
        "changed_snapshot",
        "unreserved_owner",
        "quarantine",
    ] {
        let f = NativePauseFixture::new().await;
        let player = f.player;
        if matches!(case, "changed_snapshot" | "unreserved_owner") {
            f.dirty().await;
        }
        f.app
            .world
            .for_player(player)
            .call(move |w| {
                if matches!(case, "changed_snapshot" | "unreserved_owner") {
                    assert_eq!(w.pending_progress().len(), 1);
                }
                if case == "quarantine" {
                    w.poison(player, "injected native pause quarantine".into())?;
                } else {
                    let p = w.players.get_mut(&player).unwrap();
                    match case {
                        "departing" => p.departing = true,
                        "death" => p.death_pending = true,
                        "nonprogress" => p.pending = true,
                        "missing_snapshot" => {
                            p.pending = true;
                            p.progress_started = true;
                        }
                        "changed_snapshot" => p.character.revision += 1,
                        _ => {}
                    }
                }
                Ok(())
            })
            .await
            .unwrap();
        f.app.world.clock.set_paused(false);
        let cohort = if case == "cohort" {
            vec![]
        } else {
            vec![f.actor]
        };
        let session = if case == "session" {
            f.player + 1
        } else {
            f.player
        };
        let error = pause(&f.app, &cohort, &[(f.actor, session)])
            .await
            .unwrap_err();
        let error = format!("{error:#}");
        assert!(
            match case {
                "cohort" => error.contains("coordinated Ruffle pause"),
                "session" => error.contains("session changed"),
                "nonprogress" => error.contains("non-progress pending"),
                "missing_snapshot" => error.contains("snapshot missing"),
                "changed_snapshot" => error.contains("snapshot mismatch"),
                "unreserved_owner" => error.contains("progress owner"),
                _ => error.contains("not active"),
            },
            "{case}: {error}"
        );
        assert!(
            !f.app.world.clock.paused(),
            "rejected native reservation froze the clock: {case}"
        );
        f.owner
            .as_ref()
            .unwrap()
            .failed("expected native pause rejection fixture".into());
        f.app.shutdown.send_replace(true);
        fs::remove_dir_all(&f.dir).unwrap();
    }
}

#[tokio::test]
async fn native_pause_timeout_preserves_paused_state_and_owned_unacknowledged_commit() {
    let f = NativePauseFixture::new().await;
    assert_eq!(f.ready().await["actors"][0]["stage"], "ready");
    f.dirty().await;
    let (entered, release) = f.app.store.lifecycle_test_pause_reply();
    let app = f.app.clone();
    let collector = tokio::spawn(async move { super::super::super::persistence_tick(&app).await });
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    f.app.world.clock.set_paused(false);
    let error = tokio::time::timeout(
        Duration::from_secs(6),
        pause(&f.app, &[f.actor], &[(f.actor, f.player)]),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(format!("{error:#}").contains("persistence did not settle"));
    assert!(f.app.world.clock.paused());
    let player = f.player;
    f.app
        .world
        .for_player(player)
        .call(move |w| {
            let p = &w.players[&player];
            assert!(p.pending && p.progress_started);
            assert!(!w.settled()?);
            Ok(())
        })
        .await
        .unwrap();
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), collector)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(f.app.world.clock.paused());
    f.app
        .world
        .for_player(player)
        .call(|w| {
            assert!(w.settled()?);
            Ok(())
        })
        .await
        .unwrap();
    f.retire().await;
}
