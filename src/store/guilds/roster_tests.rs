use super::*;

#[tokio::test]
async fn guild_roster_pages_permissions_and_rejoined_contribution() {
    let content = Content::load(Path::new("data")).unwrap();
    let path = std::env::temp_dir().join(format!("guild-roster-{}.db", rand::random::<u64>()));
    let store = Store::open(&path, &content).unwrap();
    let mut accounts = Vec::new();
    for name in ["Founder", "Alpha", "Beta", "Outsider"] {
        let account = store
            .register(
                name.into(),
                format!("{name}@example.test"),
                "test-password".into(),
            )
            .await
            .unwrap();
        store.choose_name(account, name.into()).await.unwrap();
        accounts.push(account);
    }
    let [founder, alpha, beta, outsider]: [i32; 4] = accounts.try_into().unwrap();
    store
        .create_prepared_guild(vec![founder, alpha, beta], "Roster Research".into())
        .await
        .unwrap();
    let mut ch = store
        .create(beta, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = 200;
    ch.hp = 0;
    ch.alive = false;
    store
        .death(ch.clone(), "prepared roster contribution".into())
        .await
        .unwrap();
    let bank = store.guild(founder).await.unwrap();
    assert!(bank.fame >= 200);
    let roster = store.guild_roster(founder, 0, 50).await.unwrap();
    assert!(roster.contains(&format!("<CurrentFame>{}</CurrentFame>", bank.fame)));
    assert!(
        roster.find("<Name>Founder</Name>").unwrap() < roster.find("<Name>Beta</Name>").unwrap()
    );
    assert!(roster.find("<Name>Beta</Name>").unwrap() < roster.find("<Name>Alpha</Name>").unwrap());
    assert!(store.guild_roster(outsider, 0, 50).await.is_err());
    for (offset, count) in [(51, 1), (0, 0), (0, 51)] {
        assert!(store.guild_roster(founder, offset, count).await.is_err());
    }
    assert!(
        store
            .change_guild(
                beta,
                GuildAction::Rank {
                    name: "Founder".into(),
                    rank: 0
                }
            )
            .await
            .is_err()
    );
    assert!(
        store
            .change_guild(beta, GuildAction::Remove("Alpha".into()))
            .await
            .is_err()
    );
    store
        .change_guild(
            founder,
            GuildAction::Rank {
                name: "Alpha".into(),
                rank: 10,
            },
        )
        .await
        .unwrap();
    let roster = store.guild_roster(alpha, 0, 50).await.unwrap();
    let pages = ["Founder", "Alpha", "Beta"];
    for (offset, name) in pages.into_iter().enumerate() {
        let page = store.guild_roster(alpha, offset, 1).await.unwrap();
        assert_eq!(page.matches("<Member>").count(), 1);
        assert!(page.contains(&format!("<Name>{name}</Name>")));
    }
    assert!(roster.find("<Name>Alpha</Name>").unwrap() < roster.find("<Name>Beta</Name>").unwrap());
    assert!(
        !store
            .guild_roster(alpha, 50, 50)
            .await
            .unwrap()
            .contains("<Member>")
    );
    store
        .change_guild(founder, GuildAction::Remove("Beta".into()))
        .await
        .unwrap();
    assert!(store.guild_roster(beta, 0, 50).await.is_err());
    assert!(
        !store
            .guild_roster(founder, 0, 50)
            .await
            .unwrap()
            .contains("<Name>Beta</Name>")
    );
    store
        .change_guild(
            beta,
            GuildAction::Join {
                guild: bank.id,
                inviter: founder,
            },
        )
        .await
        .unwrap();
    let rejoined = store.guild_roster(beta, 0, 50).await.unwrap();
    assert!(rejoined.contains("<Member><Name>Beta</Name><Rank>0</Rank><Fame>0</Fame></Member>"));
    assert!(
        store
            .death(ch, "duplicate roster contribution".into())
            .await
            .is_err()
    );
    assert_eq!(store.guild(founder).await.unwrap().fame, bank.fame);
    assert_eq!(store.guild_roster(beta, 0, 50).await.unwrap(), rejoined);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.guild_roster(beta, 0, 50).await.unwrap(), rejoined);
    assert_eq!(reopened.guild(founder).await.unwrap().earned, bank.earned);
}
