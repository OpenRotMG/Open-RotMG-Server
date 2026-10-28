use super::*;

#[tokio::test]
async fn wallets_keep_death_earnings_purchases_and_swap_separate_after_reopen() {
    let content = Content::load(Path::new("data")).unwrap();
    let root = std::env::temp_dir().join(format!("wallets-{}", rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let guid = "wallets-owner".to_owned();
    let account = store.list(guid.clone()).await.unwrap().id;
    let foreign = store.list("wallets-foreign".into()).await.unwrap().id;
    assert_eq!(
        (
            store.presentation(account).await.unwrap().credits,
            store.presentation(account).await.unwrap().fame
        ),
        (100, 0)
    );
    let mut ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(
        &content,
        200000,
        &mut <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(903),
    )
    .unwrap();
    store.progress(ch.clone()).await.unwrap();
    assert_eq!(store.presentation(account).await.unwrap().fame, 0);
    ch.alive = false;
    ch.hp = 0;
    store
        .death(ch.clone(), "wallet fixture".into())
        .await
        .unwrap();
    let paid = store.presentation(account).await.unwrap().fame;
    assert!(paid >= ch.fame && paid > 36);
    assert!(store.death(ch, "replay".into()).await.is_err());
    assert_eq!(store.presentation(account).await.unwrap().fame, paid);
    store
        .register(
            guid,
            "wallets@example.invalid".into(),
            "wallet-fixture".into(),
        )
        .await
        .unwrap();
    let mut ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    for (currency, item, price) in [(0, 0x705, 25), (1, 0xa50, 35)] {
        let slot = (4..12).find(|slot| ch.items[*slot] == -1).unwrap();
        let mut items = ch.items.clone();
        items[slot] = i32::from(item);
        let write = InventoryWrite {
            id: ch.id,
            account,
            revision: ch.revision,
            items,
        };
        let offer = purchases::Purchase {
            item,
            slot,
            price,
            currency,
        };
        let mut wrong_owner = write.clone();
        wrong_owner.account = foreign;
        assert!(store.purchase(wrong_owner, offer.clone()).await.is_err());
        let too_expensive = purchases::Purchase {
            price: i32::MAX,
            ..offer.clone()
        };
        assert!(
            store
                .purchase(write.clone(), too_expensive)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(store.load(account, ch.id).await.unwrap().items, ch.items);
        assert_eq!(
            store.purchase(write.clone(), offer.clone()).await.unwrap(),
            Some((ch.revision + 1, if currency == 0 { 75 } else { paid - 35 }))
        );
        assert!(store.purchase(write, offer).await.is_err());
        ch = store.load(account, ch.id).await.unwrap();
    }
    assert_eq!(
        (
            store.presentation(account).await.unwrap().credits,
            store.presentation(account).await.unwrap().fame
        ),
        (75, paid - 35)
    );
    for amount in [0, -2, 3, i32::MAX - 1] {
        assert!(store.swap_fame(account, amount).await.is_err());
    }
    assert_eq!(store.swap_fame(account, 2).await.unwrap(), (paid - 37, 76));
    let retained = store.presentation(account).await.unwrap();
    assert_eq!(store.presentation(foreign).await.unwrap().credits, 100);
    assert_eq!(store.presentation(foreign).await.unwrap().fame, 0);
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    let listed = store
        .characters("wallets@example.invalid".into(), "wallet-fixture".into())
        .await
        .unwrap();
    assert_eq!(
        (listed.credits, listed.fame, listed.earned_fame),
        (76, i64::from(paid - 37), i64::from(paid))
    );
    assert_eq!(store.presentation(account).await.unwrap(), retained);
    let loaded = store.load(account, ch.id).await.unwrap();
    assert_eq!(
        (loaded.items, loaded.revision, loaded.fame),
        (ch.items, ch.revision, 0)
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn class_quest_gold_pays_each_colour_once_and_survives_reopen() {
    let content = Content::load(Path::new("data")).unwrap();
    let root = std::env::temp_dir().join(format!("class-quest-gold-{}", rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let account = store.list("class-quest-gold".into()).await.unwrap().id;
    let mut balance = 100;
    for (fame, amount) in [20, 150, 400, 800, 2000]
        .into_iter()
        .zip([75, 150, 250, 750, 2500])
    {
        for (index, class) in content.classes.keys().enumerate() {
            let mut ch = store
                .create_prepared(account, content.get(*class).unwrap(), None)
                .await
                .unwrap();
            ch.fame = fame;
            store.progress(ch.clone()).await.unwrap();
            store.delete_character(account, ch.id).await.unwrap();
            if index + 1 == content.classes.len() {
                balance += amount;
                let db = Connection::open(&path).unwrap();
                assert_eq!(
                    read_presentation(&db, account, store.legacy_base, &store.classes)
                        .unwrap()
                        .0
                        .credits,
                    balance - amount
                );
                let (presentation, gold) =
                    tokio::join!(store.presentation(account), store.gold(account));
                assert_eq!(presentation.unwrap().credits, balance);
                assert_eq!(gold.unwrap().balance, balance);
            }
            assert_eq!(
                store.presentation(account).await.unwrap().credits,
                balance,
                "Gold must change only at the client colour boundary"
            );
        }
        let gold = store.gold(account).await.unwrap();
        assert_eq!(gold.rewards.len(), 1);
        assert!(gold.message().unwrap().contains(&amount.to_string()));
        store.gold_notified(account, gold.rewards).await.unwrap();
        assert!(store.gold(account).await.unwrap().rewards.is_empty());
    }
    assert_eq!(balance, 3825);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(
        reopened
            .list("class-quest-gold".into())
            .await
            .unwrap()
            .credits,
        balance
    );
    assert!(reopened.gold(account).await.unwrap().rewards.is_empty());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM gold_rewards WHERE account=?1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        5
    );
}

#[tokio::test]
async fn class_quest_gold_schema18_preserves_schema17_wallet_and_receipts() {
    let content = Content::load(Path::new("data")).unwrap();
    let root = std::env::temp_dir().join(format!(
        "class-quest-gold-migration-{}",
        rand::random::<u64>()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let account = store
        .list("class-quest-gold-migration".into())
        .await
        .unwrap()
        .id;
    for class in content.classes.keys() {
        let mut ch = store
            .create_prepared(account, content.get(*class).unwrap(), None)
            .await
            .unwrap();
        ch.fame = 20;
        store.progress(ch.clone()).await.unwrap();
        store.delete_character(account, ch.id).await.unwrap();
    }
    drop(store);
    let db = Connection::open(&path).unwrap();
    let old = SCHEMA.replace(",'class_star_blue','class_star_red','class_star_orange','class_star_yellow','class_star_white'", "");
    assert!(!old.contains("class_star_"));
    db.execute_batch("DROP TABLE gold_rewards;").unwrap();
    db.execute_batch(&old).unwrap();
    db.execute("UPDATE accounts SET credits=123456 WHERE id=?1", [account])
        .unwrap();
    db.execute(
        "INSERT INTO gold_rewards VALUES(?1,'discord_link',123,456)",
        [account],
    )
    .unwrap();
    db.execute(
        "INSERT INTO gold_rewards VALUES(?1,'first_bug_report',789,NULL)",
        [account],
    )
    .unwrap();
    db.execute_batch("DROP TABLE email_password_resets; DROP TABLE verified_emails; DROP TABLE email_verifications; DROP TABLE email_send_attempts;").unwrap();
    db.pragma_update(None, "user_version", 17).unwrap();
    let rows = || {
        db.prepare("SELECT account,reward,awarded_at,notified_at FROM gold_rewards ORDER BY reward")
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, i32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let before = rows();
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(rows(), before);
    assert_eq!(
        db.query_row("SELECT credits FROM accounts WHERE id=?1", [account], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        123456
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        21
    );
    check_schema(&db, 18).unwrap();
    let gold = reopened.gold(account).await.unwrap();
    assert_eq!(gold.balance, 123531);
    assert!(
        gold.rewards
            .iter()
            .any(|reward| reward == "class_star_blue")
    );
    assert_eq!(reopened.gold(account).await.unwrap().balance, 123531);
    drop(reopened);
    db.execute_batch("DROP TABLE gold_rewards;").unwrap();
    db.execute_batch(&old).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        Store::open(&path, &content)
            .err()
            .unwrap()
            .to_string()
            .contains("schema-18 gold reward metadata mismatch")
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
