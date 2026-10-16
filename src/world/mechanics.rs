use super::*;
use crate::{condition, encounter};

pub(super) struct Grenade {
    owner: i32,
    kind: u16,
    position: Pos,
    radius: f32,
    damage: i32,
    deadline: u64,
}

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
    pub(super) fn orbit_anchor(
        &self,
        id: i32,
        anchor: encounter::Anchor,
        player: Pos,
    ) -> Result<Pos> {
        let e = self.entities.get(&id).context("orbiting entity missing")?;
        match anchor {
            encounter::Anchor::Player => Ok(player),
            encounter::Anchor::QuestMember(kind) => {
                let tag = e.quest.context("quest anchor lacks encounter ownership")?;
                self.entities
                    .values()
                    .filter(|x| x.quest == Some(tag) && x.kind == kind)
                    .min_by(|a, b| a.pos.distance(e.pos).total_cmp(&b.pos.distance(e.pos)))
                    .map(|x| x.pos)
                    .context("own quest anchor absent")
            }
            encounter::Anchor::Home => Ok(e.home),
            encounter::Anchor::CampLeader => {
                let camp = self
                    .camps
                    .get(e.camp.context("entity has no family camp")?)
                    .context("family camp missing")?;
                let leader = self.population[camp.start]
                    .entity
                    .context("family leader slot is empty")?;
                Ok(self
                    .entities
                    .get(&leader)
                    .context("family leader is no longer alive")?
                    .pos)
            }
            encounter::Anchor::CampMember(kind) => Ok(self.entities[&self
                .camp_member(id, kind)?
                .context("own-camp orbit member absent")?]
                .pos),
            encounter::Anchor::Parent => Ok(self
                .entities
                .get(&e.parent.context("entity has no explicit parent")?)
                .context("entity parent no longer exists")?
                .pos),
            encounter::Anchor::Kind(kind) => self
                .entities
                .values()
                .filter(|candidate| candidate.id != id && candidate.kind == kind)
                .min_by(|a, b| e.pos.distance(a.pos).total_cmp(&e.pos.distance(b.pos)))
                .map(|e| e.pos)
                .context("selected orbit anchor absent"),
        }
    }

    pub(super) fn spawn_children(
        &mut self,
        parent: i32,
        kind: u16,
        maximum: usize,
        radius: f32,
    ) -> Result<()> {
        crate::encounter::validate(&self.content, kind)?;
        crate::loot::validate(&self.content, kind)?;
        let count = self
            .entities
            .values()
            .filter(|e| e.parent == Some(parent) && e.kind == kind)
            .count();
        let missing = maximum.saturating_sub(count);
        if missing == 0 {
            return Ok(());
        }
        ensure!(
            self.entities
                .values()
                .filter(|e| e.parent.is_some() || e.quest.is_some())
                .count()
                + missing
                <= 256,
            "summon budget exhausted"
        );
        let home = self.entities[&parent].pos;
        let positions: Vec<_> = (0..32)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 32.;
                Pos {
                    x: home.x + radius * angle.cos(),
                    y: home.y + radius * angle.sin(),
                }
            })
            .filter(|pos| self.walkable(*pos))
            .collect();
        ensure!(
            positions.len() >= missing,
            "insufficient walkable summon positions around {parent}"
        );
        for i in 0..missing {
            let id = self.spawn_entity(kind, positions[i * positions.len() / missing], vec![])?;
            self.entities.get_mut(&id).unwrap().parent = Some(parent);
        }
        tracing::info!(
            parent,
            kind,
            spawned = missing,
            maximum,
            "bounded encounter children summoned"
        );
        Ok(())
    }

    pub(super) fn encounter_mechanics(&mut self, id: i32, target: Pos, elapsed: u64) -> Result<()> {
        if self.entities[&id].quest.is_some() {
            return self.quest_mechanics(id);
        }
        if crate::endgame::ROSTER.contains(&self.entities[&id].kind) {
            return self.finale_mechanics(id, target, elapsed);
        }
        let kind = self.entities[&id].kind;
        if matches!(kind, 0x62b | 0x900..=0x902) {
            return self.midland_mechanics(id);
        }
        if kind == 0x91a || super::gods::CONSTRUCTS.contains(&kind) {
            return self.god_mechanics(id, elapsed);
        }
        let e = &self.entities[&id];
        let (kind, hp, pos, home) = (e.kind, e.hp, e.pos, e.home);
        let stage = encounter::stage(kind, hp, elapsed);
        let changed = e.stage != Some(stage);
        let shield = match kind {
            0x917 => elapsed % 6000 >= 2000,
            0xd06 => {
                elapsed % 43300 < 2000
                    || (12300..32300).contains(&(elapsed % 43300))
                        && (elapsed % 43300 - 12300) % 5000 >= 1500
            }
            0xd07..=0xd0a => true,
            0xd90 => elapsed % 13000 < 3000,
            _ => false,
        };
        let phase_bits =
            condition::bit(condition::INVULNERABLE) | condition::bit(condition::ARMORED);
        let e = self.entities.get_mut(&id).unwrap();
        if matches!(kind, 0x917 | 0xd06..=0xd0a | 0xd90) {
            e.condition &= !phase_bits;
            if shield {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
            if kind == 0xd06 && stage % 5 == 4 {
                e.condition |= condition::bit(condition::ARMORED);
            }
        }
        if changed {
            e.stage = Some(stage);
            tracing::debug!(entity = id, kind, stage, "encounter phase entered");
            match kind {
                0x614 if stage == 1 => {
                    let mut out = Writer::new();
                    out.u8(15)
                        .i32(id)
                        .f32(0.6)
                        .f32(5.)
                        .f32(0.)
                        .f32(0.)
                        .u32(0xff666666);
                    for p in self
                        .players
                        .values()
                        .filter(|p| p.known.contains(&id) && p.pos.distance(pos) < 20.)
                    {
                        send(p, wire::SHOW_EFFECT, out.0.clone());
                    }
                    tracing::debug!(entity = id, "Goblin horde commanded by wounded mage");
                }
                0x917 if stage == 1 => self.spawn_children(id, 0x919, 8, 3.)?,
                0xd06 if stage % 5 == 3 => {
                    for radius in [9.5, 14.] {
                        for (kind, degrees) in
                            [(0xd07, 315f32), (0xd08, 225.), (0xd09, 135.), (0xd0a, 45.)]
                        {
                            let angle = degrees.to_radians();
                            let corner = Pos {
                                x: home.x + radius * angle.cos(),
                                y: home.y + radius * angle.sin(),
                            };
                            let child = self.spawn_entity(kind, corner, vec![])?;
                            let e = self.entities.get_mut(&child).unwrap();
                            e.parent = Some(id);
                            e.activated = Some(self.now);
                            e.expires = Some(self.now + 20000);
                            e.condition = condition::bit(condition::INVULNERABLE);
                        }
                    }
                }
                0xd90 if stage.is_multiple_of(4) => {
                    for (kind, count) in [
                        (0xdaa, 1),
                        (0xdaf, 2),
                        (0xdac, 1),
                        (0xdad, 2),
                        (0xdae, 1),
                        (0xdab, 2),
                    ] {
                        self.spawn_children(id, kind, count, 2.)?;
                    }
                }
                _ => {}
            }
        }
        if self.now < self.entities[&id].next_aux {
            return Ok(());
        }
        self.entities.get_mut(&id).unwrap().next_aux = self.now
            + match kind {
                0x173 | 0xdae => 5000,
                0x953 => 2500,
                0x654 => 3000,
                0x917 => 500,
                _ => 1000,
            };
        match kind {
            0x953 => self.throw_grenade(id, target, 2., 12)?,
            0x173 => {
                self.spawn_children(id, 0x223, 2, 1.5)?;
                self.spawn_children(id, 0x224, 2, 2.)?;
            }
            0x654 if pos.distance(target) < 8. => self.throw_grenade(id, target, 4., 150)?,
            0x917 if stage == 2 => self.throw_grenade(id, target, 5., 100)?,
            0xd06 if stage % 5 == 4 => {
                self.spawn_children(id, 0x625, 1, 2.)?;
                self.spawn_children(id, 0x626, 2, 3.)?;
            }
            0xd90 if stage % 4 == 3 && elapsed % 13000 >= 3000 => {
                for kind in [0xdb0, 0xdb1, 0xdb2] {
                    self.spawn_children(id, kind, 2, 2.)?;
                }
            }
            0xda2 | 0xda3 if pos.distance(target) < 3. => {
                self.entities.get_mut(&id).unwrap().expires = Some(self.now + 50);
            }
            0xd99 | 0xd9a if pos.distance(target) < 3. => {
                let amount = self.content.objects[&kind].projectiles[&0].min;
                self.area_damage(kind, pos, 3., amount)?;
                let e = self.entities.get_mut(&id).unwrap();
                e.hp = (e.hp + amount).min(self.content.objects[&kind].hp);
            }
            0xdae => {
                let allies: Vec<_> = self
                    .entities
                    .values()
                    .filter(|e| (0xdaa..=0xdaf).contains(&e.kind) && pos.distance(e.pos) < 5.)
                    .map(|e| e.id)
                    .collect();
                for ally in allies {
                    let e = self.entities.get_mut(&ally).unwrap();
                    e.hp = (e.hp + 100).min(self.content.objects[&e.kind].hp);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn throw_grenade(
        &mut self,
        owner: i32,
        position: Pos,
        radius: f32,
        damage: i32,
    ) -> Result<()> {
        ensure!(self.grenades.len() < 128, "grenade budget exhausted");
        if self.entities[&owner].conditions(self.now) & condition::bit(condition::STUNNED) != 0 {
            return Ok(());
        }
        let mut out = Writer::new();
        out.u8(4)
            .i32(owner)
            .f32(position.x)
            .f32(position.y)
            .f32(0.)
            .f32(0.)
            .u32(0xffff0000);
        for p in self.players.values().filter(|p| p.known.contains(&owner)) {
            send(p, wire::SHOW_EFFECT, out.0.clone());
        }
        self.grenades.push(Grenade {
            owner,
            kind: self.entities[&owner].kind,
            position,
            radius,
            damage,
            deadline: self.now + 1500,
        });
        Ok(())
    }

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
            if p.pos.distance(position) >= radius {
                continue;
            }
            let damage =
                condition::damage(amount, self.defense(id)?, p.conditions(self.now), false);
            let p = self.players.get_mut(&id).unwrap();
            p.character.hp = (p.character.hp - damage).max(0);
            if p.character.hp == 0 {
                p.death_pending = true;
                p.killer = self.content.objects[&kind].name.clone();
                p.bot_goal = None;
                p.bot_aim = None;
            }
            tracing::debug!(
                player = id,
                kind,
                damage,
                "authoritative encounter area damage"
            );
        }
        Ok(())
    }

    pub(super) fn grenade_step(&mut self) {
        let grenades = std::mem::take(&mut self.grenades);
        for grenade in grenades {
            if !self.entities.contains_key(&grenade.owner) {
                continue;
            }
            if grenade.deadline > self.now {
                self.grenades.push(grenade);
                continue;
            }
            if let Err(error) = self.area_damage(
                grenade.kind,
                grenade.position,
                grenade.radius,
                grenade.damage,
            ) {
                tracing::error!(%error, "grenade damage rejected");
            }
        }
    }

    pub(super) fn mechanic_expiry(&mut self) {
        let expired: Vec<_> = self
            .entities
            .values()
            .filter(|e| {
                e.expires.is_some_and(|deadline| deadline <= self.now)
                    || e.parent
                        .is_some_and(|parent| !self.entities.contains_key(&parent))
            })
            .map(|e| e.id)
            .collect();
        for id in expired {
            if matches!(self.entities[&id].kind, 0x91a | 0x900)
                && self.entities[&id]
                    .expires
                    .is_some_and(|deadline| deadline <= self.now)
            {
                if let Err(error) = self.finish_timed_defeat(id) {
                    tracing::error!(entity=id,%error,"timed enemy defeat failed");
                }
            } else {
                self.remove_entity(id);
            }
        }
    }

    pub(super) fn encounter_death(&mut self, defeated: &Entity) -> Result<()> {
        if let Some(tag) = defeated.quest {
            if defeated.kind == 0x929 {
                let king = self
                    .entities
                    .values()
                    .find(|e| e.quest == Some(tag) && e.kind == 0x928)
                    .map(|e| e.id);
                if let Some(king) = king {
                    self.quest_transform(king, 0x92d)?;
                }
            } else if defeated.kind == 0x92c
                && !self
                    .entities
                    .values()
                    .any(|e| e.quest == Some(tag) && e.kind == 0x928)
            {
                let count = self.rng.gen_range(2..=3);
                self.quest_spawn_survivors(defeated, 0x669, count)?;
            }
            return Ok(());
        }
        if crate::endgame::ROSTER.contains(&defeated.kind) {
            return self.finale_death(defeated);
        }
        let (kind, count) = match defeated.kind {
            0xd9b | 0xd9c => (0xd9d, 4),
            0xd9e => (0xd9f, 6),
            _ => return Ok(()),
        };
        crate::encounter::validate(&self.content, kind)?;
        crate::loot::validate(&self.content, kind)?;
        ensure!(
            self.entities
                .values()
                .filter(|e| self.content.objects[&e.kind].enemy)
                .count()
                + count
                <= 512,
            "slime split budget exhausted"
        );
        for i in 0..count {
            let angle = i as f32 * std::f32::consts::TAU / count as f32;
            let pos = Pos {
                x: defeated.pos.x + 0.2 * angle.cos(),
                y: defeated.pos.y + 0.2 * angle.sin(),
            };
            self.spawn_entity(kind, pos, vec![])?;
        }
        Ok(())
    }
}
