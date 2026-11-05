use super::*;
use crate::world::Output;
use tokio::sync::watch;

#[test]
fn cancel_before_deadline_fences_the_exact_owner_and_cannot_be_replayed() {
    let mut service = Service::default();
    let now = Instant::now();
    assert!(service.status(now).is_err());
    let pending = service.begin("a".repeat(64), now).unwrap();
    assert!(service.begin("b".repeat(64), now).is_err());
    assert!(service.finish(&pending.receipt, false, now).is_err());
    let mut foreign = pending.receipt.clone();
    for field in [0, 1, 2] {
        match field {
            0 => foreign.instance = "b".repeat(64),
            1 => foreign.owner = "b".repeat(64),
            _ => foreign.deadline_ms += 1,
        }
        assert!(service.finish(&foreign, true, now).is_err());
        foreign = pending.receipt.clone();
    }
    let cancelled = service
        .finish(
            &pending.receipt,
            true,
            now + NOTICE - Duration::from_nanos(1),
        )
        .unwrap();
    assert_eq!(cancelled.state, "cancelled");
    assert!(service.finish(&pending.receipt, true, now).is_err());
    assert_eq!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .unwrap()
            .state,
        "cancelled"
    );
    assert!(service.begin("a".repeat(64), now + NOTICE).is_err());
    let next = service.begin("b".repeat(64), now + NOTICE).unwrap();
    assert!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .is_err()
    );
    assert_eq!(service.status(now + NOTICE).unwrap().receipt, next.receipt);
}

#[test]
fn deadline_and_consumption_are_irreversible_and_process_instances_are_distinct() {
    let now = Instant::now();
    let mut service = Service::default();
    let pending = service.begin("a".repeat(64), now).unwrap();
    assert!(service.status(now + NOTICE).is_err());
    assert!(
        service
            .finish(&pending.receipt, true, now + NOTICE)
            .is_err()
    );
    let mut replacement = Service::default();
    replacement.begin("a".repeat(64), now).unwrap();
    assert!(replacement.finish(&pending.receipt, true, now).is_err());
    assert_eq!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .unwrap()
            .state,
        "consumed"
    );
    assert!(
        service
            .finish(&pending.receipt, true, now + NOTICE)
            .is_err()
    );
    assert!(
        service
            .finish(&pending.receipt, false, now + NOTICE)
            .is_err()
    );
    assert!(service.begin("b".repeat(64), now + NOTICE).is_err());
}

#[test]
fn expired_pending_owner_can_be_replaced_but_never_consumed_by_a_stale_request() {
    let now = Instant::now();
    let mut service = Service::default();
    for owner in ["", "a", &"A".repeat(64), &"g".repeat(64)] {
        assert!(service.begin(owner.into(), now).is_err());
    }
    let stale = service.begin("a".repeat(64), now).unwrap();
    service.begin("b".repeat(64), now + NOTICE).unwrap();
    assert!(service.finish(&stale.receipt, false, now + NOTICE).is_err());
}

#[test]
fn typed_update_intent_is_emitted_only_at_the_first_valid_stop_consumption() {
    let now = Instant::now();
    let mut service = Service::default();
    let pending = service.begin("a".repeat(64), now).unwrap();
    assert!(service.consume(&pending.receipt, now).is_err());
    let mut foreign = pending.receipt.clone();
    foreign.instance = "f".repeat(64);
    assert!(service.consume(&foreign, now + NOTICE).is_err());
    let (status, update) = service.consume(&pending.receipt, now + NOTICE).unwrap();
    assert_eq!(status.state, "consumed");
    assert_eq!(update.unwrap().operation, pending.receipt);
    assert!(service.consume(&pending.receipt, now + NOTICE).is_err());

    let mut service = Service::default();
    let cancelled = service.begin("b".repeat(64), now).unwrap();
    service.finish(&cancelled.receipt, true, now).unwrap();
    let (status, update) = service.consume(&cancelled.receipt, now + NOTICE).unwrap();
    assert_eq!(status.state, "cancelled");
    assert!(update.is_none());
    let expired = service.begin("c".repeat(64), now + NOTICE).unwrap();
    service.begin("d".repeat(64), now + NOTICE * 2).unwrap();
    assert!(service.consume(&expired.receipt, now + NOTICE * 2).is_err());
}

async fn call(
    app: &App,
    body: serde_json::Value,
    peer: &str,
    authorized: bool,
) -> (StatusCode, serde_json::Value) {
    let mut headers = HeaderMap::new();
    if authorized {
        headers.insert(
            "authorization",
            format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
        );
    }
    let response = request(
        State(app.clone()),
        ConnectInfo(peer.parse().unwrap()),
        headers,
        Json(serde_json::from_value(body).unwrap()),
    )
    .await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

async fn achievement_fixture() -> (std::path::PathBuf, App) {
    achievement_fixture_with_capacity(16).await
}
async fn achievement_fixture_with_capacity(capacity: usize) -> (std::path::PathBuf, App) {
    let (dir, mut app) = crate::server::test_app();
    app.max_players = capacity;
    app.world = Handle::spawn(
        crate::world::Directory::prepared(
            std::path::Path::new("data"),
            app.content.clone(),
            capacity,
            1376,
            2,
        )
        .unwrap(),
    );
    app.world.clock.set_paused(true);
    (dir, app)
}
async fn achievement_actor(
    app: &App,
    name: &str,
    template: Option<&str>,
    bind: bool,
) -> (Owner, i32, crate::store::Character) {
    let account = app.store.list(name.into()).await.unwrap().id;
    let ch = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let owner = app.claim_session(account).unwrap();
    let snapshot = ch.clone();
    let template = template.map(str::to_owned);
    let id = app
        .world
        .directory(move |d| {
            let world = match template {
                Some(template) => d.prepared_instance(&template)?,
                None => -2,
            };
            d.worlds.get_mut(&world).unwrap().enter(snapshot, None)
        })
        .await
        .unwrap();
    if bind {
        owner.bind(id).unwrap();
    }
    (owner, id, ch)
}
async fn begin_achievement_update(app: &App, owner: char) -> serde_json::Value {
    let (code, status) = call(
        app,
        serde_json::json!({"operation":"begin","owner":owner.to_string().repeat(64)}),
        "127.0.0.1:1",
        true,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    status["receipt"].clone()
}
fn elapsed_for_unit_test(app: &App) {
    // Advance only this fresh fixture's deadline, never production/native clocks or the 90-second policy.
    app.restart
        .lock()
        .unwrap()
        .current
        .as_mut()
        .unwrap()
        .deadline = Instant::now() - NOTICE;
}
async fn achievement_balance(app: &App, account: i32) -> i32 {
    app.store.presentation(account).await.unwrap().credits
}

#[tokio::test]
async fn achievement_actual_consume_route_awards_three_worlds_once_and_preserves_cancellation_and_concurrency()
 {
    let (_dir, app) = achievement_fixture().await;
    let mut actors = Vec::new();
    for (i, template) in [
        crate::endgame::CASTLE,
        crate::endgame::CHAMBER,
        crate::endgame::CELLAR,
    ]
    .into_iter()
    .enumerate()
    {
        actors.push(
            achievement_actor(&app, &format!("update-eligible-{i}"), Some(template), true).await,
        );
    }
    let nexus = achievement_actor(&app, "update-nexus", None, true).await;
    let dead = achievement_actor(&app, "update-dead", Some(crate::endgame::CASTLE), true).await;
    app.world
        .for_player(dead.1)
        .call(move |w| {
            let p = w.players.get_mut(&dead.1).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            Ok(())
        })
        .await
        .unwrap();
    let bot = app
        .store
        .guest_persona_character("guest:v1:1376".into(), app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let bot_owner = app.claim_session(bot.account).unwrap();
    let bot_snapshot = bot.clone();
    let bot_id = app
        .world
        .directory(move |d| {
            let world = d.prepared_instance(crate::endgame::CASTLE)?;
            d.worlds.get_mut(&world).unwrap().enter(bot_snapshot, None)
        })
        .await
        .unwrap();
    bot_owner.bind(bot_id).unwrap();
    let receipt = begin_achievement_update(&app, 'a').await;
    for (_, _, ch) in &actors {
        assert_eq!(achievement_balance(&app, ch.account).await, 100);
    }
    let consume = serde_json::json!({"operation":"consume","receipt":receipt});
    assert_eq!(
        call(&app, consume.clone(), "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    let mut foreign = receipt.clone();
    foreign["instance"] = "f".repeat(64).into();
    assert_eq!(
        call(
            &app,
            serde_json::json!({"operation":"consume","receipt":foreign}),
            "127.0.0.1:1",
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let cancel = serde_json::json!({"operation":"cancel","receipt":receipt,"actor_id":"1558662335076376580"});
    assert_eq!(
        call(&app, cancel, "127.0.0.1:1", true).await.1["state"],
        "cancelled"
    );
    assert_eq!(
        call(&app, consume, "127.0.0.1:1", true).await.1["state"],
        "cancelled"
    );
    for (_, _, ch) in &actors {
        assert_eq!(achievement_balance(&app, ch.account).await, 100);
    }
    let receipt = begin_achievement_update(&app, 'b').await;
    elapsed_for_unit_test(&app);
    let consume = serde_json::json!({"operation":"consume","receipt":receipt});
    let (first, second) = tokio::join!(
        call(&app, consume.clone(), "127.0.0.1:1", true),
        call(&app, consume.clone(), "127.0.0.1:1", true)
    );
    let mut codes = vec![first.0.as_u16(), second.0.as_u16()];
    codes.sort();
    assert_eq!(codes, vec![200, 409]);
    for (_, _, ch) in &actors {
        assert_eq!(achievement_balance(&app, ch.account).await, 200);
    }
    for ch in [&nexus.2, &dead.2, &bot] {
        assert_eq!(achievement_balance(&app, ch.account).await, 100);
    }
    assert_eq!(
        app.restart
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .status
            .state,
        "consumed"
    );
    assert!(lifecycle::drain::open(&app));
    assert!(!*app.shutdown.borrow());
    assert_eq!(
        call(&app, consume, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    for (owner, id, _) in actors.into_iter().chain([nexus, dead]) {
        super::super::depart(&app, id).await.unwrap();
        owner.release();
    }
    super::super::depart(&app, bot_id).await.unwrap();
    bot_owner.release();
}

#[tokio::test]
async fn achievement_consumed_update_failure_is_irreversible_and_rolls_back_the_award() {
    let (dir, app) = achievement_fixture().await;
    let (first_owner, first_id, first) = achievement_actor(
        &app,
        "update-atomic-first",
        Some(crate::endgame::CASTLE),
        true,
    )
    .await;
    let (owner, id, ch) =
        achievement_actor(&app, "update-overflow", Some(crate::endgame::CHAMBER), true).await;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute(
        "UPDATE accounts SET credits=2147483647 WHERE id=?1",
        [ch.account],
    )
    .unwrap();
    let receipt = begin_achievement_update(&app, 'a').await;
    elapsed_for_unit_test(&app);
    let consume = serde_json::json!({"operation":"consume","receipt":receipt});
    let failed = call(&app, consume.clone(), "127.0.0.1:1", true).await;
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        failed.1["error"]
            .as_str()
            .unwrap()
            .contains("already consumed")
    );
    assert_eq!(
        app.restart
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .status
            .state,
        "consumed"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM gold_rewards WHERE reward='achievement_no'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(achievement_balance(&app, ch.account).await, i32::MAX);
    assert_eq!(achievement_balance(&app, first.account).await, 100);
    assert_eq!(
        call(&app, consume, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    super::super::depart(&app, id).await.unwrap();
    super::super::depart(&app, first_id).await.unwrap();
    owner.release();
    first_owner.release();
}

#[tokio::test]
async fn achievement_update_route_requires_current_avatar_ownership_and_allows_no_eligible_actors()
{
    let (_dir, app) = achievement_fixture().await;
    let receipt = begin_achievement_update(&app, 'a').await;
    elapsed_for_unit_test(&app);
    assert_eq!(
        call(
            &app,
            serde_json::json!({"operation":"consume","receipt":receipt}),
            "127.0.0.1:1",
            true
        )
        .await
        .1["state"],
        "consumed"
    );
    let (_dir, app) = achievement_fixture().await;
    let (owner, _id, ch) =
        achievement_actor(&app, "update-unbound", Some(crate::endgame::CELLAR), false).await;
    let receipt = begin_achievement_update(&app, 'b').await;
    elapsed_for_unit_test(&app);
    let failed = call(
        &app,
        serde_json::json!({"operation":"consume","receipt":receipt}),
        "127.0.0.1:1",
        true,
    )
    .await;
    assert_eq!(failed.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        failed.1["error"]
            .as_str()
            .unwrap()
            .contains("does not bind")
    );
    assert_eq!(achievement_balance(&app, ch.account).await, 100);
    assert_eq!(
        app.restart
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .status
            .state,
        "consumed"
    );
    drop(owner);
}

#[tokio::test]
async fn production_handler_preserves_private_authorization_and_shutdown_and_drain_fences() {
    let (dir, app) = crate::server::test_app();
    let begin = serde_json::json!({"operation":"begin", "owner":"b".repeat(64)});
    for (peer, auth) in [("127.0.0.1:1", false), ("192.0.2.1:1", true)] {
        assert_eq!(
            call(&app, begin.clone(), peer, auth).await.0,
            StatusCode::FORBIDDEN
        );
        assert!(app.restart.lock().unwrap().current.is_none());
    }
    let pending = call(&app, begin, "127.0.0.1:1", true).await;
    assert_eq!(pending.0, StatusCode::OK);
    let receipt = pending.1["receipt"].clone();
    let invalid =
        serde_json::json!({"operation":"cancel", "receipt": receipt, "actor_id":"invalid"});
    assert_eq!(
        call(&app, invalid, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    let cancel = serde_json::json!({"operation":"cancel", "receipt": receipt, "actor_id":"1558662335076376580"});
    assert_eq!(
        call(&app, cancel.clone(), "127.0.0.1:1", true).await.1["state"],
        "cancelled"
    );
    assert!(!*app.shutdown.borrow());
    assert!(lifecycle::drain::open(&app));
    assert_eq!(
        call(&app, cancel.clone(), "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    app.shutdown.send_replace(true);
    assert_eq!(
        call(&app, cancel.clone(), "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    app.shutdown.send_replace(false);
    lifecycle::drain::prepare(&app);
    assert_eq!(
        call(&app, cancel, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

async fn achievement_human(
    app: &App,
    name: &str,
) -> (
    Owner,
    i32,
    crate::store::Character,
    tokio::sync::mpsc::Receiver<crate::wire::Packet>,
) {
    let account = app.store.list(name.into()).await.unwrap().id;
    let character = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let owner = app.claim_session(account).unwrap();
    let snapshot = character.clone();
    let (packets, inbox) = tokio::sync::mpsc::channel(256);
    let output = Output {
        trace: None,
        packets,
        lost: watch::channel(None).0,
    };
    let id = app
        .world
        .directory(move |d| {
            let castle = d.prepared_instance(crate::endgame::CASTLE)?;
            d.worlds
                .get_mut(&castle)
                .unwrap()
                .enter(snapshot, Some(output))
        })
        .await
        .unwrap();
    owner.bind(id).unwrap();
    (owner, id, character, inbox)
}

fn release_on_cancel(owner: Owner) -> tokio::task::JoinHandle<()> {
    let mut cancelled = owner.cancel.clone().unwrap();
    tokio::spawn(async move {
        if !*cancelled.borrow() {
            cancelled.changed().await.unwrap();
        }
        owner.release();
    })
}

#[tokio::test]
async fn achievement_update_accepts_85_humans_and_refuses_the_86th_before_entry() {
    let (dir, app) = achievement_fixture_with_capacity(85).await;
    let castle = app
        .world
        .directory(|d| d.prepared_instance(crate::endgame::CASTLE))
        .await
        .unwrap();
    let mut owners = Vec::new();
    let mut inboxes = Vec::new();
    let mut characters = Vec::new();
    for index in 0..86 {
        let account = app
            .store
            .list(format!("update-boundary-{index}"))
            .await
            .unwrap()
            .id;
        let character = app
            .store
            .create(account, app.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        if index < 85 {
            let owner = app.claim_session(account).unwrap();
            let snapshot = character.clone();
            let (packets, inbox) = tokio::sync::mpsc::channel(256);
            let output = Output {
                trace: None,
                packets,
                lost: watch::channel(None).0,
            };
            let id = app
                .world
                .directory(move |d| {
                    let world = d.worlds.get_mut(&castle).unwrap();
                    let id = world.enter(snapshot, Some(output))?;
                    assert!(world.players[&id].controller == crate::world::Controller::Human);
                    Ok(id)
                })
                .await
                .unwrap();
            owner.bind(id).unwrap();
            owners.push((owner, id));
            inboxes.push(inbox);
        } else {
            assert!(
                app.claim_session(account)
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("full")
            );
        }
        characters.push(character);
    }
    assert_eq!(
        app.world
            .directory(|d| Ok(d.update_achievement_actors().len()))
            .await
            .unwrap(),
        85
    );
    assert!(
        app.store
            .achievement_update(characters.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid intentional update achievement batch")
    );
    for character in &characters {
        assert_eq!(achievement_balance(&app, character.account).await, 100);
    }
    let receipt = begin_achievement_update(&app, 'a').await;
    elapsed_for_unit_test(&app);
    let consume = serde_json::json!({"operation":"consume","receipt":receipt});
    assert_eq!(
        call(&app, consume.clone(), "127.0.0.1:1", true).await.1["state"],
        "consumed"
    );
    assert_eq!(
        call(&app, consume, "127.0.0.1:1", true).await.0,
        StatusCode::CONFLICT
    );
    for (index, character) in characters.iter().enumerate() {
        assert_eq!(
            achievement_balance(&app, character.account).await,
            if index < 85 { 200 } else { 100 }
        );
    }
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM gold_rewards WHERE reward='achievement_no'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        85
    );
    for (owner, id) in owners {
        super::super::depart(&app, id).await.unwrap();
        owner.release();
    }
}

#[tokio::test]
async fn achievement_generic_shutdown_drain_never_awards_no() {
    let (dir, app) = achievement_fixture().await;
    let (owner, _, character, _inbox) = achievement_human(&app, "generic-drain-no").await;
    let released = release_on_cancel(owner);
    assert!(app.restart.lock().unwrap().current.is_none());
    super::super::drain_shutdown(&app, std::future::ready(Ok(())))
        .await
        .unwrap();
    assert_eq!(achievement_balance(&app, character.account).await, 100);
    assert!(app.restart.lock().unwrap().current.is_none());
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM gold_rewards WHERE reward='achievement_no'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    released.await.unwrap();
}

#[tokio::test]
async fn achievement_consumed_intent_survives_discarded_ack_and_later_drain_failure() {
    let (dir, app) = achievement_fixture().await;
    let (owner, id, character, _inbox) = achievement_human(&app, "lost-update-ack").await;
    let receipt = begin_achievement_update(&app, 'a').await;
    elapsed_for_unit_test(&app);
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    let response = request(
        State(app.clone()),
        ConnectInfo("127.0.0.1:1".parse().unwrap()),
        headers,
        Json(Request::Consume {
            receipt: serde_json::from_value(receipt.clone()).unwrap(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Discard the actual handler response without acknowledging it to a client.
    drop(response);
    assert_eq!(achievement_balance(&app, character.account).await, 200);
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let paid: (i64, Option<i64>) = db.query_row("SELECT awarded_at,notified_at FROM gold_rewards WHERE account=?1 AND reward='achievement_no'", [character.account], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    db.execute_batch("CREATE TRIGGER fixture_later_drain_failure BEFORE UPDATE ON characters BEGIN SELECT RAISE(ABORT,'fixture post-consume save failure'); END;").unwrap();
    let released = release_on_cancel(owner);
    let finalization = lifecycle::finalize(&app, id, CloseReason::Shutdown).unwrap();
    let (shutdown, persistence) = tokio::join!(
        super::super::drain_shutdown(&app, std::future::ready(Ok(()))),
        finalization.wait()
    );
    assert!(
        persistence
            .unwrap_err()
            .to_string()
            .contains("fixture post-consume save failure")
    );
    assert!(
        shutdown
            .unwrap_err()
            .to_string()
            .contains("shutdown drain failed")
    );
    let exact: Receipt = serde_json::from_value(receipt.clone()).unwrap();
    {
        let mut service = app.restart.lock().unwrap();
        assert_eq!(service.current.as_ref().unwrap().status.state, "consumed");
        assert!(service.consume(&exact, Instant::now()).is_err());
        assert!(service.begin("b".repeat(64), Instant::now()).is_err());
        assert!(service.finish(&exact, true, Instant::now()).is_err());
    }
    assert_eq!(
        call(
            &app,
            serde_json::json!({"operation":"consume","receipt":receipt}),
            "127.0.0.1:1",
            true
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(achievement_balance(&app, character.account).await, 200);
    assert_eq!(db.query_row("SELECT awarded_at,notified_at FROM gold_rewards WHERE account=?1 AND reward='achievement_no'", [character.account], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,Option<i64>>(1)?))).unwrap(), paid);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM gold_rewards WHERE reward='achievement_no'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    db.execute_batch("DROP TRIGGER fixture_later_drain_failure")
        .unwrap();
    released.await.unwrap();
}
