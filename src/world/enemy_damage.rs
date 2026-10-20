use super::*;

impl World {
    pub(super) fn damage_enemy(
        &mut self,
        owner: i32,
        target: i32,
        raw: i32,
        piercing: bool,
        effects: &[(u8, u64)],
        bullet: Option<u8>,
    ) -> Result<i32> {
        let Some(e) = self.entities.get(&target) else {
            tracing::debug!(owner, target, "damage target already removed");
            return Ok(0);
        };
        // Selected Boss Totems have no HP; zero HP must not turn an immune hit into a defeat.
        if e.kind == crate::dungeon::jungle::BOSS_TOTEM {
            return Ok(0);
        }
        let flags = e.conditions(self.now);
        let affects = !crate::condition::protected(flags);
        let damage =
            crate::condition::damage(raw, self.content.objects[&e.kind].defense, flags, piercing);
        // Evaluation shells select a fight; only the transformed boss can be defeated for rewards.
        let damage = if e.quest.is_some() && matches!(e.kind, 0x91b | 0x91f | 0x928) {
            damage.min((e.hp - 1).max(0))
        } else {
            damage
        };
        let actual = damage.min(e.hp).max(0);
        if affects {
            for (index, duration) in effects {
                self.apply_condition(target, *index, *duration)?;
            }
        }
        let e = self.entities.get_mut(&target).unwrap();
        if let Some(p) = self.players.get(&owner) {
            *e.damage.entry(p.character.id).or_default() += damage.min(e.hp).max(0);
        }
        e.hp -= damage;
        // A damaged Malphas missile ends through its telegraphed explosion, not ordinary loot/death.
        if e.kind == 0x909 {
            e.hp = e.hp.max(1);
        }
        let lethal = e.hp <= 0;
        let transform =
            lethal && e.quest.is_some() && matches!(e.kind, 0x675 | 0x6c2 | 0x92a | 0x92b);
        let kill = lethal && !transform;

        let quest = e.kind == 0x6b1 || self.content.objects[&e.kind].quest;
        if let Some(p) = self.players.get_mut(&owner) {
            if damage > 0 && bullet.is_some() {
                p.character.hits += 1;
            }
            if kill {
                p.character.kills += 1;
                if quest {
                    p.character.quests += 1;
                }
            }
        }
        let mut o = Writer::new();
        o.i32(target)
            .u8(if affects { effects.len() as u8 } else { 0 });
        if affects {
            for (index, _) in effects {
                o.u8(*index);
            }
        }
        o.i16(damage as i16)
            .u8(u8::from(kill))
            .u8(bullet.unwrap_or(0))
            .i32(if bullet.is_some() { owner } else { 0 });
        for p in self.players.values() {
            if p.known.contains(&target) {
                send(p, 47, o.0.clone());
            }
        }
        if transform {
            self.quest_lethal_transform(target)?;
        } else if kill {
            self.defeat_entity(target)?;
            tracing::info!(entity = target, owner = owner, "enemy defeated");
        }
        Ok(actual)
    }
}
