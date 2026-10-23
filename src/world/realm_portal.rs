use super::*;

pub(super) const NAMES: &[&str] = &[
    "Medusa", "Beholder", "Cyclops", "Djinn", "Ogre", "Blob", "Sprite", "Demon", "Golem", "Flayer",
    "Spider", "Imp",
];

impl Directory {
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
