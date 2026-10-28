use super::*;

#[tokio::test]
async fn vault_purchase_rollback_capacity_and_death_preserve_account_storage() {
    use crate::store::{InventoryWrite, Store};
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let path = std::env::temp_dir().join(format!("vault-{}.db", rand::random::<u64>()));
    let store = Store::open(&path, &content).unwrap();
    let account = store
        .register(
            "vault-fixture".into(),
            "vault@example.test".into(),
            "vault-fixture-password".into(),
        )
        .await
        .unwrap();
    let other = store
        .account("vault-other".into(), String::new())
        .await
        .unwrap()
        .id;
    let ch = store
        .create(account, content.get(0x30e).unwrap())
        .await
        .unwrap();
    let mut world = World::new(
        content.clone(),
        Arc::new(Map::load(std::path::Path::new("data/vault.wmap"), &content).unwrap()),
    )
    .unwrap();
    world.name = "Vault".into();
    world.owner = Some(account);
    world
        .load_vaults(store.vaults(account).await.unwrap())
        .unwrap();
    let player = world
        .enter_at(
            ch.clone(),
            None,
            None,
            store.presentation(account).await.unwrap(),
        )
        .unwrap();
    let write = |plan: &InventoryPlan| InventoryWrite {
        id: plan.character,
        account: plan.account,
        revision: plan.revision,
        items: plan.items.clone(),
    };
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE accounts SET credits=499 WHERE id=?1", [account])
        .unwrap();
    let seller = *world.closed_vaults.keys().next().unwrap();
    world.players.get_mut(&player).unwrap().known.insert(seller);
    world.players.get_mut(&player).unwrap().pos = world.entities[&seller].pos;
    let plan = world.buy(player, seller).unwrap();
    let chest = plan.vault_purchase.as_ref().unwrap().chest;
    let declined = store.purchase_vault(write(&plan), chest).await.unwrap();
    assert!(declined.is_none());
    world.finish_vault_purchase(plan, Ok(declined)).unwrap();
    assert_eq!(store.presentation(account).await.unwrap().credits, 499);
    assert_eq!(store.load(account, ch.id).await.unwrap().revision, 0);
    assert_eq!(world.vaults.len(), 1);
    assert!(!world.entities[&seller].reserved);
    db.execute("UPDATE accounts SET credits=9500 WHERE id=?1", [account])
        .unwrap();
    for count in 2..=20 {
        let seller = *world.closed_vaults.keys().next().unwrap();
        world.players.get_mut(&player).unwrap().known.insert(seller);
        world.players.get_mut(&player).unwrap().pos = world.entities[&seller].pos;
        let plan = world.buy(player, seller).unwrap();
        assert!(world.buy(player, seller).is_err());
        let write = write(&plan);
        let chest = plan.vault_purchase.as_ref().unwrap().chest;
        if count == 2 {
            db.execute_batch("CREATE TRIGGER reject_vault BEFORE INSERT ON vaults BEGIN SELECT RAISE(ABORT,'vault rollback fixture'); END;").unwrap();
            assert!(store.purchase_vault(write.clone(), chest).await.is_err());
            assert_eq!(store.presentation(account).await.unwrap().credits, 9500);
            assert_eq!(store.vaults(account).await.unwrap().len(), 1);
            assert_eq!(store.load(account, ch.id).await.unwrap().revision, 0);
            db.execute_batch("DROP TRIGGER reject_vault").unwrap();
        }
        let receipt = store.purchase_vault(write.clone(), chest).await.unwrap();
        assert!(store.purchase_vault(write, chest).await.is_err());
        world.finish_vault_purchase(plan, Ok(receipt)).unwrap();
        assert_eq!(world.vaults.len(), count);
        assert_eq!(store.vaults(account).await.unwrap().len(), count);
        assert_eq!(
            store.presentation(account).await.unwrap().credits,
            (20 - count as i32) * 500
        );
        assert!(!world.entities.contains_key(&seller));
    }
    assert!(world.closed_vaults.is_empty());
    assert_eq!(store.load(account, ch.id).await.unwrap().revision, 19);
    let mut chest = store.vault(account, 0).await.unwrap();
    chest.items[7] = 0xa22;
    let mut saved = store.load(account, ch.id).await.unwrap();
    saved.revision = store
        .exchange(
            vec![InventoryWrite {
                id: saved.id,
                account,
                revision: saved.revision,
                items: saved.items.clone(),
            }],
            vec![chest],
        )
        .await
        .unwrap()[0];
    let before = store.vaults(account).await.unwrap();
    saved.hp = 0;
    saved.alive = false;
    store
        .death(saved, "Vault persistence fixture".into())
        .await
        .unwrap();
    let replacement = store
        .create(account, content.get(0x30e).unwrap())
        .await
        .unwrap();
    assert_ne!(replacement.id, ch.id);
    assert!(store.load(account, ch.id).await.is_err());
    drop(world);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.vaults(account).await.unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(reopened.vaults(other).await.unwrap()[0].items, [-1; 8]);
    assert_eq!(reopened.vaults(other).await.unwrap().len(), 1);
    assert_eq!(reopened.presentation(account).await.unwrap().credits, 0);
    assert!(reopened.vault(account, 20).await.is_err());
    assert!(
        reopened
            .purchase_vault(
                InventoryWrite {
                    id: replacement.id,
                    account: other,
                    revision: replacement.revision,
                    items: replacement.items
                },
                1
            )
            .await
            .is_err()
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE character=?1",
            [ch.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    drop(reopened);
    drop(db);
    std::fs::remove_file(path).unwrap();
}

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
    let (packets, mut inbox) = mpsc::channel(64);
    let (lost, _) = watch::channel(None);
    world.players.get_mut(&player).unwrap().tx = Some(Output { packets, lost });
    world.step();
    world.step();
    while inbox.try_recv().is_ok() {}
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
    world.step();
    world.step();
    let mut published = BTreeMap::new();
    while let Ok(packet) = inbox.try_recv() {
        if packet.id != wire::NEW_TICK {
            continue;
        }
        let mut r = wire::Reader::new(&packet.body);
        r.i32().unwrap();
        r.i32().unwrap();
        for _ in 0..r.i16().unwrap() {
            let entity = r.i32().unwrap();
            r.f32().unwrap();
            r.f32().unwrap();
            for _ in 0..r.i16().unwrap() {
                let key = r.u8().unwrap();
                if matches!(key, 31 | 62) {
                    r.utf().unwrap();
                } else {
                    let value = r.i32().unwrap();
                    if entity == seller {
                        published.insert(key, value);
                    }
                }
            }
        }
        r.done().unwrap();
    }
    assert_eq!(published.get(&43), Some(&5));
    assert_eq!(published.get(&44), Some(&25));
    assert_eq!(published.get(&36), Some(&90));
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE accounts SET credits=90 WHERE id=?1", [account])
        .unwrap();
    let plan = world.buy(player, seller).unwrap();
    let write = settle(&plan);
    let purchase = plan.purchase.as_ref().unwrap().1.clone();
    let slot = purchase.slot;
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
    assert_eq!(world.players[&player].character.items[slot], 0x131e);
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
