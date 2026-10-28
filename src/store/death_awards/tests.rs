use super::*;

async fn setup() -> (std::path::PathBuf, Content, Store, Character) {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!(
        "death-awards-{}-{}.db",
        std::process::id(),
        rand::random::<u64>()
    ));
    let store = Store::open(&path, &content).unwrap();
    let owner = store.list("death-awards".into()).await.unwrap();
    let ch = store
        .create(owner.id, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    (path, content, store, ch)
}

async fn equip(store: &Store, ch: &mut Character, items: Vec<i32>) {
    ch.revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    ch.items = items;
}
fn fatal(ch: &mut Character, fame: i32) {
    ch.fame = fame;
    ch.hp = 0;
    ch.alive = false;
}

#[tokio::test]
async fn robes_keep_caps_separate_and_only_equipped_fame_counts() {
    let (_, content, _, mut ch) = setup().await;
    ch.base = content.classes[&WIZARD].caps;
    let base = ch.base;
    for (kind, fame) in [
        (0xa5c, 0),
        (0xa7e, 0),
        (0xa5d, 0),
        (0xa7f, 0),
        (0xa80, 0),
        (0xa5e, 0),
        (0xa5f, 0),
        (0xadc, 1),
        (0xa60, 1),
        (0xa94, 2),
        (0xa95, 3),
        (0xa96, 4),
        (0xb05, 4),
        (0xb06, 5),
        (0xb07, 6),
        (0xc14, 2),
        (0xc1f, 3),
        (0xc32, 5),
    ] {
        ch.items.fill(-1);
        ch.items[2] = kind;
        let effective = ch.effective(&content).unwrap();
        assert_eq!(equipment_percent(&ch, &content).unwrap(), fame);
        assert_eq!(ch.base, base);
        assert!(effective[1] > base[1] || effective[3] > base[3]);
        ch.items.swap(2, 4);
        assert_eq!(ch.effective(&content).unwrap(), base);
        assert_eq!(equipment_percent(&ch, &content).unwrap(), 0);
    }
}

#[tokio::test]
async fn class_stars_retain_live_deleted_and_committed_death_bests_not_wallets() {
    let (path, content, store, mut ch) = setup().await;
    ch.level = 20;
    ch.exp = 19_999;
    ch.fame = 19;
    store.progress(ch.clone()).await.unwrap();
    assert_eq!(store.presentation(ch.account).await.unwrap().stars(), 0);
    ch.gain_experience(&content, 1, &mut rand::thread_rng())
        .unwrap();
    store.progress(ch.clone()).await.unwrap();
    let p = store.presentation(ch.account).await.unwrap();
    assert_eq!(
        (p.stars(), p.next_quest(WIZARD, ch.fame), p.fame),
        (1, 150, 0)
    );
    store.delete_character(ch.account, ch.id).await.unwrap();
    let mut lower = store
        .create(ch.account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    lower.fame = 19;
    store.progress(lower.clone()).await.unwrap();
    assert_eq!(
        store.presentation(ch.account).await.unwrap().best_fame[&WIZARD],
        20
    );
    store.delete_character(ch.account, lower.id).await.unwrap();

    let class = *content
        .classes
        .keys()
        .find(|class| **class != WIZARD)
        .unwrap();
    let mut dead = store
        .create_prepared(ch.account, content.get(class).unwrap(), None)
        .await
        .unwrap();
    fatal(&mut dead, 18);
    store
        .death(dead.clone(), "class quest fixture".into())
        .await
        .unwrap();
    let db = Connection::open(&path).unwrap();
    let receipt: String = db
        .query_row(
            "SELECT receipt FROM deaths WHERE character=?1",
            [dead.id],
            |r| r.get(0),
        )
        .unwrap();
    let total = decode_receipt(&receipt).unwrap().total;
    assert!(total >= 20);
    let p = store.presentation(ch.account).await.unwrap();
    assert_eq!(p.best_fame, BTreeMap::from([(WIZARD, 20), (class, total)]));
    assert_eq!(p.fame, total);
    let expected = 1 + AccountPresentation::THRESHOLDS
        .iter()
        .filter(|threshold| total >= **threshold)
        .count() as i32;
    assert_eq!(p.stars(), expected);
    assert!(store.death(dead.clone(), "duplicate".into()).await.is_err());
    db.execute(
        "UPDATE account_fame SET balance=0 WHERE account=?1",
        [ch.account],
    )
    .unwrap();
    db.execute(
        "UPDATE characters SET stats=json_set(stats,'$.fame',9999) WHERE id=?1",
        [dead.id],
    )
    .unwrap();
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    let p = reopened.presentation(ch.account).await.unwrap();
    assert_eq!(
        (p.stars(), p.fame, p.best_fame[&class]),
        (expected, 0, total)
    );
    let xml = reopened
        .list("death-awards".into())
        .await
        .unwrap()
        .xml("", 0.);
    assert!(xml.contains(&format!("<ClassStats objectType=\"{class}\"><BestLevel>1</BestLevel><BestFame>{total}</BestFame></ClassStats>")));
    db.execute(
        "UPDATE deaths SET receipt=json_set(receipt,'$.total',-1) WHERE character=?1",
        [dead.id],
    )
    .unwrap();
    assert!(reopened.presentation(ch.account).await.is_err());
}

#[tokio::test]
async fn equipment_death_atomic_wallet_guild_rollback_and_concurrent_retry() {
    let (path, content, store, mut ch) = setup().await;
    store.rename(ch.account, "Awardchief".into()).await.unwrap();
    fatal(&mut ch, 1000);
    store
        .death(ch.clone(), "guild funding".into())
        .await
        .unwrap();
    store
        .change_guild(ch.account, GuildAction::Create("Award Guild".into()))
        .await
        .unwrap();
    let mut ch = store
        .create(ch.account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    equip(
        &store,
        &mut ch,
        vec![0xaa2, -1, -1, 0xabf, 0xb07, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    let saved = ch.clone();
    fatal(&mut ch, 19);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_award BEFORE UPDATE OF fame ON guild_members BEGIN SELECT RAISE(ABORT,'forced guild storage failure'); END;").unwrap();
    assert!(store.death(ch.clone(), "rollback".into()).await.is_err());
    assert_eq!(
        store.load(ch.account, ch.id).await.unwrap().items,
        saved.items
    );
    assert_eq!(store.presentation(ch.account).await.unwrap().fame, 232);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(store.guild(ch.account).await.unwrap().earned, 0);
    db.execute_batch("DROP TRIGGER reject_award").unwrap();
    let other = Store::open(&path, &content).unwrap();
    let (a, b) = tokio::join!(
        store.death(ch.clone(), "one".into()),
        other.death(ch.clone(), "two".into())
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(store.presentation(ch.account).await.unwrap().fame, 252);
    assert_eq!(
        store.list("death-awards".into()).await.unwrap().earned_fame,
        1252
    );
    let guild = store.guild(ch.account).await.unwrap();
    assert_eq!((guild.fame, guild.earned), (20, 20));
    assert_eq!(
        db.query_row(
            "SELECT fame FROM guild_members WHERE account=?1",
            [ch.account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        20
    );
    assert_eq!(
        db.query_row(
            "SELECT equipment_fame FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert!(store.load(ch.account, ch.id).await.is_err());
}

fn history() -> DeathHistory {
    DeathHistory {
        retained_deaths: 1,
        other_same_class_max_level: 20,
        legacy_claimed: false,
        prior_paid_max: i32::MAX,
    }
}
fn empty_balances() -> PayoutBalances {
    PayoutBalances {
        wallet: (0, 0),
        guild: None,
    }
}

#[tokio::test]
async fn all_death_fame_phase2_schema17_roundtrip_corruption() {
    let (path, content, store, mut ch) = setup().await;
    assert_eq!(ch.lifetime.completeness, LifetimeCompleteness::Complete);
    let mut missing = serde_json::to_value(&ch).unwrap();
    missing.as_object_mut().unwrap().remove("lifetime");
    assert!(serde_json::from_value::<Character>(missing).is_err());
    let mut future = serde_json::to_value(&ch).unwrap();
    future["schema"] = serde_json::json!(2);
    assert!(serde_json::from_value::<Character>(future).is_err());
    for field in ["version", "projectile_hits"] {
        let mut bad = serde_json::to_value(&ch).unwrap();
        bad["lifetime"][field] = serde_json::json!(99);
        assert!(serde_json::from_value::<Character>(bad.clone()).is_err());
        assert!(decode_legacy_character(&bad.to_string(), ch.base).is_err());
    }
    fatal(&mut ch, 100);
    store
        .death(ch.clone(), "phase2 receipt".into())
        .await
        .unwrap();
    let before = store.fame(ch.account, ch.id).await.unwrap();
    assert!(
        before.contains("<Bonus id=\"Ancestor\" desc=\"First death on this account\">30</Bonus>")
    );
    assert!(before.contains("<Bonus id=\"First Born\" desc=\"Beat the Fame total of every previous character\">13</Bonus>"));
    drop(store);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        21
    );
    let original: String = db
        .query_row(
            "SELECT receipt FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get(0),
        )
        .unwrap();
    let receipt: DeathFameReceipt = serde_json::from_str(&original).unwrap();
    assert_eq!(receipt.policy, ReceiptPolicy::Historical);
    receipt.validate().unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.fame(ch.account, ch.id).await.unwrap(), before);
    drop(reopened);
    for mutation in 0..8 {
        let mut raw: serde_json::Value = serde_json::from_str(&original).unwrap();
        match mutation {
            0 => raw["version"] = serde_json::json!(2),
            1 => raw["policy"] = serde_json::json!("unknown"),
            2 => raw["total"] = serde_json::json!(101),
            3 => raw["rows"] = serde_json::json!([{"kind":"unknown","amount":0}]),
            4 => raw["rows"] = serde_json::json!([{"kind":"well_equipped","amount":-1}]),
            5 => {
                raw["rows"] = serde_json::json!([{"kind":"well_equipped","amount":0},{"kind":"well_equipped","amount":0}])
            }
            6 => {
                raw["rows"] = serde_json::json!([{"kind":"first_born","amount":0},{"kind":"ancestor","amount":0}])
            }
            7 => raw["rows"] = serde_json::json!([{"kind":"ancestor","amount":0}]),
            _ => unreachable!(),
        }
        db.execute("UPDATE deaths SET receipt=?1", [raw.to_string()])
            .unwrap();
        assert!(
            Store::open(&path, &content).is_err(),
            "receipt mutation {mutation}"
        );
        db.execute("UPDATE deaths SET receipt=?1", [&original])
            .unwrap();
    }
    let mut raw: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&ch).unwrap()).unwrap();
    raw.as_object_mut().unwrap().remove("lifetime");
    db.execute("UPDATE deaths SET snapshot=?1", [raw.to_string()])
        .unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "current receipt cannot authorize a missing lifetime"
    );
    db.execute(
        "UPDATE deaths SET snapshot=?1",
        [serde_json::to_string(&ch).unwrap()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO legacy_claims(account,class,character) VALUES(?1,?2,?3)",
        params![ch.account, ch.class, ch.id],
    )
    .unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "equipment receipt cannot manufacture a Legacy claim"
    );
    db.execute("DELETE FROM legacy_claims", []).unwrap();
    db.pragma_update(None, "foreign_keys", false).unwrap();
    db.execute(
        "INSERT INTO level_credits(recipient,reached_level,giver) VALUES(?1,2,999999)",
        [ch.id],
    )
    .unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "orphan giver must reject startup"
    );
    db.execute("DELETE FROM level_credits", []).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    assert!(Store::open(&path, &content).is_ok());
    // An explicit prepared receipt isolates claim storage from the runtime history comparator.
    ch.level = 2;
    ch.exp = 50;
    let paid = evaluate(
        &ch,
        &ch.lifetime,
        DeathHistory {
            other_same_class_max_level: 0,
            ..history()
        },
        &content,
        empty_balances(),
    )
    .unwrap();
    assert_eq!(
        paid.rows,
        [BonusRow {
            kind: BonusKind::LegacyBuilder,
            amount: 20
        }]
    );
    let snapshot = serde_json::to_string(&ch).unwrap();
    db.execute(
        "UPDATE deaths SET snapshot=?1,receipt=?2",
        params![snapshot, serde_json::to_string(&paid).unwrap()],
    )
    .unwrap();
    db.execute("UPDATE characters SET stats=?1", [&snapshot])
        .unwrap();
    db.execute("UPDATE account_fame SET balance=120,earned=120", [])
        .unwrap();
    db.execute(
        "INSERT INTO legacy_claims(account,class,character) VALUES(?1,?2,?3)",
        params![ch.account, ch.class, ch.id],
    )
    .unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    assert!(
        reopened
            .fame(ch.account, ch.id)
            .await
            .unwrap()
            .contains("<TotalFame>120</TotalFame>")
    );
    drop(reopened);
    db.execute("UPDATE legacy_claims SET class=class+1", [])
        .unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "foreign class claim rejected"
    );
}

fn phase5_receipt(db: &Connection, id: i32) -> DeathFameReceipt {
    decode_receipt(
        &db.query_row("SELECT receipt FROM deaths WHERE character=?", [id], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
    )
    .unwrap()
}
fn phase5_state(db: &Connection) -> Vec<String> {
    ["SELECT json_array(id,alive,stats,items,revision) FROM characters ORDER BY id","SELECT json_array(character,snapshot,receipt,died_at,equipment_fame) FROM deaths ORDER BY character","SELECT json_array(account,class,character) FROM legacy_claims ORDER BY account,class","SELECT json_array(account,balance,earned) FROM account_fame ORDER BY account","SELECT json_array(id,fame,earned) FROM guilds ORDER BY id","SELECT json_array(account,fame) FROM guild_members ORDER BY account"].into_iter().flat_map(|sql|db.prepare(sql).unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()).collect()
}

#[tokio::test]
async fn all_death_fame_phase5_atomic_claims_receipt() {
    let (path, content, store, mut funding) = setup().await;
    store
        .rename(funding.account, "Fullchief".into())
        .await
        .unwrap();
    fatal(&mut funding, 1000);
    store
        .death(funding.clone(), "funding".into())
        .await
        .unwrap();
    store
        .change_guild(funding.account, GuildAction::Create("Full Guild".into()))
        .await
        .unwrap();
    let mut ch = store
        .create(funding.account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    equip(&store, &mut ch, vec![-1; 12]).await;
    ch.level = 2;
    ch.exp = 50;
    fatal(&mut ch, 100);
    let db = Connection::open(&path).unwrap();
    let initial = phase5_state(&db);
    let mut overflow = ch.clone();
    overflow.fame = i32::MAX;
    assert!(store.death(overflow, "overflow".into()).await.is_err());
    assert_eq!(phase5_state(&db), initial);
    let mut invalid = ch.clone();
    invalid.lifetime.projectile_hits = 1;
    assert!(
        store
            .death(invalid, "invalid collision history".into())
            .await
            .is_err()
    );
    assert_eq!(phase5_state(&db), initial);
    for (table, event) in [
        ("deaths", "INSERT"),
        ("legacy_claims", "INSERT"),
        ("account_fame", "UPDATE"),
        ("guilds", "UPDATE"),
        ("guild_members", "UPDATE"),
    ] {
        db.execute_batch(&format!("CREATE TRIGGER fail_full BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'injected {table} failure'); END;")).unwrap();
        assert!(
            store.death(ch.clone(), "rollback".into()).await.is_err(),
            "{table}"
        );
        assert_eq!(phase5_state(&db), initial, "{table}");
        db.execute_batch("DROP TRIGGER fail_full").unwrap();
    }
    let other = Store::open(&path, &content).unwrap();
    let (a, b) = tokio::join!(
        store.death(ch.clone(), "one".into()),
        other.death(ch.clone(), "two".into())
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let paid = phase5_receipt(&db, ch.id);
    assert_eq!(paid.policy, ReceiptPolicy::Historical);
    assert_eq!(paid.total, 120);
    assert_eq!(
        paid.rows,
        vec![BonusRow {
            kind: BonusKind::LegacyBuilder,
            amount: 20
        }]
    );
    assert_eq!(store.guild(ch.account).await.unwrap().fame, 120);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM legacy_claims WHERE account=?",
            [ch.account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    let old = phase5_state(&db);
    assert!(store.death(ch.clone(), "duplicate".into()).await.is_err());
    assert_eq!(phase5_state(&db), old);
    // Prepared final level-two snapshots compete against retained level-one records.
    let account = store
        .list("full-concurrent-history".into())
        .await
        .unwrap()
        .id;
    store
        .register(
            "full-concurrent-history".into(),
            "concurrent@example.invalid".into(),
            "fixture-password".into(),
        )
        .await
        .unwrap();
    db.execute("UPDATE accounts SET credits=1100 WHERE id=?", [account])
        .unwrap();
    store.purchase_character_slot(account, 1).await.unwrap();
    let mut left = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut right = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    for c in [&mut left, &mut right] {
        equip(&store, c, vec![-1; 12]).await;
        c.level = 2;
        c.exp = 50;
        fatal(c, 100);
    }
    let (a, b) = tokio::join!(
        store.death(left.clone(), "left".into()),
        other.death(right.clone(), "right".into())
    );
    if let Err(error) = &a {
        assert!(
            format!("{error:#}").contains("database is locked"),
            "unexpected concurrent failure: {error:#}"
        );
        store
            .death(left.clone(), "retry-left".into())
            .await
            .unwrap();
    }
    if let Err(error) = &b {
        assert!(
            format!("{error:#}").contains("database is locked"),
            "unexpected concurrent failure: {error:#}"
        );
        store
            .death(right.clone(), "retry-right".into())
            .await
            .unwrap();
    }
    let receipts = [phase5_receipt(&db, left.id), phase5_receipt(&db, right.id)];
    for kind in [BonusKind::Ancestor, BonusKind::LegacyBuilder] {
        assert_eq!(
            receipts
                .iter()
                .flat_map(|r| r.rows.iter())
                .filter(|r| r.kind == kind)
                .count(),
            1
        );
    }
    assert_eq!(
        store.presentation(account).await.unwrap().fame,
        receipts.iter().map(|r| r.total).sum::<i32>()
    );
}
