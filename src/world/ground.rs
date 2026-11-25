use super::*;

impl World {
    pub fn ground_damage(&mut self, id: i32, time: i32, pos: Pos) -> Result<()> {
        ensure!(
            time >= 0 && pos.x.is_finite() && pos.y.is_finite(),
            protocol("invalid ground damage report")
        );
        let p = self
            .players
            .get(&id)
            .context("ground damage player missing")?;
        // Old-position reports already drew RNG; onGoto's ACK bounds their discard: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/messaging/impl/GameServerConnection.as#L1333-L1341
        let correcting = p.controller == Controller::Human && p.active() && p.correcting()?;
        ensure!(
            correcting || p.pos.distance(pos) <= 3.,
            protocol("ground damage position is outside the native movement window")
        );
        let c = self
            .map
            .cell(pos.x, pos.y)
            .ok_or_else(|| protocol("ground damage outside map"))?;
        let i = pos.y as usize * self.map.width as usize + pos.x as usize;
        let ground = c.ground;
        let hazard = self
            .content
            .hazards
            .get(&ground)
            .ok_or_else(|| protocol("ground report has no current or prior hazard"))?
            .clone();
        let protected = self
            .cells
            .ids(i)
            .iter()
            .any(|id| self.content.objects[&self.entities[id].kind].protects_ground);
        let p = self.players.get_mut(&id).unwrap();
        // Selected 123.5.1 Player.update requires lastDamage + 500 < currentTime per square.
        let ignored = correcting
            || self.safe
            || protected
            || !p.character.alive
            || p.paused
            || p.departing
            || p.death_pending
            || p.ground_hits
                .get(&i)
                .is_some_and(|previous| time.saturating_sub(*previous) <= 500);
        if ignored && p.controller != Controller::Human {
            return Ok(());
        }
        let damage = p.roll_between(hazard.min, hazard.max);
        if correcting {
            tracing::warn!(
                player = id,
                client_time_ms = time,
                report_x = pos.x,
                report_y = pos.y,
                player_x = p.pos.x,
                player_y = p.pos.y,
                "discarded ground damage awaiting GOTO acknowledgement"
            );
        }
        let raw_damage = damage;
        let flags = p.conditions(self.now);
        let hp_before = p.character.hp;
        let previous = p.ground_hits.get(&i).copied();
        let damage = if ignored {
            0
        } else {
            crate::condition::damage(raw_damage, 0, flags, true)
        };
        if !ignored {
            p.ground_hits
                .retain(|_, previous| time.saturating_sub(*previous) < 1000);
            p.ground_hits.insert(i, time);
            p.character.hp = (p.character.hp - damage).max(0);
            p.record_bot_damage(self.now, hp_before);
            if p.character.hp == 0 {
                p.death_pending = true;
                p.killer = hazard.name;
                p.bot_goal = None;
                p.bot_aim = None;
            }
        }
        p.audit_fact(id, &self.name, self.now, "ground", || serde_json::json!({"client_ms":time,"last_move_ms":p.last_move,
            "position":[pos.x,pos.y],"player_position":[p.pos.x,p.pos.y],"ground":ground,"previous_ms":previous,
            "ignored":ignored,"correcting":correcting,"protected":protected,"safe":self.safe,"paused":p.paused,
            "conditions":flags,"raw_damage":raw_damage,"damage":damage,"hp_before":hp_before,"hp_after":p.character.hp}));
        if ignored {
            return Ok(());
        }
        tracing::debug!(
            player = id,
            ground,
            cell = i,
            client_time_ms = time,
            world_time_ms = self.now,
            report_x = pos.x,
            report_y = pos.y,
            player_x = p.pos.x,
            player_y = p.pos.y,
            damage,
            hp = p.character.hp,
            "ground damage committed"
        );
        Ok(())
    }

    pub(super) fn ground_step(&mut self) {
        let actors: Vec<_> = self
            .players
            .iter()
            .filter(|(_, p)| {
                p.controller != Controller::Human
                    && p.character.alive
                    && !p.paused
                    && !p.departing
                    && !p.death_pending
            })
            .filter(|(_, p)| {
                self.map
                    .cell(p.pos.x, p.pos.y)
                    .is_some_and(|c| self.content.hazards.contains_key(&c.ground))
            })
            .map(|(id, p)| (*id, p.pos))
            .collect();
        for (id, pos) in actors {
            if let Err(error) = self.ground_damage(id, self.now as i32, pos) {
                tracing::error!(player=id,%error,"actor ground damage refused");
            }
        }
    }
    /// Compiled UsesBreath (dungeons.xml:1079) read per second like RestoresBreath's dt*speed (behavior.cc:2665-2683).
    pub(super) fn breath_step(&mut self) {
        let Some(rate) = self.uses_breath else { return };
        let drained = |ms: u64| (rate * ms as f64 / 1000.).floor() as i32;
        let drain = drained(self.now) - drained(self.now.saturating_sub(50));
        // No recovered suffocation value: 10 HP per 150 ms is base ocean.rs:754-790 / realm-src HandleOceanTrenchGround.
        let suffocate = self.now.is_multiple_of(150);
        for (id, p) in self.players.iter_mut().filter(|(_, p)| {
            p.character.alive && !p.paused && !p.departing && !p.death_pending && !p.death_started
        }) {
            if p.oxygen > 0 {
                p.oxygen = (p.oxygen - drain).clamp(0, 100);
            } else if suffocate {
                let hp_before = p.character.hp;
                p.character.hp = (p.character.hp - 10).max(0);
                p.record_bot_damage(self.now, hp_before);
                p.audit_hp(*id, &self.name, self.now, "suffocation", hp_before);
                if p.character.hp == 0 {
                    p.death_pending = true;
                    p.killer = "suffocation".into();
                    p.bot_goal = None;
                    p.bot_aim = None;
                }
                tracing::debug!(
                    player = id,
                    hp = p.character.hp,
                    "suffocation damage committed"
                );
            }
        }
    }
}
