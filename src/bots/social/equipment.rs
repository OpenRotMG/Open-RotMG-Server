use crate::bots::{crowds, pursuits::market::Market};
use crate::content::{Ability, Content, Object, STATS};
use crate::world::view::{OutputState, PlayerView};
use anyhow::{Result, ensure};

#[derive(Clone)]
pub(in crate::bots) struct Policy {
    pub future_class: Option<u16>,
    pub current_goal: Option<i32>,
    pub willingness: f32,
    // The consumer retains its actual hazard, commitment and detour checks.
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::bots) enum Reason {
    MissingEquipment { equipped_slot: usize },
    PublicUpgrade { equipped_slot: usize, previous: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::bots) struct Candidate {
    pub world: i32,
    pub own_account: i32,
    pub own_character: i32,
    pub own_player: i32,
    pub slot: usize,
    pub kind: i32,
    pub inventory_revision: i32,
    pub recipient_player: i32,
    pub recipient_account: i32,
    pub recipient_name: String,
    pub reason: Reason,
}

// Selection is not consent or settlement; all bindings and spendability must be rechecked before action.
pub(in crate::bots) fn select(
    v: &PlayerView,
    c: &Content,
    market: &Market,
    policy: &Policy,
) -> Result<Option<Candidate>> {
    ensure!(
        v.session.account > 0
            && v.session.character > 0
            && v.session.player > 0
            && v.character.inventory_revision >= 0
            && v.character.items.len() == 12
            && v.character.stats[0] > 0,
        "invalid equipment gift own inventory/session binding"
    );
    for class in [Some(v.character.class), policy.future_class]
        .into_iter()
        .flatten()
    {
        ensure!(
            c.classes.contains_key(&class) && c.get(class)?.slots.len() >= 4,
            "invalid equipment gift own class {class}"
        );
    }
    ensure!(
        policy.willingness.is_finite(),
        "invalid equipment gift willingness"
    );
    for (slot, kind) in v.character.items.iter().copied().enumerate() {
        ensure!(kind >= -1, "invalid equipment gift own kind {kind}");
        if kind == -1 {
            continue;
        }
        let item = c.get(u16::try_from(kind)?)?;
        ensure!(
            item.class == "Equipment"
                && (slot >= 4 || item.slot_type == c.get(v.character.class)?.slots[slot]),
            "invalid equipment gift own slot {slot} kind {kind}"
        );
    }
    if let Some(kind) = policy.current_goal {
        c.get(u16::try_from(kind)?)?;
    }
    if !policy.available
        || policy.willingness < 6.
        || v.map.name != "Realm"
        || !v.lifecycle.alive
        || v.lifecycle.busy
        || v.lifecycle.death_pending
        || v.lifecycle.departing
        || v.lifecycle.quarantined
        || !matches!(v.lifecycle.output, OutputState::Live)
        || v.character.hp <= 0
        || v.character.hp * 4 < v.character.stats[0] * 3
        || v.character.conditions != 0
    {
        return Ok(None);
    }
    for (slot, kind) in v.character.items.iter().copied().enumerate().skip(4) {
        if kind < 0 || Some(kind) == policy.current_goal {
            continue;
        }
        let item = c.get(u16::try_from(kind)?)?;
        if !eligible(item)
            || effects(item).is_none()
            || market.spendable_surplus(v, c, policy.future_class, kind)? == 0
        {
            continue;
        }
        for peer in &v.objects {
            let Some(account) = peer.account.filter(|a| *a > 0 && *a != v.session.account) else {
                continue;
            };
            let distance = v.position.distance(peer.position);
            if peer.id <= 0
                || peer.id == v.session.player
                || peer.hp <= 0
                || peer.max_hp <= 0
                || peer.name.is_empty()
                || peer.name.len() > 16
                || peer.equipped.len() != 4
                || !distance.is_finite()
                || distance >= 2.
                || !crowds::survivable(v, peer.position)
                || v.objects
                    .iter()
                    .filter(|o| {
                        o.id == peer.id || o.account == Some(account) || o.name == peer.name
                    })
                    .count()
                    != 1
                || !c.classes.contains_key(&peer.kind)
            {
                continue;
            }
            let class = c.get(peer.kind)?;
            let Some(equipped_slot) = class
                .slots
                .iter()
                .take(4)
                .position(|s| *s == item.slot_type)
            else {
                continue;
            };
            let previous = peer.equipped[equipped_slot];
            let old = if previous == -1 {
                None
            } else {
                let Some(old) = u16::try_from(previous).ok().and_then(|k| c.objects.get(&k)) else {
                    continue;
                };
                if !eligible(old) || old.slot_type != item.slot_type {
                    continue;
                }
                Some(old)
            };
            if !improves(item, old) {
                continue;
            }
            return Ok(Some(Candidate {
                world: v.session.world,
                own_account: v.session.account,
                own_character: v.session.character,
                own_player: v.session.player,
                slot,
                kind,
                inventory_revision: v.character.inventory_revision,
                recipient_player: peer.id,
                recipient_account: account,
                recipient_name: peer.name.clone(),
                reason: if old.is_none() {
                    Reason::MissingEquipment { equipped_slot }
                } else {
                    Reason::PublicUpgrade {
                        equipped_slot,
                        previous,
                    }
                },
            }));
        }
    }
    Ok(None)
}

fn eligible(item: &Object) -> bool {
    item.class == "Equipment"
        && !item.consumable
        && !item.soulbound
        && !item.unsupported_equipment
        && !item.resurrects
}

// Only known effects participate; XML absence is zero, not an invented tier or price.
fn effects(item: &Object) -> Option<[f32; 12]> {
    let mut values = [0.; 12];
    for (stat, amount) in &item.bonuses {
        let index = STATS.iter().position(|(id, _)| id == stat)?;
        values[index] += *amount as f32;
    }
    match item.abilities.as_slice() {
        [] if item.activate.is_empty() => {}
        [Ability::Heal { amount, .. }] if item.activate == "HealNova" => {
            values[11] = *amount as f32
        }
        _ => return None,
    }
    if !item.projectiles.is_empty() {
        if item.projectiles.len() != 1 {
            return None;
        }
        let p = item.projectiles.get(&0)?;
        if p.min < 0 || p.max <= 0 || p.max < p.min || !p.effects.is_empty() {
            return None;
        }
        values[8] = p.min as f32;
        values[9] = p.max as f32;
        values[10] = item.rate;
    }
    values
        .iter()
        .all(|n| n.is_finite() && *n >= 0.)
        .then_some(values)
}

fn improves(item: &Object, old: Option<&Object>) -> bool {
    let Some(values) = effects(item) else {
        return false;
    };
    let before = if let Some(old) = old {
        // Different shot patterns or ability costs are not a public promise of better DPS/utility.
        if item.shots != old.shots
            || item.arc != old.arc
            || item.activate != old.activate
            || item.mana_cost != old.mana_cost
            || item.cooldown_ms != old.cooldown_ms
            || item.heal_range != old.heal_range
        {
            return false;
        }
        match (item.projectiles.get(&0), old.projectiles.get(&0)) {
            (Some(a), Some(b))
                if a.speed == b.speed
                    && a.lifetime == b.lifetime
                    && a.amplitude == b.amplitude
                    && a.frequency == b.frequency
                    && a.passes_cover == b.passes_cover
                    && a.multi_hit == b.multi_hit
                    && a.armor_piercing == b.armor_piercing
                    && a.wavy == b.wavy
                    && a.parametric == b.parametric
                    && a.boomerang == b.boomerang
                    && a.magnitude == b.magnitude => {}
            (None, None) => {}
            _ => return false,
        }
        let Some(before) = effects(old) else {
            return false;
        };
        before
    } else {
        [0.; 12]
    };
    values.iter().zip(before).all(|(a, b)| *a >= b)
        && values.iter().zip(before).any(|(a, b)| *a > b)
}

#[cfg(test)]
mod tests;
