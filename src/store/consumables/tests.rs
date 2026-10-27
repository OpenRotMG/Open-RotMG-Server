use super::*;

async fn fixture(path: &Path) -> (Store, Character) {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(path, &content).unwrap();
    let account = s.list("dye-consumables".into()).await.unwrap();
    let mut ch = s
        .create(account.id, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.items = vec![
        0xa97, 0xa2e, 0xa5c, 0xa4f, 0x1000, 0xa50, 0xa50, 0xa50, 0xa50, 0xa50, 0xa50, 0xa50,
    ];
    ch.revision = s.exchange(vec![write(&ch)], vec![]).await.unwrap()[0];
    let id = ch.id;
    s.call(move |db| {
        db.execute(
            "UPDATE characters SET stats=json_set(stats,'$.tex1',17,'$.tex2',23) WHERE id=?1",
            [id],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let ch = s.load(ch.account, ch.id).await.unwrap();
    (s, ch)
}

fn write(ch: &Character) -> InventoryWrite {
    InventoryWrite {
        id: ch.id,
        account: ch.account,
        revision: ch.revision,
        items: ch.items.clone(),
    }
}

async fn snapshot(s: &Store, id: i32) -> (String, String, i32, bool) {
    s.call(move |db| {
        Ok(db.query_row(
            "SELECT stats,items,revision,alive FROM characters WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn dye_transaction_failure_rolls_back_appearance_inventory_and_revision() {
    let (s, ch) = fixture(Path::new(":memory:")).await;
    s.call(|db| {
        // AFTER forces the real UPDATE to run before SQLite aborts the transaction.
        db.execute_batch("CREATE TRIGGER dye_fail AFTER UPDATE ON characters BEGIN SELECT RAISE(ABORT,'injected dye transaction failure'); END;")?;
        Ok(())
    }).await.unwrap();
    let before = snapshot(&s, ch.id).await;
    for ground in [false, true] {
        let mut request = write(&ch);
        if !ground {
            request.items[4] = -1;
        }
        let result = if ground {
            s.consume_ground_dye(request, 0x1000, [0x01f0f8ff, ch.tex2])
                .await
        } else {
            s.consume_dye(request, 4, 0x1000, [0x01f0f8ff, ch.tex2])
                .await
        };
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected dye transaction failure")
        );
        assert_eq!(snapshot(&s, ch.id).await, before);
    }
    s.call(|db| {
        db.execute_batch("DROP TRIGGER dye_fail")?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        s.consume_ground_dye(write(&ch), 0x1000, [0x01f0f8ff, ch.tex2])
            .await
            .unwrap(),
        ch.revision + 1
    );
}
