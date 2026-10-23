use super::*;
use crate::endgame::{CASTLE, CELLAR, HEROES, INCANTATION, LOCKED_CELLAR, RealmCycle, RealmStage};
use rand::seq::IteratorRandom;

// Gameplay documentation specifies two minutes, unlike modified forks' 20s: https://wikiwiki.jp/rmd/イベント#jded12e1
const REMAINING_TAUNT_MS: u64 = 120_000;

#[derive(Default)]
pub(super) struct RealmDirectory {
    pub realms: BTreeMap<i32, RealmRecord>,
    pub(super) forced: BTreeMap<i32, ForcedRoute>,
}
pub(super) struct RealmRecord {
    pub cycle: RealmCycle,
    pub portal: Option<i32>,
    pub name: usize,
    pub enabled: bool,
    replacement: Option<i32>,
    pub castle: Option<i32>,
    pub pending_transfers: BTreeSet<i32>,
    publish_at: Option<u64>,
    retry_at: u64,
    next_taunt: u64,
}
impl RealmRecord {
    fn new(cycle: RealmCycle, now: u64) -> Self {
        Self {
            cycle,
            portal: None,
            name: 0,
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
impl Directory {
    pub fn stage_unlock(&mut self, plan: InventoryPlan) -> Result<InventoryPlan> {
        let Some(portal) = plan.unlock else {
            return Ok(plan);
        };
        let PortalPlan { locked, opened } = portal;
        ensure!(!self.portals.contains_key(&opened), "portal already staged");
        let origin = self.location(plan.player)?;
        let w = &self.worlds[&origin];
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
        let template = if let Some(locked) = locked {
            ensure!(
                w.entities
                    .get(&locked)
                    .is_some_and(|e| e.kind == LOCKED_CELLAR && e.reserved && e.published),
                "locked portal reservation missing"
            );
            ensure!(kind == 0x700, "Incantation staged the wrong portal");
            Some(CELLAR)
        } else {
            crate::dungeon::DUNGEONS
                .iter()
                .chain(crate::endgame::DUNGEONS)
                .find(|(portal, _, _, _)| *portal == kind)
                .map(|(_, template, _, _)| *template)
        };
        ensure!(
            w.entities
                .get(&opened)
                .is_some_and(|e| e.kind == kind && e.reserved && !e.published),
            "unpublished portal missing"
        );
        let destination = match template
            .map(|template| self.instance(template, None))
            .transpose()
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
                destination,
                expires: self.tick + portal.lifetime(),
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
    pub fn finish_inventory(&mut self, mut plan: InventoryPlan, result: Result<i32>) -> Result<()> {
        let Some(portal) = plan.unlock.take() else {
            return self
                .resolve(Target::Player(plan.player))?
                .finish_inventory(plan, result);
        };
        let PortalPlan { locked, opened } = portal;
        let player = plan.player;
        let origin = self.location(player)?;
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
            locked.is_none_or(|id| w
                .entities
                .get(&id)
                .is_some_and(|e| e.reserved && e.kind == LOCKED_CELLAR))
                && w.entities
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
        if result.is_err() {
            self.worlds
                .get_mut(&origin)
                .unwrap()
                .discard_unlock(locked, opened)?;
            self.portals.remove(&opened);
            if let Some(destination) = destination {
                ensure!(
                    self.worlds[&destination].players.is_empty()
                        && !self.grants.values().any(|g| g.destination == destination),
                    "unpublished dungeon acquired a participant"
                );
                self.worlds.remove(&destination);
                self.idle.remove(&destination);
            }
            return self
                .worlds
                .get_mut(&origin)
                .unwrap()
                .finish_inventory(plan, result);
        }
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
        self.portals.get_mut(&opened).unwrap().expires = self.tick + portal.lifetime();
        if locked.is_none() {
            let name = w.players[&player].character.name.clone();
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
            "consumable committed; bound dungeon portal published"
        );
        Ok(())
    }
    fn chamber_step(&mut self) {
        let markers: Vec<_> = self
            .worlds
            .iter()
            .filter(|(_, w)| w.name == "Oryx's Castle" && w.cleared)
            .flat_map(|(origin, w)| {
                w.entities
                    .values()
                    .filter(|e| e.kind == 0xd7a && e.next_aux <= self.tick * 50)
                    .map(|e| (*origin, e.id, e.pos))
            })
            .collect();
        let origins: Vec<_> = markers.iter().map(|(origin, _, _)| *origin).collect();
        for (origin, marker, pos) in markers {
            let result = (|| -> Result<()> {
                ensure!(
                    self.worlds[&origin].walkable(pos),
                    "authored Chamber entrance position blocked"
                );
                self.reserve_worlds(1, &origins)?;
                let destination = self.instance(crate::endgame::CHAMBER, None)?;
                let portal =
                    match self
                        .worlds
                        .get_mut(&origin)
                        .unwrap()
                        .spawn_entity(0xd7b, pos, vec![])
                    {
                        Ok(portal) => portal,
                        Err(error) => {
                            self.worlds.remove(&destination);
                            self.idle.remove(&destination);
                            return Err(error);
                        }
                    };
                self.portals.insert(
                    portal,
                    Portal {
                        origin,
                        destination: Some(destination),
                        expires: self.tick + 2400,
                    },
                );
                let w = self.worlds.get_mut(&origin).unwrap();
                w.remove_entity(marker);
                w.oryx_notice("You have defeated my Guardians. Face me in my Chamber!");
                tracing::info!(
                    origin,
                    portal,
                    destination,
                    "Guardian pair earned its bound Chamber portal"
                );
                Ok(())
            })();
            if let Err(error) = result {
                self.worlds
                    .get_mut(&origin)
                    .unwrap()
                    .entities
                    .get_mut(&marker)
                    .unwrap()
                    .next_aux = self.tick * 50 + 5000;
                tracing::error!(origin,%error,"earned Chamber binding failed; retry scheduled without publishing a portal");
            }
        }
    }
    pub(super) fn bind_realm_heroes(&mut self, origin: i32, heroes: BTreeSet<i32>) -> Result<()> {
        let world = self.worlds.get(&origin).context("closure Realm missing")?;
        ensure!(
            world.name == "Realm" && world.admission_open,
            "closure roster requires an open Realm"
        );
        ensure!(
            !self.endgame.realms.contains_key(&origin),
            "Realm roster already bound"
        );
        for id in &heroes {
            let hero = world.entities.get(id).context("closure hero missing")?;
            ensure!(
                (HEROES.contains(&hero.kind)
                    || crate::quest::root(hero.kind) && hero.quest == Some(*id))
                    && hero.hp > 0,
                "closure identity is not a living qualifying hero"
            );
        }
        let cycle = RealmCycle::new(heroes)?;
        tracing::info!(
            origin,
            heroes = cycle.remaining.len(),
            "finite Realm closure roster bound"
        );
        self.endgame
            .realms
            .insert(origin, RealmRecord::new(cycle, self.tick * 50));
        Ok(())
    }
    pub(super) fn endgame_step(&mut self) {
        self.chamber_step();
        let now = self.tick * 50;
        for (origin, world) in &mut self.worlds {
            let deaths = std::mem::take(&mut world.critical_deaths);
            let Some(record) = self.endgame.realms.get_mut(origin) else {
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
                if cycle.defeated(id, now) {
                    tracing::info!(
                        origin,
                        hero = id,
                        remaining = cycle.remaining.len(),
                        "qualifying Realm hero defeated"
                    );
                    if !cycle.admits() {
                        world.admission_open = false;
                        world.oryx_notice(
                            "I HAVE CLOSED THIS REALM! YOU WILL NOT LIVE TO SEE THE LIGHT OF DAY!",
                        );
                        self.grants.retain(|_, g| g.destination != *origin);
                        world.admissions.clear();
                    }
                }
            }
            if record.enabled && cycle.admits() && now >= record.next_taunt {
                record.next_taunt = now + REMAINING_TAUNT_MS;
                if let Some((kind, count, text)) = world.remaining_taunt(&cycle.remaining) {
                    world.oryx_notice(&text);
                    tracing::info!(world = origin, time_ms = now, kind, count, %text, "Oryx remaining encounters announced");
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
                    let quake = cooperative::effect(13, -1, Pos { x: 0., y: 0. });
                    for p in world.players.values() {
                        send(p, wire::SHOW_EFFECT, quake.clone());
                    }
                }
            }
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
                "safe collective departure completed; replacement published"
            );
        }
        self.endgame.forced.retain(|player, route| {
            self.worlds
                .get(&route.origin)
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
                    && self.worlds[id].players.is_empty()
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
        self.reserve_worlds(2, &[origin])?;
        let castle = self.make(CASTLE, "Oryx's Castle", false, None)?;
        let realm = self.make("realm-shore", "Realm", false, None)?;
        let next_cycle = RealmCycle::new(realm.hero_identities())?;
        let name = (1..=super::realm_portal::NAMES.len())
            .map(|offset| {
                (self.endgame.realms[&origin].name + offset) % super::realm_portal::NAMES.len()
            })
            .find(|name| self.endgame.realms.values().all(|r| r.name != *name))
            .context("no unused Realm name")?;
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
                        .is_some_and(|tx| !tx.packets.is_closed() && !*tx.lost.borrow())
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
        let portal = nexus.spawn_entity(0x712, entrance.pos, vec![])?;
        nexus.entities.get_mut(&portal).unwrap().published = false;
        let mut realm = realm;
        realm.admission_open = false;
        self.worlds.insert(castle_id, castle);
        self.worlds.insert(realm_id, realm);
        self.next_world = next;
        self.idle.insert(castle_id, self.tick);
        let mut record = RealmRecord::new(next_cycle, self.tick * 50);
        record.portal = Some(portal);
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
            self.endgame.forced.insert(
                *player,
                ForcedRoute {
                    origin,
                    destination: castle_id,
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

    #[cfg(test)]
    pub fn forced_transfer_candidates(&self) -> Vec<i32> {
        self.endgame.forced.keys().copied().collect()
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
        if output.packets.is_closed() || *output.lost.borrow() {
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
    pub(super) fn populate_realm_heroes(&mut self) -> Result<()> {
        for (kind, terrains) in [(HEROES[0], &[1][..]), (HEROES[1], &[3, 4][..])] {
            for _ in 0..3 {
                let pos = self.place_named_setpiece(kind, terrains)?;
                self.spawn_entity(kind, pos, vec![])?;
            }
            tracing::info!(kind, "three finite landmark heroes placed");
        }
        Ok(())
    }
    pub(super) fn hero_identities(&self) -> BTreeSet<i32> {
        self.entities
            .values()
            .filter(|e| {
                HEROES.contains(&e.kind) || (crate::quest::root(e.kind) && e.quest == Some(e.id))
            })
            .map(|e| e.id)
            .collect()
    }
    pub(super) fn prepare_endgame(&mut self) -> Result<()> {
        if self.name == "Oryx's Castle" {
            for e in self.entities.matching_mut(|e| e.kind == 0xd7d) {
                e.condition |= crate::condition::bit(crate::condition::INVINCIBLE);
                e.published = false;
            }
            ensure!(
                crate::endgame::GUARDIANS.iter().all(|kind| self
                    .entities
                    .values()
                    .filter(|e| e.kind == *kind)
                    .count()
                    == 1),
                "Castle requires the authored Guardian pair"
            );
            let markers: Vec<_> = self
                .entities
                .values()
                .filter(|e| e.kind == 0xd7a)
                .map(|e| e.id)
                .collect();
            ensure!(
                markers.len() == 1,
                "Castle requires one authored Guardian taskmaster"
            );
            let e = self.entities.get_mut(&markers[0]).unwrap();
            e.published = false;
            e.condition |= crate::condition::bit(crate::condition::INVINCIBLE);
        }
        if matches!(self.name.as_str(), "Oryx's Chamber" | "Wine Cellar") {
            let boss = if self.name == "Wine Cellar" {
                crate::endgame::ORYX2
            } else {
                crate::endgame::ORYX
            };
            ensure!(
                self.entities.values().filter(|e| e.kind == boss).count() == 1,
                "endgame room requires one selected boss"
            );
            for e in self.entities.matching_mut(|e| e.kind == 0x71d) {
                e.published = false;
            }
        }
        Ok(())
    }
    pub(super) fn begin_unlock(
        &mut self,
        id: i32,
        slot: Slot,
        item: &crate::content::Object,
    ) -> Result<InventoryPlan> {
        ensure!(
            item.kind == INCANTATION && item.activations == 1,
            "unsupported portal unlock consumable"
        );
        let (dungeon, locked_name) = item
            .unlock_portal
            .as_ref()
            .context("unlock activation metadata missing")?;
        ensure!(
            dungeon == "Wine Cellar" && locked_name == &self.content.get(LOCKED_CELLAR)?.name,
            "Incantation activation contradicts selected Cellar portal"
        );
        ensure!(
            self.name == "Oryx's Chamber" && self.cleared,
            controller("Wine Cellar requires a defeated Oryx")
        );
        let p = &self.players[&id];
        let locked = self
            .entities
            .values()
            .filter(|e| e.kind == LOCKED_CELLAR && e.published && p.pos.distance(e.pos) <= 3.)
            .min_by(|a, b| p.pos.distance(a.pos).total_cmp(&p.pos.distance(b.pos)))
            .ok_or_else(|| controller("no locked Wine Cellar portal nearby"))?;
        ensure!(
            !locked.reserved,
            controller("another Incantation already reserves this portal")
        );
        let (locked, pos) = (locked.id, locked.pos);
        self.reserve_portal(id, slot, 0x700, Some(locked), pos, dungeon.clone())
    }
    pub(super) fn begin_create(
        &mut self,
        id: i32,
        slot: Slot,
        item: &crate::content::Object,
    ) -> Result<InventoryPlan> {
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
        let usable = crate::dungeon::DUNGEONS
            .iter()
            .chain(crate::endgame::DUNGEONS)
            .any(|(portal, _, _, _)| *portal == kind);
        let mut items = self.players[&id].character.items.clone();
        items[slot.slot] = -1;
        let opened = self.spawn_entity(kind, pos, vec![])?;
        let e = self.entities.get_mut(&opened).unwrap();
        e.name = name;
        e.extra_stats.retain(|(stat, _)| *stat != 37);
        e.extra_stats.push((37, i32::from(usable)));
        e.published = false;
        e.reserved = true;
        if let Some(locked) = locked {
            self.entities.get_mut(&locked).unwrap().reserved = true;
        }
        self.players.get_mut(&id).unwrap().pending = true;
        let mut plan = self.plan(id, items, None);
        plan.unlock = Some(PortalPlan { locked, opened });
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
    fn remaining_taunt(&self, remaining: &BTreeSet<i32>) -> Option<(u16, usize, String)> {
        let mut families: BTreeMap<u16, BTreeSet<i32>> = BTreeMap::new();
        for e in self.actors().filter(|e| e.hp > 0) {
            let family = match e.kind {
                0x91b | 0x91c => 0x91b,
                0x91f | 0x920 => 0x91f,
                0x928 | 0x92d => 0x928,
                0x675 | 0x6c1 | 0x6c2 => 0x675,
                kind if HEROES.contains(&kind)
                    || crate::quest::EVENT_BOSSES.contains(&kind)
                    || kind == 0x678 =>
                {
                    kind
                }
                _ => continue,
            };
            let tag = match e.quest {
                Some(tag) => tag,
                None => e.id,
            };
            if remaining.contains(&tag) {
                families.entry(family).or_default().insert(tag);
            }
        }
        let (&kind, ids) = families.iter().choose(&mut rand::thread_rng())?;
        let count = ids.len();
        let text = match (kind, count == 1) {
            (0x91b, false) => format!("I am invincible while my {count} Liches still stand!"),
            (0x91b, true) => "My final Lich will protect me forever!".into(),
            (0x91f, false) => {
                format!("Mortal scum! My {count} Ent Ancients will defend me forever!")
            }
            (0x91f, true) => "My final Ent Ancient will destroy you all!".into(),
            (0x928, false) => {
                format!("My {count} Ghost Kings give me more than enough protection!")
            }
            (0x928, true) => "My final Ghost King is untouchable!".into(),
            (0x675, false) => format!("Maggots! My {count} Phoenix Lords will burn you to ash!"),
            (0x675, true) => "My final Phoenix Lord will never fall!".into(),
            (0x678, false) => format!("You have no hope against my {count} Oasis Giants!"),
            (0x678, true) => "A powerful Oasis Giant still fights for me!".into(),
            (0x668, false) => format!("Fools! There is no escape from my {count} Red Demons!"),
            (0x668, true) => "My final Red Demon is unassailable!".into(),
            (0x67d, false) => format!("Cretins! I have {count} Cyclops Gods to guard me!"),
            (0x67d, true) => "My last Cyclops God will smash you to pieces!".into(),
            (0xd56, false) => format!("Imbeciles! My {count} Skull Shrines make me invincible!"),
            (0xd56, true) => "Pathetic fools! A Skull Shrine guards me!".into(),
            (0xd59, false) => format!("Filthy vermin! My {count} Cube Gods will exterminate you!"),
            (0xd59, true) => "Worthless mortals! A mighty Cube God defends me!".into(),
            (0xd5f, false) => format!("Wretched creatures! {count} Pentaracts remain!"),
            (0xd5f, true) => "Ignorant fools! A Pentaract guards me still!".into(),
            (0xd54, false) => format!(
                "You dull-spirited apes! You shall pose no challenge for {count} Grand Sphinxes!"
            ),
            (0xd54, true) => {
                "Gaze upon the beauty of the Grand Sphinx and feel your last hopes drain away."
                    .into()
            }
            (0xd50, _) => "My Lord of the Lost Lands will make short work of you!".into(),
            (0xd61, _) => {
                "My Hermit God's thousand tentacles shall drag you to a watery grave!".into()
            }
            (0xe37, _) => "My Ghost Ship will send you to a watery grave.".into(),
            _ => unreachable!("unsupported Oryx announcement family {kind:#x}"),
        };
        Some((kind, count, text))
    }

    pub(super) fn oryx_notice(&self, text: &str) {
        self.broadcast_text("#Oryx the Mad God", text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quest;

    #[test]
    fn oryx_announcements_follow_two_minute_counts_with_realm_isolation() {
        use crate::wire::Reader;
        fn listener(d: &mut Directory, world: i32) -> (i32, tokio::sync::mpsc::Receiver<Packet>) {
            let w = d.worlds.get_mut(&world).unwrap();
            let (tx, rx) = tokio::sync::mpsc::channel(1024);
            let (lost, _) = tokio::sync::watch::channel(false);
            let mut ch = super::super::tests::ch(w);
            ch.account = w.players.len() as i32 + 1;
            ch.id = ch.account;
            let id = w.enter(ch, Some(Output { packets: tx, lost })).unwrap();
            (id, rx)
        }
        fn texts(rx: &mut tokio::sync::mpsc::Receiver<Packet>) -> Vec<(String, String)> {
            let mut out = Vec::new();
            while let Ok(packet) = rx.try_recv() {
                if packet.id != wire::TEXT {
                    continue;
                }
                let mut r = Reader::new(&packet.body);
                let name = r.utf().unwrap();
                assert_eq!(r.i32().unwrap(), -1);
                r.i32().unwrap();
                assert_eq!(r.u8().unwrap(), 0);
                assert_eq!(r.utf().unwrap(), "");
                out.push((name, r.utf().unwrap()));
            }
            out
        }
        let mut d = super::super::tests::directory();
        let origin = -3;
        let other = *d.endgame.realms.keys().find(|id| **id != origin).unwrap();
        let (owner, mut a) = listener(&mut d, origin);
        let (_, mut peer) = listener(&mut d, origin);
        let (_, mut b) = listener(&mut d, other);
        let (_, mut nexus) = listener(&mut d, -2);
        let w = d.worlds.get_mut(&origin).unwrap();
        w.event_reserves.clear();
        let pos = w.players[&owner].pos;
        let first = w.spawn_entity(0x668, pos, vec![]).unwrap();
        let last = w.spawn_entity(0x668, pos, vec![]).unwrap();
        w.spawn_entity(0x668, pos, vec![]).unwrap();
        d.endgame.realms.get_mut(&origin).unwrap().cycle.remaining = BTreeSet::from([first, last]);
        let w = d.worlds.get_mut(&other).unwrap();
        let cyclops = w.spawn_entity(0x67d, pos, vec![]).unwrap();
        d.endgame.realms.get_mut(&other).unwrap().cycle.remaining = BTreeSet::from([cyclops]);
        d.tick = 2399;
        d.endgame_step();
        assert!(texts(&mut a).is_empty());
        d.tick = 2400;
        d.endgame_step();
        let plural = vec![(
            "#Oryx the Mad God".into(),
            "Fools! There is no escape from my 2 Red Demons!".into(),
        )];
        assert_eq!(texts(&mut a), plural);
        assert_eq!(texts(&mut peer), plural);
        assert_eq!(
            texts(&mut b),
            vec![(
                "#Oryx the Mad God".into(),
                "My last Cyclops God will smash you to pieces!".into()
            )]
        );
        assert!(texts(&mut nexus).is_empty());
        d.endgame_step();
        assert!(texts(&mut a).is_empty());
        d.worlds
            .get_mut(&origin)
            .unwrap()
            .damage_enemy(owner, first, 100000, false, &[], None)
            .unwrap();
        d.tick = 2401;
        d.endgame_step();
        assert!(texts(&mut a).is_empty());
        d.tick = 4800;
        d.endgame_step();
        assert_eq!(
            texts(&mut a),
            vec![(
                "#Oryx the Mad God".into(),
                "My final Red Demon is unassailable!".into()
            )]
        );
        d.worlds
            .get_mut(&origin)
            .unwrap()
            .damage_enemy(owner, last, 100000, false, &[], None)
            .unwrap();
        d.tick = 4801;
        d.endgame_step();
        assert!(!d.endgame.realms[&origin].cycle.admits());
        assert_eq!(texts(&mut a).len(), 1);
        d.tick = 7200;
        d.endgame_step();
        assert!(texts(&mut a).is_empty());
    }

    #[test]
    fn oryx_counts_deduplicate_transformations_and_do_not_consume_gameplay_rng() {
        use rand::RngCore;
        let mut w = super::super::tests::arena(0);
        let pos = Pos { x: 10.5, y: 10.5 };
        for (family, aliases) in [
            (0x91b, &[0x91b, 0x91c][..]),
            (0x91f, &[0x91f, 0x920][..]),
            (0x928, &[0x928, 0x92d][..]),
            (0x675, &[0x675, 0x6c2, 0x6c1][..]),
            (0x678, &[0x678][..]),
            (0x668, &[0x668][..]),
            (0x67d, &[0x67d][..]),
            (0xd56, &[0xd56][..]),
            (0xd59, &[0xd59][..]),
            (0xd5f, &[0xd5f][..]),
            (0xd54, &[0xd54][..]),
            (0xd50, &[0xd50][..]),
            (0xd61, &[0xd61][..]),
            (0xe37, &[0xe37][..]),
        ] {
            let mut ids = Vec::new();
            for kind in aliases {
                let id = w.spawn_entity(*kind, pos, vec![]).unwrap();
                w.entities.get_mut(&id).unwrap().quest = Some(700);
                ids.push(id);
            }
            let follower = w.spawn_entity(0x676, pos, vec![]).unwrap();
            w.entities.get_mut(&follower).unwrap().quest = Some(700);
            assert!(w.remaining_taunt(&BTreeSet::new()).is_none());
            let mut rng = w.rng.clone();
            for _ in 0..4 {
                let (kind, count, text) = w.remaining_taunt(&BTreeSet::from([700])).unwrap();
                assert_eq!((kind, count), (family, 1));
                assert!(!text.contains('{'));
                if family != 0xd54 {
                    assert!(!text.contains("Sphinx"));
                }
            }
            assert_eq!(w.rng.next_u64(), rng.next_u64());
            for id in ids {
                w.remove_entity(id);
            }
            assert!(w.remaining_taunt(&BTreeSet::from([700])).is_none());
            w.remove_entity(follower);
        }
    }

    #[test]
    fn realm_pool_large_collective_departure_drains_with_bounded_live_grants() {
        fn arrive(d: &mut Directory, route: ForcedTransfer) -> (i32, Character) {
            let destination = route.destination.unwrap();
            let w = d.worlds.get_mut(&-3).unwrap();
            let account = w.players[&route.player].character.account;
            assert!(
                d.consume(account, destination, route.key_time, route.key.clone())
                    .is_err()
            );
            let w = d.worlds.get_mut(&-3).unwrap();
            let ch = w.begin_departure(route.player).unwrap().unwrap();
            w.committed(route.player, Ok(())).unwrap();
            w.finish_departure(route.player).unwrap();
            d.realm_departed(-3, route.player);
            assert_eq!(
                d.consume(account, destination, route.key_time, route.key.clone())
                    .unwrap(),
                ch.id
            );
            assert!(
                d.consume(account, destination, route.key_time, route.key)
                    .is_err()
            );
            (destination, ch)
        }
        for count in [65, 85] {
            let mut d = Directory::load(
                std::path::Path::new("data"),
                Arc::new(Content::load(std::path::Path::new("data")).unwrap()),
                85,
            )
            .unwrap();
            let untouched = d.worlds[&1].hero_identities();
            let mut players = Vec::new();
            let mut receivers = Vec::new();
            for account in 1..=count {
                let w = d.worlds.get_mut(&-3).unwrap();
                let mut ch = super::super::tests::ch(w);
                ch.account = account;
                ch.id = account;
                let (packets, mut rx) = mpsc::channel(64);
                let (lost, _) = watch::channel(false);
                players.push(w.enter(ch, Some(Output { packets, lost })).unwrap());
                while rx.try_recv().is_ok() {}
                receivers.push(rx);
            }
            // This capacity fixture controls the closure stage; it is not an earned boss/SQLite/native proof.
            d.worlds.get_mut(&-3).unwrap().admission_open = false;
            d.endgame.realms.get_mut(&-3).unwrap().cycle.stage = RealmStage::Transferring;
            d.endgame_step();
            let castle = d.endgame.realms[&-3].castle.unwrap();
            let replacement = d.endgame.realms[&-3].replacement.unwrap();
            let mut routes: Vec<_> = players
                .iter()
                .filter_map(|id| d.forced_transfer(*id).unwrap())
                .collect();
            assert_eq!(routes.len(), 64);
            assert_eq!(d.grants.len(), 64);
            assert_eq!(d.endgame.forced.len(), count as usize - 64);
            assert!(d.issue(1000, 1000, -2).is_err());
            for waiting in players.iter().skip(64) {
                let (destination, ch) = arrive(&mut d, routes.pop().unwrap());
                assert_eq!(destination, castle);
                assert!(
                    d.forced_transfer(*waiting).unwrap().is_none(),
                    "consumed but not arrived still reserves a live grant"
                );
                assert_eq!(d.grants.len(), 64);
                assert!(!d.worlds[&-3].players[waiting].departing);
                assert!(d.worlds[&-3].players[waiting].tx.is_some());
                d.worlds
                    .get_mut(&destination)
                    .unwrap()
                    .enter(ch, None)
                    .unwrap();
                let route = d.forced_transfer(*waiting).unwrap().unwrap();
                assert_eq!(route.destination, Some(castle));
                routes.push(route);
                assert_eq!(d.grants.len(), 64);
            }
            for route in routes {
                let (destination, ch) = arrive(&mut d, route);
                assert_eq!(destination, castle);
                d.worlds
                    .get_mut(&destination)
                    .unwrap()
                    .enter(ch, None)
                    .unwrap();
            }
            d.retire_transfer_grants();
            assert!(d.grants.is_empty() && d.endgame.forced.is_empty());
            assert!(d.worlds[&-3].players.is_empty());
            assert_eq!(d.worlds[&castle].players.len(), count as usize);
            assert!(d.endgame.realms[&-3].pending_transfers.is_empty());
            assert!(!d.worlds[&replacement].admission_open);
            assert_eq!(d.worlds[&1].hero_identities(), untouched);
            assert!(d.worlds[&1].admission_open);
            d.tick += 39;
            d.endgame_step();
            assert!(!d.worlds[&replacement].admission_open);
            d.tick += 1;
            d.endgame_step();
            assert!(d.worlds[&replacement].admission_open);
        }
    }

    #[test]
    fn realm_pool_collective_grant_budget_queues_without_starving_parties() {
        let mut d = super::super::tests::directory();
        d.worlds.get_mut(&-2).unwrap().max_players = 85;
        let mut receivers = Vec::new();
        for (origin, account) in [(-3, 1), (-3, 2), (1, 3), (1, 4)] {
            let w = d.worlds.get_mut(&origin).unwrap();
            let mut ch = super::super::tests::ch(w);
            ch.account = account;
            ch.id = account;
            let (packets, rx) = mpsc::channel(64);
            let (lost, _) = watch::channel(false);
            w.enter(ch, Some(Output { packets, lost })).unwrap();
            receivers.push(rx);
        }
        for account in 100..164 {
            d.issue(account, account, -2).unwrap();
        }
        for origin in [-3, 1] {
            d.worlds.get_mut(&origin).unwrap().admission_open = false;
            d.endgame.realms.get_mut(&origin).unwrap().cycle.stage = RealmStage::Transferring;
        }
        d.endgame_step();
        for origin in [-3, 1] {
            assert!(d.endgame.realms[&origin].castle.is_some());
            assert!(d.endgame.realms[&origin].replacement.is_some());
            assert_eq!(d.endgame.realms[&origin].pending_transfers.len(), 2);
        }
        assert_eq!(d.endgame.forced.len(), 4);
        for player in d.forced_transfer_candidates() {
            assert!(d.forced_transfer(player).unwrap().is_none());
        }
        assert_eq!(d.grants.len(), 64);
    }

    #[test]
    fn realm_events_wait_for_qualifying_quest_defeats() {
        let mut d = super::super::tests::directory();
        let origin = -3;
        let w = d.worlds.get_mut(&origin).unwrap();
        let events: Vec<_> = w
            .entities
            .values()
            .filter(|e| quest::EVENT_BOSSES.contains(&e.kind))
            .map(|e| e.kind)
            .collect();
        assert!(events.is_empty(), "unearned startup events: {events:?}");
        assert!(w.event_reserves.len() >= 5);
        let crystal = w.entities.values().find(|e| e.kind == 0x935).unwrap().pos;
        for event in &w.event_reserves {
            for e in std::iter::once(&event.root).chain(&event.children) {
                if !matches!(event.root.kind, 0xd50 | 0xd54) {
                    assert!(
                        e.pos.distance(crystal) >= 32.,
                        "event reservation changed crystal spacing"
                    );
                }
                assert!(!w.entities.contains_key(&e.id));
                assert!(!w.actor_ids.contains(&e.id));
                assert!(!w.npc_global_ids.contains(&e.id));
                assert!(!w.timed_ids.contains(&e.id));
                assert!(w.quests_by_level.values().all(|ids| !ids.contains(&e.id)));
                assert!(!d.endgame.realms[&origin].cycle.remaining.contains(&e.id));
            }
        }
        let held = w.event_reserves.remove(
            w.event_reserves
                .iter()
                .position(|e| e.root.kind == 0xd59)
                .unwrap(),
        );
        let held_id = held.root.id;
        let total = w.event_reserves.len();
        let pos = Pos {
            x: w.map.spawns[0].0,
            y: w.map.spawns[0].1,
        };
        let owner = w.enter(super::super::tests::ch(w), None).unwrap();
        let admin = w.spawn_entity(0x668, pos, vec![]).unwrap();
        w.remove_entity(admin);
        let wandering = w.spawn_entity(0x604, pos, vec![]).unwrap();
        w.damage_enemy(owner, wandering, 100000, false, &[], None)
            .unwrap();
        d.endgame_step();
        assert_eq!(d.worlds[&origin].event_reserves.len(), total);
        let mut winning_seed = None;
        let mut misses = 0;
        let mut born = BTreeSet::new();
        for seed in 0..128 {
            let w = d.worlds.get_mut(&origin).unwrap();
            let source = w.spawn_entity(0x668, pos, vec![]).unwrap();
            d.endgame
                .realms
                .get_mut(&origin)
                .unwrap()
                .cycle
                .remaining
                .insert(source);
            w.damage_enemy(owner, source, 1, false, &[], None).unwrap();
            assert!(w.critical_deaths.is_empty());
            w.damage_enemy(owner, source, 100000, false, &[], None)
                .unwrap();
            w.damage_enemy(owner, source, 100000, false, &[], None)
                .unwrap();
            assert_eq!(w.critical_deaths, vec![source]);
            let before = w.event_reserves.len();
            w.rng = rand::rngs::StdRng::seed_from_u64(seed);
            d.endgame_step();
            let w = d.worlds.get_mut(&origin).unwrap();
            assert!(before - w.event_reserves.len() <= 1);
            if w.event_reserves.len() < before {
                winning_seed = Some(seed);
            } else {
                misses += 1;
            }
            for e in w
                .entities
                .values()
                .filter(|e| quest::EVENT_BOSSES.contains(&e.kind))
            {
                born.insert(e.id);
                assert_eq!(e.hp, w.content.objects[&e.kind].hp);
                assert!(d.endgame.realms[&origin].cycle.remaining.contains(&e.id));
            }
            let after = w.event_reserves.len();
            w.critical_deaths.push(source);
            d.endgame_step();
            let w = d.worlds.get_mut(&origin).unwrap();
            assert_eq!(
                w.event_reserves.len(),
                after,
                "duplicate credit rerolled admission"
            );
            for id in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
                w.remove_entity(id);
            }
            if after == 0 {
                break;
            }
        }
        assert_eq!(born.len(), total);
        assert!(misses > 0);
        let w = d.worlds.get_mut(&origin).unwrap();
        for e in w
            .entities
            .values()
            .filter(|e| e.quest.is_some_and(|id| born.contains(&id)))
        {
            assert!(w.actor_ids.contains(&e.id));
            assert!(w.npc_global_ids.contains(&e.id));
            let cell = e.pos.y as usize * w.map.width as usize + e.pos.x as usize;
            assert!(w.cells.ids(cell).contains(&e.id));
            if let Some(parent) = *e.parent {
                assert!(w.entities.contains_key(&parent));
                assert!(w.timed_ids.contains(&e.id));
            }
        }
        let source = w.spawn_entity(0x668, pos, vec![]).unwrap();
        d.endgame.realms.get_mut(&origin).unwrap().cycle.remaining = BTreeSet::from([source]);
        w.players.get_mut(&owner).unwrap().pos = held.root.pos;
        let next_aux = held.root.next_aux;
        assert!(held.children.is_empty());
        w.event_reserves.push(held);
        w.now = 10000;
        d.tick = 200;
        w.damage_enemy(owner, source, 100000, false, &[], None)
            .unwrap();
        w.rng = rand::rngs::StdRng::seed_from_u64(winning_seed.unwrap());
        d.endgame_step();
        assert_eq!(d.worlds[&origin].event_waiting, Some(source));
        assert_eq!(
            d.endgame.realms[&origin].cycle.stage,
            crate::endgame::RealmStage::Open
        );
        let mut rng = d.worlds[&origin].rng.clone();
        let next = rng.r#gen::<u64>();
        for _ in 0..3 {
            d.endgame_step();
        }
        assert_eq!(
            d.worlds[&origin].rng.clone().r#gen::<u64>(),
            next,
            "blocked earned summon rerolled"
        );
        d.worlds
            .get_mut(&origin)
            .unwrap()
            .players
            .get_mut(&owner)
            .unwrap()
            .pos = pos;
        let w = d.worlds.get_mut(&origin).unwrap();
        let occupied = w
            .actors()
            .filter(|e| e.parent.is_some() || e.quest.is_some())
            .count()
            + w.named_births.len();
        let mut fillers: Vec<_> = (occupied..256)
            .map(|_| {
                let id = w.spawn_entity(0xd57, pos, vec![]).unwrap();
                w.entities.get_mut(&id).unwrap().quest = Some(id);
                id
            })
            .collect();
        d.endgame_step();
        assert_eq!(d.worlds[&origin].event_waiting, Some(source));
        assert!(!d.worlds[&origin].entities.contains_key(&held_id));
        let w = d.worlds.get_mut(&origin).unwrap();
        w.remove_entity(fillers.pop().unwrap());
        let kage = w
            .spawn_entity(crate::dungeon::manor::KAGE, pos, vec![])
            .unwrap();
        w.entities.get_mut(&kage).unwrap().stage = Some(2);
        assert_eq!(w.summon_occupancy(), 255);
        w.manor_entry_mechanics(kage, Some(pos), 0).unwrap();
        assert_eq!(w.manor_births.len(), 1);
        assert_eq!(w.summon_occupancy(), 256);
        let mut rng = w.rng.clone();
        let next = rng.r#gen::<u64>();
        for _ in 0..3 {
            d.endgame_step();
            assert_eq!(d.worlds[&origin].event_waiting, Some(source));
            assert!(!d.worlds[&origin].entities.contains_key(&held_id));
            assert_eq!(d.worlds[&origin].rng.clone().r#gen::<u64>(), next);
        }
        let w = d.worlds.get_mut(&origin).unwrap();
        w.now = 11499;
        w.manor_birth_step();
        assert_eq!(w.manor_births.len(), 1);
        assert!(!w.actors().any(|e| e.parent == Some(kage)));
        w.now = 11500;
        w.manor_birth_step();
        assert!(w.manor_births.is_empty());
        let mines: Vec<_> = w
            .actors()
            .filter(|e| e.parent == Some(kage))
            .map(|e| e.id)
            .collect();
        assert_eq!(
            mines.len(),
            1,
            "admitted Kage mine must land, not be discarded"
        );
        let mine = mines[0];
        assert_eq!(w.entities[&mine].kind, crate::dungeon::manor::MINE);
        assert_eq!(w.entities[&mine].activated, Some(11500));
        assert_eq!(*w.entities[&mine].expires, Some(15550));
        assert_eq!(w.summon_occupancy(), 256);
        d.tick = 230;
        d.endgame_step();
        assert_eq!(d.worlds[&origin].event_waiting, Some(source));
        assert!(!d.worlds[&origin].entities.contains_key(&held_id));
        assert!(d.worlds[&origin].entities.contains_key(&mine));
        assert_eq!(d.worlds[&origin].rng.clone().r#gen::<u64>(), next);
        for id in fillers {
            d.worlds.get_mut(&origin).unwrap().remove_entity(id);
        }
        d.endgame_step();
        let w = &d.worlds[&origin];
        assert_eq!(w.event_waiting, None);
        assert_eq!(w.entities[&held_id].next_aux, 11500 + next_aux);
        assert!(w.event_reserves.is_empty());
        assert!(w.entities.contains_key(&mine));
        assert!(w.actor_ids.contains(&held_id));
        assert!(w.quests_by_level.values().any(|ids| ids.contains(&held_id)));
        assert_eq!(
            d.endgame.realms[&origin].cycle.remaining,
            BTreeSet::from([held_id])
        );
        assert_eq!(
            d.endgame.realms[&origin].cycle.stage,
            crate::endgame::RealmStage::Open
        );
        d.endgame_step();
        assert_eq!(
            d.endgame.realms[&origin].cycle.remaining,
            BTreeSet::from([held_id])
        );
        assert!(d.worlds[&origin].event_reserves.is_empty());
        let w = d.worlds.get_mut(&origin).unwrap();
        w.damage_enemy(owner, held_id, 100000, false, &[], None)
            .unwrap();
        d.endgame_step();
        assert!(matches!(
            d.endgame.realms[&origin].cycle.stage,
            crate::endgame::RealmStage::Closing { deadline: 131500 }
        ));
        assert!(!d.worlds[&origin].admission_open);
    }
}
