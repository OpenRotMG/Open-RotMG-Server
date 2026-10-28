use super::*;

async fn fresh_schema_account(content: &Content) -> (std::path::PathBuf, i32) {
    let dir = std::env::temp_dir().join(format!(
        "community-schema-preserved-{}",
        rand::random::<u64>()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    eprintln!("fresh schema fixture: {}", path.display());
    let store = Store::open(&path, content).unwrap();
    let ch = store
        .persona_character(
            "persona:v1:Schema".into(),
            vec![Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap()],
            vec![[-1; 8]],
            0,
            None,
        )
        .await
        .unwrap();
    store
        .save_bot_mind(crate::bots::pursuits::Mind::new(ch.account))
        .await
        .unwrap();
    let mut vault = store.vault(ch.account, 0).await.unwrap();
    vault.items[0] = 0xa22;
    store
        .exchange_transaction(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items,
            }],
            vec![vault],
            None,
            Some(("schema-fixture-receipt".into(), None)),
        )
        .await
        .unwrap();
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE accounts SET vault_mask=3 WHERE id=?1", [ch.account])
        .unwrap();
    db.execute("INSERT INTO vaults(account,chest,items,revision) VALUES(?1,1,'[2594,-1,-1,-1,-1,-1,-1,-1]',7)", [ch.account]).unwrap();
    (path, ch.account)
}

fn canonical_schema_rows(db: &Connection) -> Vec<String> {
    [
        "SELECT json_group_array(json_array(id,guid,name,credits,character_slots,vault_mask)) FROM accounts",
        "SELECT json_group_array(json_array(id,account,stats,items,revision,alive)) FROM characters",
        "SELECT json_group_array(json_array(account,chest,items,revision)) FROM vaults",
        "SELECT json_group_array(json_array(account,balance,earned)) FROM account_fame",
    ].map(|sql| db.query_row(sql, [], |r| r.get(0)).unwrap()).to_vec()
}

#[tokio::test]
async fn community_schema17_migrates_canonical16_and_committed_reopen_preserves_account_character_vault_mind_and_receipt()
 {
    let content = Content::load(Path::new("data")).unwrap();
    let (path, account) = fresh_schema_account(&content).await;
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE email_password_resets; DROP TABLE verified_emails; DROP TABLE email_verifications; DROP TABLE email_send_attempts; DROP TABLE legacy_claims; DROP TABLE level_credits; ALTER TABLE deaths DROP COLUMN receipt; PRAGMA user_version=16;").unwrap();
    let before = canonical_schema_rows(&db);
    let feature_rows = |db: &Connection| -> Vec<String> {
        ["SELECT json_group_array(json_array(account,revision,mind)) FROM bot_minds",
         "SELECT json_group_array(json_array(id,owners,writes,revisions,delivery,market_key)) FROM trade_transactions"]
        .map(|sql| db.query_row(sql, [], |r| r.get(0)).unwrap()).to_vec()
    };
    let features = feature_rows(&db);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        16
    );
    let reopened = Store::open(&path, &content)
        .expect("committed schema17 must reopen without losing canonical or community state");
    assert_eq!(reopened.vaults(account).await.unwrap().len(), 2);
    assert_eq!(
        reopened.bot_mind(account).await.unwrap().unwrap().revision,
        1
    );
    assert_eq!(canonical_schema_rows(&db), before);
    assert_eq!(feature_rows(&db), features);
    drop(reopened);
    db.execute_batch("PRAGMA user_version=22").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        Store::open(&path, &content)
            .err()
            .unwrap()
            .to_string()
            .contains("unsupported game database schema 22")
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
