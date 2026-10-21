use super::*;

pub(super) const NAMES: &[&str] = &[
    "Medusa", "Beholder", "Cyclops", "Djinn", "Ogre", "Blob", "Sprite", "Demon", "Golem", "Flayer",
    "Spider", "Imp",
];

impl Directory {
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
        let target = self.endgame.active;
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
    pub(super) fn sync_realm_portal(&mut self) {
        let realm = &self.worlds[&self.endgame.active];
        let count = realm.players.len();
        let limit = realm.max_players;
        let open = realm.admission_open;
        let name = format!("{} ({count}/{limit})", NAMES[self.realm_name]);
        let stats = vec![(
            37,
            i32::from(open && count + realm.reserved_slots() < limit),
        )];
        let portal = self
            .worlds
            .get_mut(&-2)
            .unwrap()
            .entities
            .get_mut(&self.realm_portal)
            .unwrap();
        if portal.name != name || portal.extra_stats != stats || portal.published != open {
            tracing::info!(realm = self.endgame.active, %name, open, "Realm portal updated");
            portal.name = name;
            portal.extra_stats = stats;
            portal.published = open;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{ch, directory, receive};
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
            d.worlds[&-2].entities[&d.realm_portal].extra_stats,
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
    #[test]
    fn realm_portal_tracks_capacity_and_closure() {
        let mut d = directory();
        let portal = d.realm_portal;
        assert_eq!(d.worlds[&-2].entities[&portal].kind, 0x712);
        assert_eq!(d.worlds[&-2].entities[&portal].name, "Medusa (0/16)");
        d.worlds.get_mut(&-3).unwrap().max_players = 1;
        let (player, _rx) = receive(&mut d, -3, 1, crate::content::WIZARD);
        d.sync_realm_portal();
        assert_eq!(d.worlds[&-2].entities[&portal].name, "Medusa (1/1)");
        assert_eq!(d.worlds[&-2].entities[&portal].extra_stats, [(37, 0)]);
        let w = d.worlds.get_mut(&-3).unwrap();
        let mut other = ch(w);
        other.id = 2;
        other.account = 2;
        assert!(w.enter(other, None).is_err());
        w.players.remove(&player);
        d.sync_realm_portal();
        assert_eq!(d.worlds[&-2].entities[&portal].extra_stats, [(37, 1)]);
        d.worlds.get_mut(&-3).unwrap().admission_open = false;
        d.sync_realm_portal();
        assert!(!d.worlds[&-2].entities[&portal].published);
        d.worlds.get_mut(&-3).unwrap().admission_open = true;
        d.sync_realm_portal();
        assert_eq!(d.worlds[&-2].entities[&portal].name, "Medusa (0/1)");
        assert!(d.worlds[&-2].entities[&portal].published);
    }

    #[test]
    fn known_realm_portal_receives_live_name_and_usability_in_native_ticks() {
        let mut d = directory();
        d.worlds.get_mut(&-3).unwrap().max_players = 1;
        let portal = d.realm_portal;
        let pos = d.worlds[&-2].entities[&portal].pos;
        let (observer, mut rx) = receive(&mut d, -2, 1, crate::content::WIZARD);
        let nexus = d.worlds.get_mut(&-2).unwrap();
        nexus.players.get_mut(&observer).unwrap().pos = pos;
        nexus.updates(observer, true).unwrap();
        while rx.try_recv().is_ok() {}
        let (occupant, _rx) = receive(&mut d, -3, 2, crate::content::WIZARD);
        for present in [true, false] {
            if !present {
                d.worlds.get_mut(&-3).unwrap().players.remove(&occupant);
            }
            d.sync_realm_portal();
            let nexus = d.worlds.get_mut(&-2).unwrap();
            nexus.step();
            nexus.step();
            let expected = nexus.stats(portal, observer).unwrap();
            assert!(
                std::iter::from_fn(|| rx.try_recv().ok())
                    .filter(|packet| packet.id == wire::NEW_TICK)
                    .any(|packet| packet.body.windows(expected.len()).any(|b| b == expected))
            );
        }
    }

    #[test]
    fn named_nexus_portal_routes_to_realm_but_return_portals_still_escape() {
        let mut d = directory();
        let portal = d.realm_portal;
        let pos = d.worlds[&-2].entities[&portal].pos;
        let (player, _rx) = receive(&mut d, -2, 1, crate::content::WIZARD);
        d.worlds
            .get_mut(&-2)
            .unwrap()
            .players
            .get_mut(&player)
            .unwrap()
            .pos = pos;
        assert_eq!(d.destination(player, Some(portal)).unwrap().2, -3);
        let (returning, _rx) = receive(&mut d, -3, 2, crate::content::WIZARD);
        let world = d.worlds.get_mut(&-3).unwrap();
        let pos = world.players[&returning].pos;
        let exit = world.spawn_entity(0x712, pos, vec![]).unwrap();
        assert_eq!(d.destination(returning, Some(exit)).unwrap().2, -2);
        assert_eq!(d.destination(returning, None).unwrap().2, -2);
    }
}
