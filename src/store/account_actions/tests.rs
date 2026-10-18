use super::*;
fn fixture() -> (std::path::PathBuf, Content, Store) {
    let dir = std::env::temp_dir().join(format!("account-actions-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    (dir, content, store)
}
async fn linked(store: &Store, guid: &str, email: &str, discord: &str) -> i32 {
    let account = store.list(guid.into()).await.unwrap().id;
    store
        .register(guid.into(), email.into(), "old-password".into())
        .await
        .unwrap();
    let code = store.issue_discord_code(account).await.unwrap();
    store
        .redeem_discord_code(code, discord.into())
        .await
        .unwrap();
    store.compete(account).await.unwrap();
    account
}
#[tokio::test]
async fn recovery_requires_scoped_consent_revokes_replays_and_changes_only_owner() {
    let (dir, content, s) = fixture();
    let discord = "1558662335076376578";
    let other_discord = "1558662335076376579";
    let account = linked(&s, "owner", "owner@example.org", discord).await;
    let other = linked(&s, "other", "other@example.org", other_discord).await;
    assert!(s.issue_reset(discord.into(), None).await.is_err());
    let code = s.issue_recovery_code(account).await.unwrap();
    assert!(s.issue_recovery_code(account).await.is_err());
    assert!(
        s.issue_reset(other_discord.into(), Some(code.clone()))
            .await
            .is_err()
    );
    assert!(
        s.redeem_discord_code(code.clone(), discord.into())
            .await
            .is_err()
    );
    let token = s
        .issue_reset(discord.into(), Some(code.clone()))
        .await
        .unwrap();
    assert!(s.issue_reset(discord.into(), Some(code)).await.is_err());
    assert!(s.issue_reset(discord.into(), None).await.is_err());
    assert!(s.reset_account(token.clone()).await.unwrap() == account);
    let secret = token.clone();
    s.call(move |db| {
        let hash: String = db.query_row(
            "SELECT hash FROM password_resets WHERE account=?1",
            [account],
            |r| r.get(0),
        )?;
        ensure!(hash != secret, "reset token stored in plaintext");
        Ok(())
    })
    .await
    .unwrap();
    for password in ["1234", "with\ncontrol", &"a".repeat(129)] {
        assert!(s.prepare_password(password.into()).await.is_err());
    }
    let verifier = s.prepare_password("new-password".into()).await.unwrap();
    let (one, two) = tokio::join!(
        s.finish_reset(token.clone(), verifier.clone()),
        s.finish_reset(token.clone(), verifier)
    );
    assert!(one.is_ok() ^ two.is_ok());
    assert!(s.reset_account(token).await.is_err());
    assert!(
        s.authenticate("owner@example.org".into(), "old-password".into())
            .await
            .is_err()
    );
    assert!(
        s.authenticate("owner@example.org".into(), "new-password".into())
            .await
            .unwrap()
            == account
    );
    assert!(
        s.authenticate("other@example.org".into(), "old-password".into())
            .await
            .unwrap()
            == other
    );
    let restarted = Store::open(&dir.join("game.db"), &content).unwrap();
    assert!(
        restarted
            .authenticate("owner@example.org".into(), "new-password".into())
            .await
            .unwrap()
            == account
    );
    restarted.disable_recovery(account).await.unwrap();
    assert!(restarted.issue_reset(discord.into(), None).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn expiry_rotation_revocation_and_missing_credentials_fail_closed() {
    let (dir, _, s) = fixture();
    let discord = "1558662335076376578";
    let account = linked(&s, "owner", "owner@example.org", discord).await;
    let code = s.issue_recovery_code(account).await.unwrap();
    s.call(|db| {
        db.execute("UPDATE recovery_codes SET issued_at=0,expires_at=0", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.issue_reset(discord.into(), Some(code)).await.is_err());
    let code = s.issue_recovery_code(account).await.unwrap();
    let expired = s.issue_reset(discord.into(), Some(code)).await.unwrap();
    s.call(|db| {
        db.execute("UPDATE password_resets SET expires_at=0", [])?;
        db.execute("UPDATE discord_recovery SET last_issued_at=0", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.reset_account(expired).await.is_err());
    let old = s.issue_reset(discord.into(), None).await.unwrap();
    s.call(|db| {
        db.execute("UPDATE discord_recovery SET last_issued_at=0", [])?;
        Ok(())
    })
    .await
    .unwrap();
    let new = s.issue_reset(discord.into(), None).await.unwrap();
    assert!(s.reset_account(old).await.is_err());
    s.disable_recovery(account).await.unwrap();
    assert!(s.reset_account(new).await.is_err());
    let code = s.issue_recovery_code(account).await.unwrap();
    let token = s.issue_reset(discord.into(), Some(code)).await.unwrap();
    s.call(|db| {
        db.execute("DELETE FROM credentials", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.linked_account(discord.into()).await.is_err());
    assert!(s.reset_account(token).await.is_err());
    assert!(s.issue_reset(discord.into(), None).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn legacy_migration_credits_deaths_once_and_missing_wallets_never_rebuild_spending() {
    let (dir, content, s) = fixture();
    let account = linked(&s, "owner", "owner@example.org", "1558662335076376578").await;
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = 40;
    ch.alive = false;
    ch.hp = 0;
    s.death(ch, "test".into()).await.unwrap();
    s.call(|db| {
        db.execute_batch("DROP TABLE guild_members; DROP TABLE guilds; DROP TABLE account_fame;")?;
        db.pragma_update(None, "user_version", 0)?;
        Ok(())
    })
    .await
    .unwrap();
    let migrated = Store::open(&dir.join("game.db"), &content).unwrap();
    assert_eq!(
        migrated.hiscores(vec![]).await.unwrap()["players"][0]["total_fame"],
        40
    );
    migrated
        .call(move |db| {
            db.execute(
                "UPDATE account_fame SET balance=27 WHERE account=?1",
                [account],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let restarted = Store::open(&dir.join("game.db"), &content).unwrap();
    assert_eq!(
        restarted.hiscores(vec![]).await.unwrap()["players"][0]["total_fame"],
        27
    );
    restarted
        .call(move |db| {
            db.execute("DELETE FROM account_fame WHERE account=?1", [account])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        Store::open(&dir.join("game.db"), &content)
            .err()
            .unwrap()
            .to_string()
            .contains("wallet missing")
    );
    assert!(restarted.hiscores(vec![]).await.is_err());
    restarted
        .call(|db| {
            db.execute("DROP TABLE account_fame", [])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        Store::open(&dir.join("game.db"), &content)
            .err()
            .unwrap()
            .to_string()
            .contains("wallet table missing")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn death_credits_wallet_once_and_play_screen_and_hiscores_share_spendable_balance() {
    let (dir, content, s) = fixture();
    let account = linked(&s, "owner", "owner@example.org", "1558662335076376578").await;
    let mut ch = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = 40;
    ch.alive = false;
    ch.hp = 0;
    s.death(ch.clone(), "test".into()).await.unwrap();
    assert!(s.death(ch, "test".into()).await.is_err());
    let screen = s
        .characters("owner@example.org".into(), "old-password".into())
        .await
        .unwrap()
        .xml("", 0.0);
    assert!(screen.contains("<Stats><Fame>40</Fame><TotalFame>40</TotalFame>"));
    s.call(move |db| {
        db.execute(
            "UPDATE account_fame SET balance=balance-13 WHERE account=?1",
            [account],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let mut living = s
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    living.fame = 100;
    let scores = s.hiscores(vec![living]).await.unwrap();
    assert_eq!(scores["players"][0]["total_fame"], 27);
    assert_eq!(scores["players"][0]["base_fame"], 100);
    let restarted = Store::open(&dir.join("game.db"), &content).unwrap();
    let screen = restarted
        .characters("owner@example.org".into(), "old-password".into())
        .await
        .unwrap()
        .xml("", 0.0);
    assert!(screen.contains("<Stats><Fame>27</Fame><TotalFame>40</TotalFame>"));
    assert_eq!(
        restarted.hiscores(vec![]).await.unwrap()["players"][0]["total_fame"],
        27
    );
    std::fs::remove_dir_all(dir).unwrap();
}
