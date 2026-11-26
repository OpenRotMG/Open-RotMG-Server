//! Authored allocation law over linked XML rows; no world, account or delivery effects.
use crate::content::game::{GameData, LandmarkChest, LootOwnership, LootRow};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, num::NonZeroUsize};

pub const ALLOCATION_VERSION: &str = "xml-loot-allocation-v1";
pub const PLAN_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DefeatContext {
    pub world: u64,
    pub encounter: Option<u64>,
    pub actor: i32,
    pub event: u64,
    pub kind: u16,
}

/// Damage is already effective HP damage, not attempted damage/overkill or party share.
/// The coordinator owns per-hit capping, transformation transfer and terminal deduplication.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub character: i32,
    pub world: u64,
    pub encounter: Option<u64>,
    pub effective_damage: u64,
    pub alive: bool,
    pub departing: bool,
}

/// Required operational work bounds, not source min/max or transient delivery capacity.
#[derive(Clone, Copy, Debug)]
pub struct PlanningLimits {
    pub max_allocations: NonZeroUsize,
    pub max_contributions: NonZeroUsize,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    Public,
    Character(i32),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Allocation {
    row: usize,
    owner: Owner,
    item: u16,
}
impl Allocation {
    #[cfg(test)]
    pub fn row(&self) -> usize {
        self.row
    }
    pub fn owner(&self) -> Owner {
        self.owner
    }
    pub fn item(&self) -> u16 {
        self.item
    }
}

/// An entitlement snapshot, NOT a commit. Persist/cache it before delivery; never reroll on retry.
/// Row indices are meaningful only against both exact fingerprints and allocation version.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AllocationPlan {
    plan_version: u32,
    allocation_version: String,
    content_fingerprint: String,
    rules_fingerprint: String,
    defeat: DefeatContext,
    seed: u64,
    allocations: Vec<Allocation>,
}
impl AllocationPlan {
    pub fn allocations(&self) -> &[Allocation] {
        &self.allocations
    }
    pub fn defeat(&self) -> DefeatContext {
        self.defeat
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn content_fingerprint(&self) -> &str {
        &self.content_fingerprint
    }

    /// Exact sampled item demand. Native packing/reservation/8-slot and 128-live-bag limits
    /// belong to the coordinator; low availability must retain this plan, not redraw it.
    pub fn demand(&self) -> BTreeMap<Owner, usize> {
        let mut counts = BTreeMap::new();
        for allocation in &self.allocations {
            *counts.entry(allocation.owner).or_default() += 1;
        }
        counts
    }

    /// Validate a restored journal against its original event/ledger and exact rules, without RNG.
    /// Authentication/integrity and once-only durable publication are the coordinator's obligation.
    pub fn validate(
        &self,
        game: &GameData,
        defeat: DefeatContext,
        ledger: &[Contribution],
        limits: PlanningLimits,
    ) -> Result<()> {
        (|| {
            ensure!(
                self.plan_version == PLAN_VERSION && self.allocation_version == ALLOCATION_VERSION,
                "incompatible allocation plan version"
            );
            ensure!(
                self.defeat == defeat,
                "allocation plan defeat context mismatch"
            );
            let checked = preflight(game, defeat, ledger, limits)?;
            ensure!(
                self.content_fingerprint == game.fingerprint
                    && self.rules_fingerprint == checked.rules_fingerprint,
                "allocation plan content/rules fingerprint mismatch; reconciliation required"
            );
            ensure!(
                self.allocations.len() <= limits.max_allocations.get(),
                "restored plan exceeds planning limit"
            );
            let mut counts = vec![0usize; checked.rows.len()];
            let mut owner_counts = BTreeMap::<(usize, Owner), usize>::new();
            for allocation in &self.allocations {
                let row = checked
                    .rows
                    .get(allocation.row)
                    .context("allocation row outside recorded definition")?;
                ensure!(
                    row.pool.binary_search(&allocation.item).is_ok(),
                    "allocation item outside linked pool"
                );
                match (&row.ownership, allocation.owner) {
                    (LootOwnership::Public, Owner::Public) => ensure!(
                        public_gate(row, &checked.contributors, checked.hp),
                        "public allocation gate failed"
                    ),
                    (LootOwnership::Private, Owner::Character(character)) => ensure!(
                        checked
                            .contributors
                            .get(&character)
                            .is_some_and(|damage| qualifies(row, *damage, checked.hp)),
                        "private allocation owner is ineligible"
                    ),
                    _ => anyhow::bail!("allocation ownership mismatch"),
                }
                counts[allocation.row] = counts[allocation.row]
                    .checked_add(1)
                    .context("allocation count overflow")?;
                let count = owner_counts.entry((allocation.row, allocation.owner)).or_default();
                *count = count.checked_add(1).context("allocation owner count overflow")?;
            }
            for (index, row) in checked.rows.iter().enumerate() {
                let n = recipient_count(row, &checked.contributors, checked.hp);
                let min = usize::try_from(row.min)?;
                let upper = row_bound(row, n)?;
                let lower = if row.probability == 1. { upper } else if n == 0 { 0 } else { min };
                ensure!(
                    counts[index] >= lower && counts[index] <= upper,
                    "restored row {index} {}:{} count {} outside {lower}..={upper}; reconciliation required",
                    row.source.file, row.source.line, counts[index]
                );
                if n > 0 && matches!(row.ownership, LootOwnership::Private) {
                    let (owner_min, owner_max) = if min <= n {
                        (0, 1)
                    } else {
                        let q = min / n;
                        (q, q.checked_add(1).context("owner multiplicity bound overflow")?)
                    };
                    for character in eligible(row, &checked.contributors, checked.hp) {
                        let count = owner_counts.get(&(index, Owner::Character(character))).copied().unwrap_or(0);
                        ensure!(
                            (owner_min..=owner_max).contains(&count),
                            "restored row {index} {}:{} character={character} count={count} outside {owner_min}..={owner_max}; reconciliation required",
                            row.source.file, row.source.line
                        );
                    }
                }
            }
            Ok(())
        })()
        .with_context(|| {
            format!(
                "restore loot kind={:#x} actor={} event={}",
                defeat.kind, defeat.actor, defeat.event
            )
        })
    }
}

trait Draws {
    fn unit(&mut self) -> Result<f64>;
    fn index(&mut self, upper: usize) -> Result<usize>;
}
struct Seeded(StdRng);
impl Draws for Seeded {
    fn unit(&mut self) -> Result<f64> {
        Ok(self.0.r#gen())
    }
    fn index(&mut self, upper: usize) -> Result<usize> {
        Ok(self.0.gen_range(0..upper))
    }
}
fn index(draws: &mut impl Draws, upper: usize) -> Result<usize> {
    let value = draws.index(upper)?;
    ensure!(value < upper, "loot draw index {value} outside 0..{upper}");
    Ok(value)
}
fn success(draws: &mut impl Draws, probability: f64) -> Result<bool> {
    if probability == 0. {
        return Ok(false);
    }
    if probability == 1. {
        return Ok(true);
    }
    let value = draws.unit()?;
    ensure!(
        value.is_finite() && (0. ..1.).contains(&value),
        "loot unit draw outside [0,1)"
    );
    Ok(value < probability)
}

struct Checked<'a> {
    rows: &'a [LootRow],
    hp: u64,
    contributors: BTreeMap<i32, u64>,
    rules_fingerprint: String,
}

fn checked_rows(game: &GameData, kind: u16) -> Result<(&[LootRow], u64, String)> {
    (|| {
        let definition = game
            .definitions
            .get(&kind)
            .context("missing declared definition")?;
        game.admission(kind, definition.admission_role()?, "loot definition")?;
        let property = game
            .properties
            .get(&kind)
            .context("missing selected properties")?;
        ensure!(property.hp >= 0, "negative selected maxHP");
        ensure!(
            property.hp > 0 || definition.loot.is_empty(),
            "loot-bearing definition needs positive selected maxHP"
        );
        let mut items = check_rows(game, &definition.loot, &property.name)?;
        items.insert(kind, property);
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                ALLOCATION_VERSION,
                definition,
                items
            ))?)
        );
        Ok((
            definition.loot.as_slice(),
            u64::try_from(property.hp)?,
            fingerprint,
        ))
    })()
    .with_context(|| format!("loot kind={kind:#x}"))
}
fn check_rows<'a>(
    game: &'a GameData,
    rows: &[LootRow],
    object: &str,
) -> Result<BTreeMap<u16, &'a crate::content::Object>> {
    let mut items = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        (|| {
            ensure!(
                row.probability.is_finite() && (0. ..=1.).contains(&row.probability),
                "invalid probability"
            );
            ensure!(
                row.threshold.is_finite() && (0. ..=1.).contains(&row.threshold),
                "invalid threshold"
            );
            ensure!(row.max.is_none_or(|max| row.min <= max), "min exceeds max");
            ensure!(!row.pool.is_empty(), "empty linked pool");
            ensure!(
                row.pool.windows(2).all(|pair| pair[0] < pair[1]),
                "linked pool must be sorted and unique"
            );
            ensure!(
                row.category.is_some() == row.tier.is_some(),
                "category/tier mismatch"
            );
            ensure!(
                row.category.is_some() || row.pool.len() == 1,
                "literal item row must have one linked item"
            );
            ensure!(row.source.object == object, "row source identity mismatch");
            for item in &row.pool {
                let selected = game
                    .properties
                    .get(item)
                    .context("pool item missing selected properties")?;
                game.admission(
                    *item,
                    crate::content::game::AdmissionRole::Native(
                        crate::content::game::NativeService::Inventory,
                    ),
                    "loot pool item",
                )?;
                ensure!(
                    row.tier.is_none_or(|tier| selected.tier == Some(tier)),
                    "pool item selected tier mismatch"
                );
                ensure!(
                    !matches!(row.ownership, LootOwnership::Public) || !selected.soulbound,
                    "public row contains selected soulbound item {item:#x}"
                );
                items.insert(*item, selected);
            }
            Ok(())
        })()
        .with_context(|| {
            format!(
                "row={index} {}:{} object={} path={}",
                row.source.file, row.source.line, row.source.object, row.source.path
            )
        })?;
    }
    Ok(items)
}

fn contributors(
    defeat: DefeatContext,
    ledger: &[Contribution],
    limits: PlanningLimits,
) -> Result<BTreeMap<i32, u64>> {
    ensure!(defeat.actor > 0, "invalid defeated actor identity");
    ensure!(
        ledger.len() <= limits.max_contributions.get(),
        "contribution input count {} exceeds planning limit {}",
        ledger.len(),
        limits.max_contributions
    );
    let mut states = BTreeMap::new();
    let mut damage = BTreeMap::<i32, u64>::new();
    for entry in ledger {
        ensure!(entry.character > 0, "invalid durable character identity");
        if entry.world != defeat.world || entry.encounter != defeat.encounter {
            continue;
        }
        let status = (entry.alive, entry.departing);
        if let Some(previous) = states.insert(entry.character, status) {
            ensure!(
                previous == status,
                "inconsistent defeat-time status for character {}",
                entry.character
            );
        }
        let total = damage.entry(entry.character).or_default();
        *total = total
            .checked_add(entry.effective_damage)
            .with_context(|| format!("effective damage overflow character={}", entry.character))?;
    }
    damage.retain(|character, total| *total > 0 && states[character] == (true, false));
    Ok(damage)
}
fn qualifies(row: &LootRow, damage: u64, hp: u64) -> bool {
    damage > 0 && damage as f64 / hp as f64 >= row.threshold
}
fn eligible(row: &LootRow, contributors: &BTreeMap<i32, u64>, hp: u64) -> Vec<i32> {
    contributors
        .iter()
        .filter(|(_, damage)| qualifies(row, **damage, hp))
        .map(|(id, _)| *id)
        .collect()
}
fn public_gate(row: &LootRow, contributors: &BTreeMap<i32, u64>, hp: u64) -> bool {
    row.threshold == 0.
        || contributors
            .values()
            .any(|damage| qualifies(row, *damage, hp))
}
fn recipient_count(row: &LootRow, contributors: &BTreeMap<i32, u64>, hp: u64) -> usize {
    match row.ownership {
        LootOwnership::Public => usize::from(public_gate(row, contributors, hp)),
        LootOwnership::Private => contributors
            .values()
            .filter(|damage| qualifies(row, **damage, hp))
            .count(),
    }
}
fn row_bound(row: &LootRow, n: usize) -> Result<usize> {
    if n == 0 || row.max == Some(0) {
        return Ok(0);
    }
    let min = usize::try_from(row.min).context("row minimum overflows platform count")?;
    let attempts = if row.probability == 0. { 0 } else { n };
    let bound = attempts.max(min);
    Ok(match row.max {
        Some(max) => bound.min(usize::try_from(max)?),
        None => bound,
    })
}
fn preflight<'a>(
    game: &'a GameData,
    defeat: DefeatContext,
    ledger: &[Contribution],
    limits: PlanningLimits,
) -> Result<Checked<'a>> {
    let (rows, hp, rules_fingerprint) = checked_rows(game, defeat.kind)?;
    let contributors = contributors(defeat, ledger, limits)?;
    let mut guaranteed = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let n = recipient_count(row, &contributors, hp);
        let _ = row_bound(row, n)?;
        if n > 0 {
            guaranteed = guaranteed
                .checked_add(usize::try_from(row.min)?)
                .context("minimum total overflow")?;
            ensure!(
                guaranteed <= limits.max_allocations.get(),
                "planning allocation minimum exceeds limit: row={index} {}:{} needed={guaranteed} limit={}",
                row.source.file,
                row.source.line,
                limits.max_allocations
            );
        }
    }
    Ok(Checked {
        rows,
        hp,
        contributors,
        rules_fingerprint,
    })
}

pub fn evaluate(
    game: &GameData,
    defeat: DefeatContext,
    ledger: &[Contribution],
    seed: u64,
    limits: PlanningLimits,
) -> Result<AllocationPlan> {
    // The event seed is captured by the coordinator, never obtained from a shared RNG here.
    evaluate_draws(
        game,
        defeat,
        ledger,
        seed,
        limits,
        &mut Seeded(StdRng::seed_from_u64(seed)),
    )
}
fn evaluate_draws(
    game: &GameData,
    defeat: DefeatContext,
    ledger: &[Contribution],
    seed: u64,
    limits: PlanningLimits,
    draws: &mut impl Draws,
) -> Result<AllocationPlan> {
    (|| -> Result<AllocationPlan> {
        let checked = preflight(game, defeat, ledger, limits)?;
        let allocations = allocate(checked.rows, None, limits, draws, |row| {
            match row.ownership {
                LootOwnership::Public if public_gate(row, &checked.contributors, checked.hp) => {
                    vec![Owner::Public]
                }
                LootOwnership::Public => Vec::new(),
                LootOwnership::Private => eligible(row, &checked.contributors, checked.hp)
                    .into_iter()
                    .map(Owner::Character)
                    .collect(),
            }
        })?;
        Ok(AllocationPlan {
            plan_version: PLAN_VERSION,
            allocation_version: ALLOCATION_VERSION.into(),
            content_fingerprint: game.fingerprint.clone(),
            rules_fingerprint: checked.rules_fingerprint,
            defeat,
            seed,
            allocations,
        })
    })()
    .with_context(|| {
        format!(
            "evaluate loot kind={:#x} world={} encounter={:?} actor={} event={} seed={seed}",
            defeat.kind, defeat.world, defeat.encounter, defeat.actor, defeat.event
        )
    })
}
fn allocate(
    rows: &[LootRow],
    ceiling: Option<usize>,
    limits: PlanningLimits,
    draws: &mut impl Draws,
    recipients: impl Fn(&LootRow) -> Vec<Owner>,
) -> Result<Vec<Allocation>> {
    let mut allocations = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        (|| -> Result<()> {
            // A reached success ceiling ends all later row draws.
            let max = row.max.map(usize::try_from).transpose()?.unwrap_or(usize::MAX)
                .min(ceiling.map_or(usize::MAX, |c| c.saturating_sub(allocations.len())));
            if max == 0 || (row.probability == 0. && row.min == 0) { return Ok(()); }
            let mut order = recipients(row);
            if order.is_empty() { return Ok(()); }
            for i in (1..order.len()).rev() {
                let j = index(draws, i + 1)?;
                order.swap(i, j);
            }
            let mut winners = Vec::new();
            let mut succeeded = vec![false; order.len()];
            let mut add = |owner| -> Result<()> {
                let needed = allocations.len().checked_add(winners.len()).and_then(|n| n.checked_add(1)).context("allocation count overflow")?;
                ensure!(needed <= limits.max_allocations.get(), "sampled planning allocation limit exceeded: needed={needed} limit={}; retain event seed, no accepted plan", limits.max_allocations);
                winners.try_reserve(1).context("reserve sampled recipients")?;
                winners.push(owner);
                Ok(())
            };
            // Pool choices happen only after all recipient draws/top-ups for this row.
            let mut successes = 0usize;
            for (position, owner) in order.iter().enumerate() {
                if successes == max { break; }
                if success(draws, row.probability)? {
                    add(*owner)?;
                    succeeded[position] = true;
                    successes += 1;
                }
            }
            let min = usize::try_from(row.min)?;
            for (position, owner) in order.iter().enumerate() {
                if successes >= min { break; }
                if !succeeded[position] { add(*owner)?; successes += 1; }
            }
            let mut cycle = 0;
            while successes < min {
                add(order[cycle % order.len()])?;
                cycle += 1;
                successes += 1;
            }
            allocations.try_reserve(winners.len()).context("reserve sampled allocations")?;
            for owner in winners {
                let choice = if row.pool.len() == 1 { 0 } else { index(draws, row.pool.len())? };
                allocations.push(Allocation { row: row_index, owner, item: row.pool[choice] });
            }
            Ok(())
        })().with_context(|| format!("row={row_index} {}:{} object={} path={}", row.source.file, row.source.line, row.source.object, row.source.path))?;
    }
    Ok(allocations)
}

fn landmark_chest_rows(game: &GameData, landmark: u16) -> Result<&LandmarkChest> {
    let chest = game
        .landmark_chests
        .get(&landmark)
        .with_context(|| format!("landmark {landmark:#x} has no declared chest"))?;
    let name = &game
        .properties
        .get(&landmark)
        .context("landmark lacks selected properties")?
        .name;
    check_rows(game, &chest.rows, name)?;
    ensure!(
        chest
            .rows
            .iter()
            .all(|r| matches!(r.ownership, LootOwnership::Public)
                && r.threshold == 0.
                && r.min == 0
                && r.max.is_none()),
        "landmark chest rows must be ungated public rows"
    );
    Ok(chest)
}
/// Construction-time public chest contents from a caller-captured seed; no contributors or durable owners.
pub fn landmark_chest(game: &GameData, landmark: u16, seed: u64) -> Result<Vec<u16>> {
    landmark_chest_draws(game, landmark, &mut Seeded(StdRng::seed_from_u64(seed)))
        .with_context(|| format!("landmark chest {landmark:#x} seed={seed}"))
}
fn landmark_chest_draws(
    game: &GameData,
    landmark: u16,
    draws: &mut impl Draws,
) -> Result<Vec<u16>> {
    let chest = landmark_chest_rows(game, landmark)?;
    // The ceiling is drawn before any row, as realm-src GetLoots(min, max) does.
    let ceiling = chest
        .ceiling
        .map(|(low, high)| -> Result<usize> {
            Ok(usize::try_from(low)? + index(draws, usize::try_from(high - low)? + 1)?)
        })
        .transpose()?;
    let limits = PlanningLimits {
        max_allocations: NonZeroUsize::new(chest.rows.len().max(1)).unwrap(),
        max_contributions: NonZeroUsize::MIN,
    };
    Ok(
        allocate(&chest.rows, ceiling, limits, draws, |_| vec![Owner::Public])?
            .into_iter()
            .map(|a| a.item)
            .collect(),
    )
}

fn row_facts(rows: &[LootRow], gate: impl Fn(&LootRow) -> &'static str) -> Vec<Value> {
    rows.iter().enumerate().map(|(index, row)| {
        json!({"row_index":index,"source_row":row,"eligibility":gate(row),
            "bernoulli_attempt_probability":row.probability,
            "uniform_pool_choice_probability":1. / row.pool.len() as f64,
            "attempt_item_probability_before_min_max":row.probability / row.pool.len() as f64,
            "final_probability":{"status":"not_projected","reason":"Attempt and pool probabilities are not final per-character chances. Final allocations depend on eligibility, seeded fair order, encounter-total minimum/maximum and repeated independent rows."},
            "minimum_scope":"total row allocations per encounter, only when private recipients/public gate exist",
            "maximum_scope":"total row successes; stop Bernoullis at maximum before minimum top-up",
            "guarantee_is_per_player":false})
    }).collect()
}
pub(crate) fn definition_facts(game: &GameData, kind: u16) -> Result<Value> {
    let (rows, hp, rules_fingerprint) = checked_rows(game, kind)?;
    let definition = &game.definitions[&kind];
    let property = &game.properties[&kind];
    let facts = row_facts(rows, |row| match row.ownership {
        LootOwnership::Private => {
            "each living nondeparting durable character in owning world/encounter with positive aggregated effective HP damage and damage/maxHP >= threshold"
        }
        LootOwnership::Public => {
            "once per defeat; threshold=0 needs no contributor, threshold>0 requires at least one otherwise eligible character meeting damage/maxHP >= threshold"
        }
    });
    Ok(json!({"id":kind,"name":property.name,"selected_max_hp":hp,
        "provenance":definition.provenance,"origin":definition.origin,"reason":definition.reason,
        "source":definition.source,"rules_fingerprint":rules_fingerprint,"rows":facts}))
}
pub(crate) fn landmark_chest_facts(game: &GameData, landmark: u16) -> Result<Value> {
    let chest = landmark_chest_rows(game, landmark)?;
    Ok(
        json!({"landmark":landmark,"landmark_name":game.properties[&landmark].name,"container":chest.container,
        "condition":"Public contents of each landmark chest created with this landmark's Realm site; not the landmark's death loot and not every container of this kind.",
        "success_ceiling":chest.ceiling.map(|(low, high)| json!({"min":low,"max":high,"draw":"uniform inclusive, before rows; later rows stop drawing once reached"})),
        "provenance":chest.provenance,"origin":chest.origin,"reason":chest.reason,"source":chest.source,
        "rows":row_facts(&chest.rows, |_| "once per chest creation; no contributor or damage gate")}),
    )
}

#[cfg(test)]
mod tests;
