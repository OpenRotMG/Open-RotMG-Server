use super::*;

#[tokio::test]
async fn personal_news_is_bounded_account_scoped_and_uses_committed_awards() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("personal-news-{}.db", rand::random::<u64>()));
    let store = Store::open(&path, &content).unwrap();
    let owner = store.list("news-owner".into()).await.unwrap().id;
    let other = store.list("news-other".into()).await.unwrap().id;
    assert!(
        store
            .news(owner, "https://example.test")
            .await
            .unwrap()
            .is_empty()
    );
    let db = Connection::open(&path).unwrap();
    let mut ids = Vec::new();
    for i in 0..12 {
        let mut ch = store
            .create(owner, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.hp = 0;
        ch.alive = false;
        ch.fame = 20 + i;
        store.death(ch.clone(), "news test".into()).await.unwrap();
        assert!(store.death(ch.clone(), "duplicate".into()).await.is_err());
        db.execute(
            "UPDATE deaths SET died_at=?2 WHERE character=?1",
            params![ch.id, 1000 + i],
        )
        .unwrap();
        ids.push(ch.id);
    }
    let expected = store.news(owner, "https://example.test").await.unwrap();
    assert!(
        store
            .news(other, "https://example.test")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.fame(other, ids[11]).await.is_err());
    let xml = format!("<News>{expected}</News>");
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let items: Vec<_> = doc
        .root_element()
        .children()
        .filter(|n| n.is_element())
        .collect();
    assert_eq!(items.len(), 10);
    for (item, id) in items.iter().zip(ids.iter().rev()) {
        let field = |tag| {
            item.children()
                .find(|n| n.has_tag_name(tag))
                .unwrap()
                .text()
                .unwrap()
                .to_string()
        };
        assert_eq!(field("Icon"), "fame");
        assert_eq!(field("Title"), "Your Wizard died at level 1");
        assert_eq!(field("Link"), format!("fame:{id}"));
        let fame = store.fame(owner, *id).await.unwrap();
        let fame = roxmltree::Document::parse(&fame).unwrap();
        let total = fame
            .descendants()
            .find(|n| n.has_tag_name("TotalFame"))
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(
            field("TagLine"),
            format!("You earned {total} glorious Fame")
        );
    }
    store
        .sync_updates(UpdateSnapshot {
            guild_id: "111111111111111111".into(),
            channel_id: "222222222222222222".into(),
            posts: vec![UpdatePost {
                id: "333333333333333333".into(),
                content: "Discord <update>\nPatch & notes".into(),
                published_at: 1007,
                edited_at: None,
                attachments: vec![],
            }],
        })
        .await
        .unwrap();
    let news = store.news(owner, "https://example.test").await.unwrap();
    assert_eq!(news.matches("<Item>").count(), 10);
    assert!(news.contains("Discord &lt;update&gt;"));
    assert!(news.contains("Patch &amp; notes"));
    assert!(news.contains("<Link>https://example.test/updates/</Link>"));
    assert!(
        news.find(&format!("fame:{}", ids[7])).unwrap()
            < news.find("Discord &lt;update&gt;").unwrap()
    );
    assert!(
        news.find("Discord &lt;update&gt;").unwrap()
            < news.find(&format!("fame:{}", ids[6])).unwrap()
    );
    assert!(!news.contains(&format!("fame:{}<", ids[2])));
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(
        reopened.news(owner, "https://example.test").await.unwrap(),
        news
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        12
    );
    db.execute(
        "UPDATE deaths SET receipt='{}' WHERE character=?1",
        [ids[11]],
    )
    .unwrap();
    assert!(reopened.news(owner, "https://example.test").await.is_err());
}
