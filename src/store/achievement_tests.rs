use super::*;
use crate::achievements::{Achievement, CATALOG};

pub(super) fn schema21_fixture(db: &Connection) {
    // Only construct an owned EMPTY achievement-era fixture; this is not recovery rollback.
    let path = Path::new(db.path().expect("fixture must be disk-backed"));
    assert!(path.is_absolute() && path.starts_with(std::env::temp_dir()) && path.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(path).unwrap().uid(), unsafe {
            libc::geteuid()
        });
    }
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        23
    );
    schema22_compat::check(db, 23).unwrap();
    gold::check_schema(db, 23).unwrap();
    for (name, _) in schema22_compat::TABLES {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r
                .get::<_, i32>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM gold_rewards WHERE reward LIKE 'achievement_%'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TABLE achievement_partners; DROP TABLE achievement_dungeons; DROP TABLE achievement_realm_credits; ALTER TABLE gold_rewards RENAME TO schema22_gold;").unwrap();
    db.execute_batch(gold::SCHEMA18).unwrap();
    db.execute_batch(
        "INSERT INTO gold_rewards SELECT * FROM schema22_gold; DROP TABLE schema22_gold;",
    )
    .unwrap();
}

fn content() -> Content {
    Content::load(Path::new("data")).unwrap()
}
async fn actor(store: &Store, content: &Content, name: &str) -> Character {
    let account = store.list(name.into()).await.unwrap().id;
    store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap()
}
async fn wallet(store: &Store, account: i32) -> i32 {
    store.presentation(account).await.unwrap().credits
}
async fn paid(store: &Store, account: i32) -> Vec<(String, i64)> {
    store.call(move |db| {
        let mut q=db.prepare("SELECT reward,awarded_at FROM gold_rewards WHERE account=?1 AND reward GLOB 'achievement_*' ORDER BY reward")?;
        Ok(q.query_map([account], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }).await.unwrap()
}

#[tokio::test]
async fn achievement_caps_award_on_progress_once_and_survive_reopen() {
    let content = content();
    let root = std::env::temp_dir().join(format!("achievement-caps-{}", rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let mut ch = actor(&store, &content, "achievement-caps").await;
    let account = ch.account;
    ch.base[..5].copy_from_slice(&content.classes[&WIZARD].caps[..5]);
    store.progress(ch.clone()).await.unwrap();
    assert_eq!(wallet(&store, account).await, 100);
    ch.base[5] = content.classes[&WIZARD].caps[5];
    let (a, b) = tokio::join!(store.progress(ch.clone()), store.progress(ch.clone()));
    a.unwrap();
    b.unwrap();
    assert_eq!(
        wallet(&store, account).await,
        125,
        "six permanent capped stats must award Baby Max once"
    );
    ch.base[6] = content.classes[&WIZARD].caps[6];
    store.progress(ch.clone()).await.unwrap();
    assert_eq!(wallet(&store, account).await, 125);
    ch.base = content.classes[&WIZARD].caps;
    store.progress(ch.clone()).await.unwrap();
    assert_eq!(wallet(&store, account).await, 225);
    let before = paid(&store, account).await;
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    store
        .progress(store.load(account, ch.id).await.unwrap())
        .await
        .unwrap();
    assert_eq!(wallet(&store, account).await, 225);
    assert_eq!(paid(&store, account).await, before);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn achievement_potential_requires_committed_permanent_death() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    for level in [16, 17, 19, 20] {
        let mut ch = actor(&store, &content, &format!("achievement-death-{level}")).await;
        ch.level = level;
        store.progress(ch.clone()).await.unwrap();
        assert!(
            store
                .achievement_dead_escape(ch.account, ch.id)
                .await
                .is_err()
        );
        ch.hp = 0;
        ch.alive = false;
        store
            .death(ch.clone(), "achievement test".into())
            .await
            .unwrap();
        assert_eq!(
            wallet(&store, ch.account).await,
            if (17..=19).contains(&level) { 110 } else { 100 }
        );
        assert!(store.death(ch.clone(), "replay".into()).await.is_err());
        store
            .achievement_dead_escape(ch.account, ch.id)
            .await
            .unwrap();
        store
            .achievement_dead_escape(ch.account, ch.id)
            .await
            .unwrap();
        assert_eq!(
            paid(&store, ch.account).await.len(),
            if (17..=19).contains(&level) { 2 } else { 1 }
        );
        assert!(
            store
                .achievement_dead_escape(ch.account + 1000, ch.id)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn achievement_damage_and_dead_attempt_survive_only_their_correct_settlement() {
    let content = std::sync::Arc::new(content());
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let mut ch = actor(&store, &content, "achievement-resurrection").await;
    ch.items[3] = 0xb3e;
    ch.revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    ch.achievements
        .extend([Achievement::HitByTruck, Achievement::NexusAfterDeath]);
    ch.hp = 0;
    ch.alive = false;
    let restored = store.resurrect(ch, 3, content).await.unwrap();
    assert!(
        !restored
            .achievements
            .contains(&Achievement::NexusAfterDeath)
    );
    assert_eq!(wallet(&store, restored.account).await, 125);
    let mut fatal = restored;
    fatal.hp = 0;
    fatal.alive = false;
    store
        .death(fatal.clone(), "later death".into())
        .await
        .unwrap();
    assert_eq!(
        paid(&store, fatal.account).await,
        vec![(
            Achievement::HitByTruck.reward().into(),
            paid(&store, fatal.account).await[0].1
        )]
    );
    assert!(
        store
            .achievement_dead_escape(fatal.account + 1, fatal.id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn achievement_trade_counts_distinct_humans_not_sessions_bots_or_replays() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let mut owner = actor(&store, &content, "achievement-trader").await;
    let bot = store
        .guest_persona_character("guest:v1:98765".into(), content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut peers = vec![bot];
    for i in 0..10 {
        peers.push(actor(&store, &content, &format!("achievement-peer-{i}")).await);
    }
    for (i, peer) in peers.iter_mut().enumerate() {
        let writes = vec![
            InventoryWrite {
                id: owner.id,
                account: owner.account,
                revision: owner.revision,
                items: owner.items.clone(),
            },
            InventoryWrite {
                id: peer.id,
                account: peer.account,
                revision: peer.revision,
                items: peer.items.clone(),
            },
        ];
        let id = format!("{:032x}", i + 1);
        let revisions = store
            .exchange_trade(id.clone(), writes.clone(), None)
            .await
            .unwrap();
        assert_eq!(
            store
                .exchange_trade(id, writes.clone(), None)
                .await
                .unwrap(),
            revisions
        );
        owner.revision = revisions[0];
        peer.revision = revisions[1];
        assert_eq!(
            wallet(&store, owner.account).await,
            if i == 10 { 135 } else { 110 }
        );
        if i == 0 {
            assert_eq!(wallet(&store, peer.account).await, 100);
        }
        let stale = store
            .exchange_trade(format!("{:032x}", 100 + i), writes, None)
            .await;
        assert!(stale.is_err());
    }
    assert_eq!(paid(&store, owner.account).await.len(), 2);
    let partner_count = store
        .call(move |db| {
            Ok(db.query_row(
                "SELECT count(*) FROM achievement_partners WHERE account=?1",
                [owner.account],
                |r| r.get::<_, i32>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(partner_count, 10);
}

#[tokio::test]
async fn achievement_dungeon_union_uses_thirteen_new_clear_deltas_across_characters() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let mut ch = actor(&store, &content, "achievement-dungeons").await;
    for i in 0..13 {
        if i == 6 {
            store.delete_character(ch.account, ch.id).await.unwrap();
            ch = store
                .create(ch.account, content.get(WIZARD).unwrap())
                .await
                .unwrap();
        }
        match i {
            0 => ch.pirate_caves += 1,
            1..=3 => ch.dungeons[i - 1] += 1,
            4 => ch.abysses += 1,
            5 => ch.trenches += 1,
            6 => ch.spider_dens += 1,
            7 => ch.mad_labs += 1,
            8 => ch.jungles += 1,
            9 => ch.manors += 1,
            10 => ch.beaches += 1,
            11 => ch.davy_lockers += 1,
            12 => ch.tombs += 1,
            _ => unreachable!(),
        }
        store.progress(ch.clone()).await.unwrap();
        store.progress(ch.clone()).await.unwrap();
        assert_eq!(
            wallet(&store, ch.account).await,
            if i == 12 { 125 } else { 100 }
        );
    }
}

#[tokio::test]
async fn achievement_realm_credits_deduplicate_one_instance_and_bind_killer_account() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let ch = actor(&store, &content, "achievement-closer").await;
    let credit = |realm, root| achievements::ClosureCredit {
        realm: format!("{realm:032x}"),
        root,
        account: ch.account,
        character: ch.id,
    };
    for root in 1..=14 {
        store.achievement_closure(credit(1, root)).await.unwrap();
        store.achievement_closure(credit(1, root)).await.unwrap();
    }
    store.achievement_closure(credit(2, 15)).await.unwrap();
    assert_eq!(wallet(&store, ch.account).await, 100);
    let mut foreign = credit(1, 15);
    foreign.account += 100;
    assert!(store.achievement_closure(foreign).await.is_err());
    store.achievement_closure(credit(1, 15)).await.unwrap();
    store.achievement_closure(credit(1, 15)).await.unwrap();
    assert_eq!(wallet(&store, ch.account).await, 125);
}

#[tokio::test]
async fn achievement_wallet_failure_rolls_back_progress_ledger_and_retries_once() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let mut ch = actor(&store, &content, "achievement-rollback").await;
    let original = ch.clone();
    let account = ch.account;
    store
        .call(move |db| {
            db.execute(
                "UPDATE accounts SET credits=2147483647 WHERE id=?1",
                [account],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    ch.base = content.classes[&WIZARD].caps;
    assert!(store.progress(ch.clone()).await.is_err());
    assert_eq!(
        store.load(account, ch.id).await.unwrap().base,
        original.base
    );
    assert!(paid(&store, account).await.is_empty());
    store
        .call(move |db| {
            db.execute("UPDATE accounts SET credits=100 WHERE id=?1", [account])?;
            Ok(())
        })
        .await
        .unwrap();
    store.progress(ch.clone()).await.unwrap();
    store.progress(ch).await.unwrap();
    assert_eq!(wallet(&store, account).await, 225);
}

#[tokio::test]
async fn achievement_schema22_preserves_old_rows_dates_wallets_and_rejects_missing_history() {
    let content = content();
    let root =
        std::env::temp_dir().join(format!("achievement-migration-{}", rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let ch = actor(&store, &content, "achievement-migration").await;
    let account = ch.account;
    store
        .call(move |db| {
            let tx = db.transaction()?;
            for key in [
                "discord_link",
                "collection_top_half",
                "first_bug_report",
                "class_star_blue",
                "class_star_red",
                "class_star_orange",
                "class_star_yellow",
                "class_star_white",
            ] {
                gold::grant(&tx, account, key)?;
            }
            tx.execute("UPDATE gold_rewards SET awarded_at=123,notified_at=456", [])?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    let balance = wallet(&store, account).await;
    drop(store);
    let db = Connection::open(&path).unwrap();
    let before:String=db.query_row("SELECT json_group_array(json_array(account,reward,awarded_at,notified_at)) FROM (SELECT * FROM gold_rewards ORDER BY reward)",[],|r|r.get(0)).unwrap();
    schema21_fixture(&db);
    db.pragma_update(None, "user_version", 21).unwrap();
    db.execute(
        "UPDATE characters SET stats=json_set(stats,'$.base',json(?1)) WHERE id=?2",
        params![
            serde_json::to_string(&content.classes[&WIZARD].caps).unwrap(),
            ch.id
        ],
    )
    .unwrap();
    drop(db);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(wallet(&store, account).await, balance);
    assert!(paid(&store, account).await.is_empty());
    drop(store);
    let db = Connection::open(&path).unwrap();
    let after:String=db.query_row("SELECT json_group_array(json_array(account,reward,awarded_at,notified_at)) FROM (SELECT * FROM gold_rewards ORDER BY reward)",[],|r|r.get(0)).unwrap();
    assert_eq!(after, before);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        23
    );
    drop(db);
    let store = Store::open(&path, &content).unwrap();
    store
        .progress(store.load(account, ch.id).await.unwrap())
        .await
        .unwrap();
    assert_eq!(wallet(&store, account).await, balance + 125);
    assert!(
        paid(&store, account)
            .await
            .iter()
            .all(|(_, time)| *time > 456)
    );
    drop(store);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE achievement_partners;")
        .unwrap();
    drop(db);
    assert!(
        Store::open(&path, &content)
            .err()
            .unwrap()
            .to_string()
            .contains("schema-23")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn achievement_stat_potion_commit_awards_base_caps_and_stale_replay_cannot_pay() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let mut ch = actor(&store, &content, "achievement-stat").await;
    ch.base = content.classes[&WIZARD].caps;
    ch.base[7] -= 1;
    store.progress(ch.clone()).await.unwrap();
    assert_eq!(wallet(&store, ch.account).await, 125);
    ch.items[4] = 0xa35;
    ch.revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    store
        .consume_stat(ch.clone(), Some(4), 7, 1, vec![])
        .await
        .unwrap();
    assert_eq!(wallet(&store, ch.account).await, 225);
    assert!(
        store
            .consume_stat(ch.clone(), Some(4), 7, 1, vec![])
            .await
            .is_err()
    );
    assert_eq!(wallet(&store, ch.account).await, 225);
    let saved = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.base, content.classes[&WIZARD].caps);
    assert_eq!(saved.items[4], -1);
}

#[tokio::test]
async fn achievement_trade_gold_failure_rolls_back_both_inventories_receipt_and_partners() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let a = actor(&store, &content, "achievement-trade-atomic-a").await;
    let b = actor(&store, &content, "achievement-trade-atomic-b").await;
    let peer = b.account;
    store
        .call(move |db| {
            db.execute("UPDATE accounts SET credits=2147483647 WHERE id=?1", [peer])?;
            Ok(())
        })
        .await
        .unwrap();
    let writes = vec![
        InventoryWrite {
            id: a.id,
            account: a.account,
            revision: a.revision,
            items: a.items.clone(),
        },
        InventoryWrite {
            id: b.id,
            account: b.account,
            revision: b.revision,
            items: b.items.clone(),
        },
    ];
    let id = "12345678901234567890123456789012".to_owned();
    assert!(
        store
            .exchange_trade(id.clone(), writes.clone(), None)
            .await
            .is_err()
    );
    assert_eq!(store.load(a.account, a.id).await.unwrap().revision, 0);
    assert_eq!(store.load(b.account, b.id).await.unwrap().revision, 0);
    assert_eq!(wallet(&store, a.account).await, 100);
    assert!(paid(&store, a.account).await.is_empty());
    store
        .call(move |db| {
            assert_eq!(
                db.query_row("SELECT count(*) FROM trade_transactions", [], |r| r
                    .get::<_, i32>(0))?,
                0
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM achievement_partners", [], |r| r
                    .get::<_, i32>(0))?,
                0
            );
            db.execute("UPDATE accounts SET credits=100 WHERE id=?1", [peer])?;
            Ok(())
        })
        .await
        .unwrap();
    store.exchange_trade(id, writes, None).await.unwrap();
    assert_eq!(wallet(&store, a.account).await, 110);
    assert_eq!(wallet(&store, b.account).await, 110);
}

#[tokio::test]
async fn achievement_catalog_rewards_once400_and_public_queries_are_private_and_read_only() {
    let content = content();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let ch = actor(&store, &content, "achievement-public").await;
    let account = ch.account;
    assert!(store.achievements(account).await.is_err());
    store
        .register(
            "achievement-public".into(),
            "achievement@example.invalid".into(),
            "achievement-password".into(),
        )
        .await
        .unwrap();
    store.compete(account).await.unwrap();
    let initial = store.hiscores(vec![]).await.unwrap();
    assert_eq!(initial["players"][0]["achievements"], 0);
    assert_eq!(initial["achievement_catalog"].as_array().unwrap().len(), 13);
    store
        .call(move |db| {
            let tx = db.transaction()?;
            for d in &CATALOG {
                achievements::grant(&tx, account, d.id)?;
                achievements::grant(&tx, account, d.id)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(wallet(&store, account).await, 500);
    let before = paid(&store, account).await;
    let profile = store.achievements(account).await.unwrap();
    assert_eq!(profile["earned"].as_array().unwrap().len(), 13);
    assert_eq!(profile.as_object().unwrap().len(), 3);
    for row in profile["earned"].as_array().unwrap() {
        assert!(row["earned_at"].as_i64().unwrap() > 0);
        assert_eq!(row.as_object().unwrap().len(), 2);
    }
    assert_eq!(
        store.hiscores(vec![]).await.unwrap()["players"][0]["achievements"],
        13
    );
    assert_eq!(paid(&store, account).await, before);
    let bot = store
        .guest_persona_character("guest:v1:98766".into(), content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(store.achievements(bot.account).await.is_err());
}
