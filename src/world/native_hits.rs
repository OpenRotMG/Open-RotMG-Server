use super::*;
use std::time::Duration;

pub(super) struct VisibleProjectile {
    bullet: Bullet,
    expires: u64,
    consumed: bool,
}

pub(super) fn expect_shoot_ack(p: &mut Player, bullets: Vec<Bullet>) -> bool {
    if p.controller != Controller::Human {
        return true;
    }
    if p.shoot_acks.len() >= 64 {
        let oldest = p.shoot_acks.front().and_then(|(_, shots)| shots.first());
        tracing::error!(
            character = p.character.id,
            owner = oldest.map(|b| b.owner),
            source_kind = oldest.map(|b| b.source_kind),
            born_ms = oldest.map(|b| b.born),
            last_native_move = p.last_move,
            "SHOOT acknowledgement backlog exceeded"
        );
        if let Some(output) = &p.tx {
            output.lost.send_replace(true);
        }
        return false;
    }
    p.shoot_acks.push_back((Instant::now(), bullets));
    true
}

impl World {
    pub(super) fn send_enemy_shot(&mut self, owner: i32, body: Vec<u8>) {
        assert!(
            body.len() == 20 || body.len() == 25,
            "invalid internal enemy volley"
        );
        let count = if body.len() == 25 { body[20] } else { 1 };
        let bullets: Vec<_> = (0..count)
            .map(|offset| {
                self.bullets
                    .iter()
                    .find(|b| {
                        !b.friendly
                            && b.born == self.now
                            && b.owner == owner
                            && b.id == body[0].wrapping_add(offset)
                    })
                    .expect("announced hostile projectile missing")
                    .clone()
            })
            .collect();
        for p in self
            .players
            .values_mut()
            .filter(|p| p.known.contains(&owner))
        {
            if expect_shoot_ack(p, bullets.clone()) {
                send(p, 19, body.clone());
            }
        }
    }

    pub fn acknowledge_shoot(&mut self, id: i32, time: i32) -> Result<()> {
        let p = self
            .players
            .get_mut(&id)
            .context("SHOOT ACK player missing")?;
        ensure!(time >= -1, "invalid SHOOT acknowledgement time");
        let (sent, bullets) = p
            .shoot_acks
            .pop_front()
            .context("unsolicited SHOOT acknowledgement")?;
        ensure!(
            sent.elapsed() <= Duration::from_secs(5),
            "SHOOT acknowledgement timed out"
        );
        for bullet in bullets {
            let key = (bullet.owner, bullet.id);
            // ACK and hit reports share the client FIFO, so an ID's next generation cannot steal an older report.
            if time < 0 {
                p.visible_projectiles.remove(&key);
            } else {
                let expires = self.now + bullet.spec.lifetime as u64 + 5000;
                p.visible_projectiles.insert(
                    key,
                    VisibleProjectile {
                        bullet,
                        expires,
                        consumed: false,
                    },
                );
            }
        }
        tracing::debug!(
            player = id,
            client_time = time,
            "native volley acknowledged"
        );
        Ok(())
    }

    pub fn native_player_hit(&mut self, id: i32, owner: i32, bullet: u8) -> Result<()> {
        let p = self
            .players
            .get_mut(&id)
            .context("hit report player missing")?;
        let Some(report) = p.visible_projectiles.get_mut(&(owner, bullet)) else {
            tracing::warn!(
                player = id,
                owner,
                bullet,
                "hit report rejected: projectile was not acknowledged or expired"
            );
            return Ok(());
        };
        if report.consumed || self.now > report.expires {
            tracing::warn!(
                player = id,
                owner,
                bullet,
                consumed = report.consumed,
                "duplicate or expired hit report rejected"
            );
            return Ok(());
        }
        report.consumed = true;
        let shot = report.bullet.clone();
        if self.safe || !p.character.alive || p.paused || p.departing || p.death_pending {
            tracing::debug!(
                player = id,
                owner,
                bullet,
                "reported hit ignored for inactive or safe character"
            );
            return Ok(());
        }
        self.damage_player(id, &shot, false)?;
        tracing::debug!(
            player = id,
            owner,
            bullet,
            hp = self.players[&id].character.hp,
            "client-observed projectile hit committed"
        );
        Ok(())
    }

    pub(super) fn expire_native_projectiles(&mut self) {
        for (id, p) in &mut self.players {
            p.visible_projectiles
                .retain(|_, report| self.now <= report.expires);
            if !p.departing
                && p.shoot_acks
                    .front()
                    .is_some_and(|(sent, _)| sent.elapsed() > Duration::from_secs(5))
            {
                if let Some(output) = &p.tx {
                    output.lost.send_replace(true);
                }
                p.departing = true;
                let (sent, bullets) = p.shoot_acks.front().expect("expired acknowledgement");
                tracing::error!(
                    player = id,
                    pending = p.shoot_acks.len(),
                    oldest_ms = sent.elapsed().as_millis(),
                    shooter = ?bullets.first().map(|b|b.owner),
                    volley = bullets.len(),
                    source_kind = bullets.first().map(|b| b.source_kind),
                    projectile = bullets.first().map(|b| b.id),
                    born_ms = bullets.first().map(|b| b.born),
                    last_native_move = p.last_move,

                    "SHOOT acknowledgement timed out; connection closed"
                );
            }
        }
    }

    pub(super) fn damage_player(&mut self, id: i32, b: &Bullet, echo: bool) -> Result<()> {
        let p = self.players.get(&id).context("projectile target missing")?;
        let flags = p.conditions(self.now);
        let affects = !crate::condition::protected(flags);
        let damage =
            crate::condition::damage(b.damage, self.defense(id)?, flags, b.spec.armor_piercing);
        if affects {
            for (index, duration) in &b.spec.effects {
                self.apply_condition(id, *index, *duration)?;
            }
        }
        let p = self.players.get_mut(&id).unwrap();
        p.character.hp = (p.character.hp - damage).max(0);
        if p.character.hp == 0 {
            p.death_pending = true;
            p.killer = self.content.objects[&b.source_kind].name.clone();
            p.bot_goal = None;
            p.bot_aim = None;
        }
        if echo {
            let mut out = Writer::new();
            out.i32(id).u8(if affects {
                b.spec.effects.len() as u8
            } else {
                0
            });
            if affects {
                for (index, _) in &b.spec.effects {
                    out.u8(*index);
                }
            }
            out.i16(damage as i16).u8(0).u8(b.id).i32(b.owner);
            send(p, 47, out.0);
        }
        tracing::debug!(
            player = id,
            damage,
            hp = p.character.hp,
            echo,
            "player damaged"
        );
        Ok(())
    }
}
