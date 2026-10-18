use super::*;

async fn account(store: &Store, content: &Content, n: usize, fame: i32) -> (i32, String) {
    let a = store.list(format!("guild-test-{n}")).await.unwrap();
    let name = format!(
        "Player{}{}",
        (b'A' + (n / 26) as u8) as char,
        (b'A' + (n % 26) as u8) as char
    );
    store.rename(a.id, name.clone()).await.unwrap();
    let mut ch = store
        .create(a.id, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = fame;
    ch.alive = false;
    ch.hp = 0;
    store.death(ch, "guild fixture".into()).await.unwrap();
    (a.id, name)
}

fn setup() -> (Content, Store) {
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    (content, store)
}

#[tokio::test]
async fn guild_creation_is_atomic_named_unique_and_spends_only_wallet_fame() {
    let (c, s) = setup();
    let guest = s.list("unnamed-guild-founder".into()).await.unwrap();
    assert!(
        s.change_guild(guest.id, GuildAction::Create("No Name".into()))
            .await
            .is_err()
    );
    let (a, _) = account(&s, &c, 0, 1000).await;
    for name in [
        "",
        " ",
        "Guild1",
        "<Guild>",
        "Guild\tName",
        "TwentyOneLettersLonger",
        "日本語",
    ] {
        assert!(
            s.change_guild(a, GuildAction::Create(name.into()))
                .await
                .is_err()
        );
        assert_eq!(s.presentation(a).await.unwrap().fame, 1000);
    }
    let (guild, _) = s
        .change_guild(a, GuildAction::Create("  Guild   One ".into()))
        .await
        .unwrap();
    let p = s.presentation(a).await.unwrap();
    assert_eq!(
        p.guild,
        Some(Membership {
            id: guild,
            name: "Guild One".into(),
            rank: 40
        })
    );
    assert_eq!(p.fame, 0);
    let listed = s.list("guild-test-0".into()).await.unwrap();
    assert_eq!(listed.earned_fame, 1000);
    assert!(
        listed
            .xml("", 0.0)
            .contains("<Guild id=\"1\"><Name>Guild One</Name><Rank>40</Rank></Guild>")
    );
    let (b, _) = account(&s, &c, 1, 1000).await;
    assert!(
        s.change_guild(b, GuildAction::Create("gUILD oNE".into()))
            .await
            .is_err()
    );
    assert_eq!(s.presentation(b).await.unwrap().fame, 1000);
    assert!(s.presentation(b).await.unwrap().guild.is_none());
    assert!(
        s.change_guild(a, GuildAction::Create("Other".into()))
            .await
            .is_err()
    );
    assert_eq!(s.guild(a).await.unwrap().fame, 0);
}

#[test]
fn guild_upgrade_prices_match_the_selected_native_merchants() {
    let xml = std::fs::read_to_string("data/objects.xml").unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    for (index, expected) in HALL_PRICES.iter().enumerate() {
        let kind = format!("0x{:04x}", 0x736 + index);
        let object = doc
            .descendants()
            .find(|n| n.has_tag_name("Object") && n.attribute("type") == Some(kind.as_str()))
            .unwrap();
        assert_eq!(
            object
                .children()
                .find(|n| n.has_tag_name("Class"))
                .unwrap()
                .text(),
            Some("GuildMerchant")
        );
        let price = object
            .children()
            .find(|n| n.has_tag_name("Price"))
            .unwrap()
            .text()
            .unwrap()
            .parse::<i32>()
            .unwrap();
        assert_eq!(price, *expected);
    }
}

#[test]
fn guild_rank_matrix_matches_the_selected_client_including_founder_exception() {
    for authority in [0, 10, 20, 30, 40] {
        for old in [0, 10, 20, 30, 40] {
            for new in [-1, 0, 10, 20, 30, 40, 50] {
                let pairs: &[(i32, i32)] = match authority {
                    20 => &[(0, 10), (10, 0)],
                    30 => &[
                        (0, 10),
                        (0, 20),
                        (0, 30),
                        (10, 0),
                        (10, 20),
                        (10, 30),
                        (20, 0),
                        (20, 10),
                        (20, 30),
                    ],
                    40 => &[
                        (0, 10),
                        (0, 20),
                        (0, 30),
                        (10, 0),
                        (10, 20),
                        (10, 30),
                        (20, 0),
                        (20, 10),
                        (20, 30),
                        (30, 0),
                        (30, 10),
                        (30, 20),
                    ],
                    _ => &[],
                };
                assert_eq!(
                    allowed_rank(authority, old, new),
                    pairs.contains(&(old, new)),
                    "{authority}: {old} -> {new}"
                );
            }
        }
    }
}

#[tokio::test]
async fn guild_permissions_board_death_contributions_and_departure_are_durable() {
    let (c, s) = setup();
    let (a, an) = account(&s, &c, 0, 1000).await;
    let (b, bn) = account(&s, &c, 1, 20).await;
    let (outsider, _) = account(&s, &c, 2, 1000).await;
    let (no_star, _) = account(&s, &c, 3, 19).await;
    let (g, _) = s
        .change_guild(a, GuildAction::Create("First Guild".into()))
        .await
        .unwrap();
    s.change_guild(outsider, GuildAction::Create("Second Guild".into()))
        .await
        .unwrap();
    assert!(
        s.change_guild(
            b,
            GuildAction::Join {
                guild: g,
                inviter: outsider
            }
        )
        .await
        .is_err()
    );
    assert!(
        s.change_guild(
            no_star,
            GuildAction::Join {
                guild: g,
                inviter: a
            }
        )
        .await
        .is_err()
    );
    s.change_guild(
        b,
        GuildAction::Join {
            guild: g,
            inviter: a,
        },
    )
    .await
    .unwrap();
    assert_eq!(s.presentation(b).await.unwrap().guild.unwrap().rank, 0);
    assert!(
        s.change_guild(
            b,
            GuildAction::Rank {
                name: an.clone(),
                rank: 0
            }
        )
        .await
        .is_err()
    );
    assert!(
        s.change_guild(outsider, GuildAction::Remove(bn.clone()))
            .await
            .is_err()
    );
    assert!(
        s.guild_board(outsider, Some("secret overwrite".into()))
            .await
            .is_ok()
    );
    assert_eq!(s.guild_board(a, None).await.unwrap(), "");
    assert!(
        s.guild_board(b, Some("not authorized".into()))
            .await
            .is_err()
    );
    let text = "Welcome!\nBring potions & friends. <not markup>\n日本語 🧙";
    assert_eq!(s.guild_board(a, Some(text.into())).await.unwrap(), text);
    assert_eq!(s.guild_board(b, None).await.unwrap(), text);
    for rank in [10, 20, 30] {
        s.change_guild(
            a,
            GuildAction::Rank {
                name: bn.clone(),
                rank,
            },
        )
        .await
        .unwrap();
    }
    assert!(
        s.change_guild(
            b,
            GuildAction::Rank {
                name: bn.clone(),
                rank: 40
            }
        )
        .await
        .is_err()
    );
    assert!(
        s.change_guild(b, GuildAction::Remove(an.clone()))
            .await
            .is_err()
    );
    s.change_guild(
        a,
        GuildAction::Rank {
            name: bn.clone(),
            rank: 20,
        },
    )
    .await
    .unwrap();
    assert!(s.guild_board(b, Some("😀".repeat(2048))).await.is_ok());
    assert!(s.guild_board(b, Some("😀".repeat(2049))).await.is_err());
    assert!(s.guild_board(b, Some("bad\0text".into())).await.is_err());
    assert_eq!(s.guild(a).await.unwrap().fame, 0);
    let mut death = s.create(b, c.get(WIZARD).unwrap()).await.unwrap();
    death.fame = 237;
    death.hp = 0;
    death.alive = false;
    s.death(death.clone(), "guild test".into()).await.unwrap();
    assert!(s.death(death, "retry".into()).await.is_err());
    let guild = s.guild(a).await.unwrap();
    assert_eq!((guild.fame, guild.earned), (237, 237));
    assert!(
        s.guild_roster(a, 0, 50)
            .await
            .unwrap()
            .contains("<Fame>237</Fame>")
    );
    assert_eq!(s.presentation(b).await.unwrap().fame, 257);
    s.change_guild(a, GuildAction::Remove(an)).await.unwrap();
    assert!(s.guild(a).await.is_err());
    assert!(
        s.change_guild(
            b,
            GuildAction::Rank {
                name: bn.clone(),
                rank: 40
            }
        )
        .await
        .is_err()
    );
    s.change_guild(b, GuildAction::Remove(bn)).await.unwrap();
    assert!(s.guild(b).await.is_err());
    let (fresh, _) = account(&s, &c, 4, 1000).await;
    let (new, _) = s
        .change_guild(fresh, GuildAction::Create("First Guild".into()))
        .await
        .unwrap();
    assert!(new > g);
    assert_eq!(s.guild(fresh).await.unwrap().board, "");
    assert_eq!(s.guild(fresh).await.unwrap().fame, 0);
    assert!(s.guild_roster(a, 0, 50).await.is_err());
    assert!(s.guild_board(a, None).await.is_err());
}

#[tokio::test]
async fn guild_capacity_race_and_upgrades_cannot_overfill_or_double_spend() {
    let (c, s) = setup();
    let (a, _) = account(&s, &c, 0, 1000).await;
    let (g, _) = s
        .change_guild(a, GuildAction::Create("Capacity".into()))
        .await
        .unwrap();
    let mut candidates = Vec::new();
    for n in 1..=51 {
        let (id, _) = account(&s, &c, n, 20).await;
        if n < 49 {
            s.change_guild(
                id,
                GuildAction::Join {
                    guild: g,
                    inviter: a,
                },
            )
            .await
            .unwrap();
        } else {
            candidates.push(id);
        }
    }
    let (x, y) = tokio::join!(
        s.change_guild(
            candidates[0],
            GuildAction::Join {
                guild: g,
                inviter: a
            }
        ),
        s.change_guild(
            candidates[1],
            GuildAction::Join {
                guild: g,
                inviter: a
            }
        )
    );
    assert!(x.is_ok() && y.is_err() || x.is_err() && y.is_ok());
    assert!(
        s.change_guild(
            candidates[2],
            GuildAction::Join {
                guild: g,
                inviter: a
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        s.guild_roster(a, 0, 50)
            .await
            .unwrap()
            .matches("<Member>")
            .count(),
        50
    );
    let mut death = s.create(a, c.get(WIZARD).unwrap()).await.unwrap();
    death.fame = 360000;
    death.hp = 0;
    death.alive = false;
    s.death(death, "upgrade funding fixture".into())
        .await
        .unwrap();
    let member = if x.is_ok() {
        candidates[0]
    } else {
        candidates[1]
    };
    assert!(
        s.change_guild(member, GuildAction::Upgrade { level: 1 })
            .await
            .is_err()
    );
    for (level, remaining) in [(1, 350000), (2, 250000), (3, 0)] {
        s.change_guild(a, GuildAction::Upgrade { level })
            .await
            .unwrap();
        let guild = s.guild(a).await.unwrap();
        assert_eq!(
            (guild.level, guild.fame, guild.earned),
            (level + 1, remaining, 360000)
        );
        assert!(
            s.change_guild(a, GuildAction::Upgrade { level })
                .await
                .is_err()
        );
        assert_eq!(s.guild(a).await.unwrap().fame, remaining);
    }
    assert_eq!(s.presentation(a).await.unwrap().fame, 360000);
    for level in [i32::MIN, -1, 0, 4, i32::MAX] {
        assert!(
            s.change_guild(a, GuildAction::Upgrade { level })
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn guild_overflow_rolls_back_the_death_and_missing_current_state_fails_closed() {
    let (c, s) = setup();
    let (a, _) = account(&s, &c, 0, 1000).await;
    let (g, _) = s
        .change_guild(a, GuildAction::Create("Overflow".into()))
        .await
        .unwrap();
    s.call(move |db| {
        db.execute(
            "UPDATE guilds SET fame=2147483647,earned=2147483647 WHERE id=?1",
            [g],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let mut ch = s.create(a, c.get(WIZARD).unwrap()).await.unwrap();
    ch.fame = 1;
    ch.hp = 0;
    ch.alive = false;
    assert!(s.death(ch.clone(), "must roll back".into()).await.is_err());
    assert!(s.load(a, ch.id).await.unwrap().alive);
    assert_eq!(s.presentation(a).await.unwrap().fame, 0);
    assert_eq!(s.guild(a).await.unwrap().fame, i32::MAX);
    let path = std::env::temp_dir().join(format!(
        "guild-migration-{:032x}.db",
        rand::random::<u128>()
    ));
    drop(Store::open(&path, &c).unwrap());
    let db = Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE guild_members;").unwrap();
    assert!(
        Store::open(&path, &c)
            .err()
            .unwrap()
            .to_string()
            .contains("restore reviewed guild state")
    );
    db.execute_batch("DROP TABLE guilds; PRAGMA user_version=8;")
        .unwrap();
    let migrated = Store::open(&path, &c).unwrap();
    let old = migrated.list("legacy-no-guild".into()).await.unwrap();
    assert!(old.guild.is_none());
    let version: i32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 12);
    drop(migrated);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
