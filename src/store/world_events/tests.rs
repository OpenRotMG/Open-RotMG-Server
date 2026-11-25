use super::*;
use crate::loot::evaluator;
use std::sync::Arc;

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
fn inventory(content: &Content, ch: &Character) -> (Event, InventoryWrite) {
    let mut w = write(ch);
    w.items[4] = 0xa21;
    let event=Event::new("inventory",&content.game.as_ref().unwrap().fingerprint,serde_json::json!({"write":w,"source":{"slot":4,"original":ch.items[4]},"publication":{"world":"fixture-world","position":[8.5,8.5]}})).unwrap();
    (event, w)
}
#[tokio::test]
async fn xml_rewards_journal_v24_to_v25_preserves_earned_rows_and_reopens_idempotently() {
    let path = path("migration");
    let (content, store, ch) = fixture(&path).await;
    let original = store.load(ch.account, ch.id).await.unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "DROP TABLE world_entitlements; DROP TABLE world_events; PRAGMA user_version=24;",
    )
    .unwrap();
    drop(db);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), original);
    let (event, w) = inventory(&content, &ch);
    store.prepare_world_event(event.clone()).await.unwrap();
    store
        .commit_inventory_event(event.clone(), w)
        .await
        .unwrap();
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    let r = store.world_event(event.id).await.unwrap().unwrap();
    assert_eq!(r.status, 1);
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        25
    );
}
#[tokio::test]
async fn xml_rewards_journal_lost_commit_reply_reopens_same_exact_inventory_result_without_second_spend()
 {
    let path = path("lost-reply");
    let (content, store, ch) = fixture(&path).await;
    let (event, w) = inventory(&content, &ch);
    store.prepare_world_event(event.clone()).await.unwrap();
    store
        .lifecycle_lost_reply
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(
        store
            .for_world_event(event.clone())
            .unwrap()
            .exchange(vec![w.clone()], vec![])
            .await
            .is_err()
    );
    let original = store.world_event(event.id.clone()).await.unwrap().unwrap();
    assert_eq!(original.status, 1);
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(
        store
            .commit_inventory_event(event.clone(), w.clone())
            .await
            .unwrap(),
        original
    );
    let saved = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.revision, ch.revision + 1);
    assert_eq!(saved.items, w.items);
    let mut changed = event.clone();
    let EventPayload::Inventory(body) = &mut changed.payload else {
        panic!("not inventory")
    };
    body["publication"]["position"][0] = serde_json::json!(9.5);
    assert!(store.prepare_world_event(changed).await.is_err());
    assert_eq!(
        store.world_event(event.id).await.unwrap().unwrap(),
        original
    );
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
    let first = store.commit_terminal_event(event.clone()).await.unwrap();
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
#[tokio::test]
async fn xml_rewards_journal_corruption_and_changed_content_retain_reconciliation_evidence() {
    let path = path("reconciliation");
    let (content, store, ch) = fixture(&path).await;
    let (event, _) = inventory(&content, &ch);
    store.prepare_world_event(event.clone()).await.unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE world_events SET payload_hash='damaged' WHERE event=?1",
        [&event.id],
    )
    .unwrap();
    drop(db);
    let store = Store::open(&path, &content).unwrap();
    assert!(store.world_event(event.id.clone()).await.is_err());
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE world_events SET payload_hash=?2 WHERE event=?1",
        params![event.id, event.encoded().unwrap().1],
    )
    .unwrap();
    drop(db);
    let mut other = content.clone();
    Arc::make_mut(other.game.as_mut().unwrap()).fingerprint = "b".repeat(64);
    let store = Store::open(&path, &other).unwrap();
    assert!(store.world_event(event.id.clone()).await.is_err());
    assert_eq!(store.validate_world_events().await.unwrap(), 1);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM world_events WHERE event=?1",
            [&event.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0,
        "lookup does not invent a disposition; origin-loss reconciliation owns it"
    );
    assert_eq!(
        db.query_row(
            "SELECT payload FROM world_events WHERE event=?1",
            [&event.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        event.encoded().unwrap().0
    );
}

#[tokio::test]
async fn xml_rewards_journal_full_snapshot_preserves_pending_unpicked_picked_and_sensitive_fields_refuse()
 {
    let path = path("snapshot");
    let (content, store, ch) = fixture(&path).await;
    let (pending, _) = inventory(&content, &ch);
    store.prepare_world_event(pending.clone()).await.unwrap();
    let game = content.game.as_ref().unwrap();
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
        effective_damage: u64::try_from(game.properties[&0x1706].hp).unwrap(),
        alive: true,
        departing: false,
    }];
    let allocation = evaluator::evaluate(game, defeat, &ledger, 52, LIMITS).unwrap();
    assert!(allocation.allocations().len() >= 2);
    let item = allocation.allocations()[0].item();
    let payload = TerminalPayload {
        defeat,
        ledger,
        seed: 52,
        allocation: Some(allocation),
        actions: serde_json::json!({}),
    };
    let event = Event::terminal(&game.fingerprint, payload.clone()).unwrap();
    store.prepare_world_event(event.clone()).await.unwrap();
    store.commit_terminal_event(event.clone()).await.unwrap();
    let mut w = write(&ch);
    w.items[5] = i32::from(item);
    let operation = "a".repeat(32);
    store
        .pick_world_entitlement(event.clone(), 0, operation.clone(), w, 5)
        .await
        .unwrap();
    let snapshot = path.with_file_name("snapshot.db");
    crate::account_save::snapshot(&path, &snapshot, &content).unwrap();
    let restored = Store::open(&snapshot, &content).unwrap();
    restored.validate_world_events().await.unwrap();
    for id in [&pending.id, &event.id, &operation] {
        assert_eq!(
            store.world_event(id.clone()).await.unwrap(),
            restored.world_event(id.clone()).await.unwrap()
        );
    }
    let sql = "SELECT event,ordinal,owner_character,item,pickup_operation,status FROM world_entitlements ORDER BY event,ordinal";
    let rows = |path: &Path| {
        let db = Connection::open(path).unwrap();
        db.prepare(sql)
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, usize>(1)?,
                    r.get::<_, Option<i32>>(2)?,
                    r.get::<_, u16>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i32>(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    assert_eq!(rows(&path), rows(&snapshot));
    for key in [
        "password",
        "password_hash",
        "credentials",
        "recovery_codes",
        "grant",
        "key",
        "auth",
        "token",
        "guid",
    ] {
        assert!(
            Event::new(
                "inventory",
                &game.fingerprint,
                serde_json::json!({"nested":[{key:"private canary"}]})
            )
            .is_err(),
            "{key}"
        );
    }
}

#[tokio::test]
async fn xml_rewards_journal_pending_portable_refuses_then_picked_history_allows_replacement_without_resurrection()
 {
    let (content, source, ch) = fixture(&path("portable-source")).await;
    assert_eq!(
        source
            .register(
                "journal-fixture".into(),
                "journal@example.test".into(),
                "source-fixture-password".into(),
            )
            .await
            .unwrap(),
        ch.account
    );
    let save = source.export_account_save(ch.account).await.unwrap();
    let store = Store::open(&path("portable-destination"), &content).unwrap();
    let imported = store
        .import_account_save(save.clone(), "local-fixture-password".into(), false)
        .await
        .unwrap();
    let mut current = store
        .call(move |db| {
            let id = db.query_row(
                "SELECT id FROM characters WHERE account=?1",
                [imported.account],
                |r| r.get::<_, i32>(0),
            )?;
            Ok((imported.account, id))
        })
        .await
        .unwrap();
    let mut ch = store.load(current.0, current.1).await.unwrap();
    let old_character = ch.id;
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
        effective_damage: u64::try_from(game.properties[&0x917].hp).unwrap(),
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
    store.commit_terminal_event(event.clone()).await.unwrap();
    store.publish_world_event(event.clone()).await.unwrap();
    let error = store
        .export_account_save(ch.account)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&event.id) && error.contains("full local database snapshot"),
        "{error}"
    );
    assert!(
        store
            .import_account_save(save.clone(), "local-fixture-password".into(), true)
            .await
            .is_err()
    );
    let mut replay = None;
    for (ordinal, item) in payload.allocations().iter().enumerate() {
        let slot = ch
            .items
            .iter()
            .enumerate()
            .skip(4)
            .find(|(_, item)| **item == -1)
            .unwrap()
            .0;
        let mut w = write(&ch);
        w.items[slot] = i32::from(item.item());
        let operation = format!("{:032x}", ordinal + 1);
        let revision = store
            .pick_world_entitlement(event.clone(), ordinal, operation.clone(), w.clone(), slot)
            .await
            .unwrap();
        let pickup = store.world_event(operation.clone()).await.unwrap().unwrap();
        store.publish_world_event(pickup.event).await.unwrap();
        if replay.is_none() {
            replay = Some((ordinal, operation, w.clone(), slot, revision));
        }
        ch.items = w.items;
        ch.revision = revision;
    }
    store.export_account_save(ch.account).await.unwrap();
    store
        .import_account_save(save, "local-fixture-password".into(), true)
        .await
        .unwrap();
    let (ordinal, operation, w, slot, revision) = replay.unwrap();
    assert_eq!(
        store
            .pick_world_entitlement(event, ordinal, operation, w, slot)
            .await
            .unwrap(),
        revision
    );
    current.1 = old_character;
    assert!(
        !store
            .call(move |db| Ok(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM characters WHERE id=?1)",
                [current.1],
                |r| r.get::<_, bool>(0)
            )?))
            .await
            .unwrap()
    );
    store.validate_world_events().await.unwrap();
}

#[tokio::test]
async fn xml_rewards_journal_failed_native_write_rolls_back_same_transaction_then_reopens_original_plan()
 {
    let path = path("rollback");
    let (content, store, ch) = fixture(&path).await;
    let (event, w) = inventory(&content, &ch);
    store.prepare_world_event(event.clone()).await.unwrap();
    let mut wrong = w.clone();
    wrong.items[4] = 0xa22;
    assert!(
        store
            .for_world_event(event.clone())
            .unwrap()
            .exchange(vec![wrong], vec![])
            .await
            .is_err()
    );
    assert_eq!(
        store
            .world_event(event.id.clone())
            .await
            .unwrap()
            .unwrap()
            .status,
        0
    );
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), ch);
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    store
        .commit_inventory_event(event.clone(), w.clone())
        .await
        .unwrap();
    assert_eq!(
        store.world_event(event.id).await.unwrap().unwrap().status,
        1
    );
    let saved = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.revision, ch.revision + 1);
    assert_eq!(saved.items, w.items);
}

#[test]
fn xml_rewards_journal_selected_operational_plan_peak_growth_stays_within_64mib() {
    const NAME: &str = "store::world_events::tests::xml_rewards_journal_selected_operational_plan_peak_growth_stays_within_64mib";
    if std::env::var_os("OPENROTMG_JOURNAL_MEMORY_WORKER").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture"])
            .env("OPENROTMG_JOURNAL_MEMORY_WORKER", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    fn memory() -> (u64, u64) {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let kib = |name: &str| {
            status
                .lines()
                .find(|line| line.starts_with(name))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u64>()
                .unwrap()
                * 1024
        };
        (kib("VmRSS:"), kib("VmHWM:"))
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let content=crate::world::test_content();
        let rules=br#"<Objects><Object type="0x600" id="Pirate" disposition="active" provenance="authored" origin="isolated operational memory fixture" reason="selected explicit planning bound, not historical loot"><Behavior>Wander</Behavior><SoulboundLoot prob="0" min="65536">Potion of Speed</SoulboundLoot></Object><Object type="0xa21" id="Potion of Speed" disposition="native" nativeService="inventory" provenance="authored" origin="selected memory fixture item" reason="exact selected item"/><Object type="0x6" id="Blade" disposition="native" nativeService="projectile" provenance="authored" origin="selected memory fixture projectile" reason="exact selected property dependency"/></Objects>"#;
        let game=crate::content::game::compile_fixture(&content,BTreeMap::from([
            ("game/123.5.1/game.xml".into(),br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/></Game>"#.to_vec()),
            ("game/123.5.1/rules.xml".into(),rules.to_vec()),("objects.xml".into(),std::fs::read("data/objects.xml").unwrap()),("grounds.xml".into(),std::fs::read("data/grounds.xml").unwrap())])).unwrap();
        let mut content=content;content.game=Some(Arc::new(game));let game=content.game.as_ref().unwrap();let store=Store::open(&path("memory"),&content).unwrap();let account=store.list("memory-fixture".into()).await.unwrap();let ch=store.create(account.id,content.get(WIZARD).unwrap()).await.unwrap();
        let defeat=DefeatContext{world:7,encounter:Some(19),actor:1000,event:1,kind:0x600};let ledger=vec![Contribution{character:ch.id,world:7,encounter:Some(19),effective_damage:u64::try_from(game.properties[&0x600].hp).unwrap(),alive:true,departing:false}];
        let loader_peak=memory().1;std::fs::write("/proc/self/clear_refs","5").expect("isolated worker cannot reset its own RSS high-water measurement");let before=memory();
        // The server-wide allocation bound retains at most this many maximum plans, whatever the per-World permits.
        let mut retained=Vec::new();
        for event_seq in 0..(crate::world::TERMINAL_PENDING_ALLOCATION_LIMIT/LIMITS.max_allocations.get()) as u64 {
            let defeat=DefeatContext{event:event_seq+1,actor:1000+i32::try_from(event_seq).unwrap(),..defeat};let ledger=ledger.clone();
            let allocation=evaluator::evaluate(game,defeat,&ledger,52+event_seq,LIMITS).unwrap();assert_eq!(allocation.allocations().len(),65_536);
            let payload=TerminalPayload{defeat,ledger,seed:52+event_seq,allocation:Some(allocation),actions:serde_json::json!({})};retained.push(Event::terminal(&game.fingerprint,payload).unwrap());
        }
        for event in &retained {store.prepare_world_event(event.clone()).await.unwrap();store.commit_terminal_event(event.clone()).await.unwrap();}
        let after=memory();let growth=after.1.checked_sub(before.1).unwrap();
        let evidence=serde_json::json!({"kind":"isolated unit process RSS/HWM delta for PENDING_LIMIT retained maximum plans/journal, not whole-server performance acceptance","plans":retained.len(),"allocations_per_plan":65536,"limit_bytes":64*1024*1024,"loader_peak_excluded":loader_peak,"before_rss":before.0,"before_peak":before.1,"after_rss":after.0,"after_peak":after.1,"peak_growth":growth});std::fs::write(std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap()).join("journal-planning-memory.json"),serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();assert!(growth<=64*1024*1024,"{evidence}");
    });
}

#[path = "recovery_tests.rs"]
mod recovery_tests;
#[tokio::test]
async fn xml_rewards_journal_reviewed_owner_withholds_marked_bags_once_at_commit() {
    let ratio =
        |keep: f64| (0..4096).filter(|i| moderation_keeps(*i, 3, keep)).count() as f64 / 4096.;
    assert!((ratio(0.5) - 0.5).abs() < 0.03 && (ratio(0.75) - 0.75).abs() < 0.03);
    let plain = path("moderation-plain");
    let (content, plain_store, ch) = fixture(&plain).await;
    let game = content.game.as_ref().unwrap().clone();
    let mut marked_content = content.clone();
    marked_content.reviewed_accounts =
        crate::content::reviews::Reviews::new(vec![crate::content::reviews::Review {
            account: ch.account,
            reviewed_on: "2026-11-05".into(),
        }]);
    let marked = path("moderation-marked");
    let marked_store = Store::open(&marked, &marked_content).unwrap();
    let a = marked_store.list("journal-fixture".into()).await.unwrap();
    assert_eq!(
        marked_store
            .create(a.id, content.get(WIZARD).unwrap())
            .await
            .unwrap()
            .id,
        ch.id
    );
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
    let marked_bag = |item: u16| {
        crate::content::reviews::marked_keep_chance(content.get(item).unwrap().bag_type)
    };
    let (seed, plan) = (0..4096)
        .map(|seed| {
            (
                seed,
                evaluator::evaluate(&game, defeat, &ledger, seed, LIMITS).unwrap(),
            )
        })
        .find(|(seed, plan)| {
            let kept: Vec<_> = plan
                .allocations()
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    marked_bag(a.item()).map(|keep| moderation_keeps(*seed, i, keep))
                })
                .collect();
            kept.contains(&true) && kept.contains(&false)
        })
        .expect("fixed seeds exercise kept and withheld marked rows");
    let payload = TerminalPayload {
        defeat,
        ledger,
        seed,
        allocation: Some(plan.clone()),
        actions: serde_json::json!({"instance": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "actor": 1000, "loot": true, "progress": [], "publication": {}, "planning_error": null}),
    };
    let event = Event::terminal(&game.fingerprint, payload).unwrap();
    let statuses = |path: &Path| -> Vec<i32> {
        Connection::open(path)
            .unwrap()
            .prepare("SELECT status FROM world_entitlements ORDER BY ordinal")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    for store in [&plain_store, &marked_store] {
        store.prepare_world_event(event.clone()).await.unwrap();
        let first = store.commit_terminal_event(event.clone()).await.unwrap();
        assert_eq!(
            store.commit_terminal_event(event.clone()).await.unwrap(),
            first
        );
    }
    assert!(statuses(&plain).iter().all(|s| *s == 0));
    let expected: Vec<i32> = plan
        .allocations()
        .iter()
        .enumerate()
        .map(|(i, a)| {
            i32::from(marked_bag(a.item()).is_some_and(|keep| !moderation_keeps(seed, i, keep))) * 3
        })
        .collect();
    assert_eq!(statuses(&marked), expected);
    let record = marked_store
        .world_event(event.id.clone())
        .await
        .unwrap()
        .unwrap();
    let withheld = expected.iter().position(|s| *s == 3).unwrap();
    assert_eq!(
        record.result.unwrap()["withheld"][withheld.to_string()]["disposition"],
        "moderation_withheld"
    );
    let unpicked = marked_store
        .unpicked_entitlements(event.clone())
        .await
        .unwrap();
    assert!(unpicked.iter().all(|row| expected[row.ordinal] == 0));
    let mut w = write(&ch);
    w.items[5] = i32::from(plan.allocations()[withheld].item());
    assert!(
        marked_store
            .pick_world_entitlement(event.clone(), withheld, "c".repeat(32), w, 5)
            .await
            .is_err()
    );
    drop(marked_store);
    let reopened = Store::open(&marked, &marked_content).unwrap();
    reopened.validate_world_events().await.unwrap();
    reopened.commit_terminal_event(event).await.unwrap();
    assert_eq!(statuses(&marked), expected);
    // Origin loss moves the commit result under reconciliation; withheld rows must stay valid.
    assert_eq!(
        reopened
            .reconcile_lost_world_events(vec![])
            .await
            .unwrap()
            .len(),
        1
    );
    reopened.validate_world_events().await.unwrap();
    let lost: Vec<i32> = expected
        .iter()
        .map(|s| if *s == 0 { 2 } else { *s })
        .collect();
    assert_eq!(statuses(&marked), lost);
}

#[tokio::test]
async fn xml_rewards_journal_batch_commits_each_event_once_and_confines_a_failure() {
    let (content, store, ch) = fixture(&path("batch")).await;
    let game = content.game.as_ref().unwrap();
    let event = |actor: i32, owner: i32| {
        let defeat = DefeatContext {
            world: 7,
            encounter: Some(99),
            actor,
            event: 1,
            kind: 0x917,
        };
        let ledger = vec![Contribution {
            character: owner,
            world: 7,
            encounter: Some(99),
            effective_damage: 40000,
            alive: true,
            departing: false,
        }];
        let allocation = evaluator::evaluate(game, defeat, &ledger, 52, LIMITS).unwrap();
        let payload = TerminalPayload {
            defeat,
            ledger,
            seed: 52,
            allocation: Some(allocation),
            actions: serde_json::json!({}),
        };
        Event::terminal(&game.fingerprint, payload).unwrap()
    };
    // The middle event's durable owner does not exist, so only its savepoint rolls back.
    let events = vec![
        event(1000, ch.id),
        event(1001, ch.id + 999),
        event(1002, ch.id),
    ];
    let outcomes = store.coordinate_terminals(events.clone()).await.unwrap();
    assert_eq!(
        outcomes
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>(),
        events.iter().map(|e| e.id.clone()).collect::<Vec<_>>()
    );
    assert!(outcomes[0].1.is_ok() && outcomes[2].1.is_ok());
    assert!(format!("{:#}", outcomes[1].1.as_ref().unwrap_err()).contains("durable owner"));
    assert_eq!(
        store
            .world_event(events[1].id.clone())
            .await
            .unwrap()
            .unwrap()
            .status,
        0,
        "failed event stays prepared"
    );
    // A repeated batch returns the same committed records without writing entitlements twice.
    let again = store
        .coordinate_terminals(vec![events[0].clone()])
        .await
        .unwrap();
    assert_eq!(
        again[0].1.as_ref().unwrap(),
        outcomes[0].1.as_ref().unwrap()
    );
    assert_eq!(
        store
            .unpicked_entitlements(events[0].clone())
            .await
            .unwrap()
            .len(),
        events[0].terminal_payload().unwrap().allocations().len()
    );
}

/// Bounded receipt comparison: one batched transaction must beat serial per-event coordination (the previous drain).
#[tokio::test]
async fn xml_rewards_journal_batch_receipts_beat_serial_coordination() {
    let (content, store, ch) = fixture(&path("throughput")).await;
    let game = content.game.as_ref().unwrap();
    let events = |base: i32| {
        (0..64)
            .map(|i| {
                let defeat = DefeatContext {
                    world: 7,
                    encounter: None,
                    actor: base + i,
                    event: 1,
                    kind: 0x600,
                };
                let ledger = vec![Contribution {
                    character: ch.id,
                    world: 7,
                    encounter: None,
                    effective_damage: 1000,
                    alive: true,
                    departing: false,
                }];
                let allocation = evaluator::evaluate(game, defeat, &ledger, 52, LIMITS).unwrap();
                Event::terminal(
                    &game.fingerprint,
                    TerminalPayload {
                        defeat,
                        ledger,
                        seed: 52,
                        allocation: Some(allocation),
                        actions: serde_json::json!({}),
                    },
                )
                .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let started = std::time::Instant::now();
    for event in events(10_000) {
        store.coordinate_terminal(event).await.unwrap();
    }
    let serial = started.elapsed();
    let started = std::time::Instant::now();
    assert!(
        store
            .coordinate_terminals(events(20_000))
            .await
            .unwrap()
            .iter()
            .all(|(_, r)| r.as_ref().is_ok_and(|r| r.status == 1))
    );
    let batched = started.elapsed();
    let report = serde_json::json!({"events":64,"serial_us":serial.as_micros() as u64,"batched_us":batched.as_micros() as u64});
    println!("THROUGHPUT {report}");
    assert!(batched < serial, "{report}");
}
#[tokio::test]
async fn xml_rewards_journal_compaction_tombstones_settled_payloads_prunes_progress_only_after_an_hour_and_keeps_replay_once_only()
 {
    let path = path("compaction");
    let (content, store, ch) = fixture(&path).await;
    let game = content.game.as_ref().unwrap();
    let instance = "d".repeat(32);
    // Shape of the measured manor progress-only kill (QA job ff75d4c1): one recipient snapshot and an empty plan.
    let kill = |actor: i32, base: &Character, exp: i32| {
        let defeat = DefeatContext {
            world: 6,
            encounter: Some(actor as u64),
            actor,
            event: 0,
            kind: 0x181a,
        };
        let ledger = vec![Contribution {
            character: ch.id,
            world: 6,
            encounter: Some(actor as u64),
            effective_damage: 3000,
            alive: true,
            departing: false,
        }];
        let allocation = evaluator::evaluate(game, defeat, &ledger, 7, LIMITS).unwrap();
        let mut progress = base.clone();
        progress.exp = exp;
        let actions = TerminalActions {
            instance: instance.clone(),
            actor,
            loot: true,
            progress: vec![(progress, vec![])],
            publication: serde_json::json!({"abandoned_births":[],"announcements":[],"births":[],"clear":null,"clear_portal":null,"clear_registration":false,"defeat_recipients":[{"character":ch.id,"player":247725,"position":{"x":205.41,"y":83.9},"xp_boost_active":false,"xp_cap":195}],"encounter":actor,"hooks":[],"initiator":null,"kind":0x181a,"now":19350,"parent":null,"portals":[],"position":{"x":205.5,"y":82.5},"quest_complete":[],"reason":"Defeated","return_route":{"destination":"random-live-realm"},"template":"manor-of-the-immortals","xp":true}),
            planning_error: None,
        };
        Event::terminal(
            &game.fingerprint,
            TerminalPayload {
                defeat,
                ledger,
                seed: 7,
                allocation: Some(allocation),
                actions: serde_json::to_value(actions).unwrap(),
            },
        )
        .unwrap()
    };
    let loot = |actor: i32| {
        let defeat = DefeatContext {
            world: 7,
            encounter: Some(99),
            actor,
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
        Event::terminal(
            &game.fingerprint,
            TerminalPayload {
                defeat,
                ledger,
                seed: 52,
                allocation: Some(allocation),
                actions: serde_json::json!({}),
            },
        )
        .unwrap()
    };
    let kills: Vec<Event> = (0..32)
        .map(|i| kill(2000 + i, &ch, ch.exp + 10 * (i + 1)))
        .collect();
    for (_, record) in store.coordinate_terminals(kills.clone()).await.unwrap() {
        store
            .publish_world_event(record.unwrap().event)
            .await
            .unwrap();
    }
    let expire_from = |event: &Event, from: usize| {
        (from..event.terminal_payload().unwrap().allocations().len())
            .map(|ordinal| (ordinal, 7, 10, 10))
            .collect::<Vec<_>>()
    };
    let settled = loot(1000);
    store.coordinate_terminal(settled.clone()).await.unwrap();
    store.publish_world_event(settled.clone()).await.unwrap();
    let mut w = write(&store.load(ch.account, ch.id).await.unwrap());
    w.items[5] = i32::from(settled.terminal_payload().unwrap().allocations()[0].item());
    let operation = "a".repeat(32);
    store
        .pick_world_entitlement(settled.clone(), 0, operation.clone(), w, 5)
        .await
        .unwrap();
    let pickup = store.world_event(operation.clone()).await.unwrap().unwrap();
    store.publish_world_event(pickup.event).await.unwrap();
    store
        .expire_world_entitlements(settled.clone(), expire_from(&settled, 1))
        .await
        .unwrap();
    let unpicked = loot(1001);
    store.coordinate_terminal(unpicked.clone()).await.unwrap();
    store.publish_world_event(unpicked.clone()).await.unwrap();
    let committed = loot(1002);
    store.coordinate_terminal(committed.clone()).await.unwrap();
    let live = loot(1003);
    store.coordinate_terminal(live.clone()).await.unwrap();
    store.publish_world_event(live.clone()).await.unwrap();
    store
        .expire_world_entitlements(live.clone(), expire_from(&live, 0))
        .await
        .unwrap();
    let current = store.load(ch.account, ch.id).await.unwrap();
    let mut write = write(&current);
    write.items[4] = 0xa21;
    let live_inventory = Event::new(
        "inventory",
        &game.fingerprint,
        serde_json::json!({"write":write,"origin":{"instance":instance}}),
    )
    .unwrap();
    store
        .prepare_world_event(live_inventory.clone())
        .await
        .unwrap();
    store
        .commit_inventory_event(live_inventory.clone(), write)
        .await
        .unwrap();
    store
        .publish_world_event(live_inventory.clone())
        .await
        .unwrap();
    let (mut lost, _) = inventory(&content, &store.load(ch.account, ch.id).await.unwrap());
    let EventPayload::Inventory(body) = &mut lost.payload else {
        unreachable!()
    };
    body["origin"] = serde_json::json!({"instance":"e".repeat(32)});
    store.prepare_world_event(lost.clone()).await.unwrap();
    store
        .reconcile_lost_world_events(vec![instance.clone()])
        .await
        .unwrap();
    let progress = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(progress.exp, ch.exp + 320);
    let db = Connection::open(&path).unwrap();
    let kill_ids = serde_json::to_string(&kills.iter().map(|e| &e.id).collect::<Vec<_>>()).unwrap();
    let row_bytes = || -> f64 {
        db.query_row("SELECT avg(length(event)+length(kind)+length(content_fp)+length(payload)+length(payload_hash)+length(coalesce(result,''))) FROM world_events WHERE event IN (SELECT value FROM json_each(?1))", [&kill_ids], |r| r.get(0)).unwrap()
    };
    let before = row_bytes();

    assert_eq!(
        store
            .compact_settled_world_events(vec![live.id.clone(), live_inventory.id.clone()])
            .await
            .unwrap(),
        33
    );
    let after = row_bytes();
    let report = serde_json::json!({"rewarded_kill_row_bytes_before":before,"rewarded_kill_row_bytes_after":after});
    println!("JOURNAL_COMPACTION {report}");
    assert!(after * 5.0 < before, "{report}");
    for id in [&unpicked.id, &committed.id, &live.id, &operation, &lost.id] {
        assert!(
            store.world_event(id.clone()).await.unwrap().is_some(),
            "{id} keeps its payload"
        );
    }
    for id in [&kills[0].id, &settled.id] {
        let error = format!("{:#}", store.world_event(id.clone()).await.unwrap_err());
        assert!(error.contains("compacted after settlement"), "{error}");
    }
    let tombstone: serde_json::Value = serde_json::from_str(
        &db.query_row(
            "SELECT payload FROM world_events WHERE event=?1",
            [&kills[0].id],
            |r| r.get::<_, String>(0),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        tombstone,
        serde_json::json!({"compacted":{"instance":instance,"characters":[ch.id],"seed":7}})
    );

    // A replayed settled event resolves from its original hash: published, no second progress or entitlement.
    let mut replay = kills.clone();
    replay.push(settled.clone());
    for (_, record) in store.coordinate_terminals(replay).await.unwrap() {
        assert_eq!(record.unwrap().status, 2);
    }
    assert_eq!(
        store
            .publish_world_event(kills[0].clone())
            .await
            .unwrap()
            .status,
        2
    );
    assert!(
        store
            .unpicked_entitlements(settled.clone())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), progress);
    let mut changed = kills[1].clone();
    let EventPayload::Terminal(body) = &mut changed.payload else {
        unreachable!()
    };
    body.actions["publication"]["now"] = serde_json::json!(19351);
    let error = format!(
        "{:#}",
        store.prepare_world_event(changed).await.unwrap_err()
    );
    assert!(error.contains("compacted settled journal hash"), "{error}");
    store.validate_world_events().await.unwrap();
    assert_eq!(
        store
            .compact_settled_world_events(vec![live.id.clone(), live_inventory.id.clone()])
            .await
            .unwrap(),
        0
    );

    // Progress-only tombstones leave after an hour even from a live origin; a held id and entitlement history stay.
    let held = kill(3000, &progress, progress.exp + 10);
    store.coordinate_terminal(held.clone()).await.unwrap();
    store.publish_world_event(held.clone()).await.unwrap();
    let progress = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(progress.exp, ch.exp + 330);
    let published = |agg: &str, ids: &str| -> i64 {
        db.query_row(&format!("SELECT {agg}(json_extract(result,'$.published_unix')) FROM world_events WHERE event IN (SELECT value FROM json_each(?1))"), [ids], |r| r.get(0)).unwrap()
    };
    let live_ids = vec![live.id.clone(), held.id.clone()];
    let prune =
        |now: i64| store.prune_settled_world_events(vec![instance.clone()], live_ids.clone(), now);
    assert_eq!(
        prune(published("min", &kill_ids) + PROGRESS_ONLY_RETENTION_S - 1)
            .await
            .unwrap(),
        (0, 0)
    );
    assert_eq!(
        prune(published("max", &kill_ids) + PROGRESS_ONLY_RETENTION_S)
            .await
            .unwrap(),
        (32, 0)
    );
    let edge =
        published("max", &serde_json::to_string(&[&settled.id]).unwrap()) + JOURNAL_RETENTION_S;
    assert_eq!(prune(edge - 1).await.unwrap(), (0, 0));
    let allocations = settled.terminal_payload().unwrap().allocations().len();
    assert_eq!(prune(edge).await.unwrap(), (1, allocations));
    // The pickup was bound by the pruned entitlement row, so it becomes history only in the next batch.
    assert_eq!(prune(edge + 60).await.unwrap(), (1, 0));
    assert_eq!(prune(edge + 60).await.unwrap(), (0, 0));
    assert_eq!(
        store
            .coordinate_terminals(vec![held.clone()])
            .await
            .unwrap()[0]
            .1
            .as_ref()
            .unwrap()
            .status,
        2
    );
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), progress);
    for id in [
        &held.id,
        &live.id,
        &live_inventory.id,
        &committed.id,
        &unpicked.id,
    ] {
        assert!(
            store.world_event(id.clone()).await.unwrap().is_some(),
            "{id} kept"
        );
    }
    store.validate_world_events().await.unwrap();
}
