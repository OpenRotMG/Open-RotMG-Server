use super::*;
#[derive(Clone)]
pub(super) struct DamageOrigin {
    pub context: crate::behavior::TerminalContext,
    pub character: Option<i32>,
}
impl DamageOrigin {
    pub(super) fn nonplayer() -> Self {
        Self {
            context: crate::behavior::TerminalContext::NonPlayer,
            character: None,
        }
    }
}

pub(in crate::world) fn record_character_lifetime_kill(
    character: &mut Character,
    definition: &crate::content::Object,
    final_blow: bool,
) -> Result<()> {
    if definition.enemy {
        let mut lifetime = character.lifetime.clone();
        lifetime.kills = lifetime
            .kills
            .checked_add(1)
            .context("lifetime kill overflow")?;
        if definition.god {
            lifetime.god_kills = lifetime
                .god_kills
                .checked_add(1)
                .context("lifetime god kill overflow")?;
        }
        if definition.cube {
            lifetime.cube_kills = lifetime
                .cube_kills
                .checked_add(1)
                .context("lifetime cube kill overflow")?;
        }
        if definition.oryx && final_blow {
            lifetime.oryx_kills = lifetime
                .oryx_kills
                .checked_add(1)
                .context("lifetime Oryx kill overflow")?;
        }
        character.lifetime = lifetime;
    }
    Ok(())
}
impl World {
    pub(super) fn damage_origin(&self, owner: i32) -> Result<DamageOrigin> {
        let p = self
            .players
            .get(&owner)
            .context("damage event has no authoritative owning-world player")?;
        ensure!(
            owner > 0 && !p.character.name.is_empty(),
            "damage event has invalid player/name attribution"
        );
        Ok(DamageOrigin {
            context: crate::behavior::TerminalContext::PlayerKill {
                world: self.xml_world_id(),
                player: owner,
                name: p.character.name.clone(),
            },
            character: Some(p.character.id),
        })
    }
    pub(super) fn damage_enemy(
        &mut self,
        owner: i32,
        target: i32,
        raw: i32,
        piercing: bool,
        effects: &[(u8, u64)],
        bullet: Option<u8>,
    ) -> Result<i32> {
        let origin = self.damage_origin(owner)?;
        self.damage_enemy_from(&origin, target, raw, piercing, effects, bullet)
    }
    pub(super) fn damage_enemy_nonplayer(
        &mut self,
        target: i32,
        raw: i32,
        piercing: bool,
        effects: &[(u8, u64)],
    ) -> Result<i32> {
        self.damage_enemy_from(
            &DamageOrigin::nonplayer(),
            target,
            raw,
            piercing,
            effects,
            None,
        )
    }
    pub(super) fn damage_enemy_from(
        &mut self,
        origin: &DamageOrigin,
        target: i32,
        raw: i32,
        piercing: bool,
        effects: &[(u8, u64)],
        bullet: Option<u8>,
    ) -> Result<i32> {
        let owner = match &origin.context {
            crate::behavior::TerminalContext::PlayerKill {
                world,
                player,
                name,
            } => {
                ensure!(
                    *world == self.xml_world_id() && *player > 0 && !name.is_empty(),
                    "damage event attribution invalid for owning world"
                );
                *player
            }
            crate::behavior::TerminalContext::NonPlayer => 0,
        };
        if self.xml_terminal_pending(target) {
            if let Err(error) = self.xml_defeated(target, origin.context.clone()) {
                self.xml_bound_terminal_retry(target, &error);
            }
            return Ok(0);
        }
        self.terminal_damage_admission(target, raw, piercing)?;
        let Some(e) = self.entities.get(&target) else {
            return Ok(0);
        };
        if e.hp <= 0 && self.content.objects[&e.kind].hp > 0 {
            return Ok(0);
        }
        let kind = e.kind;
        let position = e.pos;
        let flags = e.conditions(self.now);
        let affects = flags
            & (crate::condition::bit(crate::condition::INVINCIBLE)
                | crate::condition::bit(crate::condition::STASIS))
            == 0;
        let damage =
            crate::condition::damage(raw, self.content.objects[&kind].defense, flags, piercing);
        let actual = damage.min(e.hp).max(0);
        if actual > 0 && actual == e.hp {
            self.reserve_terminal_damage(target, raw, piercing)?;
        }
        if affects {
            for (index, duration) in effects {
                self.apply_condition(target, *index, *duration)?;
            }
        }
        let e = self
            .entities
            .get_mut(&target)
            .context("damage target disappeared")?;
        if let Some(character) = origin.character {
            let prior = e.damage.entry(character).or_default();
            *prior = prior
                .checked_add(actual)
                .context("effective damage ledger overflow")?;
        }
        e.hp = (e.hp - actual).max(0);
        let lethal = e.hp == 0 && self.content.objects[&kind].hp > 0;
        let mut out = Writer::new();
        out.i32(target)
            .u8(if affects { effects.len() as u8 } else { 0 });
        if affects {
            for (index, _) in effects {
                out.u8(*index);
            }
        }
        // The selected client reads this with readUnsignedShort only for its "-N" text (_011/Damage.as,
        // GameObject.damage); HP arrives in stats, so a larger hit applies in full and its text saturates.
        out.i16(actual.clamp(0, i32::from(u16::MAX)) as u16 as i16)
            .u8(u8::from(lethal))
            .u8(bullet.unwrap_or(0))
            .i32(if bullet.is_some() { owner } else { 0 });
        for (id, p) in &self.players {
            if p.known.contains(&target)
                && p.pos.distance(position) < 20.
                && (*id != owner || bullet.is_none() || !self.content.objects[&kind].enemy)
            {
                send(p, 47, out.0.clone());
            }
        }
        if let Some(p) = self.players.get_mut(&owner)
            && actual > 0
            && bullet.is_some()
        {
            p.character.hits += 1;
        }
        if lethal {
            self.xml_declared_admission(kind, "damage terminal lifecycle")?;
            if let Err(error) = self.xml_defeated(target, origin.context.clone()) {
                if !self.xml_terminal_pending(target) {
                    return Err(error);
                }
                self.xml_bound_terminal_retry(target, &error);
            }
            tracing::info!(entity = target, kind, owner, "enemy defeated");
        }
        Ok(actual)
    }
}
