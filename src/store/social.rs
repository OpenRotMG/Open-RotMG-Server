use super::*;
use std::collections::BTreeSet;

pub type AccountLists = [BTreeSet<i32>; 2];

fn lists(db: &Connection, account: i32) -> Result<AccountLists> {
    let mut lists = AccountLists::default();
    let mut query = db.prepare("SELECT list_id,target FROM account_lists WHERE account=?1")?;
    for row in query.query_map([account], |r| Ok((r.get::<_, usize>(0)?, r.get(1)?)))? {
        let (list, target) = row?;
        lists
            .get_mut(list)
            .context("invalid account list")?
            .insert(target);
    }
    ensure!(
        lists.iter().all(|l| l.len() <= i16::MAX as usize),
        "account list too long"
    );
    Ok(lists)
}

#[cfg(test)]
#[tokio::test]
async fn locks_bind_accounts_across_death_replacement_and_store_reopen() {
    let content = Content::load(Path::new("data")).unwrap();
    let dir = std::env::temp_dir().join(format!("account-locks-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let owner = store.list("lock-owner".into()).await.unwrap().id;
    let peer = store.list("lock-peer".into()).await.unwrap().id;
    let class = content.get(0x30e).unwrap();
    let mut old = Vec::new();
    for account in [owner, peer] {
        old.push(store.create(account, class).await.unwrap());
    }
    let expected = [BTreeSet::from([peer]), BTreeSet::new()];
    assert_eq!(
        store.edit_account_list(owner, 0, true, peer).await.unwrap(),
        expected
    );
    assert_eq!(
        store.edit_account_list(owner, 0, true, peer).await.unwrap(),
        expected
    );
    assert_eq!(
        store.account_lists(peer).await.unwrap(),
        AccountLists::default()
    );
    for (list, target) in [(0, owner), (2, peer), (0, i32::MAX)] {
        assert!(
            store
                .edit_account_list(owner, list, true, target)
                .await
                .is_err()
        );
        assert_eq!(store.account_lists(owner).await.unwrap(), expected);
    }
    for mut ch in old.clone() {
        ch.hp = 0;
        ch.alive = false;
        store
            .death(ch, "prepared lock retention test".into())
            .await
            .unwrap();
    }
    for ch in &old {
        let new = store.create(ch.account, class).await.unwrap();
        assert_ne!(new.id, ch.id);
        assert_eq!(new.account, ch.account);
    }
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(store.account_lists(owner).await.unwrap(), expected);
    assert_eq!(
        store.account_lists(peer).await.unwrap(),
        AccountLists::default()
    );
    assert_eq!(
        store
            .edit_account_list(owner, 0, false, peer)
            .await
            .unwrap(),
        AccountLists::default()
    );
    assert_eq!(
        store
            .edit_account_list(owner, 0, false, peer)
            .await
            .unwrap(),
        AccountLists::default()
    );
    drop(store);
    assert_eq!(
        Store::open(&path, &content)
            .unwrap()
            .account_lists(owner)
            .await
            .unwrap(),
        AccountLists::default()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

impl Store {
    pub async fn account_named(&self, name: String) -> Result<i32> {
        self.call(move |db| {
            accounts::named_account(db, &name)?
                .ok_or_else(|| anyhow::anyhow!(Rejected("Player not found.")))
        })
        .await
    }

    pub async fn account_lists(&self, account: i32) -> Result<AccountLists> {
        self.call(move |db| lists(db, account)).await
    }

    pub async fn edit_account_list(
        &self,
        account: i32,
        list: i32,
        add: bool,
        target: i32,
    ) -> Result<AccountLists> {
        ensure!(
            (0..=1).contains(&list) && account != target,
            Rejected("invalid account list edit")
        );
        self.call(move |db| {
            let tx = db.transaction()?;
            if add {
                tx.execute(
                    "INSERT OR IGNORE INTO account_lists(account,list_id,target) VALUES(?1,?2,?3)",
                    params![account, list, target],
                )?;
            } else {
                tx.execute(
                    "DELETE FROM account_lists WHERE account=?1 AND list_id=?2 AND target=?3",
                    params![account, list, target],
                )?;
            }
            let lists = lists(&tx, account)?;
            tx.commit()?;
            Ok(lists)
        })
        .await
    }
}
