use super::*;

fn store() -> (Store, Content, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "openrotmg-presentation-{}-{}.db",
        std::process::id(),
        rand::random::<u64>()
    ));
    let content = Content::load(Path::new("data")).unwrap();
    (Store::open(&path, &content).unwrap(), content, path)
}

fn class_record(xml: &str, class: u16) -> (i32, i32) {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let kind = class.to_string();
    let record = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("ClassStats") && node.attribute("objectType") == Some(kind.as_str())
        })
        .unwrap();
    let value = |tag| {
        record
            .children()
            .find(|node| node.has_tag_name(tag))
            .unwrap()
            .text()
            .unwrap()
            .parse()
            .unwrap()
    };
    (value("BestLevel"), value("BestFame"))
}

#[test]
fn target_stars_and_next_quest_use_class_maxima_not_wallet() {
    for (fame, stars, next) in [
        (0, 0, 20),
        (19, 0, 20),
        (20, 1, 150),
        (149, 1, 150),
        (150, 2, 400),
        (399, 2, 400),
        (400, 3, 800),
        (799, 3, 800),
        (800, 4, 2000),
        (1999, 4, 2000),
        (2000, 5, -1),
        (i32::MAX, 5, -1),
    ] {
        let p = AccountPresentation {
            guild: None,
            credits: 100,
            fame: i32::MAX,
            best_fame: BTreeMap::from([(WIZARD, fame)]),
        };
        assert_eq!(p.stars(), stars, "fame {fame}");
        assert_eq!(p.next_quest(WIZARD, 0), next);
        assert_eq!(p.next_quest(WIZARD, fame), next);
        assert_eq!(p.next_quest(PRIEST, fame), next);
        assert_eq!(p.next_quest(PRIEST, 0), 20);
    }
    let p = AccountPresentation {
        guild: None,
        credits: 100,
        fame: 0,
        best_fame: BTreeMap::from([(WIZARD, 150), (PRIEST, 20)]),
    };
    assert_eq!(p.stars(), 3);
    assert_eq!(p.next_quest(WIZARD, 400), 800);
    assert_eq!(p.next_quest(PRIEST, 149), 150);
    let empty = AccountPresentation {
        guild: None,
        credits: 100,
        fame: 0,
        best_fame: BTreeMap::new(),
    };
    assert_eq!(empty.stars(), 0);
    assert_eq!(empty.next_quest(WIZARD, 0), 20);
}

#[tokio::test]
async fn presentation_and_xml_share_living_and_dead_class_bests_across_reopen() {
    let (s, content, path) = store();
    let account = s.list("class-history".into()).await.unwrap().id;
    for fame in [19, 20, 149, 150, 400, 800, 2000] {
        let mut ch = s
            .create(account, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.fame = fame;
        ch.level = 5;
        let wallet = s.presentation(account).await.unwrap().fame;
        s.progress(ch.clone()).await.unwrap();
        let before = s.presentation(account).await.unwrap();
        assert_eq!(before.fame, wallet);
        assert_eq!(before.best_fame[&WIZARD], fame);
        assert_eq!(
            class_record(
                &s.list("class-history".into()).await.unwrap().xml("", 0.0),
                WIZARD
            ),
            (5, fame)
        );
        ch.hp = 0;
        ch.alive = false;
        s.death(ch, "prepared threshold".into()).await.unwrap();
        let listed = s.list("class-history".into()).await.unwrap();
        assert_eq!(class_record(&listed.xml("", 0.0), WIZARD), (5, fame));
        let p = s.presentation(account).await.unwrap();
        assert_eq!(p.best_fame, listed.best_fame);
        assert_eq!(p.best_fame[&WIZARD], fame);
    }
    for (class, fame) in [(WIZARD, 1), (PRIEST, 150)] {
        let mut ch = s
            .create(account, content.get(class).unwrap())
            .await
            .unwrap();
        ch.fame = fame;
        ch.hp = 0;
        ch.alive = false;
        s.death(ch, "later death".into()).await.unwrap();
    }
    let mut replacement = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    replacement.fame = 3000;
    s.progress(replacement).await.unwrap();
    s.call(move |db| {
        db.execute(
            "UPDATE account_fame SET balance=27 WHERE account=?1",
            [account],
        )?;
        db.execute("UPDATE characters SET stats=json_set(stats,'$.fame',9000) WHERE account=?1 AND alive=0", [account])?;
        Ok(())
    })
    .await
    .unwrap();
    let expected = s.presentation(account).await.unwrap();
    assert_eq!(expected.fame, 27);
    assert_eq!(
        expected.best_fame,
        BTreeMap::from([(WIZARD, 3000), (PRIEST, 150)])
    );
    assert_eq!(expected.stars(), 7);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.presentation(account).await.unwrap(), expected);
    let restored_path = path.with_extension("restored.db");
    let destination = restored_path.to_str().unwrap().to_owned();
    s.call(move |db| {
        db.execute("VACUUM INTO ?1", [destination])?;
        Ok(())
    })
    .await
    .unwrap();
    let restored = Store::open(&restored_path, &content).unwrap();
    assert_eq!(restored.presentation(account).await.unwrap(), expected);
    let listed = reopened.list("class-history".into()).await.unwrap();
    assert_eq!(listed.fame, 27);
    assert_eq!(listed.earned_fame, 3689);
    assert_eq!(class_record(&listed.xml("", 0.0), WIZARD), (5, 3000));
    assert_eq!(class_record(&listed.xml("", 0.0), PRIEST), (1, 150));
    assert!(content.unlocked(PRIEST, &listed.best_levels).unwrap());
    let other = reopened.list("unrelated".into()).await.unwrap();
    assert_eq!(
        reopened.presentation(other.id).await.unwrap(),
        AccountPresentation {
            guild: None,
            credits: 100,
            fame: 0,
            best_fame: BTreeMap::new()
        }
    );
}

#[tokio::test]
async fn living_class_star_requires_a_successful_progress_commit_not_a_death() {
    let (s, content, _) = store();
    let account = s.list("living-star".into()).await.unwrap().id;
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert_eq!(s.presentation(account).await.unwrap().stars(), 0);
    ch.level = 20;
    ch.exp = 19_999;
    ch.fame = 19;
    s.progress(ch.clone()).await.unwrap();
    s.call(|db| {
        db.execute_batch("CREATE TRIGGER fail_progress BEFORE UPDATE OF stats ON characters BEGIN SELECT RAISE(ABORT,'injected progress failure'); END;")?;
        Ok(())
    }).await.unwrap();
    ch.exp += 1;
    ch.fame += 1;
    assert!(s.progress(ch.clone()).await.is_err());
    let before = s.presentation(account).await.unwrap();
    assert_eq!(
        (before.best_fame[&WIZARD], before.stars(), before.fame),
        (19, 0, 0)
    );
    s.call(|db| {
        db.execute_batch("DROP TRIGGER fail_progress")?;
        Ok(())
    })
    .await
    .unwrap();
    s.progress(ch.clone()).await.unwrap();
    let after = s.presentation(account).await.unwrap();
    assert_eq!(
        (after.best_fame[&WIZARD], after.stars(), after.fame),
        (20, 1, 0)
    );
    assert!(s.load(account, ch.id).await.unwrap().alive);
    assert!(s.fame(account, ch.id).await.is_err());
}

#[tokio::test]
async fn presentation_decodes_legacy_committed_snapshots_without_birth_metadata() {
    let (s, content, _) = store();
    let account = s.list("legacy-history".into()).await.unwrap().id;
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = 40;
    ch.hp = 0;
    ch.alive = false;
    s.death(ch.clone(), "fixture".into()).await.unwrap();
    let mut raw = serde_json::to_value(ch).unwrap();
    for key in ["schema", "class", "base", "created_at"] {
        raw.as_object_mut().unwrap().remove(key);
    }
    let raw = serde_json::to_string(&raw).unwrap();
    s.call(move |db| {
        db.execute("UPDATE deaths SET snapshot=?1", [raw])?;
        Ok(())
    })
    .await
    .unwrap();
    let p = s.presentation(account).await.unwrap();
    assert_eq!(p.fame, 40);
    assert_eq!(p.best_fame, BTreeMap::from([(WIZARD, 40)]));
    assert_eq!(p.stars(), 1);
    assert_eq!(
        class_record(
            &s.list("legacy-history".into()).await.unwrap().xml("", 0.0),
            WIZARD
        ),
        (1, 40)
    );
}

#[tokio::test]
async fn presentation_and_list_reject_corrupt_committed_snapshots() {
    let (s, content, _) = store();
    let account = s.list("corrupt-history".into()).await.unwrap().id;
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = 40;
    ch.hp = 0;
    ch.alive = false;
    s.death(ch.clone(), "fixture".into()).await.unwrap();
    let original = serde_json::to_value(&ch).unwrap();
    let mut invalid = vec!["{".to_owned()];
    for (key, value) in [
        ("fame", (-1).into()),
        ("fame", (i64::from(i32::MAX) + 1).into()),
        ("schema", 2.into()),
        ("account", (account + 1).into()),
        ("id", (ch.id + 1).into()),
        ("alive", true.into()),
        ("hp", 1.into()),
        ("class", 65535.into()),
        ("level", 0.into()),
        ("level", 21.into()),
        ("items", serde_json::json!([])),
        ("base", serde_json::to_value([-1; 8]).unwrap()),
    ] {
        let mut bad = original.clone();
        bad[key] = value;
        invalid.push(serde_json::to_string(&bad).unwrap());
    }
    for raw in invalid {
        s.call(move |db| {
            db.execute("UPDATE deaths SET snapshot=?1", [raw])?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(s.presentation(account).await.is_err());
        assert!(s.list("corrupt-history".into()).await.is_err());
    }
}

#[tokio::test]
async fn presentation_and_list_reject_missing_invalid_and_out_of_range_wallets() {
    let (s, _, _) = store();
    let account = s.list("invalid-wallet".into()).await.unwrap().id;
    for (balance, earned) in [
        (-1, 0),
        (20, 19),
        (i64::from(i32::MAX) + 1, i64::from(i32::MAX) + 1),
        (0, i64::from(i32::MAX) + 1),
    ] {
        s.call(move |db| {
            db.execute_batch("PRAGMA ignore_check_constraints=ON")?;
            db.execute(
                "UPDATE account_fame SET balance=?1,earned=?2 WHERE account=?3",
                params![balance, earned, account],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(s.presentation(account).await.is_err());
        assert!(s.list("invalid-wallet".into()).await.is_err());
    }
    s.call(move |db| {
        db.execute("DELETE FROM account_fame WHERE account=?1", [account])?;
        Ok(())
    })
    .await
    .unwrap();
    let error = s.presentation(account).await.unwrap_err();
    assert_eq!(error.to_string(), "read account fame wallet");
    assert!(s.list("invalid-wallet".into()).await.is_err());
    assert!(s.presentation(account + 100).await.is_err());
}
