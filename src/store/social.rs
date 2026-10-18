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

impl Store {
    pub async fn account_named(&self, name: String) -> Result<i32> {
        self.call(move |db| {
            db.query_row(
                "SELECT id FROM accounts WHERE name=?1 COLLATE NOCASE",
                [name],
                |r| r.get(0),
            )
            .optional()?
            .context("Player not found.")
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
            "invalid account list edit"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_are_idempotent_isolated_and_survive_restart_and_legacy_migration() {
        let path =
            std::env::temp_dir().join(format!("openrotmg-social-{}.db", rand::random::<u64>()));
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(&path, &content).unwrap();
        let a = s.list("social-a".into()).await.unwrap().id;
        let b = s.list("social-b".into()).await.unwrap().id;
        for _ in 0..2 {
            s.edit_account_list(a, 0, true, b).await.unwrap();
            s.edit_account_list(a, 1, true, b).await.unwrap();
        }
        assert_eq!(
            s.account_lists(a).await.unwrap(),
            [BTreeSet::from([b]), BTreeSet::from([b])]
        );
        assert_eq!(s.account_lists(b).await.unwrap(), AccountLists::default());
        assert_eq!(s.account_named(format!("gUeSt{b}")).await.unwrap(), b);
        assert!(s.account_named("Missing".into()).await.is_err());
        for (list, target) in [(2, b), (0, a), (1, 999999)] {
            assert!(s.edit_account_list(a, list, true, target).await.is_err());
        }
        drop(s);
        let s = Store::open(&path, &content).unwrap();
        assert_eq!(s.account_lists(a).await.unwrap()[1], BTreeSet::from([b]));
        for _ in 0..2 {
            s.edit_account_list(a, 0, false, b).await.unwrap();
        }
        assert_eq!(
            s.account_lists(a).await.unwrap(),
            [BTreeSet::new(), BTreeSet::from([b])]
        );
        drop(s);
        let db = Connection::open(&path).unwrap();
        db.execute("DROP TABLE account_lists", []).unwrap();
        assert!(Store::open(&path, &content).is_err());
        db.execute_batch("DROP TABLE guild_members; DROP TABLE guilds;")
            .unwrap();
        crate::store::vaults::legacy_fixture(&db).unwrap();
        db.pragma_update(None, "user_version", 5).unwrap();
        assert_eq!(
            Store::open(&path, &content)
                .unwrap()
                .account_lists(a)
                .await
                .unwrap(),
            AccountLists::default()
        );
        drop(db);
        let _ = std::fs::remove_file(path);
    }
}
