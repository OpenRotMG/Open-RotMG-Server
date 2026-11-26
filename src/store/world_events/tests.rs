use super::*;
use crate::loot::evaluator;

fn path(name: &str) -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join(format!("journal-{name}-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    dir.join("game.db")
}

async fn fixture(path: &Path) -> (Content, Store, Character) {
    let content = crate::world::test_content();
    let store = Store::open(path, &content).unwrap();
    let a = store.list("journal-fixture".into()).await.unwrap();
    let ch = store
        .create(a.id, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    (content, store, ch)
}

fn write(ch: &Character) -> InventoryWrite {
    InventoryWrite {
        id: ch.id,
        account: ch.account,
        revision: ch.revision,
        items: ch.items.clone(),
    }
}

#[tokio::test]
async fn xml_rewards_journal_terminal_plan_seed_ledger_and_pickup_bind_exact_replay_across_restart()
{
    let path = path("terminal");
    let (content, store, ch) = fixture(&path).await;
    let game = content.game.as_ref().unwrap();
    let defeat = DefeatContext {
        world: 7,
        encounter: Some(99),
        actor: 1000,
        event: 1,
        kind: 0x917,
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
    assert!(!allocation.allocations().is_empty());
    let payload = TerminalPayload {
        defeat,
        ledger,
        seed: 52,
        allocation: Some(allocation),
        actions: serde_json::json!({}),
    };
    let event = Event::terminal(&game.fingerprint, payload.clone()).unwrap();
    store.prepare_world_event(event.clone()).await.unwrap();
    store.lifecycle_test_lose_reply();
    assert!(store.commit_terminal_event(event.clone()).await.is_err());
    let first = store.world_event(event.id.clone()).await.unwrap().unwrap();
    assert_eq!(first.status, 1);
    assert_eq!(
        store.commit_terminal_event(event.clone()).await.unwrap(),
        first
    );
    store.publish_world_event(event.clone()).await.unwrap();
    let item = payload.allocations()[0].item();
    let mut w = write(&ch);
    w.items[5] = i32::from(item);
    let operation = "a".repeat(32);
    store
        .pick_world_entitlement(event.clone(), 0, operation.clone(), w.clone(), 5)
        .await
        .unwrap();
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(
        store
            .pick_world_entitlement(event.clone(), 0, operation.clone(), w.clone(), 5)
            .await
            .unwrap(),
        ch.revision + 1
    );
    assert!(
        store
            .pick_world_entitlement(event.clone(), 0, "b".repeat(32), w.clone(), 5)
            .await
            .is_err()
    );
    let mut changed = w.clone();
    changed.revision += 1;
    assert!(
        store
            .pick_world_entitlement(event.clone(), 0, operation, changed, 5)
            .await
            .is_err()
    );
    assert_eq!(store.load(ch.account, ch.id).await.unwrap().items, w.items);
    let mut modified = event.clone();
    let EventPayload::Terminal(body) = &mut modified.payload else {
        panic!("not terminal")
    };
    body.seed = 53;
    assert!(store.prepare_world_event(modified).await.is_err());
    let mut modified = event;
    let EventPayload::Terminal(body) = &mut modified.payload else {
        panic!("not terminal")
    };
    body.ledger[0].effective_damage = 39999;
    assert!(store.prepare_world_event(modified).await.is_err());
}
