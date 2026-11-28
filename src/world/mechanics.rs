use super::*;
use crate::condition;

impl World {
    pub(super) fn heal_enemy(&mut self, healer: i32, target: i32, amount: i32) -> Result<i32> {
        ensure!(amount > 0, "nonpositive enemy healing amount");
        let kind = self
            .entities
            .get(&target)
            .context("enemy heal target missing")?
            .kind;
        let maximum = self.content.get(kind)?.hp;
        ensure!(maximum > 0, "enemy heal target has no selected HP");
        let e = self.entities.get_mut(&target).unwrap();
        let before = e.hp;
        if e.conditions(self.now) & condition::bit(condition::SICK) == 0 {
            e.hp = (e.hp + amount).min(maximum);
        }
        let gained = e.hp - before;
        if gained > 0 {
            let pos = e.pos;
            let mut out = Writer::new();
            out.i32(target).utf(&format!("+{gained}")).u32(0xff00ff00);
            for p in self
                .players
                .values()
                .filter(|p| p.known.contains(&healer) && p.known.contains(&target))
            {
                send(p, wire::NOTIFICATION, out.0.clone());
                send(p, wire::SHOW_EFFECT, cooperative::effect(1, target, pos));
                send(p, wire::SHOW_EFFECT, cooperative::effect(7, healer, pos));
            }
            tracing::debug!(healer, target, amount = gained, "ordinary enemy healed");
        }
        Ok(gained)
    }

    #[cfg(test)]
    pub(super) fn area_damage(
        &mut self,
        kind: u16,
        position: Pos,
        radius: f32,
        amount: i32,
    ) -> Result<()> {
        let mut out = Writer::new();
        out.f32(position.x)
            .f32(position.y)
            .f32(radius)
            .i16(i16::try_from(amount)?)
            .u8(0)
            .f32(0.)
            .i16(kind as i16);
        let ids: Vec<_> = self.players.keys().copied().collect();
        for id in ids {
            let p = &self.players[&id];
            if !p.character.alive || p.death_pending || p.departing {
                continue;
            }
            if p.pos.distance(position) < radius + 20. {
                send(p, 68, out.0.clone());
            }
            if p.paused || p.pos.distance(position) >= radius {
                continue;
            }
            let flags = p.conditions(self.now);
            let defense = self.defense(id)?;
            let damage = condition::damage(amount, defense, flags, false);
            let p = self.players.get_mut(&id).unwrap();
            let hp_before = p.character.hp;
            p.character.hp = (p.character.hp - damage).max(0);
            p.record_bot_damage(self.now, hp_before);
            if p.character.hp == 0 {
                p.death_pending = true;
                p.killer = self.content.objects[&kind].name.clone();
                p.bot_goal = None;
                p.bot_aim = None;
            }
            p.audit_fact(id, &self.name, self.now, "area_damage", || serde_json::json!({"source_kind":kind,
                "position":[position.x,position.y],"radius":radius,"raw_damage":amount,"damage":damage,
                "conditions":flags,"defense":defense,"hp_before":hp_before,"hp_after":p.character.hp}));
            tracing::debug!(
                player = id,
                kind,
                damage,
                "authoritative encounter area damage"
            );
        }
        Ok(())
    }
}
