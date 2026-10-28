use super::*;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub(super) const RESET_SQL: &str = "CREATE TABLE email_password_resets(account INTEGER PRIMARY KEY REFERENCES credentials(account),email TEXT NOT NULL,hash TEXT NOT NULL UNIQUE,issued_at INTEGER NOT NULL,expires_at INTEGER NOT NULL)";
pub(super) const SCHEMA: [(&str, &str); 3] = [
    (
        "verified_emails",
        "CREATE TABLE verified_emails(account INTEGER PRIMARY KEY REFERENCES credentials(account),email TEXT NOT NULL,verified_at INTEGER NOT NULL CHECK(verified_at>0))",
    ),
    (
        "email_verifications",
        "CREATE TABLE email_verifications(account INTEGER PRIMARY KEY REFERENCES credentials(account),email TEXT NOT NULL,hash TEXT NOT NULL UNIQUE,issued_at INTEGER NOT NULL,expires_at INTEGER NOT NULL)",
    ),
    (
        "email_send_attempts",
        "CREATE TABLE email_send_attempts(account INTEGER NOT NULL REFERENCES accounts(id),ip_hash TEXT NOT NULL,issued_at INTEGER NOT NULL,PRIMARY KEY(account,issued_at))",
    ),
];

pub(super) fn migrate(db: &Connection, version: i32) -> Result<()> {
    for (name, sql) in SCHEMA {
        if version < 20 {
            db.execute_batch(sql)?;
        } else {
            let actual: Option<String> = db
                .query_row("SELECT sql FROM sqlite_schema WHERE name=?1", [name], |r| {
                    r.get(0)
                })
                .optional()?;
            ensure!(
                actual.as_deref() == Some(sql),
                "schema-20 email table missing or mismatched: {name}; restore reviewed authentication state"
            );
        }
    }
    if version < 21 {
        db.execute_batch(RESET_SQL)?;
    } else {
        let actual: Option<String> = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='email_password_resets'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            actual.as_deref() == Some(RESET_SQL),
            "schema-21 email reset table missing or mismatched; restore reviewed authentication state"
        );
    }
    Ok(())
}
fn reserve(
    tx: &rusqlite::Transaction<'_>,
    account: i32,
    ip: std::net::IpAddr,
    time: i64,
) -> Result<()> {
    let ip_hash = format!("{:x}", Sha256::digest(ip.to_string().as_bytes()));
    tx.execute(
        "DELETE FROM email_send_attempts WHERE issued_at<=?1",
        [time - 31 * 86400],
    )?;
    let (recent, daily): (i64, i64) = tx.query_row("SELECT count(CASE WHEN issued_at>?2-60 THEN 1 END),count(CASE WHEN issued_at>?2-86400 THEN 1 END) FROM email_send_attempts WHERE account=?1", params![account,time], |r| Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(recent == 0, Rejected("Wait one minute before resending."));
    ensure!(daily < 5, Rejected("Five emails today; retry tomorrow."));
    let ip_count: i64 = tx.query_row(
        "SELECT count(*) FROM email_send_attempts WHERE ip_hash=?1 AND issued_at>?2",
        params![ip_hash, time - 3600],
        |r| r.get(0),
    )?;
    ensure!(
        ip_count < 10,
        Rejected("Connection limit; retry in one hour.")
    );
    let (daily, monthly): (i64, i64) = tx.query_row(
        "SELECT count(CASE WHEN issued_at>?1-86400 THEN 1 END),count(*) FROM email_send_attempts",
        [time],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        daily < 90 && monthly < 2500,
        Rejected("Mail allowance exhausted; retry later.")
    );
    tx.execute(
        "INSERT INTO email_send_attempts(account,ip_hash,issued_at) VALUES(?1,?2,?3)",
        params![account, ip_hash, time],
    )?;
    Ok(())
}
fn token_hash(token: &str) -> Result<String> {
    ensure!(
        token.len() == 64
            && token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid or expired verification link"
    );
    Ok(format!("{:x}", Sha256::digest(token.as_bytes())))
}
impl Store {
    pub async fn email_verified(&self, account: i32) -> Result<bool> {
        self.call(move |db| Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM verified_emails v JOIN credentials c ON c.account=v.account AND c.email=v.email WHERE v.account=?1)", [account], |r| r.get(0))?)).await
    }
    pub async fn issue_email_verification(
        &self,
        account: i32,
        ip: std::net::IpAddr,
    ) -> Result<(String, String)> {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let hash = token_hash(&token)?;
        let time = hiscores::now()?;
        let email = self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let email: String = tx.query_row("SELECT email FROM credentials c JOIN accounts a ON a.id=c.account WHERE c.account=?1 AND a.guid IS NOT NULL", [account], |r| r.get(0)).optional()?.context("Register before verifying an email.")?;
            let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM verified_emails WHERE account=?1 AND email=?2)", params![account,email], |r| r.get(0))?;
            ensure!(!verified, "Email already verified.");
            reserve(&tx, account, ip, time)?;
            tx.execute("DELETE FROM email_verifications WHERE expires_at<=?1", [time])?;
            tx.execute("INSERT INTO email_verifications(account,email,hash,issued_at,expires_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(account) DO UPDATE SET email=excluded.email,hash=excluded.hash,issued_at=excluded.issued_at,expires_at=excluded.expires_at", params![account,email,hash,time,time+86400])?;
            tx.commit()?;
            Ok(email)
        }).await?;
        Ok((email, token))
    }
    pub async fn issue_email_reset(
        &self,
        email: String,
        ip: std::net::IpAddr,
    ) -> Result<Option<(i32, String, String)>> {
        let email = accounts::email(&email)?;
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let hash = token_hash(&token)?;
        let time = hiscores::now()?;
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let account: Option<i32> = tx.query_row("SELECT c.account FROM credentials c JOIN accounts a ON a.id=c.account WHERE c.email=?1 AND a.guid IS NOT NULL AND NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=c.account)", [&email], |r| r.get(0)).optional()?;
            let Some(account) = account else { return Ok(None); };
            if let Err(error) = reserve(&tx, account, ip, time) {
                if error.is::<Rejected>() {
                    tracing::info!(account, error=%error, "email reset request throttled");
                    return Ok(None);
                }
                return Err(error);
            }
            tx.execute("DELETE FROM email_password_resets WHERE expires_at<=?1", [time])?;
            tx.execute("INSERT INTO email_password_resets(account,email,hash,issued_at,expires_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(account) DO UPDATE SET email=excluded.email,hash=excluded.hash,issued_at=excluded.issued_at,expires_at=excluded.expires_at", params![account,email,hash,time,time+900])?;
            tx.commit()?;
            Ok(Some((account,email,token)))
        }).await
    }
    pub async fn confirm_email(&self, token: String) -> Result<i32> {
        let hash = token_hash(&token)?;
        let time = hiscores::now()?;
        let account = self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let (account,email): (i32,String) = tx.query_row("SELECT v.account,v.email FROM email_verifications v JOIN credentials c ON c.account=v.account AND c.email=v.email JOIN accounts a ON a.id=v.account WHERE v.hash=?1 AND v.expires_at>?2 AND a.guid IS NOT NULL", params![hash,time], |r| Ok((r.get(0)?,r.get(1)?))).optional()?.context("invalid or expired verification link")?;
            tx.execute("INSERT INTO verified_emails(account,email,verified_at) VALUES(?1,?2,?3) ON CONFLICT(account) DO UPDATE SET email=excluded.email,verified_at=excluded.verified_at", params![account,email,time])?;
            tx.execute("DELETE FROM email_verifications WHERE account=?1", [account])?;
            tx.commit()?;
            Ok(account)
        }).await?;
        tracing::info!(
            account,
            "email ownership verified; confirmation token consumed"
        );
        Ok(account)
    }
}

#[cfg(test)]
mod reset_tests;
#[cfg(test)]
mod tests;
