#[cfg(test)]
use crate::content::{ARCHER, KNIGHT, PALADIN, ROGUE, WARRIOR};
use crate::content::{Class, Content, Object, PRIEST, WIZARD};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::mpsc};
use tokio::sync::oneshot;

mod account_actions;
mod accounts;
mod character_slots;
mod vaults;
pub use vaults::{VAULT_CHESTS, VAULT_PRICE};
mod consumables;
#[cfg(test)]
mod name_choice_tests;
pub(crate) use character_slots::SlotReplay;
#[cfg(all(test, unix))]
mod crash_tests;
#[cfg(test)]
mod credits_tests;
mod death_awards;
#[cfg(test)]
mod death_date_tests;
#[cfg(test)]
mod deletion_tests;
pub(crate) mod gold;
mod guilds;
mod hiscores;
mod legends;
pub use guilds::{Guild, GuildAction, Membership as GuildMembership};
pub(crate) mod pictures;
#[cfg(test)]
mod presentation_tests;
mod purchases;
mod resurrection;
mod social;
pub use purchases::Purchase;
mod updates;
pub use social::AccountLists;
pub use updates::{UpdateFeed, UpdatePost, UpdateSnapshot};

#[derive(Debug)]
pub(crate) enum NameChoice {
    Rejected(&'static str),
    Committed { balance: i32 },
}

#[derive(Debug)]
pub(crate) struct Rejected(pub &'static str);
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Rejected {}

#[derive(Debug)]
pub struct InvalidCredentials;
impl std::fmt::Display for InvalidCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid email or password")
    }
}
impl std::error::Error for InvalidCredentials {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountPresentation {
    pub guild: Option<GuildMembership>,
    pub credits: i32,
    pub fame: i32,
    pub best_fame: BTreeMap<u16, i32>,
}
impl AccountPresentation {
    const THRESHOLDS: [i32; 5] = [20, 150, 400, 800, 2000];

    pub fn stars(&self) -> i32 {
        self.best_fame
            .values()
            .map(|best| {
                Self::THRESHOLDS
                    .iter()
                    .filter(|threshold| best >= *threshold)
                    .count() as i32
            })
            .sum()
    }

    pub fn next_quest(&self, class: u16, current_fame: i32) -> i32 {
        let best = self
            .best_fame
            .get(&class)
            .copied()
            .unwrap_or(0)
            .max(current_fame);
        Self::THRESHOLDS
            .into_iter()
            .find(|threshold| *threshold > best)
            .unwrap_or(-1)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct XpBoost {
    pub active: bool,
    pub bonus_exp: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Character {
    pub schema: u8,
    pub class: u16,
    pub base: [i32; 8],
    #[serde(default)]
    pub tex1: u32,
    #[serde(default)]
    pub tex2: u32,
    #[serde(default)]
    pub pet_kind: Option<u16>,
    pub id: i32,
    pub account: i32,
    pub name: String,
    pub level: i32,
    pub exp: i32,
    // Pre-booster schema-one writers had no boost metadata; present metadata must be complete.
    #[serde(default)]
    pub xp_boost: XpBoost,
    pub fame: i32,
    pub hp: i32,
    pub mp: i32,
    pub items: Vec<i32>,
    pub revision: i32,
    pub alive: bool,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub shots: u64,
    #[serde(default)]
    pub hits: u64,
    #[serde(default)]
    pub kills: u64,
    #[serde(default)]
    pub quests: u64,
    #[serde(default)]
    pub tiles: u64,
    #[serde(default)]
    pub pirate_caves: u64,
    // Earlier schema-one writers could not enter these dungeons.
    #[serde(default)]
    pub dungeons: [u64; 3],
    #[serde(default)]
    pub abysses: u64,
    #[serde(default)]
    pub trenches: u64,
    #[serde(default)]
    pub spider_dens: u64,
}
pub fn level_start(level: i32) -> Result<i32> {
    ensure!((1..=20).contains(&level), "invalid character level {level}");
    Ok(50 * (level - 1).pow(2))
}
pub fn experience_goal(level: i32) -> Result<i32> {
    level_start(level)?;
    Ok(50 + (level - 1) * 100)
}
impl Character {
    pub fn gain_experience(
        &mut self,
        content: &Content,
        amount: i32,
        rng: &mut impl rand::Rng,
    ) -> Result<bool> {
        self.effective(content)?;
        ensure!(amount >= 0, "negative experience reward");
        let mut next = self.clone();
        let bonus = if next.xp_boost.active { amount } else { 0 };
        next.exp = next
            .exp
            .checked_add(amount)
            .and_then(|exp| exp.checked_add(bonus))
            .context("experience overflow")?;
        next.xp_boost.bonus_exp = next
            .xp_boost
            .bonus_exp
            .checked_add(bonus)
            .context("bonus experience overflow")?;
        // Expired boost XP must never turn into Fame on a later award.
        next.fame = next
            .exp
            .checked_sub(next.xp_boost.bonus_exp)
            .context("invalid bonus experience")?
            / 1000;
        let class = &content.classes[&next.class];
        let previous = next.level;
        while next.level < 20 && next.exp >= level_start(next.level + 1)? {
            next.level += 1;
            for (i, (min, max)) in class.growth.iter().enumerate() {
                next.base[i] = next.base[i]
                    .checked_add(rng.gen_range(*min..=*max))
                    .context("growth overflow")?
                    .min(class.caps[i]);
            }
        }
        if next.level == 20 {
            next.xp_boost.active = false;
        }
        if next.level > previous {
            let effective = next.effective(content)?;
            next.hp = effective[0];
            next.mp = effective[1];
        }
        *self = next;
        Ok(self.level > previous)
    }
    pub fn effective(&self, content: &Content) -> Result<[i32; 8]> {
        let class = content
            .classes
            .get(&self.class)
            .context("unknown character class")?;
        level_start(self.level)?;
        ensure!(
            (0..=self.exp).contains(&self.xp_boost.bonus_exp)
                && (!self.xp_boost.active || self.level < 20),
            "invalid XP boost state: level={}, experience={}, bonus={}, active={}",
            self.level,
            self.exp,
            self.xp_boost.bonus_exp,
            self.xp_boost.active
        );
        ensure!(
            self.schema == 1 && self.items.len() == 12,
            "invalid character schema or inventory"
        );
        ensure!(
            self.base
                .iter()
                .zip(class.caps)
                .all(|(n, cap)| *n >= 0 && *n <= cap),
            "invalid base stats"
        );
        let slots = &content.get(self.class)?.slots;
        let mut stats = self.base;
        for (slot, item) in self.items[..4].iter().enumerate() {
            if *item == -1 {
                continue;
            }
            let item = content.get(u16::try_from(*item)?)?;
            ensure!(
                item.slot_type == slots[slot] && !item.unsupported_equipment,
                "unsupported or incompatible equipped item {} in slot {slot}",
                item.name
            );
            for (id, amount) in &item.bonuses {
                let index = crate::content::STATS
                    .iter()
                    .position(|(stat, _)| stat == id)
                    .context("unsupported equipment stat")?;
                stats[index] = stats[index]
                    .checked_add(*amount)
                    .context("effective stat overflow")?;
            }
        }
        ensure!(stats.iter().all(|v| *v >= 0), "negative effective stat");
        Ok(stats)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Account {
    pub id: i32,
    pub name: String,
    pub next_id: i32,
    pub character_slots: i32,
    pub next_slot_price: i32,
    pub guild: Option<GuildMembership>,
    pub credits: i32,
    pub fame: i64,
    pub earned_fame: i64,
    pub best_levels: std::collections::BTreeMap<u16, i32>,
    pub best_fame: BTreeMap<u16, i32>,
    pub characters: Vec<Character>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Vault {
    pub account: i32,
    pub chest: i32,
    pub items: Vec<i32>,
    pub revision: i32,
}
#[derive(Clone, Debug)]
pub struct InventoryWrite {
    pub id: i32,
    pub account: i32,
    pub revision: i32,
    pub items: Vec<i32>,
}
type Job = Box<dyn FnOnce(&mut Connection) + Send>;
#[cfg(test)]
type LifecycleReplyGate =
    std::sync::Arc<std::sync::Mutex<Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>>>;
enum Creation {
    Normal(Vec<(u16, i32)>),
    Prepared(Option<String>),
}
#[derive(Clone)]
pub struct Store {
    #[cfg(test)]
    lifecycle_lost_reply: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    lifecycle_jobs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    lifecycle_reply_gate: LifecycleReplyGate,
    tx: mpsc::Sender<Job>,
    legacy_base: [i32; 8],
    classes: std::sync::Arc<std::collections::BTreeMap<u16, Class>>,
    content: std::sync::Arc<Content>,
    hashes: std::sync::Arc<tokio::sync::Semaphore>,
    dummy_hash: String,
    dyes: std::sync::Arc<BTreeMap<u16, [Option<u32>; 2]>>,
}
pub(crate) fn decode_character(raw: &str, legacy_base: [i32; 8]) -> Result<Character> {
    let mut value: serde_json::Value = serde_json::from_str(raw)?;
    match value.get("schema") {
        None => {
            // The version-zero writer only supported level-one Wizards.
            ensure!(
                value.get("class").is_none() && value.get("base").is_none(),
                "class metadata requires a schema version"
            );
            ensure!(
                value["level"] == 1 && value["exp"] == 0,
                "unrecognized legacy character progression"
            );
            value["schema"] = 1.into();
            value["class"] = WIZARD.into();
            value["base"] = serde_json::to_value(legacy_base)?;
        }
        Some(schema) => ensure!(schema == 1, "unsupported character schema {schema}"),
    }
    Ok(serde_json::from_value(value)?)
}
impl Store {
    pub fn open(path: &Path, content: &Content) -> Result<Self> {
        let legacy_base = content
            .classes
            .get(&WIZARD)
            .context("Wizard class missing")?
            .base;
        let mut c = Connection::open(path)?;
        let version: i32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            matches!(version, 0 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15),
            "unsupported game database schema {version}"
        );
        death_awards::check_schema(&c, version)?;
        let rewards_exist: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='gold_rewards')",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            version < 10 || rewards_exist,
            "gold reward history missing; restore reviewed state instead of allowing duplicate grants"
        );
        let credits_exist: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('accounts') WHERE name='credits')",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            version < 8 || credits_exist,
            "schema-8 gold balance missing; restore reviewed state instead of recreating balances"
        );
        let wallet_exists: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='account_fame')",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            version < 5 || wallet_exists,
            "schema-5 fame wallet table missing; restore reviewed state instead of recreating balances"
        );
        let lists_exist: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='account_lists')",
            [], |r| r.get(0),
        )?;
        ensure!(
            version < 6 || lists_exist,
            "schema-6 account lists missing; restore reviewed state instead of clearing preferences"
        );
        let updates_exist: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type=\'table\' AND name=\'update_feed\')", [], |r| r.get(0),
        )?;
        ensure!(
            version < 7 || updates_exist,
            "schema-7 update feed missing; restore reviewed state instead of clearing published news"
        );
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS accounts(id INTEGER PRIMARY KEY,guid TEXT NOT NULL UNIQUE,name TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS shore_bots(name TEXT PRIMARY KEY,account INTEGER NOT NULL UNIQUE REFERENCES accounts(id));
CREATE TABLE IF NOT EXISTS characters(id INTEGER PRIMARY KEY,account INTEGER NOT NULL REFERENCES accounts(id),stats TEXT NOT NULL,items TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 0,alive INTEGER NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS deaths(character INTEGER PRIMARY KEY REFERENCES characters(id),snapshot TEXT NOT NULL,killed_by TEXT NOT NULL,died_at INTEGER NOT NULL DEFAULT(unixepoch()));
CREATE TABLE IF NOT EXISTS vaults(account INTEGER PRIMARY KEY REFERENCES accounts(id),items TEXT NOT NULL CHECK(json_array_length(items)=8),revision INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS credentials(account INTEGER PRIMARY KEY REFERENCES accounts(id),email TEXT NOT NULL UNIQUE,password_hash TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS competitors(account INTEGER PRIMARY KEY REFERENCES accounts(id));
CREATE TABLE IF NOT EXISTS collections(account INTEGER NOT NULL REFERENCES accounts(id),item INTEGER NOT NULL CHECK(item BETWEEN 0 AND 65535),found_at INTEGER NOT NULL,PRIMARY KEY(account,item));
CREATE TABLE IF NOT EXISTS discord_links(account INTEGER PRIMARY KEY REFERENCES accounts(id),discord_id TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS discord_codes(account INTEGER PRIMARY KEY REFERENCES accounts(id),hash TEXT NOT NULL UNIQUE,issued_at INTEGER NOT NULL,expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS account_fame(account INTEGER PRIMARY KEY REFERENCES accounts(id),balance INTEGER NOT NULL CHECK(balance BETWEEN 0 AND 2147483647),earned INTEGER NOT NULL CHECK(earned BETWEEN balance AND 2147483647));
CREATE TABLE IF NOT EXISTS discord_recovery(account INTEGER PRIMARY KEY REFERENCES discord_links(account),enabled_at INTEGER NOT NULL,last_issued_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS recovery_codes(account INTEGER PRIMARY KEY REFERENCES accounts(id),hash TEXT NOT NULL UNIQUE,issued_at INTEGER NOT NULL,expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS password_resets(account INTEGER PRIMARY KEY REFERENCES accounts(id),hash TEXT NOT NULL UNIQUE,issued_at INTEGER NOT NULL,expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS account_lists(account INTEGER NOT NULL REFERENCES accounts(id),list_id INTEGER NOT NULL CHECK(list_id IN (0,1)),target INTEGER NOT NULL REFERENCES accounts(id),PRIMARY KEY(account,list_id,target),CHECK(account<>target));
CREATE TABLE IF NOT EXISTS update_feed(id INTEGER PRIMARY KEY CHECK(id=1),snapshot TEXT NOT NULL CHECK(json_valid(snapshot)),synced_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS deaths_by_time ON deaths(died_at);")?;
        let journal: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        let synchronous: i32 = c.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
        ensure!(
            journal == "wal" || path == Path::new(":memory:"),
            "WAL unavailable: {journal}"
        );
        ensure!(
            synchronous == 2,
            "FULL durability unavailable: synchronous={synchronous}"
        );
        tracing::info!(%journal, synchronous, "account store durability configured");
        let migration = c.transaction()?;
        guilds::migrate(&migration, version)?;
        gold::migrate(&migration, version, rewards_exist)?;
        if !credits_exist {
            migration.execute_batch("ALTER TABLE accounts ADD COLUMN credits INTEGER NOT NULL DEFAULT 100 CHECK(credits BETWEEN 0 AND 2147483647)")?;
        }
        character_slots::migrate(&migration, version)?;
        vaults::migrate(&migration, version)?;
        death_awards::migrate(&migration, version, legacy_base)?;
        pictures::migrate(&migration, version)?;
        let records = {
            let mut q = migration.prepare("SELECT id,stats FROM characters ORDER BY id")?;
            q.query_map([], |r| Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut migrated = 0;
        for (id, raw) in records {
            let ch = decode_character(&raw, legacy_base)
                .with_context(|| format!("character {id} migration"))?;
            let definition = content
                .classes
                .get(&ch.class)
                .context("persisted character has unknown class")?;
            ensure!(
                (1..=20).contains(&ch.level)
                    && ch
                        .base
                        .iter()
                        .zip(definition.caps)
                        .all(|(v, cap)| *v >= 0 && *v <= cap),
                "invalid persisted class stats for character {id}"
            );
            if serde_json::from_str::<serde_json::Value>(&raw)?
                .get("schema")
                .is_none()
            {
                migration.execute(
                    "UPDATE characters SET stats=?2 WHERE id=?1",
                    params![id, serde_json::to_string(&ch)?],
                )?;
                migrated += 1;
            }
        }
        if !wallet_exists {
            let mut earned = std::collections::BTreeMap::<i32, i64>::new();
            {
                let mut q = migration.prepare(
                "SELECT c.account,d.snapshot FROM deaths d JOIN characters c ON c.id=d.character",
            )?;
                for row in q.query_map([], |r| Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?)))? {
                    let (account, raw) = row?;
                    let ch = decode_character(&raw, legacy_base)?;
                    ensure!(
                        ch.account == account && !ch.alive && ch.fame >= 0,
                        "invalid death credit"
                    );
                    let amount = earned.entry(account).or_default();
                    *amount = amount
                        .checked_add(i64::from(ch.fame))
                        .context("account fame overflow")?;
                }
            }
            let accounts = {
                let mut q = migration.prepare("SELECT id FROM accounts")?;
                q.query_map([], |r| r.get::<_, i32>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for account in accounts {
                let amount = earned.get(&account).copied().unwrap_or(0);
                migration.execute(
                    "INSERT INTO account_fame(account,balance,earned) VALUES(?1,?2,?2)",
                    params![account, amount],
                )?;
            }
        } else {
            let missing: bool = migration.query_row("SELECT EXISTS(SELECT 1 FROM accounts a LEFT JOIN account_fame f ON f.account=a.id WHERE f.account IS NULL)", [], |r| r.get(0))?;
            ensure!(
                !missing,
                "account fame wallet missing; restore reviewed state instead of recreating balances"
            );
        }
        death_awards::validate_wallets(&migration)?;
        migration.pragma_update(None, "user_version", 15)?;
        migration.commit()?;
        if migrated > 0 {
            tracing::info!(
                migrated,
                from_schema = 0,
                to_schema = 1,
                "character metadata migrated; death snapshots unchanged"
            );
        }
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("account-store".into())
            .spawn(move || {
                let mut c = c;
                for job in rx {
                    job(&mut c);
                }
            })?;
        Ok(Self {
            #[cfg(test)]
            lifecycle_lost_reply: Default::default(),
            #[cfg(test)]
            lifecycle_jobs: Default::default(),
            #[cfg(test)]
            lifecycle_reply_gate: Default::default(),
            tx,
            legacy_base,
            classes: std::sync::Arc::new(content.classes.clone()),
            content: std::sync::Arc::new(content.clone()),
            hashes: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
            dummy_hash: accounts::hash("unknown-account")?,
            dyes: std::sync::Arc::new(
                content
                    .objects
                    .values()
                    .filter(|item| item.class == "Dye" && item.activate == "Dye")
                    .map(|item| (item.kind, item.textures))
                    .collect(),
            ),
        })
    }
    #[cfg(test)]
    pub(crate) fn lifecycle_test_pause_reply(&self) -> (oneshot::Receiver<()>, mpsc::Sender<()>) {
        let (entered, ready) = oneshot::channel();
        let (release, wait) = mpsc::channel();
        assert!(
            self.lifecycle_reply_gate
                .lock()
                .unwrap()
                .replace((entered, wait))
                .is_none()
        );
        (ready, release)
    }
    #[cfg(test)]
    pub(crate) fn lifecycle_test_lose_reply(&self) {
        assert!(
            !self
                .lifecycle_lost_reply
                .swap(true, std::sync::atomic::Ordering::SeqCst)
        );
    }
    #[cfg(test)]
    pub(crate) fn lifecycle_test_jobs(&self) -> usize {
        self.lifecycle_jobs
            .load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) async fn lifecycle_test_job<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.call(f).await
    }
    async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        #[cfg(test)]
        let lost = self
            .lifecycle_lost_reply
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        let jobs = self.lifecycle_jobs.clone();
        #[cfg(test)]
        let gate = self.lifecycle_reply_gate.lock().unwrap().take();
        #[cfg(test)]
        jobs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.tx
            .send(Box::new(move |c| {
                let result = f(c);
                #[cfg(test)]
                if let Some((entered, wait)) = gate {
                    let _ = entered.send(());
                    if wait.recv().is_err() {
                        let _ = tx.send(Err(anyhow::anyhow!("lifecycle Store gate abandoned")));
                        return;
                    }
                }
                #[cfg(test)]
                jobs.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                #[cfg(test)]
                if lost {
                    return;
                }
                let _ = tx.send(result);
            }))
            .map_err(|_| anyhow::anyhow!("account store stopped"))?;
        rx.await.context("account worker failed")?
    }
    pub async fn list(&self, guid: String) -> Result<Account> {
        let id = self.guest(guid, true).await?;
        self.list_id(id).await
    }
    pub async fn characters(&self, guid: String, password: String) -> Result<Account> {
        let id = if password.is_empty() {
            self.guest(guid, true).await?
        } else {
            self.authenticate(guid, password).await?
        };
        self.list_id(id).await
    }
    pub async fn presentation(&self, account: i32) -> Result<AccountPresentation> {
        let legacy_base = self.legacy_base;
        let classes = self.classes.clone();
        self.call(move |db| {
            let tx = db.transaction()?;
            let (presentation, _) = read_presentation(&tx, account, legacy_base, &classes)?;
            tx.commit()?;
            Ok(presentation)
        })
        .await
    }
    async fn list_id(&self, id: i32) -> Result<Account> {
        let legacy_base = self.legacy_base;
        let classes = self.classes.clone();
        let account = self.call(move|c|{let tx=c.transaction()?;
            let name:String=tx.query_row("SELECT name FROM accounts WHERE id=?1",[id],|r|r.get(0))?;
            let character_slots=character_slots::read(&tx,id)?;
            let next_slot_price=character_slots::price(character_slots)?;
            let mut characters=Vec::new();{let mut q=tx.prepare("SELECT id,stats,items,revision,alive FROM characters WHERE account=?1 AND alive=1 ORDER BY id")?;let rows=q.query_map([id],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i32>(3)?,r.get::<_,bool>(4)?)))?;
            for row in rows{let(cid,stats,items,rev,alive)=row?;let mut ch:Character=serde_json::from_str(&stats)?;ch.id=cid;ch.account=id;ch.name=name.clone();ch.items=serde_json::from_str(&items)?;ch.revision=rev;ch.alive=alive;characters.push(ch);}}
            let next_id:i32=tx.query_row("SELECT coalesce(max(id),0)+1 FROM characters WHERE account=?1",[id],|r|r.get(0))?;let best_levels = { let mut q = tx.prepare("SELECT json_extract(stats,'$.class'),max(json_extract(stats,'$.level')) FROM characters WHERE account=?1 GROUP BY json_extract(stats,'$.class')")?; q.query_map([id], |r| Ok((r.get::<_,u16>(0)?, r.get::<_,i32>(1)?)))?.collect::<rusqlite::Result<std::collections::BTreeMap<_,_>>>()? };let (presentation,earned_fame) = read_presentation(&tx, id, legacy_base, &classes)?;tx.commit()?;Ok(Account{id,name,next_id,character_slots,next_slot_price,guild:presentation.guild,credits:presentation.credits,fame:i64::from(presentation.fame),earned_fame,best_levels,best_fame:presentation.best_fame,characters})}).await?;
        self.vaults(account.id).await?;
        Ok(account)
    }
    pub async fn shore_character(
        &self,
        identity: String,
        name: String,
        content: &Content,
        prefer_priest: bool,
    ) -> Result<Character> {
        let guid = self.call(move |db| {
            let tx = db.transaction()?;
            let existing: Option<(String, i32)> = tx.query_row(
                "SELECT guid,accounts.id FROM accounts JOIN shore_bots ON accounts.id=shore_bots.account WHERE shore_bots.name=?1",
                [&identity], |r| Ok((r.get(0)?, r.get(1)?))).optional()
                .context("read shore bot identity; credential-free restores require operator recovery")?;
            let guid = match existing {
                Some((guid, account)) => {
                    if tx.execute("UPDATE accounts SET name=?1 WHERE id=?2 AND name<>?1", params![name,account])? > 0 {
                        tracing::info!(account, identity, name, "autonomous account renamed");
                    }
                    guid
                },
                None => {
                    let guid = format!("shore-{:032x}", rand::random::<u128>());
                    tx.execute("INSERT INTO accounts(guid,name) VALUES(?1,?2)", params![guid,name])?;
                    let account = tx.last_insert_rowid();
                    vaults::provision(&tx, i32::try_from(account)?)?;
                    tx.execute("INSERT INTO account_fame(account,balance,earned) VALUES(?1,0,0)", [account])?;
                    tx.execute("INSERT INTO shore_bots(name,account) VALUES(?1,?2)", params![identity,account])?;
                    guid
                }
            };
            tx.commit()?;
            Ok(guid)
        }).await?;
        let mut account = self.list(guid).await?;
        ensure!(
            account.characters.len() <= 1,
            "shore account has multiple live characters"
        );
        if let Some(character) = account.characters.pop() {
            return Ok(character);
        }
        let class = if prefer_priest && content.unlocked(PRIEST, &account.best_levels)? {
            PRIEST
        } else {
            WIZARD
        };
        self.create(account.id, content.get(class)?).await
    }
    pub async fn create(&self, account: i32, class: &Object) -> Result<Character> {
        ensure!(
            self.classes.contains_key(&class.kind) && class.class == "Player",
            "unknown playable class"
        );
        let requirements = self
            .classes
            .get(&class.kind)
            .context("class metadata missing")?
            .requires
            .clone();
        self.create_character(account, class, Creation::Normal(requirements))
            .await
    }
    pub(crate) async fn create_prepared(
        &self,
        account: i32,
        class: &Object,
        name: Option<String>,
    ) -> Result<Character> {
        tracing::info!(
            account,
            class = class.kind,
            "explicit prepared creation requested; no prerequisite progress granted"
        );
        self.create_character(account, class, Creation::Prepared(name))
            .await
    }
    async fn create_character(
        &self,
        account: i32,
        class: &Object,
        creation: Creation,
    ) -> Result<Character> {
        ensure!(
            self.classes.contains_key(&class.kind) && class.class == "Player",
            "unknown playable class"
        );
        let kind = class.kind;
        let base = [
            class.hp,
            class.mp,
            class.attack,
            class.defense,
            class.speed,
            class.dexterity,
            class.vitality,
            class.wisdom,
        ];
        let hp = class.hp;
        let mp = class.mp;
        let items = class.equipment.clone();
        self.call(move |c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let slots = character_slots::read(&tx, account)?;
            let normal = matches!(&creation, Creation::Normal(_));
            let (requirements, prepared_name) = match creation {
                Creation::Normal(requirements) => (requirements, None),
                Creation::Prepared(name) => {
                    let count: i64 = tx.query_row("SELECT count(*) FROM characters WHERE account=?1 AND alive=1", [account], |r| r.get(0))?;
                    ensure!(count == 0, "prepared creation cannot implicitly replace a living character");
                    (Vec::new(), name)
                }
            };
            for (required_class, required_level) in requirements {
                let best: Option<i32> = tx.query_row(
                    "SELECT max(json_extract(stats,\'$.level\')) FROM characters WHERE account=?1 AND json_extract(stats,\'$.class\')=?2",
                    params![account, required_class], |r| r.get(0),
                )?;
                ensure!(best.is_some_and(|level| level >= required_level), anyhow::anyhow!(Rejected("class is locked")).context(format!("class {kind:#x} is locked: requires {required_class:#x} level {required_level}")));
            }
            if normal {
                let living: i32 = tx.query_row("SELECT count(*) FROM characters WHERE account=?1 AND alive=1", [account], |r| r.get(0))?;
                ensure!(living < slots, Rejected("No free character slots."));
            }
            if let Some(name) = prepared_name {
                tx.execute("UPDATE accounts SET name=?1 WHERE id=?2", params![name,account])?;
            }
            let name: String =
                tx.query_row("SELECT name FROM accounts WHERE id=?1", [account], |r| {
                    r.get(0)
                })?;
            let mut ch = Character {
                schema: 1,
                class: kind,
                base,
                tex1: 0,
                tex2: 0,
                pet_kind: None,
                id: 0,
                account,
                name,
                level: 1,
                exp: 0,
                xp_boost: XpBoost::default(),
                fame: 0,
                hp,
                mp,
                items,
                revision: 0,
                alive: true,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
                shots: 0,
                hits: 0,
                kills: 0,
                quests: 0,
                tiles: 0,
                pirate_caves: 0,
                dungeons: [0; 3],
                abysses: 0,
                trenches: 0,
                spider_dens: 0,
            };
            tx.execute(
                "INSERT INTO characters(account,stats,items) VALUES(?1,?2,?3)",
                params![
                    account,
                    serde_json::to_string(&ch)?,
                    serde_json::to_string(&ch.items)?
                ],
            )?;
            ch.id = i32::try_from(tx.last_insert_rowid())?;
            tx.commit()?;
            Ok(ch)
        })
        .await
    }
    pub(crate) async fn character_alive(&self, id: i32, guid: String) -> Result<bool> {
        self.call(move |c| Ok(c.query_row("SELECT c.alive FROM characters c JOIN accounts a ON a.id=c.account WHERE c.id=?1 AND a.guid=?2",params![id,guid],|r|r.get(0))?)).await
    }
    pub async fn load(&self, account: i32, id: i32) -> Result<Character> {
        self.call(move|c|{let(stats,items,revision,name):(String,String,i32,String)=c.query_row("SELECT c.stats,c.items,c.revision,a.name FROM characters c JOIN accounts a ON a.id=c.account WHERE c.id=?1 AND c.account=?2 AND c.alive=1",params![id,account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).context("character does not belong to this account or is dead")?;let mut ch:Character=serde_json::from_str(&stats)?;ch.id=id;ch.account=account;ch.name=name;ch.items=serde_json::from_str(&items)?;ch.revision=revision;ch.alive=true;Ok(ch)}).await
    }
    pub async fn delete_character(&self, account: i32, id: i32) -> Result<()> {
        self.call(move |db| {
            // Class unlocks depend on lifetime character records, not just surviving characters.
            let n = db.execute(
                "UPDATE characters SET alive=0,items=?3,revision=revision+1,stats=json_set(stats,'$.alive',json('false'),'$.hp',0,'$.items',json(?3),'$.revision',revision+1) WHERE id=?1 AND account=?2 AND alive=1",
                params![id, account, serde_json::to_string(&vec![-1; 12])?],
            )?;
            ensure!(n == 1, Rejected("character does not belong to this account or is no longer alive"));
            Ok(())
        })
        .await?;
        tracing::info!(
            account,
            character = id,
            "character deleted without death rewards"
        );
        Ok(())
    }
    pub async fn exchange(
        &self,
        writes: Vec<InventoryWrite>,
        vaults: Vec<Vault>,
    ) -> Result<Vec<i32>> {
        self.exchange_collected(writes, vaults, None).await
    }
    pub async fn exchange_collected(
        &self,
        writes: Vec<InventoryWrite>,
        vaults: Vec<Vault>,
        collected: Option<u16>,
    ) -> Result<Vec<i32>> {
        ensure!(
            collected.is_none() || writes.len() == 1,
            "collection requires one pickup owner"
        );
        let collector = writes.first().context("missing inventory owner")?.account;
        ensure!(
            (1..=2).contains(&writes.len()),
            "exchange must contain one or two characters"
        );
        let mut chests = std::collections::BTreeSet::new();
        ensure!(vaults.len() <= 2, "too many vault exchange sources");
        for vault in &vaults {
            ensure!(chests.insert(vault.chest), "exchange repeats a chest");
            ensure!(
                writes.len() == 1 && writes[0].account == vault.account,
                "vault exchange must use its owner's character"
            );
            ensure!(
                vault.items.len() == 8 && vault.items.iter().all(|i| (-1..=65535).contains(i)),
                "invalid eight-slot vault"
            );
            ensure!(
                vault.revision >= 0 && vault.revision < i32::MAX,
                "invalid vault revision"
            );
        }
        let mut owners = std::collections::BTreeSet::new();
        for write in &writes {
            ensure!(owners.insert(write.account), "exchange repeats an account");
            ensure!(
                write.items.len() == 12 && write.items.iter().all(|i| (-1..=65535).contains(i)),
                "invalid twelve-slot inventory"
            );
            ensure!(
                write.revision >= 0 && write.revision < i32::MAX,
                "invalid inventory revision"
            );
        }
        self.call(move |c| {
            let tx = c.transaction()?;
            let mut revisions = Vec::new();
            for write in writes {
                let n = tx.execute("UPDATE characters SET items=?4,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1", params![write.id, write.account, write.revision, serde_json::to_string(&write.items)?])?;
                ensure!(n == 1, "inventory conflict: character {}, account {}, expected revision {}", write.id, write.account, write.revision);
                revisions.push(write.revision + 1);
            }
            let rewarded = if let Some(item) = collected {
                tx.execute("INSERT OR IGNORE INTO collections(account,item,found_at) VALUES(?1,?2,unixepoch())", params![collector,item])? > 0
                    && gold::collection(&tx, collector)?
            } else { false };
            for vault in vaults { vaults::save(&tx, &vault)?; }
            tx.commit()?;
            if rewarded {
                tracing::info!(account=collector, amount=gold::AMOUNT, reward="collection_top_half", "gold reward committed");
            }
            Ok(revisions)
        }).await
    }
    pub async fn consume_stat(
        &self,
        mut ch: Character,
        slot: Option<usize>,
        index: usize,
        amount: i32,
        vaults: Vec<Vault>,
    ) -> Result<i32> {
        let (kind, step) = match index {
            0 => (0xae9, 5),
            1 => (0xaea, 5),
            2 => (0xa1f, 1),
            3 => (0xa20, 1),
            4 => (0xa21, 1),
            5 => (0xa4c, 1),
            6 => (0xa34, 1),
            7 => (0xa35, 1),
            _ => anyhow::bail!("unsupported permanent potion stat"),
        };
        ensure!(
            amount > 0
                && amount <= step
                && ch.items.len() == 12
                && slot.is_none_or(|slot| ch.items.get(slot) == Some(&kind)),
            "permanent potion ownership or activation mismatch"
        );
        let cap = self
            .classes
            .get(&ch.class)
            .context("unknown potion class")?
            .caps[index];
        ensure!(
            ch.alive
                && ch.hp > 0
                && ch.base[index] >= 0
                && ch.base[index] < cap
                && amount == (cap - ch.base[index]).min(step),
            "dead character or permanent stat already capped"
        );
        ensure!(
            ch.revision >= 0 && ch.revision < i32::MAX,
            "invalid potion revision"
        );
        ensure!(
            vaults.len() <= 1
                && vaults
                    .iter()
                    .all(|v| slot.is_none() && v.account == ch.account),
            "potion vault owner mismatch"
        );
        self.call(move |db| {
            let tx = db.transaction()?;
            let (raw, items): (String, String) = tx.query_row(
                "SELECT stats,items FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![ch.id, ch.account, ch.revision], |r| Ok((r.get(0)?, r.get(1)?)))
                .context("permanent potion conflict: dead, foreign or stale character")?;
            let saved: Character = serde_json::from_str(&raw)?;
            ensure!(saved.schema == ch.schema && saved.class == ch.class && saved.base == ch.base
                && saved.level == ch.level && saved.exp == ch.exp && saved.fame == ch.fame
                && saved.pirate_caves == ch.pirate_caves && saved.dungeons == ch.dungeons && saved.abysses == ch.abysses && saved.trenches == ch.trenches && saved.spider_dens == ch.spider_dens
                && serde_json::from_str::<Vec<i32>>(&items)? == ch.items,
                "permanent potion reservation changed");
            ch.base[index] += amount;
            if let Some(slot) = slot {
                ch.items[slot] = -1;
            }
            ch.revision += 1;
            let n = tx.execute("UPDATE characters SET stats=?4,items=?5,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![ch.id, ch.account, ch.revision - 1, serde_json::to_string(&ch)?, serde_json::to_string(&ch.items)?])?;
            ensure!(n == 1, "permanent potion commit lost its reservation");
            for vault in vaults { vaults::save(&tx, &vault)?; }
            tx.commit()?;
            Ok(ch.revision)
        }).await
    }
    pub async fn progress(&self, ch: Character) -> Result<()> {
        self.progress_batch(vec![ch]).await
    }
    pub async fn progress_batch(&self, characters: Vec<Character>) -> Result<()> {
        ensure!(
            (1..=85).contains(&characters.len()),
            "invalid progress batch size"
        );
        let mut accounts = std::collections::BTreeSet::new();
        ensure!(
            characters.iter().all(|ch| accounts.insert(ch.account)),
            "progress batch repeats an account"
        );
        self.call(move |c| {
            let tx = c.transaction()?;
            for ch in characters {
                let n = tx.execute("UPDATE characters SET stats=?3 WHERE id=?1 AND account=?2 AND alive=1 AND revision=?4", params![ch.id, ch.account, serde_json::to_string(&ch)?, ch.revision])?;
                ensure!(n == 1, "progress conflict: dead, missing or stale character {} at revision {}", ch.id, ch.revision);
            }
            tx.commit()?;
            Ok(())
        }).await
    }
    pub async fn death(&self, mut ch: Character, killed_by: String) -> Result<()> {
        ch.xp_boost.active = false;
        ensure!(
            !ch.alive && ch.hp == 0 && ch.fame >= 0 && ch.revision >= 0 && ch.revision < i32::MAX,
            "death snapshot must be dead with nonnegative fame and a valid revision"
        );
        let content = self.content.clone();
        self.call(move |db| {
            let tx = db.transaction()?;
            let (raw, items): (String, String) = tx.query_row(
                "SELECT stats,items FROM characters WHERE id=?1 AND account=?2 AND alive=1 AND revision=?3",
                params![ch.id, ch.account, ch.revision], |r| Ok((r.get(0)?, r.get(1)?)))
                .context("death conflict: duplicate, foreign or stale character")?;
            let saved: Character = serde_json::from_str(&raw)?;
            ensure!(saved.class == ch.class && saved.schema == ch.schema && ch.exp >= saved.exp
                && ch.fame >= saved.fame && ch.xp_boost.bonus_exp >= saved.xp_boost.bonus_exp
                && serde_json::from_str::<Vec<i32>>(&items)? == ch.items,
                "death snapshot lost committed progress or equipment");
            let bonus = death_awards::equipment(&ch, &content)?;
            let total = death_awards::total(&ch, bonus)?;
            let snapshot = serde_json::to_string(&ch)?;
            let n = tx.execute("UPDATE characters SET alive=0,stats=?3,items=?4,revision=revision+1 WHERE id=?1 AND account=?2 AND alive=1 AND revision=?5",
                params![ch.id,ch.account,snapshot,serde_json::to_string(&[-1;12])?,ch.revision])?;
            ensure!(n == 1, "death lost its reservation");
            tx.execute("INSERT INTO deaths(character,snapshot,killed_by,equipment_fame) VALUES(?1,?2,?3,?4)",
                params![ch.id,snapshot,killed_by,bonus])?;
            ensure!(tx.execute("UPDATE account_fame SET balance=balance+?2,earned=earned+?2 WHERE account=?1",params![ch.account,total])? == 1,
                "account fame wallet missing");
            guilds::credit_death(&tx,ch.account,total)?;
            tx.commit()?;
            tracing::info!(account=ch.account, character=ch.id, base_fame=ch.fame, equipment_fame=bonus, total_fame=total, "permanent death award committed");
            Ok(())
        }).await
    }
    pub async fn fame(&self, account: i32, id: i32) -> Result<String> {
        let legacy_base = self.legacy_base;
        // The legacy field is called CreatedOn, but the client labels it as the death date.
        self.call(move|db|{let (snapshot,killer,died_at,bonus):(String,String,u64,i32)=db.query_row("SELECT snapshot,killed_by,died_at,equipment_fame FROM deaths JOIN characters ON characters.id=deaths.character WHERE characters.account=?1 AND deaths.character=?2",params![account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;let c = decode_character(&snapshot, legacy_base)?;ensure!(c.id==id && c.account==account && !c.alive && c.hp==0 && c.items.len()==12,"invalid archived death snapshot");let total=death_awards::total(&c,bonus)?;let equipment=if bonus>0 {format!("<Bonus id=\"Well Equipped\" desc=\"Bonus for equipment\">{bonus}</Bonus>")}else{String::new()};ensure!(died_at>0 && died_at<=8_640_000_000_000,"invalid recorded death time");Ok(format!("<Fame><Char id=\"{}\"><Account><Name>{}</Name></Account><ObjectType>{}</ObjectType><Level>{}</Level><Tex1>{}</Tex1><Tex2>{}</Tex2></Char><CreatedOn>{}</CreatedOn><KilledBy>{}</KilledBy><TotalFame>{}</TotalFame><Shots>{}</Shots><ShotsThatDamage>{}</ShotsThatDamage><TilesUncovered>{}</TilesUncovered><MonsterKills>{}</MonsterKills><GodKills>0</GodKills><OryxKills>0</OryxKills><QuestsCompleted>{}</QuestsCompleted><PirateCavesCompleted>{}</PirateCavesCompleted><UndeadLairsCompleted>{}</UndeadLairsCompleted><AbyssOfDemonsCompleted>{}</AbyssOfDemonsCompleted><SnakePitsCompleted>{}</SnakePitsCompleted><SpiderDensCompleted>{}</SpiderDensCompleted><SpriteWorldsCompleted>{}</SpriteWorldsCompleted><TombsCompleted>0</TombsCompleted><LevelUpAssists>0</LevelUpAssists><BaseFame>{}</BaseFame>{}</Fame>",c.id,xml_escape(&c.name),c.class,c.level,c.tex1,c.tex2,died_at,xml_escape(&killer),total,c.shots,c.hits,c.tiles,c.kills,c.quests,c.pirate_caves,c.dungeons[2],c.abysses,c.dungeons[0],c.spider_dens,c.dungeons[1],c.fame,equipment))}).await
    }
    #[cfg(test)]
    pub async fn rename(&self, account: i32, name: String) -> Result<()> {
        ensure!(
            (3..=10).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_alphabetic()),
            "names must be 3–10 letters"
        );
        self.call(move |c| {
            let n = c.execute(
                "UPDATE accounts SET name=?2 WHERE id=?1",
                params![account, name],
            )?;
            ensure!(n == 1, "account missing");
            Ok(())
        })
        .await
    }
}
fn read_presentation(
    db: &Connection,
    account: i32,
    legacy_base: [i32; 8],
    classes: &BTreeMap<u16, Class>,
) -> Result<(AccountPresentation, i64)> {
    let (balance, earned, credits): (i64, i64, i32) = db
        .query_row(
            "SELECT balance,earned,credits FROM account_fame JOIN accounts ON accounts.id=account_fame.account WHERE account=?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .context("read account fame wallet")?;
    ensure!(credits >= 0, "invalid account gold balance");
    let fame = i32::try_from(balance).context("account fame exceeds wire range")?;
    ensure!(
        fame >= 0 && earned >= balance && earned <= i64::from(i32::MAX),
        "invalid account fame wallet"
    );
    let mut best_fame = BTreeMap::<u16, i32>::new();
    // SQL owns live identities; a committed death's immutable snapshot remains authoritative.
    let mut q = db.prepare("SELECT c.id,CASE WHEN d.character IS NULL THEN json_set(c.stats,'$.id',c.id,'$.account',c.account) ELSE d.snapshot END,d.character IS NOT NULL FROM characters c LEFT JOIN deaths d ON d.character=c.id WHERE c.account=?1")?;
    for row in q.query_map([account], |r| {
        Ok((
            r.get::<_, i32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, bool>(2)?,
        ))
    })? {
        let (id, raw, committed_death) = row?;
        let ch = decode_character(&raw, legacy_base)
            .with_context(|| format!("decode saved class record {id}"))?;
        let class = classes
            .get(&ch.class)
            .with_context(|| format!("unknown class in saved class record {id}"))?;
        ensure!(
            ch.id == id
                && ch.account == account
                && (!committed_death || (!ch.alive && ch.hp == 0))
                && ch.fame >= 0
                && (1..=20).contains(&ch.level)
                && ch.items.len() == 12
                && ch
                    .base
                    .iter()
                    .zip(class.caps)
                    .all(|(value, cap)| *value >= 0 && *value <= cap),
            "invalid saved class record {id}"
        );
        let best = best_fame.entry(ch.class).or_default();
        *best = (*best).max(ch.fame);
    }
    Ok((
        AccountPresentation {
            guild: guilds::membership(db, account)?,
            credits,
            fame,
            best_fame,
        },
        earned,
    ))
}
pub fn name_chosen(account: i32, name: &str) -> bool {
    name != format!("Guest{account}")
}
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
impl Account {
    pub fn xml(&self, news: &str, usage: f64) -> String {
        let mut chars = String::new();
        for c in &self.characters {
            chars += &format!(
                "<Char id=\"{}\"><ObjectType>{}</ObjectType><Level>{}</Level><Exp>{}</Exp><CurrentFame>{}</CurrentFame><Tex1>{}</Tex1><Tex2>{}</Tex2><Equipment>{}</Equipment><MaxHitPoints>{}</MaxHitPoints><HitPoints>{}</HitPoints><MaxMagicPoints>{}</MaxMagicPoints><MagicPoints>{}</MagicPoints><Attack>{}</Attack><Defense>{}</Defense><Speed>{}</Speed><Dexterity>{}</Dexterity><HpRegen>{}</HpRegen><MpRegen>{}</MpRegen></Char>",
                c.id,
                c.class,
                c.level,
                c.exp,
                c.fame,
                c.tex1,
                c.tex2,
                c.items
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                c.base[0],
                c.hp,
                c.base[1],
                c.mp,
                c.base[2],
                c.base[3],
                c.base[4],
                c.base[5],
                c.base[6],
                c.base[7]
            );
        }
        let chosen = if name_chosen(self.id, &self.name) {
            "<NameChosen/>"
        } else {
            ""
        };
        let guild = self
            .guild
            .as_ref()
            .map(|g| {
                format!(
                    "<Guild id=\"{}\"><Name>{}</Name><Rank>{}</Rank></Guild>",
                    g.id,
                    xml_escape(&g.name),
                    g.rank
                )
            })
            .unwrap_or_default();
        let records: String = self.best_levels.iter().map(|(class, level)| {
            let fame = self.best_fame.get(class).copied().unwrap_or(0);
            format!("<ClassStats objectType=\"{class}\"><BestLevel>{level}</BestLevel><BestFame>{fame}</BestFame></ClassStats>")
        }).collect();
        format!(
            "<Chars nextCharId=\"{}\" maxNumChars=\"{}\">{chars}<Account><AccountId>{}</AccountId><Name>{}</Name>{chosen}<Stats><Fame>{}</Fame><TotalFame>{}</TotalFame>{records}</Stats><Credits>{}</Credits><NextCharSlotPrice>{}</NextCharSlotPrice>{guild}</Account><Servers><Server><Name>Local development</Name><DNS>127.0.0.1</DNS><Lat>0</Lat><Long>0</Long><Usage>{usage}</Usage></Server></Servers><News>{news}</News></Chars>",
            self.next_id,
            self.character_slots,
            self.id,
            xml_escape(&self.name),
            self.fame,
            self.earned_fame,
            self.credits,
            self.next_slot_price
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn ground_stat_potions_preserve_backpacks_and_commit_vault_sources_atomically() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let account = s.list("ground-stat-potion".into()).await.unwrap();
        let mut ch = s
            .create(account.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let mut vault = s.vault(account.id, 0).await.unwrap();
        vault.items[0] = 0xa1f;
        ch.revision = s
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![vault],
            )
            .await
            .unwrap()[0];
        let source = s.vault(account.id, 0).await.unwrap();
        let mut consumed = source.clone();
        consumed.items[0] = -1;
        let mut stale = consumed.clone();
        stale.revision += 1;
        assert!(
            s.consume_stat(ch.clone(), None, 2, 1, vec![stale])
                .await
                .is_err()
        );
        let saved = s.load(account.id, ch.id).await.unwrap();
        assert_eq!((saved.base, saved.revision), (ch.base, ch.revision));
        assert_eq!(s.vault(account.id, 0).await.unwrap().items, source.items);
        let revision = s
            .consume_stat(ch.clone(), None, 2, 1, vec![consumed.clone()])
            .await
            .unwrap();
        assert!(
            s.consume_stat(ch.clone(), None, 2, 1, vec![consumed])
                .await
                .is_err()
        );
        let saved = s.load(account.id, ch.id).await.unwrap();
        assert_eq!(saved.items, ch.items);
        assert_eq!(saved.base[2], ch.base[2] + 1);
        assert_eq!(saved.revision, revision);
        let after = s.vault(account.id, 0).await.unwrap();
        assert_eq!(after.items[0], -1);
        assert_eq!(after.revision, source.revision + 1);
        s.consume_stat(saved.clone(), None, 2, 1, Vec::new())
            .await
            .unwrap();
        let ground = s.load(account.id, ch.id).await.unwrap();
        assert_eq!(ground.items, saved.items);
        assert_eq!(ground.base[2], saved.base[2] + 1);
    }
    #[tokio::test]
    async fn permanent_potions_commit_items_and_base_together_and_reject_replay() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let account = s.list("stat-potions".into()).await.unwrap();
        let mut ch = s
            .create(account.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        for (slot, index, kind) in [
            (4, 2, 0xa1f),
            (5, 3, 0xa20),
            (6, 4, 0xa21),
            (7, 5, 0xa4c),
            (8, 6, 0xa34),
            (9, 7, 0xa35),
        ] {
            ch.items[slot] = kind;
            ch.revision = s
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account: ch.account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    Vec::new(),
                )
                .await
                .unwrap()[0];
            let before = ch.base;
            let revision = s
                .consume_stat(ch.clone(), Some(slot), index, 1, Vec::new())
                .await
                .unwrap();
            assert!(
                s.consume_stat(ch.clone(), Some(slot), index, 1, Vec::new())
                    .await
                    .is_err()
            );
            let mut foreign = ch.clone();
            foreign.account += 1;
            assert!(
                s.consume_stat(foreign, Some(slot), index, 1, Vec::new())
                    .await
                    .is_err()
            );
            ch = s.load(ch.account, ch.id).await.unwrap();
            assert_eq!(ch.revision, revision);
            assert_eq!(ch.items[slot], -1);
            let mut expected = before;
            expected[index] += 1;
            assert_eq!(ch.base, expected);
        }
        let cap = content.classes[&WIZARD].caps[4];
        ch.base[4] = cap;
        s.progress(ch.clone()).await.unwrap();
        ch.items[4] = 0xa21;
        ch.revision = s
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                Vec::new(),
            )
            .await
            .unwrap()[0];
        assert!(
            s.consume_stat(ch.clone(), Some(4), 4, 1, Vec::new())
                .await
                .is_err()
        );
        assert!(
            s.consume_stat(ch.clone(), Some(4), 4, 2, Vec::new())
                .await
                .is_err()
        );
        let saved = s.load(ch.account, ch.id).await.unwrap();
        assert_eq!(
            (saved.base[4], saved.items[4], saved.revision),
            (cap, 0xa21, ch.revision)
        );
        let mut dead = ch.clone();
        dead.alive = false;
        dead.hp = 0;
        s.death(dead, "potion regression".into()).await.unwrap();
        assert!(s.consume_stat(ch, Some(4), 4, 1, Vec::new()).await.is_err());
    }
    #[tokio::test]
    async fn permanent_potion_conflicts_with_trade_and_vault_without_duplication() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        for (index, kind) in [(4, 0xa21), (5, 0xa4c), (7, 0xa35)] {
            for vault_exchange in [false, true] {
                for potion_first in [false, true] {
                    let guid = format!("potion-race-{index}-{vault_exchange}-{potion_first}");
                    let owner = s.list(guid.clone()).await.unwrap();
                    let recipient = s.list(format!("{guid}-recipient")).await.unwrap();
                    let mut source = s
                        .create(owner.id, content.get(WIZARD).unwrap())
                        .await
                        .unwrap();
                    let target = s
                        .create(recipient.id, content.get(WIZARD).unwrap())
                        .await
                        .unwrap();
                    source.items[4] = kind;
                    source.revision = s
                        .exchange(
                            vec![InventoryWrite {
                                id: source.id,
                                account: source.account,
                                revision: source.revision,
                                items: source.items.clone(),
                            }],
                            Vec::new(),
                        )
                        .await
                        .unwrap()[0];
                    let mut remaining = source.items.clone();
                    remaining[4] = -1;
                    let mut writes = vec![InventoryWrite {
                        id: source.id,
                        account: source.account,
                        revision: source.revision,
                        items: remaining,
                    }];
                    let vault = if vault_exchange {
                        let mut vault = s.vault(owner.id, 0).await.unwrap();
                        vault.items[0] = kind;
                        Some(vault)
                    } else {
                        let mut items = target.items.clone();
                        items[4] = kind;
                        writes.push(InventoryWrite {
                            id: target.id,
                            account: target.account,
                            revision: target.revision,
                            items,
                        });
                        None
                    };
                    if potion_first {
                        s.consume_stat(source.clone(), Some(4), index, 1, Vec::new())
                            .await
                            .unwrap();
                        assert!(
                            s.exchange(writes, vault.into_iter().collect())
                                .await
                                .is_err()
                        );
                    } else {
                        s.exchange(writes, vault.into_iter().collect())
                            .await
                            .unwrap();
                        assert!(
                            s.consume_stat(source.clone(), Some(4), index, 1, Vec::new())
                                .await
                                .is_err()
                        );
                    }
                    let saved = s.load(owner.id, source.id).await.unwrap();
                    let other = s.load(recipient.id, target.id).await.unwrap();
                    let chest = s.vault(owner.id, 0).await.unwrap();
                    assert_eq!(
                        saved.base[index],
                        source.base[index] + i32::from(potion_first)
                    );
                    assert_eq!(saved.items[4], -1);
                    let count = saved
                        .items
                        .iter()
                        .chain(&other.items)
                        .chain(&chest.items)
                        .filter(|&&item| item == kind)
                        .count();
                    assert_eq!(count, usize::from(!potion_first));
                    assert_eq!(
                        other.items[4],
                        if !potion_first && !vault_exchange {
                            kind
                        } else {
                            target.items[4]
                        }
                    );
                    assert_eq!(
                        chest.items[0],
                        if !potion_first && vault_exchange {
                            kind
                        } else {
                            -1
                        }
                    );
                }
            }
        }
    }
    #[tokio::test]
    async fn prepared_priest_does_not_grant_unlocks_or_replace_a_character() {
        let content = Content::load(Path::new("data")).unwrap();
        let store = Store::open(Path::new(":memory:"), &content).unwrap();
        let account = store.list("prepared-priest-only".into()).await.unwrap();
        assert!(
            store
                .create(account.id, content.get(PRIEST).unwrap())
                .await
                .is_err()
        );
        let priest = store
            .create_prepared(
                account.id,
                content.get(PRIEST).unwrap(),
                Some("Juniper".into()),
            )
            .await
            .unwrap();
        assert_eq!((priest.class, priest.name.as_str()), (PRIEST, "Juniper"));
        assert!(
            store
                .create(account.id, content.get(PRIEST).unwrap())
                .await
                .is_err()
        );
        assert!(
            store
                .create_prepared(account.id, content.get(WIZARD).unwrap(), None)
                .await
                .is_err()
        );
        assert_eq!(
            store
                .list("prepared-priest-only".into())
                .await
                .unwrap()
                .characters
                .len(),
            1
        );
        let mut dead = priest.clone();
        dead.hp = 0;
        dead.alive = false;
        store
            .death(dead, "prepared retry regression".into())
            .await
            .unwrap();
        let replacement = store
            .create_prepared(account.id, content.get(PRIEST).unwrap(), None)
            .await
            .unwrap();
        assert_ne!(replacement.id, priest.id);
        assert!(
            !store
                .character_alive(priest.id, "prepared-priest-only".into())
                .await
                .unwrap()
        );
        assert!(store.load(account.id, priest.id).await.is_err());
    }
    #[tokio::test]
    async fn cooperative_progress_batch_rolls_back_every_stale_participant() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let mut characters = Vec::new();
        for guid in ["progress-a", "progress-b"] {
            let account = s.list(guid.into()).await.unwrap();
            let mut ch = s
                .create(account.id, content.get(WIZARD).unwrap())
                .await
                .unwrap();
            ch.exp = 49;
            ch.pirate_caves = 1;
            ch.dungeons = [1, 2, 3];
            ch.abysses = 4;
            ch.spider_dens = 5;
            characters.push(ch);
        }
        characters[1].revision = 1;
        assert!(s.progress_batch(characters.clone()).await.is_err());
        for ch in &characters {
            let saved = s.load(ch.account, ch.id).await.unwrap();
            assert_eq!(saved.exp, 0);
            assert_eq!(saved.pirate_caves, 0);
            assert_eq!(saved.dungeons, [0; 3]);
            assert_eq!(saved.abysses, 0);
            assert_eq!(saved.spider_dens, 0);
        }
        characters[1].revision = 0;
        s.progress_batch(characters.clone()).await.unwrap();
        for ch in &characters {
            let mut saved = s.load(ch.account, ch.id).await.unwrap();
            assert_eq!(saved.exp, 49);
            assert_eq!(saved.pirate_caves, 1);
            assert_eq!(saved.dungeons, [1, 2, 3]);
            assert_eq!(saved.abysses, 4);
            assert_eq!(saved.spider_dens, 5);
            saved.alive = false;
            saved.hp = 0;
            s.death(saved, "test clear snapshot".into()).await.unwrap();
            let fame = s.fame(ch.account, ch.id).await.unwrap();
            for counter in [
                "<PirateCavesCompleted>1</PirateCavesCompleted>",
                "<SnakePitsCompleted>1</SnakePitsCompleted>",
                "<SpriteWorldsCompleted>2</SpriteWorldsCompleted>",
                "<UndeadLairsCompleted>3</UndeadLairsCompleted>",
                "<AbyssOfDemonsCompleted>4</AbyssOfDemonsCompleted>",
                "<SpiderDensCompleted>5</SpiderDensCompleted>",
            ] {
                assert!(fame.contains(counter));
            }
        }
    }
    #[tokio::test]
    async fn experience_thresholds_growth_caps_and_level_twenty_boundary() {
        use rand::SeedableRng;
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let account = s.list("experience".into()).await.unwrap();
        let mut ch = s
            .create(account.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        assert_eq!(level_start(20).unwrap(), 18050);
        assert_eq!(experience_goal(20).unwrap(), 1950);
        assert!(level_start(0).is_err());
        assert!(!ch.gain_experience(&content, 49, &mut rng).unwrap());
        ch.hp = 1;
        ch.mp = 1;
        assert!(ch.gain_experience(&content, 1, &mut rng).unwrap());
        assert_eq!(ch.level, 2);
        for (i, (min, max)) in content.classes[&WIZARD].growth.iter().enumerate() {
            let increase = ch.base[i] - content.classes[&WIZARD].base[i];
            assert!((*min..=*max).contains(&increase));
        }
        assert_eq!(ch.hp, ch.effective(&content).unwrap()[0]);
        assert_eq!(ch.mp, ch.effective(&content).unwrap()[1]);
        assert!(ch.gain_experience(&content, 150, &mut rng).unwrap());
        assert_eq!(ch.level, 3);
        let amount = 18050 - ch.exp;
        assert!(ch.gain_experience(&content, amount, &mut rng).unwrap());
        assert_eq!(ch.level, 20);
        assert_eq!(ch.fame, 18);
        assert!(
            ch.base
                .iter()
                .zip(content.classes[&WIZARD].caps)
                .all(|(v, cap)| *v <= cap)
        );
        let base = ch.base;
        assert!(!ch.gain_experience(&content, 195, &mut rng).unwrap());
        assert_eq!(ch.base, base);
        assert_eq!(ch.exp, 18245);
        assert!(ch.gain_experience(&content, -1, &mut rng).is_err());
        assert!(ch.gain_experience(&content, i32::MAX, &mut rng).is_err());
        assert_eq!(ch.exp, 18245);
        s.progress(ch.clone()).await.unwrap();
        let loaded = s.load(ch.account, ch.id).await.unwrap();
        assert_eq!(loaded.level, 20);
        assert_eq!(loaded.exp, ch.exp);
        assert_eq!(loaded.base, ch.base);
    }
    #[tokio::test]
    async fn vault_exchange_is_atomic_private_and_survives_death() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let owner = s.list("vault-owner".into()).await.unwrap();
        let other = s.list("vault-other".into()).await.unwrap();
        let mut ch = s
            .create(owner.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let mut chest = s.vault(owner.id, 0).await.unwrap();
        assert_eq!(chest.items, vec![-1; 8]);
        let original = ch.items.clone();
        chest.items[0] = ch.items[4];
        let mut write = InventoryWrite {
            id: ch.id,
            account: ch.account,
            revision: 0,
            items: ch.items.clone(),
        };
        write.items[4] = -1;
        chest.revision = 1;
        assert!(
            s.exchange(vec![write.clone()], vec![chest.clone()])
                .await
                .is_err()
        );
        assert_eq!(s.load(owner.id, ch.id).await.unwrap().items, original);
        assert_eq!(s.vault(owner.id, 0).await.unwrap().items, vec![-1; 8]);
        chest.revision = 0;
        let mut foreign = chest.clone();
        foreign.account = other.id;
        assert!(
            s.exchange(vec![write.clone()], vec![foreign])
                .await
                .is_err()
        );
        assert_eq!(
            s.exchange(vec![write], vec![chest.clone()]).await.unwrap(),
            vec![1]
        );
        ch = s.load(owner.id, ch.id).await.unwrap();
        assert_eq!(ch.items[4], -1);
        assert_eq!(s.vault(owner.id, 0).await.unwrap().items[0], original[4]);
        assert_eq!(s.vault(other.id, 0).await.unwrap().items, vec![-1; 8]);
        ch.hp = 0;
        ch.alive = false;
        let dead_id = ch.id;
        s.death(ch, "vault test".into()).await.unwrap();
        let replacement = s
            .create(owner.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        assert_ne!(replacement.id, dead_id);
        assert_eq!(s.vault(owner.id, 0).await.unwrap().items[0], original[4]);
        let mut invalid = s.vault(owner.id, 0).await.unwrap();
        invalid.items.push(-1);
        let write = InventoryWrite {
            id: replacement.id,
            account: owner.id,
            revision: 0,
            items: replacement.items,
        };
        assert!(s.exchange(vec![write], vec![invalid]).await.is_err());
    }
    #[tokio::test]
    async fn qualifying_class_records_survive_death_and_match_unlock_requirements() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let account = s.list("class-records".into()).await.unwrap();
        assert!(content.unlocked(WIZARD, &account.best_levels).unwrap());
        assert!(!content.unlocked(0x0310, &account.best_levels).unwrap());
        let mut ch = s
            .create(account.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.level = 5;
        ch.exp = 800;
        s.progress(ch.clone()).await.unwrap();
        ch.hp = 0;
        ch.alive = false;
        s.death(ch, "record test".into()).await.unwrap();
        let account = s.list("class-records".into()).await.unwrap();
        assert_eq!(account.best_levels.get(&WIZARD), Some(&5));
        assert!(account.characters.is_empty());
        assert!(content.unlocked(0x0310, &account.best_levels).unwrap());
        assert!(!content.unlocked(0x0300, &account.best_levels).unwrap());
        assert!(!content.unlocked(0x031f, &account.best_levels).unwrap());
        let xml = account.xml("", 0.0);
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let record = parsed
            .descendants()
            .find(|n| n.has_tag_name("ClassStats"))
            .unwrap();
        assert_eq!(record.attribute("objectType"), Some("782"));
        assert_eq!(
            record
                .children()
                .find(|n| n.has_tag_name("BestLevel"))
                .unwrap()
                .text(),
            Some("5")
        );
    }
    #[tokio::test]
    async fn priest_creation_is_locked_transactionally_then_uses_selected_stats_and_persistence() {
        let c = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &c).unwrap();
        let a = s.list("priest-unlock".into()).await.unwrap();
        assert!(s.create(a.id, c.get(PRIEST).unwrap()).await.is_err());
        let mut wizard = s.create(a.id, c.get(WIZARD).unwrap()).await.unwrap();
        wizard.level = 4;
        wizard.exp = level_start(4).unwrap();
        s.progress(wizard.clone()).await.unwrap();
        wizard.hp = 0;
        wizard.alive = false;
        s.death(wizard, "ordinary death fixture".into())
            .await
            .unwrap();
        assert!(s.create(a.id, c.get(PRIEST).unwrap()).await.is_err());
        let mut wizard = s.create(a.id, c.get(WIZARD).unwrap()).await.unwrap();
        wizard.level = 5;
        wizard.exp = level_start(5).unwrap();
        s.progress(wizard.clone()).await.unwrap();
        assert!(s.create(a.id, c.get(PRIEST).unwrap()).await.is_err());
        wizard.hp = 0;
        wizard.alive = false;
        s.death(wizard, "ordinary death fixture".into())
            .await
            .unwrap();
        let priest = s.create(a.id, c.get(PRIEST).unwrap()).await.unwrap();
        assert_eq!(priest.class, PRIEST);
        assert_eq!(priest.base, c.classes[&PRIEST].base);
        assert_eq!(priest.items, c.get(PRIEST).unwrap().equipment);
        assert_eq!(priest.effective(&c).unwrap(), priest.base);
        let mut next = priest.clone();
        assert!(
            next.gain_experience(&c, 800, &mut rand::thread_rng())
                .unwrap()
        );
        s.progress(next.clone()).await.unwrap();
        let loaded = s.load(a.id, priest.id).await.unwrap();
        assert_eq!(
            (loaded.class, loaded.level, loaded.base, loaded.items),
            (PRIEST, next.level, next.base, next.items)
        );
        assert!(s.create(a.id, c.get(0x0320).unwrap()).await.is_err());
    }
    #[tokio::test]
    async fn archer_requires_priest_level_five_and_preserves_starter_gear_and_save() {
        let c = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &c).unwrap();
        let a = s.list("archer-unlock".into()).await.unwrap();
        assert!(s.create(a.id, c.get(ARCHER).unwrap()).await.is_err());
        let mut priest = s
            .create_prepared(a.id, c.get(PRIEST).unwrap(), None)
            .await
            .unwrap();
        priest.level = 4;
        priest.exp = level_start(4).unwrap();
        s.progress(priest.clone()).await.unwrap();
        assert!(
            s.create(a.id, c.get(ARCHER).unwrap())
                .await
                .unwrap_err()
                .to_string()
                .contains("locked")
        );
        priest.level = 5;
        priest.exp = level_start(5).unwrap();
        s.progress(priest.clone()).await.unwrap();
        assert!(s.create(a.id, c.get(ARCHER).unwrap()).await.is_err());
        priest.hp = 0;
        priest.alive = false;
        s.death(priest, "ordinary death fixture".into())
            .await
            .unwrap();
        let archer = s.create(a.id, c.get(ARCHER).unwrap()).await.unwrap();
        assert_eq!(archer.base, c.classes[&ARCHER].base);
        assert_eq!(archer.items, c.get(ARCHER).unwrap().equipment);
        let loaded = s.load(a.id, archer.id).await.unwrap();
        assert_eq!(
            (loaded.class, loaded.level, loaded.mp, loaded.items),
            (ARCHER, 1, archer.mp, archer.items)
        );
        assert!(s.create(a.id, c.get(0x0320).unwrap()).await.is_err());
    }
    #[tokio::test]
    async fn all_classes_preserve_normal_unlock_death_starter_and_save_rules() {
        let c = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &c).unwrap();
        for (class, requirements) in [
            (ROGUE, vec![(ARCHER, 5)]),
            (WARRIOR, vec![(ROGUE, 5)]),
            (KNIGHT, vec![(WARRIOR, 20)]),
            (PALADIN, vec![(KNIGHT, 20), (PRIEST, 20)]),
            (0x320, vec![(ROGUE, 20), (WIZARD, 20)]),
            (0x321, vec![(WIZARD, 20), (PRIEST, 20)]),
            (0x322, vec![(ROGUE, 20), (ARCHER, 20)]),
            (0x323, vec![(0x322, 20), (0x321, 20)]),
            (0x324, vec![(0x320, 20), (PALADIN, 20)]),
            (0x325, vec![(0x321, 20), (0x320, 20)]),
        ] {
            assert_eq!(c.classes[&class].requires, requirements);
            let a = s.list(format!("class-unlock-{class}")).await.unwrap();
            assert!(
                s.create(a.id, c.get(class).unwrap())
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("locked")
            );
            for (prior, level) in requirements {
                let mut previous = s
                    .create_prepared(a.id, c.get(prior).unwrap(), None)
                    .await
                    .unwrap();
                previous.level = level - 1;
                previous.exp = level_start(level - 1).unwrap();
                s.progress(previous.clone()).await.unwrap();
                assert!(
                    s.create(a.id, c.get(class).unwrap())
                        .await
                        .unwrap_err()
                        .to_string()
                        .contains("locked")
                );
                previous.level = level;
                previous.exp = level_start(level).unwrap();
                s.progress(previous.clone()).await.unwrap();
                assert!(s.create(a.id, c.get(class).unwrap()).await.is_err());
                previous.alive = false;
                previous.hp = 0;
                s.death(previous, "ordinary death fixture".into())
                    .await
                    .unwrap();
            }
            let created = s.create(a.id, c.get(class).unwrap()).await.unwrap();
            let loaded = s.load(a.id, created.id).await.unwrap();
            assert_eq!(
                (loaded.class, loaded.level, loaded.base, loaded.items),
                (
                    class,
                    1,
                    c.classes[&class].base,
                    c.get(class).unwrap().equipment.clone()
                )
            );
        }
    }
    #[tokio::test]
    async fn equipment_derives_stats_without_stacking_or_backpack_bonuses() {
        let content = Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let account = s.list("effective-stats".into()).await.unwrap();
        let mut ch = s
            .create(account.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        assert_eq!(ch.effective(&content).unwrap(), ch.base);
        ch.items[3] = 0xa4f;
        let equipped = ch.effective(&content).unwrap();
        assert_eq!(equipped[3], ch.base[3] + 1);
        assert_eq!(ch.effective(&content).unwrap(), equipped);
        ch.items[3] = -1;
        ch.items[4] = 0xa4f;
        assert_eq!(ch.effective(&content).unwrap(), ch.base);
        ch.items[3] = ch.items[0];
        assert!(ch.effective(&content).is_err());
        ch.items[3] = -1;
        ch.base[0] = content.classes[&WIZARD].caps[0] + 1;
        assert!(ch.effective(&content).is_err());
    }
    #[tokio::test]
    async fn prepared_unlock_record_selects_priest_only_after_permanent_death() {
        let content = Content::load(Path::new("data")).unwrap();
        let store = Store::open(Path::new(":memory:"), &content).unwrap();
        let mut wizard = store
            .shore_character("Bayleaf".into(), "Triceps".into(), &content, true)
            .await
            .unwrap();
        assert_eq!(wizard.class, WIZARD);
        wizard.level = 5;
        wizard.exp = level_start(5).unwrap();
        store.progress(wizard.clone()).await.unwrap();
        let living = store
            .shore_character("Bayleaf".into(), "Triceps".into(), &content, true)
            .await
            .unwrap();
        assert_eq!(living.id, wizard.id);
        assert_eq!(living.class, WIZARD);
        let dead_id = wizard.id;
        wizard.hp = 0;
        wizard.alive = false;
        store
            .death(wizard.clone(), "prepared lifecycle fixture".into())
            .await
            .unwrap();
        let replacement = store
            .shore_character("Bayleaf".into(), "Triceps".into(), &content, true)
            .await
            .unwrap();
        assert_eq!(replacement.account, wizard.account);
        assert_ne!(replacement.id, dead_id);
        assert_eq!(replacement.class, PRIEST);
        assert_eq!(replacement.level, 1);
        assert_eq!(replacement.exp, 0);
        assert_eq!(replacement.items, content.get(PRIEST).unwrap().equipment);
        assert!(store.load(wizard.account, dead_id).await.is_err());
        let snapshot: String = store
            .call(move |db| {
                Ok(db.query_row(
                    "SELECT snapshot FROM deaths WHERE character=?1",
                    [dead_id],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        let dead: Character = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(dead.id, dead_id);
        assert!(!dead.alive);
        assert_eq!(dead.items, wizard.items);
    }

    #[tokio::test]
    async fn bot_accounts_resume_saves_and_replace_without_reviving_the_dead() {
        let content = Content::load(Path::new("data")).unwrap();
        let path = std::env::temp_dir().join(format!(
            "openrotmg-shore-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let store = Store::open(&path, &content).unwrap();
        let wizard = content.get(WIZARD).unwrap();
        let human = store.list("shore-human".into()).await.unwrap();
        store.rename(human.id, "Triceps".into()).await.unwrap();
        let mut bot = store
            .shore_character("Bayleaf".into(), "Bayleaf".into(), &content, true)
            .await
            .unwrap();
        assert_ne!(bot.account, human.id);
        let identity = (bot.account, bot.id);
        assert_eq!(bot.items, wizard.equipment);
        bot.exp = 12;
        bot.shots = 4;
        bot.hp = 42;
        store.progress(bot.clone()).await.unwrap();
        let reopened = Store::open(&path, &content).unwrap();
        let resumed = reopened
            .shore_character("Bayleaf".into(), "Triceps".into(), &content, true)
            .await
            .unwrap();
        assert_eq!((resumed.account, resumed.id), identity);
        assert_eq!(resumed.name, "Triceps");
        assert_eq!(resumed.items, bot.items);
        assert_eq!(resumed.revision, bot.revision);
        assert_eq!(reopened.list_id(human.id).await.unwrap().name, "Triceps");
        assert_eq!(resumed.exp, 12);
        assert_eq!(resumed.hp, 42);
        assert_eq!(resumed.shots, 4);
        bot.hp = 0;
        bot.alive = false;
        store.death(bot, "test enemy".into()).await.unwrap();
        let replacement = reopened
            .shore_character("Bayleaf".into(), "Triceps".into(), &content, true)
            .await
            .unwrap();
        assert_eq!(replacement.account, identity.0);
        assert_ne!(replacement.id, identity.1);
        assert_eq!(replacement.class, WIZARD);
        assert_eq!(replacement.level, 1);
        assert_eq!(replacement.exp, 0);
        assert_eq!(replacement.items, wizard.equipment);
        let resumed = store
            .shore_character("Bayleaf".into(), "Triceps".into(), &content, true)
            .await
            .unwrap();
        assert_eq!(resumed.id, replacement.id);
        assert_eq!(resumed.name, "Triceps");
        assert_eq!(
            store
                .call(|db| Ok(db.query_row(
                    "SELECT count(*) FROM shore_bots WHERE name=\'Bayleaf\'",
                    [],
                    |r| r.get::<_, i32>(0)
                )?))
                .await
                .unwrap(),
            1
        );
        assert!(store.load(identity.0, identity.1).await.is_err());
        let records: i32 = store
            .call(move |db| {
                Ok(db.query_row(
                    "SELECT count(*) FROM characters WHERE account=?1",
                    [identity.0],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(records, 2);
        drop(reopened);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn legacy_metadata_migration_preserves_immutable_death_bytes() {
        let content = Content::load(Path::new("data")).unwrap();
        let path = std::env::temp_dir().join(format!(
            "openrotmg-migration-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let s = Store::open(&path, &content).unwrap();
        let mut characters = Vec::new();
        for guid in ["legacy-live", "legacy-dead"] {
            let account = s.list(guid.into()).await.unwrap();
            characters.push(
                s.create(account.id, content.get(WIZARD).unwrap())
                    .await
                    .unwrap(),
            );
        }
        let mut dead = characters[1].clone();
        dead.hp = 0;
        dead.alive = false;
        s.death(dead.clone(), "legacy pirate".into()).await.unwrap();
        let legacy = |ch: &Character| {
            let mut value = serde_json::to_value(ch).unwrap();
            for key in ["schema", "class", "base", "pirate_caves"] {
                value.as_object_mut().unwrap().remove(key);
            }
            serde_json::to_string(&value).unwrap()
        };
        let live_raw = legacy(&characters[0]);
        let dead_raw = legacy(&dead);
        let expected = dead_raw.clone();
        let live_id = characters[0].id;
        let dead_id = dead.id;
        s.call(move |db| {
            db.execute(
                "UPDATE characters SET stats=?2 WHERE id=?1",
                params![live_id, live_raw],
            )?;
            db.execute(
                "UPDATE characters SET stats=?2 WHERE id=?1",
                params![dead_id, dead_raw],
            )?;
            db.execute(
                "UPDATE deaths SET snapshot=?2 WHERE character=?1",
                params![dead_id, dead_raw],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        drop(s);
        let s = Store::open(&path, &content).unwrap();
        let migrated = s.load(characters[0].account, live_id).await.unwrap();
        assert_eq!(migrated.schema, 1);
        assert_eq!(migrated.class, WIZARD);
        assert_eq!(migrated.base, content.classes[&WIZARD].base);
        assert!(
            s.fame(dead.account, dead.id)
                .await
                .unwrap()
                .contains("legacy pirate")
        );
        let unchanged: String = s
            .call(move |db| {
                Ok(db.query_row(
                    "SELECT snapshot FROM deaths WHERE character=?1",
                    [dead_id],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(unchanged, expected);
        assert!(s.load(dead.account, dead.id).await.is_err());
        assert!(decode_character("{\"schema\":2}", migrated.base).is_err());
        assert!(decode_character("{\"level\":1,\"exp\":0,\"class\":784}", migrated.base).is_err());
        assert!(decode_character("{\"level\":2,\"exp\":50}", migrated.base).is_err());
        drop(s);
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn exchange_is_atomic_and_rejects_stale_or_foreign_owners() {
        let content = crate::content::Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &content).unwrap();
        let mut characters = Vec::new();
        for guid in ["exchange-a", "exchange-b"] {
            let account = s.list(guid.into()).await.unwrap();
            characters.push(
                s.create(account.id, content.get(crate::content::WIZARD).unwrap())
                    .await
                    .unwrap(),
            );
        }
        let mut writes: Vec<_> = characters
            .iter()
            .map(|ch| InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            })
            .collect();
        writes[0].items[4] = -1;
        writes[1].items[5] = characters[0].items[4];
        writes[1].revision = 1;
        assert!(s.exchange(writes.clone(), Vec::new()).await.is_err());
        for ch in &characters {
            let loaded = s.load(ch.account, ch.id).await.unwrap();
            assert_eq!(loaded.items, ch.items);
            assert_eq!(loaded.revision, 0);
        }
        writes[1].revision = 0;
        let mut foreign = writes.clone();
        foreign[1].id = characters[0].id;
        assert!(s.exchange(foreign, Vec::new()).await.is_err());
        assert_eq!(
            s.exchange(writes.clone(), Vec::new()).await.unwrap(),
            vec![1, 1]
        );
        for (ch, write) in characters.iter().zip(&writes) {
            let loaded = s.load(ch.account, ch.id).await.unwrap();
            assert_eq!(loaded.items, write.items);
            assert_eq!(loaded.revision, 1);
        }
        assert!(s.exchange(writes, Vec::new()).await.is_err());
        assert!(s.progress(characters[0].clone()).await.is_err());
        let mut current = s
            .load(characters[0].account, characters[0].id)
            .await
            .unwrap();
        current.exp = 49;
        s.progress(current.clone()).await.unwrap();
        assert_eq!(s.load(current.account, current.id).await.unwrap().exp, 49);
    }
    #[tokio::test]
    async fn inventory_is_versioned_and_death_is_permanent() {
        let c = crate::content::Content::load(Path::new("data")).unwrap();
        let s = Store::open(Path::new(":memory:"), &c).unwrap();
        let a = s.list("test-account".into()).await.unwrap();
        let mut ch = s
            .create(a.id, c.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let mut inv = ch.items.clone();
        inv[4] = -1;
        let write = InventoryWrite {
            id: ch.id,
            account: a.id,
            revision: 0,
            items: inv.clone(),
        };
        ch.revision = s.exchange(vec![write.clone()], Vec::new()).await.unwrap()[0];
        assert!(s.exchange(vec![write], Vec::new()).await.is_err());
        ch.items = inv;
        ch.hp = 0;
        ch.alive = false;
        s.death(ch.clone(), "test enemy".into()).await.unwrap();
        assert!(s.load(a.id, ch.id).await.is_err());
        assert!(s.death(ch.clone(), "test enemy".into()).await.is_err());
        let fame = s.fame(a.id, ch.id).await.unwrap();
        let root = roxmltree::Document::parse(&fame).unwrap();
        assert_eq!(root.root_element().tag_name().name(), "Fame");
        assert_eq!(
            root.descendants()
                .find(|n| n.has_tag_name("KilledBy"))
                .unwrap()
                .text(),
            Some("test enemy")
        );
        let fresh = s.list("second-account".into()).await.unwrap();
        assert_eq!(fresh.next_id, 1);
        s.rename(fresh.id, "Guest".into()).await.unwrap();
        assert_eq!(s.list("second-account".into()).await.unwrap().name, "Guest");
        assert!(
            s.list("test-account".into())
                .await
                .unwrap()
                .characters
                .is_empty()
        );
    }
}

#[cfg(test)]
mod equipment_fame_tests;
