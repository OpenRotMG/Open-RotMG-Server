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
    #[cfg(test)]
    pub(crate) fn lifecycle_test_goto_acks(&self) -> usize {
        self.goto_acks.len()
    }
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
    pub fn acknowledge_tick(&mut self, id: i32, tick: i32) -> Result<()> {
        let p = self
            .players
            .get_mut(&id)
            .context("tick ACK player missing")?;
        ensure!(
            tick >= 0 && tick < p.tick,
            crate::server::lifecycle::protocol("unsolicited tick acknowledgement")
        );
        // NEW_TICK queues MOVE before applying inventory; earlier SHOOTs still use the old weapon.
        p.weapon_ticks.retain(|(sent, _)| *sent >= tick);
        Ok(())
    }
    pub(super) fn validate_native_position(&self, id: i32, pos: Pos, time: i32) -> Result<()> {
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
            self.player_step_valid(p.pos, pos),
            crate::server::lifecycle::controller("native position is outside walkable terrain")
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
            let rate = 0.0015 + 0.0065 * p.effective(&self.content, self.now)?[5] as f64 / 75.;
            let cooldown = (1. / (rate * f64::from(item.rate))).floor() as f32;
            ensure!(
                i64::from(time) - i64::from(last) >= cooldown as i64 - 2,
                crate::server::lifecycle::protocol("discarded attack too fast")
            );
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

#[cfg(test)]
mod lifecycle_finalize_tests {
    use super::*;
    use crate::server::{
        self,
        lifecycle::{self, CloseReason, Stage},
    };

    async fn swap(a: &server::App, id: i32) {
        lifecycle::account(
            a,
            vec![
                a.world
                    .for_player(id)
                    .call(move |w| Ok(w.players[&id].character.account))
                    .await
                    .unwrap(),
            ],
            "finalize_ack_inventory",
            move |a| async move {
                lifecycle::stage(Stage::Reservation);
                let plan = a
                    .world
                    .for_player(id)
                    .call(move |w| {
                        let items = &w.players[&id].character.items;
                        w.swap(
                            id,
                            Slot {
                                object: id,
                                slot: 4,
                                kind: items[4],
                            },
                            Slot {
                                object: id,
                                slot: 5,
                                kind: items[5],
                            },
                        )
                    })
                    .await?;
                lifecycle::stage(Stage::Store);
                let result = a
                    .store
                    .exchange(
                        vec![crate::store::InventoryWrite {
                            id: plan.character,
                            account: plan.account,
                            revision: plan.revision,
                            items: plan.items.clone(),
                        }],
                        vec![],
                    )
                    .await
                    .and_then(|v| v.into_iter().next().context("missing committed revision"));
                lifecycle::stage(Stage::Publication);
                a.world
                    .for_player(id)
                    .call(move |w| w.finish_inventory(plan, result))
                    .await?;
                lifecycle::stage(Stage::Settled);
                Ok(())
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn lifecycle_finalize_autonomous_expired_goto_preserves_commit_peer_and_exclusive_reentry()
     {
        let (dir, a) = server::lifecycle_test_fixture();
        let mut actors = vec![];
        let mut outputs = vec![];
        for name in ["finalize-ack-bot", "finalize-ack-peer"] {
            let account = a.store.list(name.into()).await.unwrap().id;
            let ch = a
                .store
                .create(account, a.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            let saved = ch.clone();
            let owner = a.claim(account).unwrap();
            let bind = lifecycle::binding(&a, account).unwrap();
            let (packets, rx) = mpsc::channel(4096);
            outputs.push(rx);
            let (lost, _) = watch::channel(false);
            let id = a
                .world
                .call(move |w| {
                    let id = w.enter_autonomous(ch, Output { packets, lost })?;
                    bind(id)?;
                    Ok(id)
                })
                .await
                .unwrap();
            actors.push((owner, id, saved));
        }
        let (owner, id, ch) = actors.remove(0);
        let (peer_owner, peer, peer_ch) = actors.remove(0);
        swap(&a, id).await;
        let mut expected = ch.items.clone();
        expected.swap(4, 5);
        a.world
            .for_player(id)
            .call(move |w| {
                let p = w.players.get_mut(&id).unwrap();
                p.goto_acks
                    .push_back((true, Instant::now() - Duration::from_secs(6)));
                let error = w.acknowledge_autonomous_goto(id).unwrap_err();
                assert_eq!(
                    error.downcast_ref::<CloseReason>(),
                    Some(&CloseReason::Protocol)
                );
                w.expire_goto_acks();
                assert!(w.players[&id].departing);
                assert!(w.players[&id].persistence_failure.is_none());
                Ok(())
            })
            .await
            .unwrap();
        server::depart(&a, id).await.unwrap();
        assert!(
            a.claim(ch.account).is_err(),
            "bot transfer must retain its exclusive account lease"
        );
        swap(&a, peer).await;
        server::depart(&a, peer).await.unwrap();
        drop(peer_owner);
        let saved = a.store.load(ch.account, ch.id).await.unwrap();
        assert_eq!(
            (saved.revision, saved.items.clone()),
            (ch.revision + 1, expected)
        );
        assert_eq!(
            a.store
                .load(peer_ch.account, peer_ch.id)
                .await
                .unwrap()
                .revision,
            peer_ch.revision + 1
        );
        let bind = lifecycle::binding(&a, ch.account).unwrap();
        let replacement = a
            .world
            .call(move |w| {
                let replacement = w.enter(saved, None)?;
                bind(replacement)?;
                Ok(replacement)
            })
            .await
            .unwrap();
        assert_ne!(id, replacement);
        assert!(lifecycle::finalize(&a, id, CloseReason::Controller).is_err());
        assert!(a.claim(ch.account).is_err());
        server::depart(&a, replacement).await.unwrap();
        drop(owner);
        assert!(a.owners.lock().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
