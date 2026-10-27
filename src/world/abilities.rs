use super::*;
use crate::content::{Ability, Object};

const THROW_MS: u64 = 1500;
const MAX_ACTIVE: usize = 256;

#[derive(Default)]
pub(super) struct State {
    thrown: Vec<Thrown>,
    poisons: Vec<Poison>,
    pub(super) objects: BTreeMap<i32, Deployed>,
}
struct Thrown {
    owner: i32,
    position: Pos,
    at: u64,
    effect: Ability,
}
struct Poison {
    owner: i32,
    start: u64,
    duration: u64,
    tick: u64,
    targets: Vec<(i32, i32, i32)>,
}
pub(super) struct Deployed {
    owner: i32,
    until: u64,
    pulse: u64,
    kind: Deployment,
}
enum Deployment {
    Trap {
        radius: f32,
        damage: i32,
        effect: u8,
        duration: u64,
    },
    Decoy {
        angle: f32,
        speed: f32,
        moving_until: u64,
    },
    Bomb,
}
impl World {
    // Client getTimer cooldowns continue during skipped physics ticks: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/objects/Player.as#L899-L917
    pub(super) fn ability_clock_ms(&self) -> Result<u64> {
        Ok(self
            .clock
            .now()
            .checked_duration_since(self.attack_epoch)
            .context("ability clock regressed")?
            .as_millis() as u64)
    }

    pub(super) fn spend_ability(
        &mut self,
        id: i32,
        slot: Slot,
        item: &Object,
        pos: Pos,
    ) -> Result<()> {
        let now = self.ability_clock_ms()?;
        let p = &self.players[&id];
        ensure!(
            p.persistence_failure.is_none(),
            "ability caster is unavailable"
        );
        ensure!(
            p.character.alive
                && !p.death_pending
                && !p.departing
                && (!p.pending || p.progress_started)
                && !p.paused
                && p.persistence_failure.is_none(),
            controller("ability caster is unavailable")
        );
        ensure!(
            slot.object == id && slot.kind == item.kind as i32 && p.character.items[1] == slot.kind,
            controller("ability item does not match equipped item")
        );
        ensure!(
            slot.slot == 1 && item.slot_type == self.content.get(p.character.class)?.slots[1],
            controller("ability must be equipped by a compatible class")
        );
        ensure!(
            p.tx.as_ref()
                .is_none_or(|tx| !tx.lost.borrow().is_some() && !tx.packets.is_closed()),
            controller("ability connection is closed")
        );
        ensure!(
            p.conditions(self.now) & crate::condition::bit(crate::condition::QUIET) == 0,
            controller("cannot cast while Quiet")
        );
        ensure!(
            p.character.mp >= item.mana_cost,
            controller("not enough mana")
        );
        ensure!(now >= p.ability_ready_ms, controller("ability cooldown"));
        let reach = item
            .abilities
            .iter()
            .filter_map(|a| {
                if let Ability::Teleport { distance } = a {
                    Some(*distance)
                } else {
                    None
                }
            })
            .fold(15., f32::max);
        ensure!(
            pos.x.is_finite()
                && pos.y.is_finite()
                && p.pos.distance(pos) <= reach
                && self.map.cell(pos.x, pos.y).is_some(),
            controller("invalid ability target or target too far")
        );
        let ability_ready_ms = now
            .checked_add(item.cooldown_ms)
            .context("ability cooldown overflow")?;
        let p = self.players.get_mut(&id).unwrap();
        p.character.mp -= item.mana_cost;
        p.character.lifetime.ability_used = true;
        p.ability_ready_ms = ability_ready_ms;
        tracing::info!(
            player = id,
            item = item.kind,
            mana_cost = item.mana_cost,
            cooldown_ms = item.cooldown_ms,
            ability_clock_ms = now,
            simulation_time_ms = self.now,
            "ability activated"
        );
        Ok(())
    }
    fn ability_allies(&self, id: i32, range: Option<f32>) -> Vec<i32> {
        let origin = self.players[&id].pos;
        self.players
            .iter()
            .filter(|(target, p)| {
                p.character.alive
                    && !p.death_pending
                    && !p.departing
                    && !p.paused
                    && p.persistence_failure.is_none()
                    && match range {
                        Some(r) => origin.distance(p.pos) <= r,
                        None => **target == id,
                    }
            })
            .map(|(id, _)| *id)
            .collect()
    }
    fn ability_enemies(&self, pos: Pos, radius: f32) -> Vec<i32> {
        let mut ids: Vec<_> = self
            .near_entities(
                Pos {
                    x: pos.x - radius,
                    y: pos.y - radius,
                },
                Pos {
                    x: pos.x + radius,
                    y: pos.y + radius,
                },
            )
            .filter(|e| {
                e.hp > 0 && self.content.objects[&e.kind].enemy && e.pos.distance(pos) <= radius
            })
            .map(|e| e.id)
            .collect();
        ids.sort_unstable();
        ids
    }
    fn ability_effect(&self, kind: u8, owner: i32, a: Pos, b: Pos, color: u32) {
        let mut out = Writer::new();
        out.u8(kind)
            .i32(owner)
            .f32(a.x)
            .f32(a.y)
            .f32(b.x)
            .f32(b.y)
            .u32(color);
        for p in self.players.values().filter(|p| p.known.contains(&owner)) {
            send(p, wire::SHOW_EFFECT, out.0.clone());
        }
    }
    pub(super) fn activate_ability(
        &mut self,
        id: i32,
        slot: Slot,
        item: &Object,
        pos: Pos,
    ) -> Result<()> {
        ensure!(
            item.abilities.len() == item.activations,
            "ability activation count mismatch"
        );
        ensure!(
            self.ability_state.thrown.len()
                + self.ability_state.poisons.len()
                + self.ability_state.objects.len()
                + item.abilities.len()
                <= MAX_ACTIVE,
            controller("active ability budget exhausted")
        );
        let origin = self.players[&id].pos;
        for effect in &item.abilities {
            match effect {
                Ability::Shoot => {
                    ensure!(
                        (1..=5).contains(&item.shots)
                            && item.activations == 1
                            && item.projectiles.contains_key(&0),
                        "unsupported ability projectile layout"
                    );
                }
                Ability::Nova => {
                    ensure!(
                        item.projectiles.contains_key(&0),
                        "ability has no projectile zero"
                    );
                }
                Ability::Teleport { distance } => {
                    ensure!(
                        origin.distance(pos) <= *distance && self.walkable(pos),
                        controller("teleport target is blocked or outside ability range")
                    );
                }
                Ability::Trap { .. } => {
                    ensure!(self.walkable(pos), controller("trap target is blocked"));
                    self.content.get(0x711)?;
                }
                Ability::Decoy { .. } => {
                    self.content.get(0x715)?;
                }
                Ability::SpiritBomb => {
                    self.content.get(0x1409)?;
                }
                _ => {}
            }
        }
        self.spend_ability(id, slot, item, pos)?;
        let heading = if item
            .abilities
            .iter()
            .any(|a| matches!(a, Ability::Decoy { .. }))
        {
            // Legacy decoys follow recent movement, or pick a random heading: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/Decoy.cs#L34-L47
            Some(match self.players[&id].last_motion {
                Some((angle, at)) if self.now - at <= 100 => angle,
                _ => self.rng.gen_range(0. ..std::f32::consts::TAU),
            })
        } else {
            None
        };
        // Preserve XML order: Purification heals before removing Sick (selected objects.xml, Tome of Purification 0xc09).
        for effect in &item.abilities {
            tracing::debug!(
                player = id,
                item = item.kind,
                ?effect,
                "ability activation applied"
            );
            match *effect {
                // Native clients report their own ability projectiles; other controllers have no client.
                Ability::Shoot if self.players[&id].controller != Controller::Human => {
                    self.ability_shots(id, item, origin, pos)?
                }
                Ability::Shoot => {}
                Ability::Nova => self.ability_nova(id, item, pos)?,
                Ability::Heal { amount, range } => self.heal_ability(id, item, amount, range)?,
                Ability::Condition {
                    effect,
                    duration,
                    range,
                } => {
                    for target in self.ability_allies(id, range) {
                        self.apply_condition(target, effect, duration)?;
                    }
                }
                Ability::Clear {
                    effect,
                    check,
                    range,
                } => {
                    for target in self.ability_allies(id, Some(range)) {
                        let p = self.players.get_mut(&target).unwrap();
                        if check
                            .is_none_or(|c| p.conditions(self.now) & crate::condition::bit(c) != 0)
                        {
                            p.statuses.remove(&effect);
                        }
                    }
                }
                Ability::Cleanse { range } => {
                    for target in self.ability_allies(id, Some(range)) {
                        self.players
                            .get_mut(&target)
                            .unwrap()
                            .statuses
                            .retain(|effect, _| !matches!(effect, 2..=11 | 14 | 16 | 27 | 28));
                    }
                }
                Ability::Poison { .. } | Ability::Trap { .. } => {
                    self.ability_effect(
                        4,
                        id,
                        pos,
                        origin,
                        if matches!(effect, Ability::Poison { .. }) {
                            0xffddff00
                        } else {
                            0xff9000ff
                        },
                    );
                    self.ability_state.thrown.push(Thrown {
                        owner: id,
                        position: pos,
                        at: self.now + THROW_MS,
                        effect: effect.clone(),
                    });
                }
                Ability::Drain { radius, damage } => {
                    self.ability_effect(7, id, pos, origin, 0xffff0000);
                    self.ability_effect(
                        8,
                        id,
                        pos,
                        Pos {
                            x: pos.x + radius,
                            y: pos.y,
                        },
                        0xffff0000,
                    );
                    let mut total = 0;
                    for target in self.ability_enemies(pos, radius) {
                        total += self.damage_enemy(id, target, damage, false, &[], None)?;
                    }
                    if total > 0 {
                        self.heal_ability(id, item, total, radius)?;
                    }
                }
                Ability::Stasis { duration } => {
                    self.ability_effect(
                        12,
                        id,
                        pos,
                        Pos {
                            x: pos.x + 3.,
                            y: pos.y,
                        },
                        0xffffffff,
                    );
                    for target in self.ability_enemies(pos, 3.) {
                        let flags = self.entities[&target].conditions(self.now);
                        if flags
                            & (crate::condition::bit(crate::condition::STASIS)
                                | crate::condition::bit(crate::condition::STASIS_IMMUNE))
                            == 0
                        {
                            self.apply_condition(target, crate::condition::STASIS, duration)?;
                        } else if flags & crate::condition::bit(crate::condition::STASIS_IMMUNE)
                            != 0
                        {
                            let mut out = Writer::new();
                            out.i32(target).utf("Immune").u32(0xff00ff00);
                            for p in self.players.values().filter(|p| p.known.contains(&target)) {
                                send(p, wire::NOTIFICATION, out.0.clone());
                            }
                            tracing::debug!(player = id, target, "stasis target immune");
                        }
                    }
                }
                Ability::Teleport { .. } => {
                    let p = self.players.get_mut(&id).unwrap();
                    p.pos = pos;
                    p.movement_credit = 0.78;
                    p.last_motion = None;
                    let mut out = Writer::new();
                    out.i32(id).f32(pos.x).f32(pos.y);
                    for (peer, p) in &mut self.players {
                        if p.known.contains(&id) {
                            reconciliation::goto(p, *peer == id, out.0.clone());
                        }
                    }
                    self.ability_effect(2, id, pos, origin, 0xffffffff);
                }
                Ability::Decoy { duration, angle } => {
                    let object = self.spawn_entity(0x715, origin, vec![])?;
                    self.ability_state.objects.insert(
                        object,
                        Deployed {
                            owner: id,
                            until: self.now + duration,
                            pulse: self.now,
                            kind: Deployment::Decoy {
                                angle: heading.unwrap() + angle,
                                speed: self.players[&id].speed(&self.content, self.now)? * 1000.,
                                moving_until: self.now + duration / 2,
                            },
                        },
                    );
                }
                Ability::Lightning {
                    damage,
                    targets,
                    condition,
                    color,
                } => self.ability_lightning(id, pos, damage, targets, condition, color)?,
                Ability::Daze { radius, damage } => {
                    self.ability_effect(
                        8,
                        id,
                        pos,
                        Pos {
                            x: pos.x + radius,
                            y: pos.y,
                        },
                        0xffffffff,
                    );
                    for target in self.ability_enemies(pos, radius) {
                        self.damage_enemy(
                            id,
                            target,
                            damage,
                            false,
                            &[(crate::condition::DAZED, 3000)],
                            None,
                        )?;
                    }
                }
                Ability::SpiritBomb => {
                    // This preserved item predates the modern six-blast prism: one 300-damage spirit bomb.
                    let object = self.spawn_entity(0x1409, origin, vec![])?;
                    self.ability_state.objects.insert(
                        object,
                        Deployed {
                            owner: id,
                            until: self.now + 3000,
                            pulse: self.now,
                            kind: Deployment::Bomb,
                        },
                    );
                }
            }
        }
        Ok(())
    }
    fn ability_shots(&mut self, id: i32, item: &Object, origin: Pos, target: Pos) -> Result<()> {
        let time = self.now as i32;
        let aim = (target.y - origin.y).atan2(target.x - origin.x);
        // Like the client, every pellet leaves the central muzzle at its own spread angle.
        let start = Pos {
            x: origin.x + 0.3 * aim.cos(),
            y: origin.y + 0.3 * aim.sin(),
        };
        let p = self.players.get_mut(&id).unwrap();
        p.ability_shot = Some((item.kind, time, true, 0));
        let first = p.next_bullet;
        for i in 0..item.shots {
            let spread = item.arc.to_radians() * (i as f32 - (item.shots - 1) as f32 / 2.);
            self.shoot(
                id,
                time,
                (first + i as u8) % 128,
                item.kind,
                start,
                aim + spread,
            )?;
        }
        Ok(())
    }
    fn ability_lightning(
        &mut self,
        owner: i32,
        pos: Pos,
        damage: i32,
        maximum: usize,
        effect: Option<(u8, u64)>,
        color: u32,
    ) -> Result<()> {
        let origin = self.players[&owner].pos;
        let aim = (pos.y - origin.y).atan2(pos.x - origin.x);
        let eligible = |e: &&Entity| {
            self.content.objects[&e.kind].enemy
                && e.hp > 0
                && e.conditions(self.now)
                    & (crate::condition::bit(crate::condition::STASIS)
                        | crate::condition::bit(crate::condition::INVINCIBLE)
                        | crate::condition::bit(crate::condition::INVULNERABLE))
                    == 0
        };
        let first = self
            .ability_enemies(origin, 14.)
            .into_iter()
            .map(|id| &self.entities[&id])
            .filter(eligible)
            .filter(|e| {
                origin.distance(e.pos) <= 14.
                    && ((e.pos.y - origin.y).atan2(e.pos.x - origin.x) - aim)
                        .sin()
                        .atan2(((e.pos.y - origin.y).atan2(e.pos.x - origin.x) - aim).cos())
                        .abs()
                        <= std::f32::consts::FRAC_PI_4
            })
            .min_by(|a, b| origin.distance(a.pos).total_cmp(&origin.distance(b.pos)))
            .map(|e| e.id);
        let mut chain = Vec::new();
        if let Some(first) = first {
            chain.push(first);
            while chain.len() < maximum {
                let from = self.entities[chain.last().unwrap()].pos;
                let next = self
                    .ability_enemies(from, 10.)
                    .into_iter()
                    .map(|id| &self.entities[&id])
                    .filter(eligible)
                    .filter(|e| !chain.contains(&e.id) && from.distance(e.pos) <= 10.)
                    .min_by(|a, b| from.distance(a.pos).total_cmp(&from.distance(b.pos)))
                    .map(|e| e.id);
                match next {
                    Some(id) => chain.push(id),
                    None => break,
                }
            }
        }
        let mut previous = owner;
        if chain.is_empty() {
            for offset in [
                0.,
                -std::f32::consts::FRAC_PI_4,
                std::f32::consts::FRAC_PI_4,
            ] {
                let angle = aim + offset;
                self.ability_effect(
                    7,
                    owner,
                    Pos {
                        x: origin.x + 14. * angle.cos(),
                        y: origin.y + 14. * angle.sin(),
                    },
                    Pos { x: 350., y: 0. },
                    color,
                );
            }
        }
        // Announce every chain segment before lethal damage can remove its previous endpoint.
        for target in &chain {
            self.ability_effect(
                11,
                previous,
                self.entities[target].pos,
                Pos { x: 350., y: 0. },
                color,
            );
            previous = *target;
        }
        let effects = effect.into_iter().collect::<Vec<_>>();
        for target in chain {
            self.damage_enemy(owner, target, damage, false, &effects, None)?;
        }
        Ok(())
    }
    // Nova geometry is server-defined, not XML: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.UseItem.cs#L206-L237
    fn ability_nova(&mut self, id: i32, item: &Object, pos: Pos) -> Result<()> {
        let spec = item
            .projectiles
            .get(&0)
            .context("ability has no projectile zero")?
            .clone();
        let p = self.players.get_mut(&id).unwrap();
        p.character.shots += 20;
        p.character.lifetime.shots = p
            .character
            .lifetime
            .shots
            .checked_add(20)
            .context("lifetime ability shots overflow")?;
        let mut messages = Vec::new();
        for i in 0..20 {
            let bullet = p.server_bullet;
            p.server_bullet = 128 + (bullet.wrapping_add(1) % 128);
            let angle = i as f32 * std::f32::consts::TAU / 20.;
            let damage = self.rng.gen_range(spec.min..=spec.max);
            self.bullets.push(Bullet {
                owner: id,
                source_kind: p.character.class,
                id: bullet,
                friendly: true,
                start: pos,
                pos,
                angle,
                born: self.now,
                damage,
                spec: spec.clone(),
                hit: BTreeSet::new(),
                hit_enemy: false,
            });
            let mut out = Writer::new();
            out.u8(bullet)
                .i32(id)
                .i16(item.kind as i16)
                .f32(pos.x)
                .f32(pos.y)
                .f32(angle)
                .i16(damage as i16);
            messages.push(out.0);
        }
        for body in messages {
            for (peer, p) in &mut self.players {
                if p.known.contains(&id)
                    && (*peer == id || p.pos.distance(pos) <= 20.)
                    && (*peer != id || native_hits::expect_shoot_ack(p, vec![], &self.clock))
                {
                    send(p, 13, body.clone());
                }
            }
        }
        Ok(())
    }
    fn ability_owner(&self, id: i32) -> bool {
        self.players.get(&id).is_some_and(|p| {
            p.character.alive
                && !p.death_pending
                && !p.departing
                && !p.paused
                && p.persistence_failure.is_none()
        })
    }
    pub(super) fn decoy_positions(&self) -> impl Iterator<Item = Pos> + '_ {
        self.ability_state
            .objects
            .iter()
            .filter(|(_, o)| matches!(o.kind, Deployment::Decoy { .. }))
            .map(|(id, _)| self.entities[id].pos)
    }
    pub(super) fn clear_abilities(&mut self, owner: i32) {
        self.ability_state.thrown.retain(|t| t.owner != owner);
        self.ability_state.poisons.retain(|p| p.owner != owner);
        let ids: Vec<_> = self
            .ability_state
            .objects
            .iter()
            .filter(|(_, o)| o.owner == owner)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.remove_entity(id);
        }
    }
    pub(super) fn ability_step(&mut self) {
        for thrown in std::mem::take(&mut self.ability_state.thrown) {
            if !self.ability_owner(thrown.owner) {
                continue;
            }
            if self.now < thrown.at {
                self.ability_state.thrown.push(thrown);
                continue;
            }
            match thrown.effect {
                Ability::Poison {
                    radius,
                    damage,
                    duration,
                } => {
                    self.ability_effect(
                        8,
                        thrown.owner,
                        thrown.position,
                        Pos {
                            x: thrown.position.x + radius,
                            y: thrown.position.y,
                        },
                        0xffddff00,
                    );
                    // Snapshot landing targets and subtract defense once per dose: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.UseItem.cs#L164-L196
                    let targets = self
                        .ability_enemies(thrown.position, radius)
                        .into_iter()
                        .map(|id| {
                            let e = &self.entities[&id];
                            (
                                id,
                                crate::condition::damage(
                                    damage,
                                    self.content.objects[&e.kind].defense,
                                    e.conditions(self.now),
                                    false,
                                ),
                                0,
                            )
                        })
                        .collect();
                    self.ability_state.poisons.push(Poison {
                        owner: thrown.owner,
                        start: self.now,
                        duration,
                        tick: self.now,
                        targets,
                    });
                }
                Ability::Trap {
                    radius,
                    damage,
                    effect,
                    duration,
                } => {
                    let id = self
                        .spawn_entity(0x711, thrown.position, vec![])
                        .expect("validated trap landing");
                    self.ability_state.objects.insert(
                        id,
                        Deployed {
                            owner: thrown.owner,
                            until: self.now + 10000,
                            pulse: self.now,
                            kind: Deployment::Trap {
                                radius,
                                damage,
                                effect,
                                duration,
                            },
                        },
                    );
                }
                _ => unreachable!("only thrown abilities enter the landing queue"),
            }
        }
        for mut poison in std::mem::take(&mut self.ability_state.poisons) {
            if !self.ability_owner(poison.owner) {
                continue;
            }
            let elapsed = (self.now - poison.start).min(poison.duration);
            if self.now - poison.tick >= 250 || elapsed == poison.duration {
                poison.tick = self.now;
                poison
                    .targets
                    .retain(|(id, _, _)| self.entities.contains_key(id));
                for (target, total, paid) in &mut poison.targets {
                    let due = (i64::from(*total) * elapsed as i64 / poison.duration as i64) as i32;
                    let amount = due - *paid;
                    *paid = due;
                    if amount > 0 {
                        self.damage_enemy(poison.owner, *target, amount, true, &[], None)
                            .expect("validated poison damage");
                    }
                }
            }
            if elapsed < poison.duration && !poison.targets.is_empty() {
                self.ability_state.poisons.push(poison);
            }
        }
        for (id, mut object) in std::mem::take(&mut self.ability_state.objects) {
            if !self.entities.contains_key(&id) {
                continue;
            }
            if !self.ability_owner(object.owner) {
                self.remove_entity(id);
                continue;
            }
            let pos = self.entities[&id].pos;
            let expired = self.now >= object.until;
            match object.kind {
                Deployment::Trap {
                    radius,
                    damage,
                    effect,
                    duration,
                } => {
                    // The smaller ring triggers a full-radius blast, not one hit: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/Trap.cs#L49-L76
                    let triggered = !self.ability_enemies(pos, radius / 2.).is_empty();
                    if triggered || expired {
                        self.ability_effect(
                            8,
                            object.owner,
                            pos,
                            Pos {
                                x: pos.x + radius,
                                y: pos.y,
                            },
                            0xff9000ff,
                        );
                        for target in self.ability_enemies(pos, radius) {
                            self.damage_enemy(
                                object.owner,
                                target,
                                damage,
                                false,
                                &[(effect, duration)],
                                None,
                            )
                            .expect("validated trap damage");
                        }
                        self.remove_entity(id);
                        continue;
                    }
                    if self.now >= object.pulse {
                        self.ability_effect(
                            10,
                            id,
                            Pos {
                                x: radius / 2.,
                                y: 0.,
                            },
                            pos,
                            0xff9000ff,
                        );
                        object.pulse = self.now + 500;
                    }
                }
                Deployment::Decoy {
                    angle,
                    speed,
                    moving_until,
                } => {
                    if expired {
                        self.ability_effect(5, id, Pos { x: 1., y: 0. }, pos, 0xffff0000);
                        self.remove_entity(id);
                        continue;
                    }
                    if self.now < moving_until {
                        assert!(
                            angle.is_finite() && speed.is_finite() && speed >= 0.,
                            "invalid decoy trajectory for {id}: angle={angle}, speed={speed}"
                        );
                        let distance = speed * 0.05;
                        let delta = Pos {
                            x: angle.cos() * distance,
                            y: angle.sin() * distance,
                        };
                        let samples = (distance / 0.1).ceil().max(1.) as usize;
                        // Legal endpoints can straddle a blocked corner; wall contact stops this step.
                        let blocked = (1..=samples).any(|i| {
                            !self.walkable(Pos {
                                x: pos.x + delta.x / samples as f32 * i as f32,
                                y: pos.y + delta.y / samples as f32 * i as f32,
                            })
                        });
                        let resolved = if blocked {
                            pos
                        } else {
                            Pos {
                                x: pos.x + delta.x,
                                y: pos.y + delta.y,
                            }
                        };
                        self.position_entity(id, resolved)
                            .expect("validated decoy movement");
                    }
                }
                Deployment::Bomb => {
                    if expired {
                        self.ability_effect(5, id, Pos { x: 4., y: 0. }, pos, 0xffffffff);
                        for target in self.ability_enemies(pos, 4.) {
                            self.damage_enemy(object.owner, target, 300, false, &[], None)
                                .expect("validated spirit bomb damage");
                        }
                        self.remove_entity(id);
                        continue;
                    }
                }
            }
            self.ability_state.objects.insert(id, object);
        }
    }
}

#[cfg(test)]
#[path = "huntress_tests.rs"]
mod huntress_tests;
#[cfg(test)]
#[path = "sorcerer_tests.rs"]
mod sorcerer_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn decoy_world(origin: Pos, angle: f32, blocked: Option<(usize, usize)>) -> (World, i32, i32) {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let mut map = Map {
            width: 32,
            height: 32,
            cells: vec![
                crate::content::Cell {
                    ground: 0x46,
                    object: None,
                    config: String::new(),
                    terrain: 0,
                    region: 0,
                };
                32 * 32
            ],
            spawns: vec![(origin.x, origin.y)],
        };
        if let Some((x, y)) = blocked {
            map.cells[y * 32 + x].ground = 255;
        }
        let mut world = World::new(content, Arc::new(map)).unwrap();
        let character: Character = serde_json::from_value(serde_json::json!({
            "schema":1,"lifetime":crate::store::FameLifetime::complete(),
            "class":0x324,"base":[720,252,50,25,75,75,40,50],
            "id":1,"account":1,"name":"Decoy","level":20,"exp":0,
            "fame":0,"hp":720,"mp":252,"revision":0,"alive":true,
            "items":[-1,0xae3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
        }))
        .unwrap();
        let owner = world.enter(character, None).unwrap();
        world.players.get_mut(&owner).unwrap().last_motion = Some((angle, world.now));
        world
            .use_item(
                owner,
                Slot {
                    object: owner,
                    slot: 1,
                    kind: 0xae3,
                },
                origin,
            )
            .unwrap();
        assert_eq!(world.players[&owner].character.mp, 192);
        let decoy = *world.ability_state.objects.keys().next().unwrap();
        assert_eq!(world.ability_state.objects[&decoy].owner, owner);
        (world, owner, decoy)
    }

    #[tokio::test]
    async fn trickster_unlock_requires_both_level_twenty_records_after_death() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for levels in [[19, 20], [20, 19], [20, 20]] {
            let store =
                crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
            let account = store.list("trickster-unlock".into()).await.unwrap();
            let class = content.get(0x324).unwrap();
            assert!(store.create(account.id, class).await.is_err());
            for (kind, level) in [0x320, 0x31f].into_iter().zip(levels) {
                let mut ch = store
                    .create_prepared(account.id, content.get(kind).unwrap(), None)
                    .await
                    .unwrap();
                ch.gain_experience(
                    &content,
                    crate::store::level_start(level).unwrap(),
                    &mut rand::thread_rng(),
                )
                .unwrap();
                store.progress(ch.clone()).await.unwrap();
                ch.hp = 0;
                ch.alive = false;
                store
                    .death(ch, "prepared prerequisite".into())
                    .await
                    .unwrap();
            }
            let result = store.create(account.id, class).await;
            if levels == [20, 20] {
                let ch = result.unwrap();
                assert_eq!(ch.items[..4], [0xa14, 0xae3, -1, -1]);
                assert_eq!(ch.base, [150, 100, 10, 0, 12, 15, 12, 12]);
                assert_eq!(
                    store
                        .list("trickster-unlock".into())
                        .await
                        .unwrap()
                        .characters
                        .len(),
                    1
                );
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn trickster_teleport_rejects_blocked_destination_before_spending_or_deploying() {
        let origin = Pos { x: 4.5, y: 4.5 };
        let (mut world, owner, _) = decoy_world(origin, 0., Some((6, 4)));
        world.players.get_mut(&owner).unwrap().ability_ready_ms = 0;
        let before = world.players[&owner].character.mp;
        let objects = world.ability_state.objects.len();
        let slot = Slot {
            object: owner,
            slot: 1,
            kind: 0xae3,
        };
        assert!(world.use_item(owner, slot, Pos { x: 6.5, y: 4.5 }).is_err());
        assert_eq!(world.players[&owner].character.mp, before);
        assert_eq!(world.ability_state.objects.len(), objects);
        assert_eq!(world.players[&owner].pos.distance(origin), 0.);
        world.use_item(owner, slot, Pos { x: 8.5, y: 4.5 }).unwrap();
        assert_eq!(world.players[&owner].character.mp, before - 60);
        assert_eq!(world.players[&owner].pos.x, 8.5);
    }

    #[test]
    fn trickster_decoy_distracts_a_nearer_follower_then_releases_it() {
        let (mut world, owner, decoy) = decoy_world(Pos { x: 10.5, y: 10.5 }, 0., None);
        world.players.get_mut(&owner).unwrap().pos = Pos { x: 20.5, y: 10.5 };
        world.safe = false;
        world.encounter = true;
        let enemy = world
            .spawn_entity(0x6b4, Pos { x: 12.5, y: 10.5 }, vec![])
            .unwrap();
        world.step();
        let distracted = world.entities[&enemy].pos.x;
        assert!(distracted < 12.5);
        world.remove_entity(decoy);
        world.step();
        assert!(world.entities[&enemy].pos.x > distracted);
    }

    #[test]
    fn trickster_spirit_bomb_reconstruction_damages_once_after_three_seconds() {
        let origin = Pos { x: 10.5, y: 10.5 };
        let (mut world, owner, decoy) = decoy_world(origin, 0., None);
        world.remove_entity(decoy);
        let p = world.players.get_mut(&owner).unwrap();
        p.character.items[1] = 0xc2a;
        p.character.mp = 252;
        p.ability_ready_ms = 0;
        let target = world
            .spawn_entity(0x678, Pos { x: 13.5, y: 10.5 }, vec![])
            .unwrap();
        let hp = world.entities[&target].hp;
        world
            .use_item(
                owner,
                Slot {
                    object: owner,
                    slot: 1,
                    kind: 0xc2a,
                },
                Pos { x: 20.5, y: 10.5 },
            )
            .unwrap();
        assert_eq!(world.players[&owner].character.mp, 162);
        assert_eq!(world.decoy_positions().count(), 0);
        for _ in 0..59 {
            world.step();
        }
        assert_eq!(world.entities[&target].hp, hp);
        assert_eq!(world.ability_state.objects.len(), 1);
        world.step();
        let after = world.entities[&target].hp;
        assert_eq!(
            hp - after,
            (300 - world.content.objects[&0x678].defense).max(45)
        );
        assert!(world.ability_state.objects.is_empty());
        world.step();
        assert_eq!(world.entities[&target].hp, after);
    }

    #[test]
    fn decoy_tick_stops_before_blocked_intermediate_tile_with_walkable_endpoint() {
        let origin = Pos { x: 4.9, y: 4.7 };
        let angle = std::f32::consts::FRAC_PI_4;
        let (mut world, owner, decoy) = decoy_world(origin, angle, Some((5, 4)));
        let distance = world.players[&owner]
            .speed(&world.content, world.now)
            .unwrap()
            * 50.;
        let goal = Pos {
            x: origin.x + angle.cos() * distance,
            y: origin.y + angle.sin() * distance,
        };
        assert!(world.walkable(origin) && world.walkable(goal));
        assert!(!world.walkable(Pos { x: 5.03, y: 4.83 }));
        world.step();
        let stopped = world.entities[&decoy].pos;
        assert!(world.walkable(stopped));
        assert!(stopped.x < 5. && stopped.y < 5.);
        for _ in 0..28 {
            world.step();
        }
        assert!(world.entities[&decoy].pos.distance(stopped) < 1e-6);
        assert_eq!(world.ability_state.objects[&decoy].owner, owner);
    }
}
