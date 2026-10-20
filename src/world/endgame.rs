use super::*;
use crate::endgame::{CASTLE, CELLAR, HEROES, INCANTATION, LOCKED_CELLAR, RealmCycle, RealmStage};

pub(super) struct RealmDirectory {
    pub active: i32,
    cycles: BTreeMap<i32, RealmCycle>,
    forced: BTreeMap<i32, ForcedRoute>,
    retry: BTreeMap<i32, u64>,
}
#[derive(Clone, Copy)]
struct ForcedRoute {
    origin: i32,
    destination: i32,
    frozen: bool,
}
impl RealmDirectory {
    pub(super) fn references_world(&self, id: i32) -> bool {
        self.active == id
            || self.cycles.contains_key(&id)
            || self
                .forced
                .values()
                .any(|r| r.origin == id || r.destination == id)
    }
}
impl Default for RealmDirectory {
    fn default() -> Self {
        Self {
            active: -3,
            cycles: BTreeMap::new(),
            forced: BTreeMap::new(),
            retry: BTreeMap::new(),
        }
    }
}
pub struct ForcedTransfer {
    pub player: i32,
    pub output: Output,
    pub destination: i32,
    pub key_time: i32,
    pub key: Vec<u8>,
    pub name: String,
}
impl ForcedTransfer {
    pub fn packet(&self) -> Packet {
        let mut o = Writer::new();
        o.utf(&self.name)
            .utf("")
            .i32(2050)
            .i32(self.destination)
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
            !self.endgame.cycles.contains_key(&origin),
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
        self.endgame.cycles.insert(origin, cycle);
        Ok(())
    }
    pub(super) fn endgame_step(&mut self) {
        self.chamber_step();
        let now = self.tick * 50;
        for (origin, world) in &mut self.worlds {
            let deaths = std::mem::take(&mut world.critical_deaths);
            let Some(cycle) = self.endgame.cycles.get_mut(origin) else {
                continue;
            };
            for id in deaths {
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
                            "My minions have failed me! This Realm will close in one minute.",
                        );
                        self.grants.retain(|_, g| g.destination != *origin);
                    }
                }
            }
            if cycle.advance(now) && matches!(cycle.stage, RealmStage::Sealed { .. }) {
                world.oryx_notice(
                    "I have closed this Realm! I will bring you to my Castle in 22 seconds.",
                );
            }
        }
        let transferring: Vec<_> = self
            .endgame
            .cycles
            .iter()
            .filter(|(id, c)| {
                c.stage == RealmStage::Transferring
                    && self.endgame.active == **id
                    && self.endgame.retry.get(id).is_none_or(|t| self.tick >= *t)
            })
            .map(|(id, _)| *id)
            .collect();
        for origin in transferring {
            if let Err(error) = self.replace_realm(origin) {
                self.endgame.retry.insert(origin, self.tick + 100);
                tracing::error!(origin, %error, "Realm transfer/replacement withheld; retry in five seconds");
            }
        }
        self.endgame.forced.retain(|player, route| {
            self.worlds
                .get(&route.origin)
                .is_some_and(|w| w.players.contains_key(player))
        });
        let retired: Vec<_> = self
            .endgame
            .cycles
            .iter()
            .filter(|(id, c)| {
                **id != self.endgame.active
                    && c.stage == RealmStage::Transferring
                    && self.worlds.get(id).is_some_and(|w| w.players.is_empty())
            })
            .map(|(id, _)| *id)
            .collect();
        for origin in retired {
            self.portals.retain(|_, p| p.origin != origin);
            self.grants.retain(|_, g| g.destination != origin);
            self.worlds.remove(&origin);
            self.idle.remove(&origin);
            self.endgame.cycles.remove(&origin);
            self.endgame.retry.remove(&origin);
            tracing::info!(origin, "empty retired Realm removed");
        }
    }
    fn replace_realm(&mut self, origin: i32) -> Result<()> {
        self.reserve_worlds(2, &[origin])?;
        let castle = self.make(CASTLE, "Oryx's Castle", false, None)?;
        let realm = self.make("realm-shore", "Realm", false, None)?;
        let next_cycle = RealmCycle::new(realm.hero_identities())?;
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
            })
            .map(|(id, _)| *id)
            .collect();
        self.worlds.insert(castle_id, castle);
        self.worlds.insert(realm_id, realm);
        self.next_world = next;
        self.idle.insert(castle_id, self.tick);
        self.endgame.active = realm_id;
        self.realm_name = (self.realm_name + 1) % super::realm_portal::NAMES.len();
        self.endgame.cycles.insert(realm_id, next_cycle);
        self.endgame.retry.remove(&origin);
        for player in &players {
            self.endgame.forced.insert(
                *player,
                ForcedRoute {
                    origin,
                    destination: castle_id,
                    frozen: false,
                },
            );
        }
        self.worlds
            .get_mut(&origin)
            .unwrap()
            .oryx_notice("Your fate awaits in my Castle!");
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
                frozen,
            },
        );
    }

    pub fn forced_transfer(&mut self, player: i32) -> Result<Option<ForcedTransfer>> {
        let Some(&ForcedRoute {
            origin,
            destination,
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
        let (key_time, key, name) = self.issue(account, character, destination)?;
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
    fn oryx_notice(&self, text: &str) {
        self.broadcast_text("Oryx the Mad God", text);
    }
}
