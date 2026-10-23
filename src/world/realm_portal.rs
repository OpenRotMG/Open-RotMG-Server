use super::*;

pub(super) const NAMES: &[&str] = &[
    "Medusa", "Beholder", "Cyclops", "Djinn", "Ogre", "Blob", "Sprite", "Demon", "Golem", "Flayer",
    "Spider", "Imp",
];

impl Directory {
    #[cfg(test)]
    pub(crate) fn prepare_single_realm_fixture(&mut self) -> Result<()> {
        ensure!(
            self.clock.paused(),
            "single Realm fixture requires a paused clock"
        );
        ensure!(
            self.worlds
                .values()
                .all(|w| w.players.is_empty() && w.reserved_slots() == 0),
            "single Realm fixture requires no enrolled players or reservations"
        );
        ensure!(
            self.worlds.get(&-3).is_some_and(|w| w.name == "Realm"),
            "primary fixture Realm missing"
        );
        for (world, record) in &mut self.endgame.realms {
            if *world != -3 {
                record.enabled = false;
                self.worlds
                    .get_mut(world)
                    .context("fixture Realm missing")?
                    .admission_open = false;
            }
        }
        self.sync_realm_portal();
        Ok(())
    }
    pub(crate) fn prepare_realm_capacity(&mut self, limit: usize) -> Result<i32> {
        ensure!(
            self.clock.paused(),
            "capacity setup requires a paused clock"
        );
        ensure!(
            self.worlds
                .values()
                .all(|w| w.players.is_empty() && w.reserved_slots() == 0),
            "capacity setup requires no enrolled players"
        );
        let target = -3;
        let realm = self
            .worlds
            .get_mut(&target)
            .context("active Realm missing")?;
        ensure!(realm.name == "Realm", "active world is not a Realm");
        ensure!(
            limit >= 1 && limit < realm.max_players,
            "capacity must lower the active Realm limit"
        );
        realm.max_players = limit;
        self.sync_realm_portal();
        Ok(target)
    }
    pub(super) fn install_realm_portal(&mut self, slot: usize) -> Result<i32> {
        let nexus = self.worlds.get_mut(&-2).context("Nexus missing")?;
        let mut positions: Vec<Pos> = Vec::new();
        for pos in nexus.map.cells.iter().enumerate().filter_map(|(i, c)| {
            let pos = Pos {
                x: (i % nexus.map.width as usize) as f32 + 0.5,
                y: (i / nexus.map.width as usize) as f32 + 0.5,
            };
            (c.region == 2 && c.object.is_none() && nexus.walkable(pos)).then_some(pos)
        }) {
            if positions.iter().all(|other| other.distance(pos) >= 4.) {
                positions.push(pos);
            }
            if positions.len() > slot {
                break;
            }
        }
        let pos = *positions
            .get(slot)
            .context("Nexus has insufficient separated walkable Realm portal regions")?;
        let id = nexus.spawn_entity(0x712, pos, vec![])?;
        nexus.entities.get_mut(&id).unwrap().published = false;
        Ok(id)
    }
    pub(super) fn sync_realm_portal(&mut self) {
        for (origin, record) in &self.endgame.realms {
            let Some(id) = record.portal else { continue };
            let realm = &self.worlds[origin];
            let count = realm.occupancy();
            let limit = realm.max_players;
            let open = record.enabled && realm.admission_open;
            let name = format!("{} ({count}/{limit})", NAMES[record.name]);
            let stats = vec![(37, i32::from(open && count < limit))];
            let portal = self
                .worlds
                .get_mut(&-2)
                .unwrap()
                .entities
                .get_mut(&id)
                .unwrap();
            if portal.name != name || portal.extra_stats != stats || portal.published != open {
                tracing::info!(realm = origin, %name, open, "Realm portal updated");
                portal.name = name;
                portal.extra_stats = stats;
                portal.published = open;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{ch, directory};
    use super::*;
    fn presentation() -> AccountPresentation {
        AccountPresentation {
            guild: None,
            credits: 100,
            fame: 0,
            best_fame: BTreeMap::new(),
        }
    }

    #[test]
    fn community_social_last_slot_is_reserved_through_consumption_and_atomic_entry() {
        let mut d = directory();
        d.worlds.get_mut(&-3).unwrap().max_players = 1;
        let mut first = ch(&d.worlds[&-3]);
        first.id = 11;
        first.account = 11;
        let mut other = first.clone();
        other.id = 12;
        other.account = 12;
        let (time, key, _) = d.issue(first.account, first.id, -3).unwrap();
        assert!(d.issue(other.account, other.id, -3).is_err());
        assert!(
            d.issue(first.account, first.id, -2).is_err(),
            "duplicate account reservation"
        );
        d.sync_realm_portal();
        assert_eq!(
            d.worlds[&-2].entities[&d.endgame.realms[&-3].portal.unwrap()].extra_stats,
            [(37, 0)]
        );
        assert!(
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .enter(other.clone(), None)
                .is_err(),
            "initial/direct actor cannot steal promised slot"
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let (lost, _) = tokio::sync::watch::channel(false);
        assert!(
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .enter(other.clone(), Some(Output { packets: tx, lost }))
                .is_err(),
            "native initial entry cannot steal promised slot"
        );
        assert!(d.consume(other.account, -3, time, key.clone()).is_err());
        assert_eq!(
            d.consume(first.account, -3, time, key.clone()).unwrap(),
            first.id
        );
        assert!(
            d.consume(first.account, -3, time, key.clone()).is_err(),
            "grant is one use"
        );
        assert!(
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .enter(first.clone(), None)
                .is_err(),
            "consumed grant still needs its session ticket"
        );
        assert!(
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .enter_admitted(other, None, None, presentation(), Some(key.clone()))
                .is_err(),
            "wrong account/character cannot use consumed ticket"
        );
        let id = d
            .worlds
            .get_mut(&-3)
            .unwrap()
            .enter_admitted(first.clone(), None, None, presentation(), Some(key.clone()))
            .unwrap();
        assert_eq!(d.worlds[&-3].players.len(), 1);
        assert_eq!(d.worlds[&-3].reserved_slots(), 0);
        d.worlds.get_mut(&-3).unwrap().leave(id).unwrap();
        assert!(
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .enter_admitted(first, None, None, presentation(), Some(key))
                .is_err(),
            "admission cannot replay after departure"
        );
    }
    #[test]
    fn community_social_unused_cancelled_expired_and_closed_admissions_do_not_bypass_guards() {
        let mut d = directory();
        d.worlds.get_mut(&-3).unwrap().max_players = 1;
        let mut c = ch(&d.worlds[&-3]);
        c.account = 7;
        c.id = 7;
        let (time, key, _) = d.issue(7, 7, -3).unwrap();
        assert!(d.cancel_admission(8, key.clone()).is_err());
        d.cancel_admission(7, key.clone()).unwrap();
        assert!(d.consume(7, -3, time, key).is_err());
        assert_eq!(d.worlds[&-3].reserved_slots(), 0);
        let (time, key, _) = d.issue(7, 7, -3).unwrap();
        d.tick += 2400;
        d.worlds.get_mut(&-3).unwrap().now += 120000;
        assert!(d.consume(7, -3, time, key).is_err());
        assert_eq!(d.worlds[&-3].reserved_slots(), 0);
        let (time, key, _) = d.issue(7, 7, -3).unwrap();
        d.consume(7, -3, time, key.clone()).unwrap();
        d.worlds.get_mut(&-3).unwrap().admission_open = false;
        assert!(
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .enter_admitted(c, None, None, presentation(), Some(key.clone()))
                .is_err()
        );
        d.cancel_admission(7, key).unwrap();
        assert_eq!(d.worlds[&-3].reserved_slots(), 0);
    }
}
