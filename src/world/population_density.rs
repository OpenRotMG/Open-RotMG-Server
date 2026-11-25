use super::*;

#[derive(Default)]
pub(super) struct Region {
    pub(super) target: usize,
    sites: Vec<Pos>,
    kinds: Vec<u16>,
}

impl World {
    pub(super) fn realm_rules(&self) -> Result<&crate::content::game::RealmRules> {
        self.xml_game().realm_rules()
    }
    pub(super) fn population_habitat(&self, kind: u16) -> Result<u8> {
        self.xml_game()
            .definitions
            .get(&kind)
            .and_then(|d| d.population.terrain)
            .with_context(|| format!("Realm population {kind:#x} lacks linked Terrain"))
    }
    pub(super) fn population_placement(
        &self,
        kind: u16,
    ) -> Result<(Option<Arc<crate::content::game::CampRule>>, usize)> {
        let definition = self.xml_declared_admission(kind, "ordinary placement")?;
        let camp = self.realm_rules()?.camps.get(&kind).cloned();
        if let Some(camp) = &camp {
            for role in &camp.roles {
                self.xml_raw_admission(role.kind)?;
            }
        }
        let authored = camp.as_ref().map_or(1, |c| c.size());
        // Reserve space for immediate live minions, not empty population slots or per-kind caps.
        let footprint = authored + self.xml_ordinary_footprint(definition);
        Ok((camp, footprint))
    }
    pub(super) fn configure_population(&mut self) -> Result<()> {
        ensure!(
            self.population.is_empty(),
            "cannot reconfigure a live population"
        );
        let density = self.realm_rules()?.density.clone();
        let roots: Vec<_> = self
            .xml_game()
            .definitions
            .values()
            .filter_map(|d| {
                let r = &d.population;
                let terrain = r.terrain?;
                (self.content.objects[&d.kind].enemy
                    && r.weight.is_some_and(|w| w > 0.)
                    && r.per_world_max.is_none())
                .then_some((d.kind, terrain))
            })
            .collect();
        for &(kind, _) in &roots {
            self.xml_raw_admission(kind)?;
        }
        self.population_regions = std::array::from_fn(|_| Region::default());
        let mut area = [0usize; 13];
        for (i, cell) in self.map.cells.iter().enumerate() {
            let terrain = cell.terrain as usize;
            ensure!(terrain < area.len(), "unsupported Realm terrain {terrain}");
            if terrain == 0 {
                continue;
            }
            area[terrain] += 1;
            let pos = Pos {
                x: (i % self.map.width as usize) as f32 + 0.5,
                y: (i / self.map.width as usize) as f32 + 0.5,
            };
            if cell.object.is_none() && self.walkable(pos) && self.outside_setpieces(pos) {
                self.population_regions[terrain].sites.push(pos);
            }
        }
        for (kind, terrain) in roots {
            self.xml_declared_admission(kind, "density population")?;
            self.population_regions[terrain as usize].kinds.push(kind);
        }
        for (terrain, area) in area.into_iter().enumerate().skip(1) {
            let region = &mut self.population_regions[terrain];
            // Only the farmed Godlands need full density; the selected ratio retains the other habitats.
            let divisor = density.divisors[terrain - 1] as usize;
            region.target = if terrain == 1 {
                area / divisor
            } else {
                area * density.numerator as usize / (divisor * density.denominator as usize)
            };
            ensure!(
                region.target == 0 || !region.sites.is_empty() && !region.kinds.is_empty(),
                "terrain {terrain} lacks population sites or species"
            );
            tracing::info!(
                terrain,
                area,
                target = region.target,
                sites = region.sites.len(),
                species = region.kinds.len(),
                "Realm population reconstruction configured"
            );
        }
        Ok(())
    }
    pub(super) fn populate_realm_population(&mut self) -> Result<()> {
        self.configure_population()?;
        for terrain in 1..=12 {
            self.fill_population(terrain)?;
        }
        Ok(())
    }
    pub(super) fn mark_population(&mut self, id: i32, slot: usize) -> Result<()> {
        ensure!(
            self.population[slot].entity == Some(id),
            "population registration lost slot ownership"
        );
        let terrain = self.population_terrain(&self.population[slot]);
        ensure!((1..=12).contains(&terrain), "invalid population origin");
        let entity = self
            .entities
            .get_mut(&id)
            .context("population entity missing")?;
        ensure!(
            entity.population_slot.is_none(),
            "population entity registered twice"
        );
        ensure!(
            entity.quest.is_none(),
            "ordinary population cannot own a scripted quest"
        );
        ensure!(
            self.non_population_ids.remove(&id),
            "population actor missing from allocation index"
        );
        entity.population_slot = Some(slot);
        self.population_counts[terrain as usize] += 1;
        Ok(())
    }
    pub(super) fn place_population_solo(&mut self, kind: u16, pos: Pos) -> Result<i32> {
        let entity = self.spawn_entity(kind, pos, vec![])?;
        let slot = self.population.len();
        self.population.push(Spawn {
            kind,
            pos,
            entity: Some(entity),
            camp: None,
        });
        self.mark_population(entity, slot)?;
        Ok(entity)
    }
    pub(super) fn sample_population_group(&mut self, kind: u16) -> usize {
        match self.xml_game().definitions[&kind].population.group {
            None => 1,
            Some(group) => {
                let u: f64 = self.rng.gen_range(f64::EPSILON..1.);
                let angle: f64 = self.rng.gen_range(0.0..std::f64::consts::TAU);
                let n = group.mean as f64
                    + group.deviation as f64 * (-2. * u.ln()).sqrt() * angle.sin();
                (n as usize).clamp(group.min, group.max)
            }
        }
    }
    fn spawn_population_group(&mut self, terrain: u8, kind: u16) -> Result<usize> {
        self.xml_raw_admission(kind)?;
        let (family, footprint) = self.population_placement(kind)?;
        let roots = self.sample_population_group(kind);
        let center = *self.population_regions[terrain as usize]
            .sites
            .choose(&mut self.rng)
            .context("population has no candidate site")?;
        let mut groups = Vec::with_capacity(roots);
        let mut occupied = Vec::new();
        for root in 0..roots {
            let mut layout = None;
            for _ in 0..if root == 0 { 1 } else { 32 } {
                let pos = if root == 0 {
                    center
                } else {
                    Pos {
                        x: center.x + self.rng.gen_range(-5.0..5.0),
                        y: center.y + self.rng.gen_range(-5.0..5.0),
                    }
                };
                if !self.family_ground(pos, terrain)
                    || occupied.iter().any(|q: &Pos| q.distance(pos) < 0.75)
                {
                    continue;
                }
                let positions = self.family_layout(pos, terrain, footprint, &occupied);
                if let Some(positions) = positions
                    && positions.iter().all(|pos| self.outside_player_view(*pos))
                {
                    layout = Some(positions);
                    break;
                }
            }
            let Some(positions) = layout else {
                return Ok(0);
            };
            occupied.extend(positions.iter().copied());
            groups.push(positions);
        }
        for positions in groups {
            match &family {
                Some(family) => self.place_camp(family.clone(), terrain, positions)?,
                None => {
                    let pos = positions[0];
                    self.place_population_solo(kind, pos)?;
                }
            }
        }
        Ok(roots)
    }
    fn fill_population(&mut self, terrain: u8) -> Result<()> {
        let target = self.population_regions[terrain as usize].target;
        let kinds = self.population_regions[terrain as usize].kinds.clone();
        let needed = target.saturating_sub(self.oryx_counted(terrain).len());
        let (mut roots, mut refused, mut stalled, mut groups) = (0, 0, 0, 0);
        let game = self
            .content
            .game
            .as_ref()
            .context("population requires linked GameData")?
            .clone();
        while roots < needed && stalled < 128 {
            // SpawnProb's original selector is lost; relative XML weights are an explicit reconstruction choice.
            let kind = *kinds
                .choose_weighted(&mut self.rng, |kind| {
                    game.definitions[kind]
                        .population
                        .weight
                        .expect("validated spawn weight") as f32
                })
                .context("invalid population weights")?;
            match self.spawn_population_group(terrain, kind)? {
                0 => {
                    refused += 1;
                    stalled += 1;
                }
                placed => {
                    roots += placed;
                    groups += 1;
                    stalled = 0;
                }
            }
        }
        tracing::info!(
            terrain,
            target,
            needed,
            roots,
            bodies = self.population_count(terrain),
            groups,
            refused,
            "Realm ordinary population allocated"
        );
        ensure!(
            roots >= needed,
            "terrain {terrain} cannot fill population: {roots}/{needed} roots after {stalled} consecutive refusals"
        );
        Ok(())
    }
    fn compact_population(&mut self) -> Result<()> {
        let mut old: Vec<_> = std::mem::take(&mut self.population)
            .into_iter()
            .map(Some)
            .collect();
        let mut slots = vec![usize::MAX; old.len()];
        let mut camps = vec![usize::MAX; self.camps.len()];
        for (index, mut camp) in std::mem::take(&mut self.camps).into_iter().enumerate() {
            let range = camp.start..camp.start + camp.family.size();
            if !range
                .clone()
                .any(|i| old[i].as_ref().expect("camp slot missing").entity.is_some())
            {
                continue;
            }
            let new_camp = self.camps.len();
            camps[index] = new_camp;
            camp.start = self.population.len();
            for i in range {
                let mut slot = old[i].take().context("population camp ranges overlap")?;
                let new_slot = self.population.len();
                slot.camp = Some(new_camp);
                slots[i] = new_slot;
                if let Some(id) = slot.entity {
                    let entity = self
                        .entities
                        .get_mut(&id)
                        .context("live population member missing")?;
                    entity.camp = Some(new_camp);
                    entity.population_slot = Some(new_slot);
                }
                self.population.push(slot);
            }
            self.camps.push(camp);
        }
        for (i, slot) in old.into_iter().enumerate() {
            if let Some(slot) = slot
                && slot.camp.is_none()
                && let Some(id) = slot.entity
            {
                slots[i] = self.population.len();
                self.entities
                    .get_mut(&id)
                    .context("live solo missing")?
                    .population_slot = Some(slots[i]);
                self.population.push(slot);
            }
        }
        Ok(())
    }
    pub(super) fn repopulate(&mut self) -> Result<()> {
        if self.population.is_empty() && self.population_regions.iter().all(|r| r.target == 0) {
            return Ok(());
        }
        if self.now >= self.population_next {
            self.population_next = self.now + 60000;
            self.compact_population()?;
            let recalled = self.recall_godlands_strays();
            // Each terrain only fills or culls its own slots, so one pass serves all twelve checks.
            let counts = self.oryx_counts();
            if !self.godland_tiles.is_empty() {
                // Live: a day-old Realm counted its full 374 Godlands roots, yet farmers crossing 35 of 45 areas met 14 gods in 25 minutes.
                let on = |pos: Pos| self.godland_tiles.contains(&(pos.x as i16, pos.y as i16));
                let counted = self.oryx_counted(1);
                let gods: Vec<_> = self
                    .entities
                    .values()
                    .filter(|e| {
                        self.content
                            .objects
                            .get(&e.kind)
                            .is_some_and(|o| o.god && o.terrain == 1)
                    })
                    .map(|e| e.pos)
                    .collect();
                let mut areas = BTreeMap::new();
                for pos in gods.iter().filter(|pos| on(**pos)) {
                    *areas
                        .entry((pos.x as i32 / 80, pos.y as i32 / 80))
                        .or_insert(0) += 1;
                }
                tracing::info!(
                    target = self.population_regions[1].target,
                    counted = counted.len(),
                    counted_on_godlands = counted
                        .iter()
                        .filter(|id| self.entities.get(id).is_some_and(|e| on(e.pos)))
                        .count(),
                    gods = gods.len(),
                    gods_on_godlands = areas.values().sum::<usize>(),
                    gods_in_player_view = gods
                        .iter()
                        .filter(|pos| !self.outside_player_view(**pos))
                        .count(),
                    recalled,
                    ?areas,
                    "Godlands population census"
                );
            }
            for terrain in 1..=12 {
                let target = self.population_regions[terrain as usize].target;
                // Kithio culls above 150% and refills below 75%: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/Oryx.cs#L298-L357
                let counted = counts[terrain as usize];
                if counted * 2 > target * 3 {
                    self.cull_population(terrain);
                // Owner chose to top up heavily farmed Godlands on every check instead of Kithio's 75%.
                } else if counted * 4 < target * 3 || terrain == 1 && counted < target {
                    self.fill_population(terrain)?;
                }
            }
        }
        self.reproduce_active()?;
        self.repopulate_families()
    }
    // Kithio counts every enemy on the terrain, but a root only summons its children once active.
    // Eager family members therefore count after their leader has been active, or once it is gone.
    fn oryx_counts_slot(&self, i: usize, slot: &Spawn) -> bool {
        slot.camp.is_none_or(|camp| {
            let start = self.camps[camp].start;
            i == start
                || self.population[start]
                    .entity
                    .and_then(|id| self.entities.get(&id))
                    .is_none_or(|leader| leader.activated.is_some())
        })
    }
    pub(super) fn oryx_counted(&self, terrain: u8) -> Vec<i32> {
        self.population
            .iter()
            .enumerate()
            .filter(|(i, slot)| {
                self.population_terrain(slot) == terrain && self.oryx_counts_slot(*i, slot)
            })
            .filter_map(|(_, slot)| slot.entity)
            .collect()
    }
    pub(super) fn oryx_counts(&self) -> [usize; 13] {
        let mut counts = [0; 13];
        for (i, slot) in self.population.iter().enumerate() {
            if slot.entity.is_some() && self.oryx_counts_slot(i, slot) {
                counts[self.population_terrain(slot) as usize] += 1;
            }
        }
        counts
    }
    /// Live: gods lured off the Godlands still filled its quota; two hours into a Realm a third of them stood elsewhere.
    /// A stray leaves once nobody can see it, and the same check restocks the Godlands itself.
    pub(super) fn recall_godlands_strays(&mut self) -> usize {
        let strays: Vec<_> = self
            .oryx_counted(1)
            .into_iter()
            .filter(|id| {
                let e = &self.entities[id];
                self.map
                    .cell(e.pos.x, e.pos.y)
                    .is_none_or(|c| c.terrain != 1)
                    && self.outside_player_view(e.pos)
            })
            .collect();
        for id in &strays {
            self.remove_entity(*id);
        }
        strays.len()
    }
    pub(super) fn cull_population(&mut self, terrain: u8) {
        let mut counted = self.oryx_counted(terrain);
        // Compaction stores families first, so slot order culled every Sprite God; oldest bodies go first instead.
        counted.sort_unstable();
        let excess = counted.len() - self.population_regions[terrain as usize].target;
        // Only bodies without a player within 10 tiles leave, so active areas keep their regrowth.
        // Breeding causes the overgrowth; culling species that cannot breed back would let them die out.
        let rules = &self
            .realm_rules()
            .expect("configured population")
            .reproduction;
        let culled: Vec<_> = counted
            .into_iter()
            .filter(|id| {
                let e = &self.entities[id];
                rules.contains_key(&e.kind)
                    && self.players.values().all(|p| p.pos.distance(e.pos) >= 10.)
            })
            .take(excess)
            .collect();
        for &id in &culled {
            let Some(e) = self.entities.get(&id) else {
                continue;
            };
            // A never-active leader takes its uncounted, not-yet-summoned family with it.
            let family: Vec<_> = match e.camp {
                Some(camp)
                    if e.activated.is_none()
                        && self.camps[camp].start
                            == e.population_slot.expect("population body lacks its slot") =>
                {
                    self.camp_members(camp).map(|e| e.id).collect()
                }
                _ => vec![id],
            };
            for member in family {
                self.remove_entity(member);
            }
        }
        tracing::info!(
            terrain,
            culled = culled.len(),
            counted = self.oryx_counted(terrain).len(),
            bodies = self.population_count(terrain),
            "Realm ordinary population culled"
        );
    }
}

#[cfg(test)]
#[test]
fn gods_lured_off_the_godlands_leave_once_nobody_sees_them() -> Result<()> {
    let (mut world, player) = movement_tests::fixture(0x18, false);
    for cell in &mut Arc::make_mut(&mut world.map).cells {
        cell.terrain = 1;
    }
    let pos = world.players[&player].pos;
    let far = |offset: f32| Pos {
        x: if pos.x < 16. { 30.5 } else { 1.5 },
        y: if pos.y < 16. {
            30.5 - offset
        } else {
            1.5 + offset
        },
    };
    let (lured, home, watched) = (
        far(0.),
        far(4.),
        Pos {
            x: pos.x + 3.,
            y: pos.y,
        },
    );
    assert!(lured.distance(pos) >= PLAYER_VIEW_RADIUS && home.distance(pos) >= PLAYER_VIEW_RADIUS);
    for spot in [lured, home, watched] {
        world.place_population_solo(0x6d8, spot)?;
    }
    for spot in [lured, watched] {
        let i = spot.y as usize * world.map.width as usize + spot.x as usize;
        Arc::make_mut(&mut world.map).cells[i].terrain = 2;
    }
    assert_eq!(world.recall_godlands_strays(), 1);
    let left: Vec<_> = world
        .oryx_counted(1)
        .iter()
        .map(|id| world.entities[id].pos)
        .collect();
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|p| p.distance(lured) > 1.));
    Ok(())
}

#[cfg(test)]
#[test]
fn natural_replenishment_checks_complete_footprints_and_the_view_boundary() {
    for (kind, distance, expected) in [(0x6d8, 19.99, 0), (0x652, 20.5, 0), (0x6d8, 20., 1)] {
        let (mut world, player) = movement_tests::fixture(0x18, false);
        for cell in &mut Arc::make_mut(&mut world.map).cells {
            cell.terrain = 1;
        }
        let pos = world.players[&player].pos;
        world.population_regions[1].sites = vec![Pos {
            x: pos.x + distance,
            y: pos.y,
        }];
        assert_eq!(world.spawn_population_group(1, kind).unwrap(), expected);
        assert_eq!(world.population.len(), expected);
        if expected == 1 {
            let id = world.population[0].entity.unwrap();
            assert!(!world.visible_with(player, None).contains(&id));
        }
    }
}

#[cfg(test)]
#[test]
fn populated_realms_keep_all_arrivals_connected() -> Result<()> {
    let path = std::path::Path::new("data");
    let content = Arc::new(crate::world::test_content());
    // Fixed geography is distinct from variable buildings: https://web.archive.org/web/20130522030032id_/https://forums.wildshadow.com/wiki/realm-mad-god/maps-builds-106-123
    for source in crate::terrain::realms::sources(path)? {
        let name = source.name().to_owned();
        let directory =
            Directory::configured(content.clone(), 16, 785, Some(785), vec![source], 2)?;
        let world = &directory.worlds[&-3];
        let width = world.map.width as usize;
        let passable: Vec<_> = (0..world.map.cells.len())
            .map(|i| {
                world.walkable(Pos {
                    x: (i % width) as f32 + 0.5,
                    y: (i / width) as f32 + 0.5,
                })
            })
            .collect();
        assert_eq!(world.map.spawns.len(), 12, "{name}");
        let index = |x: f32, y: f32| y as usize * width + x as usize;
        let (x, y) = world.map.spawns[0];
        let start = index(x, y);
        let mut distance = vec![u32::MAX; passable.len()];
        distance[start] = 0;
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some(i) = queue.pop_front() {
            for next in [
                i.wrapping_sub(width),
                i + width,
                if i % width > 0 { i - 1 } else { usize::MAX },
                if i % width + 1 < width {
                    i + 1
                } else {
                    usize::MAX
                },
            ] {
                if next < passable.len() && passable[next] && distance[next] == u32::MAX {
                    distance[next] = distance[i] + 1;
                    queue.push_back(next);
                }
            }
        }
        for &(x, y) in &world.map.spawns {
            let i = index(x, y);
            assert!(
                passable[i] && distance[i] != u32::MAX,
                "{name}: arrival {x},{y}"
            );
            assert_eq!(world.map.cells[i].terrain, 11, "{name}: arrival habitat");
            println!("{name} arrival={x},{y} distance_from_south={}", distance[i]);
        }
        for (label, matches) in [("road", 0xd0), ("rock", 0x60)] {
            assert!(
                world
                    .map
                    .cells
                    .iter()
                    .enumerate()
                    .any(|(i, c)| c.ground == matches && distance[i] != u32::MAX),
                "{name}: unreachable {label}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn reconstructed_realms_naturally_populate_reviewed_habitats() -> Result<()> {
    let path = std::path::Path::new("data");
    let content = Arc::new(crate::world::test_content());
    // Period MidForest uses Light Grass and HighPlains is snowy; boundaries remain reconstruction: https://wikiwiki.jp/rmd/%E5%9C%B0%E5%BD%A2
    let sources: Vec<_> = crate::terrain::realms::sources(path)?
        .into_iter()
        .filter(|s| {
            matches!(
                s.name(),
                "classic-world-01-reconstruction"
                    | "classic-world-02-reconstruction"
                    | "classic-world-03-reconstruction"
            )
        })
        .collect();
    assert_eq!(sources.len(), 3);
    for source in sources {
        let name = source.name().to_owned();
        let directory =
            Directory::configured(content.clone(), 16, 756, Some(756), vec![source], 2)?;
        let world = &directory.worlds[&-3];
        let snowy = name != "classic-world-01-reconstruction";
        for terrain in 1..=12 {
            assert_eq!(
                world.population_regions[terrain].target > 0,
                terrain != 3 || snowy,
                "{name}: terrain {terrain}"
            );
        }
        for terrain in [2, 3, 7, 8] {
            let region = &world.population_regions[terrain];
            if terrain == 3 && !snowy {
                assert!(region.sites.is_empty() && world.population_count(3) == 0);
                continue;
            }
            assert!(!region.sites.is_empty(), "{name}: terrain {terrain}");
            let area = world
                .map
                .cells
                .iter()
                .filter(|c| c.terrain as usize == terrain)
                .count();
            let count = world.population_count(terrain as u8);
            assert!(world.oryx_counted(terrain as u8).len() >= region.target);
            let kinds: BTreeSet<_> = world
                .entities
                .values()
                .filter(|e| {
                    e.population_slot.is_some()
                        && content.objects[&e.kind].terrain as usize == terrain
                })
                .map(|e| e.kind)
                .collect();
            println!(
                "{name} terrain={terrain} area={area} sites={} target={} bodies={count} kinds={kinds:?}",
                region.sites.len(),
                region.target
            );
            for kind in match terrain {
                2 => &[0x646, 0x649, 0x903][..],
                3 => &[0x640, 0x900, 0x957],
                7 => &[0x61f, 0x62d, 0x62e, 0x945],
                8 => &[0x60d, 0x6c4, 0x6c7],
                _ => unreachable!(),
            } {
                assert!(kinds.contains(kind), "{name}: missing natural {kind:#x}");
            }
        }
        for entity in world
            .entities
            .values()
            .filter(|e| e.population_slot.is_some())
        {
            let slot = entity.population_slot.unwrap();
            assert_eq!(world.population[slot].entity, Some(entity.id));
            assert_eq!(
                world.map.cell(entity.pos.x, entity.pos.y).unwrap().terrain,
                world.population_terrain(&world.population[slot])
            );
            assert!(world.walkable(entity.pos) && world.outside_setpieces(entity.pos));
            assert!(entity.quest.is_none());
        }
        assert_eq!(
            world.population_counts.iter().sum::<usize>(),
            world
                .entities
                .values()
                .filter(|e| e.population_slot.is_some())
                .count()
        );
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn culling_an_overgrown_godlands_keeps_every_species() -> Result<()> {
    let path = std::path::Path::new("data");
    let content = Arc::new(crate::world::test_content());
    let source = crate::terrain::realms::sources(path)?.remove(0);
    let mut directory = Directory::configured(content, 16, 756, Some(756), vec![source], 2)?;
    let world = &mut directory.worlds[&-3];
    let count = |w: &World, kinds: &[u16]| {
        w.entities
            .values()
            .filter(|e| kinds.contains(&e.kind))
            .count()
    };
    let (sprite_gods, unbred) = (count(world, &[0x652]), count(world, &[0x6d8, 0x905]));
    assert!(sprite_gods > 0 && unbred > 0);
    world.compact_population()?;
    // Live, the Godlands bred to 150% and one cull removed every Sprite God.
    world.population_regions[1].target = world.oryx_counted(1).len() / 2;
    world.cull_population(1);
    assert!(count(world, &[0x652]) * 4 > sprite_gods);
    assert_eq!(count(world, &[0x6d8, 0x905]), unbred);
    Ok(())
}

#[cfg(test)]
#[test]
fn xml_runtime_world_population_family_omission_refuses_before_sampling_or_slot_mutation() {
    let (mut w, p) = movement_tests::fixture(0x18, false);
    let family = w.realm_rules().unwrap().camps[&0x62d].clone();
    let terrain = w.population_habitat(family.leader).unwrap();
    let missing = family.roles[1].kind;
    Arc::make_mut(Arc::make_mut(&mut w.content).game.as_mut().unwrap())
        .definitions
        .remove(&missing);
    let pos = w.players[&p].pos;
    // Missing metadata is a construction fixture, not permission to swap a live world's catalogue.
    let mut w = World::in_domain(w.content.clone(), w.map.clone(), WorldDomain::Realm).unwrap();
    let before = w.rng.clone().r#gen::<u64>();
    let ids = w.ids.load(Ordering::Relaxed);
    let positions = vec![pos; family.size()];
    let error = format!(
        "{:#}",
        w.place_camp(family.clone(), terrain, positions)
            .unwrap_err()
    );
    assert!(
        error.contains("missing explicit admission definition")
            && error.contains(&format!("{missing:#x}")),
        "{error}"
    );
    assert_eq!(w.rng.clone().r#gen::<u64>(), before);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert!(w.camps.is_empty() && w.population.is_empty());
    let error = format!(
        "{:#}",
        w.spawn_population_group(terrain, family.leader)
            .unwrap_err()
    );
    assert!(
        error.contains("missing explicit admission definition")
            && error.contains(&format!("{missing:#x}")),
        "{error}"
    );
    assert_eq!(w.rng.clone().r#gen::<u64>(), before);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert!(w.camps.is_empty() && w.population.is_empty());
}

#[cfg(test)]
#[test]
fn xml_cutover_ordinary_goblin_births_follow_linked_counts_ownership_and_removal() {
    use crate::content::game;
    let path = std::path::Path::new("data");
    let source = crate::world::test_content();
    let actors = "game/123.5.1/actors.xml";
    let inputs: BTreeMap<_, _> = source
        .game
        .as_ref()
        .unwrap()
        .inputs
        .keys()
        .map(|file| (file.clone(), std::fs::read(path.join(file)).unwrap()))
        .collect();
    let before =
        r#"<Behavior childIds="Goblin Rogue" cooldown="12" maxChildren="7">MakeMinions</Behavior>"#;
    assert_eq!(
        String::from_utf8_lossy(&inputs[actors])
            .matches(before)
            .count(),
        1
    );
    for (row, rogue, warrior) in [
        (before, 7, 7),
        (
            r#"<Behavior childIds="Goblin Rogue" cooldown="12" maxChildren="2">MakeMinions</Behavior>"#,
            2,
            7,
        ),
        (
            r#"<Behavior childIds="Goblin Warrior" cooldown="12" maxChildren="2">MakeMinions</Behavior>"#,
            0,
            7,
        ),
        ("", 0, 7),
    ] {
        let mut inputs = inputs.clone();
        let mut content = source.clone();
        inputs.insert(
            actors.into(),
            String::from_utf8_lossy(&inputs[actors])
                .replace(before, row)
                .into_bytes(),
        );
        content.game = Some(Arc::new(
            game::compile_fixture_at(&content, inputs, path).unwrap(),
        ));
        let (fixture, _) = movement_tests::fixture(0x18, false);
        let mut map = (*fixture.map).clone();
        for cell in &mut map.cells {
            cell.terrain = 10;
        }
        let mut w = World::in_domain(Arc::new(content), Arc::new(map), WorldDomain::Realm).unwrap();
        let center = Pos { x: 16.5, y: 16.5 };
        w.population_regions[10].sites = vec![center];
        assert_eq!(w.spawn_population_group(10, 0x614).unwrap(), 1);
        let parent = w.population[0].entity.unwrap();
        let player = movement_tests::test_player(&mut w);
        w.players.get_mut(&player).unwrap().pos = Pos { x: 24.5, y: 16.5 };
        for at in [0, 50, 150, 12050, 24050, 36050] {
            w.now = at;
            w.xml_step().unwrap();
            w.repopulate_families().unwrap();
        }
        for (kind, count) in [(0x612, rogue), (0x613, warrior)] {
            let children: Vec<_> = w.actors().filter(|e| e.kind == kind).collect();
            assert_eq!(children.len(), count, "{row} {kind:#x}");
            assert!(
                children
                    .iter()
                    .all(|e| e.parent == Some(parent) && e.population_slot.is_none())
            );
        }
        assert_eq!(w.population.len(), 1);
    }
}
