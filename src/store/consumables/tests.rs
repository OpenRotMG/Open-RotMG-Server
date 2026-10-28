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
async fn stat_potions_clip_all_class_caps_and_reject_stale_or_failed_commits() {
    let content = Content::load(Path::new("data")).unwrap();
    let s = Store::open(Path::new(":memory:"), &content).unwrap();
    for (&class, definition) in &content.classes {
        for (index, kind) in [0xae9, 0xaea, 0xa1f, 0xa20, 0xa21, 0xa4c, 0xa34, 0xa35]
            .into_iter()
            .enumerate()
        {
            for remaining in 1..=if index < 2 { 6 } else { 2 } {
                let account = s
                    .list(format!("potions-{class}-{index}-{remaining}"))
                    .await
                    .unwrap();
                let mut ch = s
                    .create_prepared(account.id, content.get(class).unwrap(), None)
                    .await
                    .unwrap();
                ch.base[index] = definition.caps[index] - remaining;
                ch.items[4] = kind;
                ch.revision = s.exchange(vec![write(&ch)], vec![]).await.unwrap()[0];
                s.progress(ch.clone()).await.unwrap();
                let before = snapshot(&s, ch.id).await;
                s.call(|db| {
                    db.execute_batch("CREATE TRIGGER potion_fail AFTER UPDATE ON characters BEGIN SELECT RAISE(ABORT,'injected potion failure'); END;")?;
                    Ok(())
                }).await.unwrap();
                let amount = remaining.min(if index < 2 { 5 } else { 1 });
                assert!(
                    s.consume_stat(ch.clone(), Some(4), index, amount, vec![])
                        .await
                        .is_err()
                );
                assert_eq!(snapshot(&s, ch.id).await, before);
                s.call(|db| {
                    db.execute_batch("DROP TRIGGER potion_fail")?;
                    Ok(())
                })
                .await
                .unwrap();
                let revision = s
                    .consume_stat(ch.clone(), Some(4), index, amount, vec![])
                    .await
                    .unwrap();
                let saved = s.load(ch.account, ch.id).await.unwrap();
                let mut expected = ch.base;
                expected[index] += amount;
                assert_eq!(saved.base, expected);
                assert_eq!(
                    (saved.items[4], revision, saved.revision),
                    (-1, ch.revision + 1, revision)
                );
                let committed = snapshot(&s, ch.id).await;
                let id = ch.id;
                assert!(
                    s.consume_stat(ch, Some(4), index, amount, vec![])
                        .await
                        .is_err()
                );
                assert!(
                    s.consume_stat(saved, Some(4), index, amount, vec![])
                        .await
                        .is_err()
                );
                assert_eq!(snapshot(&s, id).await, committed);
            }
        }
    }
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
