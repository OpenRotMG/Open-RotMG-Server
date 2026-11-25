use super::*;

#[tokio::test]
async fn xml_rewards_journal_restart_origin_loss_preserves_committed_progress_picked_and_expired_without_rebind()
 {
    let path = path("origin-loss");
    let (content, store, ch) = fixture(&path).await;
    let game = content.game.as_ref().unwrap();
    let origin = "f".repeat(32);
    let defeat = DefeatContext {
        world: 7,
        encounter: Some(99),
        actor: 1000,
        event: 1,
        kind: 0x1706,
    };
    let ledger = vec![Contribution {
        character: ch.id,
        world: 7,
        encounter: Some(99),
        effective_damage: 40000,
        alive: true,
        departing: false,
    }];
    let allocation = evaluator::evaluate(game, defeat, &ledger, 52, LIMITS).unwrap();
    assert!(allocation.allocations().len() >= 2);
    let mut progress = ch.clone();
    progress.exp += 5;
    progress.dungeons[1] += 1;
    let actions = TerminalActions {
        instance: origin.clone(),
        actor: 1000,
        loot: true,
        progress: vec![(progress.clone(), vec![])],
        publication: serde_json::json!({"template":"ocean-trench","position":{"x":8.5,"y":8.5}}),
        planning_error: None,
    };
    let event = Event::terminal(
        &game.fingerprint,
        TerminalPayload {
            defeat,
            ledger,
            seed: 52,
            allocation: Some(allocation),
            actions: serde_json::to_value(actions).unwrap(),
        },
    )
    .unwrap();
    let receipt = store.coordinate_terminal(event.clone()).await.unwrap();
    assert_eq!(receipt.status, 1);
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), progress);
    let rows = store.unpicked_entitlements(event.clone()).await.unwrap();
    let mut picked = write(&progress);
    picked.items[5] = i32::from(rows[0].item);
    store
        .pick_world_entitlement(event.clone(), 0, "a".repeat(32), picked.clone(), 5)
        .await
        .unwrap();
    store
        .expire_world_entitlements(event.clone(), vec![(1, 55, 10, 11), (0, 55, 10, 11)])
        .await
        .unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM world_entitlements WHERE event=?1 AND ordinal=0",
            [&event.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT status FROM world_entitlements WHERE event=?1 AND ordinal=1",
            [&event.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        2
    );
    let (mut prepared, _) = inventory(&content, &store.load(ch.account, ch.id).await.unwrap());
    let EventPayload::Inventory(payload) = &mut prepared.payload else {
        unreachable!()
    };
    payload["origin"] = serde_json::json!({"instance":origin});
    store.prepare_world_event(prepared.clone()).await.unwrap();
    assert!(
        store
            .reconcile_lost_world_events(vec![origin.clone()])
            .await
            .unwrap()
            .is_empty()
    );
    drop(db);
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    let outcomes = store.reconcile_lost_world_events(vec![]).await.unwrap();
    assert!(outcomes.contains(&(event.id.clone(), "origin_lost".into())));
    assert!(outcomes.contains(&(prepared.id.clone(), "aborted_origin_lost".into())));
    assert!(
        store
            .reconcile_lost_world_events(vec!["e".repeat(32)])
            .await
            .unwrap()
            .is_empty()
    );
    let saved = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.exp, progress.exp);
    assert_eq!(saved.dungeons, progress.dungeons);
    assert_eq!(saved.items, picked.items);
    assert_eq!(saved.revision, ch.revision + 1);
    let record = store.world_event(event.id.clone()).await.unwrap().unwrap();
    assert_eq!(record.event, event);
    assert_eq!(record.status, 3);
    assert!(store.commit_terminal_event(event.clone()).await.is_err());
    assert!(store.unpicked_entitlements(event).await.is_err());
    assert!(
        store
            .commit_inventory_event(prepared, write(&saved))
            .await
            .is_err()
    );
    store.validate_world_events().await.unwrap();
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), saved);
}
