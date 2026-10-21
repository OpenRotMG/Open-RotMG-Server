use super::*;
use crate::store::{InventoryWrite, Store};
use std::{fs, path::Path};

async fn seed(path: &Path) -> (Store, Terms, Vec<InventoryWrite>) {
    let c = content();
    let store = Store::open(path, &c).unwrap();
    let mut writes = Vec::new();
    for (i, kind) in [0xa21, 0xa4c].into_iter().enumerate() {
        let a = store.list(format!("market-durable-{i}")).await.unwrap();
        let mut ch = store.create(a.id, c.get(WIZARD).unwrap()).await.unwrap();
        ch.items[4] = kind;
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: a.id,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                None,
            )
            .await
            .unwrap()[0];
        ch.items[4] = if i == 0 { 0xa4c } else { 0xa21 };
        writes.push(InventoryWrite {
            id: ch.id,
            account: a.id,
            revision: ch.revision,
            items: ch.items,
        });
    }
    let t = Terms {
        version: 31,
        accounts: [1, 2],
        names: ["Guest1".into(), "Guest2".into()],
        goods: [
            Goods {
                kind: 0xa21,
                quantity: 2,
            },
            Goods {
                kind: 0xa4c,
                quantity: 2,
            },
        ],
        meeting: [-2, 76, 72],
        expires: now() + 180,
        batch: 1,
    };
    let mut mind = Mind::new(1);
    mind.market.replace(t.clone());
    mind.revision = store.save_bot_mind(mind.clone()).await.unwrap();
    (store, t, writes)
}
#[tokio::test]
async fn community_market_receipt_write_failure_rolls_back_both_and_duplicate_transactions_cannot_repay()
 {
    let dir = std::env::temp_dir().join(format!("market-durable-{}", rand::random::<u64>()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let (store, t, writes) = seed(&path).await;
    let delivery = t.delivery(0).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER refuse_market_receipt BEFORE INSERT ON trade_transactions BEGIN SELECT RAISE(ABORT,'prepared receipt write failure'); END;").unwrap();
    let id = format!("{:032x}", 1);
    assert!(
        store
            .exchange_trade(id.clone(), writes.clone(), Some(delivery.clone()))
            .await
            .is_err()
    );
    assert_eq!(store.load(1, 1).await.unwrap().items[4], 0xa21);
    assert_eq!(store.load(2, 2).await.unwrap().items[4], 0xa4c);
    assert!(
        store
            .market_receipts(1, t.version)
            .await
            .unwrap()
            .is_empty()
    );
    conn.execute_batch("DROP TRIGGER refuse_market_receipt")
        .unwrap();
    let revisions = store
        .exchange_trade(id.clone(), writes.clone(), Some(delivery.clone()))
        .await
        .unwrap();
    assert_eq!(
        store
            .exchange_trade(id.clone(), writes.clone(), Some(delivery.clone()))
            .await
            .unwrap(),
        revisions
    );
    let mut changed = writes.clone();
    changed[0].items[4] = -1;
    assert!(
        store
            .exchange_trade(id, changed, Some(delivery.clone()))
            .await
            .is_err()
    );
    let current = writes
        .iter()
        .map(|w| InventoryWrite {
            revision: w.revision + 1,
            ..w.clone()
        })
        .collect();
    assert!(
        store
            .exchange_trade(format!("{:032x}", 2), current, Some(delivery))
            .await
            .is_err()
    );
    assert_eq!(store.load(1, 1).await.unwrap().revision, revisions[0]);
    assert_eq!(store.market_receipts(1, t.version).await.unwrap().len(), 1);
    assert!(
        store
            .market_receipts(3, t.version)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    println!("market failed-write/replay evidence: {}", dir.display());
}
#[tokio::test]
async fn community_market_crash_child_after_exchange_before_mind_update() {
    let Ok(dir) = std::env::var("COMMUNITY_MARKET_CRASH_CHILD") else {
        return;
    };
    let path = Path::new(&dir).join("game.db");
    let store = Store::open(&path, &content()).unwrap();
    let (t, writes): (Terms, Vec<InventoryWrite>) =
        serde_json::from_slice(&fs::read(Path::new(&dir).join("operation.json")).unwrap()).unwrap();
    store
        .exchange_trade(format!("{:032x}", 1), writes, Some(t.delivery(0).unwrap()))
        .await
        .unwrap();
    // The process stops at the exact durable-exchange / unsaved-mind boundary.
    std::process::abort();
}
#[tokio::test]
async fn community_market_process_crash_recovers_receipt_not_holdings_or_revision_and_never_repays()
{
    use std::os::unix::process::ExitStatusExt;
    let dir = std::env::temp_dir().join(format!("market-crash-{}", rand::random::<u64>()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let (store, t, writes) = seed(&path).await;
    fs::write(
        dir.join("operation.json"),
        serde_json::to_vec(&(t.clone(), writes.clone())).unwrap(),
    )
    .unwrap();
    drop(store);
    let status=std::process::Command::new("prlimit").arg("--core=0").arg("--").arg(std::env::current_exe().unwrap())
        .args(["--exact","bots::pursuits::market::tests::durability_tests::community_market_crash_child_after_exchange_before_mind_update","--test-threads=2","--nocapture"])
        .env("COMMUNITY_MARKET_CRASH_CHILD",&dir).stdout(fs::File::create(dir.join("child.log")).unwrap()).stderr(fs::File::create(dir.join("child.err")).unwrap()).status().unwrap();
    assert_eq!(status.signal(), Some(6));
    let store = Store::open(&path, &content()).unwrap();
    let mut mind = store.bot_mind(1).await.unwrap().unwrap();
    assert_eq!(mind.market.bargain.as_ref().unwrap().confirmed, 0);
    let receipt = store.market_receipts(1, t.version).await.unwrap();
    assert_eq!(receipt, vec![t.delivery(0).unwrap()]);
    mind.market.recover(receipt).unwrap();
    assert_eq!(mind.market.bargain.as_ref().unwrap().confirmed, 1);
    let after = store.load(1, 1).await.unwrap();
    assert_eq!(after.items[4], 0xa4c);
    store
        .exchange_trade(format!("{:032x}", 1), writes, Some(t.delivery(0).unwrap()))
        .await
        .unwrap();
    assert_eq!(store.load(1, 1).await.unwrap().revision, after.revision);
    mind.revision = store.save_bot_mind(mind.clone()).await.unwrap();
    fs::write(dir.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"signal":status.signal(),"mind":mind,"receipt":store.market_receipts(1,t.version).await.unwrap(),"revision":after.revision,"items":after.items})).unwrap()).unwrap();
    println!("market process-crash evidence: {}", dir.display());
}
