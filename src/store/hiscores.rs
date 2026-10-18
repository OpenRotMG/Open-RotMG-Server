use super::*;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

pub(super) fn now() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?)
}
pub(super) fn discord_id(id: &str) -> Result<()> {
    ensure!(
        (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()),
        "invalid Discord identity"
    );
    Ok(())
}
pub(super) fn code_hash(code: &str) -> Result<String> {
    ensure!(
        code.len() == 32 && code.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid or expired link code"
    );
    Ok(format!("{:x}", Sha256::digest(code.as_bytes())))
}
impl Store {
    pub async fn compete(&self, account: i32) -> Result<()> {
        self.call(move |db| {
            db.execute("INSERT OR IGNORE INTO competitors(account) SELECT account FROM credentials WHERE account=?1", [account])?;
            Ok(())
        }).await
    }
    pub async fn issue_discord_code(&self, account: i32) -> Result<String> {
        let time = now()?;
        let code = format!("{:032x}", rand::random::<u128>());
        let hash = code_hash(&code)?;
        self.call(move |db| {
            let tx = db.transaction()?;
            let registered: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM credentials WHERE account=?1)", [account], |r| r.get(0))?;
            ensure!(registered, "register and sign in before linking Discord");
            let linked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM discord_links WHERE account=?1)", [account], |r| r.get(0))?;
            ensure!(!linked, "this game account is already linked");
            let previous: Option<i64> = tx.query_row("SELECT issued_at FROM discord_codes WHERE account=?1", [account], |r| r.get(0)).optional()?;
            ensure!(previous.is_none_or(|t| time-t >= 60), "wait one minute before requesting another link code");
            tx.execute("DELETE FROM discord_codes WHERE expires_at<=?1", [time])?;
            tx.execute("INSERT INTO discord_codes(account,hash,issued_at,expires_at) VALUES(?1,?2,?3,?4) ON CONFLICT(account) DO UPDATE SET hash=excluded.hash,issued_at=excluded.issued_at,expires_at=excluded.expires_at", params![account,hash,time,time+300])?;
            tx.commit()?;
            Ok(())
        }).await?;
        tracing::info!(account, "private Discord link code issued");
        Ok(code)
    }
    pub async fn redeem_discord_code(
        &self,
        code: String,
        discord: String,
    ) -> Result<(i32, String)> {
        discord_id(&discord)?;
        let hash = code_hash(&code)?;
        let time = now()?;
        self.call(move |db| {
            let tx = db.transaction()?;
            let account: Option<i32> = tx.query_row("SELECT account FROM discord_codes WHERE hash=?1 AND expires_at>?2", params![hash,time], |r| r.get(0)).optional()?;
            let account = account.context("invalid or expired link code")?;
            let name: String = tx.query_row("SELECT name FROM accounts JOIN credentials ON credentials.account=accounts.id WHERE id=?1", [account], |r| r.get(0))?;
            let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM discord_links WHERE account=?1 OR discord_id=?2)", params![account,discord], |r| r.get(0))?;
            ensure!(!conflict, "a game or Discord account is already linked; links are never silently replaced");
            tx.execute("INSERT INTO discord_links(account,discord_id) VALUES(?1,?2)", params![account,discord])?;
            tx.execute("DELETE FROM discord_codes WHERE account=?1", [account])?;
            let rewarded = gold::grant(&tx, account, gold::DISCORD)?;
            tx.commit()?;
            tracing::info!(account, rewarded, reward=gold::DISCORD, "Discord account linked");
            Ok((account, name))
        }).await
    }
    pub async fn hiscores(&self, living: Vec<Character>) -> Result<serde_json::Value> {
        let base = self.legacy_base;
        self.call(move |db| {
            let tx = db.transaction()?;
            let mut records = std::collections::BTreeMap::<i32, serde_json::Value>::new();
            {
                let mut q=tx.prepare("SELECT a.id,a.name,l.discord_id,f.balance FROM accounts a JOIN competitors c ON c.account=a.id LEFT JOIN account_fame f ON f.account=a.id LEFT JOIN discord_links l ON l.account=a.id WHERE NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=a.id) ORDER BY a.id")?;
                for row in q.query_map([], |r| Ok((r.get::<_,i32>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,i64>(3)?)))? {
                    let (id,name,discord,balance)=row?;
                    ensure!(balance >= 0, "negative account fame");
                    records.insert(id,serde_json::json!({"account":id,"name":name,"discord_id":discord,"base_fame":0,"total_fame":balance,"collection":0}));
                }
            }
            let mut characters = std::collections::BTreeMap::new();
            {
                let mut q=tx.prepare("SELECT stats FROM characters WHERE alive=1 UNION ALL SELECT snapshot FROM deaths")?;
                for raw in q.query_map([],|r|r.get::<_,String>(0))? {
                    let c=decode_character(&raw?,base)?; characters.insert(c.id,c);
                }
            }
            for c in living {
                if characters.get(&c.id).is_some_and(|saved| !saved.alive) { continue; }
                characters.insert(c.id,c);
            }
            for c in characters.values() {
                if let Some(row)=records.get_mut(&c.account) {
                    ensure!(c.fame >= 0, "negative persisted fame for character {}", c.id);
                    let best=row["base_fame"].as_i64().context("invalid best fame")?;
                    if i64::from(c.fame)>best {row["base_fame"]=c.fame.into();}
                }
            }
            {
                let mut q=tx.prepare("SELECT account,count(*) FROM collections GROUP BY account")?;
                for row in q.query_map([],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,i64>(1)?)))? {
                    let (id,count)=row?;if let Some(row)=records.get_mut(&id) {row["collection"]=count.into();}
                }
            }
            tx.commit()?;
            Ok(serde_json::json!({"updated_at":now()?,"players":records.into_values().collect::<Vec<_>>()}))
        }).await
    }
    pub async fn collection(&self, account: i32) -> Result<serde_json::Value> {
        self.call(move |db| {
            let tx=db.transaction()?;
            let name:String=tx.query_row("SELECT name FROM accounts JOIN competitors ON competitors.account=accounts.id WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM shore_bots WHERE shore_bots.account=accounts.id)",[account],|r|r.get(0)).context("player has no competitive profile")?;
            let mut q=tx.prepare("SELECT item,found_at FROM collections WHERE account=?1 ORDER BY item")?;
            let items=q.query_map([account],|r|Ok(serde_json::json!({"item":r.get::<_,i32>(0)?,"found_at":r.get::<_,i64>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            drop(q);tx.commit()?;
            Ok(serde_json::json!({"account":account,"name":name,"items":items}))
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn private_registered_codes_expire_consume_once_and_never_replace_links() {
        let content = Content::load(Path::new("data")).unwrap();
        let dir = std::env::temp_dir().join(format!("hiscores-link-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let s = Store::open(&dir.join("game.db"), &content).unwrap();
        let guest = s.list("owned-guest".into()).await.unwrap();
        assert!(s.issue_discord_code(guest.id).await.is_err());
        s.register(
            "owned-guest".into(),
            "owner@example.org".into(),
            "password123".into(),
        )
        .await
        .unwrap();
        s.compete(guest.id).await.unwrap();
        let code = s.issue_discord_code(guest.id).await.unwrap();
        assert!(s.issue_discord_code(guest.id).await.is_err());
        assert!(
            s.redeem_discord_code("wrong".into(), "1558662335076376578".into())
                .await
                .is_err()
        );
        assert!(
            s.redeem_discord_code(code.clone(), "not-an-id".into())
                .await
                .is_err()
        );
        assert_eq!(
            s.redeem_discord_code(code.clone(), "1558662335076376578".into())
                .await
                .unwrap()
                .1,
            guest.name
        );
        assert!(
            s.redeem_discord_code(code, "1558662335076376579".into())
                .await
                .is_err()
        );
        assert!(s.issue_discord_code(guest.id).await.is_err());
        let scores = s.hiscores(vec![]).await.unwrap();
        assert_eq!(scores["players"][0]["discord_id"], "1558662335076376578");
        assert_eq!(
            s.collection(guest.id).await.unwrap()["items"],
            serde_json::json!([])
        );
        let other = s.list("other-owned-guest".into()).await.unwrap();
        s.register(
            "other-owned-guest".into(),
            "other@example.org".into(),
            "password123".into(),
        )
        .await
        .unwrap();
        let expired = s.issue_discord_code(other.id).await.unwrap();
        s.call(|db| {
            db.execute("UPDATE discord_codes SET expires_at=0,issued_at=0", [])?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(
            s.redeem_discord_code(expired, "1558662335076376579".into())
                .await
                .is_err()
        );
        let fresh = s.issue_discord_code(other.id).await.unwrap();
        assert!(
            s.redeem_discord_code(fresh.clone(), "1558662335076376578".into())
                .await
                .is_err()
        );
        s.redeem_discord_code(fresh, "1558662335076376579".into())
            .await
            .unwrap();
        let restarted = Store::open(&dir.join("game.db"), &content).unwrap();
        assert_eq!(
            restarted.hiscores(vec![]).await.unwrap()["players"][0]["discord_id"],
            "1558662335076376578"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn collection_and_inventory_commit_together_and_fame_deduplicates_deaths() {
        let content = Content::load(Path::new("data")).unwrap();
        let dir = std::env::temp_dir().join(format!("hiscores-progress-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let s = Store::open(&dir.join("game.db"), &content).unwrap();
        let a = s.list("score-owner".into()).await.unwrap();
        s.compete(a.id).await.unwrap();
        assert_eq!(
            s.hiscores(vec![]).await.unwrap()["players"],
            serde_json::json!([])
        );
        s.register(
            "score-owner".into(),
            "scores@example.org".into(),
            "password123".into(),
        )
        .await
        .unwrap();
        s.compete(a.id).await.unwrap();
        let mut c = s.create(a.id, content.get(WIZARD).unwrap()).await.unwrap();
        let write = |c: &Character| InventoryWrite {
            account: c.account,
            id: c.id,
            revision: c.revision,
            items: c.items.clone(),
        };
        assert_eq!(
            s.collection(a.id).await.unwrap()["items"],
            serde_json::json!([])
        );
        c.items[4] = 0xa22;
        let revision = s
            .exchange_collected(vec![write(&c)], Vec::new(), Some(0xa22))
            .await
            .unwrap()[0];
        assert!(
            s.exchange_collected(vec![write(&c)], Vec::new(), Some(0xa23))
                .await
                .is_err()
        );
        assert_eq!(
            s.collection(a.id).await.unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        c.revision = revision;
        c.revision = s
            .exchange_collected(vec![write(&c)], Vec::new(), Some(0xa22))
            .await
            .unwrap()[0];
        assert_eq!(
            s.collection(a.id).await.unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut live = c.clone();
        live.fame = 100;
        let mut invalid = live.clone();
        invalid.fame = -1;
        assert!(s.hiscores(vec![invalid]).await.is_err());
        let scores = s.hiscores(vec![live.clone()]).await.unwrap();
        assert_eq!(scores["players"][0]["base_fame"], 100);
        assert_eq!(scores["players"][0]["total_fame"], 0);
        c.fame = 50;
        c.alive = false;
        c.hp = 0;
        s.death(c, "test enemy".into()).await.unwrap();
        let scores = s.hiscores(vec![live]).await.unwrap();
        assert_eq!(scores["players"][0]["base_fame"], 50);
        assert_eq!(scores["players"][0]["total_fame"], 50);
        let mut replacement = s.create(a.id, content.get(WIZARD).unwrap()).await.unwrap();
        replacement.fame = 80;
        let scores = s.hiscores(vec![replacement]).await.unwrap();
        assert_eq!(scores["players"][0]["base_fame"], 80);
        assert_eq!(scores["players"][0]["total_fame"], 50);
        assert_eq!(scores["players"][0]["collection"], 1);
        s.call(move |db| {
            db.execute(
                "INSERT INTO shore_bots(name,account) VALUES('test-bot',?1)",
                [a.id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            s.hiscores(vec![]).await.unwrap()["players"],
            serde_json::json!([])
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
