use super::*;
use crate::achievements::{Achievement, CATALOG};
use anyhow::bail;
use rusqlite::Transaction;

use super::schema22_compat::TABLES;

pub(super) fn check_legacy_death_attempts(db: &Connection) -> Result<()> {
    // Use the same Unix-seconds UTC domain as Store::fame's archived death dates.
    const MAX_UTC_SECONDS: i64 = 8_640_000_000_000;
    for (id, label) in [
        (Achievement::NexusAfterDeath, "Nexus"),
        (Achievement::Ah7uck, "Ah 7uck"),
    ] {
        let reward = id.reward();
        let flag = serde_json::to_value(id)?
            .as_str()
            .context("achievement ID is not a string")?
            .to_owned();
        let mut dates = db.prepare("SELECT account,'awarded_at',typeof(awarded_at) FROM gold_rewards WHERE reward=?2 AND (typeof(awarded_at)<>'integer' OR awarded_at NOT BETWEEN 1 AND ?1) UNION ALL SELECT account,'notified_at',typeof(notified_at) FROM gold_rewards WHERE reward=?2 AND notified_at IS NOT NULL AND (typeof(notified_at)<>'integer' OR notified_at NOT BETWEEN 1 AND ?1) ORDER BY 1,2")?;
        let invalid = dates
            .query_map(params![MAX_UTC_SECONDS, reward], |r| {
                Ok((
                    r.get::<_, i32>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(
            invalid.is_empty(),
            "invalid {label} receipt metadata (account,date field,stored type)={invalid:?}; preserve input and quarantine for explicit review"
        );
        let mut q = db.prepare("SELECT source,account,character FROM (SELECT 'characters.stats' AS source,c.account,c.id AS character,c.stats AS raw FROM characters c UNION ALL SELECT 'deaths.snapshot',c.account,d.character,d.snapshot FROM deaths d JOIN characters c ON c.id=d.character) h WHERE EXISTS(SELECT 1 FROM json_each(h.raw,'$.achievements') WHERE value=?1) AND NOT EXISTS(SELECT 1 FROM gold_rewards r WHERE r.account=h.account AND r.reward=?2 AND r.awarded_at>0 AND (r.notified_at IS NULL OR r.notified_at>0)) ORDER BY source,account,character")?;
        let ambiguous = q
            .query_map(params![flag, reward], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i32>(1)?,
                    r.get::<_, i32>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(
            ambiguous.is_empty(),
            "ambiguous unpaid legacy {label} attempt history (source,account,character)={ambiguous:?}; preserve input and quarantine for explicit review; no history inferred or rewritten"
        );
    }
    Ok(())
}

pub(super) fn check_reusable_observation(tx: &Transaction<'_>, ch: &Character) -> Result<()> {
    for id in [Achievement::NexusAfterDeath, Achievement::Ah7uck] {
        ensure!(
            !ch.achievements.contains(&id) || earned(tx, ch.account, id)?,
            "unpaid pending death attempt {id:?} must settle only in its atomic permanent-death transaction"
        );
    }
    Ok(())
}

pub(super) fn migrate(tx: &Transaction<'_>, create: bool) -> Result<()> {
    if create {
        for (_, sql) in TABLES {
            tx.execute_batch(sql)?;
        }
    }
    Ok(())
}
fn human(tx: &Transaction<'_>, account: i32) -> Result<bool> {
    ensure!(
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
            [account],
            |r| r.get::<_, bool>(0)
        )?,
        "achievement account {account} missing"
    );
    Ok(!tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM shore_bots WHERE account=?1)",
        [account],
        |r| r.get::<_, bool>(0),
    )?)
}
pub(super) fn grant(tx: &Transaction<'_>, account: i32, id: Achievement) -> Result<bool> {
    if !human(tx, account)? {
        return Ok(false);
    }
    let granted = gold::grant(tx, account, id.reward())?;
    tracing::debug!(account, achievement=?id, granted, "achievement reward staged");
    Ok(granted)
}
fn earned(tx: &Transaction<'_>, account: i32, id: Achievement) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM gold_rewards WHERE account=?1 AND reward=?2)",
        params![account, id.reward()],
        |r| r.get(0),
    )?)
}
pub(super) fn observed(tx: &Transaction<'_>, ch: &Character) -> Result<()> {
    for id in &ch.achievements {
        match id {
            Achievement::HitByTruck | Achievement::NotEvenClose => {
                grant(tx, ch.account, *id)?;
            }
            Achievement::NexusAfterDeath | Achievement::Ah7uck => {
                let dead: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM deaths d JOIN characters c ON c.id=d.character WHERE c.id=?1 AND c.account=?2 AND c.alive=0)", params![ch.id,ch.account], |r| r.get(0))?;
                if dead {
                    grant(tx, ch.account, *id)?;
                }
            }
            _ => bail!("invalid runtime achievement observation {id:?}"),
        }
    }
    Ok(())
}
fn clears(ch: &Character) -> [u64; 13] {
    [
        ch.pirate_caves,
        ch.dungeons[0],
        ch.dungeons[1],
        ch.dungeons[2],
        ch.abysses,
        ch.trenches,
        ch.spider_dens,
        ch.mad_labs,
        ch.jungles,
        ch.manors,
        ch.beaches,
        ch.davy_lockers,
        ch.tombs,
    ]
}
pub(super) fn progress(
    tx: &Transaction<'_>,
    previous: &Character,
    ch: &Character,
    classes: &BTreeMap<u16, Class>,
) -> Result<()> {
    observed(tx, ch)?;
    if !human(tx, ch.account)? {
        return Ok(());
    }
    let caps = &classes
        .get(&ch.class)
        .context("achievement class missing")?
        .caps;
    let maxed = ch.base.iter().zip(caps).filter(|(n, cap)| n >= cap).count();
    if maxed >= 6 {
        grant(tx, ch.account, Achievement::BabyMax)?;
    }
    if maxed == 8 {
        grant(tx, ch.account, Achievement::Max)?;
    }
    for ((before, after), (_, key, _, _)) in clears(previous)
        .into_iter()
        .zip(clears(ch))
        .zip(crate::dungeon::DUNGEONS)
    {
        ensure!(
            after >= before,
            "achievement dungeon history decreased for character {}",
            ch.id
        );
        if after > before {
            tx.execute("INSERT INTO achievement_dungeons(account,dungeon) VALUES(?1,?2) ON CONFLICT DO NOTHING", params![ch.account,key])?;
        }
    }
    let count: i32 = tx.query_row(
        "SELECT count(*) FROM achievement_dungeons WHERE account=?1",
        [ch.account],
        |r| r.get(0),
    )?;
    if count == 13 {
        grant(tx, ch.account, Achievement::DungeonMaster)?;
    }
    Ok(())
}
pub(super) fn trade(tx: &Transaction<'_>, writes: &[InventoryWrite]) -> Result<()> {
    ensure!(
        writes.len() == 2 && writes[0].account != writes[1].account,
        "achievement trade participants changed"
    );
    for side in 0..2 {
        let (account, peer) = (writes[side].account, writes[1 - side].account);
        if !human(tx, account)? {
            continue;
        }
        grant(tx, account, Achievement::Trader)?;
        if human(tx, peer)? && !earned(tx, account, Achievement::Merchant)? {
            tx.execute("INSERT INTO achievement_partners(account,peer) VALUES(?1,?2) ON CONFLICT DO NOTHING", params![account,peer])?;
            let count: i32 = tx.query_row(
                "SELECT count(*) FROM achievement_partners WHERE account=?1",
                [account],
                |r| r.get(0),
            )?;
            if count >= 10 {
                grant(tx, account, Achievement::Merchant)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClosureCredit {
    pub realm: String,
    pub root: i32,
    pub account: i32,
    pub character: i32,
}
impl Store {
    pub(crate) async fn achievement_update(
        &self,
        actors: Vec<Character>,
    ) -> Result<Vec<(i32, i32)>> {
        ensure!(
            (1..=85).contains(&actors.len()),
            "invalid intentional update achievement batch"
        );
        let mut accounts = std::collections::BTreeSet::new();
        ensure!(
            actors
                .iter()
                .all(|ch| ch.alive && ch.hp > 0 && accounts.insert(ch.account)),
            "intentional update repeated or dead achievement actor"
        );
        self.call(move |db| {
            let tx = db.transaction()?;
            let mut awards = Vec::new();
            for ch in actors {
                ensure!(tx.query_row("SELECT EXISTS(SELECT 1 FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1)", params![ch.id,ch.account,ch.revision], |r| r.get::<_,bool>(0))?, "intentional update achievement character is stale, dead or foreign: account={} character={} revision={}", ch.account,ch.id,ch.revision);
                if grant(&tx, ch.account, Achievement::No)? {
                    let balance = tx.query_row("SELECT credits FROM accounts WHERE id=?1", [ch.account], |r| r.get(0))?;
                    awards.push((ch.account, balance));
                }
            }
            tx.commit()?;
            Ok(awards)
        }).await
    }
    pub(crate) async fn achievement_closure(&self, credit: ClosureCredit) -> Result<()> {
        ensure!(
            credit.realm.len() == 32
                && credit.realm.bytes().all(|b| b.is_ascii_hexdigit())
                && credit.root > 0,
            "invalid achievement Realm identity"
        );
        self.call(move |db| {
            let tx = db.transaction()?;
            let owner: i32 = tx.query_row("SELECT account FROM characters WHERE id=?1", [credit.character], |r| r.get(0))?;
            ensure!(owner == credit.account, "achievement closure killer ownership changed");
            if human(&tx, owner)? && !earned(&tx, owner, Achievement::RealmCloser)? {
                tx.execute("INSERT INTO achievement_realm_credits(account,realm,root) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING", params![owner,credit.realm,credit.root])?;
                let count: i32 = tx.query_row("SELECT count(*) FROM achievement_realm_credits WHERE account=?1 AND realm=?2", params![owner,credit.realm], |r| r.get(0))?;
                if count >= 15 { grant(&tx, owner, Achievement::RealmCloser)?; }
            }
            tx.commit()?;
            tracing::info!(account=credit.account, character=credit.character, realm=%credit.realm, root=credit.root, "achievement closure credit committed");
            Ok(())
        }).await
    }
    pub(crate) async fn achievement_invite(&self, account: i32, guild: i32) -> Result<()> {
        self.call(move |db| {
            let tx = db.transaction()?;
            ensure!(
                guilds::membership(&tx, account)?.is_some_and(|m| m.id == guild && m.rank >= 20),
                "achievement invitation authority changed"
            );
            grant(&tx, account, Achievement::Friends)?;
            tx.commit()?;
            tracing::info!(account, "guild invitation achievement committed");
            Ok(())
        })
        .await
    }
    pub(crate) async fn achievement_dead_escape(&self, account: i32, character: i32) -> Result<()> {
        self.call(move |db| {
            let tx = db.transaction()?;
            ensure!(tx.query_row("SELECT EXISTS(SELECT 1 FROM deaths d JOIN characters c ON c.id=d.character WHERE c.id=?1 AND c.account=?2 AND c.alive=0)", params![character,account], |r| r.get::<_, bool>(0))?, "escape achievement requires this actor's committed permanent death");
            grant(&tx, account, Achievement::NexusAfterDeath)?;
            tx.commit()?;
            tracing::info!(account, character, "dead Nexus attempt achievement committed");
            Ok(())
        }).await
    }
    pub async fn achievements(&self, account: i32) -> Result<serde_json::Value> {
        self.call(move |db| {
            let tx = db.transaction()?;
            let name: String = tx.query_row("SELECT a.name FROM accounts a JOIN competitors c ON c.account=a.id WHERE a.id=?1 AND NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=a.id)", [account], |r| r.get(0)).context("player has no competitive profile")?;
            let mut earned = Vec::new();
            for d in &CATALOG {
                if let Some(time) = tx.query_row("SELECT awarded_at FROM gold_rewards WHERE account=?1 AND reward=?2", params![account,d.id.reward()], |r| r.get::<_, i64>(0)).optional()? {
                    earned.push(serde_json::json!({"id":d.id,"earned_at":time}));
                }
            }
            tx.commit()?;
            Ok(serde_json::json!({"account":account,"name":name,"earned":earned}))
        }).await
    }
}
