use super::*;
use crate::achievements::{Achievement, CATALOG};
use std::sync::Arc;

async fn fixture(schema22: bool) -> (std::path::PathBuf, Arc<Content>, Store, Character) {
    let dir = std::env::temp_dir().join(format!("schema22-compat-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Arc::new(Content::load(Path::new("data")).unwrap());
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    let account = store
        .list("synthetic-compat-owner".into())
        .await
        .unwrap()
        .id;
    let peer = store.list("synthetic-compat-peer".into()).await.unwrap().id;
    let ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    if schema22 {
        let id = ch.id;
        // The active store has already created22; seed only owned prior receipt/history rows.
        store.call(move |db| {
            let tx = db.transaction()?;
            tx.execute("INSERT INTO gold_rewards VALUES(?1,'discord_link',111,222)", [account])?;
            tx.execute("INSERT INTO gold_rewards VALUES(?1,'achievement_hit_by_truck',333,NULL)", [account])?;
            tx.execute("INSERT INTO gold_rewards VALUES(?1,'achievement_trader',334,335)", [account])?;
            tx.execute("UPDATE accounts SET credits=135 WHERE id=?1", [account])?;
            tx.execute("INSERT INTO achievement_partners VALUES(?1,?2)", params![account,peer])?;
            tx.execute("INSERT INTO achievement_dungeons VALUES(?1,'pirate-cave')", [account])?;
            tx.execute("INSERT INTO achievement_realm_credits VALUES(?1,?2,1)", params![account,"a".repeat(32)])?;
            tx.execute("UPDATE characters SET stats=json_set(stats,'$.achievements',json('[\"hit_by_truck\"]')) WHERE id=?1", [id])?;
            tx.commit()?;
            Ok(())
        }).await.unwrap();
    }
    let ch = store.load(account, ch.id).await.unwrap();
    (dir, content, store, ch)
}

fn snapshot(path: &Path) -> Vec<String> {
    let db = Connection::open(path).unwrap();
    let mut out = vec![format!(
        "version:{}",
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap()
    )];
    let mut meta = db.prepare("SELECT name,coalesce(sql,'') FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
    out.extend(
        meta.query_map([], |r| {
            Ok(format!(
                "{}:{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?
            ))
        })
        .unwrap()
        .map(|row| row.unwrap()),
    );
    let mut q = db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
    let tables = q
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for table in tables {
        let mut q = db.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns = q.column_count();
        let mut rows = q
            .query_map([], |r| {
                let values = (0..columns)
                    .map(|i| r.get::<_, rusqlite::types::Value>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(format!("{table}:{values:?}"))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.sort();
        out.extend(rows);
    }
    out
}

fn ledger(path: &Path) -> Vec<(i32, String, i64, Option<i64>)> {
    let db = Connection::open(path).unwrap();
    let mut q = db.prepare("SELECT account,reward,awarded_at,notified_at FROM gold_rewards ORDER BY account,reward").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[tokio::test]
async fn schema23_promotion_fresh_store_has23_without_awards() {
    let (dir, content, store, ch) = fixture(false).await;
    let path = dir.join("game.db");
    let before = snapshot(&path);
    assert!(ch.achievements.is_empty());
    assert!(
        serde_json::to_value(&ch)
            .unwrap()
            .get("achievements")
            .is_none()
    );
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(snapshot(&path), before);
    let db = Connection::open(&path).unwrap();
    gold::check_schema(&db, 23).unwrap();
    check(&db, 23).unwrap();
    for (name, _) in TABLES {
        assert!(
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name=?1)",
                [name],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
        );
    }
    assert_eq!(
        reopened.presentation(ch.account).await.unwrap().credits,
        100
    );
    assert!(ledger(&path).is_empty());
    drop(db);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema23_known23_reopen_retains_every_logical_row_and_pending_gold() {
    let (dir, content, store, ch) = fixture(true).await;
    let path = dir.join("game.db");
    let before = snapshot(&path);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(snapshot(&path), before);
    assert_eq!(reopened.vaults(ch.account).await.unwrap().len(), 1);
    let saved = reopened.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.achievements, ch.achievements);
    assert_eq!(saved.achievements.len(), 1);
    let gold = reopened.gold(ch.account).await.unwrap();
    assert_eq!(gold.balance, 135);
    assert_eq!(gold.rewards, ["achievement_hit_by_truck"]);
    assert_eq!(
        gold.message().unwrap(),
        "You received 25 Gold for Hit by a truck!"
    );
    assert_eq!(snapshot(&path), before);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema22_compat_real_writers_preserve_flags_ledger_wallet_dates_and_replay_guards() {
    let (dir, content, store, mut ch) = fixture(true).await;
    let path = dir.join("game.db");
    let history = ledger(&path);
    let mut flags = ch.achievements.clone();
    ch.exp += 1;
    store.progress(ch.clone()).await.unwrap();
    ch = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(ch.achievements, flags);
    let mut items = ch.items.clone();
    items[4] = 0xa35;
    items[3] = 0xb3e;
    let write = InventoryWrite {
        id: ch.id,
        account: ch.account,
        revision: ch.revision,
        items,
    };
    let mut vault = store.vault(ch.account, 0).await.unwrap();
    vault.items[0] = 0xa35;
    store
        .exchange(vec![write.clone()], vec![vault])
        .await
        .unwrap();
    assert!(store.exchange(vec![write], vec![]).await.is_err());
    ch = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(store.vault(ch.account, 0).await.unwrap().items[0], 0xa35);
    assert_eq!(ch.achievements, flags);
    store
        .consume_stat(ch.clone(), Some(4), 7, 1, vec![])
        .await
        .unwrap();
    assert!(
        store
            .consume_stat(ch.clone(), Some(4), 7, 1, vec![])
            .await
            .is_err()
    );
    ch = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(ch.achievements, flags);
    let mut fatal = ch.clone();
    fatal.alive = false;
    fatal.hp = 0;
    ch = store
        .resurrect(fatal.clone(), 3, content.clone())
        .await
        .unwrap();
    flags.remove(&Achievement::NexusAfterDeath);
    assert!(store.resurrect(fatal, 3, content.clone()).await.is_err());
    assert_eq!(ch.achievements, flags);
    ch.alive = false;
    ch.hp = 0;
    store
        .death(ch.clone(), "synthetic compatibility test".into())
        .await
        .unwrap();
    assert!(store.death(ch.clone(), "replay".into()).await.is_err());
    let db = Connection::open(&path).unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Character>(&raw)
            .unwrap()
            .achievements,
        flags
    );
    assert_eq!(ledger(&path), history);
    assert_eq!(store.presentation(ch.account).await.unwrap().credits, 135);
    store.reward_first_bug(ch.account).await.unwrap();
    store.reward_first_bug(ch.account).await.unwrap();
    assert_eq!(store.presentation(ch.account).await.unwrap().credits, 185);
    for old in &history {
        assert!(ledger(&path).contains(old));
    }
    let pending = store.gold(ch.account).await.unwrap();
    assert_eq!(pending.rewards.len(), 2);
    assert!(
        pending
            .message()
            .unwrap()
            .starts_with("You received 75 Gold for ")
    );
    store
        .gold_notified(ch.account, pending.rewards.clone())
        .await
        .unwrap();
    let notified = snapshot(&path);
    assert!(
        store
            .gold_notified(ch.account, pending.rewards)
            .await
            .is_err()
    );
    assert_eq!(snapshot(&path), notified);
    let rows = ledger(&path);
    assert!(rows.contains(&(ch.account, "discord_link".into(), 111, Some(222))));
    assert!(rows.contains(&(ch.account, "achievement_trader".into(), 334, Some(335))));
    assert!(
        rows.iter()
            .any(|r| r.1 == "achievement_hit_by_truck" && r.2 == 333 && r.3.is_some())
    );
    for (table, _) in TABLES {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i32>(0))
                .unwrap(),
            1
        );
    }
    drop(db);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(snapshot(&path), notified);
    assert!(
        reopened
            .fame(ch.account, ch.id)
            .await
            .unwrap()
            .contains("<Fame>")
    );
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema22_compat_missing_malformed_unknown_and_mixed_fail_without_logical_writes() {
    let mut mutations = vec![
        "DROP TABLE gold_rewards".to_owned(),
        "PRAGMA user_version=24".into(),
        "PRAGMA user_version=999".into(),
        "PRAGMA user_version=21".into(),
    ];
    for (name, _) in TABLES {
        mutations.push(format!("DROP TABLE {name}"));
        mutations.push(format!(
            "ALTER TABLE {name} ADD COLUMN unknown_metadata TEXT"
        ));
    }
    mutations.push(format!(
        "ALTER TABLE gold_rewards RENAME TO held_bad_rewards;{}",
        GOLD.replace("awarded_at>0", "awarded_at>=0")
    ));
    for mutation in mutations {
        let (dir, content, store, _) = fixture(true).await;
        let path = dir.join("game.db");
        drop(store);
        Connection::open(&path)
            .unwrap()
            .execute_batch(&mutation)
            .unwrap();
        let before = snapshot(&path);
        assert!(Store::open(&path, &content).is_err(), "{mutation}");
        assert_eq!(snapshot(&path), before, "{mutation}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    // A malformed claimed21 schema is built directly in memory, not by rewinding a DB.
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(TABLES[0].1).unwrap();
    let before: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [TABLES[0].0],
            |r| r.get(0),
        )
        .unwrap();
    assert!(check(&db, 21).is_err());
    let after: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [TABLES[0].0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[test]
fn schema22_compat_actual_serde_and_gold_message_keep_closed_known_metadata() {
    let content = Content::load(Path::new("data")).unwrap();
    let mut ch = Character::fresh(content.get(WIZARD).unwrap()).unwrap();
    ch.achievements.insert(Achievement::HitByTruck);
    let raw = serde_json::to_value(&ch).unwrap();
    assert_eq!(
        serde_json::from_value::<Character>(raw.clone()).unwrap(),
        ch
    );
    let mut bad = raw;
    bad["achievements"] = serde_json::json!(["future_unknown_flag"]);
    assert!(serde_json::from_value::<Character>(bad).is_err());
    let rewards = CATALOG.iter().map(|d| d.id.reward().to_owned()).collect();
    let message = gold::Gold {
        balance: 400,
        rewards,
    }
    .message()
    .unwrap();
    assert!(message.starts_with("You received 400 Gold for "));
    for definition in CATALOG {
        assert!(message.contains(definition.name));
    }
    assert!(
        gold::Gold {
            balance: 0,
            rewards: vec!["future_unknown_reward".into()]
        }
        .message()
        .is_err()
    );
}

#[tokio::test]
async fn schema23_recovery_successful_resurrection_invalidates_only_pending_dead_attempt() {
    let (dir, content, store, mut ch) = fixture(true).await;
    ch.achievements.insert(Achievement::NexusAfterDeath);
    let path = dir.join("game.db");
    let account = ch.account;
    store
        .call(move |db| {
            let tx = db.transaction()?;
            tx.execute(
                "INSERT INTO gold_rewards VALUES(?1,'achievement_nexus_after_death',444,445)",
                [account],
            )?;
            tx.execute(
                "UPDATE accounts SET credits=credits+10 WHERE id=?1",
                [account],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
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
    let mut fatal = ch.clone();
    fatal.alive = false;
    fatal.hp = 0;
    assert!(fatal.achievements.contains(&Achievement::NexusAfterDeath));
    assert!(fatal.achievements.contains(&Achievement::HitByTruck));
    let before = snapshot(&path);
    let history = ledger(&path);
    let progress_rows = || {
        snapshot(&path)
            .into_iter()
            .filter(|row| {
                TABLES
                    .iter()
                    .any(|(name, _)| row.starts_with(&format!("{name}:")))
            })
            .collect::<Vec<_>>()
    };
    let progress = progress_rows();
    let mut foreign = fatal.clone();
    foreign.account += 10000;
    assert!(store.resurrect(foreign, 3, content.clone()).await.is_err());
    assert_eq!(snapshot(&path), before);
    let mut stale = fatal.clone();
    stale.revision += 1;
    assert!(store.resurrect(stale, 3, content.clone()).await.is_err());
    assert_eq!(snapshot(&path), before);
    let restored = store
        .resurrect(fatal.clone(), 3, content.clone())
        .await
        .unwrap();
    let mut expected = fatal.achievements.clone();
    expected.remove(&Achievement::NexusAfterDeath);
    assert_eq!(restored.achievements, expected);
    assert_eq!(
        store.load(account, ch.id).await.unwrap().achievements,
        expected
    );
    assert_eq!(ledger(&path), history);
    assert_eq!(progress_rows(), progress);
    assert_eq!(store.presentation(account).await.unwrap().credits, 145);
    assert!(ledger(&path).contains(&(
        account,
        "achievement_nexus_after_death".into(),
        444,
        Some(445)
    )));
    let committed = snapshot(&path);
    assert!(store.resurrect(fatal, 3, content).await.is_err());
    assert_eq!(snapshot(&path), committed);
    assert_eq!(ledger(&path), history);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema23_recovery_pending_nexus_cleanup_survives_fresh_store_reopen() {
    let (dir, content, store, mut ch) = fixture(true).await;
    ch.achievements.insert(Achievement::NexusAfterDeath);
    let path = dir.join("game.db");
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
    let stale = InventoryWrite {
        id: ch.id,
        account: ch.account,
        revision: ch.revision,
        items: ch.items.clone(),
    };
    ch.alive = false;
    ch.hp = 0;
    assert!(ch.achievements.contains(&Achievement::NexusAfterDeath));
    assert!(ch.achievements.contains(&Achievement::HitByTruck));
    let other_rows = |rows: Vec<String>| {
        rows.into_iter()
            .filter(|row| !row.starts_with("characters:["))
            .collect::<Vec<_>>()
    };
    let history = other_rows(snapshot(&path));
    let restored = store
        .resurrect(ch.clone(), 3, content.clone())
        .await
        .unwrap();
    let mut expected = ch.achievements.clone();
    expected.remove(&Achievement::NexusAfterDeath);
    assert_eq!(restored.achievements, expected);
    let committed = snapshot(&path);
    assert_eq!(other_rows(committed.clone()), history);
    assert_eq!(store.lifecycle_pending_jobs(), 0);
    drop(store);

    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(snapshot(&path), committed);
    let saved = reopened.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved, restored);
    assert_eq!(saved.achievements, expected);
    assert_eq!(
        reopened.presentation(ch.account).await.unwrap().credits,
        135
    );
    assert!(reopened.resurrect(ch, 3, content.clone()).await.is_err());
    assert_eq!(snapshot(&path), committed);
    assert!(reopened.exchange(vec![stale], vec![]).await.is_err());
    assert_eq!(snapshot(&path), committed);
    assert_eq!(other_rows(snapshot(&path)), history);
    assert_eq!(reopened.lifecycle_pending_jobs(), 0);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema23_promotion_ambiguous_nexus_rejects_both_locations_before_write() {
    for version in [21, 23] {
        for archived in [false, true] {
            let (dir, content, store, mut ch) = fixture(false).await;
            let path = dir.join("game.db");
            if archived {
                ch.alive = false;
                ch.hp = 0;
                store
                    .death(ch.clone(), "owned legacy input".into())
                    .await
                    .unwrap();
            }
            assert_eq!(store.lifecycle_pending_jobs(), 0);
            drop(store);
            let db = Connection::open(&path).unwrap();
            if version == 21 {
                crate::store::achievement_tests::schema21_fixture(&db);
                db.pragma_update(None, "user_version", 21).unwrap();
            }
            let (source, sql) = if archived {
                (
                    "deaths.snapshot",
                    "UPDATE deaths SET snapshot=json_set(snapshot,'$.achievements',json('[\"nexus_after_death\"]')) WHERE character=?1",
                )
            } else {
                (
                    "characters.stats",
                    "UPDATE characters SET stats=json_set(stats,'$.achievements',json('[\"nexus_after_death\"]')) WHERE id=?1",
                )
            };
            db.execute(sql, [ch.id]).unwrap();
            drop(db);
            let before = snapshot(&path);
            let error = Store::open(&path, &content)
                .err()
                .expect("ambiguous input must be quarantined")
                .to_string();
            assert!(error.contains("ambiguous unpaid legacy Nexus"), "{error}");
            assert!(
                error.contains(&format!("(\"{source}\", {}, {})", ch.account, ch.id)),
                "{error}"
            );
            assert_eq!(snapshot(&path), before);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[tokio::test]
async fn schema23_promotion_paid_nexus_history_retained_and_receipt_not_validation_bypass() {
    for mutation in [
        None,
        Some(
            "UPDATE characters SET stats=json_set(stats,'$.achievements',json('[\"nexus_after_death\",\"unknown_flag\"]'))",
        ),
        Some(
            "PRAGMA ignore_check_constraints=ON;UPDATE gold_rewards SET awarded_at=0 WHERE reward='achievement_nexus_after_death';PRAGMA ignore_check_constraints=OFF;",
        ),
    ] {
        let (dir, content, store, mut ch) = fixture(true).await;
        let path = dir.join("game.db");
        ch.achievements.insert(Achievement::NexusAfterDeath);
        ch.alive = false;
        ch.hp = 0;
        store
            .death(ch.clone(), "actual owned permanent death".into())
            .await
            .unwrap();
        store
            .gold_notified(
                ch.account,
                vec![Achievement::NexusAfterDeath.reward().into()],
            )
            .await
            .unwrap();
        assert_eq!(store.presentation(ch.account).await.unwrap().credits, 145);
        assert_eq!(store.lifecycle_pending_jobs(), 0);
        drop(store);
        if let Some(sql) = mutation {
            Connection::open(&path).unwrap().execute_batch(sql).unwrap();
        }
        let before = snapshot(&path);
        let result = Store::open(&path, &content);
        if mutation.is_some() {
            assert!(result.is_err());
        } else {
            let reopened = result.unwrap();
            assert_eq!(
                reopened.presentation(ch.account).await.unwrap().credits,
                145
            );
            assert_eq!(
                reopened.gold(ch.account).await.unwrap().rewards,
                [Achievement::HitByTruck.reward()]
            );
            assert_eq!(reopened.lifecycle_pending_jobs(), 0);
            drop(reopened);
        }
        assert_eq!(snapshot(&path), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn schema23_promotion_unpaid_observation_cannot_enter_reusable_progress_or_potion() {
    let (dir, content, store, mut ch) = fixture(false).await;
    let path = dir.join("game.db");
    ch.achievements.insert(Achievement::NexusAfterDeath);
    let before = snapshot(&path);
    assert!(store.progress(ch.clone()).await.is_err());
    assert_eq!(snapshot(&path), before);
    assert!(
        store
            .consume_stat(ch.clone(), None, 7, 1, vec![])
            .await
            .is_err()
    );
    assert_eq!(snapshot(&path), before);
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
    let bounded = snapshot(&path);
    ch.alive = false;
    ch.hp = 0;
    assert!(
        store
            .death(ch.clone(), "atomic NX overflow".into())
            .await
            .is_err()
    );
    assert_eq!(snapshot(&path), bounded);
    store
        .call(move |db| {
            db.execute("UPDATE accounts SET credits=100 WHERE id=?1", [account])?;
            Ok(())
        })
        .await
        .unwrap();
    store.death(ch, "atomic NX retry".into()).await.unwrap();
    assert_eq!(store.presentation(account).await.unwrap().credits, 110);
    assert_eq!(store.lifecycle_pending_jobs(), 0);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.presentation(account).await.unwrap().credits, 110);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn schema23_promotion_nexus_receipt_dates_validate_stored_types_without_pending_flags() {
    for (field, value, stored_type, valid) in [
        ("awarded_at", "'not-a-date'", "text", false),
        ("notified_at", "'not-a-date'", "text", false),
        ("awarded_at", "1.25", "real", false),
        ("notified_at", "1.25", "real", false),
        ("awarded_at", "8640000000001", "integer", false),
        ("notified_at", "8640000000001", "integer", false),
        ("awarded_at", "'123'", "integer", true),
        ("notified_at", "'456'", "integer", true),
        ("notified_at", "NULL", "null", true),
        ("awarded_at", "8640000000000", "integer", true),
        ("notified_at", "8640000000000", "integer", true),
    ] {
        let (dir, content, store, mut ch) = fixture(true).await;
        let path = dir.join("game.db");
        ch.alive = false;
        ch.hp = 0;
        store
            .death(ch.clone(), "paid receipt metadata fixture".into())
            .await
            .unwrap();
        store
            .achievement_dead_escape(ch.account, ch.id)
            .await
            .unwrap();
        store
            .gold_notified(
                ch.account,
                vec![Achievement::NexusAfterDeath.reward().into()],
            )
            .await
            .unwrap();
        assert_eq!(store.lifecycle_pending_jobs(), 0);
        drop(store);
        let db = Connection::open(&path).unwrap();
        db.execute(&format!("UPDATE gold_rewards SET {field}={value} WHERE account=?1 AND reward='achievement_nexus_after_death'"), [ch.account]).unwrap();
        assert_eq!(db.query_row(&format!("SELECT typeof({field}) FROM gold_rewards WHERE account=?1 AND reward='achievement_nexus_after_death'"), [ch.account], |r| r.get::<_,String>(0)).unwrap(), stored_type);
        assert_eq!(db.query_row("SELECT count(*) FROM characters c,json_each(c.stats,'$.achievements') j WHERE j.value='nexus_after_death'", [], |r| r.get::<_,i32>(0)).unwrap(), 0);
        drop(db);
        let before = snapshot(&path);
        let result = Store::open(&path, &content);
        if valid {
            let reopened = result.unwrap();
            assert_eq!(
                reopened.presentation(ch.account).await.unwrap().credits,
                145
            );
            assert_eq!(reopened.lifecycle_pending_jobs(), 0);
            drop(reopened);
        } else {
            let error = result
                .err()
                .expect("invalid receipt must fail even without pending flag")
                .to_string();
            assert!(error.contains("invalid Nexus receipt metadata"), "{error}");
            assert!(
                error.contains(field)
                    && error.contains(stored_type)
                    && error.contains(&ch.account.to_string()),
                "{error}"
            );
        }
        assert_eq!(snapshot(&path), before);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn schema23_main_integration_preserves_realm_close_state_and_public_backup_contract() {
    let dir =
        std::env::temp_dir().join(format!("achievement-main-tokens-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&path, &content).unwrap();
    let account = store
        .register(
            "synthetic-main-token".into(),
            "synthetic-main@example.invalid".into(),
            "synthetic-token-password".into(),
        )
        .await
        .unwrap();
    let peer = store.list("synthetic-main-peer".into()).await.unwrap().id;
    let discord = "1558662335076376578".to_string();
    let code = store.issue_discord_code(account).await.unwrap();
    store
        .redeem_discord_code(code, discord.clone())
        .await
        .unwrap();
    for request in ["1558662335076376580", "1558662335076376581"] {
        store
            .grant_realm_close_token(
                discord.clone(),
                request.into(),
                "1558662335076376579".into(),
            )
            .await
            .unwrap();
    }
    store
        .spend_realm_close_token(discord, "1558662335076376590".into())
        .await
        .unwrap();
    assert_eq!(store.lifecycle_pending_jobs(), 0);
    drop(store);
    let db = Connection::open(&path).unwrap();
    let tokens = |db: &Connection| {
        let mut query = db.prepare("SELECT request_id,account,discord_id,actor_id,used_request FROM discord_realm_close_tokens ORDER BY request_id").unwrap();
        query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let before = tokens(&db);
    crate::store::achievement_tests::schema21_fixture(&db);
    db.pragma_update(None, "user_version", 21).unwrap();
    drop(db);
    let store = Store::open(&path, &content).unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(tokens(&db), before);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        23
    );
    assert_eq!(store.presentation(account).await.unwrap().credits, 200);
    store
        .call(move |db| {
            let tx = db.transaction()?;
            tx.execute(
                "INSERT INTO gold_rewards VALUES(?1,'achievement_trader',111,222)",
                [account],
            )?;
            tx.execute(
                "UPDATE accounts SET credits=credits+35 WHERE id=?1",
                [account],
            )?;
            tx.execute(
                "INSERT INTO achievement_partners VALUES(?1,?2)",
                params![account, peer],
            )?;
            tx.execute(
                "INSERT INTO achievement_dungeons VALUES(?1,'pirate-cave')",
                [account],
            )?;
            tx.execute(
                "INSERT INTO achievement_realm_credits VALUES(?1,?2,1)",
                params![account, "a".repeat(32)],
            )?;
            tx.execute(
                "INSERT INTO gold_rewards VALUES(?1,'achievement_ah_7uck',112,223)",
                [account],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(store.lifecycle_pending_jobs(), 0);
    drop(store);
    drop(db);
    let before_reopen = snapshot(&path);
    let reopened = Store::open(&path, &content).unwrap();
    assert!(
        snapshot(&path) == before_reopen,
        "schema22 reopen changed logical state"
    );
    assert_eq!(reopened.presentation(account).await.unwrap().credits, 235);
    let result = std::process::Command::new("python3")
        .args([
            "scripts/backups/test_achievements.py",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "backup contract failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        snapshot(&path) == before_reopen,
        "public export changed source logical state"
    );
    let restored_path = dir.join("backup-test-restored/game.db");
    let restored = Store::open(&restored_path, &content).unwrap();
    assert!(
        tokens(&Connection::open(&restored_path).unwrap()).is_empty(),
        "public archive intentionally omits private tokens; startup creates an empty private table"
    );
    assert_eq!(restored.presentation(account).await.unwrap().credits, 235);
    assert_eq!(ledger(&restored_path), ledger(&path));
}

#[tokio::test]
async fn schema23_known22_paid_history_tokens_migrate_transactionally_without_awards() {
    let (dir, content, store, ch) = fixture(true).await;
    assert_eq!(store.lifecycle_pending_jobs(), 0);
    drop(store);
    let path = dir.join("game.db");
    let db = Connection::open(&path).unwrap();
    db.execute("INSERT INTO discord_realm_close_tokens VALUES('1558662335076376580',?1,'1558662335076376578','1558662335076376579',NULL)",[ch.account]).unwrap();
    db.execute_batch("ALTER TABLE gold_rewards RENAME TO owned23_gold;")
        .unwrap();
    db.execute_batch(GOLD).unwrap();
    db.execute_batch("INSERT INTO gold_rewards SELECT * FROM owned23_gold; DROP TABLE owned23_gold; PRAGMA user_version=22;").unwrap();
    gold::check_schema(&db, 22).unwrap();
    let history = ledger(&path);
    let before = snapshot(&path);
    db.execute_batch("CREATE VIEW old_gold_rewards AS SELECT 1;")
        .unwrap();
    let rollback = snapshot(&path);
    assert!(
        Store::open(&path, &content).is_err(),
        "ledger migration collision must roll back"
    );
    assert!(snapshot(&path) == rollback);
    db.execute_batch("DROP VIEW old_gold_rewards;").unwrap();
    let altered = GOLD.replace("awarded_at>0", "awarded_at>=0");
    db.execute_batch("ALTER TABLE gold_rewards RENAME TO owned22_rows;")
        .unwrap();
    db.execute_batch(&altered).unwrap();
    db.execute_batch(
        "INSERT INTO gold_rewards SELECT * FROM owned22_rows; DROP TABLE owned22_rows;",
    )
    .unwrap();
    let unknown = snapshot(&path);
    assert!(
        Store::open(&path, &content).is_err(),
        "unknown22 fingerprint must not be accepted"
    );
    assert!(snapshot(&path) == unknown);
    db.execute_batch("ALTER TABLE gold_rewards RENAME TO owned_unknown_rows;")
        .unwrap();
    db.execute_batch(GOLD).unwrap();
    db.execute_batch(
        "INSERT INTO gold_rewards SELECT * FROM owned_unknown_rows; DROP TABLE owned_unknown_rows;",
    )
    .unwrap();
    assert!(snapshot(&path) == before);
    drop(db);
    let current = Store::open(&path, &content).unwrap();
    assert_eq!(ledger(&path), history);
    assert_eq!(current.presentation(ch.account).await.unwrap().credits, 135);
    let mut expected = before;
    expected.retain(|line| {
        !line.starts_with("version:") && !line.starts_with("gold_rewards:CREATE TABLE")
    });
    let mut actual = snapshot(&path);
    actual.retain(|line| {
        !line.starts_with("version:") && !line.starts_with("gold_rewards:CREATE TABLE")
    });
    assert!(
        actual == expected,
        "only schema marker and exact gold CHECK may change"
    );
    assert_eq!(current.lifecycle_pending_jobs(), 0);
    drop(current);
    let settled = snapshot(&path);
    let reopened = Store::open(&path, &content).unwrap();
    assert!(snapshot(&path) == settled);
    assert_eq!(
        reopened.presentation(ch.account).await.unwrap().credits,
        135
    );
}
