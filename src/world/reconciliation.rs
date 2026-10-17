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
        output.lost.send_replace(true);
        return;
    }
    // ACK has no object ID, so other players' GOTOs must share the same FIFO.
    p.goto_acks.push_back((own, Instant::now()));
    send(p, wire::GOTO, body);
}

impl Player {
    pub(super) fn advertised_weapon(&self, kind: u16) -> bool {
        self.weapon_ticks.iter().any(|(_, k)| *k == i32::from(kind))
    }
    pub(super) fn remember_weapon_tick(&mut self) {
        if self.controller != Controller::Human {
            return;
        }
        if self.weapon_ticks.len() == ACK_LIMIT {
            tracing::error!(
                character = self.character.id,
                "tick acknowledgement backlog exceeded"
            );
            if let Some(output) = &self.tx {
                output.lost.send_replace(true);
            }
            return;
        }
        self.weapon_ticks
            .push_back((self.tick, self.character.items[0]));
    }
    pub(super) fn correcting(&self) -> Result<bool> {
        ensure!(self.active(), "inactive player cannot reconcile position");
        if let Some((_, sent)) = self.goto_acks.iter().find(|(own, _)| *own) {
            ensure!(
                sent.elapsed() <= ACK_TIMEOUT,
                "GOTO acknowledgement timed out"
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
    pub fn acknowledge_tick(&mut self, id: i32, tick: i32) -> Result<()> {
        let p = self
            .players
            .get_mut(&id)
            .context("tick ACK player missing")?;
        ensure!(
            tick >= 0 && tick < p.tick,
            "unsolicited tick acknowledgement"
        );
        // NEW_TICK queues MOVE before applying inventory; earlier SHOOTs still use the old weapon.
        p.weapon_ticks.retain(|(sent, _)| *sent >= tick);
        Ok(())
    }
    pub(super) fn validate_native_position(&self, id: i32, pos: Pos, time: i32) -> Result<()> {
        let p = self.players.get(&id).context("native player missing")?;
        ensure!(p.active(), "inactive native character");
        ensure!(
            time >= 0 && time >= p.last_move,
            "native position clock regressed"
        );
        ensure!(
            pos.x.is_finite() && pos.y.is_finite(),
            "non-finite native position"
        );
        ensure!(
            self.player_step_valid(p.pos, pos),
            "native position is outside walkable terrain"
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
        let (own, sent) = *p
            .goto_acks
            .front()
            .context("unsolicited GOTO acknowledgement")?;
        ensure!(time >= 0, "negative GOTO acknowledgement time");
        ensure!(
            sent.elapsed() <= ACK_TIMEOUT,
            "GOTO acknowledgement timed out"
        );
        if own {
            ensure!(time >= p.last_move, "GOTO acknowledgement clock regressed");
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
            "invalid correcting SHOOT time"
        );
        ensure!(
            p.character.items[0] == i32::from(kind) || p.advertised_weapon(kind),
            "weapon does not match equipped item"
        );
        ensure!(bullet == p.next_bullet, "unexpected player bullet counter");
        let (last, count) = p
            .discarded_attack
            .unwrap_or((p.last_attack, p.attack_count));
        ensure!(time >= last, "discarded attack clock regressed");
        let item = self.content.get(kind)?;
        let count = if time == last { count } else { 0 };
        if time != last {
            let rate = 0.0015 + 0.0065 * p.effective(&self.content, self.now)?[5] as f64 / 75.;
            let cooldown = (1. / (rate * f64::from(item.rate))).floor() as f32;
            ensure!(
                i64::from(time) - i64::from(last) >= cooldown as i64 - 2,
                "discarded attack too fast"
            );
        }
        ensure!(
            count < item.shots,
            "too many discarded projectiles in attack"
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
            "invalid correcting MOVE"
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
            ensure!(time >= last, "discarded attack clock regressed");
        }
        if !correcting && !continuing {
            return Ok(false);
        }
        ensure!(
            start.x.is_finite() && start.y.is_finite() && angle.is_finite(),
            "invalid correcting SHOOT"
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
                    output.lost.send_replace(true);
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
