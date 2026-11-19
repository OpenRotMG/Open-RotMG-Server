use super::*;

pub(super) fn potion_index(content: &Content, kind: i32) -> Result<Option<usize>> {
    let item = content.get(kind as u16)?;
    if !matches!(item.kind, 0xa1f | 0xa20 | 0xa21 | 0xa34 | 0xa35 | 0xa4c) {
        return Ok(None);
    }
    let (stat, _) = item.stat_increase.context("stat potion has no increase")?;
    Ok(Some(
        crate::content::STATS
            .iter()
            .position(|(id, _)| *id == stat)
            .context("stat potion has no supported stat")?,
    ))
}

pub(super) fn spare(content: &Content, kind: i32) -> Result<bool> {
    if potion_index(content, kind)?.is_some() {
        return Ok(true);
    }
    let item = content.get(kind as u16)?;
    Ok(!item.unsupported_equipment
        && item.slot_type != 10
        && item.tier.is_some_and(|tier| tier >= 3))
}

pub(super) fn mana(v: &PlayerView, content: &Content) -> Result<Option<Action>> {
    if v.character.conditions & crate::condition::bit(crate::condition::QUIET) != 0 {
        return Ok(None);
    }
    for (slot, kind) in v
        .character
        .items
        .iter()
        .enumerate()
        .skip(4)
        .filter(|(_, k)| **k >= 0)
    {
        let item = content.get(*kind as u16)?;
        if item.activate == "Magic"
            && v.character.stats[1] - v.character.mp
                >= (item.heal.min(v.character.stats[1]) as f32 * 0.75) as i32
        {
            return Ok(Some(Action::Use {
                slot,
                x: v.position.x,
                y: v.position.y,
            }));
        }
    }
    Ok(None)
}

pub(super) fn consume(
    v: &PlayerView,
    content: &Content,
    market: &pursuits::market::Market,
    future: Option<u16>,
) -> Result<Option<Action>> {
    for (slot, kind) in v
        .character
        .items
        .iter()
        .enumerate()
        .skip(4)
        .filter(|(_, k)| **k >= 0)
    {
        if let Some(index) = potion_index(content, *kind)?
            && v.character.base_stats[index] < content.classes[&v.character.class].caps[index]
            && market.can_use(v, content, future, *kind)?
        {
            return Ok(Some(Action::Use {
                slot,
                x: v.position.x,
                y: v.position.y,
            }));
        }
    }
    Ok(None)
}

pub(super) fn pickup(
    v: &PlayerView,
    content: &Content,
    bag: &crate::world::view::VisibleObject,
    future: Option<u16>,
    market: Option<&pursuits::market::Market>,
) -> Result<Option<Action>> {
    let mut best = None;
    for (slot, kind) in bag.contents.iter().enumerate().filter(|(_, k)| **k >= 0) {
        let value = priority(v, content, *kind, false, future)?.max(
            if market.is_some_and(|m| m.gathers(content, *kind)) {
                80
            } else {
                0
            },
        );
        // Owner, 2012: a poor player takes nearly anything; a rich one only what it needs or the best bags.
        if value == 0
            || value < 60 && market.is_some_and(|m| m.beneath(v, content, *kind))
            || junk(v, content, *kind, future)?
        {
            continue;
        }
        for to in 4..v.character.items.len() {
            if v.character.items[to] >= 0
                && let Some(market) = market
                && !market.can_use(v, content, future, v.character.items[to])?
            {
                continue;
            }
            let held = priority(v, content, v.character.items[to], true, future)?;
            if value > held && best.as_ref().is_none_or(|(gain, _)| value - held > *gain) {
                best = Some((
                    value - held,
                    Action::Swap {
                        from: Slot {
                            object: bag.id,
                            slot,
                            kind: *kind,
                        },
                        to: own(v, to),
                    },
                ));
            }
        }
    }
    Ok(best.map(|(_, action)| action))
}

pub(super) fn ready(v: &PlayerView, content: &Content, portal: u16) -> bool {
    match portal {
        0x717 => v.character.level >= 2,
        0x718 => {
            v.character.level >= 14
                && v.character.stats[0] >= 350
                && v.character.stats[3] >= 4
                && content.objects[&(v.character.items[0] as u16)]
                    .tier
                    .is_some_and(|tier| tier >= 4)
        }
        0x890 | 0xd7b | 0x700 => events::ready(v, content),
        _ => false,
    }
}

pub(super) fn vulnerable(enemy: &crate::world::view::VisibleObject) -> bool {
    enemy.condition_mask.is_none_or(|mask| {
        mask & (crate::condition::bit(crate::condition::INVINCIBLE)
            | crate::condition::bit(crate::condition::INVULNERABLE)
            | crate::condition::bit(crate::condition::STASIS))
            == 0
    })
}

pub(super) fn can_fight(
    v: &PlayerView,
    content: &Content,
    enemy: &crate::world::view::VisibleObject,
) -> bool {
    let definition = &content.objects[&enemy.kind];
    let damage = definition
        .projectiles
        .values()
        .map(|p| p.max)
        .fold(0, i32::max);
    let shared_ready = events::shared(&v.map.name) && events::equipped(v, content);
    damage - v.character.stats[3]
        <= (v.character.stats[0] as f32 * if shared_ready { 0.65 } else { 0.4 }) as i32
        // Realm events are crowd fights nobody solos; refusing them left bots shooting only minions.
        && (enemy.max_hp <= v.character.stats[0] * 50 || shared_ready || definition.encounter)
        && (!events::shared(&v.map.name)
            || !matches!(
                enemy.kind,
                crate::dungeon::lab::HEXXER
                    | crate::dungeon::lab::DEHEXXER
                    | crate::dungeon::lab::BUBBLE
            ))
}

pub(super) fn stash(v: &PlayerView, content: &Content, future: Option<u16>) -> Result<bool> {
    for kind in v.character.items.iter().skip(4).filter(|k| **k >= 0) {
        if spare(content, *kind)? && !junk(v, content, *kind, future)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Gear nobody would pay a whole potion for and neither this nor the next character uses.
/// Live: 59 of 70 banking trips came back full; unsellable gear filled the Vault.
pub(super) fn junk(
    v: &PlayerView,
    content: &Content,
    kind: i32,
    future: Option<u16>,
) -> Result<bool> {
    Ok(potion_index(content, kind)?.is_none()
        && spare(content, kind)?
        && content.item_values.quote_range(kind).is_err()
        && priority(v, content, kind, true, future)? < 60)
}

/// Owner, 2012: unwanted gear was left in Nexus bags for whoever wanted it.
pub(super) fn discard(
    v: &PlayerView,
    content: &Content,
    future: Option<u16>,
) -> Result<Option<Action>> {
    for (slot, kind) in v.character.items.iter().enumerate().skip(4) {
        if *kind >= 0 && junk(v, content, *kind, future)? {
            return Ok(Some(Action::Drop { slot: own(v, slot) }));
        }
    }
    Ok(None)
}

pub(super) fn bank(
    v: &PlayerView,
    content: &Content,
    grid: &Grid,
    future: Option<u16>,
    exclude: Option<i32>,
) -> Result<Option<Decision>> {
    let Some(chest) = v.objects.iter().find(|o| o.kind == 0x504) else {
        return Ok(None);
    };
    let mut transfer = None;
    let mut reason = "private_stock_satisfied_or_unavailable";
    // Withdraw only for a current use; saved future/exchange goods must not cycle through the backpack.
    for (slot, kind) in chest.contents.iter().enumerate().filter(|(_, k)| **k >= 0) {
        let value = priority(v, content, *kind, false, future)?;
        if value < 100 {
            continue;
        }
        for to in 4..v.character.items.len() {
            if value > priority(v, content, v.character.items[to], true, future)? {
                transfer = Some(Action::Swap {
                    from: Slot {
                        object: chest.id,
                        slot,
                        kind: *kind,
                    },
                    to: own(v, to),
                });
                reason = "withdraw_for_current_use";
                break;
            }
        }
        if transfer.is_some() {
            break;
        }
    }
    let empty = (4..v.character.items.len()).find(|i| v.character.items[*i] < 0);
    for (slot, kind) in chest.contents.iter().enumerate() {
        if transfer.is_none()
            && let Some(to) = empty
            && *kind >= 0
            && junk(v, content, *kind, future)?
        {
            transfer = Some(Action::Swap {
                from: Slot {
                    object: chest.id,
                    slot,
                    kind: *kind,
                },
                to: own(v, to),
            });
            reason = "withdraw_junk_for_nexus";
        }
    }
    if transfer.is_none() {
        for (from, kind) in v
            .character
            .items
            .iter()
            .enumerate()
            .skip(4)
            .filter(|(_, k)| **k >= 0)
        {
            let value = priority(v, content, *kind, true, future)?;
            if value >= 100
                || exclude == Some(*kind)
                || !spare(content, *kind)?
                || junk(v, content, *kind, future)?
            {
                continue;
            }
            let to = chest.contents.iter().position(|k| *k == -1);
            if let Some(to) = to {
                transfer = Some(Action::Swap {
                    from: own(v, from),
                    to: Slot {
                        object: chest.id,
                        slot: to,
                        kind: -1,
                    },
                });
                reason = "store_future_or_exchange_spare";
                break;
            }
        }
    }
    if let Some(action) = transfer {
        let near = v.position.distance(chest.position) < 1.4;
        tracing::info!(
            account = v.session.account,
            character = v.session.character,
            revision = v.character.inventory_revision,
            ?action,
            reason,
            "autonomous private stock choice"
        );
        return Ok(Some(Decision {
            goal: Goal::Bank(chest.id),
            movement: if near {
                None
            } else {
                Some(
                    grid.towards(v.position, chest.position)
                        .context("observed Vault chest has no known route")?,
                )
            },
            item: near.then_some(action),
            aim: None,
            journey: None,
            chat: None,
        }));
    }
    tracing::info!(
        account = v.session.account,
        revision = v.character.inventory_revision,
        reason,
        "autonomous private organization finished; continuing outing with retained holdings"
    );
    Ok(Some(Decision {
        goal: Goal::Wait,
        movement: None,
        aim: None,
        item: None,
        journey: Some(Journey::Nexus),
        chat: None,
    }))
}

pub(super) fn priority(
    v: &PlayerView,
    content: &Content,
    kind: i32,
    held: bool,
    future: Option<u16>,
) -> Result<i32> {
    if kind < 0 {
        return Ok(-1);
    }
    let item = content.get(kind as u16)?;
    if matches!(item.activate.as_str(), "Heal" | "Magic") {
        let count = v
            .character
            .items
            .iter()
            .skip(4)
            .filter(|k| **k == kind)
            .count();
        let needed = item.activate == "Heal" || v.character.items[1] >= 0;
        return Ok(
            if needed && ((held && item.activate == "Heal") || count < 2 + usize::from(held)) {
                120
            } else {
                0
            },
        );
    }
    if let Some(index) = potion_index(content, kind)? {
        return Ok(
            if v.character.base_stats[index] < content.classes[&v.character.class].caps[index] {
                110
            } else {
                30
            },
        );
    }
    if item.unsupported_equipment || (!held && v.character.items.contains(&kind)) {
        return Ok(0);
    }
    if let Some(slot) = content
        .get(v.character.class)?
        .slots
        .iter()
        .take(4)
        .position(|s| *s == item.slot_type)
        && quality(v, content, kind)? > quality(v, content, v.character.items[slot])?
    {
        return Ok(140);
    }
    if let Some(class) = future
        && item.tier.is_some_and(|t| t >= 3)
        && content
            .get(class)?
            .slots
            .iter()
            .take(4)
            .any(|s| *s == item.slot_type)
    {
        return Ok(60 + i32::from(item.tier.unwrap()));
    }
    Ok(match item.tier {
        Some(tier) if spare(content, kind)? && (held || tier >= 6) => {
            i32::from(tier) + if tier >= 6 { 30 } else { 0 }
        }
        _ => 0,
    })
}

pub(super) fn quality(v: &PlayerView, content: &Content, kind: i32) -> Result<f32> {
    if kind < 0 {
        return Ok(-1.);
    }
    let item = content.get(kind as u16)?;
    let damage = item
        .projectiles
        .get(&0)
        .map_or(0., |p| (p.min + p.max) as f32 * 0.5 * item.rate);
    let bonuses: f32 = item
        .bonuses
        .iter()
        .map(|(stat, n)| {
            *n as f32
                * match stat {
                    0 => {
                        if v.character.hp * 2 < v.character.stats[0] {
                            3.
                        } else {
                            1.
                        }
                    }
                    3 => {
                        if v.character.mp * 2 < v.character.stats[1] {
                            3.
                        } else {
                            0.5
                        }
                    }
                    21 => 5.,
                    _ => 1.,
                }
        })
        .sum();
    Ok(damage + bonuses + item.heal as f32 * 0.1)
}

pub(super) fn clear_cover(
    v: &PlayerView,
    content: &Content,
    grid: &Grid,
    destination: Pos,
    range: f32,
    passes_cover: bool,
    previous: &Goal,
) -> Option<Decision> {
    if v.character.conditions & crate::content::STUNNED != 0 {
        return None;
    }
    let (wall, movement, _) = v
        .objects
        .iter()
        .filter(|o| o.blocks && o.hp > 0 && content.objects[&o.kind].destructible && vulnerable(o))
        .filter_map(|wall| {
            let kept = *previous == Goal::Break(wall.id);
            let clear = sight(v, grid, wall.position, passes_cover, Some(wall.id));
            let movement = if clear && v.position.distance(wall.position) < range {
                None
            } else if clear || kept {
                let (x, y) = tile(wall.position);
                let mut spots = [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)].map(center);
                // Rank firing positions, not first steps that can lead around to the far side of the wall.
                spots.sort_by(|a, b| a.distance(v.position).total_cmp(&b.distance(v.position)));
                Some(spots.into_iter().find_map(|p| grid.next(v.position, p))?)
            } else {
                return None;
            };
            Some((wall, movement, kept))
        })
        .min_by(|(a, a_step, a_kept), (b, b_step, b_kept)| {
            // Switching to every wall that comes into view can prevent reaching any firing position.
            (!a_kept, a_step.is_some())
                .cmp(&(!b_kept, b_step.is_some()))
                .then_with(|| {
                    a.position
                        .distance(destination)
                        .total_cmp(&b.position.distance(destination))
                })
        })?;
    Some(Decision {
        goal: Goal::Break(wall.id),
        movement,
        aim: movement.is_none().then_some(wall.position),
        item: None,
        journey: None,
        chat: None,
    })
}
