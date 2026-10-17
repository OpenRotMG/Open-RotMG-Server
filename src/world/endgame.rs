use super::*;
use crate::endgame::{CASTLE, CELLAR, HEROES, INCANTATION, LOCKED_CELLAR, RealmCycle, RealmStage};

pub(super) struct RealmDirectory {
    pub active: i32,
    cycles: BTreeMap<i32, RealmCycle>,
    forced: BTreeMap<i32, (i32, i32)>,
    retry: BTreeMap<i32, u64>,
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
        let (template, kind) = if let Some(locked) = locked {
            ensure!(
                w.entities
                    .get(&locked)
                    .is_some_and(|e| e.kind == LOCKED_CELLAR && e.reserved && e.published),
                "locked portal reservation missing"
            );
            (CELLAR, 0x700)
        } else {
            ("pirate-cave", 0x717)
        };
        ensure!(
            w.entities
                .get(&opened)
                .is_some_and(|e| e.kind == kind && e.reserved && !e.published),
            "unpublished portal missing"
        );
        let destination = match self.instance(template, None) {
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
            ensure!(
                self.worlds[&destination].players.is_empty()
                    && !self.grants.values().any(|g| g.destination == destination),
                "unpublished dungeon acquired a participant"
            );
            self.worlds.remove(&destination);
            self.idle.remove(&destination);
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
        self.portals.get_mut(&opened).unwrap().expires = self.tick + portal.lifetime();
        if locked.is_none() {
            let name = w.players[&player].character.name.clone();
            w.broadcast_text("", &format!("Pirate Cave opened by {name}!"));
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
        for (origin, marker, pos) in markers {
            let result = (|| -> Result<()> {
                ensure!(
                    self.worlds[&origin].walkable(pos),
                    "authored Chamber entrance position blocked"
                );
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
                        destination,
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
        self.endgame.forced.retain(|player, (origin, _)| {
            self.worlds
                .get(origin)
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
        ensure!(
            self.worlds.len() + 2 <= 12,
            "no room for collective Castle and replacement Realm"
        );
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
            self.endgame.forced.insert(*player, (origin, castle_id));
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
    pub(super) fn return_to_nexus(&mut self, player: i32, origin: i32) {
        self.endgame.forced.insert(player, (origin, -2));
    }
    #[cfg(test)]
    pub fn forced_transfer_candidates(&self) -> Vec<i32> {
        self.endgame.forced.keys().copied().collect()
    }
    pub fn forced_transfer(&mut self, player: i32) -> Result<Option<ForcedTransfer>> {
        let Some(&(origin, destination)) = self.endgame.forced.get(&player) else {
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
            || p.departing
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
            for e in self.entities.values_mut().filter(|e| e.kind == 0xd7d) {
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
            for e in self.entities.values_mut().filter(|e| e.kind == 0x71d) {
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
            "Wine Cellar requires a defeated Oryx"
        );
        let p = &self.players[&id];
        let locked = self
            .entities
            .values()
            .filter(|e| e.kind == LOCKED_CELLAR && e.published && p.pos.distance(e.pos) <= 3.)
            .min_by(|a, b| p.pos.distance(a.pos).total_cmp(&p.pos.distance(b.pos)))
            .context("no locked Wine Cellar portal nearby")?;
        ensure!(
            !locked.reserved,
            "another Incantation already reserves this portal"
        );
        let (locked, pos) = (locked.id, locked.pos);
        self.reserve_portal(id, slot, 0x700, Some(locked), pos)
    }
    pub(super) fn begin_create(
        &mut self,
        id: i32,
        slot: Slot,
        item: &crate::content::Object,
    ) -> Result<InventoryPlan> {
        ensure!(
            item.kind == 0x705 && item.activations == 1,
            "This dungeon key is not supported yet."
        );
        let portal = self.content.get(0x717)?;
        ensure!(
            portal.class == "Portal" && item.create_object.as_deref() == Some(portal.name.as_str()),
            "Pirate Cave key activation contradicts selected portal"
        );
        self.reserve_portal(id, slot, 0x717, None, self.players[&id].pos)
    }
    fn reserve_portal(
        &mut self,
        id: i32,
        slot: Slot,
        kind: u16,
        locked: Option<i32>,
        pos: Pos,
    ) -> Result<InventoryPlan> {
        let name = crate::dungeon::DUNGEONS
            .iter()
            .chain(crate::endgame::DUNGEONS)
            .find(|(portal, _, _, _)| *portal == kind)
            .context("consumable dungeon registration missing")?
            .2
            .to_owned();
        let mut items = self.players[&id].character.items.clone();
        items[slot.slot] = -1;
        let opened = self.spawn_entity(kind, pos, vec![])?;
        let e = self.entities.get_mut(&opened).unwrap();
        e.name = name;
        e.published = false;
        e.reserved = true;
        if let Some(locked) = locked {
            self.entities.get_mut(&locked).unwrap().reserved = true;
        }
        self.players.get_mut(&id).unwrap().pending = true;
        let mut plan = self.plan(id, items, None, 0);
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

#[cfg(test)]
mod tests {
    use super::*;
    fn recipient(d: &mut Directory, account: i32) -> (i32, mpsc::Receiver<Packet>) {
        let w = d.worlds.get_mut(&-3).unwrap();
        let mut ch = super::super::tests::ch(w);
        ch.account = account;
        ch.id = account;
        let (packets, mut rx) = mpsc::channel(64);
        let (lost, _) = watch::channel(false);
        let player = w.enter(ch, Some(Output { packets, lost })).unwrap();
        while rx.try_recv().is_ok() {}
        (player, rx)
    }
    fn arm(d: &mut Directory) -> BTreeSet<i32> {
        d.endgame.cycles[&-3].remaining.clone()
    }
    fn defeat(d: &mut Directory, heroes: BTreeSet<i32>) {
        let w = d.worlds.get_mut(&-3).unwrap();
        for id in heroes {
            w.remove_entity(id);
            w.critical_deaths.push(id);
        }
        d.endgame_step();
    }
    #[test]
    fn final_named_deaths_remove_only_original_bound_closure_ids_once() {
        let mut d = super::super::tests::directory();
        let originals = d.endgame.cycles[&-3].remaining.clone();
        assert_eq!(originals.len(), 13);
        let saved_cycle = d.endgame.cycles.remove(&-3).unwrap();
        let w = d.worlds.get_mut(&-3).unwrap();
        let pos = w.entities[originals.iter().next().unwrap()].pos;
        let forged = w.spawn_entity(0x91f, pos, vec![]).unwrap();
        assert!(d.bind_realm_heroes(-3, BTreeSet::from([forged])).is_err());
        d.worlds.get_mut(&-3).unwrap().remove_entity(forged);
        d.endgame.cycles.insert(-3, saved_cycle);
        for kind in crate::quest::ROOTS {
            let w = d.worlds.get_mut(&-3).unwrap();
            let tag = w.entities.values().find(|e| e.kind == kind).unwrap().id;
            let final_kind = match kind {
                0x91f => 0x920,
                0x91b => 0x91c,
                0x928 => 0x92d,
                _ => 0x6c1,
            };
            let final_id = w.quest_transform(tag, final_kind).unwrap();
            assert!(w.critical_deaths.is_empty());
            w.defeat_entity(final_id).unwrap();
            assert_eq!(w.critical_deaths, vec![tag]);
            d.endgame_step();
            assert!(!d.endgame.cycles[&-3].remaining.contains(&tag));
            assert!(d.worlds[&-3].admission_open);
            assert!(d.worlds[&-3].critical_deaths.is_empty());
        }
        assert_eq!(d.endgame.cycles[&-3].remaining.len(), 9);
    }
    #[test]
    fn finite_closure_seals_both_admission_paths_and_replaces_without_erasing_owned_players() {
        let mut d = super::super::tests::directory();
        let (a, mut ar) = recipient(&mut d, 1);
        let (b, _br) = recipient(&mut d, 2);
        let (time, key, _) = d.issue(3, 3, -3).unwrap();
        let heroes = arm(&mut d);
        assert!(d.bind_realm_heroes(-3, heroes.clone()).is_err());
        defeat(&mut d, heroes);
        assert!(!d.worlds[&-3].admission_open);
        assert!(d.initial(-3).is_err());
        assert!(d.consume(3, -3, time, key).is_err());
        let ch = super::super::tests::ch(&d.worlds[&-3]);
        assert!(d.worlds.get_mut(&-3).unwrap().enter(ch, None).is_err());
        assert_eq!(ar.try_recv().unwrap().id, wire::TEXT);
        d.tick = 1200;
        d.endgame_step();
        assert!(matches!(
            d.endgame.cycles[&-3].stage,
            RealmStage::Sealed { .. }
        ));
        d.tick = 1640;
        d.endgame_step();
        let realm = d.initial(-3).unwrap();
        assert!(realm > 0 && realm != -3);
        d.sync_realm_portal();
        let portal = &d.worlds[&-2].entities[&d.realm_portal];
        assert_eq!(portal.name, "Beholder (0/16)");
        assert!(portal.published);
        assert_eq!(portal.extra_stats, [(37, 1)]);
        let a_output = d.worlds[&-3].players[&a]
            .tx
            .as_ref()
            .unwrap()
            .packets
            .clone();
        let b_output = d.worlds[&-3].players[&b]
            .tx
            .as_ref()
            .unwrap()
            .packets
            .clone();
        assert_eq!(d.location(a).unwrap(), -3);
        assert_eq!(d.location(b).unwrap(), -3);
        let transfers: Vec<_> = d
            .forced_transfer_candidates()
            .into_iter()
            .filter_map(|id| d.forced_transfer(id).unwrap())
            .collect();
        assert_eq!(transfers.len(), 2);
        assert!(
            transfers
                .iter()
                .find(|t| t.player == a)
                .unwrap()
                .output
                .packets
                .same_channel(&a_output)
        );
        assert!(
            transfers
                .iter()
                .find(|t| t.player == b)
                .unwrap()
                .output
                .packets
                .same_channel(&b_output)
        );
        assert_eq!(transfers[0].destination, transfers[1].destination);
        assert_eq!(d.worlds[&transfers[0].destination].name, "Oryx's Castle");
        assert!(transfers.iter().all(|t| t.packet().id == 39));
        assert!(
            d.worlds[&-3]
                .players
                .values()
                .all(|p| p.departing && p.tx.is_none())
        );
        assert!(d.forced_transfer_candidates().is_empty());
        assert_eq!(d.worlds[&-3].players.len(), 2);
    }
    fn cellar_fixture() -> (Directory, i32, i32, i32) {
        let mut d = super::super::tests::directory();
        let (a, _) = recipient(&mut d, 1);
        let (b, _) = recipient(&mut d, 2);
        let w = d.worlds.get_mut(&-3).unwrap();
        w.name = "Oryx's Chamber".into();
        w.cleared = true;
        let pos = w.players[&a].pos;
        w.players.get_mut(&b).unwrap().pos = pos;
        for id in [a, b] {
            w.players.get_mut(&id).unwrap().character.items[4] = i32::from(INCANTATION);
        }
        let locked = w.spawn_entity(LOCKED_CELLAR, pos, vec![]).unwrap();
        (d, a, b, locked)
    }
    fn incantation(w: &mut World, id: i32) -> Result<InventoryPlan> {
        let slot = Slot {
            object: id,
            slot: 4,
            kind: i32::from(INCANTATION),
        };
        w.use_item(id, slot, w.players[&id].pos)?
            .context("Incantation did not return a commit plan")
    }
    #[test]
    fn incantation_race_publishes_one_bound_cellar_only_after_commit() {
        let (mut d, a, b, locked) = cellar_fixture();
        let w = d.worlds.get_mut(&-3).unwrap();
        let plan = incantation(w, a).unwrap();
        let opened = plan.unlock.unwrap().opened;
        assert!(incantation(w, b).is_err());
        assert_eq!(w.players[&a].character.items[4], i32::from(INCANTATION));
        assert!(w.finish_inventory(plan.clone(), Ok(1)).is_err());
        let plan = d.stage_unlock(plan).unwrap();
        assert!(d.stage_unlock(plan.clone()).is_err());
        assert!(d.destination(b, Some(opened)).is_err());
        let cellar = d.portals[&opened].destination;
        assert_eq!(d.worlds[&cellar].name, "Wine Cellar");
        let replay = plan.clone();
        d.finish_inventory(plan, Ok(1)).unwrap();
        assert!(d.finish_inventory(replay, Ok(1)).is_err());
        assert!(!d.worlds[&-3].entities.contains_key(&locked));
        assert_eq!(d.worlds[&-3].players[&a].character.items[4], -1);
        assert_eq!(
            d.worlds[&-3].players[&b].character.items[4],
            i32::from(INCANTATION)
        );
        assert_eq!(d.destination(a, Some(opened)).unwrap().2, cellar);
        assert_eq!(d.destination(b, Some(opened)).unwrap().2, cellar);
        assert!(incantation(d.worlds.get_mut(&-3).unwrap(), b).is_err());
    }
    #[test]
    fn locked_portal_survives_empty_inventory_until_deadline_and_reservation_release() {
        let (mut d, _, _, locked) = cellar_fixture();
        let w = d.worlds.get_mut(&-3).unwrap();
        w.bag_expiry.insert(locked, w.now + 120_000);
        w.step();
        assert!(
            w.entities.contains_key(&locked),
            "empty portal is not an empty loot bag"
        );
        w.entities.get_mut(&locked).unwrap().reserved = true;
        w.now = 119_950;
        w.step();
        assert!(w.entities.contains_key(&locked));
        w.entities.get_mut(&locked).unwrap().reserved = false;
        w.step();
        assert!(!w.entities.contains_key(&locked));
    }
    #[test]
    fn committed_cellar_portal_survives_ticks_and_has_one_directory_expiry_owner() {
        let (mut d, a, _, _) = cellar_fixture();
        let plan = incantation(d.worlds.get_mut(&-3).unwrap(), a).unwrap();
        let plan = d.stage_unlock(plan).unwrap();
        let opened = plan.unlock.unwrap().opened;
        d.finish_inventory(plan, Ok(1)).unwrap();
        for _ in 0..3 {
            d.step();
        }
        assert!(d.worlds[&-3].entities.contains_key(&opened));
        assert!(!d.worlds[&-3].bag_expiry.contains_key(&opened));
        d.tick = d.portals[&opened].expires - 1;
        d.worlds.get_mut(&-3).unwrap().now += 120_000;
        d.step();
        assert!(!d.worlds[&-3].entities.contains_key(&opened));
        assert!(!d.portals.contains_key(&opened));
        d.step();
    }
    #[test]
    fn failed_cellar_save_or_instance_preflight_conserves_keys_and_removes_unpublished_worlds() {
        let (mut d, a, b, locked) = cellar_fixture();
        let plan = incantation(d.worlds.get_mut(&-3).unwrap(), a).unwrap();
        let plan = d.stage_unlock(plan).unwrap();
        let opened = plan.unlock.unwrap().opened;
        let cellar = d.portals[&opened].destination;
        assert!(
            d.finish_inventory(plan, Err(anyhow::anyhow!("fixture stale inventory write")))
                .is_err()
        );
        assert!(!d.worlds.contains_key(&cellar) && !d.portals.contains_key(&opened));
        let w = d.worlds.get_mut(&-3).unwrap();
        assert!(w.entities[&locked].published && !w.entities[&locked].reserved);
        assert!(!w.entities.contains_key(&opened));
        assert_eq!(w.players[&a].character.items[4], i32::from(INCANTATION));
        assert!(w.players[&a].persistence_failure.is_some());
        while d.worlds.len() < 12 {
            d.instance("pirate-cave", None).unwrap();
        }
        let plan = incantation(d.worlds.get_mut(&-3).unwrap(), b).unwrap();
        let opened = plan.unlock.unwrap().opened;
        assert!(d.stage_unlock(plan).is_err());
        let w = &d.worlds[&-3];
        assert_eq!(w.players[&b].character.items[4], i32::from(INCANTATION));
        assert!(!w.players[&b].pending && w.players[&b].persistence_failure.is_none());
        assert!(!w.entities.contains_key(&opened) && !w.entities[&locked].reserved);
    }
    #[test]
    fn chunked_boss_rewards_conserve_every_item_ownership_and_collection() {
        let mut w = super::super::tests::world();
        let mut rng = rand::rngs::mock::StepRng::new(0, 0);
        let rewards: Vec<i32> = crate::loot::rewards(&w.content, crate::endgame::ORYX2, &mut rng)
            .into_iter()
            .map(i32::from)
            .collect();
        assert_eq!(rewards.len(), 13);
        let (x, y) = w.map.spawns[0];
        let pos = Pos { x, y };
        assert!(w.loot_bag(pos, Some(7), rewards.clone()).is_err());
        let existing: BTreeSet<_> = w.entities.keys().copied().collect();
        w.loot_bags(pos, Some(7), &rewards).unwrap();
        let bags: Vec<_> = w
            .entities
            .values()
            .filter(|e| !existing.contains(&e.id))
            .collect();
        assert_eq!(bags.len(), 2);
        assert!(bags.iter().all(|e| {
            e.items.len() == 8
                && e.accessible(7)
                && !e.accessible(8)
                && e.drop_slots
                    == e.items
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| **item >= 0)
                        .map(|(slot, _)| slot)
                        .collect()
        }));
        let items: Vec<_> = bags
            .into_iter()
            .flat_map(|e| e.items.iter().copied().filter(|i| *i >= 0))
            .collect();
        assert_eq!(items, rewards);
    }
    #[test]
    fn pending_save_dead_quarantined_and_disconnected_recipients_do_not_duplicate_transfer() {
        let mut d = super::super::tests::directory();
        let (pending, _pending_rx) = recipient(&mut d, 1);
        let (dead, _) = recipient(&mut d, 2);
        let (failed, _) = recipient(&mut d, 3);
        let (gone, _) = recipient(&mut d, 4);
        let (disconnected, disconnected_rx) = recipient(&mut d, 5);
        drop(disconnected_rx);
        let heroes = arm(&mut d);
        defeat(&mut d, heroes);
        d.tick = 1200;
        d.endgame_step();
        d.tick = 1640;
        d.endgame_step();
        let w = d.worlds.get_mut(&-3).unwrap();
        w.players.get_mut(&pending).unwrap().pending = true;
        w.players.get_mut(&dead).unwrap().character.hp = 0;
        w.poison(failed, "fixture persistence failure".into())
            .unwrap();
        w.players.remove(&gone);
        assert!(
            d.forced_transfer_candidates()
                .into_iter()
                .all(|id| d.forced_transfer(id).unwrap().is_none())
        );
        assert!(d.worlds.contains_key(&-3));
        assert!(d.worlds[&-3].players.contains_key(&disconnected));
        d.worlds
            .get_mut(&-3)
            .unwrap()
            .players
            .get_mut(&pending)
            .unwrap()
            .pending = false;
        let transfers: Vec<_> = d
            .forced_transfer_candidates()
            .into_iter()
            .filter_map(|id| d.forced_transfer(id).unwrap())
            .collect();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].player, pending);
        assert!(d.forced_transfer_candidates().is_empty());
    }
}
