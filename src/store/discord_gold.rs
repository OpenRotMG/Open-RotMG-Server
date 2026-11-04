use super::*;

pub(super) const SCHEMA: &str = "CREATE TABLE discord_gold_grants(request_id TEXT PRIMARY KEY NOT NULL,account INTEGER NOT NULL REFERENCES accounts(id),discord_id TEXT NOT NULL,actor_id TEXT NOT NULL,amount INTEGER NOT NULL CHECK(typeof(amount)='integer' AND amount BETWEEN 1 AND 2147483647));";

pub(super) fn check_schema(db: &Connection, version: i32) -> Result<()> {
    if version >= 19 {
        let sql: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='discord_gold_grants'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            sql.as_deref() == Some(SCHEMA.trim_end_matches(';')),
            "schema-19 Discord gold receipts missing or mismatched; restore reviewed state instead of allowing duplicate grants"
        );
    }
    Ok(())
}

pub(super) const REALM_CLOSE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS discord_realm_close_tokens(request_id TEXT PRIMARY KEY NOT NULL,account INTEGER NOT NULL REFERENCES accounts(id),discord_id TEXT NOT NULL,actor_id TEXT NOT NULL,used_request TEXT UNIQUE);";

fn linked(db: &Connection, discord: &str) -> Result<i32> {
    db.query_row(
        "SELECT l.account FROM discord_links l JOIN credentials c ON c.account=l.account JOIN accounts a ON a.id=l.account WHERE l.discord_id=?1 AND a.guid IS NOT NULL AND NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=l.account)",
        [discord], |r| r.get(0),
    ).optional()?.context("target user has no linked registered game account; they must use /link first")
}

impl Store {
    /// Returns the recipient's account and unused token count; a replayed request grants nothing new.
    pub async fn grant_realm_close_token(
        &self,
        discord: String,
        request: String,
        actor: String,
    ) -> Result<(i32, i64)> {
        for id in [&discord, &request, &actor] {
            hiscores::discord_id(id)?;
        }
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let account = linked(&tx, &discord)?;
            let inserted = tx.execute("INSERT INTO discord_realm_close_tokens(request_id,account,discord_id,actor_id) VALUES(?1,?2,?3,?4) ON CONFLICT(request_id) DO NOTHING", params![request, account, discord, actor])?;
            let same: bool = tx.query_row("SELECT account=?2 AND discord_id=?3 AND actor_id=?4 FROM discord_realm_close_tokens WHERE request_id=?1", params![request, account, discord, actor], |r| r.get(0))?;
            ensure!(same, "Realm Close grant request ID reused with different parameters");
            let available: i64 = tx.query_row("SELECT COUNT(*) FROM discord_realm_close_tokens WHERE account=?1 AND used_request IS NULL", [account], |r| r.get(0))?;
            tx.commit()?;
            tracing::info!(account, discord_id=%discord, actor_id=%actor, request_id=%request, replayed=inserted==0, available, "Discord Realm Close token grant confirmed");
            Ok((account, available))
        }).await
    }
    /// Marks one unused token as spent by `request` and returns the owning account.
    pub async fn spend_realm_close_token(&self, discord: String, request: String) -> Result<i32> {
        for id in [&discord, &request] {
            hiscores::discord_id(id)?;
        }
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let account = linked(&tx, &discord)?;
            let spent = tx.execute("UPDATE discord_realm_close_tokens SET used_request=?2 WHERE rowid=(SELECT MIN(rowid) FROM discord_realm_close_tokens WHERE account=?1 AND used_request IS NULL)", params![account, request])?;
            ensure!(spent == 1, crate::store::Rejected("You have no Realm Close tokens."));
            tx.commit()?;
            tracing::info!(account, discord_id=%discord, request_id=%request, "Discord Realm Close token spent");
            Ok(account)
        }).await
    }
    /// Returns a token whose closure could not start.
    pub async fn refund_realm_close_token(&self, request: String) -> Result<()> {
        self.call(move |db| {
            ensure!(
                db.execute(
                    "UPDATE discord_realm_close_tokens SET used_request=NULL WHERE used_request=?1",
                    [&request]
                )? == 1,
                "spent Realm Close token {request} not found for refund"
            );
            tracing::info!(request_id=%request, "Discord Realm Close token refunded");
            Ok(())
        })
        .await
    }
    pub async fn grant_discord_gold(
        &self,
        discord: String,
        amount: i32,
        request: String,
        actor: String,
    ) -> Result<(i32, i32)> {
        for id in [&discord, &request, &actor] {
            hiscores::discord_id(id)?;
        }
        ensure!(amount > 0, "gold amount must be positive");
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let previous: Option<(i32, String, String, i32)> = tx.query_row(
                "SELECT account,discord_id,actor_id,amount FROM discord_gold_grants WHERE request_id=?1",
                [&request], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            ).optional()?;
            let replayed = previous.is_some();
            let account = if let Some((account, target, author, value)) = previous {
                ensure!((target, author, value) == (discord.clone(), actor.clone(), amount), "gold request ID reused with different parameters");
                account
            } else {
                let account = linked(&tx, &discord)?;
                ensure!(tx.execute("UPDATE accounts SET credits=credits+?2 WHERE id=?1 AND credits BETWEEN 0 AND 2147483647-?2", params![account,amount])? == 1, "gold balance would exceed 2147483647 or wallet is invalid");
                tx.execute("INSERT INTO discord_gold_grants(request_id,account,discord_id,actor_id,amount) VALUES(?1,?2,?3,?4,?5)", params![request,account,discord,actor,amount])?;
                account
            };
            let balance: i32 = tx.query_row("SELECT credits FROM accounts WHERE id=?1", [account], |r| r.get(0))?;
            tx.commit()?;
            tracing::info!(account, discord_id=%discord, actor_id=%actor, request_id=%request, amount, balance, replayed, "Discord gold grant confirmed");
            Ok((account, balance))
        }).await
    }
}

#[cfg(test)]
mod tests;
