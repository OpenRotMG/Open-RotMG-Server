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
async fn equipment_death_zero_one_multiple_backpack_and_checked_boundaries() {
    let (_, _, store, mut ch) = setup().await;
    for (base, equipment, expected) in [
        (19, vec![-1; 12], 0),
        (
            19,
            vec![0xaa2, -1, -1, -1, 0xc01, -1, -1, -1, -1, -1, -1, -1],
            0,
        ),
        (
            19,
            vec![0xaa2, -1, -1, 0xabf, 0xc01, -1, -1, -1, -1, -1, -1, -1],
            1,
        ),
        (
            100,
            vec![0xaa2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
            3,
        ),
    ] {
        ch.items = equipment;
        ch.fame = base;
        assert_eq!(super::equipment(&ch, &store.content).unwrap(), expected);
    }
    ch.fame = 0;
    assert_eq!(super::equipment(&ch, &store.content).unwrap(), 0);
    ch.fame = i32::MAX;
    assert!(super::equipment(&ch, &store.content).is_err());
    ch.items.fill(-1);
    assert_eq!(super::equipment(&ch, &store.content).unwrap(), 0);
    ch.items[0] = 65535;
    assert!(super::equipment(&ch, &store.content).is_err());
    ch.items[0] = -1;
    ch.items[3] = 0xb3e;
    assert!(super::equipment(&ch, &store.content).is_err());
    ch.items[3] = -1;
    ch.items[0] = 0xaa2;
    let mut huge = (*store.content).clone();
    huge.objects.get_mut(&0xaa2).unwrap().fame_bonus = u32::MAX;
    assert!(super::equipment(&ch, &huge).is_err());
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
        vec![0xaa2, -1, -1, 0xabf, -1, -1, -1, -1, -1, -1, -1, -1],
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
    assert_eq!(store.presentation(ch.account).await.unwrap().fame, 0);
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
    assert_eq!(store.presentation(ch.account).await.unwrap().fame, 20);
    assert_eq!(
        store.list("death-awards".into()).await.unwrap().earned_fame,
        1020
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

#[tokio::test]
async fn equipment_death_rejects_stale_gear_foreign_owner_and_lost_progress() {
    let (_, _, store, mut ch) = setup().await;
    let mut stale = ch.clone();
    equip(
        &store,
        &mut ch,
        vec![0xaa2, -1, -1, 0xabf, -1, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    fatal(&mut stale, 19);
    assert!(store.death(stale, "stale revision".into()).await.is_err());
    let mut wrong = ch.clone();
    wrong.items[0] = -1;
    fatal(&mut wrong, 19);
    assert!(
        store
            .death(wrong, "stale transient gear".into())
            .await
            .is_err()
    );
    let mut wrong = ch.clone();
    wrong.account += 1;
    fatal(&mut wrong, 19);
    assert!(store.death(wrong, "foreign".into()).await.is_err());
    let mut old = ch.clone();
    ch.exp = 20000;
    ch.fame = 20;
    store.progress(ch.clone()).await.unwrap();
    fatal(&mut old, 19);
    assert!(store.death(old, "lost earned XP".into()).await.is_err());
    fatal(&mut ch, 20);
    store
        .death(ch.clone(), "final canonical equipment".into())
        .await
        .unwrap();
    assert_eq!(store.presentation(ch.account).await.unwrap().fame, 21);
}

#[tokio::test]
async fn equipment_death_legacy_migration_preserves_snapshots_wallet_and_paid_vaults() {
    let (path, content, store, mut ch) = setup().await;
    assert_eq!(
        store
            .register(
                "death-awards".into(),
                "awards@example.test".into(),
                "test-password".into()
            )
            .await
            .unwrap(),
        ch.account
    );
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE accounts SET credits=600 WHERE id=?1", [ch.account])
        .unwrap();
    ch.revision = store
        .purchase_vault(
            InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            },
            1,
        )
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut chest = store.vault(ch.account, 1).await.unwrap();
    chest.items[0] = 0xc01;
    ch.revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![chest],
        )
        .await
        .unwrap()[0];
    fatal(&mut ch, 19);
    store
        .death(ch.clone(), "old base-only death".into())
        .await
        .unwrap();
    let snapshot: String = db
        .query_row(
            "SELECT snapshot FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get(0),
        )
        .unwrap();
    let vaults = store.vaults(ch.account).await.unwrap();
    drop(store);
    db.execute_batch("ALTER TABLE deaths DROP COLUMN equipment_fame; PRAGMA user_version=13;")
        .unwrap();
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT snapshot FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        snapshot
    );
    assert_eq!(
        db.query_row(
            "SELECT equipment_fame FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(store.presentation(ch.account).await.unwrap().fame, 19);
    assert_eq!(
        serde_json::to_value(store.vaults(ch.account).await.unwrap()).unwrap(),
        serde_json::to_value(vaults).unwrap()
    );
    drop(store);
    db.execute_batch("ALTER TABLE deaths DROP COLUMN equipment_fame")
        .unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "missing current award cannot masquerade as legacy"
    );
    db.execute_batch("ALTER TABLE deaths ADD COLUMN equipment_fame INTEGER NOT NULL DEFAULT 0")
        .unwrap();
    assert!(
        Store::open(&path, &content).is_err(),
        "current award cannot default silently"
    );
}
