use super::*;
use rand::SeedableRng;

#[tokio::test]
async fn death_record_survives_replacement_restart_and_rejected_replays() {
    let content = Content::load(Path::new("data")).unwrap();
    let dir = std::env::temp_dir().join(format!("death-record-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let guid = "death-record-fixture".to_owned();
    let account = store.list(guid.clone()).await.unwrap().id;
    let other = store.list("death-record-other".into()).await.unwrap().id;
    let mut ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.items[4] = 0xb3e;
    ch.revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    ch.gain_experience(&content, 20000, &mut rand::rngs::StdRng::seed_from_u64(783))
        .unwrap();
    ch.lifetime.shots = 9;
    ch.lifetime.projectile_hits = 3;
    ch.lifetime.tiles = 11;
    ch.lifetime.kills = 2;
    store.progress(ch.clone()).await.unwrap();
    ch.alive = false;
    ch.hp = 0;
    store.death(ch.clone(), "A & B <C>".into()).await.unwrap();
    let fame = store.fame(account, ch.id).await.unwrap();
    let xml = roxmltree::Document::parse(&fame).unwrap();
    let text = |tag: &str| {
        xml.descendants()
            .find(|n| n.has_tag_name(tag))
            .unwrap()
            .text()
            .unwrap()
    };
    assert_eq!(text("Name"), ch.name);
    assert_eq!(text("ObjectType"), WIZARD.to_string());
    assert_eq!(text("Level"), "20");
    assert_eq!(text("KilledBy"), "A & B <C>");
    assert_eq!(text("BaseFame"), "20");
    assert_eq!(
        [
            text("Shots"),
            text("ShotsThatDamage"),
            text("TilesUncovered"),
            text("MonsterKills")
        ],
        ["9", "3", "11", "2"]
    );
    let stamp = text("CreatedOn").parse::<u64>().unwrap();
    assert!(stamp > 0);
    let paid = store.presentation(account).await.unwrap();
    assert_eq!(text("TotalFame").parse::<i64>().unwrap(), paid.fame);
    assert!(store.load(account, ch.id).await.is_err());
    assert!(store.fame(other, ch.id).await.is_err());
    assert!(store.death(ch.clone(), "replayed".into()).await.is_err());
    assert!(store.progress(ch.clone()).await.is_err());
    let replacement = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert_ne!(replacement.id, ch.id);
    assert_eq!(replacement.level, 1);
    assert_eq!(store.list(guid.clone()).await.unwrap().characters.len(), 1);
    let dead_id = ch.id;
    store.call(move |db| {
        let (items, snapshot, recorded): (String, String, u64) = db.query_row("SELECT c.items,d.snapshot,d.died_at FROM characters c JOIN deaths d ON d.character=c.id WHERE c.id=?1", [dead_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        assert_eq!(stamp, recorded);
        assert_eq!(serde_json::from_str::<Vec<i32>>(&items)?, [-1; 12]);
        let archived: Character = serde_json::from_str(&snapshot)?;
        assert_eq!(archived.items, ch.items);
        assert!(!archived.alive && archived.hp == 0);
        Ok(())
    }).await.unwrap();
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(store.fame(account, dead_id).await.unwrap(), fame);
    assert_eq!(store.presentation(account).await.unwrap(), paid);
    assert!(store.load(account, dead_id).await.is_err());
    assert_eq!(store.load(account, replacement.id).await.unwrap().level, 1);
    assert_eq!(store.list(guid).await.unwrap().characters.len(), 1);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
