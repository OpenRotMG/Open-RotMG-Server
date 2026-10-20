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
            ensure!(registered, Rejected("register and sign in before linking Discord"));
            let linked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM discord_links WHERE account=?1)", [account], |r| r.get(0))?;
            ensure!(!linked, Rejected("this game account is already linked"));
            let previous: Option<i64> = tx.query_row("SELECT issued_at FROM discord_codes WHERE account=?1", [account], |r| r.get(0)).optional()?;
            ensure!(previous.is_none_or(|t| time-t >= 60), Rejected("wait one minute before requesting another link code"));
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
                // Tombstones prevent stale world snapshots from restoring dead or deleted scores.
                let mut q=tx.prepare("SELECT id,alive,stats FROM characters")?;
                for row in q.query_map([],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,bool>(1)?,r.get::<_,String>(2)?)))? {
                    let (id,alive,raw)=row?;
                    let mut c=decode_character(&raw,base)?; c.id=id; c.alive=alive;
                    characters.insert(id,c);
                }
            }
            for c in living {
                if characters.get(&c.id).is_some_and(|saved| !saved.alive) { continue; }
                characters.insert(c.id,c);
            }
            for c in characters.values().filter(|c| c.alive) {
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
