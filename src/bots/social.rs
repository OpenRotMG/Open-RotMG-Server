use super::*;
use crate::content::PRIEST;
use crate::world::view::VisibleObject;

pub(super) fn companions<'a>(
    v: &'a PlayerView,
    group: Option<&adventures::Adventure>,
) -> Vec<&'a VisibleObject> {
    v.objects
        .iter()
        .filter(|o| {
            o.hp > 0
                && !o.equipped.is_empty()
                && group.is_some_and(|g| {
                    !g.finished && g.members.iter().any(|m| Some(m.account) == o.account)
                })
        })
        .collect()
}

pub(super) fn healing(v: &PlayerView, content: &Content) -> Result<Option<Action>> {
    if v.character.class != PRIEST
        || v.character.items[1] < 0
        || v.time_ms < v.character.ability_ready_ms
        || v.character.conditions & crate::condition::bit(crate::condition::QUIET) != 0
    {
        return Ok(None);
    }
    let tome = content.get(v.character.items[1] as u16)?;
    if tome.activate != "HealNova" || v.character.mp < tome.mana_cost {
        return Ok(None);
    }
    let own_need = v.character.stats[0] - v.character.hp;
    let needs_healing = own_need >= (tome.heal.min(v.character.stats[0]) as f32 * 0.35) as i32
        || v.objects.iter().any(|o| {
            !o.equipped.is_empty()
                && o.hp > 0
                && o.max_hp - o.hp >= (tome.heal.min(o.max_hp) as f32 * 0.35) as i32
                && v.position.distance(o.position) <= tome.heal_range
        });
    Ok(needs_healing.then_some(Action::Use {
        slot: 1,
        x: v.position.x,
        y: v.position.y,
    }))
}
