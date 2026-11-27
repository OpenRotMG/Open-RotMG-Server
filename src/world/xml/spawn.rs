use super::*;

pub(super) struct Birth {
    pub game: Arc<crate::content::game::GameData>,
    pub source: crate::content::game::Source,
    pub at: u64,
    pub id: i32,
    pub identity: Identity,
    pub kind: u16,
    pub pos: Pos,
}
/// Release 7.0 behavior.cc MaintainProtectorsBehavior: slot i lives at ChildPos(i) and its last
/// child's damage record becomes that slot's corpse ledger when every protector is dead.
pub(super) struct Protectors {
    behavior: Behavior,
    slots: Vec<i32>,
    records: Vec<BTreeMap<i32, i32>>,
    armed: Option<u64>,
    rng: rand::rngs::StdRng,
}
fn slot_pos(b: &Behavior, host: Pos, slot: usize) -> Pos {
    let count = n(b, F::NumChildren);
    point(
        host,
        std::f64::consts::TAU * slot as f64 / count,
        n(b, F::Distance),
    )
}
fn kinds(b: &Behavior) -> &[u16] {
    if !objects(b, F::GroupId).is_empty() {
        objects(b, F::GroupId)
    } else if !objects(b, F::ChildIds).is_empty() {
        objects(b, F::ChildIds)
    } else {
        objects(b, F::ChildId)
    }
}
impl World {
    fn xml_child_count(&self, parent: i32, kinds: &[u16]) -> usize {
        self.child_entities(parent)
            .filter(|e| {
                kinds.contains(&e.kind) && (e.hp > 0 || self.content.objects[&e.kind].hp == 0)
            })
            .count()
            + self
                .xml
                .births
                .iter()
                .filter(|b| b.identity.parent == Some(parent) && kinds.contains(&b.kind))
                .count()
    }
    pub(super) fn xml_owned_count(&self, parent: i32) -> usize {
        self.child_entities(parent).count()
            + self
                .xml
                .births
                .iter()
                .filter(|b| b.identity.parent == Some(parent))
                .count()
    }
    pub(super) fn xml_spawn_ready(&self, b: &Behavior, step: Step) -> Result<bool> {
        let parent = self.xml_scope(step.token.scope)?.id;
        let kinds = kinds(b);
        ensure!(!kinds.is_empty(), "spawn lacks linked child/group kinds");
        for kind in kinds {
            self.xml_preflight_kind(*kind)?;
        }
        if b.kind == K::ObjectToss && targeted(b) && self.xml_attack_target(b, step)?.is_none() {
            return Ok(false);
        }
        if b.kind == K::MaintainProtectors {
            return Ok(!self.xml.protectors.contains_key(&step.token));
        }
        let max = if b.kind == K::MakeMinions {
            n(b, F::MaxChildren) as usize
        } else {
            CHILD_LIMIT
        };
        Ok(self.xml_child_count(parent, kinds) < max
            && self.xml_live_count() < ACTOR_LIMIT
            && self.xml.births.len() < CHILD_LIMIT)
    }
    pub(super) fn xml_spawn(
        &mut self,
        b: &Behavior,
        step: Step,
        rng: &mut rand::rngs::StdRng,
    ) -> Result<ActionOutcome> {
        let parent = step.token.scope.identity.actor;
        let pos = self.xml_scope(step.token.scope)?.pos;
        let kinds = kinds(b);
        ensure!(!kinds.is_empty(), "child operation lacks linked kinds");
        for kind in kinds {
            self.xml_preflight_kind(*kind)?;
        }
        if b.kind == K::MaintainProtectors {
            return self.xml_construct_protectors(b, step, rng);
        }
        let owned = self.xml_child_count(parent, kinds);
        let count = match b.kind {
            K::MakeMinions => {
                let remaining = (n(b, F::MaxChildren) as usize).saturating_sub(owned);
                if step.emissions == 0 && flag(b, F::StartWithMinions) {
                    remaining
                } else {
                    remaining.min(1)
                }
            }
            K::Shoot => n(b, F::NumShots) as usize,
            _ => 1,
        };
        if count == 0
            || self.xml_owned_count(parent) + count > CHILD_LIMIT
            || self.xml_live_count() + count > ACTOR_LIMIT
            || self.xml.births.len() + count > CHILD_LIMIT
        {
            return Ok(ActionOutcome::Blocked);
        }
        let landing = self
            .now
            .checked_add(1500)
            .context("ObjectToss deadline overflow")?;
        let mut draw = rng.clone();
        let mut births = vec![];
        for i in 0..count {
            let kind = kinds[draw.gen_range(0..kinds.len())];
            let mut p = if b.kind == K::ObjectToss {
                let target = self.xml_attack_target(b, step)?;
                if targeted(b) {
                    let Some((_, p)) = target else {
                        return Ok(ActionOutcome::Blocked);
                    };
                    p
                } else {
                    let jitter = n(b, F::RangeJitter);
                    let distance = n(b, F::Range)
                        + if jitter > 0. {
                            draw.gen_range(-jitter..=jitter)
                        } else {
                            0.
                        };
                    ensure!(distance >= 0., "negative ObjectToss distance");
                    point(pos, n(b, F::DefaultAngle), distance)
                }
            } else if b.kind == K::Shoot {
                let target = self.xml_attack_target(b, step)?;
                let angle = if targeted(b) {
                    let Some((_, p)) = target else {
                        return Ok(ActionOutcome::Blocked);
                    };
                    heading(pos, p)
                } else {
                    n(b, F::DefaultAngle)
                };
                point(
                    pos,
                    angle + (i as f64 - (count - 1) as f64 / 2.) * n(b, F::Angle),
                    0.1,
                )
            } else {
                pos
            };
            if b.kind == K::ObjectToss && flag(b, F::CenterObject) {
                p = Pos {
                    x: p.x.floor() + 0.5,
                    y: p.y.floor() + 0.5,
                };
            }
            // Oryx 2's 10.8-tile Monstrosity Scarab tosses (actors.xml:4175) can land past a small map's edge: blocked like a wall.
            if b.kind == K::ObjectToss && self.map.cell(p.x, p.y).is_none() {
                return Ok(ActionOutcome::Blocked);
            }
            self.xml_preflight_admission(kind, p)?;
            if !self.xml_placeable(kind, p, &[], None)? {
                return Ok(ActionOutcome::Blocked);
            }
            births.push((kind, p));
        }
        if b.kind == K::ObjectToss {
            self.xml_publish_actor(parent)?;
        }
        let ids = self.xml_reserve_ids(births.len())?;
        for ((kind, p), id) in births.into_iter().zip(ids) {
            if b.kind == K::ObjectToss {
                self.xml.births.push(Birth {
                    game: self
                        .content
                        .game
                        .as_ref()
                        .expect("validated GameData")
                        .clone(),
                    source: b.source.clone(),
                    at: landing,
                    id,
                    identity: Identity {
                        world: self.xml.world,
                        actor: id,
                        parent: Some(parent),
                        encounter: step.token.scope.identity.encounter,
                    },
                    kind,
                    pos: p,
                });
                self.xml_effect_known(4, parent, p, Pos { x: 0., y: 0. }, 0xffffffff);
            } else {
                self.insert_entity_with_id(id, kind, p, vec![])
                    .expect("preflighted XML child");
                self.xml_own(id, Some(parent), step.token.scope.identity.encounter)
                    .expect("preflighted XML ownership");
            }
        }
        *rng = draw;
        Ok(ActionOutcome::Applied)
    }
    pub(super) fn xml_birth_deadlines(&mut self) -> Result<()> {
        for birth in self.xml.births.iter().filter(|b| b.at <= self.now) {
            (|| -> Result<()> {
                ensure!(
                    Arc::ptr_eq(&birth.game, self.content.game.as_ref().context("GameData absent")?),
                    "ObjectToss owning catalog changed"
                );
                ensure!(
                    birth.identity.world == self.xml.world
                        && birth.identity.actor == birth.id
                        && !self.entities.contains_key(&birth.id),
                    "ObjectToss reserved identity changed"
                );
                self.xml_preflight_admission(birth.kind, birth.pos)?;
                self.xml_placeable(birth.kind, birth.pos, &[], None)?;
                Ok(())
            })()
            .with_context(|| {
                format!(
                    "{}:{} object={} state={} ObjectToss reservation={} parent={:?} world={} kind={:#x}",
                    birth.source.file, birth.source.line, birth.source.object, birth.source.path,
                    birth.id, birth.identity.parent, self.xml.world, birth.kind
                )
            })?;
        }
        let mut index = 0;
        while index < self.xml.births.len() {
            let birth = &self.xml.births[index];
            if birth.at > self.now || !self.xml_placeable(birth.kind, birth.pos, &[], None)? {
                index += 1;
                continue;
            }
            let birth = self.xml.births.remove(index);
            self.insert_entity_with_id(birth.id, birth.kind, birth.pos, vec![])
                .expect("preflighted thrown birth");
            self.xml_own(birth.id, birth.identity.parent, birth.identity.encounter)
                .expect("reserved thrown ownership");
        }
        Ok(())
    }
    pub(in crate::world) fn xml_ordinary_footprint(
        &self,
        d: &crate::content::game::Definition,
    ) -> usize {
        super::interest::initial_states(d)
            .iter()
            .flat_map(|state| &d.states[*state as usize].behaviors)
            .map(|b| match b.kind {
                K::MakeMinions if flag(b, F::StartWithMinions) => n(b, F::MaxChildren) as usize,
                K::MaintainProtectors => n(b, F::NumChildren) as usize,
                _ => 0,
            })
            .sum()
    }
    /// Children that MaintainProtectors builds when its initial state is entered.
    pub(in crate::world) fn xml_entry_children(&self, kind: u16) -> usize {
        self.xml_game().definitions.get(&kind).map_or(0, |d| {
            super::interest::initial_states(d)
                .iter()
                .flat_map(|state| &d.states[*state as usize].behaviors)
                .filter(|b| b.kind == K::MaintainProtectors)
                .map(|b| n(b, F::NumChildren) as usize)
                .sum()
        })
    }
    fn xml_construct_protectors(
        &mut self,
        b: &Behavior,
        step: Step,
        rng: &mut rand::rngs::StdRng,
    ) -> Result<ActionOutcome> {
        let host = step.token.scope.identity.actor;
        let pos = self.xml_scope(step.token.scope)?.pos;
        let count = n(b, F::NumChildren) as usize;
        let mut draw = rng.clone();
        let mut births = vec![];
        for slot in 0..count {
            let kind = kinds(b)[draw.gen_range(0..kinds(b).len())];
            let p = slot_pos(b, pos, slot);
            self.xml_preflight_admission(kind, p)?;
            if !self.xml_placeable(kind, p, &[], None)? {
                tracing::warn!(
                    host,
                    slot,
                    kind,
                    x = p.x,
                    y = p.y,
                    "MaintainProtectors construction blocked; retrying"
                );
                return Ok(ActionOutcome::Blocked);
            }
            births.push((kind, p));
        }
        if self.xml_owned_count(host) + count > CHILD_LIMIT
            || self.xml_live_count() + count > ACTOR_LIMIT
        {
            tracing::warn!(
                host,
                count,
                live = self.xml_live_count(),
                "MaintainProtectors construction exceeds live bounds; retrying"
            );
            return Ok(ActionOutcome::Blocked);
        }
        let ids = self.xml_reserve_ids(count)?;
        for ((kind, p), id) in births.into_iter().zip(&ids) {
            self.insert_entity_with_id(*id, kind, p, vec![])
                .expect("preflighted protector");
            self.xml_own(*id, Some(host), step.token.scope.identity.encounter)
                .expect("preflighted protector ownership");
        }
        let protectors = Protectors {
            behavior: b.clone(),
            slots: ids,
            records: vec![BTreeMap::new(); count],
            armed: None,
            rng: rand::rngs::StdRng::seed_from_u64(draw.r#gen()),
        };
        self.xml.protectors.insert(step.token, protectors);
        *rng = draw;
        Ok(ActionOutcome::Applied)
    }
    fn xml_protector_alive(&self, id: i32) -> bool {
        self.entities
            .get(&id)
            .is_some_and(|e| !e.reserved && (e.hp > 0 || self.content.objects[&e.kind].hp == 0))
    }
    /// Captured at the child's defeat so a dormant host cannot miss the killing hit.
    pub(super) fn xml_protector_record(&mut self, child: &Entity) {
        for (_, p) in self
            .xml
            .protectors
            .iter_mut()
            .filter(|(token, _)| Some(token.scope.identity.actor) == *child.parent)
        {
            if let Some(slot) = p.slots.iter().position(|id| *id == child.id) {
                p.records[slot] = child.damage.clone();
            }
        }
    }
    /// behavior.cc:1724-1737: each live slot streams to a random other slot, yellow while cooling down, else red.
    fn xml_protector_streams(&mut self, token: Token, host: i32, pos: Pos, dead: &[usize]) {
        let p = self.xml.protectors.get_mut(&token).unwrap();
        let color: u32 = if p.armed.is_some() {
            0xFFFF00
        } else {
            0xFF0000
        };
        let mut streams = Vec::new();
        for i in (0..p.slots.len()).filter(|i| !dead.contains(i)) {
            let mut j = p.rng.gen_range(0..p.slots.len() - 1);
            if j >= i {
                j += 1;
            }
            streams.push((
                p.slots[i],
                slot_pos(&p.behavior, pos, i),
                slot_pos(&p.behavior, pos, j),
            ));
        }
        for (child, a, b) in streams {
            let mut out = Writer::new();
            out.u8(3)
                .i32(child)
                .f32(a.x)
                .f32(a.y)
                .f32(b.x)
                .f32(b.y)
                .u32(color);
            for player in self
                .players
                .values()
                .filter(|q| q.known.contains(&host) || q.known.contains(&child))
            {
                send(player, wire::SHOW_EFFECT, out.0.clone());
            }
        }
    }
    /// World-clock service: towers sit beyond the 20-tile dormancy radius of their host, so arming,
    /// respawn and completion must not wait for a player to approach the host.
    pub(super) fn xml_protector_deadlines(&mut self) -> Result<()> {
        for token in self.xml.protectors.keys().copied().collect::<Vec<_>>() {
            let host = token.scope.identity.actor;
            let p = &self.xml.protectors[&token];
            let dead: Vec<_> = (0..p.slots.len())
                .filter(|slot| !self.xml_protector_alive(p.slots[*slot]))
                .collect();
            let Some(&Value::Millis(cooldown)) = value(&p.behavior, F::Cooldown) else {
                bail!("unmaterialized MaintainProtectors cooldown host={host}")
            };
            let pos = self
                .entities
                .get(&host)
                .context("protector host absent")?
                .pos;
            if self.now.is_multiple_of(200) && p.slots.len() > 1 {
                self.xml_protector_streams(token, host, pos, &dead);
            }
            let p = &self.xml.protectors[&token];
            match p.armed {
                None if !dead.is_empty() => {
                    let at = self
                        .now
                        .checked_add(cooldown)
                        .context("protector deadline overflow")?;
                    tracing::info!(host, ?dead, respawn_at = at, "MaintainProtectors armed");
                    self.xml.protectors.get_mut(&token).unwrap().armed = Some(at);
                }
                Some(_) if dead.len() == p.slots.len() => {
                    let p = self.xml.protectors.remove(&token).unwrap();
                    // Authored, legacy-derived (base 79a4c92b realm_events.rs:571-584 added each tower ledger to the Pentaract
                    // root): a never-damaged host's quest participation is its protectors' recorded ledgers.
                    let ledger = &mut self
                        .entities
                        .get_mut(&host)
                        .context("protector host absent")?
                        .damage;
                    for (character, damage) in p.records.iter().flatten() {
                        let total = ledger.entry(*character).or_default();
                        *total = total
                            .checked_add(*damage)
                            .context("protector ledger overflow")?;
                    }
                    let mut corpses = vec![];
                    for (slot, record) in p.records.into_iter().enumerate() {
                        let Some(kind) = objects(&p.behavior, F::ChildCorpseId).first().copied()
                        else {
                            break;
                        };
                        let at = slot_pos(&p.behavior, pos, slot);
                        self.xml_preflight_admission(kind, at)?;
                        if !self.xml_placeable(kind, at, &p.slots, None)?
                            || self.xml_live_count() >= ACTOR_LIMIT
                        {
                            tracing::warn!(
                                host,
                                slot,
                                kind,
                                x = at.x,
                                y = at.y,
                                "protector corpse blocked; its loot ledger is not published"
                            );
                            continue;
                        }
                        let id = self.xml_reserve_ids(1)?[0];
                        self.insert_entity_with_id(id, kind, at, vec![])
                            .expect("preflighted protector corpse");
                        self.xml_own(id, Some(host), token.scope.identity.encounter)
                            .expect("preflighted protector corpse ownership");
                        self.entities.get_mut(&id).unwrap().damage = record;
                        corpses.push(id);
                    }
                    tracing::info!(host, ?corpses, "all protectors dead; host defeated");
                    if let Err(error) = self.xml_defeated(host, TerminalContext::NonPlayer) {
                        if !self.xml.retry_terminals.contains(&host) {
                            return Err(error);
                        }
                        super::terminal::log_pending(
                            host,
                            &error,
                            "XML protector host defeat pending",
                        );
                    }
                }
                Some(at) if at <= self.now => {
                    let mut p = self.xml.protectors.remove(&token).unwrap();
                    p.armed = None;
                    for slot in dead {
                        let kinds = kinds(&p.behavior);
                        let kind = kinds[p.rng.gen_range(0..kinds.len())];
                        let at = slot_pos(&p.behavior, pos, slot);
                        self.xml_preflight_admission(kind, at)?;
                        if !self.xml_placeable(kind, at, &[], None)?
                            || self.xml_live_count() >= ACTOR_LIMIT
                            || self.xml_owned_count(host) >= CHILD_LIMIT
                        {
                            tracing::warn!(
                                host,
                                slot,
                                kind,
                                x = at.x,
                                y = at.y,
                                "protector respawn blocked; slot stays dead"
                            );
                            continue;
                        }
                        let id = self.xml_reserve_ids(1)?[0];
                        self.insert_entity_with_id(id, kind, at, vec![])
                            .expect("preflighted protector respawn");
                        self.xml_own(id, Some(host), token.scope.identity.encounter)
                            .expect("preflighted protector respawn ownership");
                        p.slots[slot] = id;
                        p.records[slot].clear();
                    }
                    tracing::info!(host, slots = ?p.slots, "MaintainProtectors respawned dead slots");
                    self.xml.protectors.insert(token, p);
                }
                _ => {}
            }
        }
        Ok(())
    }
}
