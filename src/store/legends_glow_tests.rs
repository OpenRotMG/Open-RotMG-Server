use super::*;

async fn fixture() -> (Store, Content) {
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    (store, content)
}

async fn death(store: &Store, content: &Content, account: i32, time: i64) -> i32 {
    let mut ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.alive = false;
    ch.hp = 0;
    let id = ch.id;
    store.death(ch, "fixture".into()).await.unwrap();
    store
        .call(move |db| {
            db.execute(
                "UPDATE deaths SET died_at=?1 WHERE character=?2",
                params![time, id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    id
}

async fn account(store: &Store, content: &Content, guid: &str) -> i32 {
    let account = store.list(guid.into()).await.unwrap().id;
    // Keep the first-death Ancestor award out of the equal-score ranking fixture.
    death(store, content, account, 4_000_000_000).await;
    account
}

async fn glowing(store: &Store, account: i32, now: i64) -> bool {
    let base = store.legacy_base;
    store
        .call(move |db| legends::is_legend_at(db, account, now, base))
        .await
        .unwrap()
}

#[tokio::test]
async fn legends_glow_ranks_characters_not_distinct_accounts_and_breaks_ties_exactly() {
    let (store, content) = fixture().await;
    let now = 2_000_000_000;
    let repeated = account(&store, &content, "glow-repeat").await;
    let tenth = account(&store, &content, "glow-tenth").await;
    let eleventh = account(&store, &content, "glow-eleventh").await;
    for _ in 0..9 {
        death(&store, &content, repeated, now).await;
    }
    death(&store, &content, tenth, now).await;
    let last = death(&store, &content, eleventh, now).await;
    let live = store
        .create(tenth, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(glowing(&store, repeated, now).await);
    assert!(glowing(&store, live.account, now).await);
    assert!(!glowing(&store, eleventh, now).await);
    store
        .call(move |db| {
            db.execute(
                "UPDATE deaths SET died_at=?1 WHERE character=?2",
                params![now - 1, last],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(glowing(&store, eleventh, now).await);
    assert!(!glowing(&store, tenth, now).await);
}

#[tokio::test]
async fn legends_glow_unions_windows_and_excludes_cutoffs_and_future_deaths() {
    let (store, content) = fixture().await;
    let now = 2_000_000_000;
    let old = account(&store, &content, "glow-all").await;
    let month = account(&store, &content, "glow-month").await;
    for _ in 0..10 {
        death(&store, &content, old, now - 31 * 86400).await;
    }
    for _ in 0..10 {
        death(&store, &content, month, now - 8 * 86400).await;
    }
    assert!(glowing(&store, old, now).await);
    assert!(glowing(&store, month, now).await);
    for (role, time, expected) in [
        ("month-cutoff", now - 30 * 86400, false),
        ("month-inside", now - 30 * 86400 + 1, true),
        ("week-cutoff", now - 7 * 86400, false),
        ("week-inside", now - 7 * 86400 + 1, true),
        ("present", now, true),
        ("future", now + 1, false),
    ] {
        let account = account(&store, &content, &format!("glow-{role}")).await;
        death(&store, &content, account, time).await;
        assert_eq!(glowing(&store, account, now).await, expected, "{role}");
    }
    let unranked = account(&store, &content, "glow-unranked").await;
    assert!(!glowing(&store, unranked, now).await);
    assert!(glowing(&store, old, now).await);
    assert!(glowing(&store, month, now).await);
}

#[tokio::test]
async fn legends_glow_propagates_foreign_receipt_and_query_errors() {
    let (store, content) = fixture().await;
    let owner = account(&store, &content, "glow-corrupt").await;
    let reader = account(&store, &content, "glow-reader").await;
    death(&store, &content, owner, 1_000_000).await;
    store
        .call(|db| {
            db.execute(
                "UPDATE deaths SET receipt=json_set(receipt,'$.version',2)",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .is_legend(reader)
            .await
            .unwrap_err()
            .to_string()
            .contains("unsupported fame version 2")
    );
    store
        .call(|db| {
            db.execute_batch("DROP TABLE deaths")?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(store.is_legend(reader).await.is_err());
}
