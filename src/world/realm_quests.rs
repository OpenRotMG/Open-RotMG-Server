// Behavior reference (GPL-3.0; data/REALM-SRC-LICENSE): https://github.com/moistosaurus/realm-src/tree/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/db
// EntAncient/Lich/GhostKing/Phoenix/Oasis contracts, credited upstream to NR-Core and Skilly; Rust reconstruction, not official AI.
// Stable encounter identity survives transformations; summon parenthood would delete surviving followers.
use super::*;
use crate::content::game::{NamedSite, SitePhase, SiteRole};

impl World {
    pub(super) fn populate_authored_sites(
        &mut self,
        sites: &[crate::terrain::realms::Site],
    ) -> Result<()> {
        use realm_setpieces::Area;
        let mut areas = Vec::new();
        for site in sites {
            let (ground, scenery) = match site.kind {
                0x91f => (0x46, Some(0x194)),
                0x91b
                    if self.realm_layout.as_deref() == Some("classic-world-13-reconstruction") =>
                {
                    (0xad, Some(0x162))
                }
                0x91b => (0xad, Some(0x160)),
                0xdc2 => (0xc5, Some(0x1f2)),
                0x928 | 0x67d => (0x60, Some(0x120)),
                0x678
                    if matches!(
                        self.realm_layout.as_deref(),
                        Some(
                            "classic-world-10-reconstruction"
                                | "classic-world-11-reconstruction"
                                | "classic-world-13-reconstruction"
                        )
                    ) =>
                {
                    (0xf5, Some(0x18e))
                }
                0x678 => (0x46, Some(0x18e)),
                0x668 => (0xf4, None),
                kind => bail!("unsupported authored quest-site kind {kind:#x}"),
            };
            let [x, y, width, height] = site.bounds;
            let area = Area {
                x,
                y,
                width,
                height,
            };
            let pos = Pos {
                x: site.spawn[0] as f32,
                y: site.spawn[1] as f32,
            };
            ensure!(
                x > 0
                    && y > 0
                    && width > 0
                    && height > 0
                    && x.checked_add(width).is_some_and(|v| v < self.map.width)
                    && y.checked_add(height).is_some_and(|v| v < self.map.height),
                "invalid authored quest-site bounds {:?} in {:?}",
                site.bounds,
                self.realm_layout
            );
            ensure!(
                pos.x >= x as f32
                    && pos.x < (x + width) as f32
                    && pos.y >= y as f32
                    && pos.y < (y + height) as f32
                    && self.walkable(pos)
                    && self
                        .map
                        .cell(pos.x, pos.y)
                        .is_some_and(|c| c.ground == ground
                            && (site.kind != 0x91f || [6, 7].contains(&c.terrain))),
                "blocked or invalid authored quest-site anchor {:?} in {:?}",
                site.spawn,
                self.realm_layout
            );
            ensure!(
                areas
                    .iter()
                    .chain(&self.setpieces)
                    .all(|a| area.separate(a)),
                "overlapping authored quest-site {:?} in {:?}",
                site.bounds,
                self.realm_layout
            );
            ensure!(
                site.chest.is_some() == matches!(site.kind, 0x668 | 0x67d | 0x678 | 0xdc2)
                    && (site.kind != 0xdc2 || site.chest == Some(site.spawn)),
                "authored site {:#x} has incorrect chest metadata",
                site.kind
            );
            if let Some([cx, cy]) = site.chest {
                ensure!(
                    cx >= x
                        && cx < x + width
                        && cy >= y
                        && cy < y + height
                        && self.walkable(Pos {
                            x: cx as f32,
                            y: cy as f32
                        }),
                    "blocked or out-of-site authored chest {:?} in {:?}",
                    site.chest,
                    self.realm_layout
                );
            }
            let count = (y..y + height)
                .flat_map(|yy| (x..x + width).map(move |xx| (xx, yy)))
                .filter(|(xx, yy)| {
                    let cell = &self.map.cells[(yy * self.map.width + xx) as usize];
                    match scenery {
                        Some(kind) => cell.object == Some(kind),
                        None => cell.ground == ground,
                    }
                })
                .count();
            ensure!(
                count == site.support && count > 0,
                "authored quest-site {:#x} {:?} in {:?}: expected {} support cells, found {count}",
                site.kind,
                site.bounds,
                self.realm_layout,
                site.support
            );
            areas.push(area);
        }
        // Enter bosses/chests without repainting; anchors are engineered against hash-bound maps, not recovered: https://github.com/Zolmex/kithio-as3-source/tree/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/setpieces
        for (site, area) in sites.iter().zip(areas) {
            self.setpieces.push(area);
            if site.kind == 0x928 {
                // Authored cells and their shared map stay immutable; exclusion needs only the area.
                self.ghost_towers.push(area);
            }
            let pos = Pos {
                x: site.spawn[0] as f32,
                y: site.spawn[1] as f32,
            };
            let Some(role) = self.xml_game().site_role(site.kind) else {
                continue;
            };
            if site.kind == 0xdc2 {
                self.spawn_landmark_chests(site.kind, pos)?;
            }
            self.spawn_site_actor(site.kind, pos, role, true)?;
            if site.kind != 0xdc2
                && let Some([x, y]) = site.chest
            {
                let chest = Pos {
                    x: x as f32,
                    y: y as f32,
                };
                if let Some(platform) = self
                    .xml_game()
                    .landmark_chests
                    .get(&site.kind)
                    .and_then(|row| row.platform)
                {
                    self.place_chest_platform(chest, platform)?;
                }
                self.spawn_landmark_chests(site.kind, chest)?;
            }
        }
        if self.retains_authored_lava() {
            // Overlay the omitted one-in-five floors without changing maps or encounter RNG: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/setpieces/LavaFissure.cs#L60-L71
            let floor = self.xml_game().landmark_chests[&0x668]
                .platform
                .context("linked Red Demon floor missing")?;
            let mut rng = self.rng.clone();
            let positions: Vec<_> = self
                .map
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| c.ground == 0xf4 && c.object.is_none())
                .filter(|_| rng.gen_ratio(1, 5))
                .map(|(i, _)| Pos {
                    x: (i as i32 % self.map.width) as f32 + 0.5,
                    y: (i as i32 / self.map.width) as f32 + 0.5,
                })
                .filter(|pos| {
                    self.entities_at(*pos)
                        .all(|e| !self.content.objects[&e.kind].static_object)
                })
                .collect();
            for pos in &positions {
                self.place_chest_platform(*pos, floor)?;
            }
            tracing::info!(
                layout = self.realm_layout,
                count = positions.len(),
                "authored lava platforms placed"
            );
        }
        Ok(())
    }

    pub(super) fn populate_named_sites(
        &mut self,
        anchors: &[crate::terrain::realms::Site],
        phase: SitePhase,
    ) -> Result<()> {
        let game = self
            .content
            .game
            .clone()
            .context("named Realm sites lack linked GameData")?;
        let rules: Vec<_> = game
            .realm_rules()?
            .sites
            .iter()
            .filter(|r| r.phase == phase)
            // Rarity rolls precede placement to preserve the established seed stream.
            .filter(|r| r.chance.is_none_or(|(n, d)| self.rng.gen_ratio(n, d)))
            .collect();
        for group in rules.chunk_by(|a, b| a.round_robin && b.round_robin) {
            if group[0].round_robin {
                for round in 0..group.iter().map(|r| r.max).max().unwrap() {
                    for rule in group.iter().filter(|r| round < r.max) {
                        self.allocate_site(rule)?;
                    }
                }
            } else {
                let rule = group[0];
                let anchors = anchors.iter().filter(|a| a.kind == rule.kind).count();
                if rule.anchors_replace_roll && anchors > 0 {
                    continue;
                }
                let requested = if rule.min == rule.max {
                    rule.min
                } else {
                    self.rng.gen_range(rule.min..=rule.max)
                };
                for _ in anchors..requested {
                    self.allocate_site(rule)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn allocate_site(&mut self, rule: &NamedSite) -> Result<()> {
        (|| {
            if let Some(placements) = &rule.island {
                return self.place_hermit_island(rule, placements);
            }
            let pos = self.place_named_setpiece(rule)?;
            if rule.kind == crate::dungeon::jungle::COIL {
                self.spawn_landmark_chests(rule.kind, pos)?;
            }
            let id = self.spawn_site_actor(rule.kind, pos, rule.role, false)?;
            if let Some(anchor) = rule.anchor {
                let anchor = self.spawn_entity(anchor, pos, vec![])?;
                self.set_quest(anchor, Some(id));
                // The linked anchor already owns this site's preplaced child.
                self.entities.get_mut(&id).unwrap().parent.set(
                    Some(anchor),
                    id,
                    &mut self.timed_ids,
                );
            }
            Ok(())
        })()
        .with_context(|| format!("NamedSite {:#x}: {}", rule.kind, rule.evidence.origin))
    }
    fn place_chest_platform(&mut self, pos: Pos, platform: u16) -> Result<()> {
        // The chest tile is always Partial Red Floor, so standing there is not lava: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/setpieces/LavaFissure.cs#L71
        let ground = self
            .map
            .cell(pos.x, pos.y)
            .context("Fissure platform outside map")?
            .ground;
        ensure!(ground == 0xf4, "Red Demon chest is not on Lava Blend");
        ensure!(
            !self.entities_at(pos).any(|e| e.kind == platform),
            "Red Demon chest already has linked platform {platform:#x}"
        );
        self.spawn_entity(
            platform,
            Pos {
                x: pos.x.floor() + 0.5,
                y: pos.y.floor() + 0.5,
            },
            vec![],
        )?;
        Ok(())
    }
    pub(super) fn spawn_site_actor(
        &mut self,
        kind: u16,
        pos: Pos,
        role: SiteRole,
        anchored: bool,
    ) -> Result<i32> {
        let id = self.spawn_entity(kind, pos, vec![])?;
        if matches!(role, SiteRole::Quest | SiteRole::Closure | SiteRole::Event)
            || (anchored && role == SiteRole::Hero)
        {
            self.set_quest(id, Some(id));
        }
        tracing::info!(
            layout = self.realm_layout,
            entity = id,
            kind,
            x = pos.x,
            y = pos.y,
            "linked Realm site placed"
        );
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_red_demon_chest_protects_only_its_tile_without_repainting_lava() {
        let (mut world, player) = super::super::movement_tests::fixture(0xf4, false);
        let original = world.map.clone();
        let pos = world.players[&player].pos;
        let initial = world.players[&player].character.hp;
        world.ground_damage(player, 1000, pos).unwrap();
        let damaged = world.players[&player].character.hp;
        assert!(damaged < initial);
        world
            .populate_authored_sites(&[crate::terrain::realms::Site {
                kind: 0x668,
                bounds: [5, 5, 7, 7],
                spawn: [8, 8],
                chest: Some([8, 8]),
                support: 49,
            }])
            .unwrap();
        assert!(Arc::ptr_eq(&original, &world.map));
        let floor = world.entities_at(pos).find(|e| e.kind == 0x153).unwrap();
        assert_eq!((floor.pos.x, floor.pos.y), (8.5, 8.5));
        let floor = floor.id;
        assert!(world.entities_at(pos).any(|e| e.kind == 0x501));
        world.updates(player, true).unwrap();
        assert!(world.players[&player].known.contains(&floor));
        world.ground_damage(player, 1601, pos).unwrap();
        assert_eq!(world.players[&player].character.hp, damaged);
        world
            .ground_damage(player, 2202, Pos { x: 9.5, y: 8.5 })
            .unwrap();
        assert!(world.players[&player].character.hp < damaged);
        world.realm_layout = Some("classic-world-01-reconstruction".into());
        world.rng = rand::rngs::StdRng::seed_from_u64(7);
        let mut expected_rng = world.rng.clone();
        world.populate_authored_sites(&[]).unwrap();
        assert!(Arc::ptr_eq(&original, &world.map));
        assert_eq!(world.rng.r#gen::<u64>(), expected_rng.r#gen::<u64>());
        assert_eq!(
            world.entities_at(pos).filter(|e| e.kind == 0x153).count(),
            1
        );
        let stone = world
            .entities
            .values()
            .find(|e| e.kind == 0x153 && e.id != floor && e.pos.distance(pos) <= 3.)
            .unwrap()
            .pos;
        let hp = world.players[&player].character.hp;
        world.ground_damage(player, 2803, stone).unwrap();
        assert_eq!(world.players[&player].character.hp, hp);
        let bare = (6..=10)
            .flat_map(|y| {
                (6..=10).map(move |x| Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                })
            })
            .find(|p| p.distance(pos) <= 3. && !world.entities_at(*p).any(|e| e.kind == 0x153))
            .unwrap();
        world.ground_damage(player, 3404, bare).unwrap();
        assert!(world.players[&player].character.hp < hp);
    }

    #[test]
    fn authored_sites_bind_finite_quests_without_repainting_or_duplicate_liches() {
        let path = std::path::Path::new("data");
        let content = Arc::new(crate::world::test_content());
        for source in crate::terrain::realms::sources(path).unwrap() {
            let sites = source.sites().unwrap();
            let expected = match source.name() {
                "classic-world-01-reconstruction" => [5, 7, 8, 6],
                "classic-world-02-reconstruction" => [1, 6, 0, 8],
                "classic-world-03-reconstruction" => [4, 3, 0, 6],
                "classic-world-04-reconstruction" | "classic-world-10-reconstruction" => {
                    [0, 0, 1, 0]
                }
                "classic-world-05-reconstruction"
                | "classic-world-11-reconstruction"
                | "classic-world-13-reconstruction" => [0, 0, 2, 0],
                "classic-world-06-reconstruction"
                | "classic-world-07-reconstruction"
                | "classic-world-08-reconstruction"
                | "classic-world-09-reconstruction"
                | "classic-world-12-reconstruction" => [0; 4],
                "realm-fsod-1" | "realm-fsod-2" | "realm-fsod-3" => [0; 4],
                name => panic!("unreviewed authored-site roster {name}"),
            };
            for (kind, count) in [0x928, 0x67d, 0x678, 0x668].into_iter().zip(expected) {
                assert_eq!(sites.iter().filter(|s| s.kind == kind).count(), count);
            }
            let map = source.open(&content).unwrap();
            let mut world =
                World::in_domain(content.clone(), map.clone(), WorldDomain::Realm).unwrap();
            world.realm_layout = Some(source.name().into());
            world.rng = rand::rngs::StdRng::seed_from_u64(595);
            if !sites.is_empty() {
                let mut invalid = sites.clone();
                invalid.last_mut().unwrap().support += 1;
                assert!(
                    world
                        .populate_authored_sites(&invalid)
                        .unwrap_err()
                        .to_string()
                        .contains("expected")
                );
                assert!(world.setpieces.is_empty());
            }
            if matches!(
                source.name(),
                "classic-world-10-reconstruction" | "classic-world-11-reconstruction"
            ) {
                let mut wrong_source = World::new(content.clone(), map.clone()).unwrap();
                wrong_source.realm_layout = Some("classic-world-05-reconstruction".into());
                assert!(
                    wrong_source
                        .populate_authored_sites(&sites)
                        .unwrap_err()
                        .to_string()
                        .contains("blocked or invalid authored quest-site anchor")
                );
                assert!(wrong_source.setpieces.is_empty());
            }
            if source.name() == "classic-world-13-reconstruction" {
                let mut wrong_source = World::new(content.clone(), map.clone()).unwrap();
                wrong_source.realm_layout = Some("classic-world-05-reconstruction".into());
                for kind in [0x91b, 0x678] {
                    let site = sites.iter().find(|s| s.kind == kind).unwrap().clone();
                    assert!(wrong_source.populate_authored_sites(&[site]).is_err());
                    assert!(wrong_source.setpieces.is_empty());
                }
            }
            world.populate_authored_sites(&sites).unwrap();
            assert!(Arc::ptr_eq(&map, &world.map));
            assert_eq!(world.setpieces.len(), sites.len());
            let authored_liches = sites.iter().filter(|s| s.kind == 0x91b).count();
            assert_eq!(
                authored_liches,
                match source.name() {
                    "classic-world-01-reconstruction" | "classic-world-04-reconstruction" => 6,
                    "classic-world-05-reconstruction"
                    | "classic-world-06-reconstruction"
                    | "classic-world-10-reconstruction" => 5,
                    "classic-world-07-reconstruction" => 10,
                    "classic-world-08-reconstruction" => 6,
                    "classic-world-09-reconstruction" => 3,
                    "classic-world-11-reconstruction" => 9,
                    "classic-world-12-reconstruction" => 8,
                    "classic-world-13-reconstruction" => 4,
                    _ => 0,
                }
            );
            for site in &sites {
                let actors: Vec<_> = world
                    .entities
                    .values()
                    .filter(|e| {
                        e.kind == site.kind
                            && e.pos.x == site.spawn[0] as f32
                            && e.pos.y == site.spawn[1] as f32
                    })
                    .collect();
                assert_eq!(actors.len(), 1);
                let actor = actors[0];
                assert_eq!(*actor.quest, Some(actor.id));
                assert_eq!(
                    world.xml_game().definitions[&site.kind]
                        .admission_role()
                        .unwrap(),
                    crate::content::game::AdmissionRole::Scripted
                );
                assert_eq!(actor.hp, content.objects[&site.kind].hp);
                assert!(world.non_population_ids.contains(&actor.id));
            }
            world
                .populate_named_sites(&sites, SitePhase::Landmarks)
                .unwrap();
            assert_eq!(
                world.actors().filter(|e| e.kind == 0xdc2).count(),
                match source.name() {
                    "classic-world-04-reconstruction" | "classic-world-09-reconstruction" => 16,
                    "classic-world-05-reconstruction" | "classic-world-13-reconstruction" => 22,
                    "classic-world-06-reconstruction" => 14,
                    "classic-world-07-reconstruction" => 21,
                    "classic-world-08-reconstruction" => 11,
                    "classic-world-10-reconstruction" => 17,
                    "classic-world-11-reconstruction" => 25,
                    "classic-world-12-reconstruction" => 8,
                    _ => 2,
                }
            );
            assert!(world.actors().all(|e| !(0x662..=0x667).contains(&e.kind)));
            for (kind, minimum, maximum) in
                [(0x928, 8, 14), (0x668, 3, 4), (0x67d, 4, 6), (0x678, 0, 4)]
            {
                let authored = sites.iter().filter(|s| s.kind == kind).count();
                let total = world.entities.values().filter(|e| e.kind == kind).count();
                assert!(
                    total >= authored.max(minimum) && total <= authored.max(maximum),
                    "{kind:#x}: {total}, authored {authored}"
                );
            }
            for site in &sites {
                if let Some([x, y]) = site.chest {
                    assert!(
                        world.entities.values().any(|e| e.kind == 0x501
                            && e.pos.x == x as f32
                            && e.pos.y == y as f32
                            && (site.kind == 0xdc2 || e.items.iter().any(|item| *item != -1))),
                        "missing authored chest for {:#x} at {x},{y} in {}",
                        site.kind,
                        source.name()
                    );
                    if site.kind == 0x668 {
                        let pos = Pos {
                            x: x as f32,
                            y: y as f32,
                        };
                        assert!(
                            world.entities_at(pos).any(|e| {
                                e.kind == 0x153 && world.content.objects[&e.kind].protects_ground
                            }),
                            "Red Demon chest at {x},{y} in {} has no Partial Red Floor",
                            source.name()
                        );
                    }
                }
            }
            let liches = world.entities.values().filter(|e| e.kind == 0x91b).count();
            assert!((authored_liches.max(4)..=authored_liches.max(6)).contains(&liches));
        }
    }

    #[test]
    fn full_population_preserves_classic_landmarks_lava_and_routes() {
        for (name, side, lava_count, groves, coils, liches) in [
            ("classic-world-01-reconstruction", 2100, 703, 19, 2, 6..=6),
            ("classic-world-02-reconstruction", 2100, 864, 24, 2, 4..=6),
            ("classic-world-03-reconstruction", 2100, 629, 21, 2, 4..=6),
            ("classic-world-04-reconstruction", 2000, 506, 21, 16, 6..=6),
            ("classic-world-05-reconstruction", 2100, 702, 19, 22, 5..=6),
            ("classic-world-06-reconstruction", 2100, 698, 25, 14, 5..=6),
            (
                "classic-world-07-reconstruction",
                2100,
                660,
                20,
                21,
                10..=10,
            ),
            ("classic-world-08-reconstruction", 2000, 581, 17, 11, 6..=6),
            ("classic-world-09-reconstruction", 2100, 426, 14, 16, 4..=6),
            ("classic-world-10-reconstruction", 2000, 460, 16, 17, 5..=6),
            ("classic-world-11-reconstruction", 2100, 577, 20, 25, 9..=9),
            ("classic-world-12-reconstruction", 2100, 635, 31, 8, 8..=8),
            ("classic-world-13-reconstruction", 2000, 634, 23, 22, 4..=6),
        ] {
            let path = std::path::Path::new("data");
            let content = Arc::new(crate::world::test_content());
            let mut sources = crate::terrain::realms::sources(path).unwrap();
            let index = sources.iter().position(|s| s.name() == name).unwrap();
            let seed = (0..256)
                .find(|seed| {
                    rand::rngs::StdRng::seed_from_u64(*seed).gen_range(0..sources.len()) == index
                })
                .unwrap();
            eprintln!("{name} native catalog-selection seed: {seed}");
            let source = sources.swap_remove(index);
            let sites = source.sites().unwrap();
            let original = source.open(&content).unwrap();
            let lava: Vec<_> = original
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| c.ground == 244)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(lava.len(), lava_count);
            let directory =
                Directory::configured(content, 85, seed, Some(seed), vec![source], 2).unwrap();
            for id in [-3, 1] {
                let world = &directory.worlds[&id];
                assert_eq!(world.realm_layout.as_deref(), Some(name));
                assert_eq!((world.map.width, world.map.height), (side, side));
                assert_eq!(world.map.spawns.len(), 12);
                assert_eq!(world.actors().filter(|e| e.kind == 0x91f).count(), groves);
                assert_eq!(world.actors().filter(|e| e.kind == 0xdc2).count(), coils);
                assert!(liches.contains(&world.actors().filter(|e| e.kind == 0x91b).count()));
                assert!(
                    world
                        .actors()
                        .filter(|e| e.population_slot.is_some())
                        .count()
                        > 100
                );
                let width = world.map.width as usize;
                let (x, y) = world.map.spawns[0];
                let origin = y.floor() as usize * width + x.floor() as usize;
                let mut reachable = vec![false; world.map.cells.len()];
                let mut pending = vec![origin];
                reachable[origin] = true;
                while let Some(i) = pending.pop() {
                    let (x, y) = ((i % width) as i32, (i / width) as i32);
                    for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                        let pos = Pos {
                            x: (x + dx) as f32 + 0.5,
                            y: (y + dy) as f32 + 0.5,
                        };
                        if world.walkable(pos) {
                            let n = pos.y as usize * width + pos.x as usize;
                            if !reachable[n] && world.map.cells[n].ground != 244 {
                                reachable[n] = true;
                                pending.push(n);
                            }
                        }
                    }
                }
                for &(x, y) in &world.map.spawns {
                    assert!(reachable[y.floor() as usize * width + x.floor() as usize]);
                }
                for site in &sites {
                    let cell = site.spawn[1] as usize * width + site.spawn[0] as usize;
                    // Red Demons and their chests intentionally stand on Fissure lava, not dry ground.
                    let hazard_approach = site.kind == 0x668
                        && world.map.cells[cell].ground == 244
                        && [(0, -1), (-1, 0), (1, 0), (0, 1)]
                            .into_iter()
                            .any(|(dx, dy)| {
                                (1..=8)
                                    .map(|distance| Pos {
                                        x: (site.spawn[0] + dx * distance) as f32 + 0.5,
                                        y: (site.spawn[1] + dy * distance) as f32 + 0.5,
                                    })
                                    .take_while(|pos| world.walkable(*pos))
                                    .any(|pos| reachable[pos.y as usize * width + pos.x as usize])
                            });
                    assert!(
                        reachable[cell] || hazard_approach,
                        "populated {name} Realm {id} lacks an approach to authored {:#x} at {:?}",
                        site.kind,
                        site.spawn
                    );
                }
                let platforms = lava
                    .iter()
                    .filter(|i| {
                        world
                            .cells
                            .ids(**i)
                            .iter()
                            .any(|id| world.entities[id].kind == 0x153)
                    })
                    .count();
                eprintln!("{name} Realm {id}: {platforms}/{lava_count} lava platforms");
                assert!(
                    (lava_count / 10..=lava_count / 3).contains(&platforms),
                    "{name} Realm {id} lacks scattered lava platforms: {platforms}/{lava_count}"
                );
                for i in &lava {
                    let (a, b) = (&original.cells[*i], &world.map.cells[*i]);
                    assert_eq!(
                        (a.ground, a.object, &a.config, a.terrain, a.region),
                        (b.ground, b.object, &b.config, b.terrain, b.region),
                        "{name} Realm {id} changed lava cell {i}"
                    );
                }
                for site in &sites {
                    let [x, y, width, height] = site.bounds;
                    for yy in y..y + height {
                        for xx in x..x + width {
                            let i = (yy * world.map.width + xx) as usize;
                            let (a, b) = (&original.cells[i], &world.map.cells[i]);
                            assert_eq!(
                                (a.ground, a.object, &a.config, a.terrain, a.region),
                                (b.ground, b.object, &b.config, b.terrain, b.region),
                                "{name} Realm {id} changed landmark cell {xx},{yy}"
                            );
                        }
                    }
                    assert!(world.walkable(Pos {
                        x: site.spawn[0] as f32,
                        y: site.spawn[1] as f32
                    }));
                    assert_eq!(
                        world
                            .actors()
                            .filter(|e| e.kind == site.kind
                                && e.home.x == site.spawn[0] as f32
                                && e.home.y == site.spawn[1] as f32)
                            .count(),
                        1
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[test]
fn ghost_king_towers_keep_authored_cells_and_hold_no_gods() {
    let path = std::path::Path::new("data");
    let content = Arc::new(crate::world::test_content());
    let source = crate::terrain::realms::sources(path)
        .unwrap()
        .into_iter()
        .find(|s| s.name() == "classic-world-01-reconstruction")
        .unwrap();
    let authored = source.open(&content).unwrap();
    let authored_towers: Vec<_> = source
        .sites()
        .unwrap()
        .into_iter()
        .filter(|site| site.kind == 0x928)
        .map(|site| site.bounds)
        .collect();
    assert!(!authored_towers.is_empty());
    let directory = Directory::configured(content, 16, 1, Some(1), vec![source], 2).unwrap();
    let world = &directory.worlds[&-3];
    let kings: Vec<_> = world
        .entities
        .values()
        .filter(|e| e.kind == 0x928)
        .map(|e| e.pos)
        .collect();
    assert!(!kings.is_empty(), "world 1 has no Ghost King");
    assert!(world.ghost_towers.len() >= kings.len());
    assert!(authored_towers.iter().all(|bounds| {
        world
            .ghost_towers
            .iter()
            .any(|area| *bounds == [area.x, area.y, area.width, area.height])
    }));
    assert!(world.ghost_towers.iter().all(|area| {
        let fixed = authored_towers.contains(&[area.x, area.y, area.width, area.height]);
        (area.y..area.y + area.height).all(|y| {
            (area.x..area.x + area.width).all(|x| {
                let pos = Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let cell = world.map.cell(pos.x, pos.y).unwrap();
                if fixed {
                    let original = authored.cell(pos.x, pos.y).unwrap();
                    assert_eq!(
                        (
                            cell.ground,
                            cell.object,
                            &cell.config,
                            cell.terrain,
                            cell.region
                        ),
                        (
                            original.ground,
                            original.object,
                            &original.config,
                            original.terrain,
                            original.region
                        )
                    );
                } else {
                    assert_ne!(cell.terrain, 1);
                }
                !world.family_ground(pos, 1)
            })
        })
    }));
    assert!(
        world
            .entities
            .values()
            .filter(|e| {
                world.content.objects[&e.kind].god && world.content.objects[&e.kind].terrain == 1
            })
            .all(|e| !world.inside_ghost_tower(e.pos))
    );
}
