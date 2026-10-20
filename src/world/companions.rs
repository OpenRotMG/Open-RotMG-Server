use super::*;
use crate::content::Object;

#[derive(Clone)]
pub(crate) struct Plan {
    pub source: Slot,
    pub permanent: bool,
    pub selected: Option<u16>,
    entity: Option<i32>,
    cooldown: u64,
}
struct Companion {
    owner: i32,
    kind: u16,
    permanent: bool,
    published: bool,
    until: Option<u64>,
    pulse: u64,
    remaining: u8,
}
#[derive(Default)]
pub(super) struct State {
    active: BTreeMap<i32, Companion>,
    bleeds: BTreeMap<i32, (u64, u64)>,
}
impl State {
    pub(super) fn contains(&self, entity: &i32) -> bool {
        self.active.contains_key(entity)
    }
}
// Firecrackers expire after their last pulse; drake lifetimes come from the period eggs table.
fn duration(kind: u16) -> Result<Option<u64>> {
    match kind {
        0x1401 => Ok(Some(60_000)),
        0x1400 | 0x1402..=0x1405 => Ok(Some(30_000)),
        0x1407 | 0x1408 => Ok(None),
        _ => bail!("This temporary companion is not implemented yet."),
    }
}
impl World {
    fn reserve_companion(
        &mut self,
        owner: i32,
        kind: u16,
        permanent: bool,
        pos: Pos,
    ) -> Result<i32> {
        ensure!(
            self.companions.active.len() < 128,
            controller("companion population limit reached")
        );
        if !permanent {
            duration(kind)?;
            if matches!(kind, 0x1407 | 0x1408) {
                // Prove all ten cumulative [-2, 2) jumps valid before consumption or RNG draws.
                ensure!(
                    pos.x.is_finite()
                        && pos.y.is_finite()
                        && self.map.cell(pos.x - 20., pos.y - 20.).is_some()
                        && self.map.cell(pos.x + 10., pos.y + 10.).is_some(),
                    controller(
                        "Firecrackers need at least 20 tiles from the left/top and more than 10 from the right/bottom world boundary."
                    )
                );
            }
            ensure!(
                self.companions
                    .active
                    .values()
                    .filter(|p| p.owner == owner && !p.permanent)
                    .count()
                    < 8,
                controller("at most eight temporary companions may be active")
            );
        }
        let entity = self.spawn_entity(kind, pos, vec![])?;
        let e = self.entities.get_mut(&entity).unwrap();
        e.published = false;
        e.reserved = true;
        for (stat, value) in [32, 33]
            .into_iter()
            .zip(self.content.objects[&kind].textures)
        {
            if let Some(value) = value {
                e.extra_stats.push((stat, value as i32));
            }
        }
        self.companions.active.insert(
            entity,
            Companion {
                owner,
                kind,
                permanent,
                published: false,
                until: None,
                pulse: self.now,
                remaining: if matches!(kind, 0x1407 | 0x1408) {
                    10
                } else {
                    0
                },
            },
        );
        Ok(entity)
    }
    pub(super) fn attach_companion(&mut self, owner: i32, kind: u16, pos: Pos) -> Result<()> {
        self.content.permanent_companion(kind)?;
        let id = self.reserve_companion(owner, kind, true, pos)?;
        self.publish_companion(id)
    }
    fn publish_companion(&mut self, id: i32) -> Result<()> {
        let pet = self
            .companions
            .active
            .get_mut(&id)
            .context("reserved companion lost")?;
        ensure!(!pet.published, "companion already published");
        pet.published = true;
        if !pet.permanent {
            pet.until = duration(pet.kind)?.map(|duration| self.now + duration);
        }
        pet.pulse = self.now + if pet.remaining > 0 { 0 } else { 50 };
        let e = self
            .entities
            .get_mut(&id)
            .context("reserved companion entity lost")?;
        ensure!(
            e.reserved && !e.published,
            "companion publication reservation lost"
        );
        e.reserved = false;
        e.published = true;
        tracing::info!(
            companion = id,
            owner = pet.owner,
            kind = pet.kind,
            until = pet.until,
            "companion published"
        );
        Ok(())
    }
    pub(super) fn begin_companion(
        &mut self,
        id: i32,
        slot: Slot,
        item: &Object,
    ) -> Result<InventoryPlan> {
        let metadata = item
            .companion
            .as_ref()
            .context("companion activation metadata missing")?;
        ensure!(
            item.class == "Equipment" && item.slot_type == 10 && item.activations == 1,
            "invalid companion item activation"
        );
        let kind = self.content.companion_kind(&metadata.object)?;
        let p = &self.players[&id];
        if metadata.permanent {
            ensure!(
                p.last_companion
                    .is_none_or(|last| self.now >= last + metadata.cooldown),
                controller("companion generator cooldown")
            );
        } else {
            ensure!(item.consumable, "temporary companion must consume its egg");
            if let Some(duration) = duration(kind)? {
                self.now
                    .checked_add(duration)
                    .context("companion deadline overflow")?;
            }
        }
        let selected =
            if metadata.permanent && !item.consumable && p.character.pet_kind == Some(kind) {
                None
            } else {
                Some(kind)
            };
        let pos = p.pos;
        let mut items = p.character.items.clone();
        if item.consumable {
            items[slot.slot] = -1;
        }
        let entity = if selected.is_some() {
            Some(self.reserve_companion(id, kind, metadata.permanent, pos)?)
        } else {
            None
        };
        let mut plan = self.plan(id, items, None);
        plan.companion = Some(Plan {
            source: slot,
            permanent: metadata.permanent,
            selected,
            entity,
            cooldown: metadata.cooldown,
        });
        self.players.get_mut(&id).unwrap().pending = true;
        tracing::info!(
            player = id,
            item = item.kind,
            kind,
            entity,
            selected,
            "companion staged; item not committed yet"
        );
        Ok(plan)
    }
    pub(super) fn discard_companion(&mut self, plan: &Option<Plan>) -> Result<()> {
        if let Some(entity) = plan.as_ref().and_then(|p| p.entity) {
            let pet = self
                .companions
                .active
                .get(&entity)
                .context("unpublished companion lost")?;
            ensure!(!pet.published, "cannot discard a published companion");
            self.remove_companion(entity);
        }
        Ok(())
    }
    pub(super) fn finish_companion(&mut self, owner: i32, plan: Option<Plan>) -> Result<()> {
        let Some(plan) = plan else { return Ok(()) };
        if plan.permanent {
            let old: Vec<_> = self
                .companions
                .active
                .iter()
                .filter(|(id, p)| p.owner == owner && p.permanent && Some(**id) != plan.entity)
                .map(|(id, _)| *id)
                .collect();
            for id in old {
                self.remove_companion(id);
            }
            let p = self
                .players
                .get_mut(&owner)
                .context("committed companion owner lost")?;
            p.character.pet_kind = plan.selected;
            p.last_companion = Some(self.now);
        }
        if let Some(entity) = plan.entity {
            self.publish_companion(entity)?;
        }
        tracing::info!(
            player = owner,
            selected = plan.selected,
            permanent = plan.permanent,
            cooldown = plan.cooldown,
            "companion selection committed"
        );
        Ok(())
    }
    fn remove_companion(&mut self, entity: i32) {
        self.companions.active.remove(&entity);
        if self.entities.contains_key(&entity) {
            self.entities.get_mut(&entity).unwrap().reserved = false;
            self.remove_entity(entity);
        }
    }
    pub(super) fn clear_companions(&mut self, owner: i32) {
        let ids: Vec<_> = self
            .companions
            .active
            .iter()
            .filter(|(_, p)| p.owner == owner)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.remove_companion(id);
        }
    }
    fn companion_effect(&self, id: i32, radius: f32) {
        let body = cooperative::effect(5, id, Pos { x: radius, y: 0. });
        for p in self.players.values().filter(|p| p.known.contains(&id)) {
            send(p, wire::SHOW_EFFECT, body.clone());
        }
    }
    fn firecracker_pulse(&mut self, id: i32, kind: u16) -> Result<()> {
        let pos = self.entities[&id].pos;
        let pos = Pos {
            x: pos.x + self.rng.gen_range(-2..2) as f32,
            y: pos.y + self.rng.gen_range(-2..2) as f32,
        };
        self.position_entity(id, pos)?;
        let radius = if kind == 0x1407 {
            1
        } else {
            self.rng.gen_range(2..4)
        } as f32;
        let color = self.rng.gen_range(0..0xffffff);
        let mut effect = Writer::new();
        effect
            .u8(5)
            .i32(id)
            .f32(radius)
            .f32(0.)
            .f32(0.)
            .f32(0.)
            .u32(color);
        let mut sound = Writer::new();
        sound.i32(id).u8(0);
        for p in self.players.values().filter(|p| p.known.contains(&id)) {
            send(p, wire::SHOW_EFFECT, effect.0.clone());
            if p.pos.distance(pos) <= 25. {
                send(p, wire::PLAY_SOUND, sound.0.clone());
            }
        }
        let targets: Vec<_> = self
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
        for target in &targets {
            self.damage_enemy(0, *target, 100, true, &[], None)?;
        }
        let pet = self.companions.active.get_mut(&id).unwrap();
        pet.remaining -= 1;
        pet.pulse = self.now
            + if kind == 0x1408 && pet.remaining == 9 {
                250
            } else {
                100
            };
        if pet.remaining == 0 {
            pet.until = Some(self.now + 1);
        }
        tracing::info!(
            companion = id,
            kind,
            x = pos.x,
            y = pos.y,
            radius,
            color,
            targets = targets.len(),
            remaining = pet.remaining,
            tick = self.now,
            "firecracker pulse"
        );
        Ok(())
    }
    pub(super) fn companion_step(&mut self) -> Result<()> {
        let ids: Vec<_> = self.companions.active.keys().copied().collect();
        for id in ids {
            let pet = &self.companions.active[&id];
            if !pet.published {
                continue;
            }
            let (owner, kind) = (pet.owner, pet.kind);
            if !self.entities.contains_key(&id)
                || pet.until.is_some_and(|until| until <= self.now)
                || self
                    .players
                    .get(&owner)
                    .is_none_or(|p| !p.active() || p.character.hp <= 0)
            {
                self.remove_companion(id);
                continue;
            }
            let owner = &self.players[&owner];
            if owner.paused {
                continue;
            }
            if matches!(kind, 0x1407 | 0x1408) {
                if pet.remaining > 0 && pet.pulse <= self.now {
                    self.firecracker_pulse(id, kind)?;
                }
                continue;
            }
            let goal = owner.pos;
            let pos = self.entities[&id].pos;
            let distance = pos.distance(goal);
            if distance > 12. {
                self.position_entity(id, goal)?;
            } else if distance > 1. {
                let step = (distance - 1.).min(0.3);
                self.position_entity(
                    id,
                    Pos {
                        x: pos.x + (goal.x - pos.x) * step / distance,
                        y: pos.y + (goal.y - pos.y) * step / distance,
                    },
                )?;
            }
            let pet = self.companions.active.get_mut(&id).unwrap();
            if pet.permanent || pet.pulse > self.now {
                continue;
            }
            pet.pulse = self.now
                + match kind {
                    0x1400 | 0x1404 => 300,
                    0x1401 => 500,
                    0x1403 => 2000,
                    0x1402 | 0x1405 => 1000,
                    _ => bail!("unvalidated companion pulse {kind:#x}"),
                };
            let position = self.entities[&id].pos;
            if kind == 0x1401 {
                let mut target: Option<(i32, f32, i32)> = None;
                for (id, p) in &self.players {
                    let cap = p.effective(&self.content, self.now)?[0];
                    let distance = p.pos.distance(position);
                    if p.active()
                        && !p.paused
                        && p.character.hp > 0
                        && p.character.hp < cap
                        && distance <= 5.
                        && p.conditions(self.now) & crate::condition::bit(crate::condition::SICK)
                            == 0
                        && target.is_none_or(|(_, nearest, _)| distance < nearest)
                    {
                        target = Some((*id, distance, cap));
                    }
                }
                if let Some((target, _, cap)) = target {
                    let p = self.players.get_mut(&target).unwrap();
                    p.character.hp = (p.character.hp + 25).min(cap);
                    self.companion_effect(target, 1.);
                }
                continue;
            }
            let target = self
                .entities
                .values()
                .filter(|e| {
                    e.hp > 0
                        && self.content.objects[&e.kind].enemy
                        && e.pos.distance(position) <= 5.
                        && !crate::condition::protected(e.conditions(self.now))
                })
                .min_by(|a, b| {
                    a.pos
                        .distance(position)
                        .total_cmp(&b.pos.distance(position))
                })
                .map(|e| e.id);
            let Some(target) = target else {
                continue;
            };
            self.companion_effect(target, 1.);
            match kind {
                0x1400 => {
                    let center = self.entities[&target].pos;
                    let targets: Vec<_> = self
                        .entities
                        .values()
                        .filter(|e| {
                            e.hp > 0
                                && self.content.objects[&e.kind].enemy
                                && e.pos.distance(center) <= 1.
                        })
                        .map(|e| e.id)
                        .collect();
                    for target in targets {
                        // Period drake damage does not establish player loot or weapon credit.
                        self.damage_enemy(0, target, 35, false, &[], None)?;
                    }
                }
                0x1402 => self.apply_condition(target, crate::condition::PARALYZED, 5000)?,
                0x1403 => self.apply_condition(target, crate::condition::STASIS, 5000)?,
                0x1404 => self.apply_condition(target, crate::condition::SLOWED, 10000)?,
                0x1405 => {
                    self.apply_condition(target, crate::condition::BLEEDING, 10000)?;
                    self.companions
                        .bleeds
                        .entry(target)
                        .and_modify(|bleed| bleed.0 = self.now + 10000)
                        .or_insert((self.now + 10000, self.now + 1000));
                }
                _ => bail!("unvalidated companion behavior {kind:#x}"),
            }
        }
        let targets: Vec<_> = self.companions.bleeds.keys().copied().collect();
        for target in targets {
            let (until, next) = self.companions.bleeds[&target];
            if !self.entities.contains_key(&target) || self.now > until {
                self.companions.bleeds.remove(&target);
                continue;
            }
            if self.now >= next {
                self.companions.bleeds.get_mut(&target).unwrap().1 = self.now + 1000;
                self.damage_enemy(0, target, 30, true, &[], None)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InventoryWrite, Store};
    use crate::wire::Reader;
    use crate::world::tests as fixtures;

    fn fixture(kind: i32) -> (World, i32) {
        let mut w = fixtures::world();
        let mut ch = fixtures::ch(&w);
        ch.items[5] = kind;
        let id = w.enter(ch, None).unwrap();
        (w, id)
    }
    fn stage(w: &mut World, id: i32, slot: usize) -> InventoryPlan {
        w.use_item(
            id,
            Slot {
                object: id,
                slot,
                kind: w.players[&id].character.items[slot],
            },
            w.players[&id].pos,
        )
        .unwrap()
        .unwrap()
    }
    fn commit(w: &mut World, plan: InventoryPlan) {
        let revision = plan.revision + 1;
        w.finish_inventory(plan, Ok(revision)).unwrap();
    }
    fn target(w: &mut World, id: i32) -> i32 {
        let pos = w.players[&id].pos;
        let enemy = w
            .spawn_entity(
                0x600,
                Pos {
                    x: pos.x + 2.,
                    y: pos.y,
                },
                vec![],
            )
            .unwrap();
        w.entities.get_mut(&enemy).unwrap().hp = 1000;
        enemy
    }

    fn firecracker_fixture(item: i32) -> (World, i32) {
        let base = fixtures::arena(1);
        let mut map = (*base.map).clone();
        map.width = 64;
        map.height = 64;
        map.cells = vec![map.cells[0].clone(); 64 * 64];
        map.spawns = vec![(25., 25.)];
        let mut w = World::new(base.content.clone(), Arc::new(map)).unwrap();
        let mut ch = fixtures::ch(&w);
        ch.items[5] = item;
        let id = w.enter(ch, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = Pos { x: 25., y: 25. };
        (w, id)
    }

    #[test]
    fn firecrackers_execute_ten_seeded_pulses_without_following_or_player_credit_then_expire() {
        use rand::{Rng, SeedableRng};
        for (item, kind) in [(0xc2b, 0x1407), (0xc2c, 0x1408)] {
            let (mut w, id) = firecracker_fixture(item);
            let origin = w.players[&id].pos;
            w.rng = rand::rngs::StdRng::seed_from_u64(12351);
            let enemy = target(&mut w, id);
            w.entities.get_mut(&enemy).unwrap().hp = 2000;
            let plan = stage(&mut w, id, 5);
            let entity = plan.companion.as_ref().unwrap().entity.unwrap();
            assert_eq!(w.entities[&entity].kind, kind);
            assert!(!w.entities[&entity].published);
            w.now += 5000;
            w.companion_step().unwrap();
            assert_eq!(w.entities[&enemy].hp, 2000);
            w.players.get_mut(&id).unwrap().pos = Pos { x: 19.5, y: 19.5 };
            commit(&mut w, plan);
            assert_eq!(w.entities[&entity].pos.distance(origin), 0.);
            assert_eq!(w.players[&id].character.items[5], -1);
            let (packets, mut rx) = tokio::sync::mpsc::channel(128);
            let (lost, _) = tokio::sync::watch::channel(false);
            let p = w.players.get_mut(&id).unwrap();
            p.tx = Some(Output { packets, lost });
            p.known.insert(entity);
            let mut rng = w.rng.clone();
            let mut position = origin;
            for pulse in 0..10 {
                position.x += rng.gen_range(-2..2) as f32;
                position.y += rng.gen_range(-2..2) as f32;
                let radius = if kind == 0x1407 {
                    1
                } else {
                    rng.gen_range(2..4)
                };
                let color: u32 = rng.gen_range(0..0xffffff);
                w.position_entity(enemy, position).unwrap();
                let before = w.entities[&enemy].hp;
                w.companion_step().unwrap();
                assert_eq!(w.entities[&entity].pos.distance(position), 0.);
                assert_eq!(w.entities[&enemy].hp, before - 100);
                assert!(w.entities[&enemy].damage.is_empty());
                let received: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
                let effects: Vec<_> = received
                    .iter()
                    .filter(|p| p.id == wire::SHOW_EFFECT)
                    .collect();
                assert_eq!(effects.len(), 1);
                let mut effect = Reader::new(&effects[0].body);
                assert_eq!(effect.u8().unwrap(), 5);
                assert_eq!(effect.i32().unwrap(), entity);
                assert_eq!(effect.f32().unwrap(), radius as f32);
                for _ in 0..3 {
                    assert_eq!(effect.f32().unwrap(), 0.);
                }
                assert_eq!(effect.i32().unwrap() as u32, color);
                effect.done().unwrap();
                let sounds: Vec<_> = received.iter().filter(|p| p.id == 44).collect();
                assert_eq!(sounds.len(), 1);
                let mut sound = Reader::new(&sounds[0].body);
                assert_eq!(sound.i32().unwrap(), entity);
                assert_eq!(sound.u8().unwrap(), 0);
                sound.done().unwrap();
                if pulse < 9 {
                    let interval = if kind == 0x1408 && pulse == 0 {
                        250
                    } else {
                        100
                    };
                    w.now += interval - 1;
                    w.companion_step().unwrap();
                    assert_eq!(w.entities[&entity].pos.distance(position), 0.);
                    assert!(rx.try_recv().is_err());
                    w.now += 1;
                }
            }
            assert_eq!(w.entities[&enemy].hp, 1000);
            assert_eq!(
                (
                    w.players[&id].character.shots,
                    w.players[&id].character.hits,
                    w.players[&id].character.kills
                ),
                (0, 0, 0)
            );
            w.companion_step().unwrap();
            assert!(rx.try_recv().is_err());
            w.now += 50;
            w.companion_step().unwrap();
            assert!(!w.entities.contains_key(&entity));
            assert!(w.companions.active.is_empty());
        }
    }

    #[test]
    fn firecrackers_reject_unsafe_envelopes_without_consumption_reservation_or_rng() {
        use rand::RngCore;
        for item in [0xc2b, 0xc2c] {
            let (mut w, id) = firecracker_fixture(item);
            let revision = w.players[&id].character.revision;
            for pos in [
                Pos { x: 19.99, y: 25. },
                Pos { x: 25., y: 19.99 },
                Pos { x: 54., y: 25. },
                Pos { x: 25., y: 54. },
            ] {
                w.players.get_mut(&id).unwrap().pos = pos;
                let mut rng = w.rng.clone();
                let error = w
                    .use_item(
                        id,
                        Slot {
                            object: id,
                            slot: 5,
                            kind: item,
                        },
                        pos,
                    )
                    .err()
                    .unwrap();
                assert_eq!(
                    error.downcast_ref::<CloseReason>(),
                    Some(&CloseReason::Controller)
                );
                assert!(crate::server::lifecycle::rule_rejection(&error));
                assert!(error.to_string().contains("world boundary"));
                assert_eq!(w.players[&id].character.items[5], item);
                assert_eq!(w.players[&id].character.revision, revision);
                assert!(!w.players[&id].pending);
                assert!(w.companions.active.is_empty());
                assert_eq!(w.rng.next_u64(), rng.next_u64());
            }
            for pos in [Pos { x: 20., y: 20. }, Pos { x: 53.99, y: 53.99 }] {
                w.players.get_mut(&id).unwrap().pos = pos;
                let plan = stage(&mut w, id, 5);
                commit(&mut w, plan);
                w.clear_companions(id);
                w.players.get_mut(&id).unwrap().character.items[5] = item;
            }
        }
        let (mut w, id) = fixture(0xc2b);
        assert!(w.map.width < 31);
        assert!(
            w.use_item(
                id,
                Slot {
                    object: id,
                    slot: 5,
                    kind: 0xc2b
                },
                w.players[&id].pos
            )
            .is_err()
        );
        assert_eq!(w.players[&id].character.items[5], 0xc2b);
    }

    #[test]
    fn firecracker_area_damage_preserves_protection_contributors_radius_and_armor_piercing() {
        use crate::condition::{INVINCIBLE, INVULNERABLE, STASIS};
        use rand::Rng;
        let (mut w, id) = firecracker_fixture(0xc2b);
        let plan = stage(&mut w, id, 5);
        commit(&mut w, plan);
        let origin = w.players[&id].pos;
        let mut rng = w.rng.clone();
        let center = Pos {
            x: origin.x + rng.gen_range(-2..2) as f32,
            y: origin.y + rng.gen_range(-2..2) as f32,
        };
        let armored = w
            .content
            .objects
            .values()
            .find(|o| o.enemy && o.defense >= 100)
            .unwrap()
            .kind;
        let mut enemies = Vec::new();
        for protection in [None, Some(INVINCIBLE), Some(INVULNERABLE), Some(STASIS)] {
            let enemy = w.spawn_entity(armored, center, vec![]).unwrap();
            w.entities.get_mut(&enemy).unwrap().hp = 1000;
            w.entities.get_mut(&enemy).unwrap().damage.insert(123, 25);
            if let Some(effect) = protection {
                w.apply_condition(enemy, effect, 10000).unwrap();
            }
            enemies.push(enemy);
        }
        let outside = w
            .spawn_entity(
                armored,
                Pos {
                    x: center.x + 1.01,
                    y: center.y,
                },
                vec![],
            )
            .unwrap();
        w.entities.get_mut(&outside).unwrap().hp = 1000;
        let player_hp = w.players[&id].character.hp;
        w.companion_step().unwrap();
        assert_eq!(w.entities[&enemies[0]].hp, 900);
        for enemy in &enemies[1..] {
            assert_eq!(w.entities[enemy].hp, 1000);
        }
        for enemy in enemies {
            assert_eq!(w.entities[&enemy].damage, BTreeMap::from([(123, 25)]));
        }
        assert_eq!(w.entities[&outside].hp, 1000);
        assert_eq!(w.players[&id].character.hp, player_hp);
    }

    #[test]
    fn firecrackers_celebrate_without_targets_and_share_caps_rollback_and_departure_cleanup() {
        for item in [0xc2b, 0xc2c] {
            for failed in [true, false] {
                let (mut w, id) = firecracker_fixture(item);
                let plan = stage(&mut w, id, 5);
                let entity = plan.companion.as_ref().unwrap().entity.unwrap();
                let result = if failed {
                    Err(anyhow::anyhow!("injected firecracker save failure"))
                } else {
                    Ok(plan.revision + 2)
                };
                assert!(w.finish_inventory(plan, result).is_err());
                assert_eq!(w.players[&id].character.items[5], item);
                assert!(!w.entities.contains_key(&entity));
                assert!(w.companions.active.is_empty());
                assert!(w.players[&id].persistence_failure.is_some());
            }
            let (mut w, id) = firecracker_fixture(item);
            for _ in 0..8 {
                let plan = stage(&mut w, id, 5);
                commit(&mut w, plan);
                w.players.get_mut(&id).unwrap().character.items[5] = item;
            }
            assert!(
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 5,
                        kind: item
                    },
                    w.players[&id].pos
                )
                .is_err()
            );
            assert_eq!(w.players[&id].character.items[5], item);
            let (packets, mut rx) = tokio::sync::mpsc::channel(32);
            let (lost, _) = tokio::sync::watch::channel(false);
            let p = w.players.get_mut(&id).unwrap();
            p.tx = Some(Output { packets, lost });
            p.known.extend(w.companions.active.keys());
            w.companion_step().unwrap();
            let packets: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
            assert_eq!(
                packets.iter().filter(|p| p.id == wire::SHOW_EFFECT).count(),
                8
            );
            assert_eq!(
                packets.iter().filter(|p| p.id == wire::PLAY_SOUND).count(),
                8
            );
            let saved = w.leave(id).unwrap();
            assert_eq!(saved.items[5], item);
            assert!(w.companions.active.is_empty());
            assert!(
                w.entities
                    .values()
                    .all(|e| !matches!(e.kind, 0x1407 | 0x1408))
            );
        }
    }

    #[tokio::test]
    async fn firecracker_consumption_uses_existing_exchange_once_and_survives_store_reopen() {
        for item in [0xc2b, 0xc2c] {
            let (mut w, _) = firecracker_fixture(item);
            w.players.clear();
            let path = std::env::temp_dir().join(format!(
                "openrotmg-firecracker-{:016x}.db",
                rand::random::<u64>()
            ));
            let store = Store::open(&path, &w.content).unwrap();
            let account = store.list("prepared-firecracker".into()).await.unwrap().id;
            let mut ch = store
                .create(account, w.content.get(0x030e).unwrap())
                .await
                .unwrap();
            ch.items[5] = item;
            ch.revision = store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap()[0];
            let character = ch.id;
            let id = w.enter(ch, None).unwrap();
            w.players.get_mut(&id).unwrap().pos = Pos { x: 25., y: 25. };
            let plan = stage(&mut w, id, 5);
            let write = InventoryWrite {
                id: character,
                account,
                revision: plan.revision,
                items: plan.items.clone(),
            };
            let revision = store.exchange(vec![write.clone()], vec![]).await.unwrap()[0];
            w.finish_inventory(plan, Ok(revision)).unwrap();
            assert!(store.exchange(vec![write], vec![]).await.is_err());
            assert!(
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 5,
                        kind: item
                    },
                    w.players[&id].pos
                )
                .is_err()
            );
            w.leave(id).unwrap();
            assert!(w.companions.active.is_empty());
            drop(store);
            let restarted = Store::open(&path, &w.content).unwrap();
            let saved = restarted.load(account, character).await.unwrap();
            assert_eq!(saved.items[5], -1);
            assert_eq!(saved.revision, revision);
            assert_eq!(saved.pet_kind, None);
        }
    }
}
