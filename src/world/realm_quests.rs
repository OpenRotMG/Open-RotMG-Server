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
                self.set_parent(id, Some(anchor));
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
        let source = crate::terrain::realms::sources(path)
            .unwrap()
            .into_iter()
            .find(|s| s.name() == "classic-world-13-reconstruction")
            .unwrap();
        let sites = source.sites().unwrap();
        let map = source.open(&content).unwrap();
        let mut world = World::in_domain(content, map.clone(), WorldDomain::Realm).unwrap();
        world.realm_layout = Some(source.name().into());
        world.rng = rand::rngs::StdRng::seed_from_u64(595);
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
        world.populate_authored_sites(&sites).unwrap();
        assert!(Arc::ptr_eq(&map, &world.map));
        assert_eq!(world.setpieces.len(), sites.len());
        for site in &sites {
            let actors: Vec<_> = world
                .actors()
                .filter(|e| {
                    e.kind == site.kind
                        && e.pos.x == site.spawn[0] as f32
                        && e.pos.y == site.spawn[1] as f32
                })
                .collect();
            assert_eq!(actors.len(), 1);
            assert_eq!(*actors[0].quest, Some(actors[0].id));
            assert!(world.non_population_ids.contains(&actors[0].id));
        }
        world
            .populate_named_sites(&sites, SitePhase::Landmarks)
            .unwrap();
        assert_eq!(world.actors().filter(|e| e.kind == 0xdc2).count(), 22);
        assert!((4..=6).contains(&world.actors().filter(|e| e.kind == 0x91b).count()));
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
    let directory = Directory::configured(content, 16, 1, Some(1), vec![source], 1).unwrap();
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
