use super::*;

async fn fixture() -> (std::path::PathBuf, Content, Store, i32) {
    let dir = std::env::temp_dir().join(format!("character-slots-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    let account = store
        .register(
            "slot-owner".into(),
            "slots@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    (dir, content, store, account)
}

fn fund(dir: &Path, account: i32, amount: i32) -> Connection {
    let db = Connection::open(dir.join("game.db")).unwrap();
    db.execute(
        "UPDATE accounts SET credits=?2 WHERE id=?1",
        params![account, amount],
    )
    .unwrap();
    db
}

#[tokio::test]
async fn character_slots_charge_each_tier_atomically_and_keep_the_cap_quote() {
    let (dir, content, store, account) = fixture().await;
    let db = fund(&dir, account, 1400);
    let first = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(
        store
            .create(account, content.get(WIZARD).unwrap())
            .await
            .is_err()
    );
    for (timer, capacity, cost, remaining) in [(0, 2, 600, 800), (i32::MAX, 3, 800, 0)] {
        store.purchase_character_slot(account, timer).await.unwrap();
        assert_eq!(read(&db, account).unwrap(), capacity);
        assert_eq!(price(capacity - 1).unwrap(), cost);
        assert_eq!(
            db.query_row("SELECT credits FROM accounts WHERE id=?1", [account], |r| r
                .get::<_, i32>(0))
                .unwrap(),
            remaining
        );
    }
    assert!(store.purchase_character_slot(account, 1).await.is_err());
    assert_eq!(read(&db, account).unwrap(), 3);
    assert!(
        store
            .purchase_character_slot(account, 0)
            .await
            .unwrap_err()
            .is::<SlotReplay>()
    );
    fund(&dir, account, 11000);
    for timer in 4..=14 {
        store.purchase_character_slot(account, timer).await.unwrap();
    }
    assert_eq!(read(&db, account).unwrap(), 14);
    assert!(
        store
            .purchase_character_slot(account, 15)
            .await
            .unwrap_err()
            .to_string()
            .contains("limit of 14")
    );
    let listing = store
        .characters("slots@example.org".into(), "valid-password".into())
        .await
        .unwrap();
    assert_eq!(
        (
            listing.character_slots,
            listing.next_slot_price,
            listing.credits
        ),
        (14, 1000, 0)
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM character_slot_purchases", [], |r| r
            .get::<_, i32>(
            0
        ))
        .unwrap(),
        13
    );
    assert!(
        store
            .create_prepared(account, content.get(WIZARD).unwrap(), None)
            .await
            .is_err()
    );
    assert!(
        store
            .create(account, content.get(KNIGHT).unwrap())
            .await
            .is_err(),
        "capacity must not bypass class unlocks"
    );
    store.delete_character(account, first.id).await.unwrap();
    assert_eq!(read(&db, account).unwrap(), 14);
    store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn character_slots_rollback_debit_capacity_and_receipt_on_storage_failure() {
    let (dir, _, store, account) = fixture().await;
    let db = fund(&dir, account, 600);
    db.execute_batch("CREATE TRIGGER fail_slot_receipt BEFORE INSERT ON character_slot_purchases BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").unwrap();
    assert!(
        store
            .purchase_character_slot(account, 42)
            .await
            .unwrap_err()
            .to_string()
            .contains("injected receipt failure")
    );
    assert_eq!(read(&db, account).unwrap(), 1);
    assert_eq!(
        db.query_row("SELECT credits FROM accounts WHERE id=?1", [account], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        600
    );
    db.execute_batch("DROP TRIGGER fail_slot_receipt").unwrap();
    store.purchase_character_slot(account, 42).await.unwrap();
    assert_eq!(read(&db, account).unwrap(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn character_slots_serialize_replays_purchases_and_creates_across_store_connections() {
    let (dir, content, store, account) = fixture().await;
    let db = fund(&dir, account, 1400);
    let other = Store::open(&dir.join("game.db"), &content).unwrap();
    let (a, b) = tokio::join!(
        store.purchase_character_slot(account, i32::MIN),
        other.purchase_character_slot(account, i32::MIN)
    );
    match (a, b) {
        (Ok(()), Err(error)) | (Err(error), Ok(())) => assert!(error.is::<SlotReplay>()),
        _ => panic!("one committed purchase and one typed replay rejection required"),
    }
    assert_eq!(read(&db, account).unwrap(), 2);
    let class = content.get(WIZARD).unwrap();
    let (a, b) = tokio::join!(store.create(account, class), other.create(account, class));
    a.unwrap();
    b.unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1 AND alive=1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        2
    );
    assert!(store.create(account, class).await.is_err());
    fund(&dir, account, 1800);
    let (a, b) = tokio::join!(
        store.purchase_character_slot(account, 1),
        other.purchase_character_slot(account, 2)
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(read(&db, account).unwrap(), 4);
    assert_eq!(
        db.query_row("SELECT credits FROM accounts WHERE id=?1", [account], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        0
    );
    let (a, b, c) = tokio::join!(
        store.create(account, class),
        store.create(account, class),
        store.create(account, class)
    );
    assert_eq!([a, b, c].into_iter().filter(Result::is_ok).count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn character_slots_guard_direct_insert_revival_reassignment_and_capacity_reduction() {
    let (dir, content, store, account) = fixture().await;
    let db = fund(&dir, account, 600);
    let first = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let foreign = store.list("other-slot-owner".into()).await.unwrap().id;
    let other = store
        .create(foreign, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(db.execute("INSERT INTO characters(account,stats,items) SELECT account,stats,items FROM characters WHERE id=?1", [first.id]).is_err());
    assert!(
        db.execute(
            "UPDATE characters SET account=?1 WHERE id=?2",
            params![account, other.id]
        )
        .is_err()
    );
    store.purchase_character_slot(account, 3).await.unwrap();
    let second = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(
        db.execute(
            "UPDATE accounts SET character_slots=1 WHERE id=?1",
            [account]
        )
        .is_err()
    );
    store.delete_character(account, first.id).await.unwrap();
    let mut third = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(
        db.execute("UPDATE characters SET alive=1 WHERE id=?1", [first.id])
            .is_err()
    );
    third.alive = false;
    third.hp = 0;
    store.death(third, "Slot regression".into()).await.unwrap();
    store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(store.load(account, second.id).await.is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn character_slots_migrate_legacy_without_rewriting_saves_and_keep_replays_after_restart() {
    let (dir, content, store, account) = fixture().await;
    let db = fund(&dir, account, 600);
    let mut ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(&content, 400, &mut rand::thread_rng())
        .unwrap();
    ch.tex1 = 123;
    ch.tex2 = 456;
    ch.pet_kind = Some(0x177);
    ch.dungeons = [1, 2, 3];
    ch.abysses = 4;
    store.progress(ch.clone()).await.unwrap();
    let before: (String, String, i32, i32) = db
        .query_row(
            "SELECT stats,items,revision,alive FROM characters WHERE id=?1",
            [ch.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    crate::store::vaults::legacy_fixture(&db).unwrap();
    db.execute_batch("DROP TRIGGER character_slot_insert; DROP TRIGGER character_slot_update; DROP TRIGGER character_slot_capacity; DROP INDEX living_characters_by_account; DROP TABLE character_slot_purchases; ALTER TABLE accounts DROP COLUMN character_slots; CREATE UNIQUE INDEX one_living_character ON characters(account) WHERE alive=1; PRAGMA user_version=11;").unwrap();
    let migrated = Store::open(&dir.join("game.db"), &content).unwrap();
    assert_eq!(read(&db, account).unwrap(), 1);
    let after: (String, String, i32, i32) = db
        .query_row(
            "SELECT stats,items,revision,alive FROM characters WHERE id=?1",
            [ch.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(before, after);
    migrated
        .purchase_character_slot(account, -42)
        .await
        .unwrap();
    let second = migrated
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let restarted = Store::open(&dir.join("game.db"), &content).unwrap();
    assert!(
        restarted
            .purchase_character_slot(account, -42)
            .await
            .unwrap_err()
            .is::<SlotReplay>()
    );
    assert_eq!(
        restarted
            .characters("slots@example.org".into(), "valid-password".into())
            .await
            .unwrap()
            .characters
            .len(),
        2
    );
    assert!(restarted.load(account, ch.id).await.is_ok());
    assert!(restarted.load(account, second.id).await.is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn character_slots_reject_missing_or_malformed_persisted_state() {
    for damage in [
        "DELETE FROM character_slot_purchases",
        "DROP TABLE character_slot_purchases",
        "DROP TRIGGER character_slot_insert",
        "PRAGMA ignore_check_constraints=ON; UPDATE accounts SET character_slots='bad'",
        "UPDATE character_slot_purchases SET slots=3",
    ] {
        let (dir, content, store, account) = fixture().await;
        let db = fund(&dir, account, 600);
        store.purchase_character_slot(account, 4).await.unwrap();
        db.execute_batch(damage).unwrap();
        assert!(
            Store::open(&dir.join("game.db"), &content).is_err(),
            "{damage}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
