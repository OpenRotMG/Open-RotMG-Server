use super::*;
use std::time::Duration;

const ACK_LIMIT: usize = 64;
const ACK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
pub enum NativeInput {
    Applied,
    DiscardedUntilGotoAck,
}

#[derive(Debug)]
pub struct RejectedNativeShot(pub String);
impl std::fmt::Display for RejectedNativeShot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

pub(super) fn attack_cooldown(
    p: &Player,
    content: &Content,
    now: u64,
    item_rate: f32,
) -> Result<f32> {
    let rate = if p.conditions(now) & crate::condition::bit(crate::condition::DAZED) != 0 {
        0.0015
    } else {
        let base = 0.0015 + (0.008 - 0.0015) * p.effective(content, now)?[5] as f64 / 75.;
        base * if p.conditions(now) & crate::condition::bit(crate::condition::BERSERK) != 0 {
            1.5
        } else {
            1.
        }
    };
    // Ruffle's integer attack period must not accumulate fractional server charges.
    Ok((1. / (rate * f64::from(item_rate))).floor() as f32)
}

pub(super) fn goto(p: &mut Player, own: bool, body: Vec<u8>) {
    let Some(output) = &p.tx else { return };
    if p.goto_acks.len() == ACK_LIMIT {
        tracing::error!(
            character = p.character.id,
            "GOTO acknowledgement backlog exceeded"
        );
        lose(&output.lost, "goto_ack_backlog");
        return;
    }
    // ACK has no object ID, so other players' GOTOs must share the same FIFO.
    p.goto_acks.push_back((own, Instant::now()));
    send(p, wire::GOTO, body);
}

impl Player {
    // Discarded native shots need the same Berserk cadence as admitted shots: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as#L664-L677
    pub(super) fn normal_attack_period(
        &self,
        dex: i32,
        now: u64,
        item_rate: f32,
        native: bool,
    ) -> f32 {
        let berserk = self.conditions(now) & crate::condition::bit(crate::condition::BERSERK) != 0
            || native
                && self
                    .statuses
                    .get(&crate::condition::BERSERK)
                    .is_some_and(|(_, until)| now >= *until && now - until < 1000);
        let frequency = 0.0015 + (0.008 - 0.0015) * f64::from(dex) / 75.;
        (1. / frequency / f64::from(item_rate) / if berserk { 1.5 } else { 1. }).floor() as f32
    }
    pub(super) fn advertised_weapon(&self, kind: u16) -> bool {
        self.weapon_ticks
            .iter()
            .any(|(_, k, _)| *k == i32::from(kind))
    }
    // NEW_TICK ACK precedes applying DEX, so in-flight cadence needs ACK-bounded views: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/messaging/impl/GameServerConnection.as#L1182-L1194
    pub(super) fn advertised_dex(&self, current: i32) -> i32 {
        self.weapon_ticks
            .iter()
            .fold(current, |dex, (_, _, past)| dex.max(*past))
    }
    pub(super) fn remember_weapon_tick(&mut self, dex: i32) {
        if self.controller != Controller::Human {
            return;
        }
        if self.weapon_ticks.len() == ACK_LIMIT {
            tracing::error!(
                character = self.character.id,
                "tick acknowledgement backlog exceeded"
            );
            if let Some(output) = &self.tx {
                lose(&output.lost, "tick_ack_backlog");
            }
            return;
        }
        self.weapon_ticks
            .push_back((self.tick, self.character.items[0], dex));
    }
    pub(super) fn correcting(&self) -> Result<bool> {
        ensure!(self.active(), "inactive player cannot reconcile position");
        if let Some((_, sent)) = self.goto_acks.iter().find(|(own, _)| *own) {
            ensure!(
                sent.elapsed() <= ACK_TIMEOUT,
                crate::server::lifecycle::protocol("GOTO acknowledgement timed out")
            );
            return Ok(true);
        }
        Ok(false)
    }
    fn consume_discarded_shot(&mut self, id: i32, time: i32, bullet: u8, count: i32) {
        self.discarded_attack = Some((time, count + 1));
        // Ruffle allocated these bullet IDs before receiving the correction; authority stays unchanged.
        self.next_bullet = (bullet + 1) % 128;
        tracing::warn!(
            player = id,
            client_time = time,
            bullet,
            "discarded SHOOT across position correction"
        );
    }
}

impl World {
    fn predicted_death_target(&self, id: i32, target: i32) -> Option<&Entity> {
        let p = self.players.get(&id)?;
        if p.controller != Controller::Human || !p.active() || !p.known.contains(&target) {
            return None;
        }
        self.entities
            .get(&target)
            .filter(|e| e.hp > 0 && self.content.objects[&e.kind].enemy)
    }

    pub fn native_enemy_hit(
        &mut self,
        id: i32,
        bullet: u8,
        target: i32,
        killed: bool,
    ) -> Result<()> {
        if killed {
            self.native_predicted_enemy_death(id, bullet, target)?;
        }
        // Client-seen hits land with the server's shared damage roll, as Kithio's ForceHit does; lag must not turn visible hits into misses: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/networking/handlers/EnemyHitPacketHandler.cs
        let Some(e) = self.predicted_death_target(id, target) else {
            return Ok(());
        };
        let Some(index) = self.bullets.iter().position(|b| {
            b.friendly
                && b.owner == id
                && b.id == bullet
                && !b.hit.contains(&-1)
                && (b.spec.multi_hit || b.hit.is_empty())
                && self.hittable(b, e)
        }) else {
            return Ok(());
        };
        let b = &self.bullets[index];
        let reach = b.spec.speed * b.spec.lifetime / 10000.
            + b.spec.amplitude.abs()
            + b.spec.magnitude
            + 2.;
        if b.start.distance(e.pos) > reach {
            tracing::warn!(
                player = id,
                target,
                bullet,
                distance = b.start.distance(e.pos),
                reach,
                "native enemy hit beyond projectile reach ignored"
            );
            return Ok(());
        }
        let mut bullets = std::mem::take(&mut self.bullets);
        self.detached_bullets = bullets.len();
        self.bullet_hit(&mut bullets[index], target);
        bullets.append(&mut self.bullets);
        self.bullets = bullets;
        self.detached_bullets = 0;
        Ok(())
    }

    fn native_predicted_enemy_death(&mut self, id: i32, bullet: u8, target: i32) -> Result<()> {
        self.players
            .get(&id)
            .context("enemy death report player missing")?;
        if self.predicted_death_target(id, target).is_some() {
            let bullets = &self.bullets;
            self.players
                .get_mut(&id)
                .unwrap()
                .predicted_deaths
                .entry(target)
                .or_insert_with(|| {
                    let born = bullets
                        .iter()
                        .find(|b| b.friendly && b.owner == id && b.id == bullet)
                        .map(|b| b.born);
                    (bullet, born)
                });
        }
        Ok(())
    }

    pub(super) fn reconcile_enemy_deaths(&mut self) {
        let pending: Vec<_> = self
            .players
            .iter()
            .flat_map(|(&id, p)| {
                p.predicted_deaths
                    .iter()
                    .map(move |(&target, &(bullet, born))| (id, target, bullet, born))
            })
            .collect();
        for (id, target, bullet, born) in pending {
            let alive = self.predicted_death_target(id, target).is_some();
            if alive
                && self.bullets.iter().any(|b| {
                    b.friendly
                        && b.owner == id
                        && b.id == bullet
                        && Some(b.born) == born
                        && !b.hit.contains(&target)
                })
            {
                continue;
            }
            self.players
                .get_mut(&id)
                .unwrap()
                .predicted_deaths
                .remove(&target);
            if !alive {
                continue;
            }
            let e = &self.entities[&target];
            let mut make = Writer::new();
            make.i16(0).i16(1).i16(e.kind as i16);
            make.0
                .extend(self.stats(target, id).expect("known living enemy stats"));
            make.i16(0);
            // Sticky native deaths need DROP then MAKE after projectile resolution: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/objects/GameObject.as#L684-L693
            let mut drop = Writer::new();
            drop.i16(0).i16(0).i16(1).i32(target);
            let p = &self.players[&id];
            send(p, wire::UPDATE, drop.0);
            send(p, wire::UPDATE, make.0);
            tracing::info!(
                player = id,
                target,
                enemy_kind = e.kind,
                bullet,
                projectile_born_ms = ?born,
                game_ms = self.now,
                hp = e.hp,
                conditions = e.conditions(self.now),
                "native enemy death prediction reconciled"
            );
        }
    }

    pub fn acknowledge_tick(&mut self, id: i32, tick: i32, received: Instant) -> Result<()> {
        let p = self
            .players
            .get_mut(&id)
            .context("tick ACK player missing")?;
        ensure!(
            tick >= 0 && tick < p.tick,
            crate::server::lifecycle::protocol("unsolicited tick acknowledgement")
        );
        p.native_lag.acknowledge(tick, received);
        // NEW_TICK queues MOVE before applying inventory; earlier SHOOTs still use the old weapon.
        p.weapon_ticks.retain(|(sent, _, _)| *sent >= tick);
        Ok(())
    }
    pub(super) fn validate_native_coordinates(&self, id: i32, pos: Pos, time: i32) -> Result<()> {
        let p = self.players.get(&id).context("native player missing")?;
        ensure!(p.persistence_failure.is_none(), "inactive native character");
        ensure!(
            p.active(),
            crate::server::lifecycle::controller("inactive native character")
        );
        ensure!(
            time >= 0 && time >= p.last_move,
            crate::server::lifecycle::controller("native position clock regressed")
        );
        ensure!(
            pos.x.is_finite() && pos.y.is_finite(),
            crate::server::lifecycle::controller("non-finite native position")
        );
        ensure!(
            self.map.cell(pos.x, pos.y).is_some(),
            crate::server::lifecycle::controller("native position is outside world bounds")
        );
        Ok(())
    }

    pub fn acknowledge_autonomous_goto(&mut self, id: i32) -> Result<()> {
        ensure!(
            self.players
                .get(&id)
                .context("GOTO ACK player missing")?
                .controller
                == Controller::Autonomous,
            "GOTO ACK requires autonomous ownership"
        );
        self.acknowledge_goto(
            id,
            self.now
                .try_into()
                .context("autonomous GOTO clock overflow")?,
        )
    }

    pub fn acknowledge_goto(&mut self, id: i32, time: i32) -> Result<()> {
        let p = self
            .players
            .get_mut(&id)
            .context("GOTO ACK player missing")?;
        let (own, sent) = *p.goto_acks.front().ok_or_else(|| {
            crate::server::lifecycle::protocol("unsolicited GOTO acknowledgement")
        })?;
        ensure!(
            time >= 0,
            crate::server::lifecycle::protocol("negative GOTO acknowledgement time")
        );
        ensure!(
            sent.elapsed() <= ACK_TIMEOUT,
            crate::server::lifecycle::protocol("GOTO acknowledgement timed out")
        );
        if own {
            ensure!(
                time >= p.last_move,
                crate::server::lifecycle::protocol("GOTO acknowledgement clock regressed")
            );
            // The native client ACKs with lastUpdate_, then moves to the supplied position.
            p.last_move = time;
        }
        p.goto_acks.pop_front();
        tracing::debug!(player = id, client_time = time, own, "GOTO acknowledged");
        Ok(())
    }

    fn validate_discarded_shot(&self, id: i32, time: i32, bullet: u8, kind: u16) -> Result<i32> {
        let p = self.players.get(&id).context("player missing")?;
        ensure!(
            time >= 0 && time >= p.last_attack,
            crate::server::lifecycle::protocol("invalid correcting SHOOT time")
        );
        ensure!(
            p.character.items[0] == i32::from(kind) || p.advertised_weapon(kind),
            crate::server::lifecycle::protocol("weapon does not match equipped item")
        );
        ensure!(
            bullet == p.next_bullet,
            crate::server::lifecycle::protocol("unexpected player bullet counter")
        );
        let (last, count) = p
            .discarded_attack
            .unwrap_or((p.last_attack, p.attack_count));
        ensure!(
            time >= last,
            crate::server::lifecycle::protocol("discarded attack clock regressed")
        );
        let item = self.content.get(kind)?;
        let count = if time == last { count } else { 0 };
        if time != last {
            let dex = p.advertised_dex(p.effective(&self.content, self.now)?[5]);
            let cooldown = p.normal_attack_period(dex, self.now, item.rate, true);
            let elapsed = i64::from(time) - i64::from(last);
            if elapsed < cooldown as i64 - 2 {
                tracing::warn!(
                    player = id,
                    kind,
                    dex,
                    client_time = time,
                    last_client_time = last,
                    elapsed_ms = elapsed,
                    required_ms = cooldown,
                    condition = p.conditions(self.now),
                    "discarded attack cadence rejected"
                );
                return Err(crate::server::lifecycle::protocol(
                    "discarded attack too fast",
                ));
            }
        }
        ensure!(
            count < item.shots,
            crate::server::lifecycle::protocol("too many discarded projectiles in attack")
        );
        Ok(count)
    }

    fn consume_native_discard(
        &mut self,
        id: i32,
        time: i32,
        bullet: u8,
        kind: u16,
        count: i32,
    ) -> Result<()> {
        let spec = self
            .content
            .get(kind)?
            .projectiles
            .get(&0)
            .context("weapon has no projectile zero")?;
        let p = self.players.get_mut(&id).context("player missing")?;
        // Ruffle allocated the shot before the correction, even though no authoritative bullet survives.
        p.roll_damage(spec);
        p.consume_discarded_shot(id, time, bullet, count);
        Ok(())
    }
    pub fn correct_native_shot(
        &mut self,
        id: i32,
        time: i32,
        bullet: u8,
        kind: u16,
        reason: &str,
    ) -> Result<()> {
        let count = self.validate_discarded_shot(id, time, bullet, kind)?;
        self.correct_position(id, time, reason)?;
        self.consume_native_discard(id, time, bullet, kind, count)
    }

    pub(super) fn discard_correcting_move(&self, id: i32, pos: Pos, time: i32) -> Result<bool> {
        let p = self.players.get(&id).context("player missing")?;
        ensure!(
            time >= 0 && pos.x.is_finite() && pos.y.is_finite(),
            crate::server::lifecycle::protocol("invalid correcting MOVE")
        );
        let correcting = p.correcting()?;
        if correcting {
            tracing::warn!(
                player = id,
                client_time = time,
                "discarded MOVE awaiting GOTO acknowledgement"
            );
        }
        Ok(correcting)
    }

    pub(super) fn discard_correcting_shot(
        &mut self,
        id: i32,
        time: i32,
        bullet: u8,
        kind: u16,
        start: Pos,
        angle: f32,
    ) -> Result<bool> {
        let p = self.players.get(&id).context("player missing")?;
        let correcting = p.correcting()?;
        let continuing = p.discarded_attack.is_some_and(|(last, _)| time == last);
        if let Some((last, _)) = p.discarded_attack {
            ensure!(
                time >= last,
                crate::server::lifecycle::protocol("discarded attack clock regressed")
            );
        }
        if !correcting && !continuing {
            return Ok(false);
        }
        ensure!(
            start.x.is_finite() && start.y.is_finite() && angle.is_finite(),
            crate::server::lifecycle::protocol("invalid correcting SHOOT")
        );
        let count = self.validate_discarded_shot(id, time, bullet, kind)?;
        self.consume_native_discard(id, time, bullet, kind, count)?;
        Ok(true)
    }

    pub(super) fn expire_goto_acks(&mut self) {
        for (id, p) in &mut self.players {
            if !p.departing
                && p.goto_acks
                    .front()
                    .is_some_and(|(_, sent)| sent.elapsed() > ACK_TIMEOUT)
            {
                if let Some(output) = &p.tx {
                    lose(&output.lost, "goto_ack_timeout");
                }
                p.departing = true;
                tracing::error!(
                    player = id,
                    "GOTO acknowledgement timed out; connection closed"
                );
            }
        }
    }
}
