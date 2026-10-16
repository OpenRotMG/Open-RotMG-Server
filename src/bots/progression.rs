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

pub(super) fn consume(v: &PlayerView, content: &Content) -> Result<Option<Action>> {
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
) -> Result<Option<Action>> {
    let empty = v
        .character
        .items
        .iter()
        .enumerate()
        .skip(4)
        .find(|(_, kind)| **kind == -1)
        .map(|(slot, _)| slot);
    let spare = v
        .character
        .items
        .iter()
        .enumerate()
        .skip(4)
        .filter(|(_, kind)| **kind >= 0)
        .filter_map(|(slot, kind)| {
            let item = &content.objects[&(*kind as u16)];
            (!item.unsupported_equipment && item.slot_type != 10)
                .then_some(item.tier)
                .flatten()
                .map(|tier| (tier, slot))
        })
        .min()
        .map(|(_, slot)| slot);
    let mut selected = None;
    for (slot, kind) in bag
        .contents
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind >= 0)
    {
        if !useful(content, v, *kind)? {
            continue;
        }
        let needed = potion_index(content, *kind)?.is_some_and(|index| {
            v.character.base_stats[index] < content.classes[&v.character.class].caps[index]
        }) || (content.objects[&(*kind as u16)].activate == "Magic"
            && v.character.items[1] >= 0
            && v.character.mp / 2 < content.get(v.character.items[1] as u16)?.mana_cost);
        let destination = match empty {
            Some(slot) => Some(slot),
            None if needed => spare,
            None => None,
        };
        if let Some(to) = destination {
            if selected.is_none() || needed {
                selected = Some(Action::Swap {
                    from: Slot {
                        object: bag.id,
                        slot,
                        kind: *kind,
                    },
                    to: own(v, to),
                });
            }
            if needed {
                break;
            }
        }
    }
    Ok(selected)
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
        _ => false,
    }
}

pub(super) fn vulnerable(enemy: &crate::world::view::VisibleObject) -> bool {
    enemy.condition_mask.is_none_or(|mask| {
        mask & (crate::condition::bit(crate::condition::INVULNERABLE)
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
    damage - v.character.stats[3] <= (v.character.stats[0] as f32 * 0.4) as i32
        && enemy.max_hp <= v.character.stats[0] * 50
}

pub(super) fn stash(v: &PlayerView, content: &Content) -> Result<bool> {
    for kind in v.character.items.iter().skip(4).filter(|k| **k >= 0) {
        if spare(content, *kind)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn bank(v: &PlayerView, content: &Content, grid: &Grid) -> Result<Option<Decision>> {
    let Some(chest) = v.objects.iter().find(|o| o.kind == 0x504) else {
        return Ok(None);
    };
    for (from, kind) in v
        .character
        .items
        .iter()
        .enumerate()
        .skip(4)
        .filter(|(_, k)| **k >= 0)
    {
        if spare(content, *kind)?
            && let Some(to) = chest.contents.iter().position(|k| *k == -1)
        {
            return Ok(Some(Decision {
                goal: Goal::Bank(chest.id),
                movement: if v.position.distance(chest.position) < 1.4 {
                    None
                } else {
                    Some(
                        grid.towards(v.position, chest.position)
                            .context("observed Vault chest has no known route")?,
                    )
                },
                item: (v.position.distance(chest.position) < 1.4).then(|| Action::Swap {
                    from: own(v, from),
                    to: Slot {
                        object: chest.id,
                        slot: to,
                        kind: -1,
                    },
                }),
                aim: None,
                journey: None,
                chat: None,
            }));
        }
    }
    if stash(v, content)? {
        tracing::info!(
            player = v.session.player,
            "autonomous Vault full; retaining carried items and deferring banking"
        );
    }
    Ok(Some(Decision {
        goal: Goal::Wait,
        movement: None,
        aim: None,
        item: None,
        journey: Some(Journey::Nexus),
        chat: None,
    }))
}

pub(super) fn clear_cover(
    v: &PlayerView,
    content: &Content,
    grid: &Grid,
    destination: Pos,
    range: f32,
    passes_cover: bool,
) -> Option<Decision> {
    if v.character.conditions & crate::content::STUNNED != 0 {
        return None;
    }
    let wall = v
        .objects
        .iter()
        .filter(|o| {
            o.blocks
                && o.hp > 0
                && content.objects[&o.kind].destructible
                && v.position.distance(o.position) < range
                && sight(v, grid, o.position, passes_cover, Some(o.id))
        })
        .min_by(|a, b| {
            a.position
                .distance(destination)
                .total_cmp(&b.position.distance(destination))
        })?;
    Some(Decision {
        goal: Goal::Break(wall.id),
        movement: None,
        aim: Some(angle(v.position, wall.position)),
        item: None,
        journey: None,
        chat: None,
    })
}
