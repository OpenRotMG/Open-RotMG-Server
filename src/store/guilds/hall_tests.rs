use super::*;

#[tokio::test]
async fn guild_hall_upgrades_are_ranked_atomic_and_match_selected_prices() {
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let founder = store.list("hall-founder".into()).await.unwrap().id;
    let peer = store.list("hall-peer".into()).await.unwrap().id;
    let outsider = store.list("hall-outsider".into()).await.unwrap().id;
    store
        .create_prepared_guild(vec![founder, peer], "Hall Tests".into())
        .await
        .unwrap();
    let guild = store.guild(founder).await.unwrap().id;
    let selected = roxmltree::Document::parse(include_str!("../../../data/objects.xml")).unwrap();
    for (level, price) in (1..=3).zip(HALL_PRICES) {
        let name = format!("Guild Hall Upgrade {level}");
        let object = selected
            .descendants()
            .find(|n| n.attribute("id") == Some(name.as_str()))
            .unwrap();
        assert_eq!(
            object
                .children()
                .find(|n| n.has_tag_name("Price"))
                .unwrap()
                .text()
                .unwrap()
                .parse::<i32>()
                .unwrap(),
            price
        );
    }
    assert!(
        store
            .change_guild(founder, GuildAction::Upgrade { level: 1 })
            .await
            .unwrap_err()
            .to_string()
            .contains("Not enough Guild Fame")
    );
    assert!(
        store
            .change_guild(outsider, GuildAction::Upgrade { level: 1 })
            .await
            .is_err()
    );
    store
        .call(move |db| {
            db.execute(
                "UPDATE guilds SET fame=360000,earned=360000 WHERE id=?1",
                [guild],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    for rank in [0, 10, 20] {
        store
            .call(move |db| {
                db.execute(
                    "UPDATE guild_members SET rank=?2 WHERE account=?1",
                    params![peer, rank],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            store
                .change_guild(peer, GuildAction::Upgrade { level: 1 })
                .await
                .unwrap_err()
                .to_string()
                .contains("Only Leaders")
        );
        assert_eq!(store.guild(founder).await.unwrap().fame, 360000);
    }
    store
        .call(move |db| {
            db.execute("UPDATE guild_members SET rank=30 WHERE account=?1", [peer])?;
            Ok(())
        })
        .await
        .unwrap();
    let mut remaining = 360000;
    for (level, price) in (1..=3).zip(HALL_PRICES) {
        let actor = if level == 1 { peer } else { founder };
        let (a, b) = tokio::join!(
            store.change_guild(actor, GuildAction::Upgrade { level }),
            store.change_guild(actor, GuildAction::Upgrade { level })
        );
        assert_ne!(a.is_ok(), b.is_ok());
        remaining -= price;
        let state = store.guild(founder).await.unwrap();
        assert_eq!(
            (state.level, state.fame, state.earned),
            (level + 1, remaining, 360000)
        );
        assert_eq!(store.presentation(founder).await.unwrap().fame, 0);
        assert_eq!(store.presentation(peer).await.unwrap().fame, 0);
    }
    assert!(
        store
            .change_guild(founder, GuildAction::Upgrade { level: 4 })
            .await
            .is_err()
    );
    assert_eq!(store.guild(founder).await.unwrap().fame, 0);
}
