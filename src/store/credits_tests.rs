use super::*;

#[tokio::test]
async fn starting_gold_is_account_owned_and_survives_death_registration_and_restart() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("starting-gold-{}.db", rand::random::<u64>()));
    let s = Store::open(&path, &content).unwrap();
    let account = s.list("starting-gold".into()).await.unwrap().id;
    assert!(
        s.list("starting-gold".into())
            .await
            .unwrap()
            .xml("")
            .contains("<Credits>100</Credits>")
    );
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    s.call(move |db| {
        db.execute("UPDATE accounts SET credits=27 WHERE id=?1", [account])?;
        Ok(())
    })
    .await
    .unwrap();
    ch.hp = 0;
    ch.alive = false;
    ch.fame = 10;
    s.death(ch, "test".into()).await.unwrap();
    s.create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert_eq!(
        s.register(
            "starting-gold".into(),
            "gold@example.org".into(),
            "test-password".into()
        )
        .await
        .unwrap(),
        account
    );
    let reopened = Store::open(&path, &content).unwrap();
    assert!(
        reopened
            .characters("gold@example.org".into(), "test-password".into())
            .await
            .unwrap()
            .xml("")
            .contains("<Credits>27</Credits>")
    );
    assert!(
        reopened
            .list("another-gold-account".into())
            .await
            .unwrap()
            .xml("")
            .contains("<Credits>100</Credits>")
    );
    reopened
        .call(move |db| {
            db.execute("UPDATE accounts SET credits=0 WHERE id=?1", [account])?;
            for invalid in [-1_i64, i64::from(i32::MAX) + 1] {
                assert!(
                    db.execute(
                        "UPDATE accounts SET credits=?2 WHERE id=?1",
                        params![account, invalid]
                    )
                    .is_err()
                );
            }
            Ok(())
        })
        .await
        .unwrap();
    let restarted = Store::open(&path, &content).unwrap();
    assert!(
        restarted
            .characters("gold@example.org".into(), "test-password".into())
            .await
            .unwrap()
            .xml("")
            .contains("<Credits>0</Credits>")
    );
    assert_eq!(restarted.presentation(account).await.unwrap().fame, 10);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn legacy_accounts_receive_starting_gold_once_and_missing_gold_state_is_an_error() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("gold-migration-{}.db", rand::random::<u64>()));
    let s = Store::open(&path, &content).unwrap();
    let account = s.list("legacy-gold".into()).await.unwrap().id;
    s.call(|db| {
        db.execute_batch("ALTER TABLE accounts DROP COLUMN credits; PRAGMA user_version=7")?;
        Ok(())
    })
    .await
    .unwrap();
    let migrated = Store::open(&path, &content).unwrap();
    assert!(
        migrated
            .list("legacy-gold".into())
            .await
            .unwrap()
            .xml("")
            .contains("<Credits>100</Credits>")
    );
    migrated
        .call(move |db| {
            db.execute("UPDATE accounts SET credits=0 WHERE id=?1", [account])?;
            Ok(())
        })
        .await
        .unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    assert!(
        reopened
            .list("legacy-gold".into())
            .await
            .unwrap()
            .xml("")
            .contains("<Credits>0</Credits>")
    );
    reopened
        .call(|db| {
            db.execute_batch("ALTER TABLE accounts DROP COLUMN credits")?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        Store::open(&path, &content)
            .err()
            .unwrap()
            .to_string()
            .contains("gold balance missing")
    );
    std::fs::remove_file(path).unwrap();
}
