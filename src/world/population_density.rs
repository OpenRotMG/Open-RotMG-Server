use super::*;

#[derive(Default)]
pub(super) struct Region {
    pub(super) target: usize,
    pub(super) limit: usize,
    sites: Vec<Pos>,
    kinds: Vec<u16>,
}

impl World {
    pub(super) fn configure_population(&mut self) -> Result<()> {
        ensure!(
            self.population.is_empty(),
            "cannot reconfigure a live population"
        );
        // Kithio's per-terrain area divisors, reindexed to selected XML terrain numbers: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/Oryx.cs#L27-L156
        let divisors = [
            0, 100, 250, 300, 300, 300, 150, 150, 200, 200, 200, 100, 150,
        ];
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
        for object in self.content.objects.values().filter(|o| {
            o.enemy
                && o.terrain > 0
                && o.spawn_weight.is_some_and(|w| w > 0.)
                && o.per_realm_max.is_none()
        }) {
            crate::encounter::validate(&self.content, object.kind)?;
            crate::loot::validate(&self.content, object.kind)?;
            if let Some(family) = population::FAMILIES
                .iter()
                .find(|f| f.leader == object.kind)
            {
                self.validate_family(family)?;
                let roots = match object.spawn_group {
                    Some(group) => group.max,
                    None => 1,
                };
                ensure!(
                    roots * family.initial() <= 32,
                    "population group exceeds birth headroom"
                );
            }
            self.population_regions[object.terrain as usize]
                .kinds
                .push(object.kind);
        }
        for &(parent, child, ..) in population::REPRODUCTION {
            let terrain = self.content.get(parent)?.terrain;
            ensure!(
                terrain > 0 && self.content.get(child)?.terrain == terrain,
                "reproduction {parent:#x}->{child:#x} crosses Realm habitats"
            );
        }
        for terrain in 1..=12 {
            let region = &mut self.population_regions[terrain];
            region.target = area[terrain] / divisors[terrain];
            // Reserve a complete maximum-size birth burst without allowing unbounded reinforcements.
            region.limit = if region.target == 0 {
                0
            } else {
                region.target * 3 / 2 + 32
            };
            ensure!(
                region.target == 0 || !region.sites.is_empty() && !region.kinds.is_empty(),
                "terrain {terrain} lacks population sites or species"
            );
            tracing::info!(
                terrain,
                area = area[terrain],
                target = region.target,
                limit = region.limit,
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
        match self.content.objects[&kind].spawn_group {
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
    fn spawn_population_group(&mut self, terrain: u8, kind: u16) -> Result<bool> {
        let family = population::FAMILIES.iter().find(|f| f.leader == kind);
        let roots = self.sample_population_group(kind);
        let cost = roots * family.map_or(1, |f| f.initial());
        if !self.population_space(terrain, cost)? {
            return Ok(false);
        }
        let center = *self.population_regions[terrain as usize]
            .sites
            .choose(&mut self.rng)
            .context("population has no candidate site")?;
        let clearance = if matches!(kind, 0x604 | 0x953) {
            64.
        } else {
            12.
        };
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
                let positions = match family {
                    Some(family) => self.family_layout(pos, family, &occupied),
                    None => Some(vec![pos]),
                };
                if let Some(positions) = positions
                    && positions.iter().all(|pos| {
                        // Oryx top-ups avoid players by Kithio's AnyPlayerNearby radius: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/Utils.cs#L34-L43
                        self.players.values().all(|p| p.pos.distance(*pos) >= 16.)
                            && self
                                .map
                                .spawns
                                .iter()
                                .all(|(x, y)| pos.distance(Pos { x: *x, y: *y }) >= clearance)
                    })
                {
                    layout = Some(positions);
                    break;
                }
            }
            let Some(positions) = layout else {
                return Ok(false);
            };
            occupied.extend(positions.iter().copied());
            groups.push(positions);
        }
        for positions in groups {
            match family {
                Some(family) => self.place_camp(family, positions)?,
                None => {
                    let pos = positions[0];
                    self.place_population_solo(kind, pos)?;
                }
            }
        }
        Ok(true)
    }
    fn fill_population(&mut self, terrain: u8) -> Result<()> {
        let target = self.population_regions[terrain as usize].target;
        let kinds = self.population_regions[terrain as usize].kinds.clone();
        let (mut refused, mut stalled, mut groups) = (0, 0, 0);
        while self.population_count(terrain) < target && stalled < 128 {
            // SpawnProb's original selector is lost; relative XML weights are an explicit reconstruction choice.
            let kind = *kinds
                .choose_weighted(&mut self.rng, |kind| {
                    self.content.objects[kind]
                        .spawn_weight
                        .expect("validated spawn weight")
                })
                .context("invalid population weights")?;
            if self.spawn_population_group(terrain, kind)? {
                groups += 1;
                stalled = 0;
            } else {
                refused += 1;
                stalled += 1;
            }
        }
        tracing::info!(
            terrain,
            target,
            bodies = self.population_count(terrain),
            groups,
            refused,
            "Realm ordinary population allocated"
        );
        ensure!(
            self.population_count(terrain) >= target,
            "terrain {terrain} cannot fill population: {}/{} bodies after {stalled} consecutive refusals",
            self.population_count(terrain),
            target
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
                && !self.fauna_births.iter().any(|b| b.camp == index)
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
        // Admitted tosses own empty slots even after their parent dies; compaction must preserve that promise.
        for birth in &mut self.fauna_births {
            ensure!(
                camps[birth.camp] != usize::MAX && slots[birth.slot] != usize::MAX,
                "compaction lost an admitted birth"
            );
            birth.camp = camps[birth.camp];
            birth.slot = slots[birth.slot];
        }
        Ok(())
    }
    pub(super) fn repopulate(&mut self) -> Result<()> {
        if self.now >= self.population_next {
            self.population_next = self.now + 60000;
            self.compact_population()?;
            for terrain in 1..=12 {
                let target = self.population_regions[terrain as usize].target;
                // Kithio culls above 150% and refills below 75%: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/Oryx.cs#L298-L357
                if self.population_count(terrain) * 2 > target * 3 {
                    self.cull_population(terrain);
                } else if self.population_count(terrain) * 4 < target * 3 {
                    self.fill_population(terrain)?;
                }
            }
        }
        self.reproduce_active();
        self.repopulate_families()
    }
    pub(super) fn cull_population(&mut self, terrain: u8) {
        let excess =
            self.population_count(terrain) - self.population_regions[terrain as usize].target;
        // Only bodies without a player within 10 tiles leave, so active areas keep their regrowth.
        let culled: Vec<_> = self
            .population
            .iter()
            .filter(|slot| self.population_terrain(slot) == terrain)
            .filter_map(|slot| slot.entity)
            .filter(|id| {
                let pos = self.entities[id].pos;
                self.players.values().all(|p| p.pos.distance(pos) >= 10.)
            })
            .take(excess)
            .collect();
        for &id in &culled {
            self.remove_entity(id);
        }
        tracing::info!(
            terrain,
            culled = culled.len(),
            bodies = self.population_count(terrain),
            "Realm ordinary population culled"
        );
    }
}

#[cfg(test)]
#[test]
fn populated_realms_keep_all_arrivals_connected() -> Result<()> {
    let path = std::path::Path::new("data");
    let content = Arc::new(Content::load(path)?);
    // Fixed geography is distinct from variable buildings: https://web.archive.org/web/20130522030032id_/https://forums.wildshadow.com/wiki/realm-mad-god/maps-builds-106-123
    for source in crate::terrain::realms::sources(path)? {
        let name = source.name().to_owned();
        let directory =
            Directory::configured(path, content.clone(), 16, 785, Some(785), vec![source], 2)?;
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
            assert!(
                world
                    .entities
                    .values()
                    .filter(|e| e.population_slot.is_some())
                    .all(|e| e.pos.distance(Pos { x, y }) >= 12.),
                "{name}: arrival population clearance"
            );
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
    let content = Arc::new(Content::load(path)?);
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
            Directory::configured(path, content.clone(), 16, 756, Some(756), vec![source], 2)?;
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
            assert!(count >= region.target && count <= region.limit);
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
                "{name} terrain={terrain} area={area} sites={} target={} limit={} bodies={count} kinds={kinds:?}",
                region.sites.len(),
                region.target,
                region.limit
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
