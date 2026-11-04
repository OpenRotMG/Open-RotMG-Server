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
    pub(super) fn next_realm_portal_position(&mut self) -> Result<Pos> {
        let nexus = self.worlds.get(&-2).context("Nexus missing")?;
        let mut reserved = Vec::new();
        for entity in nexus.entities.values().filter(|e| e.kind == 0x712) {
            if self.realm_entrance_reserves_position(entity.id)? {
                reserved.push(entity.pos);
            }
        }
        let available: Vec<_> = self
            .portal_positions
            .iter()
            .copied()
            .filter(|pos| {
                nexus.walkable(*pos) && reserved.iter().all(|other| other.distance(*pos) >= 4.)
            })
            .collect();
        available.choose(&mut self.portal_rng).copied().context(
            "Nexus Realm entrance packing exhausted: all positions occupied, reserved or blocked",
        )
    }
    pub(super) fn install_realm_portal(&mut self, _slot: usize) -> Result<i32> {
        let pos = self.next_realm_portal_position()?;
        let nexus = self.worlds.get_mut(&-2).context("Nexus missing")?;
        let id = nexus.spawn_entity(0x712, pos, vec![])?;
        nexus.entities.get_mut(&id).unwrap().published = false;
        tracing::info!(
            portal = id,
            x = pos.x,
            y = pos.y,
            "Realm entrance position reserved"
        );
        Ok(id)
    }
    pub(super) fn sync_realm_portal(&mut self) {
        for (origin, record) in &self.endgame.realms {
            let Some(id) = record.portal else { continue };
            let realm = self.worlds.summary(origin).expect("Realm metadata missing");
            let count = realm.occupancy;
            let limit = realm.max_players;
            let open = record.enabled && realm.admission_open;
            let name = format!("{} ({count}/{limit})", NAMES[record.name]);
            let stats = vec![(37, i32::from(open && count < limit))];
            let nexus = self.worlds.summary(&-2).expect("Nexus metadata missing");
            let display = nexus
                .portal_displays
                .get(&id)
                .expect("Realm entrance metadata missing");
            if display.0 != name || display.1 != stats || display.2 != open {
                let portal = self
                    .worlds
                    .get_mut(&-2)
                    .expect("Nexus missing")
                    .entities
                    .get_mut(&id)
                    .expect("Realm entrance missing");
                tracing::info!(realm = origin, %name, open, "Realm portal updated");
                portal.name = name;
                portal.extra_stats = stats;
                portal.published = open;
            }
        }
    }
}

impl World {
    pub(super) fn realm_portal_positions(&self) -> Result<Vec<Pos>> {
        // Kithio randomizes region tiles; a fixed separated packing prevents random-greedy starvation below eight.
        // https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/RealmPortalMonitor.cs#L34-L46
        let mut positions: Vec<Pos> = Vec::new();
        for (i, cell) in self.map.cells.iter().enumerate() {
            let pos = Pos {
                x: (i % self.map.width as usize) as f32 + 0.5,
                y: (i / self.map.width as usize) as f32 + 0.5,
            };
            if cell.region == 2
                && cell.object.is_none()
                && self.walkable(pos)
                && positions.iter().all(|other| other.distance(pos) >= 4.)
            {
                positions.push(pos);
            }
        }
        ensure!(
            positions.len() >= 8,
            "Nexus Realm entrance packing has {} positions; at least eight required",
            positions.len()
        );
        Ok(positions)
    }
}

#[cfg(test)]
#[test]
fn realm_portal_counts_reservations_and_retires_closed_entrances() {
    let path = std::path::Path::new("data");
    let content = Arc::new(Content::load(path).unwrap());
    let mut d = Directory::test_with_seed(path, content, 1, 901).unwrap();
    let portal = d.endgame.realms[&-3].portal.unwrap();
    let other = d.endgame.realms[&1].portal.unwrap();
    let names: BTreeSet<_> = d.endgame.realms.values().map(|r| r.name).collect();
    assert_eq!(names.len(), 2);
    assert_eq!(d.worlds[&-2].entities[&portal].extra_stats, vec![(37, 1)]);
    assert!(d.worlds[&-2].entities[&portal].name.ends_with("(0/1)"));
    d.issue(1, 1, -3).unwrap();
    d.sync_realm_portal();
    let entrance = &d.worlds[&-2].entities[&portal];
    assert!(entrance.published);
    assert!(entrance.name.ends_with("(1/1)"));
    assert_eq!(entrance.extra_stats, vec![(37, 0)]);
    assert!(d.worlds[&-2].entities[&other].name.ends_with("(0/1)"));
    assert_eq!(d.worlds[&-2].entities[&other].extra_stats, vec![(37, 1)]);
    assert!(d.issue(2, 2, -3).unwrap_err().to_string().contains("full"));
    d.worlds.get_mut(&-3).unwrap().admission_open = false;
    d.sync_realm_portal();
    // Closed Realms disappear in the primary comparator; LockedPortal is a different client definition.
    // https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/RealmPortalMonitor.cs#L80-L89
    assert!(!d.worlds[&-2].entities[&portal].published);
    assert!(d.worlds[&-2].entities[&other].published);
    assert!(
        d.issue(2, 2, -3)
            .unwrap_err()
            .to_string()
            .contains("closed")
    );
    d.tick += 2400;
    d.retire_transfer_grants();
    d.worlds.get_mut(&-3).unwrap().admission_open = true;
    d.sync_realm_portal();
    let entrance = &d.worlds[&-2].entities[&portal];
    assert!(entrance.published);
    assert!(entrance.name.ends_with("(0/1)"));
    assert_eq!(entrance.extra_stats, vec![(37, 1)]);
}

#[cfg(test)]
#[test]
fn realm_portal_placement_seed_changes_initial_positions() {
    let data = std::path::Path::new("data");
    let content = Arc::new(Content::load(data).unwrap());
    let positions = |seed| {
        let d = Directory::test_with_seed(data, content.clone(), 1, seed).unwrap();
        d.endgame
            .realms
            .values()
            .map(|r| {
                let p = d.worlds[&-2].entities[&r.portal.unwrap()].pos;
                (p.x as i32, p.y as i32)
            })
            .collect::<Vec<_>>()
    };
    let first = positions(1343);
    assert_eq!(first, positions(1343), "placement must be reproducible");
    assert_ne!(
        first,
        positions(1344),
        "placement must not always scan the same first cells"
    );
}

#[cfg(test)]
#[test]
fn realm_portal_placement_reserves_unpublished_entrances_and_reuses_only_released_positions() {
    let data = std::path::Path::new("data");
    let content = Arc::new(Content::load(data).unwrap());
    let mut d = Directory::test_with_seed(data, content, 1, 1343).unwrap();
    let mut portals: Vec<_> = d
        .endgame
        .realms
        .values()
        .map(|r| r.portal.unwrap())
        .collect();
    for _ in 2..8 {
        let portal = d.install_realm_portal(0).unwrap();
        assert!(!d.worlds[&-2].entities[&portal].published);
        let pos = d.worlds[&-2].entities[&portal].pos;
        assert!(
            portals
                .iter()
                .all(|id| d.worlds[&-2].entities[id].pos.distance(pos) >= 4.)
        );
        portals.push(portal);
    }
    assert_eq!(portals.len(), 8);
    for id in portals {
        d.worlds.get_mut(&-2).unwrap().remove_entity(id);
    }
    let realm_random_before = d.realm_rng.clone().r#gen::<u64>();
    let packing = d.portal_positions.clone();
    assert_eq!(
        packing.len(),
        10,
        "the unchanged Nexus supports ten separated reservations"
    );
    for seed in 0..2048 {
        d.portal_rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut allocated = Vec::new();
        for _ in 0..packing.len() {
            let id = d.install_realm_portal(0).unwrap();
            let pos = d.worlds[&-2].entities[&id].pos;
            assert!(packing.iter().any(|p| p.distance(pos) == 0.));
            assert!(
                allocated
                    .iter()
                    .all(|id| d.worlds[&-2].entities[id].pos.distance(pos) >= 4.)
            );
            allocated.push(id);
        }
        assert!(
            d.install_realm_portal(0)
                .unwrap_err()
                .to_string()
                .contains("packing exhausted")
        );
        let released = allocated.pop().unwrap();
        let pos = d.worlds[&-2].entities[&released].pos;
        d.worlds.get_mut(&-2).unwrap().remove_entity(released);
        let replacement = d.install_realm_portal(0).unwrap();
        assert_eq!(d.worlds[&-2].entities[&replacement].pos.distance(pos), 0.);
        allocated.push(replacement);
        for id in allocated {
            d.worlds.get_mut(&-2).unwrap().remove_entity(id);
        }
    }
    assert_eq!(
        d.realm_rng.clone().r#gen::<u64>(),
        realm_random_before,
        "placement must not consume Realm content randomness"
    );
    let nexus = d.worlds.get_mut(&-2).unwrap();
    let blocking_kind = nexus
        .content
        .objects
        .values()
        .find(|o| o.blocks)
        .unwrap()
        .kind;
    let blockers: Vec<_> = packing
        .iter()
        .map(|pos| nexus.spawn_entity(blocking_kind, *pos, vec![]).unwrap())
        .collect();
    assert!(
        d.install_realm_portal(0)
            .unwrap_err()
            .to_string()
            .contains("packing exhausted")
    );
    for id in blockers {
        d.worlds.get_mut(&-2).unwrap().remove_entity(id);
    }
    assert!(d.install_realm_portal(0).is_ok());
    let nexus = d.worlds.get_mut(&-2).unwrap();
    for cell in &mut Arc::make_mut(&mut nexus.map).cells {
        cell.region = 0;
    }
    assert!(
        nexus
            .realm_portal_positions()
            .unwrap_err()
            .to_string()
            .contains("at least eight required")
    );
}
