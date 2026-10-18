use super::*;

async fn fixture() -> (std::path::PathBuf, Content, Store, Character) {
    let path = std::env::temp_dir().join(format!("vaults-{}.db", rand::random::<u64>()));
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&path, &content).unwrap();
    let account = store
        .register(
            "vault-owner".into(),
            "vault@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    (path, content, store, ch)
}
fn write(ch: &Character) -> InventoryWrite {
    InventoryWrite {
        id: ch.id,
        account: ch.account,
        revision: ch.revision,
        items: ch.items.clone(),
    }
}
fn fund(path: &Path, account: i32, amount: i32) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute(
        "UPDATE accounts SET credits=?2 WHERE id=?1",
        params![account, amount],
    )
    .unwrap();
    db
}

#[tokio::test]
async fn vault_purchase_charges_exactly_once_per_identity_and_stops_at_twenty() {
    let (path, content, store, mut ch) = fixture().await;
    let db = fund(&path, ch.account, 499);
    assert!(store.purchase_vault(write(&ch), 7).await.unwrap().is_none());
    assert_eq!(store.vaults(ch.account).await.unwrap().len(), 1);
    fund(&path, ch.account, 9500);
    let mut order = (1..20).collect::<Vec<_>>();
    order.swap(0, 6);
    for (i, chest) in order.into_iter().enumerate() {
        let (revision, balance, vault) = store
            .purchase_vault(write(&ch), chest)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (revision, balance, vault.chest, vault.revision),
            (ch.revision + 1, 9500 - (i as i32 + 1) * 500, chest, 0)
        );
        assert_eq!(vault.items, vec![-1; 8]);
        ch.revision = revision;
        assert!(store.purchase_vault(write(&ch), chest).await.is_err());
    }
    for chest in [-1, 0, 1, 19, 20] {
        assert!(store.purchase_vault(write(&ch), chest).await.is_err());
    }
    assert_eq!(read(&db, ch.account).unwrap().len(), 20);
    assert_eq!(store.presentation(ch.account).await.unwrap().credits, 0);
    drop(store);
    assert_eq!(
        Store::open(&path, &content)
            .unwrap()
            .vaults(ch.account)
            .await
            .unwrap()
            .len(),
        20
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn vault_purchase_rolls_back_all_state_and_rejects_stale_foreign_dead_or_guest() {
    let (path, _, store, ch) = fixture().await;
    let db = fund(&path, ch.account, 1000);
    db.execute_batch("CREATE TRIGGER fail_vault BEFORE INSERT ON vaults BEGIN SELECT RAISE(ABORT,'injected vault insert failure'); END;").unwrap();
    assert!(
        store
            .purchase_vault(write(&ch), 1)
            .await
            .unwrap_err()
            .to_string()
            .contains("injected vault")
    );
    assert_eq!(store.presentation(ch.account).await.unwrap().credits, 1000);
    assert_eq!(
        store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    assert_eq!(store.vaults(ch.account).await.unwrap().len(), 1);
    db.execute_batch("DROP TRIGGER fail_vault;").unwrap();
    let guest = store.list("other-vault-guest".into()).await.unwrap();
    let mut foreign = write(&ch);
    foreign.account = guest.id;
    assert!(store.purchase_vault(foreign, 1).await.is_err());
    let mut stale = write(&ch);
    stale.revision += 1;
    assert!(store.purchase_vault(stale, 1).await.is_err());
    let mut changed = write(&ch);
    changed.items[4] = -1;
    assert!(store.purchase_vault(changed, 1).await.is_err());
    db.execute("DELETE FROM credentials WHERE account=?1", [ch.account])
        .unwrap();
    assert!(store.purchase_vault(write(&ch), 1).await.is_err());
    db.execute("UPDATE characters SET alive=0 WHERE id=?1", [ch.id])
        .unwrap();
    assert!(store.purchase_vault(write(&ch), 1).await.is_err());
    assert_eq!(store.presentation(ch.account).await.unwrap().credits, 1000);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn vault_independent_store_competition_conserves_gold_and_one_chest() {
    let (path, content, store, ch) = fixture().await;
    fund(&path, ch.account, 1000);
    let other = Store::open(&path, &content).unwrap();
    let (a, b) = tokio::join!(
        store.purchase_vault(write(&ch), 9),
        other.purchase_vault(write(&ch), 9)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(store.presentation(ch.account).await.unwrap().credits, 500);
    assert_eq!(
        store
            .vaults(ch.account)
            .await
            .unwrap()
            .iter()
            .map(|v| v.chest)
            .collect::<Vec<_>>(),
        vec![0, 9]
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn vault_two_chest_cas_is_atomic_and_missing_paid_storage_is_never_recreated() {
    let (path, content, store, mut ch) = fixture().await;
    let db = fund(&path, ch.account, 1000);
    ch.revision = store
        .purchase_vault(write(&ch), 3)
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut first = store.vault(ch.account, 0).await.unwrap();
    let mut second = store.vault(ch.account, 3).await.unwrap();
    first.items[0] = ch.items[4];
    second.items[0] = ch.items[5];
    let mut inventory = write(&ch);
    inventory.items[4] = -1;
    inventory.items[5] = -1;
    store
        .exchange(vec![inventory], vec![first.clone(), second.clone()])
        .await
        .unwrap();
    ch = store.load(ch.account, ch.id).await.unwrap();
    first = store.vault(ch.account, 0).await.unwrap();
    second = store.vault(ch.account, 3).await.unwrap();
    std::mem::swap(&mut first.items[0], &mut second.items[0]);
    let mut stale = second.clone();
    stale.revision += 1;
    assert!(
        store
            .exchange(vec![write(&ch)], vec![first.clone(), stale])
            .await
            .is_err()
    );
    assert_eq!(store.vault(ch.account, 0).await.unwrap().items[0], 0xa22);
    assert_eq!(
        store.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    store
        .exchange(vec![write(&ch)], vec![first, second])
        .await
        .unwrap();
    assert_eq!(store.vault(ch.account, 3).await.unwrap().items[0], 0xa22);
    db.execute(
        "DELETE FROM vaults WHERE account=?1 AND chest=3",
        [ch.account],
    )
    .unwrap();
    assert!(store.vaults(ch.account).await.is_err());
    assert!(Store::open(&path, &content).is_err());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM vaults WHERE account=?1",
            [ch.account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn vault_legacy_migration_preserves_items_revision_and_intentionally_provisions_unread_accounts()
 {
    let (path, content, store, ch) = fixture().await;
    let legacy = store.list("legacy-unread-vault".into()).await.unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE vaults RENAME TO current_vaults;
CREATE TABLE vaults(account INTEGER PRIMARY KEY REFERENCES accounts(id),items TEXT NOT NULL,revision INTEGER NOT NULL);
INSERT INTO vaults SELECT account,items,revision FROM current_vaults WHERE account=1;
DROP TABLE current_vaults;
ALTER TABLE accounts DROP COLUMN vault_mask;
PRAGMA user_version=12;").unwrap();
    db.execute(
        "UPDATE vaults SET items='[2594,2595,-1,-1,-1,-1,-1,-1]',revision=37 WHERE account=?1",
        [ch.account],
    )
    .unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    let saved = reopened.vault(ch.account, 0).await.unwrap();
    assert_eq!(
        (saved.revision, saved.items[..2].to_vec()),
        (37, vec![0xa22, 0xa23])
    );
    assert_eq!(
        reopened.vault(legacy.id, 0).await.unwrap().items,
        vec![-1; 8]
    );
    db.execute_batch("PRAGMA user_version=12;").unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "current shape cannot masquerade as legacy"
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn vault_second_chest_stat_and_xp_consumption_bind_the_exact_revision() {
    for xp in [false, true] {
        let (path, _, store, mut ch) = fixture().await;
        fund(&path, ch.account, 500);
        ch.revision = store
            .purchase_vault(write(&ch), 4)
            .await
            .unwrap()
            .unwrap()
            .0;
        let mut source = store.vault(ch.account, 4).await.unwrap();
        source.items[0] = if xp { 0xc42 } else { 0xa1f };
        ch.revision = store
            .exchange(vec![write(&ch)], vec![source])
            .await
            .unwrap()[0];
        let mut consumed = store.vault(ch.account, 4).await.unwrap();
        consumed.items[0] = -1;
        let mut stale = consumed.clone();
        stale.revision += 1;
        if xp {
            assert!(
                store
                    .consume_xp_booster(write(&ch), None, vec![stale])
                    .await
                    .is_err()
            );
            ch.revision = store
                .consume_xp_booster(write(&ch), None, vec![consumed])
                .await
                .unwrap();
            assert!(store.load(ch.account, ch.id).await.unwrap().xp_boost.active);
        } else {
            assert!(
                store
                    .consume_stat(ch.clone(), None, 2, 1, vec![stale])
                    .await
                    .is_err()
            );
            ch.revision = store
                .consume_stat(ch.clone(), None, 2, 1, vec![consumed])
                .await
                .unwrap();
            assert_eq!(
                store.load(ch.account, ch.id).await.unwrap().base[2],
                ch.base[2] + 1
            );
        }
        assert_eq!(store.vault(ch.account, 0).await.unwrap().revision, 0);
        assert_eq!(store.vault(ch.account, 4).await.unwrap().revision, 2);
        assert_eq!(store.vault(ch.account, 4).await.unwrap().items, vec![-1; 8]);
        assert_eq!(
            store.load(ch.account, ch.id).await.unwrap().revision,
            ch.revision
        );
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn vault_current_schema_rejects_missing_unknown_or_malformed_state() {
    for mutation in [
        "PRAGMA user_version=14",
        "DROP TABLE vaults",
        "ALTER TABLE accounts DROP COLUMN vault_mask",
        "ALTER TABLE vaults DROP COLUMN items",
        "PRAGMA ignore_check_constraints=ON; UPDATE accounts SET vault_mask=2",
        "PRAGMA ignore_check_constraints=ON; UPDATE vaults SET chest=20",
        "PRAGMA ignore_check_constraints=ON; UPDATE vaults SET revision=-1",
        "PRAGMA ignore_check_constraints=ON; UPDATE vaults SET items='[65536,-1,-1,-1,-1,-1,-1,-1]'",
        "PRAGMA foreign_keys=OFF; UPDATE vaults SET account=99999",
    ] {
        let (path, content, store, _) = fixture().await;
        drop(store);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(Store::open(&path, &content).is_err(), "{mutation}");
        std::fs::remove_file(path).unwrap();
    }
}
