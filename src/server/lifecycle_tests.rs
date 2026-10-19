use super::*;

async fn player(a: &App, name: &str) -> (Owner, i32, crate::store::Character) {
    let account = a.store.list(name.into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let snapshot = ch.clone();
    let presentation = a.store.presentation(account).await.unwrap();
    let owner = a.claim_session(account).unwrap();
    let bind = lifecycle::binding(a, account).unwrap();
    let id = a
        .world
        .call(move |w| {
            let id = w.enter_at(ch, None, None, presentation)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    (owner, id, snapshot)
}

async fn idle(a: &App) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while lifecycle::outstanding(a) != 0
            || lifecycle::live_tasks() != 0
            || a.store.lifecycle_test_jobs() != 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owned work did not settle");
}

fn swap_slots(id: i32, ch: &crate::store::Character) -> (Slot, Slot) {
    (
        Slot {
            object: id,
            slot: 4,
            kind: ch.items[4],
        },
        Slot {
            object: id,
            slot: 5,
            kind: ch.items[5],
        },
    )
}

#[tokio::test]
async fn lifecycle_owned_dropped_loaded_owner_does_not_release_admission() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = player(&a, "owned-drop").await;
    drop(owner);
    assert!(
        a.claim(ch.account).is_err(),
        "unacknowledged avatar lost its lease"
    );
    depart(&a, id).await.unwrap();
    assert!(!a.owners.lock().unwrap().contains_key(&ch.account));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_owned_cancel_at_each_boundary_publishes_once_then_retires() {
    let (dir, a) = account_actions::tests::fixture();
    for checkpoint in [
        "before_reservation",
        "after_reservation",
        "after_commit",
        "before_publication",
        "before_release",
    ] {
        let (owner, id, ch) = player(&a, checkpoint).await;
        let (from, to) = swap_slots(id, &ch);
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        let (done, completed) = tokio::sync::oneshot::channel();
        let app = a.clone();
        let caller = tokio::spawn(async move {
            lifecycle::player(&app, id, "boundary_swap", move |a| async move {
                let pause = async move {
                    entered.send(()).unwrap();
                    wait.await.unwrap();
                };
                tokio::pin!(pause);
                if checkpoint == "before_reservation" {
                    pause.as_mut().await;
                }
                lifecycle::stage(Stage::Reservation);
                let plan = a
                    .world
                    .for_player(id)
                    .call(move |w| w.swap(id, from, to))
                    .await?;
                if checkpoint == "after_reservation" {
                    pause.as_mut().await;
                }
                lifecycle::stage(Stage::Store);
                let result = a
                    .store
                    .exchange(
                        vec![InventoryWrite {
                            id: plan.character,
                            account: plan.account,
                            revision: plan.revision,
                            items: plan.items.clone(),
                        }],
                        Vec::new(),
                    )
                    .await
                    .and_then(|v| v.into_iter().next().context("missing committed revision"));
                if checkpoint == "after_commit" || checkpoint == "before_publication" {
                    pause.as_mut().await;
                }
                lifecycle::stage(Stage::Publication);
                a.world
                    .for_player(id)
                    .call(move |w| w.finish_inventory(plan, result))
                    .await?;
                if checkpoint == "before_release" {
                    pause.as_mut().await;
                }
                done.send(()).unwrap();
                Ok(())
            })
            .await
        });
        ready.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert!(
            a.claim(ch.account).is_err(),
            "{checkpoint}: early account release"
        );
        assert_eq!(lifecycle::outstanding(&a), 1, "{checkpoint}");
        owner.release();
        assert!(
            a.claim(ch.account).is_err(),
            "{checkpoint}: release bypassed accepted completion"
        );
        release.send(()).unwrap();
        completed.await.unwrap();
        idle(&a).await;
        let mut expected = ch.items.clone();
        expected.swap(4, 5);
        let saved = a.store.load(ch.account, ch.id).await.unwrap();
        assert_eq!(
            (saved.items, saved.revision),
            (expected.clone(), ch.revision + 1),
            "{checkpoint}"
        );
        assert_eq!(
            a.world
                .for_player(id)
                .call(move |w| {
                    let p = &w.players[&id];
                    Ok((p.pending, p.character.items.clone(), p.character.revision))
                })
                .await
                .unwrap(),
            (false, expected, ch.revision + 1),
            "{checkpoint}"
        );
        assert!(
            a.claim(ch.account).is_err(),
            "{checkpoint}: retirement not yet acknowledged"
        );
        depart(&a, id).await.unwrap();
        assert!(
            a.owners.lock().unwrap().is_empty(),
            "{checkpoint}: successful completion leaked a claim"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_owned_sql_failure_lost_replies_and_task_panic_fence_only_affected_account() {
    let (dir, a) = account_actions::tests::fixture();
    let (healthy_owner, healthy, healthy_ch) = player(&a, "healthy-sibling").await;
    let mut healthy_ch = healthy_ch;
    for fault in [
        "sql",
        "store_reply",
        "world_reservation_reply",
        "world_publication_reply",
        "panic_after_reservation",
        "panic_after_commit",
    ] {
        let (owner, id, ch) = player(&a, fault).await;
        let (from, to) = swap_slots(id, &ch);
        if fault == "sql" {
            let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
            db.execute_batch(&format!("CREATE TRIGGER lifecycle_failure BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'lifecycle injected SQL write failure'); END;", ch.account)).unwrap();
        }
        let result = lifecycle::player(&a, id, "injected_swap", move |a| async move {
            lifecycle::stage(Stage::Reservation);
            if fault == "world_reservation_reply" {
                a.world.lifecycle_test_lose_reply();
            }
            let plan = a
                .world
                .for_player(id)
                .call(move |w| w.swap(id, from, to))
                .await?;
            if fault == "panic_after_reservation" {
                panic!("lifecycle injected completion panic after reservation");
            }
            lifecycle::stage(Stage::Store);
            if fault == "store_reply" {
                a.store.lifecycle_test_lose_reply();
            }
            let result = a
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: plan.character,
                        account: plan.account,
                        revision: plan.revision,
                        items: plan.items.clone(),
                    }],
                    Vec::new(),
                )
                .await
                .and_then(|v| v.into_iter().next().context("missing committed revision"));
            if fault == "panic_after_commit" {
                assert!(result.is_ok());
                panic!("lifecycle injected completion panic after commit");
            }
            lifecycle::stage(Stage::Publication);
            if fault == "world_publication_reply" {
                a.world.lifecycle_test_lose_reply();
            }
            a.world
                .for_player(id)
                .call(move |w| w.finish_inventory(plan, result))
                .await
        })
        .await;
        let error = result.unwrap_err();
        assert!(
            if fault == "sql" {
                format!("{error:#}").contains("lifecycle injected SQL write failure")
            } else if fault.starts_with("panic") {
                error.to_string().contains("completion task panicked")
            } else {
                format!("{error:#}").contains("worker failed")
            },
            "{fault}: {error:#}"
        );
        drop(owner);
        idle(&a).await;
        assert_eq!(
            lifecycle::state(&a, ch.account),
            Some((false, Some(id), true)),
            "{fault}"
        );
        assert!(a.claim(ch.account).is_err(), "{fault}");
        assert!(
            depart(&a, id).await.is_err(),
            "{fault}: uncertainty must not be cleared by a final save"
        );
        let saved = a.store.load(ch.account, ch.id).await.unwrap();
        let committed = matches!(
            fault,
            "store_reply" | "world_publication_reply" | "panic_after_commit"
        );
        assert_eq!(
            saved.revision,
            ch.revision + i32::from(committed),
            "{fault}: no fabricated success or replay"
        );
        let (from, to) = swap_slots(healthy, &healthy_ch);
        inventory(&a, healthy, move |w| w.swap(healthy, from, to).map(Some))
            .await
            .unwrap()
            .unwrap();
        healthy_ch = a
            .store
            .load(healthy_ch.account, healthy_ch.id)
            .await
            .unwrap();
        assert!(lifecycle::state(&a, healthy_ch.account).is_some_and(|(_, _, fenced)| !fenced));
        if fault == "sql" {
            rusqlite::Connection::open(dir.join("game.db"))
                .unwrap()
                .execute_batch("DROP TRIGGER lifecycle_failure")
                .unwrap();
        }
    }
    assert_eq!(healthy_ch.revision, 6);
    depart(&a, healthy).await.unwrap();
    healthy_owner.release();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_owned_repeated_cancelled_short_callers_leave_finite_jobs_and_no_claims() {
    let (dir, a) = account_actions::tests::fixture();
    let account = a.store.list("repeated-short".into()).await.unwrap().id;
    for _ in 0..24 {
        let owner = a.claim(account).unwrap();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        let (done, completed) = tokio::sync::oneshot::channel();
        let app = a.clone();
        let caller = tokio::spawn(async move {
            lifecycle::account(&app, vec![account], "short_counter", move |a| async move {
                entered.send(()).unwrap();
                wait.await.unwrap();
                lifecycle::stage(Stage::Store);
                a.store
                    .lifecycle_test_job(move |db| {
                        db.execute(
                            "UPDATE accounts SET credits=credits+1 WHERE id=?1",
                            [account],
                        )?;
                        Ok(())
                    })
                    .await?;
                done.send(()).unwrap();
                Ok(())
            })
            .await
        });
        ready.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        drop(owner);
        assert_eq!(lifecycle::outstanding(&a), 1);
        assert_eq!(lifecycle::live_tasks(), 1);
        assert_eq!(lifecycle::state(&a, account), Some((true, None, false)));
        assert_eq!(a.store.lifecycle_test_jobs(), 0);
        assert!(a.claim(account).is_err());
        release.send(()).unwrap();
        completed.await.unwrap();
        idle(&a).await;
        assert!(a.owners.lock().unwrap().is_empty());
    }
    assert_eq!(a.store.presentation(account).await.unwrap().credits, 124);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_owned_stale_generation_cannot_retire_or_release_replacement() {
    let (dir, a) = account_actions::tests::fixture();
    let account = a.store.list("stale-generation".into()).await.unwrap().id;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let presentation = a.store.presentation(account).await.unwrap();
    let player = a
        .world
        .call(move |w| w.enter_at(ch, None, None, presentation))
        .await
        .unwrap();
    let old = a.claim_session(account).unwrap();
    old.bind(player).unwrap();
    let old_binding = binding(&a, account).unwrap();
    let old_retirement = retirement(&a, account, player).unwrap();
    let mut old_guard =
        reserve(&a, &generations(&a, &[account]).unwrap(), "old_completion").unwrap();
    // Reproduce the legacy registry's early removal while an old completion still exists.
    a.owners.lock().unwrap().remove(&account);
    let stale = old_guard.accounts.clone();
    let replacement = a.claim_session(account).unwrap();
    replacement.bind(player).unwrap();
    let guard = reserve(
        &a,
        &generations(&a, &[account]).unwrap(),
        "replacement_completion",
    )
    .unwrap();
    assert!(
        old_binding(player)
            .unwrap_err()
            .to_string()
            .contains("stale session generation")
    );
    assert!(
        old_retirement()
            .unwrap_err()
            .to_string()
            .contains("stale retirement generation")
    );
    assert!(
        reserve(&a, &stale, "stale_enqueue")
            .err()
            .unwrap()
            .to_string()
            .contains("stale operation generation")
    );
    old_guard.completed = true;
    drop(old_guard);
    drop(old);
    assert_eq!(state(&a, account), Some((true, Some(player), false)));
    complete(guard, |_| async { Ok(()) }).await.unwrap();
    depart(&a, player).await.unwrap();
    replacement.release();
    assert!(a.owners.lock().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[derive(Clone)]
struct FailingSink {
    target: &'static str,
    reports: Arc<Mutex<Vec<String>>>,
    session: Option<String>,
}
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for FailingSink {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if !event.metadata().target().ends_with(self.target)
            || *event.metadata().level() != tracing::Level::ERROR
        {
            return;
        }
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                write!(&mut self.0, "{}={value:?};", field.name()).unwrap();
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        if self
            .session
            .as_ref()
            .is_some_and(|session| !fields.0.contains(session))
        {
            return;
        }
        let mut reports = self.reports.lock().unwrap();
        if !reports.is_empty() {
            return;
        }
        reports.push(fields.0);
        drop(reports);
        panic!("injected diagnostic sink failure");
    }
}

#[tokio::test]
async fn lifecycle_owned_diagnostic_sink_panic_during_uncertain_world_sqlite_handoff() {
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::layer::SubscriberExt;
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = player(&a, "sink-failure").await;
    let (healthy_owner, healthy, healthy_ch) = player(&a, "sink-healthy").await;
    let reports = Arc::new(Mutex::new(Vec::new()));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(FailingSink {
        target: "lifecycle",
        reports: reports.clone(),
        session: None,
    }));
    let (from, to) = swap_slots(id, &ch);
    let result = lifecycle::account(&a, vec![ch.account], "sink_handoff", move |a| async move {
        stage(Stage::Reservation);
        let plan = a
            .world
            .for_player(id)
            .call(move |w| w.swap(id, from, to))
            .await?;
        stage(Stage::Store);
        let revisions = a
            .store
            .exchange(
                vec![InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                }],
                vec![],
            )
            .await?;
        stage(Stage::Publication);
        a.world.lifecycle_test_lose_reply();
        a.world
            .for_player(id)
            .call(move |w| w.finish_inventory(plan, Ok(revisions[0])))
            .await
    })
    .with_subscriber(dispatch)
    .await;
    assert!(
        format!("{:#}", result.unwrap_err()).contains("world directory worker failed"),
        "diagnostic failure replaced the underlying uncertainty"
    );
    idle(&a).await;
    let failure = {
        let claim = a.owners.lock().unwrap();
        claim[&ch.account]
            .failure
            .clone()
            .expect("sink failure erased the original fence")
    };
    for field in [
        format!("account={}", ch.account),
        "session=".into(),
        "operation=sink_handoff".into(),
        "stage=publication".into(),
        "world directory worker failed".into(),
    ] {
        assert!(failure.contains(&field), "missing {field}: {failure}");
    }
    assert_eq!(reports.lock().unwrap().len(), 1);
    assert!(reports.lock().unwrap()[0].contains(&failure));
    drop(owner);
    assert!(a.claim(ch.account).is_err());
    assert!(depart(&a, id).await.is_err());
    assert_eq!(state(&a, ch.account), Some((false, Some(id), true)));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let mut expected = ch.items.clone();
    expected.swap(4, 5);
    assert_eq!(
        db.query_row(
            "SELECT revision,items FROM characters WHERE id=?1",
            [ch.id],
            |r| Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        (ch.revision + 1, serde_json::to_string(&expected).unwrap())
    );
    assert_eq!(
        a.world
            .for_player(id)
            .call(move |w| Ok((
                w.players[&id].pending,
                w.players[&id].character.items.clone(),
                w.players[&id].character.revision
            )))
            .await
            .unwrap(),
        (false, expected, ch.revision + 1)
    );
    let (from, to) = swap_slots(healthy, &healthy_ch);
    inventory(&a, healthy, move |w| w.swap(healthy, from, to).map(Some))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        a.store
            .load(healthy_ch.account, healthy_ch.id)
            .await
            .unwrap()
            .revision,
        healthy_ch.revision + 1
    );
    depart(&a, healthy).await.unwrap();
    drop(healthy_owner);
    assert!(a.claim(healthy_ch.account).is_ok());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_owned_diagnostic_sink_panic_during_uncertain_collector_handoff() {
    use tracing_subscriber::layer::SubscriberExt;
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = player(&a, "collector-sink-failure").await;
    let (healthy_owner, healthy, healthy_ch) = player(&a, "collector-sink-healthy").await;
    a.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().experience_queued = 50;
            Ok(())
        })
        .await
        .unwrap();
    let reports = Arc::new(Mutex::new(Vec::new()));
    let session = format!(
        "session={:p}",
        Arc::as_ptr(&a.owners.lock().unwrap()[&ch.account].generation)
    );
    // The response is dropped on the already-running World worker, outside caller task scopes.
    // This one-shot sink matches only this generation and remains inert after its first report.
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(FailingSink {
        target: "lifecycle",
        reports: reports.clone(),
        session: Some(session.clone()),
    }))
    .unwrap();
    a.world.lifecycle_test_lose_reply();
    let error = persistence_tick(&a).await.unwrap_err();
    assert!(format!("{error:#}").contains("world directory worker failed"));
    assert_eq!(
        reports.lock().unwrap().len(),
        1,
        "real World-worker reporting sink was not exercised"
    );
    let failure = a.owners.lock().unwrap()[&ch.account]
        .failure
        .clone()
        .unwrap();
    for field in [
        format!("account={}", ch.account),
        session,
        "operation=progress".into(),
        "stage=reservation".into(),
        "completion task abandoned".into(),
    ] {
        assert!(failure.contains(&field), "missing {field}: {failure}");
    }
    // A failed diagnostic must not kill the worker that still owns unrelated sessions.
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.pending && p.progress_started);
            assert_eq!(p.character.exp, 0);
            Ok(())
        })
        .await
        .expect("diagnostic sink panic killed the authoritative World worker");
    drop(owner);
    assert!(a.claim(ch.account).is_err());
    assert!(depart(&a, id).await.is_err());
    assert_eq!(state(&a, ch.account), Some((false, Some(id), true)));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let stats: String = db
        .query_row("SELECT stats FROM characters WHERE id=?1", [ch.id], |r| {
            r.get(0)
        })
        .unwrap();
    let saved: crate::store::Character = serde_json::from_str(&stats).unwrap();
    assert_eq!((saved.exp, saved.revision), (ch.exp, ch.revision));
    let (from, to) = swap_slots(healthy, &healthy_ch);
    inventory(&a, healthy, move |w| w.swap(healthy, from, to).map(Some))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM characters WHERE id=?1",
            [healthy_ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        healthy_ch.revision + 1
    );
    assert_eq!(
        a.world
            .for_player(healthy)
            .call(move |w| Ok(w.players[&healthy].character.revision))
            .await
            .unwrap(),
        healthy_ch.revision + 1
    );
    depart(&a, healthy).await.unwrap();
    drop(healthy_owner);
    assert!(a.claim(healthy_ch.account).is_ok());
    idle(&a).await;
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn lifecycle_owned_quarantine_sql_sink_failure_keeps_world_and_original_cause() {
    use tracing_subscriber::layer::SubscriberExt;
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = player(&a, "quarantine-sink-failure").await;
    let (healthy_owner, healthy, healthy_ch) = player(&a, "quarantine-sink-healthy").await;
    let (packets, _rx) = mpsc::channel(64);
    let (lost, loss) = tokio::sync::watch::channel(false);
    a.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().tx = Some(crate::world::Output { packets, lost });
            Ok(())
        })
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch(&format!("CREATE TRIGGER quarantine_sink_sql BEFORE UPDATE ON characters WHEN OLD.account={} BEGIN SELECT RAISE(ABORT,'quarantine SQL diagnostic failure'); END;", ch.account)).unwrap();
    let reports = Arc::new(Mutex::new(Vec::new()));
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(FailingSink {
        target: "world",
        reports: reports.clone(),
        session: None,
    }));
    let (from, to) = swap_slots(id, &ch);
    let result = lifecycle::account(
        &a,
        vec![ch.account],
        "sql_sink_publication",
        move |a| async move {
            stage(Stage::Reservation);
            let plan = a
                .world
                .for_player(id)
                .call(move |w| w.swap(id, from, to))
                .await?;
            stage(Stage::Store);
            let result = a
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: plan.character,
                        account: plan.account,
                        revision: plan.revision,
                        items: plan.items.clone(),
                    }],
                    vec![],
                )
                .await
                .and_then(|v| v.into_iter().next().context("missing revision"));
            stage(Stage::Publication);
            a.world
                .for_player(id)
                .call(move |w| {
                    tracing::dispatcher::with_default(&dispatch, || {
                        w.finish_inventory(plan, result)
                    })
                })
                .await
        },
    )
    .await;
    assert_eq!(
        reports.lock().unwrap().len(),
        1,
        "actual World quarantine diagnostic was not exercised"
    );
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.pending);
            let error = w.begin_departure(id).unwrap_err();
            assert!(format!("{error:#}").contains("quarantine SQL diagnostic failure"));
            Ok(())
        })
        .await
        .expect("quarantine diagnostic sink panic killed the World worker");
    assert!(
        *loss.borrow(),
        "quarantine did not notify the actual session output before reporting"
    );
    assert!(
        format!("{:#}", result.unwrap_err()).contains("quarantine SQL diagnostic failure"),
        "diagnostic failure replaced the original SQL error"
    );
    let failure = a.owners.lock().unwrap()[&ch.account]
        .failure
        .clone()
        .unwrap();
    for field in [
        format!("account={}", ch.account),
        "session=".into(),
        "operation=sql_sink_publication".into(),
        "stage=publication".into(),
        "quarantine SQL diagnostic failure".into(),
    ] {
        assert!(failure.contains(&field), "missing {field}: {failure}");
    }
    assert!(reports.lock().unwrap()[0].contains("quarantine SQL diagnostic failure"));
    drop(owner);
    assert_eq!(state(&a, ch.account), Some((false, Some(id), true)));
    assert!(a.claim(ch.account).is_err());
    assert!(depart(&a, id).await.is_err());
    assert_eq!(
        db.query_row(
            "SELECT revision,items FROM characters WHERE id=?1",
            [ch.id],
            |r| Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        (ch.revision, serde_json::to_string(&ch.items).unwrap())
    );
    assert_eq!(
        a.world
            .for_player(id)
            .call(move |w| Ok((
                w.players[&id].character.revision,
                w.players[&id].character.items.clone()
            )))
            .await
            .unwrap(),
        (ch.revision, ch.items)
    );
    let (from, to) = swap_slots(healthy, &healthy_ch);
    inventory(&a, healthy, move |w| w.swap(healthy, from, to).map(Some))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT revision FROM characters WHERE id=?1",
            [healthy_ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        healthy_ch.revision + 1
    );
    assert_eq!(
        a.world
            .for_player(healthy)
            .call(move |w| Ok(w.players[&healthy].character.revision))
            .await
            .unwrap(),
        healthy_ch.revision + 1
    );
    depart(&a, healthy).await.unwrap();
    drop(healthy_owner);
    assert!(a.claim(healthy_ch.account).is_ok());
    idle(&a).await;
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_finalize_stale_fenced_completion_cannot_stop_replacement_generation() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = player(&a, "stale-fenced-stop").await;
    let (packets, _rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    a.world
        .for_player(id)
        .call(move |w| {
            let p = w.players.get_mut(&id).unwrap();
            p.bot_goal = Some(p.pos);
            p.bot_aim = Some(0.);
            p.tx = Some(crate::world::Output { packets, lost });
            Ok(())
        })
        .await
        .unwrap();
    owner.failed("original stale close".into());
    let (ready, release) = a.world.lifecycle_test_pause_job(1, true);
    let close = owner.close(&a, CloseReason::Internal).unwrap().unwrap();
    ready.await.unwrap();
    a.owners.lock().unwrap().remove(&ch.account);
    let replacement = a.claim_session(ch.account).unwrap();
    replacement.bind(id).unwrap();
    release.send(()).unwrap();
    let error = close.wait().await.unwrap_err().to_string();
    assert!(
        error.contains("stale controller-stop generation")
            && error.contains("controller_stop=unverified")
    );
    assert_eq!(state(&a, ch.account), Some((false, Some(id), false)));
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(!p.departing && p.bot_goal.is_some() && p.bot_aim.is_some() && p.tx.is_some());
            Ok(())
        })
        .await
        .unwrap();
    depart(&a, id).await.unwrap();
    replacement.release();
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn lifecycle_finalize_stale_avatar_stop_cannot_freeze_rebound_avatar() {
    let (dir, a) = account_actions::tests::fixture();
    let (owner, id, ch) = player(&a, "stale-avatar-stop").await;
    let generation = owner.generation.clone();
    depart(&a, id).await.unwrap();
    let saved = a.store.load(ch.account, ch.id).await.unwrap();
    let bind = binding(&a, ch.account).unwrap();
    let replacement = a
        .world
        .call(move |w| {
            let id = w.enter(saved, None)?;
            bind(id)?;
            w.players.get_mut(&id).unwrap().bot_goal = Some(w.players[&id].pos);
            Ok(id)
        })
        .await
        .unwrap();
    assert!(
        stop_owned_avatar(&a, ch.account, &generation, id)
            .await
            .unwrap_err()
            .to_string()
            .contains("stale controller-stop avatar")
    );
    a.world
        .for_player(replacement)
        .call(move |w| {
            assert!(
                !w.players[&replacement].departing && w.players[&replacement].bot_goal.is_some()
            );
            Ok(())
        })
        .await
        .unwrap();
    depart(&a, replacement).await.unwrap();
    owner.release();
    std::fs::remove_dir_all(dir).unwrap();
}
