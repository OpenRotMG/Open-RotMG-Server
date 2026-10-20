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
