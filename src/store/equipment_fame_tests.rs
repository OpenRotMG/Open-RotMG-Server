use super::*;

#[tokio::test]
async fn well_equipped_commits_once_and_archives_aggregate_floor() {
    let mut content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!(
        "well-equipped-{}-{}.db",
        std::process::id(),
        rand::random::<u64>()
    ));
    let store = Store::open(&path, &content).unwrap();
    let owner = store.list("well-equipped-owner".into()).await.unwrap();
    let mut ch = store
        .create(owner.id, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.items = vec![0xaa2, -1, -1, 0xabf, 0xc01, -1, -1, -1, -1, -1, -1, -1];
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
    ch.fame = 19;
    ch.exp = 19000;
    ch.hp = 0;
    ch.alive = false;
    store.death(ch.clone(), "Sand Devil".into()).await.unwrap();
    assert_eq!(
        store.presentation(ch.account).await.unwrap().fame,
        20,
        "combined two3% items must floor once, backpack6% is excluded"
    );
    let account = store.list("well-equipped-owner".into()).await.unwrap();
    assert_eq!(
        (
            account.fame,
            account.earned_fame,
            account.best_fame[&WIZARD]
        ),
        (20, 20, 19)
    );
    assert!(store.death(ch.clone(), "duplicate".into()).await.is_err());
    let xml = store.fame(ch.account, ch.id).await.unwrap();
    assert!(xml.contains("<BaseFame>19</BaseFame>"));
    assert!(xml.contains("<TotalFame>20</TotalFame>"));
    assert!(xml.contains("<Bonus id=\"Well Equipped\" desc=\"Bonus for equipment\">1</Bonus>"));
    let legends = store
        .legends("all".into(), ch.account, ch.id)
        .await
        .unwrap();
    assert!(legends.contains("<TotalFame>20</TotalFame>"));
    drop(store);
    content.objects.remove(&0xaa2);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.fame(ch.account, ch.id).await.unwrap(), xml);
    assert_eq!(
        reopened
            .legends("all".into(), ch.account, ch.id)
            .await
            .unwrap(),
        legends
    );
    assert_eq!(reopened.presentation(ch.account).await.unwrap().fame, 20);
}

#[tokio::test]
async fn well_equipped_legends_orders_actual_payout_and_keeps_personal_total() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("equipment-legends-{}.db", rand::random::<u64>()));
    let store = Store::open(&path, &content).unwrap();
    let mut ids = Vec::new();
    for i in 0..22 {
        let owner = store.list(format!("equipment-legend-{i}")).await.unwrap();
        let mut ch = store
            .create(owner.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        if i >= 20 {
            ch.items[0] = 0xaa2;
            ch.items[3] = 0xabf;
            if i == 20 {
                ch.items[2] = 0xb05;
            }
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
        }
        ch.fame = match i {
            20 => 95,
            21 => 19,
            _ => 100,
        };
        ch.hp = 0;
        ch.alive = false;
        ids.push((ch.account, ch.id));
        store
            .death(ch, "Legends equipment fixture".into())
            .await
            .unwrap();
    }
    let xml = store
        .legends("all".into(), ids[21].0, ids[21].1)
        .await
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let first = doc
        .descendants()
        .find(|n| n.has_tag_name("FameListElem"))
        .unwrap();
    assert_eq!(
        first.attribute("charId"),
        Some(ids[20].1.to_string().as_str())
    );
    assert_eq!(
        first
            .children()
            .find(|n| n.has_tag_name("TotalFame"))
            .unwrap()
            .text(),
        Some("104")
    );
    let mine = doc
        .descendants()
        .find(|n| n.has_tag_name("MyFameListElem"))
        .unwrap();
    assert_eq!(
        mine.children()
            .find(|n| n.has_tag_name("TotalFame"))
            .unwrap()
            .text(),
        Some("20")
    );
}
