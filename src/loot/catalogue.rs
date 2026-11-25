use super::evaluator::{ALLOCATION_VERSION, definition_facts, landmark_chest_facts};
use crate::content::game::{BehaviorKind, Field, GameData, Value as GameValue};
use anyhow::{Context, Result};
use serde_json::{Value, json};

/// Every item a compiled defeat or landmark chest row can allocate.
pub(crate) fn collectibles(game: &GameData) -> std::collections::BTreeSet<u16> {
    game.definitions
        .values()
        .flat_map(|definition| &definition.loot)
        .chain(game.landmark_chests.values().flat_map(|chest| &chest.rows))
        .flat_map(|row| row.pool.iter().copied())
        .collect()
}

pub(crate) fn snapshot(game: &GameData, revision: Option<&str>) -> Result<Value> {
    let mut objects = Vec::new();
    for definition in game.definitions.values() {
        let death_births = definition.states.iter().flat_map(|state| {
            state.behaviors.iter().filter(|behavior| behavior.kind == BehaviorKind::MetamorphoseAtDeath)
                .map(move |behavior| (state, behavior))
        }).map(|(state, behavior)| -> Result<Value> {
            let Some(GameValue::Objects(targets)) = behavior.fields.get(&Field::ChildId) else {
                anyhow::bail!("{}:{} death birth lacks linked ChildId", behavior.source.file, behavior.source.line);
            };
            let [id] = targets.as_slice() else {
                anyhow::bail!("{}:{} death birth requires one linked ChildId", behavior.source.file, behavior.source.line);
            };
            let target = game.properties.get(id).context("death birth lacks selected target properties")?;
            Ok(json!({"state_id":state.id,"state_source":state.source,"compiled_behavior":behavior,
                "target":{"id":id,"name":target.name,"class":target.class,"dungeon_name":target.dungeon_name},
                "condition":"Only if this source state/hook is active at terminal claim; probability is a source attempt, not unconditional portal delivery. Route, lifetime and atomic publication require the world coordinator."}))
        }).collect::<Result<Vec<_>>>()?;
        if definition.loot.is_empty() && death_births.is_empty() {
            continue;
        }
        let mut facts = definition_facts(game, definition.kind)?;
        facts["death_births"] = json!(death_births);
        objects.push(facts);
    }
    let landmark_chests = game
        .landmark_chests
        .keys()
        .map(|kind| landmark_chest_facts(game, *kind))
        .collect::<Result<Vec<_>>>()?;
    let items: Vec<_> = collectibles(game).into_iter().map(|id| {
            let item = &game.properties[&id];
            json!({"id":id,"name":item.name,"slot":item.slot_type,"tier":item.tier,"bag_type":item.bag_type,"soulbound":item.soulbound})
        }).collect();
    Ok(
        json!({"schema_version":2,"revision":revision,"content_fingerprint":game.fingerprint,
        "allocation_version":ALLOCATION_VERSION,"items":items,"objects":objects,"landmark_chests":landmark_chests,
        "policy":{
            "qualification":"Recovered Release 7.0 literal XML rows, strict selected 123.5.1 properties; allocation/public-gate/RNG law is authored, not authenticated historical engine behavior.",
            "independence":"Each source row, including identical repeats, is separate; never merge eligibility, ownership, source or min/max.",
            "recipient_order":"Sort eligible durable character IDs, then reverse Fisher-Yates with unbiased bounded indices, independently per row. Public uses one virtual recipient.",
            "allocation":"Bernoullis in fair order stop at max successes. Top up to min in that order, first recipients without success, cycle only when min exceeds recipient count. Pool choices occur after row recipient allocation, uniformly over sorted linked IDs. Duplicates are retained.",
            "rng":"p=0/p=1 need no unit draw; max=0, p=0 with min=0, no eligible private character and failed public gate need no row draws. Singleton permutation/pool choices need no index draw. u < p succeeds; equality fails.",
            "probability_labels":"Attempt-item probabilities are before min/max and are not final per-character probabilities. Final chance is deliberately not projected by this context-free endpoint.",
            "delivery":"Allocation plans are pending entitlements, not committed drops. Native packing, transient capacity, durable ownership/pickup and once-only commit are coordinator obligations; insufficient capacity must not reroll."
        }}),
    )
}
