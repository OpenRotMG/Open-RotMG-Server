use super::*;

pub(super) const NAMES: &[&str] = &[
    "Medusa", "Beholder", "Cyclops", "Djinn", "Ogre", "Blob", "Sprite", "Demon", "Golem", "Flayer",
    "Spider", "Imp",
];

impl Directory {
    pub(super) fn sync_realm_portal(&mut self) {
        let realm = &self.worlds[&self.endgame.active];
        let count = realm.players.len();
        let limit = realm.max_players;
        let open = realm.admission_open;
        let name = format!("{} ({count}/{limit})", NAMES[self.realm_name]);
        let stats = vec![(37, i32::from(open && count < limit))];
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
