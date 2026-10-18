use super::*;

async fn fixture() -> (std::path::PathBuf, Content, Store, i32, Connection) {
    let dir = std::env::temp_dir().join(format!("paid-name-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    let account = store
        .register(
            "name-owner".into(),
            "owner@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    let db = Connection::open(dir.join("game.db")).unwrap();
    (dir, content, store, account, db)
}
fn balance(db: &Connection, account: i32) -> (String, i32) {
    db.query_row(
        "SELECT name,credits FROM accounts WHERE id=?1",
        [account],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}
fn unrelated(db: &Connection) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
    let tables=db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut query = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = (0..query.column_count())
                .filter(|i| {
                    table != "accounts"
                        || !["name", "credits"].contains(&query.column_name(*i).unwrap())
                })
                .collect::<Vec<_>>();
            let rows = query
                .query_map([], |r| {
                    columns
                        .iter()
                        .map(|i| r.get(*i))
                        .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}

#[tokio::test]
async fn paid_names_preserve_free_policy_and_all_saves_across_restart() {
    let (dir, content, store, a, db) = fixture().await;
    let guest = store.list("name-guest".into()).await.unwrap().id;
    assert!(matches!(
        store
            .name_choice(guest, "GuestName".into(), true)
            .await
            .unwrap(),
        NameChoice::Rejected(_)
    ));
    for bad in ["A", "Ab", "Bad1", "Ünicode", "WayTooLongName"] {
        assert!(matches!(
            store.name_choice(a, bad.into(), true).await.unwrap(),
            NameChoice::Rejected(_)
        ));
    }
    store.choose_name(a, "Before".into()).await.unwrap();
    assert_eq!(balance(&db, a), ("Before".into(), 100));
    let mut dead = store.create(a, content.get(WIZARD).unwrap()).await.unwrap();
    dead.alive = false;
    dead.hp = 0;
    store.death(dead, "Test fixture".into()).await.unwrap();
    let first = store.create(a, content.get(WIZARD).unwrap()).await.unwrap();
    db.execute("UPDATE accounts SET credits=3600 WHERE id=?1", [a])
        .unwrap();
    store.purchase_character_slot(a, 7).await.unwrap();
    let second = store.create(a, content.get(WIZARD).unwrap()).await.unwrap();
    store.vault(a, 0).await.unwrap();
    let unchanged = unrelated(&db);
    assert!(store.choose_name(a, "HttpPaid".into()).await.is_err());
    assert!(matches!(
        store.name_choice(a, "After".into(), true).await.unwrap(),
        NameChoice::Committed { balance: 2000 }
    ));
    for name in ["After", "aFTER"] {
        assert!(matches!(
            store.name_choice(a, name.into(), true).await.unwrap(),
            NameChoice::Rejected(_)
        ));
    }
    assert_eq!(balance(&db, a), ("After".into(), 2000));
    // Name-only commands have no intent token: returning to an earlier available name is another purchase.
    assert!(matches!(
        store.name_choice(a, "Before".into(), true).await.unwrap(),
        NameChoice::Committed { balance: 1000 }
    ));
    assert!(matches!(
        store.name_choice(a, "After".into(), true).await.unwrap(),
        NameChoice::Committed { balance: 0 }
    ));
    assert!(matches!(
        store.name_choice(a, "NoMoney".into(), true).await.unwrap(),
        NameChoice::Rejected(_)
    ));
    assert_eq!(unrelated(&db), unchanged);
    drop(store);
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    assert_eq!(balance(&db, a), ("After".into(), 0));
    assert!(matches!(
        store.name_choice(a, "After".into(), true).await.unwrap(),
        NameChoice::Rejected(_)
    ));
    for id in [first.id, second.id] {
        assert_eq!(store.load(a, id).await.unwrap().name, "After");
        assert!(store.load(guest, id).await.is_err());
    }
    assert_eq!(unrelated(&db), unchanged);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn paid_names_rollback_storage_failures_and_do_not_mask_corruption() {
    let (dir, _content, store, a, db) = fixture().await;
    store.choose_name(a, "Before".into()).await.unwrap();
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [a])
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_name BEFORE UPDATE OF name ON accounts BEGIN SELECT RAISE(ABORT,'simulated rename write failure'); END;").unwrap();
    assert!(store.name_choice(a, "After".into(), true).await.is_err());
    assert_eq!(balance(&db, a), ("Before".into(), 1000));
    db.execute_batch("DROP TRIGGER fail_name; PRAGMA ignore_check_constraints=ON;")
        .unwrap();
    db.execute("UPDATE accounts SET credits=-1 WHERE id=?1", [a])
        .unwrap();
    assert!(store.name_choice(a, "After".into(), true).await.is_err());
    assert_eq!(balance(&db, a), ("Before".into(), -1));
    db.execute_batch("DROP TABLE credentials;").unwrap();
    assert!(store.name_choice(a, "After".into(), true).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn paid_names_serialize_competing_names_and_wallets_across_connections() {
    let (dir, content, store, a, db) = fixture().await;
    store.choose_name(a, "First".into()).await.unwrap();
    let b = store
        .register(
            "name-other".into(),
            "other@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    store.choose_name(b, "Second".into()).await.unwrap();
    db.execute("UPDATE accounts SET credits=1000", []).unwrap();
    let other = Store::open(&dir.join("game.db"), &content).unwrap();
    let (x, y) = tokio::join!(
        store.name_choice(a, "Shared".into(), true),
        other.name_choice(b, "sHARED".into(), true)
    );
    assert_eq!(
        [x.unwrap(), y.unwrap()]
            .iter()
            .filter(|r| matches!(r, NameChoice::Committed { balance: 0 }))
            .count(),
        1
    );
    assert_eq!(balance(&db, a).1 + balance(&db, b).1, 1000);
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [a])
        .unwrap();
    let (x, y) = tokio::join!(
        store.name_choice(a, "Alpha".into(), true),
        other.name_choice(a, "Beta".into(), true)
    );
    assert_eq!(
        [x.unwrap(), y.unwrap()]
            .iter()
            .filter(|r| matches!(r, NameChoice::Committed { balance: 0 }))
            .count(),
        1
    );
    assert_eq!(balance(&db, a).1, 0);
    std::fs::remove_dir_all(dir).unwrap();
}
