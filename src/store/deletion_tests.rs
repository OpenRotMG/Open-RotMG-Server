use super::*;
use rand::{SeedableRng, rngs::StdRng};

#[tokio::test]
async fn deletion_retires_inventory_without_rewards_and_preserves_account_history_after_reopen() {
    let directory =
        std::env::temp_dir().join(format!("openrotmg-delete-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("game.db");
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(&path, &content).unwrap();
    let account = s.list("delete-history".into()).await.unwrap().id;
    let mut dead = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    dead.alive = false;
    dead.hp = 0;
    dead.fame = 150;
    s.death(dead.clone(), "Pirate".into()).await.unwrap();
    let death = s.fame(account, dead.id).await.unwrap();
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(
        &content,
        level_start(20).unwrap(),
        &mut StdRng::seed_from_u64(12351),
    )
    .unwrap();
    ch.fame = 999;
    s.progress(ch.clone()).await.unwrap();
    let mut vault = s.vault(account, 0).await.unwrap();
    vault.items[0] = ch.items[4];
    ch.items[4] = -1;
    ch.revision = s
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![vault],
        )
        .await
        .unwrap()[0];
    let vault = serde_json::to_value(s.vault(account, 0).await.unwrap()).unwrap();
    let before = s.list("delete-history".into()).await.unwrap();
    let presentation = s.presentation(account).await.unwrap();
    assert_eq!(presentation.best_fame[&WIZARD], 999);
    assert_eq!(presentation.stars(), 4);
    s.delete_character(account, ch.id).await.unwrap();
    assert!(s.delete_character(account, ch.id).await.is_err());
    assert!(s.delete_character(account, dead.id).await.is_err());
    assert!(s.load(account, ch.id).await.is_err());
    assert!(s.progress(ch.clone()).await.is_err());
    assert!(
        s.exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            Vec::new()
        )
        .await
        .is_err()
    );
    let mut stale_death = ch.clone();
    stale_death.hp = 0;
    stale_death.alive = false;
    assert!(s.death(stale_death, "Late death".into()).await.is_err());
    drop(s);

    let s = Store::open(&path, &content).unwrap();
    let after = s.list("delete-history".into()).await.unwrap();
    assert!(after.characters.is_empty());
    assert_eq!(after.best_levels, before.best_levels);
    assert_eq!(after.next_id, before.next_id);
    assert_eq!((after.fame, after.earned_fame), (150, 150));
    assert_eq!(s.presentation(account).await.unwrap(), presentation);
    assert_eq!(s.fame(account, dead.id).await.unwrap(), death);
    assert!(s.fame(account, ch.id).await.is_err());
    assert_eq!(
        serde_json::to_value(s.vault(account, 0).await.unwrap()).unwrap(),
        vault
    );
    let db = Connection::open(&path).unwrap();
    let (raw, items, revision): (String, String, i32) = db
        .query_row(
            "SELECT stats,items,revision FROM characters WHERE id=?1 AND alive=0",
            [ch.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let retired: Character = serde_json::from_str(&raw).unwrap();
    assert!(!retired.alive);
    assert_eq!(
        (retired.hp, retired.revision, revision),
        (0, ch.revision + 1, ch.revision + 1)
    );
    assert_eq!(retired.items, vec![-1; 12]);
    assert_eq!(
        serde_json::from_str::<Vec<i32>>(&items).unwrap(),
        retired.items
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    let replacement = s
        .create(account, content.get(PRIEST).unwrap())
        .await
        .unwrap();
    assert!(replacement.id > ch.id);
    assert!(s.load(account, ch.id).await.is_err());
    drop(db);
    drop(s);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn deletion_is_owner_scoped_atomic_and_once_only() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    let owner = s.list("delete-owner".into()).await.unwrap().id;
    let other = s.list("delete-other".into()).await.unwrap().id;
    let ch = s.create(owner, content.get(WIZARD).unwrap()).await.unwrap();
    assert!(s.delete_character(other, ch.id).await.is_err());
    assert!(s.delete_character(owner, i32::MAX).await.is_err());
    s.call(|db| { db.execute_batch("CREATE TRIGGER fail_delete BEFORE UPDATE OF alive ON characters BEGIN SELECT RAISE(ABORT,'injected storage failure'); END;")?; Ok(()) }).await.unwrap();
    assert!(
        s.delete_character(owner, ch.id)
            .await
            .unwrap_err()
            .to_string()
            .contains("injected storage failure")
    );
    assert_eq!(s.load(owner, ch.id).await.unwrap().items, ch.items);
    s.call(|db| {
        db.execute_batch("DROP TRIGGER fail_delete")?;
        Ok(())
    })
    .await
    .unwrap();
    let (a, b) = tokio::join!(
        s.delete_character(owner, ch.id),
        s.delete_character(owner, ch.id)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(s.load(owner, ch.id).await.is_err());
}
