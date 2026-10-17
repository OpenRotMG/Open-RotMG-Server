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
// The selected eggs omit lifetime/AI; the period Drake Eggs table supplies these profiles.
fn duration(kind: u16) -> Result<u64> {
    match kind {
        0x1401 => Ok(60_000),
        0x1400 | 0x1402..=0x1405 => Ok(30_000),
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
            "companion population limit reached"
        );
        if !permanent {
            duration(kind)?;
            ensure!(
                self.companions
                    .active
                    .values()
                    .filter(|p| p.owner == owner && !p.permanent)
                    .count()
                    < 8,
                "at most eight temporary companions may be active"
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
            pet.until = Some(self.now + duration(pet.kind)?);
        }
        pet.pulse = self.now + 50;
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
                "companion generator cooldown"
            );
        } else {
            ensure!(item.consumable, "temporary companion must consume its egg");
            self.now
                .checked_add(duration(kind)?)
                .context("companion deadline overflow")?;
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

    #[test]
    fn temporary_drake_is_invisible_until_commit_damages_without_player_credit_and_expires() {
        let (mut w, id) = fixture(0xaa9);
        let enemy = target(&mut w, id);
        let plan = stage(&mut w, id, 5);
        let entity = plan.companion.as_ref().unwrap().entity.unwrap();
        assert!(!w.entities[&entity].published && w.entities[&entity].reserved);
        assert!(
            w.use_item(
                id,
                Slot {
                    object: id,
                    slot: 5,
                    kind: 0xaa9
                },
                w.players[&id].pos
            )
            .is_err()
        );
        w.now += 40000;
        w.companion_step().unwrap();
        assert_eq!(w.entities[&enemy].hp, 1000);
        commit(&mut w, plan);
        assert_eq!(w.players[&id].character.items[5], -1);
        assert!(w.entities[&entity].published && !w.entities[&entity].reserved);
        w.now += 50;
        w.companion_step().unwrap();
        assert_eq!(w.entities[&enemy].hp, 965);
        assert!(w.entities[&enemy].damage.is_empty());
        assert_eq!(
            (
                w.players[&id].character.shots,
                w.players[&id].character.hits
            ),
            (0, 0)
        );
        w.now += 30000;
        w.companion_step().unwrap();
        assert!(!w.entities.contains_key(&entity));
        assert!(!w.companions.active.contains_key(&entity));
    }

    #[test]
    fn failed_egg_save_and_companion_limits_preserve_items_without_hidden_reservations() {
        for failed_write in [true, false] {
            let (mut w, id) = fixture(0xaa9);
            let plan = stage(&mut w, id, 5);
            let entity = plan.companion.as_ref().unwrap().entity.unwrap();
            let result = if failed_write {
                Err(anyhow::anyhow!("injected egg save failure"))
            } else {
                Ok(plan.revision + 2)
            };
            assert!(w.finish_inventory(plan, result).is_err());
            assert!(w.players[&id].persistence_failure.is_some());
            assert_eq!(w.players[&id].character.items[5], 0xaa9);
            assert!(!w.entities.contains_key(&entity) && w.companions.active.is_empty());
        }
        let (mut w, id) = fixture(0xaa9);
        for _ in 0..8 {
            w.players.get_mut(&id).unwrap().character.items[5] = 0xaa9;
            let plan = stage(&mut w, id, 5);
            commit(&mut w, plan);
        }
        w.players.get_mut(&id).unwrap().character.items[5] = 0xaa9;
        assert!(
            w.use_item(
                id,
                Slot {
                    object: id,
                    slot: 5,
                    kind: 0xaa9
                },
                w.players[&id].pos
            )
            .is_err()
        );
        assert_eq!(w.players[&id].character.items[5], 0xaa9);
        assert!(!w.players[&id].pending);
        assert_eq!(w.companions.active.len(), 8);
    }

    #[test]
    fn each_drake_color_executes_its_selected_healing_or_control_profile() {
        for (item, effect) in [
            (0xaf0, crate::condition::PARALYZED),
            (0xaf1, crate::condition::STASIS),
            (0xaf2, crate::condition::SLOWED),
        ] {
            let (mut w, id) = fixture(item);
            let enemy = target(&mut w, id);
            let plan = stage(&mut w, id, 5);
            commit(&mut w, plan);
            w.now += 50;
            w.companion_step().unwrap();
            assert_ne!(
                w.entities[&enemy].conditions(w.now) & crate::condition::bit(effect),
                0
            );
        }
        let (mut w, id) = fixture(0xaef);
        w.players.get_mut(&id).unwrap().character.hp = 10;
        let plan = stage(&mut w, id, 5);
        commit(&mut w, plan);
        let entity = *w.companions.active.keys().next().unwrap();
        assert_eq!(w.companions.active[&entity].until, Some(w.now + 60000));
        w.now += 50;
        w.companion_step().unwrap();
        assert_eq!(w.players[&id].character.hp, 35);
        w.apply_condition(id, crate::condition::SICK, 10000)
            .unwrap();
        w.now += 500;
        w.companion_step().unwrap();
        assert_eq!(w.players[&id].character.hp, 35);
    }

    #[test]
    fn yellow_drake_bleeding_is_nonstacking_and_completes_after_the_companion_leaves() {
        let (mut w, id) = fixture(0xaf3);
        let enemy = target(&mut w, id);
        let plan = stage(&mut w, id, 5);
        commit(&mut w, plan);
        w.now += 50;
        w.companion_step().unwrap();
        assert_ne!(
            w.entities[&enemy].conditions(w.now)
                & crate::condition::bit(crate::condition::BLEEDING),
            0
        );
        w.clear_companions(id);
        for _ in 0..10 {
            w.now += 1000;
            w.companion_step().unwrap();
        }
        assert_eq!(w.entities[&enemy].hp, 700);
        assert!(w.entities[&enemy].damage.is_empty());
        w.now += 50;
        w.companion_step().unwrap();
        assert!(w.companions.bleeds.is_empty());
    }

    #[test]
    fn zero_hp_companion_motion_reaches_native_ticks() {
        let (mut w, id) = fixture(0x1513);
        let (packets, mut rx) = tokio::sync::mpsc::channel(32);
        let (lost, _) = tokio::sync::watch::channel(false);
        w.players.get_mut(&id).unwrap().tx = Some(Output { packets, lost });
        let plan = stage(&mut w, id, 5);
        commit(&mut w, plan);
        let entity = *w.companions.active.keys().next().unwrap();
        let origin = w.entities[&entity].pos;
        assert_eq!(w.entities[&entity].hp, 0);
        assert!(w.entities[&entity].items.is_empty());
        w.updates(id, false).unwrap();
        assert!(w.players[&id].known.contains(&entity));
        while rx.try_recv().is_ok() {}
        w.players.get_mut(&id).unwrap().pos.x += 3.;
        w.step();
        w.step();
        assert!(w.entities[&entity].pos.distance(origin) > 0.1);
        let expected = w.stats(entity, id).unwrap();
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .filter(|packet| packet.id == wire::NEW_TICK)
                .any(|packet| packet
                    .body
                    .windows(expected.len())
                    .any(|body| body == expected))
        );
    }

    #[test]
    fn generators_replace_toggle_and_follow_owners_without_consumption_or_surviving_death() {
        let (mut w, id) = fixture(0x1513);
        w.players.get_mut(&id).unwrap().character.items[6] = 0x151b;
        w.players.get_mut(&id).unwrap().character.items[7] = 0x151e;
        for (slot, kind) in [(5, 0x1613), (6, 0x161b), (7, 0x161e)] {
            let items = w.players[&id].character.items.clone();
            let plan = stage(&mut w, id, slot);
            commit(&mut w, plan);
            assert_eq!(w.players[&id].character.items, items);
            assert_eq!(w.players[&id].character.pet_kind, Some(kind));
            assert_eq!(w.companions.active.len(), 1);
            let entity = *w.companions.active.keys().next().unwrap();
            assert_eq!(w.entities[&entity].kind, kind);
            assert!(
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot,
                        kind: items[slot]
                    },
                    w.players[&id].pos
                )
                .is_err()
            );
            w.now += 10000;
        }
        let entity = *w.companions.active.keys().next().unwrap();
        let old = w.players[&id].pos;
        w.players.get_mut(&id).unwrap().pos = Pos {
            x: old.x + 3.,
            y: old.y,
        };
        for _ in 0..12 {
            w.now += 50;
            w.companion_step().unwrap();
        }
        assert!(w.entities[&entity].pos.distance(w.players[&id].pos) <= 1.01);
        let plan = stage(&mut w, id, 7);
        commit(&mut w, plan);
        assert_eq!(w.players[&id].character.pet_kind, None);
        assert!(w.companions.active.is_empty());
        w.now += 10000;
        let plan = stage(&mut w, id, 5);
        commit(&mut w, plan);
        w.players.get_mut(&id).unwrap().character.hp = 0;
        w.players.get_mut(&id).unwrap().death_pending = true;
        assert_eq!(w.pending_deaths().len(), 1);
        w.finish_death(id).unwrap();
        assert!(w.companions.active.is_empty());
    }

    #[tokio::test]
    async fn generator_selection_is_atomic_restartable_and_failed_save_does_not_summon() {
        for fail in [false, true] {
            let (mut w, _) = fixture(0x1513);
            w.players.clear();
            let path = std::env::temp_dir().join(format!(
                "openrotmg-companion-{:016x}.db",
                rand::random::<u64>()
            ));
            let store = Store::open(&path, &w.content).unwrap();
            let account = store.list("prepared-companion".into()).await.unwrap().id;
            let mut ch = store
                .create(account, w.content.get(0x030e).unwrap())
                .await
                .unwrap();
            ch.items[5] = 0x1513;
            ch.revision = store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    None,
                )
                .await
                .unwrap()[0];
            let character = ch.id;
            let id = w.enter(ch, None).unwrap();
            let plan = stage(&mut w, id, 5);
            let selection = plan.companion.as_ref().unwrap().selected;
            if fail {
                rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER fail_pet BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'injected companion save failure'); END;").unwrap();
            }
            let write = InventoryWrite {
                id: character,
                account,
                revision: plan.revision,
                items: plan.items.clone(),
            };
            let result = store
                .select_companion(write.clone(), 5, 0x1513, selection, w.content.clone())
                .await;
            assert_eq!(result.is_err(), fail);
            assert_eq!(w.finish_inventory(plan, result).is_err(), fail);
            if !fail {
                assert!(
                    store
                        .select_companion(write, 5, 0x1513, selection, w.content.clone())
                        .await
                        .is_err()
                );
            }
            drop(store);
            let restarted = Store::open(&path, &w.content).unwrap();
            let saved = restarted.load(account, character).await.unwrap();
            assert_eq!(saved.items[5], 0x1513);
            if fail {
                assert_eq!(saved.pet_kind, None);
                assert!(w.companions.active.is_empty());
            } else {
                assert_eq!(saved.pet_kind, Some(0x1613));
                let mut next = fixtures::world();
                let next_id = next.enter(saved, None).unwrap();
                assert_eq!(next.companions.active.len(), 1);
                assert_eq!(next.players[&next_id].character.pet_kind, Some(0x1613));
                let saved = next.leave(next_id).unwrap();
                assert_eq!(saved.pet_kind, Some(0x1613));
                assert!(next.companions.active.is_empty());
            }
        }
    }
}
