use super::*;

// Population dependence is period-documented; the empty spare, bounds/300s grace are reconstruction policy:
// https://github.com/OpenRotMG/Open-RotMG-Server/issues/1344
pub(crate) const MAX_CURRENT: usize = 8;
const IDLE_GRACE: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(super) struct Population {
    preparing: Option<(i32, oneshot::Receiver<Result<World>>)>,
    retry_at: Option<Instant>,
    bound_reported: Option<&'static str>,
}

pub(crate) struct InitialEntryHold {
    realm: i32,
    count: Arc<std::sync::atomic::AtomicUsize>,
}
impl Drop for InitialEntryHold {
    fn drop(&mut self) {
        let previous = self.count.fetch_sub(1, Ordering::AcqRel);
        assert!(previous > 0, "initial Realm entry hold released twice");
        tracing::info!(
            realm = self.realm,
            holds = previous - 1,
            "initial Realm entry retirement hold released"
        );
    }
}

impl Directory {
    // Closed predecessor identities must survive durable handoff without pinning invisible entrance resources.
    pub(super) fn realm_entrance_reserves_position(&self, portal: i32) -> Result<bool> {
        let entity = self
            .worlds
            .get(&-2)
            .context("Nexus missing")?
            .entities
            .get(&portal)
            .context("Realm entrance entity missing")?;
        ensure!(entity.kind == 0x712, "Realm entrance kind changed");
        let mut bindings = self
            .endgame
            .realms
            .iter()
            .filter(|(_, r)| r.portal == Some(portal));
        let Some((origin, record)) = bindings.next() else {
            return Ok(true);
        };
        ensure!(
            bindings.next().is_none(),
            "Realm entrance has multiple bindings"
        );
        let world = self
            .worlds
            .get(origin)
            .context("bound Realm world missing")?;
        Ok(!(record.enabled
            && !record.cycle.admits()
            && !world.admission_open
            && !entity.published))
    }
    pub(super) fn unused_realm_name(&self, predecessor: Option<i32>) -> Result<usize> {
        let old = predecessor
            .map(|id| {
                self.endgame
                    .realms
                    .get(&id)
                    .map(|r| r.name)
                    .context("Realm predecessor missing")
            })
            .transpose()?;
        for offset in 0..super::realm_portal::NAMES.len() {
            let name = match old {
                Some(old) => (old + 1 + offset) % super::realm_portal::NAMES.len(),
                None => offset,
            };
            if Some(name) == old {
                continue;
            }
            let mut reserved = false;
            for record in self.endgame.realms.values().filter(|r| r.name == name) {
                let portal = record.portal.context("named Realm has no bound entrance")?;
                reserved |= self.realm_entrance_reserves_position(portal)?;
            }
            if !reserved {
                return Ok(name);
            }
        }
        bail!("no unused Realm name")
    }
    pub(crate) fn hold_initial_entry(&mut self, world: i32) -> Result<Option<InitialEntryHold>> {
        ensure!(
            self.worlds.contains_key(&world),
            "initial entry world missing"
        );
        let Some(record) = self.endgame.realms.get_mut(&world) else {
            return Ok(None);
        };
        let previous = record
            .initial_entries
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| anyhow::anyhow!("initial Realm entry hold count exhausted"))?;
        record.empty_since = None;
        tracing::info!(
            realm = world,
            holds = previous + 1,
            "initial Realm entry retirement hold acquired"
        );
        Ok(Some(InitialEntryHold {
            realm: world,
            count: record.initial_entries.clone(),
        }))
    }
    fn current_realm_count(&self) -> usize {
        self.endgame
            .realms
            .values()
            .filter(|r| r.replacement.is_none())
            .count()
    }
    fn realm_dependencies(&self, id: i32) -> usize {
        self.grants
            .values()
            .filter(|g| g.origin == Some(id) || g.destination == id)
            .count()
            + self
                .portals
                .values()
                .filter(|p| p.origin == id || p.destination == Some(id))
                .count()
            + self
                .worlds
                .summaries()
                .filter(|(_, w)| w.realm_return == Some(id))
                .count()
            + self.endgame.population_references(id)
            + self
                .endgame
                .realms
                .get(&id)
                .map_or(0, |r| r.initial_entries.load(Ordering::Acquire))
    }
    fn log_realm_population(&self, realm: i32, reason: &'static str, decision: &'static str) {
        let physical = self.endgame.realms.len();
        let current = self.current_realm_count();
        let open = self
            .endgame
            .realms
            .iter()
            .filter(|(id, r)| {
                r.enabled
                    && self
                        .worlds
                        .summary(id)
                        .expect("Realm metadata missing")
                        .admission_open
            })
            .count();
        let (players, reservations) = self.endgame.realms.keys().fold((0, 0), |(p, r), id| {
            let w = self.worlds.summary(id).expect("Realm metadata missing");
            (
                p + w.players.len(),
                r + w.occupancy.saturating_sub(w.players.len()),
            )
        });
        tracing::info!(
            realm,
            physical,
            current,
            open,
            retiring = physical - current,
            allocating = self.realm_population.preparing.is_some(),
            players,
            reservations,
            empty_spares = self
                .available_realms()
                .iter()
                .filter(|(_, n)| *n == 0)
                .count(),
            dependencies = self.realm_dependencies(realm),
            reason,
            decision,
            "Realm population decision"
        );
    }
    pub(super) fn realm_population_step(&mut self, now: Instant) {
        let result = self.update_realm_population(now);
        if let Err(error) = result {
            self.realm_population.retry_at = Some(now + Duration::from_secs(5));
            tracing::error!(%error, "Realm population allocation failed; retry in five seconds");
        }
    }
    fn update_realm_population(&mut self, now: Instant) -> Result<()> {
        if let Some((id, mut receiver)) = self.realm_population.preparing.take() {
            match receiver.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => {
                    self.realm_population.preparing = Some((id, receiver));
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    bail!("Realm allocation worker stopped without a result")
                }
                Ok(result) => {
                    let world = result?;
                    ensure!(
                        self.current_realm_count() < MAX_CURRENT,
                        "Realm allocation lost its current slot"
                    );
                    let slot = (0..MAX_CURRENT)
                        .find(|slot| {
                            self.endgame
                                .realms
                                .values()
                                .all(|r| r.replacement.is_some() || r.portal_slot != *slot)
                        })
                        .context("no unused current Realm entrance slot")?;
                    let name = self.unused_realm_name(None)?;
                    let mut record = endgame::RealmRecord::new(
                        crate::endgame::RealmCycle::new(world.hero_identities())?,
                        self.tick * 50,
                        now,
                    );
                    record.portal = Some(self.install_realm_portal(slot)?);
                    record.portal_slot = slot;
                    record.name = name;
                    self.worlds.insert(id, world);
                    self.endgame.realms.insert(id, record);
                    self.log_realm_population(id, "published", "fresh Realm published");
                }
            }
        }
        let mut retiring = Vec::new();
        for id in self.endgame.realms.keys().copied().collect::<Vec<_>>() {
            let world = self.worlds.summary(&id).context("Realm metadata missing")?;
            let record = &self.endgame.realms[&id];
            let eligible = record.enabled
                && record.replacement.is_none()
                && record.cycle.admits()
                && world.admission_open
                && world.players.is_empty()
                && world.admissions_empty
                && world.bags_empty
                && self.realm_dependencies(id) == 0;
            let record = self.endgame.realms.get_mut(&id).unwrap();
            if eligible {
                let since = *record.empty_since.get_or_insert(now);
                if now.duration_since(since) >= IDLE_GRACE
                    && now.duration_since(record.created_at) >= IDLE_GRACE
                {
                    retiring.push(id);
                }
            } else {
                record.empty_since = None;
            }
        }
        for id in retiring {
            if !self
                .available_realms()
                .iter()
                .any(|(other, occupancy)| *other != id && *occupancy == 0)
            {
                break;
            }
            // Take an owned snapshot again before removal; pending world work can have changed the published census.
            self.worlds.get(&id).context("retiring Realm missing")?;
            let world = self.worlds.summary(&id).unwrap();
            if !world.players.is_empty()
                || !world.admissions_empty
                || !world.bags_empty
                || self.realm_dependencies(id) != 0
            {
                self.endgame.realms.get_mut(&id).unwrap().empty_since = None;
                self.worlds.release_world(&id);
                continue;
            }
            self.log_realm_population(id, "surplus", "300s empty dependency-free surplus retired");
            let portal = self.endgame.realms[&id]
                .portal
                .context("retiring Realm has no entrance")?;
            self.worlds
                .get_mut(&-2)
                .context("Nexus missing")?
                .remove_entity(portal);
            self.worlds.remove(&id);
            self.endgame.realms.remove(&id);
            self.idle.remove(&id);
        }
        let available = self.available_realms();
        if available.iter().any(|(_, occupancy)| *occupancy == 0) {
            self.realm_population.bound_reported = None;
            return Ok(());
        }
        let reason = if !available.is_empty() {
            "empty_spare"
        } else if self.endgame.realms.iter().any(|(id, record)| {
            record.enabled
                && self
                    .worlds
                    .summary(id)
                    .expect("Realm metadata missing")
                    .admission_open
        }) {
            "hard_exhaustion"
        } else {
            "closed"
        };
        if self.realm_population.preparing.is_some()
            || self.realm_population.retry_at.is_some_and(|at| now < at)
        {
            return Ok(());
        }
        if self.current_realm_count() >= MAX_CURRENT {
            if self.realm_population.bound_reported != Some(reason) {
                self.log_realm_population(
                    0,
                    reason,
                    if reason == "empty_spare" {
                        "empty spare target unmet: eight current Realm slots; hard capacity remains available"
                    } else {
                        "allocation refused: eight current Realm slots; no unreserved open capacity"
                    },
                );
                self.realm_population.bound_reported = Some(reason);
            }
            return Ok(());
        }
        let id = self.next_world;
        let next = id.checked_add(1).context("world identity exhausted")?;
        ensure!(
            !self.worlds.contains_key(&id),
            "Realm allocation identity collision"
        );
        let build = self.construction("realm-shore", "Realm", false, None);
        let receiver = self.worlds.prepare_replacement(id, self.tick, build)?;
        self.next_world = next;
        self.realm_population.preparing = Some((id, receiver));
        self.log_realm_population(id, reason, "one Realm allocation started");
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn realm_population_grows_once_when_all_capacity_is_reserved() {
    let path = std::path::Path::new("data");
    let content = Arc::new(Content::load(path).unwrap());
    let mut d = Directory::test_with_seed(path, content, 1, 1344).unwrap();
    d.issue(1, 1, -3).unwrap();
    d.issue(2, 2, 1).unwrap();
    assert!(d.available_realms().is_empty());
    for _ in 0..3 {
        d.coordinator_step();
    }
    assert_eq!(d.endgame.realms.len(), 3);
    assert_eq!(d.available_realms().len(), 1);
    assert_eq!(d.worlds[&-3].occupancy(), 1);
    assert_eq!(d.worlds[&1].occupancy(), 1);
    for _ in 0..3 {
        d.coordinator_step();
    }
    assert_eq!(d.endgame.realms.len(), 3);
}

#[cfg(test)]
#[path = "realm_population_tests.rs"]
mod tests;
