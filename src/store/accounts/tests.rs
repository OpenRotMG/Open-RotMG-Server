use super::*;

#[tokio::test]
async fn name_choice_refusals_preserve_identity_and_gold_across_reopen() {
    let content = Content::load(Path::new("data")).unwrap();
    let root = std::env::temp_dir().join(format!("name-choice-{}", rand::random::<u64>()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let guest = store.list("name-guest".into()).await.unwrap().id;
    assert!(matches!(
        store
            .claim_name_choice(guest, "GuestName".into(), true, vec![])
            .await
            .unwrap(),
        NameChoice::Rejected("register before choosing a name")
    ));
    let account = store
        .register(
            "name-guest".into(),
            "name@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    assert_eq!(account, guest);
    for name in ["", "Ab", "Abcdefghijk", "Abc1", "Ab c", "Abé"] {
        assert!(matches!(
            store
                .claim_name_choice(account, name.into(), false, vec![])
                .await
                .unwrap(),
            NameChoice::Rejected("names must be 3–10 letters")
        ));
    }
    assert!(matches!(
        store
            .claim_name_choice(account, "FirstName".into(), false, vec![])
            .await
            .unwrap(),
        NameChoice::Committed { balance: 100 }
    ));
    let other = store
        .register(
            "other-name".into(),
            "other-name@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    store.choose_name(other, "TakenName".into()).await.unwrap();
    for (name, paid, reason) in [
        (
            "SecondName",
            false,
            "Paid name changes must be made in game.",
        ),
        ("FirstName", true, "name already taken"),
        ("firstname", true, "name already taken"),
        ("takenname", true, "name already taken"),
        ("SecondName", true, "Not enough Gold."),
    ] {
        assert!(
            matches!(store.claim_name_choice(account, name.into(), paid, vec![]).await.unwrap(), NameChoice::Rejected(actual) if actual == reason)
        );
    }
    let refused = store.list_id(account).await.unwrap();
    assert_eq!((&*refused.name, refused.credits), ("FirstName", 100));
    store.call(move |db| {
        db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [account])?;
        db.execute_batch("CREATE TRIGGER refuse_name BEFORE UPDATE OF name ON accounts WHEN NEW.name='SecondName' BEGIN SELECT RAISE(ABORT,'prepared storage refusal'); END;")?;
        Ok(())
    }).await.unwrap();
    assert!(
        store
            .claim_name_choice(account, "SecondName".into(), true, vec![])
            .await
            .is_err()
    );
    let unchanged = store.list_id(account).await.unwrap();
    assert_eq!((&*unchanged.name, unchanged.credits), ("FirstName", 1000));
    store
        .call(|db| {
            db.execute_batch("DROP TRIGGER refuse_name")?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .claim_name_choice(account, "SecondName".into(), true, vec![])
            .await
            .unwrap(),
        NameChoice::Committed { balance: 0 }
    ));
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    let saved = reopened.list_id(account).await.unwrap();
    assert_eq!((&*saved.name, saved.credits), ("SecondName", 0));
    assert!(matches!(
        reopened
            .claim_name_choice(account, "ThirdName".into(), true, vec![])
            .await
            .unwrap(),
        NameChoice::Rejected("Not enough Gold.")
    ));
    assert_eq!(reopened.list_id(other).await.unwrap().name, "TakenName");
}
