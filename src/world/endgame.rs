use super::*;
use crate::content::game::SiteRole;
#[cfg(test)]
use crate::endgame::HEROES;
use crate::endgame::{CASTLE, RealmCycle, RealmStage};

// Gameplay documentation specifies two minutes, unlike modified forks' 20s: https://wikiwiki.jp/rmd/イベント#jded12e1
pub(super) const REMAINING_TAUNT_MS: u64 = 120_000;

#[derive(Default)]
pub(super) struct RealmDirectory {
    pub realms: BTreeMap<i32, RealmRecord>,
    pub(super) forced: BTreeMap<i32, ForcedRoute>,
    preparing: Option<RealmPreparation>,
}
struct RealmPreparation {
    origin: i32,
    castle: i32,
    realm: i32,
    rng_before: rand::rngs::StdRng,
    realm_started: Arc<std::sync::atomic::AtomicBool>,
    result: tokio::sync::oneshot::Receiver<Result<(World, World, RealmCycle)>>,
}
pub(super) struct RealmRecord {
    pub cycle: RealmCycle,
    pub portal: Option<i32>,
    pub portal_slot: usize,
    pub name: usize,
    pub created_at: Instant,
    pub empty_since: Option<Instant>,
    pub initial_entries: Arc<std::sync::atomic::AtomicUsize>,
    pub enabled: bool,
    pub(super) replacement: Option<i32>,
    pub castle: Option<i32>,
    pub pending_transfers: BTreeSet<i32>,
    publish_at: Option<u64>,
    retry_at: u64,
    next_taunt: u64,
}
impl RealmRecord {
    pub(super) fn new(cycle: RealmCycle, now: u64, created_at: Instant) -> Self {
        Self {
            cycle,
            portal: None,
            portal_slot: 0,
            name: 0,
            created_at,
            empty_since: None,
            initial_entries: Arc::default(),
            enabled: true,
            replacement: None,
            castle: None,
            pending_transfers: BTreeSet::new(),
            publish_at: None,
            retry_at: 0,
            next_taunt: now + REMAINING_TAUNT_MS,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct ForcedRoute {
    pub origin: i32,
    destination: i32,
    select_when_paused: bool,
    frozen: bool,
}
impl RealmDirectory {
    pub(super) fn population_references(&self, id: i32) -> usize {
        self.realms
            .values()
            .filter(|r| r.replacement == Some(id) || r.castle == Some(id))
            .count()
            + self
                .forced
                .values()
                .filter(|r| r.origin == id || r.destination == id)
                .count()
            + usize::from(
                self.preparing
                    .as_ref()
                    .is_some_and(|p| p.origin == id || p.realm == id || p.castle == id),
            )
    }
    pub(super) fn references_world(&self, id: i32) -> bool {
        self.realms.contains_key(&id)
            || self
                .realms
                .values()
                .any(|r| r.castle == Some(id) && !r.pending_transfers.is_empty())
            || self
                .forced
                .values()
                .any(|r| r.origin == id || r.destination == id)
    }
}
pub struct ForcedTransfer {
    pub player: i32,
    pub output: Output,
    pub destination: Option<i32>,
    pub key_time: i32,
    pub key: Vec<u8>,
    pub name: String,
}
impl ForcedTransfer {
    pub fn packet(&self) -> Packet {
        let mut o = Writer::new();
        let Some(destination) = self.destination else {
            // Selected 123.5.1 FAILURE 5 completes the scene without reconnecting a paused member.
            o.i32(5)
                .utf("Realm closed while you were paused. Select your character to continue.");
            return Packet { id: 0, body: o.0 };
        };
        o.utf(&self.name)
            .utf("")
            .i32(2050)
            .i32(destination)
            .i32(self.key_time)
            .i16(self.key.len() as i16);
        o.0.extend_from_slice(&self.key);
        Packet { id: 39, body: o.0 }
    }
}
fn seal_realm(world: &mut World, grants: &mut BTreeMap<Vec<u8>, Grant>, origin: i32) {
    world.admission_open = false;
    world.oryx_notice("I HAVE CLOSED THIS REALM! YOU WILL NOT LIVE TO SEE THE LIGHT OF DAY!");
    grants.retain(|_, g| g.destination != origin);
    world.admissions.clear();
}
impl Directory {
    /// Starts the ordinary closure countdown for the Realm a player is standing in.
    pub(crate) fn force_realm_closure(&mut self, player: i32) -> Result<String> {
        let origin = self.location(player)?;
        let now = self.tick * 50;
        let record = self
            .endgame
            .realms
            .get_mut(&origin)
            .filter(|record| record.enabled)
            .context("You must be inside a Realm to close it.")?;
        ensure!(record.cycle.admits(), "This Realm is already closing.");
        record.cycle.remaining.clear();
        record.cycle.stage = RealmStage::Closing {
            deadline: now + crate::endgame::CLOSE_DELAY_MS,
        };
        let world = self
            .worlds
            .get_mut(&origin)
            .context("Realm world missing")?;
        let name = world.players[&player].character.name.clone();
        world.broadcast_text("", &format!("{name} used a Realm Close token!"));
        seal_realm(world, &mut self.grants, origin);
        tracing::info!(origin, player, %name, "Realm closure started by a Discord token");
        Ok(name)
    }
    pub(super) fn check_portal_catalog(&self, origin: i32) -> Result<()> {
        let world = self.worlds.get(&origin).context("portal origin missing")?;
        world.xml_check_catalog()?;
        ensure!(
            Arc::ptr_eq(
                world
                    .content
                    .game
                    .as_ref()
                    .context("portal origin catalog absent")?,
                self.content
                    .game
                    .as_ref()
                    .context("Directory catalog absent")?
            ),
            "portal Directory owning catalog changed"
        );
        Ok(())
    }
    pub fn stage_unlock(&mut self, plan: InventoryPlan) -> Result<InventoryPlan> {
        let Some(portal) = plan.unlock else {
            return Ok(plan);
        };
        let PortalPlan { locked, opened, .. } = portal;
        ensure!(!self.portals.contains_key(&opened), "portal already staged");
        let origin = self.location(plan.player)?;
        let w = &self.worlds[&origin];
        let template = match self
            .check_portal_catalog(origin)
            .and_then(|_| w.validate_portal_inventory_at(&plan, portal, None))
        {
            Ok(template) => template.to_owned(),
            Err(error) => {
                let w = self.worlds.get_mut(&origin).unwrap();
                w.discard_unlock(locked, opened)?;
                w.players
                    .get_mut(&plan.player)
                    .context("portal owner disappeared")?
                    .pending = false;
                return Err(error);
            }
        };
        let p = &w.players[&plan.player];
        ensure!(
            p.pending
                && p.character.revision == plan.revision
                && p.character.account == plan.account
                && p.character.id == plan.character,
            "portal owner reservation changed"
        );
        let kind = w
            .entities
            .get(&opened)
            .context("staged portal missing")?
            .kind;
        let expires = portal
            .lifetime()
            .map(|lifetime| {
                self.tick
                    .checked_add(lifetime)
                    .context("portal binding deadline overflow")
            })
            .transpose()?;
        let destination = match self
            .dungeon_return_from(origin)
            .and_then(|route| self.instance(&template, None, Some(route)))
        {
            Ok(destination) => destination,
            Err(error) => {
                let w = self.worlds.get_mut(&origin).unwrap();
                w.discard_unlock(locked, opened)?;
                w.players.get_mut(&plan.player).unwrap().pending = false;
                return Err(error);
            }
        };
        self.portals.insert(
            opened,
            Portal {
                origin,
                destination: Some(destination),
                expires,
            },
        );
        tracing::info!(
            player = plan.player,
            opened,
            destination,
            template,
            origin,
            kind,
            "portal binding staged; consumable not committed or portal published yet"
        );
        Ok(plan)
    }
    #[cfg(test)]
    pub fn finish_inventory(&mut self, plan: InventoryPlan, result: Result<i32>) -> Result<()> {
        self.finish_inventory_receipt(plan, result, None)
    }
    fn finish_inventory_receipt(
        &mut self,
        mut plan: InventoryPlan,
        result: Result<i32>,
        receipt: Option<&crate::store::world_events::Event>,
    ) -> Result<()> {
        let Some(portal) = plan.unlock.take() else {
            return self
                .resolve(Target::Player(plan.player))?
                .finish_inventory(plan, result);
        };
        let PortalPlan { locked, opened, .. } = portal;
        let player = plan.player;
        let origin = self.location(player)?;
        self.check_portal_catalog(origin)?;
        let w = &self.worlds[&origin];
        let p = &w.players[&player];
        ensure!(
            p.pending
                && p.character.revision == plan.revision
                && p.character.account == plan.account
                && p.character.id == plan.character,
            "stale or foreign portal completion"
        );
        ensure!(
            locked.is_none_or(|id| {
                w.unlock_rule().is_some_and(|rule| {
                    w.entities
                        .get(&id)
                        .is_some_and(|e| e.reserved && e.kind == rule.locked)
                })
            }) && w
                .entities
                .get(&opened)
                .is_some_and(|e| e.reserved && !e.published),
            "portal reservation lost or already published"
        );
        let binding = self
            .portals
            .get(&opened)
            .context("staged portal binding lost")?;
        ensure!(binding.origin == origin, "staged portal origin changed");
        let destination = binding.destination;
        if result.is_err()
            || receipt.is_none()
                && portal.locked.is_some_and(|id| {
                    w.xml_terminal_pending(id)
                        || w.xml_portal_expiry(id).is_some_and(|at| at <= w.now)
                })
        {
            let outcome = if result.is_err() {
                "unknown_commit"
            } else {
                "committed_after_source_expiry"
            };
            let evidence = format!(
                "phase4 portal inventory reconciliation required: outcome={outcome} account={} character={} revision={} committed_revision={:?} world={} source_slot={} source_item={} locked={:?} opened={} deadline={:?} now={} input={}; reservations retained; original full inventory event and outcome require shared durable reconciliation",
                plan.account,
                plan.character,
                plan.revision,
                result.as_ref().ok(),
                portal.world,
                portal.source.slot,
                portal.source.kind,
                portal.locked,
                opened,
                portal.locked.and_then(|id| w.xml_portal_expiry(id)),
                w.now,
                w.xml_game().fingerprint
            );
            tracing::error!(account=plan.account,character=plan.character,revision=plan.revision,world=portal.world,locked=?portal.locked,opened,source_slot=portal.source.slot,source_item=portal.source.kind,deadline=?portal.locked.and_then(|id|w.xml_portal_expiry(id)),now=w.now,input=%w.xml_game().fingerprint,outcome,%evidence,"portal completion quarantined; no refund, publication or retry authorization");
            self.worlds
                .get_mut(&origin)
                .unwrap()
                .poison(player, evidence.clone())?;
            return Err(match result {
                Err(error) => error.context(evidence),
                Ok(_) => anyhow::anyhow!(evidence),
            });
        }
        let accepted = receipt
            .map(|event| -> Result<u64> {
                let payload = event.native_payload()?;
                let at = payload["origin"]["accepted_at"]
                    .as_u64()
                    .context("journal accepted time absent")?;
                if let Some(deadline) = payload["portal"]["locked_deadline"].as_u64() {
                    ensure!(
                        at < deadline,
                        "journal request was not accepted before source deadline"
                    );
                }
                Ok(at)
            })
            .transpose()?;
        w.validate_portal_inventory_at(&plan, portal, accepted)?;
        let expires = portal
            .lifetime()
            .map(|lifetime| {
                self.tick
                    .checked_add(lifetime)
                    .context("portal binding deadline overflow")
            })
            .transpose()?;
        self.worlds
            .get_mut(&origin)
            .unwrap()
            .finish_inventory(plan, result)?;
        let w = self.worlds.get_mut(&origin).unwrap();
        if let Some(locked) = locked {
            w.entities
                .get_mut(&locked)
                .context("committed locked portal lost")?
                .reserved = false;
            w.remove_entity(locked);
        }
        let e = w
            .entities
            .get_mut(&opened)
            .context("committed portal lost")?;
        e.reserved = false;
        e.published = true;
        let kind = e.kind;
        let dungeon = e.name.clone();
        w.xml_activate_portal(opened)?;
        self.portals.get_mut(&opened).unwrap().expires = expires;
        let name = &w.players[&player].character.name;
        if locked.is_some() {
            // Period unlock feedback belongs after commit, never reservation: https://www.youtube.com/watch?v=H44UToO5faY&t=9
            let mut note = Writer::new();
            note.i32(player)
                .utf(&format!("Unlocked by {name}"))
                .u32(0xff00ff00);
            for p in w
                .players
                .values()
                .filter(|p| !p.departing && p.persistence_failure.is_none())
            {
                send(p, wire::NOTIFICATION, note.0.clone());
            }
            w.broadcast_text("", &format!("{dungeon} unlocked by {name}"));
        } else {
            w.broadcast_text("", &format!("{dungeon} opened by {name}!"));
        }
        let p = &w.players[&player];
        tracing::info!(
            player,
            account = p.character.account,
            character = p.character.id,
            revision = p.character.revision,
            opened,
            kind,
            destination,
            origin,
            expires_tick = ?self.portals[&opened].expires,
            "consumable committed; bound dungeon portal published"
        );
        Ok(())
    }
    pub(crate) fn prepare_realm_closure(&mut self, origin: i32) -> Result<(i32, Pos)> {
        ensure!(
            self.prepared_seed.is_some(),
            "closure setup requires prepared state"
        );
        ensure!(self.clock.paused(), "closure setup requires a paused clock");
        ensure!(
            self.worlds
                .values()
                .all(|w| w.players.is_empty() && w.reserved_slots() == 0),
            "closure setup requires no enrolled players or reservations"
        );
        let record = self
            .endgame
            .realms
            .get_mut(&origin)
            .context("closure Realm missing")?;
        ensure!(
            record.cycle.admits() && record.enabled,
            "closure setup requires an open Realm"
        );
        let world = self
            .worlds
            .get_mut(&origin)
            .context("closure world missing")?;
        let hero = record
            .cycle
            .remaining
            .iter()
            .copied()
            .find(|id| {
                world
                    .entities
                    .get(id)
                    .is_some_and(|e| e.kind == 0x67d && e.hp > 0)
            })
            .context("closure setup needs a living Cyclops God")?;
        let position = world.entities[&hero].pos;
        ensure!(
            world.walkable(position),
            "prepared final hero arrival is blocked"
        );
        world.entities.get_mut(&hero).unwrap().hp = 1;
        // Native positions need client acknowledgement; prepare ordinary arrival before enrollment instead.
        Arc::make_mut(&mut world.map).spawns = vec![(position.x, position.y)];
        record.cycle.remaining = BTreeSet::from([hero]);
        world.event_reserves.clear();
        tracing::info!(
            realm = origin,
            hero,
            "prepared final Realm hero and arrival; native defeat still required"
        );
        Ok((hero, position))
    }
    pub(super) fn bind_realm_heroes(&mut self, origin: i32, heroes: BTreeSet<i32>) -> Result<()> {
        let world = self.worlds.get(&origin).context("closure Realm missing")?;
        ensure!(
            world.template_id() == Some("realm") && world.admission_open,
            "closure roster requires an open Realm"
        );
        ensure!(
            !self.endgame.realms.contains_key(&origin),
            "Realm roster already bound"
        );
        for id in &heroes {
            let hero = world.entities.get(id).context("closure hero missing")?;
            ensure!(
                world.initial_closure_actor(hero) && hero.hp > 0,
                "closure identity is not a living qualifying hero"
            );
        }
        let cycle = RealmCycle::new(heroes)?;
        tracing::info!(
            origin,
            heroes = cycle.remaining.len(),
            "finite Realm closure roster bound"
        );
        self.endgame.realms.insert(
            origin,
            RealmRecord::new(cycle, self.tick * 50, self.clock.now()),
        );
        Ok(())
    }
    pub(super) fn endgame_step(&mut self, ready: &[i32]) {
        let now = self.tick * 50;
        let due:Vec<_>=self.worlds.summaries().filter(|(id,world)| {
            ready.contains(id) && world.critical || (ready.contains(id) || !world.needs_coordinator()) && self.endgame.realms.get(id).is_some_and(|record| {
                record.enabled && record.cycle.admits() && now>=record.next_taunt
                    || matches!(record.cycle.stage, RealmStage::Closing{deadline}|RealmStage::Sealed{deadline} if now>=deadline)
            })
        }).map(|(id,_)|*id).collect();
        let mut cheaters = Vec::new();
        for origin in &due {
            let world = self.worlds.get_mut(origin).expect("endgame source missing");
            let deaths = std::mem::take(&mut world.critical_deaths);
            let Some(record) = self.endgame.realms.get_mut(origin) else {
                self.worlds.release_world(origin);
                continue;
            };
            let cycle = &mut record.cycle;
            for id in deaths {
                if cycle.admits() && cycle.remaining.contains(&id) {
                    match world.earn_realm_event(id) {
                        Ok(realm_events::EventAdmission::Spawned(event)) => {
                            assert!(
                                cycle.remaining.insert(event),
                                "summoned event already counted"
                            );
                        }
                        Ok(realm_events::EventAdmission::Waiting) => {
                            world.critical_deaths.push(id);
                            continue;
                        }
                        Err(error) => {
                            tracing::error!(origin, hero=id, %error, "earned Realm event initialization refused; retained for retry");
                            world.critical_deaths.push(id);
                            continue;
                        }
                        Ok(realm_events::EventAdmission::Miss) => {}
                    }
                }
                let killer = world.critical_killers.remove(&id);
                if cycle.defeated(id, now) {
                    if let Some((kind, player)) = killer {
                        world.realm_killed_notice(kind, &player);
                    }
                    world.last_realm_enemy_notice(&cycle.remaining);
                    tracing::info!(
                        origin,
                        hero = id,
                        remaining = cycle.remaining.len(),
                        "qualifying Realm hero defeated"
                    );
                    if !cycle.admits() {
                        seal_realm(world, &mut self.grants, *origin);
                    }
                }
            }
            if record.enabled && cycle.admits() && now >= record.next_taunt {
                record.next_taunt = now + REMAINING_TAUNT_MS;
                match world.remaining_taunt(&cycle.remaining) {
                    Ok(Some((kind, count, text))) => {
                        world.oryx_notice(&text);
                        tracing::info!(world = origin, time_ms = now, kind, count, %text, "Oryx remaining encounters announced");
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::error!(world = origin, error = %format_args!("{error:#}"), "Oryx remaining taunt refused")
                    }
                }
            }
            while cycle.advance(now) {
                tracing::info!(origin, now, stage = ?cycle.stage, "Realm closure deadline reached");
                if matches!(cycle.stage, RealmStage::Sealed { .. }) {
                    for text in [
                        "MY MINIONS HAVE FAILED ME!",
                        "BUT NOW YOU SHALL FEEL MY WRATH",
                        "COME MEET YOUR DOOM AT THE WALLS OF MY CASTLE!",
                    ] {
                        world.oryx_notice(text);
                    }
                    // The selected client starts camera jitter at 14; 13 needs a target: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/net/_1f.as#L900-L908
                    let quake = cooperative::effect(14, -1, Pos { x: 0., y: 0. });
                    for (id, p) in &world.players {
                        send(p, wire::SHOW_EFFECT, quake.clone());
                        if p.controller == Controller::Human
                            && world.content.reviewed(p.character.account)
                        {
                            cheaters.push((*id, *origin, p.character.account));
                        }
                    }
                }
            }
            // Preparation owns its inputs; even closing Realms must run while another loan is pending.
            self.worlds.release_world(origin);
        }
        for (player, origin, account) in cheaters {
            tracing::warn!(
                player,
                origin,
                account,
                "marked account sent to the Nexus before Oryx's Castle"
            );
            self.return_to_nexus(player, origin, false);
        }
        let transferring: Vec<_> = self
            .endgame
            .realms
            .iter()
            .filter(|(_, record)| {
                record.cycle.stage == RealmStage::Transferring
                    && record.replacement.is_none()
                    && self.tick >= record.retry_at
            })
            .map(|(id, _)| *id)
            .collect();
        for origin in transferring {
            if let Err(error) = self.replace_realm(origin) {
                self.endgame.realms.get_mut(&origin).unwrap().retry_at = self.tick + 100;
                tracing::error!(origin, %error, "Realm transfer/replacement withheld; retry in five seconds");
            }
        }
        let publishing: Vec<_> = self
            .endgame
            .realms
            .iter()
            .filter(|(_, record)| record.publish_at.is_some_and(|tick| self.tick >= tick))
            .map(|(origin, record)| (*origin, record.replacement.unwrap()))
            .collect();
        for (origin, replacement) in publishing {
            self.endgame.realms.get_mut(&replacement).unwrap().enabled = true;
            self.worlds.get_mut(&replacement).unwrap().admission_open = true;
            self.endgame.realms.get_mut(&origin).unwrap().publish_at = None;
            tracing::info!(
                origin,
                replacement,
                directory_tick = self.tick,
                "safe collective departure completed; replacement published"
            );
        }
        self.endgame.forced.retain(|player, route| {
            self.worlds
                .summary(&route.origin)
                .is_some_and(|w| w.players.contains_key(player))
        });
        let retired: Vec<_> = self
            .endgame
            .realms
            .iter()
            .filter(|(id, record)| {
                record.replacement.is_some()
                    && record.publish_at.is_none()
                    && record.pending_transfers.is_empty()
                    && record.initial_entries.load(Ordering::Acquire) == 0
                    && self
                        .worlds
                        .summary(id)
                        .expect("retiring Realm metadata missing")
                        .players
                        .is_empty()
                    && !self
                        .grants
                        .values()
                        .any(|g| g.origin == Some(**id) || g.destination == **id)
                    && !self
                        .endgame
                        .forced
                        .values()
                        .any(|route| route.origin == **id)
            })
            .map(|(id, _)| *id)
            .collect();
        for origin in retired {
            self.portals.retain(|_, p| p.origin != origin);
            if let Some(portal) = self.endgame.realms[&origin].portal {
                self.worlds.get_mut(&-2).unwrap().remove_entity(portal);
            }
            self.worlds.remove(&origin);
            self.idle.remove(&origin);
            self.endgame.realms.remove(&origin);
            tracing::info!(origin, "empty retired Realm removed");
        }
    }
    pub(crate) fn realm_departed(&mut self, origin: i32, player: i32) {
        let Some(record) = self.endgame.realms.get_mut(&origin) else {
            return;
        };
        if record.pending_transfers.remove(&player) && record.pending_transfers.is_empty() {
            record.publish_at = Some(self.tick + crate::endgame::REPLACEMENT_DELAY_MS / 50);
            tracing::info!(
                origin,
                publish_tick = record.publish_at,
                "all collective recipients durably departed; replacement delay started"
            );
        }
    }
    fn replace_realm(&mut self, origin: i32) -> Result<()> {
        ensure!(self.worlds.contains_key(&origin), "closing Realm missing");
        if let Some(preparation) = &self.endgame.preparing {
            if preparation.origin != origin {
                return Ok(());
            }
        } else {
            let castle_id = self.next_world;
            let realm_id = castle_id
                .checked_add(1)
                .context("world identity exhausted")?;
            let next = realm_id
                .checked_add(1)
                .context("world identity exhausted")?;
            ensure!(
                !self.worlds.contains_key(&castle_id) && !self.worlds.contains_key(&realm_id),
                "world identity collision"
            );
            let rng_before = self.realm_rng.clone();
            let castle = self.construction(CASTLE, "Oryx's Castle", false, None);
            let realm = self.construction("realm", "Realm", false, None);
            let realm_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let started = realm_started.clone();
            let result = match self.worlds.prepare_replacement(origin, self.tick, move || {
                let castle = castle()?;
                started.store(true, std::sync::atomic::Ordering::Release);
                let realm = realm()?;
                let cycle = RealmCycle::new(realm.hero_identities())?;
                Ok((castle, realm, cycle))
            }) {
                Ok(result) => result,
                Err(error) => {
                    self.realm_rng = rng_before;
                    return Err(error);
                }
            };
            // One unpublished build preserves Realm selection/publication order without occupying simulation workers.
            self.endgame.preparing = Some(RealmPreparation {
                origin,
                castle: castle_id,
                realm: realm_id,
                rng_before,
                realm_started,
                result,
            });
            self.next_world = next;
        }
        let result = match self.endgame.preparing.as_mut().unwrap().result.try_recv() {
            Ok(result) => result,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => return Ok(()),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.endgame.preparing.take();
                bail!("Realm preparation worker stopped without returning its result");
            }
        };
        let preparation = self.endgame.preparing.take().unwrap();
        if !preparation
            .realm_started
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.realm_rng = preparation.rng_before;
        }
        let (castle, realm, next_cycle) = result?;
        let (castle_id, realm_id) = (preparation.castle, preparation.realm);
        let name = self.unused_realm_name(Some(origin))?;
        let players: Vec<_> = self.worlds[&origin]
            .players
            .iter()
            .filter(|(_, p)| {
                p.character.alive
                    && p.character.hp > 0
                    && !p.death_pending
                    && !p.departing
                    && p.persistence_failure.is_none()
                    && p.tx
                        .as_ref()
                        .is_some_and(|tx| !tx.packets.is_closed() && !tx.lost.borrow().is_some())
            })
            .map(|(id, _)| *id)
            .collect();
        let old_portal = self.endgame.realms[&origin]
            .portal
            .context("closing Realm has no exact Nexus portal binding")?;
        let nexus = self.worlds.get_mut(&-2).context("Nexus missing")?;
        let entrance = nexus
            .entities
            .get(&old_portal)
            .context("closing Realm entrance missing")?;
        ensure!(
            entrance.kind == 0x712,
            "closing Realm entrance kind changed"
        );
        let portal = self.install_realm_portal(0)?;
        let position = self.worlds[&-2].entities[&portal].pos;
        tracing::info!(
            realm = realm_id,
            portal,
            x = position.x,
            y = position.y,
            "Replacement Realm entrance position reserved"
        );
        let mut realm = realm;
        realm.admission_open = false;
        let mut castle = castle;
        // One island per 20 arrivals, up to 4, from a later comparator: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/realm/worlds/logic/Castle.cs#L24-L44
        // 2012 thresholds are UNKNOWN: "4 or 3 groups" busy, "2 groups" quiet (https://anticosmic.blogspot.com/2012/07/oryx-castle-drops.html); footage wpi7Ff-mOD4 t=0-48 shows 3 players on one island.
        Arc::make_mut(&mut castle.map)
            .spawns
            .truncate(1 + players.len() / 20);
        self.worlds.insert(castle_id, castle);
        self.worlds.insert(realm_id, realm);
        self.idle.insert(castle_id, self.tick);
        let mut record = RealmRecord::new(next_cycle, self.tick * 50, self.clock.now());
        record.portal = Some(portal);
        record.portal_slot = self.endgame.realms[&origin].portal_slot;
        record.name = name;
        record.enabled = false;
        self.endgame.realms.insert(realm_id, record);
        let record = self.endgame.realms.get_mut(&origin).unwrap();
        record.replacement = Some(realm_id);
        record.castle = Some(castle_id);
        record.pending_transfers = players.iter().copied().collect();
        if players.is_empty() {
            record.publish_at = Some(self.tick + crate::endgame::REPLACEMENT_DELAY_MS / 50);
        }
        for player in &players {
            let account = self.worlds[&origin].players[player].character.account;
            self.endgame.forced.insert(
                *player,
                ForcedRoute {
                    origin,
                    // A marked player still departing from the earthquake keeps its Nexus route.
                    destination: if self.content.reviewed(account) {
                        -2
                    } else {
                        castle_id
                    },
                    select_when_paused: true,
                    frozen: false,
                },
            );
        }
        tracing::info!(
            origin,
            castle = castle_id,
            replacement = realm_id,
            recipients = players.len(),
            "Realm closed; collective Castle route and fresh Realm installed"
        );
        Ok(())
    }
    pub(crate) fn native_resurrection_waiting(&self, player: i32, origin: i32) -> bool {
        self.prepared_seed.is_some()
            && self.endgame.forced.get(&player).is_some_and(|route| {
                route.origin == origin
                    && route.destination == -2
                    && route.frozen
                    && !route.select_when_paused
            })
    }
    pub(super) fn return_to_nexus(&mut self, player: i32, origin: i32, frozen: bool) {
        self.endgame.forced.insert(
            player,
            ForcedRoute {
                origin,
                destination: -2,
                select_when_paused: false,
                frozen,
            },
        );
    }

    pub(crate) fn cancel_forced_transfer(&mut self, transfer: &ForcedTransfer) {
        if let Some(grant) = self.grants.remove(&transfer.key) {
            self.worlds
                .get_mut(&grant.destination)
                .unwrap()
                .admissions
                .remove(&transfer.key);
            tracing::error!(
                origin = grant.origin,
                destination = grant.destination,
                player = transfer.player,
                "durable collective departure failed; reservation revoked and origin retained for quarantine"
            );
        }
    }
    pub fn forced_transfer(&mut self, player: i32) -> Result<Option<ForcedTransfer>> {
        let Some(&ForcedRoute {
            origin,
            destination,
            select_when_paused,
            frozen,
        }) = self.endgame.forced.get(&player)
        else {
            return Ok(None);
        };
        let Some(p) = self
            .worlds
            .get(&origin)
            .and_then(|w| w.players.get(&player))
        else {
            self.endgame.forced.remove(&player);
            return Ok(None);
        };
        if !p.character.alive
            || p.character.hp <= 0
            || p.death_pending
            || p.departing && !frozen
            || p.persistence_failure.is_some()
        {
            self.endgame.forced.remove(&player);
            return Ok(None);
        }
        if p.pending || p.experience_queued > 0 || p.clears_queued.iter().any(|count| *count > 0) {
            return Ok(None);
        }
        let destination = (!select_when_paused || !p.paused).then_some(destination);
        let output =
            p.tx.clone()
                .context("collective transfer recipient lacks an owned RECONNECT output")?;
        if output.packets.is_closed() || output.lost.borrow().is_some() {
            self.endgame.forced.remove(&player);
            tracing::info!(
                player,
                origin,
                "disconnected recipient skipped without erasing its owned avatar"
            );
            return Ok(None);
        }
        let (account, character) = (p.character.account, p.character.id);
        self.retire_transfer_grants();
        if destination.is_some() && self.grants.len() >= 64 {
            return Ok(None);
        }
        let (key_time, key, name) = match destination {
            Some(destination) => self.issue(account, character, destination)?,
            None => (0, Vec::new(), String::new()),
        };
        let world = self.worlds.get_mut(&origin).unwrap();
        world.cancel_trade(player, "Trade cancelled by Realm closure.");
        let p = world.players.get_mut(&player).unwrap();
        // Freeze in the same actor job as grant issuance, before an enemy can kill the departing avatar.
        p.departing = true;
        p.tx = None;
        p.bot_goal = None;
        p.bot_aim = None;
        self.endgame.forced.remove(&player);
        Ok(Some(ForcedTransfer {
            player,
            output,
            destination,
            key_time,
            key,
            name,
        }))
    }
}
impl World {
    fn initial_closure_actor(&self, e: &Entity) -> bool {
        match self.xml_game().site_role(e.kind) {
            Some(SiteRole::Hero) => true,
            Some(SiteRole::Closure | SiteRole::Event) => e.quest == Some(e.id),
            _ => false,
        }
    }
    pub(super) fn hero_identities(&self) -> BTreeSet<i32> {
        self.entities
            .values()
            .filter(|e| self.initial_closure_actor(e))
            .map(|e| e.id)
            .collect()
    }
    pub(super) fn publish_endgame_exits(&mut self) {
        if let Some(kind) = self
            .xml_world_template()
            .and_then(|t| t.clear.as_ref()?.reveal)
        {
            for e in self.entities.matching_mut(|e| e.kind == kind) {
                e.published = true;
            }
        }
    }
    pub(super) fn prepare_endgame(&mut self) -> Result<()> {
        if let Some(world) = self.xml_world_template() {
            for &kind in &world.required_actors {
                ensure!(
                    self.entities.values().filter(|e| e.kind == kind).count() == 1,
                    "World {} requires one selected map actor {kind:#x}",
                    world.id
                );
            }
        }
        if let Some(clear) = self.xml_world_template().and_then(|t| t.clear.clone())
            && let Some(kind) = clear.reveal
        {
            ensure!(
                self.entities
                    .values()
                    .filter(|e| e.kind == clear.boss)
                    .count()
                    == 1,
                "{} requires one selected clear actor {:#x}",
                self.name,
                clear.boss
            );
            for e in self.entities.matching_mut(|e| e.kind == kind) {
                e.published = false;
            }
        }
        if let Some(kind) = self
            .xml_world_template()
            .and_then(|w| w.corner_placement.as_ref().map(|p| p.kind))
        {
            // The occupied cell and the inner diagonal distinguish room corners from doorway edges.
            let open = |x: i32, y: i32| {
                self.walkable(Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                })
            };
            let corners: Vec<_> = (0..self.map.height)
                .flat_map(|y| (0..self.map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let cell = &self.map.cells[(y * self.map.width + x) as usize];
                    cell.object.is_none()
                        && cell.region != 1
                        && open(x, y)
                        && [(-1, -1), (-1, 1), (1, -1), (1, 1)]
                            .iter()
                            .any(|&(dx, dy)| {
                                !open(x - dx, y)
                                    && !open(x, y - dy)
                                    && open(x + dx, y)
                                    && open(x, y + dy)
                                    && open(x + dx, y + dy)
                            })
                })
                .collect();
            ensure!(
                !corners.is_empty(),
                "World {:?} has no eligible corners for {kind:#x}",
                self.xml.template
            );
            for &(x, y) in &corners {
                let pos = Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                self.spawn_entity(kind, pos, vec![])?;
            }
            tracing::info!(
                kind,
                count = corners.len(),
                "linked room-corner actors placed"
            );
        }
        Ok(())
    }
    fn validate_portal_inventory_at(
        &self,
        plan: &InventoryPlan,
        portal: PortalPlan,
        accepted: Option<u64>,
    ) -> Result<&str> {
        let p = self
            .players
            .get(&plan.player)
            .context("portal owner missing")?;
        ensure!(
            portal.world == self.xml_world_id()
                && p.pending
                && p.character.id == plan.character
                && p.character.account == plan.account
                && p.character.revision == plan.revision
                && portal.source.object == plan.player
                && p.character.items.get(portal.source.slot) == Some(&portal.source.kind),
            "portal source owner, world, inventory or revision changed"
        );
        let mut expected = p.character.items.clone();
        expected[portal.source.slot] = -1;
        ensure!(
            plan.items == expected && plan.bags.is_empty(),
            "portal inventory publication changed"
        );
        let e = self
            .entities
            .get(&portal.opened)
            .context("reserved portal entity absent")?;
        ensure!(
            e.reserved && !e.published,
            "reserved portal already published or released"
        );
        let definition = self.xml_portal_definition(e.kind, "reserved portal source binding")?;
        let lifetime = if definition.native_service == Some(NativeService::Portal) {
            Some(if portal.locked.is_some() {
                2400
            } else {
                DUNGEON_PORTAL_TICKS
            })
        } else {
            None
        };
        ensure!(
            portal.native_lifetime == lifetime,
            "reserved portal lifetime authority changed"
        );
        let item = self
            .xml_game()
            .properties
            .get(&u16::try_from(portal.source.kind)?)
            .context("reserved portal selected item absent")?;
        self.xml_game().admission(
            item.kind,
            AdmissionRole::Native(NativeService::Inventory),
            "reserved portal source item",
        )?;
        if let Some(locked) = portal.locked {
            ensure!(
                self.unlock_rule().is_some_and(|rule| item.kind == rule.item
                    && e.kind == rule.opened
                    && self
                        .entities
                        .get(&locked)
                        .is_some_and(|e| e.kind == rule.locked && e.reserved && e.published)),
                "reserved Incantation source or locked portal changed"
            );
            ensure!(
                match accepted {
                    Some(request) => self.xml_portal_expiry(locked).is_none_or(|at| request < at),
                    None =>
                        !self.xml_terminal_pending(locked)
                            && self
                                .xml_portal_expiry(locked)
                                .is_none_or(|at| at > self.now),
                },
                "Incantation locked source expired or terminal pending: actor={locked} deadline={:?} now={}",
                self.xml_portal_expiry(locked),
                self.now
            );
        } else {
            ensure!(
                item.create_object.as_deref() == Some(self.content.objects[&e.kind].name.as_str())
                    && self
                        .xml_game()
                        .property_dependencies
                        .get(&item.kind)
                        .is_some_and(|kinds| kinds.contains(&e.kind)),
                "reserved key lacks actual compiled portal target edge"
            );
        }
        self.xml_preflight_position(e.kind, e.pos)?;
        let ignored: Vec<_> = std::iter::once(portal.opened)
            .chain(portal.locked)
            .collect();
        ensure!(
            self.xml_placeable(e.kind, e.pos, &ignored, None)?,
            "reserved portal geometry changed"
        );
        self.xml_preflight_portal_activation(portal.opened)?;
        self.xml_game()
            .portal_instance(e.kind, self.xml.template.as_deref())
    }
    fn unlock_rule(&self) -> Option<&crate::content::game::UnlockRule> {
        self.xml_world_template()?.unlock.as_ref()
    }
    pub(super) fn begin_unlock(
        &mut self,
        id: i32,
        slot: Slot,
        item: &crate::content::Object,
    ) -> Result<InventoryPlan> {
        let rule = self
            .unlock_rule()
            .filter(|_| self.cleared)
            .ok_or_else(|| controller("Wine Cellar requires a defeated Oryx"))?;
        ensure!(
            u16::try_from(slot.kind)? == rule.item && item.kind == rule.item,
            "unsupported portal unlock consumable"
        );
        let (locked_kind, opened_kind) = (rule.locked, rule.opened);
        let name = item
            .unlock_portal
            .as_ref()
            .context("unlock activation metadata missing")?
            .0
            .clone();
        let p = &self.players[&id];
        let locked = self
            .entities
            .values()
            .filter(|e| e.kind == locked_kind && e.published && p.pos.distance(e.pos) <= 3.)
            .min_by(|a, b| p.pos.distance(a.pos).total_cmp(&p.pos.distance(b.pos)))
            .ok_or_else(|| controller("no locked Wine Cellar portal nearby"))?;
        ensure!(
            !locked.reserved,
            controller("another Incantation already reserves this portal")
        );
        ensure!(
            !self.xml_terminal_pending(locked.id)
                && self
                    .xml_portal_expiry(locked.id)
                    .is_none_or(|at| at > self.now),
            "Incantation locked source expired or terminal pending: actor={} deadline={:?} now={}",
            locked.id,
            self.xml_portal_expiry(locked.id),
            self.now
        );
        let (locked, pos) = (locked.id, locked.pos);
        self.reserve_portal(id, slot, opened_kind, Some(locked), pos, name)
    }
    pub(super) fn begin_create(
        &mut self,
        id: i32,
        slot: Slot,
        _item: &crate::content::Object,
    ) -> Result<InventoryPlan> {
        let item = self
            .xml_game()
            .properties
            .get(&u16::try_from(slot.kind)?)
            .context("actual key selected item absent")?;
        ensure!(
            item.consumable && item.activations == 1,
            "invalid dungeon key activation"
        );
        let name = item
            .create_object
            .as_ref()
            .context("key portal identity missing")?;
        let portal = self
            .content
            .objects
            .values()
            .find(|p| &p.name == name)
            .context("key references an unknown portal")?;
        ensure!(
            portal.class == "Portal" && portal.dungeon_name.is_some(),
            "key target is not a dungeon portal"
        );
        let dungeon = portal
            .dungeon_name
            .clone()
            .context("key portal dungeon name missing")?;
        self.reserve_portal(id, slot, portal.kind, None, self.players[&id].pos, dungeon)
    }
    fn reserve_portal(
        &mut self,
        id: i32,
        slot: Slot,
        kind: u16,
        locked: Option<i32>,
        pos: Pos,
        name: String,
    ) -> Result<InventoryPlan> {
        self.xml_game()
            .portal_instance(kind, self.xml.template.as_deref())
            .context("owned portal preflight before inventory reservation")?;
        ensure!(
            slot.object == id
                && self.players[&id].character.items.get(slot.slot) == Some(&slot.kind),
            "portal reservation is not current owned inventory"
        );
        self.xml_game().admission(
            u16::try_from(slot.kind)?,
            AdmissionRole::Native(NativeService::Inventory),
            "actual owned portal item activation",
        )?;
        let mut items = self.players[&id].character.items.clone();
        items[slot.slot] = -1;
        let native_lifetime = if self
            .xml_portal_definition(kind, "owned portal item lifetime")?
            .native_service
            == Some(NativeService::Portal)
        {
            Some(if locked.is_some() {
                2400
            } else {
                DUNGEON_PORTAL_TICKS
            })
        } else {
            None
        };
        let mut plan = self.plan(id, items, None)?;
        let opened = self.insert_portal_entity(
            kind,
            pos,
            "actual owned key/Incantation portal reservation",
        )?;
        let e = self.entities.get_mut(&opened).unwrap();
        e.name = name;
        e.extra_stats.retain(|(stat, _)| *stat != 37);
        e.extra_stats.push((37, 1));
        e.published = false;
        e.reserved = true;
        if let Some(locked) = locked {
            self.entities.get_mut(&locked).unwrap().reserved = true;
        }
        self.xml_prepare_portal_actor(opened)
            .expect("preflighted owned portal program");
        self.cancel_trade(id, "Trade cancelled by owned portal item activation.");
        self.players.get_mut(&id).unwrap().pending = true;
        plan.unlock = Some(PortalPlan {
            locked,
            opened,
            source: slot,
            world: self.xml_world_id(),
            native_lifetime,
        });
        Ok(plan)
    }
    fn discard_unlock(&mut self, locked: Option<i32>, opened: i32) -> Result<()> {
        ensure!(
            self.entities
                .get(&opened)
                .is_some_and(|e| !e.published && e.reserved),
            "cannot discard a published portal"
        );
        self.entities.get_mut(&opened).unwrap().reserved = false;
        self.remove_entity(opened);
        if let Some(locked) = locked {
            self.entities
                .get_mut(&locked)
                .context("reserved locked portal lost")?
                .reserved = false;
        }
        Ok(())
    }
    pub(super) fn oryx_notice(&self, text: &str) {
        self.broadcast_text("#Oryx the Mad God", text);
    }
}

#[cfg(test)]
#[test]
fn prepare_realm_closure_preserves_other_cycle_and_refuses_reservations() {
    let content = Arc::new(crate::world::test_content());
    let mut d = Directory::fixture(content, 16, 12351, Some(12351), 2).unwrap();
    d.clock.set_paused(true);
    let other = d.endgame.realms[&1].cycle.remaining.clone();
    let other_spawns = d.worlds[&1].map.spawns.clone();
    d.issue(7, 7, -3).unwrap();
    assert!(
        d.prepare_realm_closure(-3)
            .unwrap_err()
            .to_string()
            .contains("reservations")
    );
    d.tick += 2400;
    d.retire_transfer_grants();
    let (hero, position) = d.prepare_realm_closure(-3).unwrap();
    assert_eq!(d.worlds[&-3].map.spawns, vec![(position.x, position.y)]);
    assert_eq!(d.worlds[&1].map.spawns, other_spawns);
    assert_eq!(
        d.endgame.realms[&-3].cycle.remaining,
        BTreeSet::from([hero])
    );
    assert!(d.endgame.realms[&-3].cycle.admits() && d.worlds[&-3].admission_open);
    assert_eq!(d.endgame.realms[&1].cycle.remaining, other);
    assert!(d.endgame.realms[&1].cycle.admits() && d.worlds[&1].admission_open);
}

#[cfg(test)]
#[test]
fn eight_realm_closures_keep_held_predecessors_across_two_replacement_generations() {
    let content = Arc::new(crate::world::test_content());
    let mut d = Directory::fixture(content, 16, 1344, Some(1344), 8).unwrap();
    let mut holds = Vec::new();
    for generation in 0..2 {
        let current: Vec<_> = d
            .endgame
            .realms
            .iter()
            .filter(|(_, r)| r.replacement.is_none())
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(current.len(), 8);
        for origin in &current {
            holds.push(d.hold_initial_entry(*origin).unwrap().unwrap());
            let cycle = &mut d.endgame.realms.get_mut(origin).unwrap().cycle;
            for root in cycle.remaining.clone() {
                assert!(cycle.defeated(root, 0));
            }
            assert!(cycle.advance(crate::endgame::CLOSE_DELAY_MS));
            assert!(cycle.advance(crate::endgame::CLOSE_DELAY_MS + crate::endgame::EARTHQUAKE_MS));
            d.worlds.get_mut(origin).unwrap().admission_open = false;
        }
        d.sync_realm_portal();
        for origin in current {
            let old_name = d.endgame.realms[&origin].name;
            let old_portal = d.endgame.realms[&origin].portal.unwrap();
            d.replace_realm(origin).unwrap();
            let next = d.endgame.realms[&origin].replacement.unwrap();
            assert_ne!(d.endgame.realms[&next].name, old_name);
            assert!(!d.endgame.realms[&next].enabled);
            assert!(!d.worlds[&next].admission_open);
            assert_eq!(d.endgame.realms[&origin].portal, Some(old_portal));
            assert_eq!(d.worlds[&-2].entities[&old_portal].kind, 0x712);
            assert_eq!(
                d.endgame.realms[&origin]
                    .initial_entries
                    .load(Ordering::Acquire),
                1
            );
        }
        let names: BTreeSet<_> = d
            .endgame
            .realms
            .values()
            .filter(|r| r.replacement.is_none())
            .map(|r| r.name)
            .collect();
        assert_eq!(names.len(), 8);
        let reserved: Vec<_> = d.worlds[&-2]
            .entities
            .values()
            .filter(|e| e.kind == 0x712 && d.realm_entrance_reserves_position(e.id).unwrap())
            .map(|e| e.pos)
            .collect();
        assert_eq!(reserved.len(), 8);
        for (i, pos) in reserved.iter().enumerate() {
            assert!(
                reserved
                    .iter()
                    .skip(i + 1)
                    .all(|other| pos.distance(*other) >= 4.)
            );
        }
        d.tick += crate::endgame::REPLACEMENT_DELAY_MS / 50;
        d.endgame_step(&[]);
        d.sync_realm_portal();
        assert_eq!(d.endgame.realms.len(), 8 * (generation + 2));
        assert_eq!(
            d.endgame
                .realms
                .values()
                .filter(|r| r.replacement.is_none())
                .count(),
            8
        );
        assert_eq!(
            d.worlds[&-2]
                .entities
                .values()
                .filter(|e| e.kind == 0x712 && e.published)
                .count(),
            8
        );
    }
    assert_eq!(holds.len(), 16);
    assert_eq!(
        d.endgame.realms.len(),
        24,
        "initial-entry holds retain physical predecessors without claiming a global bound"
    );
}

#[cfg(test)]
#[tokio::test]
async fn realm_close_token_starts_the_ordinary_closure_once() {
    let content = Arc::new(crate::world::test_content());
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let account = store.list("realm-close-token".into()).await.unwrap();
    let character = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let mut directory = Directory::fixture(content.clone(), 16, 12351, Some(12351), 2).unwrap();
    let map = Map {
        elevation: None,
        width: 40,
        height: 40,
        cells: vec![
            crate::content::Cell {
                ground: 0x46,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            1600
        ],
        spawns: vec![(14.5, 20.5)],
    };
    let mut world = World::with_ids(content, Arc::new(map), directory.ids.clone()).unwrap();
    world.name = "Realm".into();
    world.xml.template = Some("realm".into());
    world.encounter = true;
    let hero = world
        .insert_entity(HEROES[0], Pos { x: 30.5, y: 30.5 }, vec![])
        .unwrap();
    let (packets, mut inbox) = mpsc::channel(64);
    let (lost, _) = watch::channel(None);
    let player = world
        .enter(character.clone(), Some(Output { packets, lost }))
        .unwrap();
    while inbox.try_recv().is_ok() {}
    directory.worlds.insert(-3, world);
    directory.endgame.realms.remove(&-3);
    directory
        .bind_realm_heroes(-3, BTreeSet::from([hero]))
        .unwrap();
    assert_eq!(
        directory.force_realm_closure(player).unwrap(),
        character.name
    );
    let texts: Vec<_> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter(|p| p.id == wire::TEXT)
        .map(|p| String::from_utf8_lossy(&p.body).into_owned())
        .collect();
    assert_eq!(texts.len(), 2);
    assert!(texts[0].contains(&format!("{} used a Realm Close token!", character.name)));
    assert!(texts[1].contains("I HAVE CLOSED THIS REALM!"));
    assert!(!directory.worlds[&-3].admission_open);
    assert!(matches!(
        directory.endgame.realms[&-3].cycle.stage,
        RealmStage::Closing { .. }
    ));
    let again = directory.force_realm_closure(player).unwrap_err();
    assert_eq!(again.to_string(), "This Realm is already closing.");
}

#[cfg(test)]
#[test]
fn pending_event_does_not_block_other_defeats_in_the_realm_cycle() {
    let content = Arc::new(crate::world::test_content());
    let mut directory = Directory::populated_fixture(content.clone(), 85, 787901).unwrap();
    let initial = directory.endgame.realms[&-3].cycle.remaining.clone();
    let other = directory.endgame.realms[&1].cycle.remaining.clone();
    let world = directory.worlds.get_mut(&-3).unwrap();
    let defeated: Vec<_> = world
        .actors()
        .filter(|e| HEROES.contains(&e.kind))
        .take(2)
        .map(|e| e.id)
        .collect();
    assert_eq!(defeated.len(), 2);
    assert!(defeated.iter().all(|id| initial.contains(id)));
    let sites: Vec<_> = world.event_reserves.iter().map(|e| e.root.pos).collect();
    assert!(!sites.is_empty());
    for (index, position) in sites.into_iter().enumerate() {
        let mut character = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
        character.id = i32::try_from(index + 1).unwrap();
        character.account = character.id;
        let player = world.enter(character, None).unwrap();
        world.players.get_mut(&player).unwrap().pos = position;
    }
    for id in &defeated {
        world.defeat_entity(*id).unwrap();
        for _ in 0..8 {
            crate::world::xml::tests::drain_events(world).unwrap();
        }
    }
    // Model an already-earned event while actual player occupancy blocks every reserved site.
    world.event_waiting = Some(defeated[0]);
    let reserves = world.event_reserves.len();
    for _ in 0..2 {
        directory.endgame_step(&[-3]);
        let world = &directory.worlds[&-3];
        assert_eq!(world.event_waiting, Some(defeated[0]));
        assert_eq!(world.event_reserves.len(), reserves);
        assert_eq!(world.critical_deaths, vec![defeated[0]]);
        let mut expected = initial.clone();
        expected.remove(&defeated[1]);
        assert_eq!(directory.endgame.realms[&-3].cycle.remaining, expected);
        assert!(directory.endgame.realms[&-3].cycle.admits() && world.admission_open);
        assert_eq!(directory.endgame.realms[&1].cycle.remaining, other);
    }
    let world = directory.worlds.get_mut(&-3).unwrap();
    world.players.clear();
    let &parent = world
        .population
        .iter()
        .find_map(|slot| slot.entity.as_ref())
        .unwrap();
    let pos = world.entities[&parent].pos;
    for _ in 0..1024 {
        let child = world.spawn_entity(0x20f, pos, vec![]).unwrap();
        world.set_parent(child, Some(parent));
    }
    directory.endgame_step(&[-3]);
    let world = &directory.worlds[&-3];
    assert_eq!(world.event_waiting, None);
    assert_eq!(world.event_reserves.len(), reserves - 1);
    assert!(
        !directory.endgame.realms[&-3]
            .cycle
            .remaining
            .contains(&defeated[0])
    );
}

#[cfg(test)]
#[tokio::test]
async fn earthquake_sends_marked_players_to_the_nexus_instead_of_the_castle() {
    let mut content = crate::world::test_content();
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let mut characters = Vec::new();
    for name in ["quake-cheater", "quake-honest"] {
        let account = store.list(name.into()).await.unwrap();
        characters.push(
            store
                .create(account.id, content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap(),
        );
    }
    content.reviewed_accounts =
        crate::content::reviews::Reviews::new(vec![crate::content::reviews::Review {
            account: characters[0].account,
            reviewed_on: "2026-11-18".into(),
        }]);
    let content = Arc::new(content);
    let mut directory = Directory::fixture(content.clone(), 16, 12351, Some(12351), 2).unwrap();
    let map = Map {
        elevation: None,
        width: 40,
        height: 40,
        cells: vec![
            crate::content::Cell {
                ground: 0x46,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            1600
        ],
        spawns: vec![(14.5, 20.5)],
    };
    let mut world = World::with_ids(content, Arc::new(map), directory.ids.clone()).unwrap();
    world.name = "Realm".into();
    world.xml.template = Some("realm".into());
    world.encounter = true;
    let hero = world
        .insert_entity(HEROES[0], Pos { x: 30.5, y: 30.5 }, vec![])
        .unwrap();
    let mut inboxes = Vec::new();
    let players: Vec<_> = characters
        .iter()
        .map(|character| {
            let (packets, inbox) = mpsc::channel(64);
            let (lost, _) = watch::channel(None);
            inboxes.push(inbox);
            world
                .enter(character.clone(), Some(Output { packets, lost }))
                .unwrap()
        })
        .collect();
    directory.worlds.insert(-3, world);
    directory.endgame.realms.remove(&-3);
    directory
        .bind_realm_heroes(-3, BTreeSet::from([hero]))
        .unwrap();
    directory.force_realm_closure(players[1]).unwrap();
    directory.tick = crate::endgame::CLOSE_DELAY_MS / 50;
    directory.endgame_step(&[-3]);
    assert!(matches!(
        directory.endgame.realms[&-3].cycle.stage,
        RealmStage::Sealed { .. }
    ));
    assert_eq!(
        directory
            .endgame
            .forced
            .get(&players[0])
            .map(|r| r.destination),
        Some(-2)
    );
    assert!(!directory.endgame.forced.contains_key(&players[1]));
}

impl Directory {
    pub(crate) fn retain_inventory_event(
        &mut self,
        plan: InventoryPlan,
    ) -> Result<crate::store::world_events::Event> {
        let origin = self.location(plan.player)?;
        let w = &self.worlds[&origin];
        let p = w
            .players
            .get(&plan.player)
            .context("journal inventory owner absent")?;
        ensure!(
            p.pending
                && p.character.id == plan.character
                && p.character.account == plan.account
                && p.character.revision == plan.revision,
            "journal inventory owner/reservation changed"
        );
        let binding=plan.unlock.map(|portal|->Result<_>{
            let route=self.portals.get(&portal.opened).context("journal portal route absent")?;
            Ok(serde_json::json!({"opened":portal.opened,"origin":route.origin,"destination":route.destination,"locked_deadline":portal.locked.and_then(|id|w.xml_portal_expiry(id)),"native_lifetime":portal.native_lifetime}))
        }).transpose()?;
        let id = plan.permit.event.clone();
        ensure!(
            !self.inventory_events.contains_key(&id),
            "inventory event already staged: {id}"
        );
        let event = crate::store::world_events::Event {
            id,
            kind: "inventory".into(),
            content: w.xml_game().fingerprint.clone(),
            payload: crate::store::world_events::EventPayload::Inventory(serde_json::json!({
                "write":crate::store::InventoryWrite{id:plan.character,account:plan.account,revision:plan.revision,items:plan.items.clone()},
                "reservation":plan,
                "claims":plan.loot_slot.filter(|_|!plan.entitlement_move).and_then(|(bag,slot)|w.xml.bag_entitlements.get(&(bag,slot)).map(|(event,ordinal,_,_)|serde_json::json!([event,ordinal]))),
                "bag_deadlines":plan.bags.iter().map(|(bag,_)|(*bag,w.bag_expiry.get(bag).copied())).collect::<Vec<_>>(),
                "origin":{"directory_world":origin,"instance":w.xml.instance,"world":w.xml_world_id(),"accepted_at":plan.accepted_at},
                "portal":binding
            })),
        };
        self.inventory_events.insert(
            event.id.clone(),
            (
                plan,
                false,
                event.clone(),
                std::time::Instant::now() + std::time::Duration::from_secs(10),
                None,
            ),
        );
        Ok(event)
    }
    pub(crate) fn inventory_journal_ids(&self) -> Vec<String> {
        self.inventory_events.keys().cloned().collect()
    }
    pub(crate) fn inventory_retries(
        &mut self,
    ) -> Vec<(InventoryPlan, crate::store::world_events::Event)> {
        let now = std::time::Instant::now();
        self.inventory_events
            .values_mut()
            .filter_map(|(plan, _, event, next, _)| {
                if now < *next {
                    return None;
                }
                *next = now + std::time::Duration::from_secs(10);
                Some((plan.clone(), event.clone()))
            })
            .collect()
    }
    pub(crate) fn confirm_lost_journal_origins(
        &self,
        // Entry identities never change or reappear; current Directory membership proves absence, not the earlier census.
        candidates: Vec<(String, String)>,
    ) -> Vec<(String, String)> {
        candidates
            .into_iter()
            .filter(|(_, origin)| !self.worlds.summaries().any(|(_, w)| w.instance == *origin))
            .collect()
    }
    pub(crate) fn retire_lost_inventory_events(&mut self, ids: Vec<String>) -> Result<()> {
        for id in ids {
            if let Some((_, _, event, _, _)) = self.inventory_events.get(&id) {
                let instance = event.native_payload()?["origin"]["instance"]
                    .as_str()
                    .context("retained inventory origin missing")?;
                ensure!(
                    !self.worlds.summaries().any(|(_, w)| w.instance == instance),
                    "cannot retire live origin inventory event {id}"
                );
                self.inventory_events.remove(&id);
            }
        }
        Ok(())
    }
    pub(crate) fn publish_inventory_event(
        &mut self,
        event: &crate::store::world_events::Event,
        result: &serde_json::Value,
    ) -> Result<Option<(u64, u64)>> {
        let (plan, published, captured, _, publication) =
            self.inventory_events.get(&event.id).context(
                "full inventory plan unavailable; retained journal requires origin reconciliation",
            )?;
        ensure!(
            captured == event,
            "inventory receipt differs from retained immutable event"
        );
        if *published {
            return Ok(*publication);
        }
        let mut plan = plan.clone();
        ensure!(
            event.native_payload()?["reservation"] == serde_json::to_value(&plan)?,
            "inventory journal differs from original full reservation"
        );
        let origin = self.location(plan.player)?;
        ensure!(
            event.content == self.worlds[&origin].xml_game().fingerprint,
            "inventory journal owning catalogue changed"
        );
        ensure!(
            event.native_payload()?["origin"]["instance"].as_str()
                == Some(self.worlds[&origin].xml.instance.as_str()),
            "inventory journal origin instance changed; retained for reconciliation"
        );
        let w = &self.worlds[&origin];
        let late = event.native_payload()?["portal"]["locked_deadline"]
            .as_u64()
            .filter(|deadline| w.now >= *deadline)
            .map(|deadline| (deadline, w.now));
        let deadlines: Vec<(i32, Option<u64>)> =
            serde_json::from_value(event.native_payload()?["bag_deadlines"].clone())?;
        for (bag, deadline) in deadlines {
            if !w.entities.contains_key(&bag) {
                ensure!(
                    deadline.is_some_and(|deadline| deadline <= w.now),
                    "native bag {bag} missing without original expiry evidence; event {} retained",
                    event.id
                );
                plan.bags.retain(|(id, _)| *id != bag);
                if plan.loot_slot.is_some_and(|(id, _)| id == bag) {
                    plan.loot_slot = None;
                }
                if plan
                    .ground_dye
                    .as_ref()
                    .is_some_and(|dye| dye.bag_id() == bag)
                {
                    plan.ground_dye = None;
                }
            }
        }
        if plan.vault_purchase.is_some() {
            let native = if result.get("rejected") == Some(&serde_json::json!(true)) {
                None
            } else {
                Some(serde_json::from_value(
                    result
                        .get("native_result")
                        .context("vault purchase journal result absent")?
                        .clone(),
                )?)
            };
            self.worlds
                .get_mut(&origin)
                .unwrap()
                .finish_vault_purchase(plan, Ok(native))?;
        } else if plan.purchase.is_some() {
            let native = if result.get("rejected") == Some(&serde_json::json!(true)) {
                None
            } else {
                Some(serde_json::from_value(
                    result
                        .get("native_result")
                        .context("purchase journal result absent")?
                        .clone(),
                )?)
            };
            self.worlds
                .get_mut(&origin)
                .unwrap()
                .finish_purchase(plan, Ok(native))?;
        } else {
            let revision = i32::try_from(
                result["revision"]
                    .as_i64()
                    .context("inventory journal revision absent")?,
            )?;
            self.finish_inventory_receipt(plan, Ok(revision), Some(event))?;
        }
        let retained = self.inventory_events.get_mut(&event.id).unwrap();
        retained.1 = true;
        retained.4 = late;
        Ok(late)
    }
    pub(crate) fn forget_inventory_event(&mut self, id: &str) -> Result<()> {
        ensure!(
            self.inventory_events
                .get(id)
                .is_some_and(|(_, published, _, _, _)| *published),
            "cannot retire unpublished inventory event"
        );
        self.inventory_events.remove(id);
        Ok(())
    }
}
