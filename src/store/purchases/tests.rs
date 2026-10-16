use super::*;

async fn fixture() -> (std::path::PathBuf, Store, Character) {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("purchase-{}.db", rand::random::<u64>()));
    let s = Store::open(&path, &content).unwrap();
    let account = s
        .register(
            "buyer".into(),
            "buyer@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    (path, s, ch)
}
fn offer(ch: &Character, currency: u8, price: i32) -> (InventoryWrite, Purchase) {
    let slot = (4..12).find(|i| ch.items[*i] == -1).unwrap();
    let mut items = ch.items.clone();
    items[slot] = 0xa50;
    (
        InventoryWrite {
            id: ch.id,
            account: ch.account,
            revision: ch.revision,
            items,
        },
        Purchase {
            slot,
            item: 0xa50,
            price,
            currency,
        },
    )
}

#[tokio::test]
async fn purchase_is_atomic_versioned_and_durable_without_resetting_fame_or_collection() {
    let (path, s, ch) = fixture().await;
    let (write, buy) = offer(&ch, 0, 60);
    let (a, b) = tokio::join!(
        s.purchase(write.clone(), buy.clone()),
        s.purchase(write, buy)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let content = Content::load(Path::new("data")).unwrap();
    let reopened = Store::open(&path, &content).unwrap();
    let bought = reopened.load(ch.account, ch.id).await.unwrap();
    assert_eq!(bought.revision, ch.revision + 1);
    assert_eq!(bought.items.iter().filter(|i| **i == 0xa50).count(), 1);
    assert_eq!(reopened.presentation(ch.account).await.unwrap().credits, 40);
    let (write, buy) = offer(&bought, 0, 41);
    assert_eq!(s.purchase(write, buy).await.unwrap(), None);
    assert_eq!(s.load(ch.account, ch.id).await.unwrap().items, bought.items);
    let account = ch.account;
    s.call(move |db| {
        db.execute(
            "UPDATE account_fame SET balance=200,earned=300 WHERE account=?1",
            [account],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let (write, buy) = offer(&bought, 1, 150);
    assert_eq!(
        s.purchase(write, buy).await.unwrap(),
        Some((bought.revision + 1, 50))
    );
    let view = s.presentation(ch.account).await.unwrap();
    assert_eq!((view.credits, view.fame), (40, 50));
    assert_eq!(
        s.characters("buyer@example.test".into(), "test-password".into())
            .await
            .unwrap()
            .earned_fame,
        300
    );
    assert_eq!(
        s.call(move |db| Ok(db.query_row(
            "SELECT count(*) FROM collections WHERE account=?1",
            [account],
            |r| r.get::<_, i32>(0)
        )?))
        .await
        .unwrap(),
        0
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn purchase_rolls_back_debit_if_inventory_write_fails() {
    let (path, s, ch) = fixture().await;
    s.call(|db|{db.execute_batch("CREATE TRIGGER purchase_fail BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'injected inventory write failure'); END;")?;Ok(())}).await.unwrap();
    let (write, buy) = offer(&ch, 0, 7);
    assert!(
        s.purchase(write, buy)
            .await
            .unwrap_err()
            .to_string()
            .contains("injected")
    );
    assert_eq!(s.presentation(ch.account).await.unwrap().credits, 100);
    assert_eq!(s.load(ch.account, ch.id).await.unwrap().items, ch.items);
    assert_eq!(
        s.load(ch.account, ch.id).await.unwrap().revision,
        ch.revision
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn purchase_rejects_foreign_dead_guest_invalid_price_and_extra_items_without_charging() {
    let (path, s, ch) = fixture().await;
    for failure in 0..6 {
        let (mut write, mut buy) = offer(&ch, 0, 7);
        match failure {
            0 => write.account += 99,
            1 => write.revision += 1,
            2 => buy.price = 0,
            3 => buy.currency = 2,
            4 => write.items[11] = 0xa50,
            5 => buy.slot = 0,
            _ => unreachable!(),
        }
        assert!(s.purchase(write, buy).await.is_err(), "case {failure}");
        assert_eq!(s.presentation(ch.account).await.unwrap().credits, 100);
    }
    let guest = s.list("guest-buyer".into()).await.unwrap().id;
    let content = Content::load(Path::new("data")).unwrap();
    let guest = s.create(guest, content.get(WIZARD).unwrap()).await.unwrap();
    let (write, buy) = offer(&guest, 0, 7);
    assert!(s.purchase(write, buy).await.is_err());
    let mut dead = ch.clone();
    dead.hp = 0;
    dead.alive = false;
    s.death(dead, "test".into()).await.unwrap();
    let (write, buy) = offer(&ch, 0, 7);
    assert!(s.purchase(write, buy).await.is_err());
    assert_eq!(s.presentation(ch.account).await.unwrap().credits, 100);
    std::fs::remove_file(path).unwrap();
}
