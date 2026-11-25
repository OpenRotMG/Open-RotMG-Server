use super::*;
use crate::loot::evaluator::{AllocationPlan, Contribution, DefeatContext, Owner, PlanningLimits};
use anyhow::bail;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, num::NonZeroUsize};

// A 65k compact plan plus bounded JSON/ledger scratch fits below the unchanged 64 MiB envelope; live bags are not an entitlement limit.
pub(crate) const LIMITS: PlanningLimits = PlanningLimits {
    max_allocations: NonZeroUsize::new(65_536).unwrap(),
    max_contributions: NonZeroUsize::new(4096).unwrap(),
};
const MAX_PAYLOAD: usize = 8 * 1024 * 1024;
// Origins are excluded while live, so this only bounds forensics: two days keeps each settled row in a nightly backup despite one failed run.
pub(crate) const JOURNAL_RETENTION_S: i64 = 2 * 24 * 3600;
// Terminal ids are (instance, never-reused actor) and unreleased ids are excluded, so only a Store call queued before
// release (10 s timeouts) can still replay a compacted progress-only row; an hour covers that without bot-kill audit volume.
pub(crate) const PROGRESS_ONLY_RETENTION_S: i64 = 3600;
pub(crate) const JOURNAL_PRUNE_BATCH: usize = 512;
/// Published, nothing deliverable, every pickup published, and not itself a pickup a parent row still binds.
const SETTLED: &str = "status=2 AND NOT EXISTS(SELECT 1 FROM world_entitlements w WHERE w.event=e.event AND w.status=0)
    AND NOT EXISTS(SELECT 1 FROM world_entitlements w JOIN world_events p ON p.event=w.pickup_operation WHERE w.event=e.event AND p.status<>2)
    AND NOT EXISTS(SELECT 1 FROM world_entitlements w WHERE w.pickup_operation=e.event)";
const SCHEMA: &str = "CREATE TABLE world_events(event TEXT PRIMARY KEY,kind TEXT NOT NULL CHECK(kind IN ('terminal','inventory','stamp')),version INTEGER NOT NULL CHECK(version=1),content_fp TEXT NOT NULL,payload TEXT NOT NULL CHECK(json_valid(payload)),payload_hash TEXT NOT NULL,status INTEGER NOT NULL CHECK(status BETWEEN 0 AND 3),result TEXT CHECK(result IS NULL OR json_valid(result)));
CREATE TABLE world_entitlements(event TEXT NOT NULL REFERENCES world_events(event),ordinal INTEGER NOT NULL CHECK(ordinal>=0),owner_character INTEGER,item INTEGER NOT NULL CHECK(item BETWEEN 0 AND 65535),pickup_operation TEXT UNIQUE,status INTEGER NOT NULL CHECK(status IN (0,1,2,3)),PRIMARY KEY(event,ordinal));";

pub(super) fn migrate(db: &Connection, version: i32) -> Result<()> {
    if version < 25 {
        db.execute_batch(SCHEMA)?;
    }
    for statement in SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let name = statement
            .split_whitespace()
            .nth(2)
            .context("event schema name missing")?
            .split('(')
            .next()
            .unwrap();
        let sql: String =
            db.query_row("SELECT sql FROM sqlite_schema WHERE name=?1", [name], |r| {
                r.get(0)
            })?;
        ensure!(sql == statement, "world event schema mismatch: {name}");
    }
    Ok(())
}

/// Base HEAD 79a4c92 src/world.rs:6184 kept_marked_drops semantics, decided once per committed ordinal from the event seed.
fn moderation_keeps(seed: u64, ordinal: usize, keep: f64) -> bool {
    let digest = Sha256::digest(serde_json::json!(["moderation", seed, ordinal]).to_string());
    let stream = u64::from_le_bytes(digest[..8].try_into().expect("sha256 prefix"));
    rand::Rng::gen_bool(
        &mut <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(stream),
        keep,
    )
}
pub(crate) fn new_id() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalPayload {
    pub defeat: DefeatContext,
    pub ledger: Vec<Contribution>,
    pub seed: u64,
    pub allocation: Option<AllocationPlan>,
    pub actions: serde_json::Value,
}
impl TerminalPayload {
    pub fn allocations(&self) -> &[crate::loot::evaluator::Allocation] {
        match &self.allocation {
            Some(plan) => plan.allocations(),
            None => &[],
        }
    }
    pub fn validate(&self, game: &crate::content::game::GameData) -> Result<()> {
        if let Some(plan) = &self.allocation {
            ensure!(
                plan.seed() == self.seed,
                "journal event seed differs from sampled allocation"
            );
            plan.validate(game, self.defeat, &self.ledger, LIMITS)?;
        }
        if self.actions.as_object().is_some_and(|a| a.is_empty()) {
            ensure!(self.allocation.is_some(), "terminal component plan absent");
            return Ok(());
        }
        let actions: TerminalActions = serde_json::from_value(self.actions.clone())?;
        ensure!(
            actions.instance.len() == 32
                && actions.actor == self.defeat.actor
                && actions.progress.len() <= 85,
            "terminal actions scope/bounds changed"
        );
        ensure!(
            actions.loot == self.allocation.is_some()
                || actions.loot
                    && self.allocation.is_none()
                    && actions
                        .planning_error
                        .as_ref()
                        .is_some_and(|error| !error.is_empty()),
            "terminal allocation differs from source reward flags"
        );
        ensure!(
            self.defeat.actor > 0
                && self.ledger.iter().all(|c| c.character > 0
                    && c.world == self.defeat.world
                    && c.encounter == self.defeat.encounter),
            "terminal captured defeat/ledger scope invalid"
        );
        game.admission(
            self.defeat.kind,
            crate::content::game::AdmissionRole::Scripted,
            "journal terminal source",
        )?;
        let mut owners = BTreeSet::new();
        for (ch, assists) in &actions.progress {
            ensure!(
                owners.insert(ch.account) && ch.alive && ch.id > 0 && assists.len() <= 19,
                "terminal progress ownership/bounds invalid"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalActions {
    pub instance: String,
    pub actor: i32,
    pub loot: bool,
    pub progress: Vec<(Character, Vec<LevelAssist>)>,
    pub publication: serde_json::Value,
    pub planning_error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Event {
    pub id: String,
    pub kind: String,
    pub content: String,
    pub payload: EventPayload,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum EventPayload {
    Terminal(Box<TerminalPayload>),
    Inventory(serde_json::Value),
    Stamp(serde_json::Value),
}
fn gameplay_fields(value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                let name = name.to_ascii_lowercase();
                ensure!(
                    ![
                        "password",
                        "secret",
                        "credential",
                        "recovery",
                        "token",
                        "auth"
                    ]
                    .iter()
                    .any(|part| name.contains(part))
                        && !matches!(name.as_str(), "guid" | "key" | "keys" | "grant" | "grants"),
                    "sensitive field {name} cannot enter a gameplay journal/backup"
                );
                gameplay_fields(value)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                gameplay_fields(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn terminal_id(payload: &TerminalPayload) -> String {
    let identity = match payload.actions.get("instance") {
        Some(instance) => serde_json::json!(["terminal", instance, payload.defeat.actor]),
        None => serde_json::json!([
            "terminal-component",
            payload.defeat.world,
            payload.defeat.encounter,
            payload.defeat.actor,
            payload.defeat.event
        ]),
    };
    format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))[..32].to_owned()
}
impl Event {
    pub fn new(kind: &str, content: &str, payload: serde_json::Value) -> Result<Self> {
        let payload = match kind {
            "inventory" => EventPayload::Inventory(payload),
            "stamp" => EventPayload::Stamp(payload),
            _ => anyhow::bail!("terminal event requires typed captured payload"),
        };
        let event = Self {
            id: new_id(),
            kind: kind.into(),
            content: content.into(),
            payload,
        };
        event.encoded()?;
        Ok(event)
    }
    pub fn terminal(content: &str, payload: TerminalPayload) -> Result<Self> {
        let event = Self {
            id: terminal_id(&payload),
            kind: "terminal".into(),
            content: content.into(),
            payload: EventPayload::Terminal(Box::new(payload)),
        };
        event.encoded()?;
        Ok(event)
    }
    pub fn native_payload(&self) -> Result<&serde_json::Value> {
        match &self.payload {
            EventPayload::Inventory(payload) | EventPayload::Stamp(payload) => Ok(payload),
            _ => anyhow::bail!("event {} is not native inventory/stamp", self.id),
        }
    }
    pub(crate) fn terminal_payload(&self) -> Result<&TerminalPayload> {
        match &self.payload {
            EventPayload::Terminal(payload) => Ok(payload),
            _ => anyhow::bail!("event {} is not terminal", self.id),
        }
    }
    fn encoded(&self) -> Result<(String, String)> {
        ensure!(
            self.id.len() == 32 && self.id.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid durable event ID"
        );
        ensure!(
            matches!(self.kind.as_str(), "terminal" | "inventory" | "stamp")
                && self.content.len() == 64,
            "invalid durable event kind/content"
        );
        match &self.payload {
            EventPayload::Terminal(payload) => {
                ensure!(
                    self.kind == "terminal"
                        && (payload.actions.get("instance").is_none()
                            || self.id == terminal_id(payload)),
                    "terminal event kind/lifetime identity changed"
                );
                gameplay_fields(&payload.actions)?;
            }
            EventPayload::Inventory(payload) => {
                ensure!(self.kind == "inventory", "inventory event kind changed");
                gameplay_fields(payload)?;
            }
            EventPayload::Stamp(payload) => {
                ensure!(self.kind == "stamp", "stamp event kind changed");
                gameplay_fields(payload)?;
            }
        }
        let payload = serde_json::to_string(self)?;
        ensure!(
            payload.len() <= MAX_PAYLOAD,
            "event {} exceeds bounded journal payload; retain captured seed/context for reconciliation",
            self.id
        );
        let hash = format!("{:x}", Sha256::digest(payload.as_bytes()));
        Ok((payload, hash))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Record {
    pub event: Event,
    pub status: i32,
    pub result: Option<serde_json::Value>,
}
type TerminalInputs = (TerminalPayload, Vec<u8>, BTreeSet<i32>);
/// Replaces the payload of settled history; id, kind, content, original payload_hash, status and result stay in their columns.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Tombstone {
    instance: Option<String>,
    characters: BTreeSet<i32>,
    seed: Option<u64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Compacted {
    compacted: Tombstone,
}
impl Tombstone {
    fn of(event: &Event) -> Result<Self> {
        Ok(match &event.payload {
            EventPayload::Terminal(p) => {
                let mut characters: BTreeSet<i32> = p.ledger.iter().map(|c| c.character).collect();
                characters.extend(p.allocations().iter().filter_map(|a| match a.owner() {
                    Owner::Character(id) => Some(id),
                    Owner::Public => None,
                }));
                let mut instance = None;
                if !p.actions.as_object().is_some_and(|a| a.is_empty()) {
                    let actions: TerminalActions = serde_json::from_value(p.actions.clone())?;
                    characters.extend(actions.progress.iter().map(|(ch, _)| ch.id));
                    instance = Some(actions.instance);
                }
                Self {
                    instance,
                    characters,
                    seed: Some(p.seed),
                }
            }
            EventPayload::Inventory(p) | EventPayload::Stamp(p) => Self {
                instance: p["origin"]["instance"].as_str().map(str::to_owned),
                characters: match event.kind.as_str() {
                    "inventory" => {
                        [serde_json::from_value::<InventoryWrite>(p["write"].clone())?.id].into()
                    }
                    _ => BTreeSet::new(),
                },
                seed: None,
            },
        })
    }
}
struct Settled {
    kind: String,
    content: String,
    hash: String,
    result: serde_json::Value,
    tombstone: Tombstone,
}
fn compacted(db: &Connection, id: &str) -> Result<Option<Settled>> {
    let row: Option<(String, String, String, String, i32, Option<String>)> = db.query_row("SELECT kind,content_fp,payload_hash,payload,status,result FROM world_events WHERE event=?1 AND json_type(payload,'$.compacted') IS NOT NULL", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).optional()?;
    row.map(|(kind, content, hash, payload, status, result)| {
        let result: serde_json::Value = serde_json::from_str(
            &result.with_context(|| format!("compacted event {id} result absent"))?,
        )?;
        ensure!(
            status == 2 && result["published_unix"].as_i64().is_some_and(|at| at > 0),
            "event {id} compacted without a settled publication receipt"
        );
        let tombstone = serde_json::from_str::<Compacted>(&payload)
            .with_context(|| format!("event {id} tombstone malformed"))?
            .compacted;
        ensure!(
            tombstone.seed.is_some() == (kind == "terminal")
                && tombstone.characters.iter().all(|c| *c > 0)
                && hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "event {id} tombstone kind/owner/hash invalid"
        );
        Ok(Settled {
            kind,
            content,
            hash,
            result,
            tombstone,
        })
    })
    .transpose()
}
fn read(db: &Connection, id: &str) -> Result<Option<Record>> {
    type Row = (
        String,
        String,
        String,
        String,
        i32,
        i32,
        Option<String>,
        bool,
    );
    let row:Option<Row>=db.query_row("SELECT payload,payload_hash,kind,content_fp,version,status,result,json_type(payload,'$.compacted') IS NOT NULL FROM world_events WHERE event=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
    row.map(|(payload, hash, kind, content, version, status, result, compacted)| {
        ensure!(
            !compacted,
            "event {id} was compacted after settlement; only its original hash, status and result remain"
        );
        ensure!(
            format!("{:x}", Sha256::digest(payload.as_bytes())) == hash,
            "event {id} payload hash mismatch; retained for reconciliation"
        );
        let event: Event = serde_json::from_str(&payload)?;
        ensure!(
            event.id == id
                && event.kind == kind
                && event.content == content
                && version == 1
                && event.encoded()?.0 == payload,
            "event {id} canonical payload/identity/version mismatch"
        );
        ensure!(
            !(status == 0 && result.is_some() || matches!(status, 1..=3) && result.is_none()),
            "event {id} result/state mismatch"
        );
        Ok(Record {
            event,
            status,
            result: result.map(|s| serde_json::from_str(&s)).transpose()?,
        })
    })
    .transpose()
}
fn prepare_row(db: &Connection, event: &Event, (payload, hash): (String, String)) -> Result<()> {
    db.execute("INSERT INTO world_events(event,kind,version,content_fp,payload,payload_hash,status) VALUES(?1,?2,1,?3,?4,?5,0) ON CONFLICT(event) DO NOTHING",params![event.id,event.kind,event.content,payload,hash])?;
    Ok(())
}
fn exact(db: &Connection, event: &Event) -> Result<Record> {
    if let Some(settled) = compacted(db, &event.id)? {
        ensure!(
            event.encoded()?.1 == settled.hash,
            "event {} differs from its compacted settled journal hash",
            event.id
        );
        return Ok(Record {
            event: event.clone(),
            status: 2,
            result: Some(settled.result),
        });
    }
    let record = read(db, &event.id)?.context("durable event not prepared")?;
    ensure!(
        record.event == *event,
        "event {} payload/seed/ledger/context differs from journal",
        event.id
    );
    ensure!(
        record.status != 3,
        "event {} requires content reconciliation",
        event.id
    );
    Ok(record)
}
/// Commits one prepared journal event inside the caller's transaction; an already committed event is unchanged.
fn commit_journal_event(
    tx: &Connection,
    event: &Event,
    inputs: Option<(TerminalPayload, Vec<u8>, BTreeSet<i32>)>,
) -> Result<()> {
    if exact(tx, event)?.status != 0 {
        return Ok(());
    }
    let Some((payload, bag_types, reviewed)) = inputs else {
        return commit(tx, event, &serde_json::json!({"committed":true}));
    };
    let demand = payload
        .allocation
        .as_ref()
        .map(AllocationPlan::demand)
        .unwrap_or_default();
    for owner in demand.keys() {
        if let Owner::Character(id) = owner {
            ensure!(
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM characters WHERE id=?1)",
                    [id],
                    |r| r.get::<_, bool>(0)
                )?,
                "terminal entitlement durable owner {id} missing"
            );
        }
    }
    let mut withheld = serde_json::Map::new();
    for (ordinal, item) in payload.allocations().iter().enumerate() {
        let owner = match item.owner() {
            Owner::Public => None,
            Owner::Character(id) => Some(id),
        };
        let mut status = 0;
        if let Some(id) = owner {
            let account: i32 =
                tx.query_row("SELECT account FROM characters WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            let bag_type = bag_types[ordinal];
            if let Some(keep) = crate::content::reviews::marked_keep_chance(bag_type)
                .filter(|_| reviewed.contains(&account))
                && !moderation_keeps(payload.seed, ordinal, keep)
            {
                status = 3;
                withheld.insert(ordinal.to_string(), serde_json::json!({"disposition":"moderation_withheld","account":account,"bag_type":bag_type,"keep_chance":keep}));
                tracing::info!(event=%event.id,ordinal,account,item=item.item(),"reviewed account entitlement withheld");
            }
        }
        tx.execute(
            "INSERT INTO world_entitlements VALUES(?1,?2,?3,?4,NULL,?5)",
            params![event.id, ordinal, owner, item.item(), status],
        )?;
    }
    if !payload.actions.as_object().is_some_and(|a| a.is_empty()) {
        let actions: TerminalActions = serde_json::from_value(payload.actions.clone())?;
        for (ch, assists) in &actions.progress {
            write_progress_row(tx, ch, assists)?;
        }
    }
    let mut result = serde_json::json!({"allocations":payload.allocations().len()});
    if !withheld.is_empty() {
        result["withheld"] = withheld.into();
    }
    commit(tx, event, &result)
}
fn commit(db: &Connection, event: &Event, result: &serde_json::Value) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE world_events SET status=1,result=?2 WHERE event=?1 AND status=0",
            params![event.id, serde_json::to_string(result)?]
        )? == 1,
        "event {} commit state changed",
        event.id
    );
    Ok(())
}
pub(super) fn run_operation<T>(
    db: &mut Connection,
    event: Option<&Event>,
    f: impl FnOnce(&mut Connection) -> Result<T>,
) -> Result<T> {
    if let Some(event) = event {
        ensure!(
            exact(db, event)?.status == 0,
            "operation {} already committed; reconcile its exact journal result instead of replaying",
            event.id
        );
    }
    let result = f(db);
    if let Some(event) = event {
        let record =
            match exact(db, event) {
                Ok(record) => record,
                Err(read_error) => return match result {
                    Err(error) => Err(error.context(format!(
                        "event {} outcome read failed: {read_error:#}; retained for reconciliation",
                        event.id
                    ))),
                    Ok(_) => Err(read_error.context(format!(
                        "event {} outcome unavailable after native write",
                        event.id
                    ))),
                },
            };
        if result.is_ok() {
            ensure!(
                record.status == 1,
                "native operation {} returned without atomic journal commit",
                event.id
            );
            tracing::info!(event=%event.id,kind=%event.kind,"native inventory and exact journal outcome committed");
        } else {
            tracing::error!(event=%event.id,status=record.status,"native inventory failed; original journal and plan retained for reconciliation");
        }
    }
    result
}
pub(super) fn reject_operation(db: &Connection, event: Option<&Event>) -> Result<()> {
    if let Some(event) = event {
        ensure!(
            !db.is_autocommit(),
            "rejection requires original transaction"
        );
        exact(db, event)?;
        commit(db, event, &serde_json::json!({"rejected":true}))?;
    }
    Ok(())
}
pub(super) fn finish_operation(
    db: &Connection,
    event: Option<&Event>,
    result: &serde_json::Value,
) -> Result<()> {
    let Some(event) = event else { return Ok(()) };
    ensure!(
        !db.is_autocommit(),
        "native inventory journal requires its original transaction"
    );
    let original = exact(db, event)?;
    ensure!(original.status == 0, "native operation already committed");
    let write: InventoryWrite = serde_json::from_value(
        event
            .native_payload()?
            .get("write")
            .context("inventory event write absent")?
            .clone(),
    )?;
    let revision = result["revision"]
        .as_i64()
        .context("native operation revision absent")?;
    ensure!(
        i64::from(write.revision) + 1 == revision,
        "native operation revision differs from captured plan"
    );
    let (saved, actual): (String, i32) = db.query_row(
        "SELECT items,revision FROM characters WHERE id=?1 AND account=?2 AND alive=1",
        params![write.id, write.account],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    ensure!(
        i64::from(actual) == revision && serde_json::from_str::<Vec<i32>>(&saved)? == write.items,
        "native operation owner/write differs from captured journal"
    );
    if let Some(claim) = event
        .native_payload()?
        .get("claims")
        .filter(|v| !v.is_null())
    {
        let (source, ordinal): (String, usize) = serde_json::from_value(claim.clone())?;
        let (owner,item,status):(Option<i32>,u16,i32)=db.query_row("SELECT owner_character,item,status FROM world_entitlements WHERE event=?1 AND ordinal=?2",params![source,ordinal],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let native = event.native_payload()?;
        let reservation = &native["reservation"];
        let consumed = [
            "potion",
            "stat_growth",
            "appearance",
            "xp_boost",
            "companion",
            "unlock",
            "ground_dye",
        ]
        .iter()
        .any(|key| reservation.get(key).is_some_and(|v| !v.is_null()));
        let vaulted = reservation["vaults"].as_array().is_some_and(|vaults| {
            vaults.iter().any(|vault| {
                vault["items"]
                    .as_array()
                    .is_some_and(|items| items.contains(&serde_json::json!(item)))
            })
        });
        ensure!(
            status == 0
                && owner.is_none_or(|owner| owner == write.id)
                && (write.items.contains(&i32::from(item)) || consumed || vaulted),
            "native pickup entitlement owner/item/state differs"
        );
        ensure!(db.execute("UPDATE world_entitlements SET status=1,pickup_operation=?3 WHERE event=?1 AND ordinal=?2 AND status=0",params![source,ordinal,event.id])?==1,"native pickup lost entitlement claim");
    }
    commit(db, event, result)
}
pub(super) fn validate_state(db: &Connection) -> Result<()> {
    let mut query = db.prepare("SELECT event FROM world_events ORDER BY event")?;
    let ids = query.query_map([], |r| r.get::<_, String>(0))?;
    for id in ids {
        validate_event(db, &id?)?;
    }
    Ok(())
}
type EntitlementRow = (usize, Option<i32>, u16, Option<String>, i32);
fn entitlement_rows(db: &Connection, id: &str) -> Result<Vec<EntitlementRow>> {
    Ok(db.prepare("SELECT ordinal,owner_character,item,pickup_operation,status FROM world_entitlements WHERE event=?1 ORDER BY ordinal")?.query_map([id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
fn validate_compacted(db: &Connection, id: &str, settled: &Settled) -> Result<()> {
    let rows = entitlement_rows(db, id)?;
    let expected = match settled.kind.as_str() {
        "terminal" => settled.result["allocations"]
            .as_u64()
            .context("compacted terminal allocation receipt absent")?,
        _ => 0,
    };
    ensure!(
        rows.len() as u64 == expected,
        "event {id} compacted entitlement count differs from its commit receipt"
    );
    ensure!(
        !db.query_row(
            "SELECT EXISTS(SELECT 1 FROM world_entitlements WHERE pickup_operation=?1)",
            [id],
            |r| r.get::<_, bool>(0)
        )?,
        "event {id} compacted while bound as an entitlement pickup"
    );
    for (expected, (ordinal, owner, _, operation, status)) in rows.into_iter().enumerate() {
        ensure!(
            ordinal == expected && status != 0,
            "event {id} compacted with deliverable or misordered entitlement {ordinal}"
        );
        let seed = settled
            .tombstone
            .seed
            .context("compacted terminal seed absent")?;
        validate_entitlement(
            db,
            id,
            Some(&settled.result),
            seed,
            ordinal,
            owner,
            operation,
            status,
        )?;
    }
    Ok(())
}
fn validate_event(db: &Connection, id: &str) -> Result<()> {
    if let Some(settled) = compacted(db, id)? {
        return validate_compacted(db, id, &settled);
    }
    let record = read(db, id)?.context("journal row disappeared")?;
    if record.status == 3 {
        let result = record
            .result
            .as_ref()
            .context("reconciliation result absent")?;
        let reconciliation = &result["reconciliation"];
        let previous = reconciliation["previous_status"]
            .as_i64()
            .context("reconciliation prior state absent")?;
        ensure!(
            (0..=2).contains(&previous),
            "event {id} invalid reconciliation prior state"
        );
        if let Some(disposition) = reconciliation.get("disposition") {
            let expected = if previous == 0 {
                "aborted_origin_lost"
            } else if record.event.kind == "inventory" {
                "committed_effect_origin_lost"
            } else {
                "origin_lost"
            };
            let source = match &record.event.payload {
                EventPayload::Terminal(p) => &p.actions["instance"],
                EventPayload::Inventory(p) | EventPayload::Stamp(p) => &p["origin"]["instance"],
            };
            ensure!(
                *disposition == expected
                    && reconciliation["source"] == *source
                    && reconciliation["at"].as_i64().is_some_and(|at| at > 0)
                    && reconciliation["reason"]
                        .as_str()
                        .is_some_and(|reason| !reason.is_empty()),
                "event {id} origin-loss disposition/source/time invalid"
            );
            ensure!(
                !db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM world_entitlements WHERE event=?1 AND status=0)",
                    [&id],
                    |r| r.get::<_, bool>(0)
                )?,
                "event {id} origin-lost entitlement remains deliverable"
            );
        }
    }

    if record.event.kind == "terminal" {
        let payload = record.event.terminal_payload()?;
        ensure!(
            payload
                .allocation
                .as_ref()
                .is_none_or(|plan| plan.seed() == payload.seed
                    && plan.defeat() == payload.defeat
                    && plan.content_fingerprint() == record.event.content),
            "event {id} original seed/context/content binding differs"
        );
        let rows = entitlement_rows(db, id)?;
        let prepared = record.status == 0
            || record.status == 3
                && record
                    .result
                    .as_ref()
                    .and_then(|v| v["reconciliation"]["previous_status"].as_i64())
                    == Some(0);
        ensure!(
            rows.len()
                == if prepared {
                    0
                } else {
                    payload.allocations().len()
                },
            "event {id} entitlement count differs from original plan"
        );
        for (expected, ((ordinal, owner, item, operation, status), allocation)) in
            rows.into_iter().zip(payload.allocations()).enumerate()
        {
            let expected_owner = match allocation.owner() {
                Owner::Public => None,
                Owner::Character(id) => Some(id),
            };
            ensure!(
                ordinal == expected && owner == expected_owner && item == allocation.item(),
                "event {id} entitlement ordinal/owner/item changed at {ordinal}"
            );
            validate_entitlement(
                db,
                id,
                record.result.as_ref(),
                payload.seed,
                ordinal,
                owner,
                operation,
                status,
            )?;
        }
    } else if record.event.kind == "inventory" {
        if let Some(publication) = record.result.as_ref().and_then(|r| r.get("publication")) {
            let deadline = record.event.native_payload()?["portal"]["locked_deadline"]
                .as_u64()
                .context("late publication original deadline missing")?;
            let at = publication["published_at"]
                .as_u64()
                .context("late publication time missing")?;
            ensure!(
                publication["disposition"] == "published_after_source_deadline"
                    && publication["source_deadline"].as_u64() == Some(deadline)
                    && at >= deadline
                    && publication["late_ms"].as_u64() == Some(at - deadline)
                    && record.event.native_payload()?["origin"]["accepted_at"]
                        .as_u64()
                        .is_some_and(|accepted| accepted < deadline),
                "event {id} late publication causal proof invalid"
            );
        }
        let write: InventoryWrite = serde_json::from_value(
            record
                .event
                .native_payload()?
                .get("write")
                .context("inventory journal write absent")?
                .clone(),
        )?;
        ensure!(
            write.id > 0
                && write.account > 0
                && (0..i32::MAX).contains(&write.revision)
                && write.items.len() == 12
                && write.items.iter().all(|kind| (-1..=65535).contains(kind)),
            "event {id} malformed inventory write"
        );
        if matches!(record.status, 0 | 1) {
            ensure!(
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM characters WHERE id=?1 AND account=?2)",
                    params![write.id, write.account],
                    |r| r.get::<_, bool>(0)
                )?,
                "event {id} unsettled inventory owner missing"
            );
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn validate_entitlement(
    db: &Connection,
    id: &str,
    result: Option<&serde_json::Value>,
    seed: u64,
    ordinal: usize,
    owner: Option<i32>,
    operation: Option<String>,
    status: i32,
) -> Result<()> {
    if status == 0 {
        ensure!(
            operation.is_none(),
            "event {id} unpicked entitlement has operation"
        );
        if let Some(owner) = owner {
            ensure!(
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM characters WHERE id=?1)",
                    [owner],
                    |r| r.get::<_, bool>(0)
                )?,
                "event {id} unpicked owner {owner} missing"
            );
        }
    } else if status == 2 {
        let result = result.context("expired entitlement receipt absent")?;
        let proof = &result["expired"][ordinal.to_string()];
        let bag_expired = proof["bag"].as_i64().is_some_and(|bag| bag > 0)
            && proof["deadline"]
                .as_u64()
                .zip(proof["now"].as_u64())
                .is_some_and(|(deadline, now)| now >= deadline)
            && proof["reason"] == "published native bag lifetime ended";
        ensure!(
            operation.is_none()
                && (result["reconciliation"]["disposition"] == "origin_lost" || bag_expired),
            "event {id} expired entitlement lacks original bag/origin receipt"
        );
    } else if status == 3 {
        let result = result.context("withheld entitlement receipt absent")?;
        // Origin-loss reconciliation keeps the commit result under previous_result.
        let withheld = if result["withheld"].is_object() {
            &result["withheld"]
        } else {
            &result["reconciliation"]["previous_result"]["withheld"]
        };
        let proof = &withheld[ordinal.to_string()];
        let owner = owner
            .with_context(|| format!("event {id} withheld entitlement {ordinal} has no owner"))?;
        let account: i64 = db
            .query_row("SELECT account FROM characters WHERE id=?1", [owner], |r| {
                r.get(0)
            })
            .with_context(|| format!("event {id} withheld owner {owner} missing"))?;
        let keep = proof["bag_type"]
            .as_u64()
            .and_then(|bag| u8::try_from(bag).ok())
            .and_then(crate::content::reviews::marked_keep_chance);
        ensure!(
            operation.is_none()
                && proof["disposition"] == "moderation_withheld"
                && proof["account"].as_i64() == Some(account)
                && keep.is_some_and(|keep| proof["keep_chance"].as_f64() == Some(keep)
                    && !moderation_keeps(seed, ordinal, keep)),
            "event {id} withheld entitlement lacks moderation receipt bound to its row"
        );
    } else {
        ensure!(status == 1, "event {id} invalid entitlement state");
        let operation = operation.context("picked entitlement operation absent")?;
        let pickup = read(db, &operation)?.context("picked entitlement operation missing")?;
        ensure!(
            pickup.event.kind == "inventory"
                && matches!(pickup.status, 1..=3)
                && (pickup.event.native_payload()?["entitlement"]
                    == serde_json::json!([id, ordinal])
                    || pickup.event.native_payload()?["claims"]
                        == serde_json::json!([id, ordinal])),
            "event {id} pickup operation binding changed"
        );
    }
    Ok(())
}
pub(super) fn portable_ready(db: &Connection, account: i32) -> Result<()> {
    let version: i32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 25 {
        return Ok(());
    }
    let mut query=db.prepare("SELECT DISTINCT event FROM world_entitlements WHERE status=0 AND owner_character IN (SELECT id FROM characters WHERE account=?1) UNION SELECT event FROM world_events WHERE kind='inventory' AND (status IN (0,1) OR (status=3 AND json_extract(result,'$.reconciliation.previous_status') IN (0,1) AND coalesce(json_extract(result,'$.reconciliation.disposition'),'') NOT IN ('origin_lost','aborted_origin_lost','committed_effect_origin_lost'))) AND json_extract(payload,'$.payload.data.write.account')=?1
UNION SELECT event FROM world_events WHERE kind='terminal' AND (status IN (0,1) OR (status=3 AND json_extract(result,'$.reconciliation.previous_status') IN (0,1) AND coalesce(json_extract(result,'$.reconciliation.disposition'),'') NOT IN ('origin_lost','aborted_origin_lost','committed_effect_origin_lost'))) AND (
 EXISTS(SELECT 1 FROM json_each(payload,'$.payload.data.actions.progress') WHERE json_extract(value,'$[0].account')=?1)
 OR EXISTS(SELECT 1 FROM json_each(payload,'$.payload.data.allocation.allocations') a JOIN characters c ON c.id=json_extract(a.value,'$.owner.Character') WHERE c.account=?1)
 OR EXISTS(SELECT 1 FROM json_each(payload,'$.payload.data.ledger') a JOIN characters c ON c.id=json_extract(a.value,'$.character') WHERE c.account=?1)) ORDER BY event")?;
    let pending = query
        .query_map([account], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        pending.is_empty(),
        "account {account} has unsettled world events {pending:?}; use a full local database snapshot until their delivery/publication is reconciled"
    );
    Ok(())
}
impl Store {
    /// Returns how many rows belong to an earlier catalogue; those are origin-lost reconciliation state, not corruption.
    pub(crate) async fn validate_world_events(&self) -> Result<usize> {
        let game = self
            .content
            .game
            .as_ref()
            .context("journal validation requires current GameData")?
            .clone();
        self.call(move |db| {
            validate_state(db)?;
            let mut prior = 0;
            let mut query = db.prepare("SELECT event FROM world_events ORDER BY event")?;
            for id in query.query_map([], |r| r.get::<_, String>(0))? {
                let id = id?;
                if let Some(settled) = compacted(db, &id)? {
                    prior += usize::from(settled.content != game.fingerprint);
                    continue;
                }
                let record = read(db, &id)?.context("validated journal disappeared")?;
                if record.event.content != game.fingerprint {
                    prior += 1;
                } else if record.event.kind == "terminal" {
                    record.event.terminal_payload()?.validate(&game)?;
                }
            }
            Ok(prior)
        })
        .await
    }
    pub(crate) async fn prepare_world_event(&self, event: Event) -> Result<Record> {
        let encoded = event.encoded()?;
        let content = self
            .content
            .game
            .as_ref()
            .context("world journal requires owning GameData")?
            .fingerprint
            .clone();
        ensure!(
            event.content == content,
            "new event catalogue differs from owning Store"
        );
        if event.kind == "terminal" {
            event
                .terminal_payload()?
                .validate(self.content.game.as_ref().unwrap())?;
        }
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            prepare_row(&tx, &event, encoded)?;
            let record = exact(&tx, &event)?;
            tx.commit()?;
            Ok(record)
        })
        .await
    }
    pub(crate) async fn world_event(&self, id: String) -> Result<Option<Record>> {
        let content = self
            .content
            .game
            .as_ref()
            .context("world journal requires owning GameData")?
            .fingerprint
            .clone();
        self.call(move|db| {
            let record=read(db,&id)?;
            // A changed catalogue implies a restart, so the origin-loss sweep owns this row's explicit disposition.
            ensure!(record.as_ref().is_none_or(|r|r.event.content==content),"event {id} content fingerprint changed; original journal retained for origin-loss reconciliation");
            Ok(record)
        }).await
    }
    pub(crate) async fn commit_terminal_event(&self, event: Event) -> Result<Record> {
        let inputs = self.terminal_inputs(&event)?;
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            commit_journal_event(&tx, &event, inputs)?;
            tx.commit()?;
            read(db, &event.id)?.context("committed terminal event disappeared")
        })
        .await
    }
    /// Store-independent checks, run before the Store thread: catalogue, captured plan and allocation bag types.
    fn terminal_inputs(&self, event: &Event) -> Result<Option<TerminalInputs>> {
        let game = self
            .content
            .game
            .as_ref()
            .context("world journal requires GameData")?;
        if event.kind == "stamp" {
            ensure!(event.content == game.fingerprint, "stamp catalogue changed");
            return Ok(None);
        }
        ensure!(event.kind == "terminal", "not a terminal journal event");
        let payload = event.terminal_payload()?.clone();
        if !payload.actions.as_object().is_some_and(|a| a.is_empty()) {
            let actions: TerminalActions = serde_json::from_value(payload.actions.clone())?;
            if let Some(error) = actions.planning_error {
                bail!(
                    "event {} captured same-seed planning failure: {error}; pending without publication",
                    event.id
                );
            }
        }
        payload.validate(game)?;
        let reviewed = self
            .content
            .reviewed_accounts
            .rows()
            .into_iter()
            .map(|review| review.account)
            .collect();
        let bag_types = payload
            .allocations()
            .iter()
            .map(|a| Ok(self.content.get(a.item())?.bag_type))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some((payload, bag_types, reviewed)))
    }
    /// Every due terminal/stamp event shares one IMMEDIATE transaction; a savepoint confines each event's failure.
    pub(crate) async fn coordinate_terminals(
        &self,
        events: Vec<Event>,
    ) -> Result<Vec<(String, Result<Record>)>> {
        let mut checked = Vec::with_capacity(events.len());
        for event in events {
            let encoded = event.encoded();
            let inputs = self.terminal_inputs(&event);
            checked.push((event, encoded, inputs));
        }
        let content = self
            .content
            .game
            .as_ref()
            .context("world journal requires GameData")?
            .fingerprint
            .clone();
        self.call(move |db| {
            let mut tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut outcomes = Vec::with_capacity(checked.len());
            for (event, encoded, inputs) in checked {
                let outcome = (|| {
                    let encoded = encoded?;
                    ensure!(
                        event.content == content,
                        "new event catalogue differs from owning Store"
                    );
                    prepare_row(&tx, &event, encoded)?;
                    let inputs = inputs?;
                    let savepoint = tx.savepoint()?;
                    commit_journal_event(&savepoint, &event, inputs)?;
                    savepoint.commit().map_err(anyhow::Error::from)
                })();
                outcomes.push((event, outcome));
            }
            tx.commit()?;
            Ok(outcomes
                .into_iter()
                .map(|(event, outcome)| {
                    let id = event.id.clone();
                    let record = outcome.and_then(|()| {
                        let record = exact(db, &event)?;
                        ensure!(matches!(record.status, 1 | 2), "event {id} not committed");
                        Ok(record)
                    });
                    let record = record.with_context(|| {
                        format!("terminal event {id} unresolved; original captured plan retained")
                    });
                    (id, record)
                })
                .collect())
        })
        .await
    }
    pub(crate) fn for_world_event(&self, event: Event) -> Result<Self> {
        ensure!(
            self.world_operation.is_none() && event.kind == "inventory",
            "nested/noninventory world operation"
        );
        event.encoded()?;
        let mut scoped = self.clone();
        scoped.world_operation = Some(event);
        Ok(scoped)
    }
    #[cfg(test)]
    pub(crate) async fn commit_inventory_event(
        &self,
        event: Event,
        write: InventoryWrite,
    ) -> Result<Record> {
        ensure!(
            event.native_payload()?.get("write") == Some(&serde_json::to_value(&write)?),
            "inventory journal does not bind exact write"
        );
        if let Some(record) = self.world_event(event.id.clone()).await? {
            ensure!(
                record.event == event,
                "inventory replay differs from journal"
            );
            if matches!(record.status, 1 | 2) {
                return Ok(record);
            }
        }
        self.for_world_event(event.clone())?
            .exchange(vec![write], vec![])
            .await?;
        self.world_event(event.id)
            .await?
            .context("committed inventory event disappeared")
    }
    pub(crate) async fn publish_world_event(&self, event: Event) -> Result<Record> {
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let record = exact(&tx, &event)?;
            ensure!(
                matches!(record.status, 1 | 2),
                "uncommitted event cannot publish"
            );
            tx.execute(
                "UPDATE world_events SET status=2,result=json_set(result,'$.published_unix',unixepoch()) WHERE event=?1 AND status=1",
                [&event.id],
            )?;
            tx.commit()?;
            exact(db, &event)
        })
        .await
    }
    pub(crate) async fn pick_world_entitlement(
        &self,
        event: Event,
        ordinal: usize,
        operation: String,
        write: InventoryWrite,
        slot: usize,
    ) -> Result<i32> {
        ensure!(
            operation.len() == 32 && operation.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid pickup operation"
        );
        let pickup = Event {
            id: operation.clone(),
            kind: "inventory".into(),
            content: event.content.clone(),
            payload: EventPayload::Inventory(
                serde_json::json!({"write":write,"slot":slot,"entitlement":[event.id,ordinal]}),
            ),
        };
        let (pickup_payload, pickup_hash) = pickup.encoded()?;
        self.call(move|db| {
            let tx=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let record=exact(&tx,&event)?;ensure!(matches!(record.status,1|2),"entitlement event is not committed");
            tx.execute("INSERT INTO world_events VALUES(?1,'inventory',1,?2,?3,?4,0,NULL) ON CONFLICT(event) DO NOTHING",params![pickup.id,pickup.content,pickup_payload,pickup_hash])?;
            let prior=exact(&tx,&pickup)?;
            if prior.status!=0 {return i32::try_from(prior.result.context("pickup result absent")?["revision"].as_i64().context("pickup revision absent")?).context("pickup revision invalid")}

            let (owner,item,picked,status):(Option<i32>,i32,Option<String>,i32)=tx.query_row("SELECT owner_character,item,pickup_operation,status FROM world_entitlements WHERE event=?1 AND ordinal=?2",params![event.id,ordinal],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            ensure!(owner.is_none_or(|id|id==write.id),"entitlement belongs to another durable character");
            let revision=write.revision.checked_add(1).context("pickup revision overflow")?;
            ensure!(status==0 && picked.is_none(),"entitlement already picked; only its exact journal operation can replay");
            let raw:String=tx.query_row("SELECT items FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",params![write.id,write.account,write.revision],|r|r.get(0))?;
            let mut expected:Vec<i32>=serde_json::from_str(&raw)?;
            ensure!(expected.len()==12 && slot<12 && expected[slot]==-1,"pickup slot is not empty");expected[slot]=item;
            ensure!(write.items==expected,"pickup write changes more than allocated item");
            ensure!(tx.execute("UPDATE characters SET items=?4,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",params![write.id,write.account,write.revision,serde_json::to_string(&write.items)?])?==1,"pickup lost owner reservation");
            tx.execute("UPDATE world_entitlements SET status=1,pickup_operation=?3 WHERE event=?1 AND ordinal=?2 AND status=0",params![event.id,ordinal,operation])?;
            commit(&tx,&pickup,&serde_json::json!({"revision":revision}))?;
            tx.commit()?;Ok(revision)
        }).await
    }
}

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct Entitlement {
    pub ordinal: usize,
    pub owner: Owner,
    pub account: Option<i32>,
    pub item: u16,
}
impl Store {
    #[cfg(test)]
    pub(crate) async fn coordinate_terminal(&self, event: Event) -> Result<Record> {
        let id = event.id.clone();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            self.prepare_world_event(event.clone()).await?;
            match self.commit_terminal_event(event.clone()).await {
                Ok(record) => Ok(record),
                Err(error) => match self.world_event(event.id.clone()).await {
                    Ok(Some(record)) if record.event == event && matches!(record.status, 1 | 2) => {
                        Ok(record)
                    }
                    _ => Err(error.context(format!(
                        "terminal event {id} unresolved; original captured plan retained"
                    ))),
                },
            }
        })
        .await
        .with_context(|| {
            format!("terminal event {id} Store timeout; original plan retained, no reroll/refund")
        })?
    }
    pub(crate) async fn unpicked_entitlements(&self, event: Event) -> Result<Vec<Entitlement>> {
        self.call(move|db| {
            let record=exact(db,&event)?;ensure!(matches!(record.status,1|2),"uncommitted event cannot deliver");
            let mut q=db.prepare("SELECT ordinal,owner_character,item,(SELECT account FROM characters WHERE id=owner_character) FROM world_entitlements WHERE event=?1 AND status=0 ORDER BY ordinal")?;
            let rows=q.query_map([&event.id],|r|Ok((r.get::<_,usize>(0)?,r.get::<_,Option<i32>>(1)?,r.get::<_,u16>(2)?,r.get::<_,Option<i32>>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().map(|(ordinal,owner,item,account)| {ensure!(owner.is_none()||account.is_some(),"event {} unpicked character owner missing",event.id);Ok(Entitlement{ordinal,owner:match owner{Some(id)=>Owner::Character(id),None=>Owner::Public},account,item})}).collect()
        }).await
    }
}

impl Store {
    pub(crate) async fn lost_world_candidates(
        &self,
        live: Vec<String>,
    ) -> Result<Vec<(String, String)>> {
        self.call(move|db|{let mut q=db.prepare("SELECT event,coalesce(json_extract(payload,'$.payload.data.actions.instance'),json_extract(payload,'$.payload.data.origin.instance')) FROM world_events WHERE status IN (0,1,2) AND coalesce(json_extract(payload,'$.payload.data.actions.instance'),json_extract(payload,'$.payload.data.origin.instance')) IS NOT NULL AND coalesce(json_extract(payload,'$.payload.data.actions.instance'),json_extract(payload,'$.payload.data.origin.instance')) NOT IN (SELECT value FROM json_each(?1)) AND (status IN (0,1) OR EXISTS(SELECT 1 FROM world_entitlements WHERE world_entitlements.event=world_events.event AND status=0)) ORDER BY event LIMIT 8")?;Ok(q.query_map([serde_json::to_string(&live)?],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)}).await
    }
    pub(crate) async fn reconcile_confirmed_lost_events(
        &self,
        confirmed: Vec<(String, String)>,
    ) -> Result<Vec<(String, String)>> {
        self.call(move|db|{let tx=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut reconciled=Vec::new();
            for (id,origin) in confirmed {
                let record=read(&tx,&id)?.context("recovery event disappeared")?;
                if record.status==3 {continue;}
                let actual=match &record.event.payload {EventPayload::Terminal(p)=>p.actions["instance"].as_str(),EventPayload::Inventory(p)|EventPayload::Stamp(p)=>p["origin"]["instance"].as_str()};
                ensure!(actual==Some(origin.as_str()),"confirmed origin changed for event {id}");
                let disposition=if record.status==0 {"aborted_origin_lost"}else if record.event.kind=="inventory" {"committed_effect_origin_lost"}else{"origin_lost"};
                let source=match &record.event.payload {EventPayload::Terminal(p)=>p.actions["instance"].clone(),EventPayload::Inventory(p)|EventPayload::Stamp(p)=>p["origin"]["instance"].clone()};
                let at:i64=tx.query_row("SELECT unixepoch()",[],|r|r.get(0))?;
                let result=serde_json::json!({"reconciliation":{"disposition":disposition,"previous_status":record.status,"previous_result":record.result,"source":source,"at":at,"reason":"exact original world instance no longer exists; no delivery, refund, reroll or rebind"}});
                tx.execute("UPDATE world_entitlements SET status=2 WHERE event=?1 AND status=0",[&id])?;
                tx.execute("UPDATE world_events SET status=3,result=?2 WHERE event=?1",params![id,serde_json::to_string(&result)?])?;
                tracing::warn!(event=%id,%disposition,%source,"world journal origin-loss reconciliation");
                reconciled.push((id,disposition.to_owned()));
            }
            tx.commit()?;Ok(reconciled)
        }).await
    }
    /// Replaces one bounded batch of settled payloads with tombstones. A live world may still read its retained ids,
    /// so those keep their payload; any other replay is answered from the original hash and status.
    pub(crate) async fn compact_settled_world_events(
        &self,
        live_events: Vec<String>,
    ) -> Result<usize> {
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let events = tx.prepare(&format!("SELECT event FROM world_events e WHERE {SETTLED} AND json_type(payload,'$.compacted') IS NULL
                AND event NOT IN (SELECT value FROM json_each(?1)) ORDER BY event LIMIT ?2"))?
                .query_map(params![serde_json::to_string(&live_events)?, JOURNAL_PRUNE_BATCH], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut released = 0;
            for id in &events {
                // The payload is the evidence for this row, so it must validate before it is discarded.
                validate_event(&tx, id).with_context(|| format!("event {id} not compacted"))?;
                let record = read(&tx, id)?.context("compaction candidate disappeared")?;
                let payload = serde_json::to_string(&Compacted { compacted: Tombstone::of(&record.event)? })?;
                released += tx.query_row("SELECT length(payload) FROM world_events WHERE event=?1", [id], |r| r.get::<_, usize>(0))? - payload.len();
                ensure!(tx.execute("UPDATE world_events SET payload=?2 WHERE event=?1 AND status=2", params![id, payload])? == 1, "event {id} settlement changed during compaction");
            }
            tx.commit()?;
            if !events.is_empty() {
                tracing::info!(events = events.len(), payload_bytes_released = released, "compacted settled world journal payloads");
            }
            Ok(events.len())
        })
        .await
    }
    /// Deletes one bounded batch of published history whose entitlements and pickups are all settled.
    /// Live events and status-3 reconciliation rows are never candidates; live origins only keep entitlement-bearing rows.
    pub(crate) async fn prune_settled_world_events(
        &self,
        live_instances: Vec<String>,
        live_events: Vec<String>,
        now: i64,
    ) -> Result<(usize, usize)> {
        self.call(move |db| {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let candidates = tx.prepare(&format!("SELECT event,progress_only FROM (SELECT event,payload,result,kind='terminal' AND json_type(payload,'$.compacted') IS NOT NULL
                AND NOT EXISTS(SELECT 1 FROM world_entitlements w WHERE w.event=e.event) AS progress_only FROM world_events e WHERE {SETTLED})
                WHERE json_extract(result,'$.published_unix')<=iif(progress_only,?5,?1)
                AND (progress_only OR coalesce(json_extract(payload,'$.payload.data.actions.instance'),json_extract(payload,'$.payload.data.origin.instance'),json_extract(payload,'$.compacted.instance'),'') NOT IN (SELECT value FROM json_each(?2)))
                AND event NOT IN (SELECT value FROM json_each(?3)) ORDER BY event LIMIT ?4"))?
                .query_map(params![now - JOURNAL_RETENTION_S, serde_json::to_string(&live_instances)?, serde_json::to_string(&live_events)?, JOURNAL_PRUNE_BATCH, now - PROGRESS_ONLY_RETENTION_S], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let progress_only = candidates.iter().filter(|(_, fast)| *fast).count();
            let events = serde_json::to_string(&candidates.into_iter().map(|(id, _)| id).collect::<Vec<_>>())?;
            let entitlements = tx.execute("DELETE FROM world_entitlements WHERE event IN (SELECT value FROM json_each(?1))", [&events])?;
            let pruned = tx.execute("DELETE FROM world_events WHERE event IN (SELECT value FROM json_each(?1))", [&events])?;
            tx.commit()?;
            if pruned > 0 {
                tracing::info!(events = pruned, progress_only, entitlements, retention_s = JOURNAL_RETENTION_S, progress_only_retention_s = PROGRESS_ONLY_RETENTION_S, "pruned settled world journal history");
            }
            Ok((pruned, entitlements))
        })
        .await
    }
    #[cfg(test)]
    pub(crate) async fn reconcile_lost_world_events(
        &self,
        live: Vec<String>,
    ) -> Result<Vec<(String, String)>> {
        // Component fixtures own their complete world census; production confirms each candidate through Directory.
        self.reconcile_confirmed_lost_events(self.lost_world_candidates(live).await?)
            .await
    }
    pub(crate) async fn expire_world_entitlements(
        &self,
        event: Event,
        proofs: Vec<(usize, i32, u64, u64)>,
    ) -> Result<()> {
        self.call(move|db| {
            let tx=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let record=exact(&tx,&event)?;ensure!(matches!(record.status,1|2),"expiry requires original committed event");
            let mut result=record.result.context("expiry commit receipt absent")?;
            if result.get("expired").is_none(){result["expired"]=serde_json::json!({});}
            for (ordinal,bag,deadline,now) in proofs {
                ensure!(bag>0&&now>=deadline,"expiry proof precedes original bag lifetime");
                if tx.execute("UPDATE world_entitlements SET status=2 WHERE event=?1 AND ordinal=?2 AND status=0",params![event.id,ordinal])?==1 {
                    result["expired"][ordinal.to_string()]=serde_json::json!({"bag":bag,"deadline":deadline,"now":now,"reason":"published native bag lifetime ended"});
                }
            }
            tx.execute("UPDATE world_events SET result=?2 WHERE event=?1",params![event.id,serde_json::to_string(&result)?])?;tx.commit()?;Ok(())
        }).await
    }
}

impl Store {
    pub(crate) async fn record_late_inventory_publication(
        &self,
        event: Event,
        deadline: u64,
        at: u64,
    ) -> Result<()> {
        self.call(move|db| {
            ensure!(at>=deadline,"late publication precedes original deadline");
            let tx=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let record=exact(&tx,&event)?;ensure!(event.kind=="inventory"&&matches!(record.status,1|2),"late publication requires committed inventory outcome");
            let payload=event.native_payload()?;
            ensure!(payload["portal"]["locked_deadline"].as_u64()==Some(deadline)&&payload["origin"]["accepted_at"].as_u64().is_some_and(|request|request<deadline),"late publication lacks exact accepted-before-deadline proof");
            let mut result=record.result.context("late publication receipt absent")?;
            let publication=serde_json::json!({"disposition":"published_after_source_deadline","source_deadline":deadline,"published_at":at,"late_ms":at-deadline});
            if let Some(existing)=result.get("publication"){ensure!(*existing==publication,"late publication replay changed causal time");}else{result["publication"]=publication;}
            tx.execute("UPDATE world_events SET result=?2 WHERE event=?1",params![event.id,serde_json::to_string(&result)?])?;tx.commit()?;
            tracing::warn!(event=%event.id,deadline,published_at=at,late_ms=at-deadline,"committed effect published after original source deadline; original clock unchanged");Ok(())
        }).await
    }
}
