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
    pub(super) fn spend_ability(
        &mut self,
        id: i32,
        slot: Slot,
        item: &Object,
        pos: Pos,
    ) -> Result<()> {
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
                .is_none_or(|tx| !*tx.lost.borrow() && !tx.packets.is_closed()),
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
        ensure!(
            self.now >= p.ability_ready_ms,
            controller("ability cooldown")
        );
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
        let ability_ready_ms = self
            .now
            .checked_add(item.cooldown_ms)
            .context("ability cooldown overflow")?;
        let p = self.players.get_mut(&id).unwrap();
        p.character.mp -= item.mana_cost;
        p.ability_ready_ms = ability_ready_ms;
        tracing::info!(
            player = id,
            item = item.kind,
            mana_cost = item.mana_cost,
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
        self.actors()
            .filter(|e| {
                e.hp > 0 && self.content.objects[&e.kind].enemy && e.pos.distance(pos) <= radius
            })
            .map(|e| e.id)
            .collect()
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
                        self.players[&id].controller == Controller::Human,
                        controller("projectile abilities require native reports")
                    );
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
            // A stationary caster has no recent movement heading; legacy decoys choose a random direction.
            Some(match self.players[&id].last_motion {
                Some((angle, at)) if self.now - at <= 100 => angle,
                _ => self.rng.gen_range(0. ..std::f32::consts::TAU),
            })
        } else {
            None
        };
        for effect in &item.abilities {
            tracing::debug!(
                player = id,
                item = item.kind,
                ?effect,
                "ability activation applied"
            );
            match *effect {
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
            .actors()
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
                    .actors()
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
    fn ability_nova(&mut self, id: i32, item: &Object, pos: Pos) -> Result<()> {
        let spec = item
            .projectiles
            .get(&0)
            .context("ability has no projectile zero")?
            .clone();
        let p = self.players.get_mut(&id).unwrap();
        p.character.shots += 20;
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
                        let goal = Pos {
                            x: pos.x + angle.cos() * speed * 0.05,
                            y: pos.y + angle.sin() * speed * 0.05,
                        };
                        if self.walkable(goal) {
                            self.move_entity(id, goal, speed * 0.05)
                                .expect("validated decoy movement");
                        }
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
mod tests {
    use super::*;
    use crate::content::ROGUE;
    use crate::world::tests::{ch, directory};

    fn rogue() -> (Directory, i32) {
        let mut d = directory();
        let w = d.worlds.get_mut(&-2).unwrap();
        let mut c = ch(w);
        c.class = ROGUE;
        c.base = w.content.classes[&ROGUE].caps;
        c.mp = c.base[1];
        c.items = vec![-1, 0xa56, -1, -1, 0xa5a, -1, -1, -1, -1, -1, -1, -1];
        let id = w.enter(c, None).unwrap();
        (d, id)
    }
    fn cast(w: &mut World, id: i32) -> Result<Option<InventoryPlan>> {
        w.use_item(
            id,
            Slot {
                object: id,
                slot: 1,
                kind: w.players[&id].character.items[1],
            },
            w.players[&id].pos,
        )
    }
    fn swap(w: &mut World, id: i32) {
        let items = &w.players[&id].character.items;
        let plan = w
            .swap(
                id,
                Slot {
                    object: id,
                    slot: 1,
                    kind: items[1],
                },
                Slot {
                    object: id,
                    slot: 4,
                    kind: items[4],
                },
            )
            .unwrap();
        let revision = plan.revision + 1;
        w.finish_inventory(plan, Ok(revision)).unwrap();
    }
    fn ready(d: &Directory, id: i32) -> u64 {
        d.player_view(id).unwrap().character.ability_ready_ms
    }

    #[test]
    fn ability_cooldown_short_to_long_accepts_at_original_deadline() {
        let (mut d, id) = rogue();
        let w = d.worlds.get_mut(&-2).unwrap();
        cast(w, id).unwrap();
        swap(w, id);
        w.now = 3999;
        let mp = w.players[&id].character.mp;
        assert!(cast(w, id).err().unwrap().to_string().contains("cooldown"));
        assert_eq!(w.players[&id].character.mp, mp);
        w.now = 4000;
        cast(w, id).unwrap();
        assert_eq!(w.players[&id].character.mp, mp - 100);
        assert_eq!(ready(&d, id), 10500);
    }

    #[test]
    fn ability_cooldown_long_to_short_cannot_shorten_original_deadline() {
        let (mut d, id) = rogue();
        let w = d.worlds.get_mut(&-2).unwrap();
        swap(w, id);
        cast(w, id).unwrap();
        swap(w, id);
        let mp = w.players[&id].character.mp;
        for now in [4000, 6499] {
            w.now = now;
            assert!(cast(w, id).err().unwrap().to_string().contains("cooldown"));
            assert_eq!(w.players[&id].character.mp, mp);
        }
        w.now = 6500;
        cast(w, id).unwrap();
        assert_eq!(w.players[&id].character.mp, mp - 60);
        assert_eq!(ready(&d, id), 10500);
    }

    #[test]
    fn ability_cooldown_view_survives_committed_swaps_and_same_item_boundary() {
        let (mut d, id) = rogue();
        assert_eq!(ready(&d, id), 0);
        cast(d.worlds.get_mut(&-2).unwrap(), id).unwrap();
        assert_eq!(ready(&d, id), 4000);
        swap(d.worlds.get_mut(&-2).unwrap(), id);
        assert_eq!(ready(&d, id), 4000);
        swap(d.worlds.get_mut(&-2).unwrap(), id);
        let w = d.worlds.get_mut(&-2).unwrap();
        w.now = 3999;
        assert!(cast(w, id).is_err());
        w.now = 4000;
        cast(w, id).unwrap();
        assert_eq!(ready(&d, id), 8000);
    }

    #[test]
    fn ability_cooldown_pre_spend_refusals_preserve_mp_deadline_and_effects() {
        for case in 0..12 {
            let (mut d, id) = rogue();
            let w = d.worlds.get_mut(&-2).unwrap();
            cast(w, id).unwrap();
            swap(w, id);
            w.now = 7000;
            let mut target = w.players[&id].pos;
            let p = w.players.get_mut(&id).unwrap();
            match case {
                0 => p.character.mp = 99,
                1 => {
                    p.statuses.insert(crate::condition::QUIET, (0, 10000));
                }
                2 => p.paused = true,
                3 => p.character.alive = false,
                4 => p.death_pending = true,
                5 => p.departing = true,
                6 => p.persistence_failure = Some("test quarantine".into()),
                7 => p.pending = true,
                8 => target.x = f32::NAN,
                9 => target.x += 21.,
                10 => {
                    let (packets, rx) = mpsc::channel(1);
                    drop(rx);
                    let (lost, _) = watch::channel(false);
                    p.tx = Some(Output { packets, lost });
                }
                11 => w.now = u64::MAX - 1000,
                _ => unreachable!(),
            }
            let mp = p.character.mp;
            let statuses = p.statuses.clone();
            let pos = p.pos;
            let deadline = ready(&d, id);
            let w = d.worlds.get_mut(&-2).unwrap();
            let error = w
                .use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 1,
                        kind: 0xa5a,
                    },
                    target,
                )
                .err()
                .unwrap_or_else(|| panic!("case {case} accepted"));
            if case == 11 {
                assert_eq!(error.to_string(), "ability cooldown overflow");
            }
            assert_eq!(w.players[&id].character.mp, mp, "case {case}");
            assert_eq!(w.players[&id].statuses, statuses, "case {case}");
            assert_eq!((w.players[&id].pos.x, w.players[&id].pos.y), (pos.x, pos.y));
            assert!(
                w.ability_state.thrown.is_empty()
                    && w.ability_state.poisons.is_empty()
                    && w.ability_state.objects.is_empty()
            );
            assert_eq!(ready(&d, id), deadline, "case {case}");
        }
    }
}
