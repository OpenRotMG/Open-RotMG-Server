use super::*;

async fn registered(s: &Store, name: &str) -> i32 {
    let a = s.list(name.into()).await.unwrap();
    s.register(
        name.into(),
        format!("{name}@example.invalid"),
        "password123".into(),
    )
    .await
    .unwrap();
    s.compete(a.id).await.unwrap();
    a.id
}
fn write(c: &Character) -> InventoryWrite {
    InventoryWrite {
        id: c.id,
        account: c.account,
        revision: c.revision,
        items: c.items.clone(),
    }
}

#[tokio::test]
async fn collection_rewards_use_competitive_ties_once_and_only_after_new_progress() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    let a = registered(&s, "leader").await;
    let b = registered(&s, "runner").await;
    let c = registered(&s, "collector").await;
    let d = registered(&s, "fourth").await;
    s.call(move |db| {
        for (account, count) in [(a, 3), (b, 2)] {
            for item in 0..count {
                db.execute(
                    "INSERT INTO collections VALUES(?1,?2,unixepoch())",
                    params![account, item],
                )?;
            }
        }
        assert!(!collection(db, c)?);
        Ok(())
    })
    .await
    .unwrap();
    let mut ch = s.create(c, content.get(WIZARD).unwrap()).await.unwrap();
    for (item, balance) in [(0xa22, 100), (0xa23, 200), (0xa24, 200)] {
        ch.revision = s
            .exchange_collected(vec![write(&ch)], Vec::new(), Some(item))
            .await
            .unwrap()[0];
        assert_eq!(s.gold(c).await.unwrap().balance, balance);
    }
    let gold = s.gold(c).await.unwrap();
    assert_eq!(gold.rewards, [COLLECTION]);
    assert_eq!(s.gold(a).await.unwrap().balance, 100);
    assert_eq!(s.gold(b).await.unwrap().balance, 100);
    assert_eq!(s.gold(d).await.unwrap().balance, 100);
    let mut stale = write(&ch);
    stale.revision -= 1;
    assert!(
        s.exchange_collected(vec![stale], Vec::new(), Some(0xa25))
            .await
            .is_err()
    );
    assert_eq!(
        s.collection(c).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    s.gold_notified(c, gold.rewards).await.unwrap();
    assert!(s.gold(c).await.unwrap().rewards.is_empty());
    assert!(s.gold_notified(c, vec![COLLECTION.into()]).await.is_err());
}

#[tokio::test]
async fn gold_and_collection_roll_back_with_inventory_and_vault_conflicts() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    let a = registered(&s, "rollback").await;
    let ch = s.create(a, content.get(WIZARD).unwrap()).await.unwrap();
    let mut vault = s.vault(a, 0).await.unwrap();
    vault.revision += 1;
    assert!(
        s.exchange_collected(vec![write(&ch)], vec![vault], Some(0xa22))
            .await
            .is_err()
    );
    assert_eq!(s.load(a, ch.id).await.unwrap().revision, ch.revision);
    assert_eq!(s.gold(a).await.unwrap().balance, 100);
    assert!(s.gold(a).await.unwrap().rewards.is_empty());
    assert!(
        s.collection(a).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    s.call(move |db| {
        db.execute("UPDATE accounts SET credits=2147483647 WHERE id=?1", [a])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.exchange_collected(vec![write(&ch)], Vec::new(), Some(0xa22))
            .await
            .is_err()
    );
    assert_eq!(s.load(a, ch.id).await.unwrap().revision, ch.revision);
    assert!(s.gold(a).await.unwrap().rewards.is_empty());
    assert!(
        s.collection(a).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn guests_and_bots_never_receive_collection_rewards() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    let guest = s.list("guest".into()).await.unwrap().id;
    let bot = registered(&s, "bot").await;
    s.call(move |db| {
        db.execute("INSERT INTO shore_bots VALUES('bot',?1)", [bot])?;
        Ok(())
    })
    .await
    .unwrap();
    for account in [guest, bot] {
        let ch = s
            .create(account, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        s.exchange_collected(vec![write(&ch)], Vec::new(), Some(0xa22))
            .await
            .unwrap();
        assert_eq!(s.gold(account).await.unwrap().balance, 100);
        assert!(s.gold(account).await.unwrap().rewards.is_empty());
    }
}

#[tokio::test]
async fn discord_reward_overflow_rolls_back_link_and_preserves_the_one_use_code() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    let a = registered(&s, "discord").await;
    let code = s.issue_discord_code(a).await.unwrap();
    s.call(move |db| {
        db.execute("UPDATE accounts SET credits=2147483647 WHERE id=?1", [a])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.redeem_discord_code(code.clone(), "12345678901234567".into())
            .await
            .is_err()
    );
    assert!(s.gold(a).await.unwrap().rewards.is_empty());
    assert!(s.linked_account("12345678901234567".into()).await.is_err());
    s.call(move |db| {
        db.execute("UPDATE accounts SET credits=0 WHERE id=?1", [a])?;
        Ok(())
    })
    .await
    .unwrap();
    s.redeem_discord_code(code.clone(), "12345678901234567".into())
        .await
        .unwrap();
    assert_eq!(s.gold(a).await.unwrap().balance, 100);
    assert!(
        s.redeem_discord_code(code, "12345678901234567".into())
            .await
            .is_err()
    );
    assert_eq!(s.gold(a).await.unwrap().rewards, [DISCORD]);
}

#[tokio::test]
async fn migration_does_not_backfill_and_reopen_preserves_claims_and_notifications() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("gold-{}.db", rand::random::<u64>()));
    let s = Store::open(&path, &content).unwrap();
    let old = registered(&s, "old").await;
    s.call(move |db| {
        db.execute(
            "INSERT INTO discord_links VALUES(?1,'12345678901234567')",
            [old],
        )?;
        crate::store::vaults::legacy_fixture(db)?;
        db.execute_batch("DROP TABLE gold_rewards; PRAGMA user_version=9")?;
        Ok(())
    })
    .await
    .unwrap();
    let migrated = Store::open(&path, &content).unwrap();
    assert_eq!(migrated.gold(old).await.unwrap().balance, 100);
    assert!(migrated.gold(old).await.unwrap().rewards.is_empty());
    let new = registered(&migrated, "new").await;
    let code = migrated.issue_discord_code(new).await.unwrap();
    migrated
        .redeem_discord_code(code, "12345678901234568".into())
        .await
        .unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.gold(new).await.unwrap().balance, 200);
    assert_eq!(reopened.gold(new).await.unwrap().rewards, [DISCORD]);
    reopened
        .gold_notified(new, vec![DISCORD.into()])
        .await
        .unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    assert!(reopened.gold(new).await.unwrap().rewards.is_empty());
    assert_eq!(reopened.gold(new).await.unwrap().balance, 200);
    reopened
        .call(|db| {
            db.execute_batch("DROP TABLE gold_rewards")?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(Store::open(&path, &content).is_err());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn bug_reward_preserves_v10_claims_and_is_once_across_concurrency_and_restart() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("bug-gold-{}.db", rand::random::<u64>()));
    let s = Store::open(&path, &content).unwrap();
    let account = s.list("reporter".into()).await.unwrap().id;
    s.call(move |db| {
        let tx = db.transaction()?;
        grant(&tx, account, DISCORD)?;
        grant(&tx, account, COLLECTION)?;
        tx.execute("UPDATE gold_rewards SET notified_at=unixepoch() WHERE reward=?1", [DISCORD])?;
        tx.execute_batch("ALTER TABLE gold_rewards RENAME TO copied_rewards")?;
        tx.execute_batch(&SCHEMA.replace(",'first_bug_report'", ""))?;
        crate::store::vaults::legacy_fixture(&tx)?;
        tx.execute_batch("INSERT INTO gold_rewards SELECT * FROM copied_rewards; DROP TABLE copied_rewards; PRAGMA user_version=10")?;
        tx.commit()?;
        Ok(())
    }).await.unwrap();
    let before = s.gold(account).await.unwrap();
    assert_eq!(before.balance, 300);
    assert_eq!(before.rewards, [COLLECTION]);
    drop(s);
    let s = Store::open(&path, &content).unwrap();
    assert_eq!(s.gold(account).await.unwrap().balance, before.balance);
    assert_eq!(s.gold(account).await.unwrap().rewards, before.rewards);
    let (one, two) = tokio::join!(s.reward_first_bug(account), s.reward_first_bug(account));
    one.unwrap();
    two.unwrap();
    let gold = s.gold(account).await.unwrap();
    assert_eq!(gold.balance, 350);
    assert_eq!(gold.rewards.len(), 2);
    assert!(
        gold.message()
            .unwrap()
            .starts_with("You received 150 Gold for ")
    );
    s.gold_notified(account, gold.rewards).await.unwrap();
    drop(s);
    let s = Store::open(&path, &content).unwrap();
    s.reward_first_bug(account).await.unwrap();
    assert_eq!(s.gold(account).await.unwrap().balance, 350);
    assert!(s.gold(account).await.unwrap().rewards.is_empty());
    drop(s);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn failed_bug_rewards_roll_back_and_guests_keep_claims_after_registration() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    let account = s.list("reporter".into()).await.unwrap().id;
    assert!(s.reward_first_bug(-1).await.is_err());
    s.call(move |db| {
        db.execute(
            "UPDATE accounts SET credits=2147483647 WHERE id=?1",
            [account],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.reward_first_bug(account).await.is_err());
    assert!(s.gold(account).await.unwrap().rewards.is_empty());
    s.call(move |db| {
        db.execute("UPDATE accounts SET credits=100 WHERE id=?1", [account])?;
        db.execute("INSERT INTO shore_bots VALUES('reporter',?1)", [account])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.reward_first_bug(account).await.is_err());
    assert_eq!(s.gold(account).await.unwrap().balance, 100);
    s.call(move |db| {
        db.execute("DELETE FROM shore_bots WHERE account=?1", [account])?;
        Ok(())
    })
    .await
    .unwrap();
    s.reward_first_bug(account).await.unwrap();
    s.register(
        "reporter".into(),
        "reporter@example.invalid".into(),
        "password123".into(),
    )
    .await
    .unwrap();
    s.reward_first_bug(account).await.unwrap();
    assert_eq!(s.gold(account).await.unwrap().balance, 150);
    assert_eq!(s.gold(account).await.unwrap().rewards, [BUG]);
}
