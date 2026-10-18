use super::*;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rusqlite::OptionalExtension;

pub(super) fn hash(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|v| v.to_string())
        .map_err(|_| anyhow::anyhow!("password hashing failed"))
}
fn email(value: &str) -> Result<String> {
    // HELLO credentials must fit the server's 2048-bit PKCS#1 v1.5 block.
    ensure!(
        value.len() <= 245 && value.is_ascii(),
        "email must be ASCII and at most 245 bytes for native login"
    );
    let (local, domain) = value.split_once('@').context("invalid email address")?;
    ensure!(
        !local.is_empty()
            && local.len() <= 64
            && !local.starts_with('.')
            && !local.ends_with('.')
            && !local.contains("..")
            && local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
            && domain.contains('.')
            && domain.split('.').all(|label| !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')),
        "invalid email address"
    );
    Ok(value.to_ascii_lowercase())
}
pub(super) fn password(value: &str) -> Result<()> {
    ensure!(
        (5..=128).contains(&value.len()) && !value.chars().any(char::is_control),
        "password must be 5–128 bytes without control characters"
    );
    Ok(())
}
fn guest(db: &Connection, guid: &str, create: bool, authentication: bool) -> Result<i32> {
    let rejected = |reason: &'static str| -> anyhow::Error {
        if authentication {
            InvalidCredentials.into()
        } else {
            anyhow::anyhow!(reason)
        }
    };
    // Reserve the email namespace so an empty password can never create a shadow login.
    if !(!guid.is_empty()
        && guid.len() <= 128
        && guid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)))
    {
        return Err(rejected("invalid guest identity"));
    }
    let found: Option<(i32, bool)> = db.query_row(
        "SELECT id,EXISTS(SELECT 1 FROM credentials WHERE account=accounts.id) FROM accounts WHERE guid=?1",
        [guid], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    if let Some((id, registered)) = found {
        if registered {
            return Err(rejected(
                "guest identity has been converted; sign in with your email and password",
            ));
        }
        return Ok(id);
    }
    if !create {
        return Err(rejected("unknown guest; load character list first"));
    }
    db.execute("INSERT INTO accounts(guid,name) VALUES(?1,'Guest')", [guid])?;
    let id = i32::try_from(db.last_insert_rowid())?;
    db.execute(
        "INSERT INTO account_fame(account,balance,earned) VALUES(?1,0,0)",
        [id],
    )?;
    db.execute(
        "UPDATE accounts SET name=?2 WHERE id=?1",
        params![id, format!("Guest{id}")],
    )?;
    Ok(id)
}
impl Store {
    pub async fn choose_name(&self, account: i32, name: String) -> Result<()> {
        match self.name_choice(account, name, false).await? {
            NameChoice::Committed { .. } => Ok(()),
            NameChoice::Rejected(reason) => anyhow::bail!(reason),
        }
    }
    pub(crate) async fn name_choice(
        &self,
        account: i32,
        name: String,
        allow_paid: bool,
    ) -> Result<NameChoice> {
        if !(3..=10).contains(&name.len()) || !name.bytes().all(|c| c.is_ascii_alphabetic()) {
            return Ok(NameChoice::Rejected("names must be 3–10 letters"));
        }
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let (current, registered, balance): (String, bool, i32) = tx.query_row(
                "SELECT name,EXISTS(SELECT 1 FROM credentials WHERE account=accounts.id),credits FROM accounts WHERE id=?1",
                [account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            ensure!(balance >= 0, "invalid account Gold balance");
            if !registered { return Ok(NameChoice::Rejected("register before choosing a name")); }
            let price = if name_chosen(account, &current) { 1000 } else { 0 };
            if price > 0 && !allow_paid { return Ok(NameChoice::Rejected("Paid name changes must be made in game.")); }
            let taken: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE name=?1 COLLATE NOCASE)", [&name], |r| r.get(0))?;
            if taken { return Ok(NameChoice::Rejected("name already taken")); }
            if balance < price { return Ok(NameChoice::Rejected("Not enough Gold.")); }
            let balance = balance - price;
            ensure!(tx.execute("UPDATE accounts SET name=?2,credits=?3 WHERE id=?1", params![account,name,balance])? == 1, "name choice account missing");
            tx.commit()?;
            tracing::info!(account, name, price, balance, "account name choice committed");
            Ok(NameChoice::Committed { balance })
        }).await
    }
    pub(super) async fn guest(&self, guid: String, create: bool) -> Result<i32> {
        self.call(move |db| {
            let tx = db.transaction()?;
            let id = guest(&tx, &guid, create, true)?;
            tx.commit()?;
            Ok(id)
        })
        .await
    }
    pub async fn register(
        &self,
        guid: String,
        new_email: String,
        new_password: String,
    ) -> Result<i32> {
        let email = email(&new_email)?;
        password(&new_password)?;
        let permit = self
            .hashes
            .clone()
            .try_acquire_owned()
            .context("account service busy; retry shortly")?;
        let verifier = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            hash(&new_password)
        })
        .await??;
        let id = self
            .call(move |db| {
                let tx = db.transaction()?;
                let id = guest(&tx, &guid, true, false)?;
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM credentials WHERE email=?1)",
                    [&email],
                    |r| r.get(0),
                )?;
                ensure!(!exists, "email already registered; sign in instead");
                tx.execute(
                    "INSERT INTO credentials(account,email,password_hash) VALUES(?1,?2,?3)",
                    params![id, email, verifier],
                )?;
                tx.commit()?;
                Ok(id)
            })
            .await?;
        tracing::info!(
            account = id,
            "guest account registered; guest authentication revoked"
        );
        Ok(id)
    }
    pub async fn authenticate(&self, guid: String, clear: String) -> Result<i32> {
        if clear.is_empty() {
            return self.guest(guid, false).await;
        }
        let email = email(&guid).map_err(|_| InvalidCredentials)?;
        password(&clear).map_err(|_| InvalidCredentials)?;
        let found: Option<(i32, String)> = self
            .call(move |db| {
                Ok(db
                    .query_row(
                        "SELECT account,password_hash FROM credentials WHERE email=?1",
                        [email],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?)
            })
            .await?;
        let verifier = match &found {
            Some((_, hash)) => hash.clone(),
            None => self.dummy_hash.clone(),
        };
        let permit = self
            .hashes
            .clone()
            .try_acquire_owned()
            .context("account service busy; retry shortly")?;
        let valid = tokio::task::spawn_blocking(move || -> Result<bool> {
            let _permit = permit;
            let hash = PasswordHash::new(&verifier)
                .map_err(|_| anyhow::anyhow!("invalid stored password verifier"))?;
            match Argon2::default().verify_password(clear.as_bytes(), &hash) {
                Ok(()) => Ok(true),
                Err(argon2::password_hash::Error::Password) => Ok(false),
                Err(_) => anyhow::bail!("invalid stored password verifier"),
            }
        })
        .await??;
        ensure!(valid, InvalidCredentials);
        let (id, _) = found.ok_or(InvalidCredentials)?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store(name: &str) -> (Store, Content, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "openrotmg-{name}-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let content = Content::load(Path::new("data")).unwrap();
        (Store::open(&path, &content).unwrap(), content, path)
    }
    #[tokio::test]
    async fn conversion_retains_every_save_and_revokes_guest_access_across_restart() {
        let (s, c, path) = store("registration");
        let a = s.list("native-guest".into()).await.unwrap();
        s.rename(a.id, "Restorer".into()).await.unwrap();
        let ch = s.create(a.id, c.get(WIZARD).unwrap()).await.unwrap();
        let vault = s.vault(a.id).await.unwrap();
        assert_eq!(
            s.register(
                "native-guest".into(),
                "Player@Example.org".into(),
                "native-password".into()
            )
            .await
            .unwrap(),
            a.id
        );
        assert!(s.list("native-guest".into()).await.is_err());
        assert!(
            s.authenticate("native-guest".into(), String::new())
                .await
                .is_err()
        );
        assert!(
            s.characters("player@example.org".into(), String::new())
                .await
                .is_err()
        );
        assert!(
            s.authenticate("player@example.org".into(), "wrong-password".into())
                .await
                .is_err()
        );
        assert!(
            s.authenticate("missing@example.org".into(), "native-password".into())
                .await
                .is_err()
        );
        assert!(
            s.register(
                "native-guest".into(),
                "other@example.org".into(),
                "native-password".into()
            )
            .await
            .is_err()
        );
        let other = s.list("other-guest".into()).await.unwrap();
        assert!(
            s.register(
                "other-guest".into(),
                "PLAYER@example.org".into(),
                "native-password".into()
            )
            .await
            .is_err()
        );
        assert_eq!(s.list("other-guest".into()).await.unwrap().id, other.id);
        let reopened = Store::open(&path, &c).unwrap();
        let loaded = reopened
            .characters("PLAYER@EXAMPLE.ORG".into(), "native-password".into())
            .await
            .unwrap();
        assert_eq!(
            (loaded.id, loaded.name.as_str(), loaded.characters[0].id),
            (a.id, "Restorer", ch.id)
        );
        assert_eq!(loaded.characters[0].items, ch.items);
        assert_eq!(reopened.vault(a.id).await.unwrap().items, vault.items);
        let db = Connection::open(path).unwrap();
        let verifier: String = db
            .query_row("SELECT password_hash FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert!(verifier.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(!verifier.contains("native-password"));
    }
    #[tokio::test]
    async fn invalid_authentication_is_typed_without_reclassifying_policy_or_service_errors() {
        let (s, _, _) = store("typed-credentials");
        let fresh = s
            .characters("new-guest".into(), String::new())
            .await
            .unwrap();
        assert_eq!(
            s.authenticate("new-guest".into(), String::new())
                .await
                .unwrap(),
            fresh.id
        );
        s.register("guest".into(), "x@example.org".into(), "password".into())
            .await
            .unwrap();
        for (guid, password) in [
            ("bad@email", "password"),
            ("x@example.org", "tiny"),
            ("x@example.org", "wrong-password"),
            ("missing@example.org", "password"),
            ("guest", ""),
            ("unknown-guest", ""),
            ("x@example.org", ""),
        ] {
            let error = s
                .authenticate(guid.into(), password.into())
                .await
                .unwrap_err();
            assert!(error.is::<InvalidCredentials>(), "{error:#}");
            assert_eq!(error.to_string(), "invalid email or password");
        }
        for guid in ["guest", "x@example.org"] {
            assert!(
                s.characters(guid.into(), String::new())
                    .await
                    .unwrap_err()
                    .is::<InvalidCredentials>()
            );
        }
        assert!(
            s.characters("x@example.org".into(), "wrong-password".into())
                .await
                .unwrap_err()
                .is::<InvalidCredentials>()
        );
        for guid in ["guest", "invalid@guest"] {
            assert!(
                !s.register(guid.into(), "other@example.org".into(), "password".into())
                    .await
                    .unwrap_err()
                    .is::<InvalidCredentials>()
            );
        }
        let permit = s.hashes.clone().acquire_many_owned(2).await.unwrap();
        let busy = s
            .authenticate("x@example.org".into(), "password".into())
            .await
            .unwrap_err();
        assert!(!busy.is::<InvalidCredentials>());
        assert_eq!(busy.to_string(), "account service busy; retry shortly");
        drop(permit);
        s.call(|db| {
            db.execute("DROP TABLE credentials", [])?;
            Ok(())
        })
        .await
        .unwrap();
        let storage = s
            .characters("x@example.org".into(), "password".into())
            .await
            .unwrap_err();
        assert!(!storage.is::<InvalidCredentials>());
        assert!(storage.to_string().contains("no such table"));
    }
    #[tokio::test]
    async fn native_name_choice_is_registered_unique_and_one_time() {
        let (s, _, _) = store("name-choice");
        let guest = s.list("name-guest".into()).await.unwrap();
        assert!(!guest.xml("", 0.0).contains("<NameChosen/>"));
        let content = Content::load(Path::new("data")).unwrap();
        let ch = s
            .create(guest.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        assert!(s.choose_name(guest.id, "Pilot".into()).await.is_err());
        s.register(
            "name-guest".into(),
            "pilot@example.org".into(),
            "password".into(),
        )
        .await
        .unwrap();
        s.choose_name(guest.id, "Pilot".into()).await.unwrap();
        let a = s
            .characters("pilot@example.org".into(), "password".into())
            .await
            .unwrap();
        assert_eq!(a.name, "Pilot");
        assert_eq!(s.load(guest.id, ch.id).await.unwrap().name, "Pilot");
        assert!(a.xml("", 0.0).contains("<NameChosen/>"));
        assert!(s.choose_name(guest.id, "Renamed".into()).await.is_err());
        let second = s
            .register(
                "other-namer".into(),
                "second@example.org".into(),
                "password".into(),
            )
            .await
            .unwrap();
        assert!(s.choose_name(second, "pILot".into()).await.is_err());
        assert!(s.choose_name(second, "Bad123".into()).await.is_err());
        s.choose_name(second, "Keeper".into()).await.unwrap();
    }
    #[tokio::test]
    async fn corrupt_verifier_is_not_reported_as_a_wrong_password() {
        let (s, _, _) = store("corrupt-verifier");
        s.register("guest".into(), "x@example.org".into(), "password".into())
            .await
            .unwrap();
        s.call(|db| {
            db.execute("UPDATE credentials SET password_hash='invalid'", [])?;
            Ok(())
        })
        .await
        .unwrap();
        let error = s
            .authenticate("x@example.org".into(), "password".into())
            .await
            .unwrap_err();
        assert!(!error.is::<InvalidCredentials>());
        assert_eq!(error.to_string(), "invalid stored password verifier");
    }
    #[tokio::test]
    async fn concurrent_conversion_has_one_winner_and_failed_validation_changes_nothing() {
        let (s, _, _) = store("registration-race");
        let a = s.list("racing-guest".into()).await.unwrap();
        let long_email = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        for (email, pw) in [
            ("bad", "native-password"),
            ("x@y.org", "tiny"),
            ("x@@y.org", "native-password"),
            (long_email.as_str(), "native-password"),
        ] {
            assert!(
                s.register("racing-guest".into(), email.into(), pw.into())
                    .await
                    .is_err()
            );
        }
        assert_eq!(s.list("racing-guest".into()).await.unwrap().id, a.id);
        let (first, second) = tokio::join!(
            s.register(
                "racing-guest".into(),
                "a@example.org".into(),
                "native-password".into()
            ),
            s.register(
                "racing-guest".into(),
                "b@example.org".into(),
                "native-password".into()
            )
        );
        assert_ne!(first.is_ok(), second.is_ok());
        assert_eq!(
            s.call(
                |db| Ok(db.query_row("SELECT count(*) FROM credentials", [], |r| r
                    .get::<_, i32>(0))?)
            )
            .await
            .unwrap(),
            1
        );
    }
}
