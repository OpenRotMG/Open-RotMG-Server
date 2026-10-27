use super::*;

fn fixture(item: u16, minutes: i32, count: i32) -> World {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let mut map = Map {
        width: 4,
        height: 4,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            16
        ],
        spawns: vec![(1.5, 2.5)],
    };
    map.cells[5].object = Some(0x1ca);
    map.cells[5].config = format!("mtype:{item};mcount:{count};mtime:{minutes}");
    World::new(content, Arc::new(map)).unwrap()
}

fn merchant(world: &World) -> &Entity {
    world
        .entities
        .values()
        .find(|e| world.content.objects[&e.kind].class == "Merchant")
        .unwrap()
}

fn values(world: &World) -> Vec<(u8, i32)> {
    merchant(world).extra_stats.clone()
}

#[test]
fn merchant_snapshot_countdown_discount_and_unknown_prices() {
    let mut world = fixture(0x131e, 6, 4);
    let id = merchant(&world).id;
    for (now, minutes, discount, price, stock) in [
        (0, 6, 0, 120, 4),
        (60_000, 5, 25, 90, 4),
        (120_000, 4, 25, 90, 4),
        (360_000, 0, 25, 90, 4),
        (420_000, 0, 0, 120, 0),
    ] {
        world.now = now;
        world.merchant_step();
        for stat in [(43, minutes), (44, discount), (36, price), (42, stock)] {
            assert!(values(&world).contains(&stat), "at {now}: {stat:?}");
        }
        assert_eq!(merchant(&world).id, id);
    }
    let mut world = fixture(0xa50, 5, 3);
    assert!(values(&world).contains(&(45, 7)));
    assert!(values(&world).contains(&(44, 0)));
    world.now = 60_000;
    world.merchant_step();
    assert!(values(&world).contains(&(44, 10)));
    assert!(values(&world).contains(&(36, 32)));
    assert!(values(&fixture(0xb3e, 0, 1)).contains(&(44, 0)));
    let world = fixture(0xa00, 0, 1);
    assert!(!values(&world).iter().any(|(id, _)| matches!(id, 36 | 40)));
    assert!(values(&fixture(0xc42, -1, 5)).contains(&(43, -1)));
    let mut world = fixture(0xb0f, i32::MAX, 3);
    assert!(values(&world).contains(&(43, i32::MAX)));
    world.now = 60_000;
    world.merchant_step();
    assert!(values(&world).contains(&(43, -1)));
}

#[tokio::test]
async fn merchant_reservations_freeze_quotes_and_settle_once_atomically() {
    let mut world = fixture(0x131e, 6, 2);
    let path = std::env::temp_dir().join(format!("merchant-{}.db", rand::random::<u64>()));
    let store = crate::store::Store::open(&path, &world.content).unwrap();
    let account = store
        .register(
            "merchant-test".into(),
            "merchant@example.test".into(),
            "merchant-test-password".into(),
        )
        .await
        .unwrap();
    let ch = store
        .create(account, world.content.get(0x30e).unwrap())
        .await
        .unwrap();
    let original = ch.clone();
    let player = world.enter(ch, None).unwrap();
    let seller = merchant(&world).id;
    world.players.get_mut(&player).unwrap().known.insert(seller);
    let saved_rank = merchant(&world).extra_stats.clone();
    world
        .entities
        .get_mut(&seller)
        .unwrap()
        .extra_stats
        .iter_mut()
        .find(|(id, _)| *id == 45)
        .unwrap()
        .1 = 50;
    assert!(format!("{:#}", world.buy(player, seller).err().unwrap()).contains("rank"));
    assert!(!world.entities[&seller].reserved);
    world.entities.get_mut(&seller).unwrap().extra_stats = saved_rank;
    let plan = world.buy(player, seller).unwrap();
    assert!(world.buy(player, seller).is_err());
    world.now = 60_000;
    world.merchant_step();
    assert!(values(&world).contains(&(36, 120)));
    let settle = |plan: &InventoryPlan| crate::store::InventoryWrite {
        id: plan.character,
        account: plan.account,
        revision: plan.revision,
        items: plan.items.clone(),
    };
    let declined = store
        .purchase(settle(&plan), plan.purchase.as_ref().unwrap().1.clone())
        .await
        .unwrap();
    assert!(declined.is_none());
    world.finish_purchase(plan, Ok(declined)).unwrap();
    assert_eq!(
        store.load(account, original.id).await.unwrap().items,
        original.items
    );
    assert!(values(&world).contains(&(42, 2)));
    world.merchant_step();
    assert!(values(&world).contains(&(36, 90)));
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE accounts SET credits=90 WHERE id=?1", [account])
        .unwrap();
    let plan = world.buy(player, seller).unwrap();
    let write = settle(&plan);
    let purchase = plan.purchase.as_ref().unwrap().1.clone();
    db.execute_batch("CREATE TRIGGER reject_delivery BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'forced purchase rollback'); END;").unwrap();
    assert!(
        store
            .purchase(write.clone(), purchase.clone())
            .await
            .is_err()
    );
    assert_eq!(store.presentation(account).await.unwrap().credits, 90);
    assert_eq!(
        store.load(account, original.id).await.unwrap().items,
        original.items
    );
    db.execute_batch("DROP TRIGGER reject_delivery").unwrap();
    world.now = 420_000;
    world.merchant_step();
    let receipt = store
        .purchase(write.clone(), purchase.clone())
        .await
        .unwrap();
    assert_eq!(receipt, Some((1, 0)));
    assert!(store.purchase(write, purchase).await.is_err());
    world.finish_purchase(plan, Ok(receipt)).unwrap();
    assert_eq!(world.players[&player].character.items[4], 0x131e);
    assert!(values(&world).contains(&(42, 1)));
    world.merchant_step();
    assert!(!world.entities[&seller].published);
    let error = world.buy(player, seller).err().unwrap();
    assert!(format!("{error:#}").contains("expired"));
    assert_eq!(store.load(account, original.id).await.unwrap().revision, 1);
    assert_eq!(store.presentation(account).await.unwrap().credits, 0);
    drop(db);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
