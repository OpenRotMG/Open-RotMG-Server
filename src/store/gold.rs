use super::*;

pub(crate) const SCHEMA: &str = "CREATE TABLE gold_rewards(account INTEGER NOT NULL REFERENCES accounts(id),reward TEXT NOT NULL CHECK(reward IN ('discord_link','collection_top_half')),awarded_at INTEGER NOT NULL CHECK(awarded_at>0),notified_at INTEGER CHECK(notified_at>0),PRIMARY KEY(account,reward));";
pub(crate) const DISCORD: &str = "discord_link";
const COLLECTION: &str = "collection_top_half";
pub(crate) const AMOUNT: i32 = 100;

pub(crate) struct Gold {
    pub balance: i32,
    pub rewards: Vec<String>,
}
impl Gold {
    pub fn message(&self) -> Result<String> {
        let reasons = self
            .rewards
            .iter()
            .map(|reward| match reward.as_str() {
                DISCORD => Ok("linking your Discord account"),
                COLLECTION => Ok("reaching the top 50% of the Collection Log hiscores"),
                _ => anyhow::bail!("unknown gold reward {reward}"),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(format!(
            "You received {} Gold for {}!",
            AMOUNT * i32::try_from(reasons.len())?,
            reasons.join(" and ")
        ))
    }
}

pub(super) fn grant(db: &Connection, account: i32, reward: &str) -> Result<bool> {
    if db.execute("INSERT INTO gold_rewards(account,reward,awarded_at) VALUES(?1,?2,unixepoch()) ON CONFLICT(account,reward) DO NOTHING", params![account,reward])? == 0 {
        return Ok(false);
    }
    ensure!(db.execute("UPDATE accounts SET credits=credits+?2 WHERE id=?1 AND credits BETWEEN 0 AND 2147483647-?2", params![account,AMOUNT])? == 1, "gold reward wallet missing, invalid or overflowing for account {account}");
    Ok(true)
}

pub(super) fn collection(db: &Connection, account: i32) -> Result<bool> {
    let eligible: bool = db.query_row("WITH scores AS (
SELECT a.id,count(l.item) AS score FROM accounts a JOIN competitors c ON c.account=a.id LEFT JOIN collections l ON l.account=a.id
WHERE NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=a.id) GROUP BY a.id
), ranked AS (SELECT id,score,rank() OVER(ORDER BY score DESC) AS place,count(*) OVER() AS players FROM scores)
SELECT EXISTS(SELECT 1 FROM ranked WHERE id=?1 AND score>0 AND place<=(players+1)/2)
AND EXISTS(SELECT 1 FROM credentials WHERE account=?1)", [account], |r| r.get(0))?;
    if eligible {
        grant(db, account, COLLECTION)
    } else {
        Ok(false)
    }
}

impl Store {
    pub(crate) async fn gold(&self, account: i32) -> Result<Gold> {
        self.call(move |db| {
            let tx = db.transaction()?;
            let balance: i32 = tx.query_row("SELECT credits FROM accounts WHERE id=?1", [account], |r| r.get(0))?;
            ensure!(balance >= 0, "negative gold balance for account {account}");
            let mut q = tx.prepare("SELECT reward FROM gold_rewards WHERE account=?1 AND notified_at IS NULL ORDER BY awarded_at,reward")?;
            let rewards = q.query_map([account], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            drop(q);
            tx.commit()?;
            Ok(Gold { balance, rewards })
        }).await
    }
    pub(crate) async fn gold_notified(&self, account: i32, rewards: Vec<String>) -> Result<()> {
        self.call(move |db| {
            let tx = db.transaction()?;
            for reward in rewards {
                ensure!(tx.execute("UPDATE gold_rewards SET notified_at=unixepoch() WHERE account=?1 AND reward=?2 AND notified_at IS NULL", params![account,reward])? == 1, "gold notification receipt changed for account {account}");
            }
            tx.commit()?;
            Ok(())
        }).await
    }
}

#[cfg(test)]
mod tests;
