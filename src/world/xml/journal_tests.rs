use super::*;
use rand::SeedableRng;

#[tokio::test]
async fn xml_hard_scripted_birth_preflight_replays_without_reroll() {
    use crate::store::Store;
    let content = Arc::new(crate::world::test_content());
    let path = std::env::temp_dir().join(format!("hard-preflight-{}.db", rand::random::<u64>()));
    let store = Store::open(&path, &content).unwrap();
    let account = store.list("hard-preflight".into()).await.unwrap();
    let ch = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let w = &mut flat_nonrealm(content.clone());
    w.xml.template = Some("snake-pit".into());
    let boss = spawn(w, 0x917, 64.5, 64.5);
    let p = w.enter(ch.clone(), None).unwrap();
    let pos = w.entities[&boss].pos;
    w.players.get_mut(&p).unwrap().pos = pos;
    for now in (0..=3500).step_by(50) {
        w.now = now;
        w.xml_step().unwrap();
    }
    w.xml.fail_next_terminal_birth_plan = true;
    let mut rng = rand::rngs::StdRng::seed_from_u64(52);
    let seed = rng.r#gen::<u64>();
    w.rng = rand::rngs::StdRng::seed_from_u64(52);
    let hp = w.entities[&boss].hp;
    assert_eq!(w.damage_enemy(p, boss, hp, true, &[], None).unwrap(), hp);
    let ledger = w.xml.facts[&boss].ledger.clone();
    assert_eq!(w.xml.facts[&boss].seed, seed);
    assert_eq!(ledger[0].effective_damage, hp as u64);
    let defeated_at = w.now;
    for delta in [
        50,
        50 + terminal::PREFLIGHT_RETRY_MS,
        100 + terminal::PREFLIGHT_RETRY_MS,
    ] {
        assert!(w.pending_terminal_events().is_empty());
        assert!(w.entities[&boss].reserved);
        assert_eq!(w.players[&p].terminal_captures, 1);
        // Neither unrelated draws nor a later position can change the original captured defeat.
        let _ = w.rng.r#gen::<u64>();
        w.players.get_mut(&p).unwrap().pos = Pos { x: 1., y: 1. };
        w.xml.fail_next_terminal_birth_plan = true;
        w.now = defeated_at + delta;
        w.xml_step().unwrap();
    }
    let event = w.pending_terminal_events().pop().unwrap();
    let payload = event.terminal_payload().unwrap();
    let publication = &payload.actions["publication"];
    let abandoned = &publication["abandoned_births"][0];
    assert_eq!(abandoned["disposition"], "physical_effect_abandoned");
    assert_eq!(abandoned["effect"], "scripted_birth_plan");
    assert!(
        abandoned["preflight"]
            .as_str()
            .unwrap()
            .contains("injected")
    );
    assert!(!publication["clear_portal"].is_null());
    assert_eq!(publication["now"], defeated_at);
    assert_eq!(publication["defeat_recipients"][0]["character"], ch.id);
    assert_eq!(payload.seed, seed);
    assert_eq!(
        serde_json::to_value(&payload.ledger).unwrap(),
        serde_json::to_value(&ledger).unwrap()
    );
    let expected = crate::loot::evaluator::evaluate(
        w.xml_game(),
        payload.defeat,
        &ledger,
        seed,
        crate::store::world_events::LIMITS,
    )
    .unwrap();
    assert!(!expected.allocations().is_empty());
    assert_eq!(
        serde_json::to_value(&payload.allocation).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    // Reject a real journal write and let both windows elapse with no receipt.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_terminal BEFORE INSERT ON world_events BEGIN SELECT RAISE(ABORT, 'preflight test Store unavailable'); END;").unwrap();
    assert!(store.coordinate_terminal(event.clone()).await.is_err());
    assert!(store.world_event(event.id.clone()).await.unwrap().is_none());
    w.now += 2 * terminal::PREFLIGHT_RETRY_MS;
    w.xml_step().unwrap();
    assert_eq!(w.pending_terminal_events(), std::slice::from_ref(&event));
    assert!(w.entities[&boss].reserved && !w.cleared);
    assert_eq!(w.players[&p].terminal_captures, 1);
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), ch);
    db.execute_batch("DROP TRIGGER reject_terminal").unwrap();
    drop(db);
    let committed = store.coordinate_terminal(event.clone()).await.unwrap();
    assert_eq!(committed.status, 1);
    drop(store);

    let store = Store::open(&path, &content).unwrap();
    assert_eq!(
        store.coordinate_terminal(event.clone()).await.unwrap(),
        committed
    );
    let rows = store.unpicked_entitlements(event.clone()).await.unwrap();
    assert_eq!(rows.len(), payload.allocations().len());
    for (ordinal, (row, allocation)) in rows.iter().zip(payload.allocations()).enumerate() {
        assert_eq!(
            (row.ordinal, row.item, row.owner),
            (ordinal, allocation.item(), allocation.owner())
        );
    }
    w.finish_terminal_event(committed.clone()).unwrap();
    w.finish_terminal_event(committed).unwrap();
    assert!(!w.entities.contains_key(&boss) && w.cleared);
    assert_eq!(w.players[&p].terminal_captures, 0);
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x704).count(), 1);
    let saved = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.kills, ch.kills + 1);
    assert_eq!(saved.dungeons[0], ch.dungeons[0] + 1);
    assert!(saved.exp > ch.exp);
    assert_eq!(w.players[&p].character, saved);
    assert_eq!(
        store
            .publish_world_event(event.clone())
            .await
            .unwrap()
            .status,
        2
    );
    assert_eq!(store.coordinate_terminal(event).await.unwrap().status, 2);
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), saved);
}
