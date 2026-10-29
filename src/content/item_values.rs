use super::{Object, Result, ensure};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const CATALOG: &str = include_str!("../../data/bot-item-values.json");
pub(crate) const DEFENSE: i32 = 2592;

/// Game reference units: 12 = one DEF by definition, not an observed market price.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ValueRange {
    pub min: u64,
    pub max: u64,
}
impl ValueRange {
    pub fn overlaps(self, quantity: u8, other: Self, other_quantity: u8) -> bool {
        self.min * u64::from(quantity) <= other.max * u64::from(other_quantity)
            && other.min * u64::from(other_quantity) <= self.max * u64::from(quantity)
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Evidence {
    pub record: String,
    pub source: String,
    pub kind: String,
    pub date: String,
    pub build: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Relation {
    pub denomination: i32,
    /// Twelfths of the historical payment item, never native fractional inventory.
    pub quantity: ValueRange,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Valuation {
    pub range: Option<ValueRange>,
    pub relation: Option<Relation>,
    pub evidence: Option<Evidence>,
    pub ineligible_reason: Option<String>,
}
#[derive(Clone)]
pub(crate) struct ItemValues(BTreeMap<i32, Valuation>);

#[derive(Deserialize)]
struct Catalog {
    schema_version: u8,
    target: Target,
    sources: BTreeMap<String, Source>,
    items: Vec<Item>,
    records: Vec<Record>,
}
#[derive(Deserialize)]
struct Target {
    client_build: String,
}
#[derive(Deserialize)]
struct Source {
    r#type: String,
    url: String,
    date: String,
    date_basis: String,
    build: Option<String>,
}
#[derive(Deserialize)]
struct Item {
    item_id: u16,
    name: String,
}
#[derive(Deserialize)]
struct Record {
    id: String,
    source: String,
    kind: String,
    status: String,
    items: Option<Vec<Component>>,
    payments: Option<Vec<Vec<Component>>>,
    #[serde(default)]
    constraints: Vec<Vec<Component>>,
    #[serde(default)]
    payment_conditions: BTreeMap<usize, String>,
    reason: Option<String>,
    #[serde(default)]
    subject_item_ids: Vec<u16>,
}
#[derive(Deserialize)]
struct Component {
    item_id: u16,
    quantity: Quantity,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Quantity {
    Exact([u64; 2]),
    Range {
        min: Option<[u64; 2]>,
        max: Option<[u64; 2]>,
        min_inclusive: bool,
        max_inclusive: bool,
    },
}
impl Quantity {
    fn validate(&self) -> Result<()> {
        let rational = |r: [u64; 2]| -> Result<()> {
            ensure!(
                r.iter().all(|n| (1..=1_000_000).contains(n)),
                "invalid value rational {r:?}"
            );
            Ok(())
        };
        match *self {
            Self::Exact(r) => rational(r)?,
            Self::Range {
                min,
                max,
                min_inclusive,
                max_inclusive,
            } => {
                ensure!(min.is_some() || max.is_some(), "value range has no bounds");
                for r in [min, max].into_iter().flatten() {
                    rational(r)?;
                }
                ensure!(min.is_some() || !min_inclusive, "inclusive absent minimum");
                ensure!(max.is_some() || !max_inclusive, "inclusive absent maximum");
                if let (Some(a), Some(b)) = (min, max) {
                    let (left, right) = (a[0] * b[1], b[0] * a[1]);
                    ensure!(
                        left < right || (left == right && min_inclusive && max_inclusive),
                        "unordered value range"
                    );
                }
            }
        }
        Ok(())
    }
    fn units(&self, denomination: ValueRange) -> Result<Option<ValueRange>> {
        let (min, max) = match *self {
            Self::Exact(r) => (r, r),
            Self::Range {
                min: Some(min),
                max: Some(max),
                min_inclusive: true,
                max_inclusive: true,
            } => (min, max),
            _ => return Ok(None),
        };
        let scale = |r: [u64; 2], units: u64| -> Result<u64> {
            let n = r[0].checked_mul(units).context("value overflow")?;
            ensure!(
                n % r[1] == 0,
                "value is not representable in reference units"
            );
            Ok(n / r[1])
        };
        Ok(Some(ValueRange {
            min: scale(min, denomination.min)?,
            max: scale(max, denomination.max)?,
        }))
    }
}
impl ItemValues {
    pub fn load(objects: &BTreeMap<u16, Object>) -> Result<Self> {
        Self::parse(CATALOG, objects).context("initialize bot item value catalog")
    }
    fn parse(json: &str, objects: &BTreeMap<u16, Object>) -> Result<Self> {
        let catalog: Catalog = serde_json::from_str(json).context("invalid item value JSON")?;
        ensure!(
            catalog.schema_version == 1 && catalog.target.client_build == "123.5.1",
            "unsupported item value catalog"
        );
        let mut identities = BTreeSet::new();
        for item in &catalog.items {
            ensure!(
                identities.insert(item.item_id),
                "duplicate catalog item {}",
                item.item_id
            );
            let object = objects
                .get(&item.item_id)
                .context("unknown catalog XML item")?;
            ensure!(
                object.name == item.name && object.class == "Equipment",
                "catalog XML identity mismatch: {}",
                item.name
            );
        }
        let mut records = BTreeSet::new();
        for record in &catalog.records {
            ensure!(
                records.insert(&record.id),
                "duplicate value record {}",
                record.id
            );
            let source = catalog
                .sources
                .get(&record.source)
                .context("unknown value source")?;
            ensure!(
                !source.url.is_empty() && !source.date.is_empty() && !source.date_basis.is_empty(),
                "incomplete value source"
            );
            ensure!(
                matches!(source.r#type.as_str(), "written_guide" | "video"),
                "invalid value source type"
            );
            ensure!(
                matches!(
                    record.kind.as_str(),
                    "guide_estimate" | "ask" | "bid" | "completed_deal"
                ),
                "invalid value record kind"
            );
            ensure!(
                matches!(record.status.as_str(), "clear" | "qualified" | "unusable"),
                "invalid value status"
            );
            ensure!(
                (record.status == "unusable") == record.payments.is_none(),
                "invalid price quarantine {}",
                record.id
            );
            if record.status == "unusable" {
                ensure!(
                    record.reason.as_ref().is_some_and(|s| !s.is_empty()),
                    "missing quarantine reason"
                );
            }
            for id in &record.subject_item_ids {
                ensure!(identities.contains(id), "unknown price subject");
            }
            if let Some(payments) = &record.payments {
                ensure!(
                    !payments.is_empty() && record.items.is_some(),
                    "missing priced basket"
                );
                ensure!(
                    record
                        .payment_conditions
                        .keys()
                        .all(|i| *i < payments.len()),
                    "invalid conditional payment index"
                );
            }
            for basket in record
                .items
                .iter()
                .chain(record.payments.iter().flatten())
                .chain(&record.constraints)
            {
                ensure!(!basket.is_empty(), "empty value basket");
                let mut seen = BTreeSet::new();
                for part in basket {
                    ensure!(
                        identities.contains(&part.item_id) && seen.insert(part.item_id),
                        "invalid value basket identity"
                    );
                    part.quantity
                        .validate()
                        .with_context(|| format!("value record {}", record.id))?;
                }
            }
        }
        // Fixed W-only, first-payment policy: never try an older video to repair a guide row.
        ensure!(
            catalog
                .sources
                .get("W")
                .is_some_and(|s| s.r#type == "written_guide"),
            "selected guide source missing or wrong kind"
        );
        let selected: BTreeMap<_, _> = catalog
            .records
            .iter()
            .filter(|r| r.source == "W")
            .map(|r| {
                ensure!(
                    r.kind == "guide_estimate",
                    "selected source is not a guide estimate"
                );
                let items = r.items.as_ref().context("guide has no subject")?;
                ensure!(
                    items.len() == 1 && matches!(items[0].quantity, Quantity::Exact([1, 1])),
                    "guide subject is not one canonical item"
                );
                Ok((i32::from(items[0].item_id), r))
            })
            .collect::<Result<_>>()?;
        ensure!(
            selected.len() == catalog.records.iter().filter(|r| r.source == "W").count(),
            "duplicate guide subject"
        );
        let mut denominations = BTreeMap::from([(DEFENSE, ValueRange { min: 12, max: 12 })]);
        // These are the guide's explicit currency relationships, not a conversion graph.
        for (kind, id) in [
            (2593, "w001"),
            (2636, "w002"),
            (2793, "w006"),
            (2878, "w112"),
        ] {
            let record = selected.get(&kind).context("missing guide denomination")?;
            ensure!(
                record.id == id && record.status == "clear",
                "invalid guide denomination"
            );
            let payment = &record
                .payments
                .as_ref()
                .context("missing denomination price")?[0];
            ensure!(
                payment.len() == 1 && i32::from(payment[0].item_id) == DEFENSE,
                "denomination must reference DEF"
            );
            denominations.insert(
                kind,
                payment[0]
                    .quantity
                    .units(denominations[&DEFENSE])?
                    .context("unbounded denomination")?,
            );
        }
        let mut values = BTreeMap::new();
        for item in catalog.items {
            let kind = i32::from(item.item_id);
            let mut value = Valuation {
                range: None,
                relation: None,
                evidence: None,
                ineligible_reason: None,
            };
            if kind == DEFENSE {
                value.range = Some(denominations[&DEFENSE]);
            } else if let Some(record) = selected.get(&kind) {
                let source = &catalog.sources[&record.source];
                value.evidence = Some(Evidence {
                    record: record.id.clone(),
                    source: record.source.clone(),
                    kind: record.kind.clone(),
                    date: source.date.clone(),
                    build: source.build.clone(),
                });
                let reason = if record.status == "unusable" {
                    Some(
                        record
                            .reason
                            .as_deref()
                            .context("missing quarantine reason")?,
                    )
                } else if record.status != "clear" {
                    Some("selected_record_not_clear")
                } else if !record.constraints.is_empty() {
                    Some("additional_constraint_not_supported")
                } else if record.payment_conditions.contains_key(&0) {
                    Some("conditional_primary_payment")
                } else {
                    let primary = &record
                        .payments
                        .as_ref()
                        .context("selected price missing payments")?[0];
                    if primary.len() != 1 {
                        Some("mixed_primary_payment")
                    } else if let Some(denomination) =
                        denominations.get(&i32::from(primary[0].item_id))
                    {
                        value.range = primary[0].quantity.units(*denomination)?;
                        if let Some(quantity) =
                            primary[0].quantity.units(ValueRange { min: 12, max: 12 })?
                        {
                            value.relation = Some(Relation {
                                denomination: i32::from(primary[0].item_id),
                                quantity,
                            });
                        }
                        value.range.is_none().then_some("strict_or_unbounded_price")
                    } else {
                        Some("unsupported_primary_denomination")
                    }
                };
                value.ineligible_reason = reason.map(str::to_owned);
            } else {
                value.ineligible_reason = Some("no_selected_guide_record".into());
            }
            let object = &objects[&item.item_id];
            if value.ineligible_reason.is_none()
                && (object.soulbound || object.unsupported_equipment || object.resurrects)
            {
                value.ineligible_reason = Some("runtime_trade_safety_restriction".into());
            }
            if let Some(range) = value.range {
                ensure!(
                    range.min > 0 && range.min <= range.max && range.max <= 1_000_000_000,
                    "invalid normalized value"
                );
            }
            values.insert(kind, value);
        }
        Ok(Self(values))
    }
    pub fn compatible(
        &self,
        kinds: [i32; 2],
        quantities: [u8; 2],
    ) -> std::result::Result<bool, &str> {
        let ranges = [self.quote_range(kinds[0])?, self.quote_range(kinds[1])?];
        let relations = kinds.map(|k| self.0[&k].relation.as_ref());
        let whole = ValueRange { min: 12, max: 12 };
        // A shared uncertain currency must cancel, not become two independently chosen rates.
        if let Some(a) = relations[0] {
            if a.denomination == kinds[1] {
                return Ok(a.quantity.overlaps(quantities[0], whole, quantities[1]));
            }
            if let Some(b) = relations[1]
                && a.denomination == b.denomination
            {
                return Ok(a
                    .quantity
                    .overlaps(quantities[0], b.quantity, quantities[1]));
            }
        }
        if let Some(b) = relations[1]
            && b.denomination == kinds[0]
        {
            return Ok(whole.overlaps(quantities[0], b.quantity, quantities[1]));
        }
        Ok(ranges[0].overlaps(quantities[0], ranges[1], quantities[1]))
    }
    pub fn kinds(&self) -> impl Iterator<Item = i32> + '_ {
        self.0.keys().copied()
    }
    pub fn get(&self, kind: i32) -> Option<&Valuation> {
        self.0.get(&kind)
    }
    pub fn quote_range(&self, kind: i32) -> std::result::Result<ValueRange, &str> {
        let value = self.get(kind).ok_or("unknown_catalog_item")?;
        if let Some(reason) = &value.ineligible_reason {
            return Err(reason);
        }
        value.range.ok_or("no_selected_price")
    }
}

#[cfg(test)]
mod tests;
