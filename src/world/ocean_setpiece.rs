// GPL-3.0 reference: realm-src@1855c017eab870d13f0b61b1cf0b9e66a01922ff, Hermit.cs/SP_Hermit.jm; data/REALM-SRC-LICENSE.
use super::realm_setpieces::Area;
use super::*;
use rand::seq::SliceRandom;

impl World {
    pub(super) fn populate_hermit(&mut self) -> Result<()> {
        ensure!(
            self.players.is_empty() && self.now == 0,
            "Hermit landmark requires an unentered Realm"
        );
        let protected: Vec<_> = self
            .population
            .iter()
            .map(|s| s.pos)
            .chain(
                self.entities
                    .values()
                    .filter(|e| self.content.objects[&e.kind].enemy)
                    .map(|e| e.pos),
            )
            .collect();
        // Inland event range: realm-src@1855c017, wServer/realm/Oryx.cs::SpawnEvent (Mountains..MidForest).
        let mut candidates: Vec<_> = self
            .map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| (1..=7).contains(&c.terrain))
            .map(|(i, _)| Area {
                x: i as i32 % self.map.width - 16,
                y: i as i32 / self.map.width - 14,
                width: 32,
                height: 29,
            })
            .collect();
        candidates.shuffle(&mut self.rng);
        let area = candidates
            .into_iter()
            .find(|a| {
                a.x > 0
                    && a.y > 0
                    && a.x + 32 < self.map.width
                    && a.y + 29 < self.map.height
                    && self.setpieces.iter().all(|p| a.separate(p))
                    && protected.iter().all(|p| a.distance(*p) >= 4.)
                    && self
                        .map
                        .spawns
                        .iter()
                        .all(|&(x, y)| a.distance(Pos { x, y }) >= 64.)
                    && (a.y..a.y + 29).all(|y| {
                        (a.x..a.x + 32)
                            .all(|x| self.map.cells[(y * self.map.width + x) as usize].region == 0)
                    })
            })
            .context("Realm has no unoccupied inland site for the Hermit landmark")?;
        // Reconstruct the source island's rings without erasing generated waterways.
        for y in 0..29 {
            for x in 0..32 {
                let radius = ((x as f32 - 16.).powi(2) + ((y as f32 - 14.) * 1.1).powi(2)).sqrt();
                if radius > 15. {
                    continue;
                }
                let pos = Pos {
                    x: (area.x + x) as f32 + 0.5,
                    y: (area.y + y) as f32 + 0.5,
                };
                let index = pos.y as usize * self.map.width as usize + pos.x as usize;
                let old: Vec<_> = self.cells.ids(index).to_vec();
                for id in old {
                    self.remove_entity(id);
                }
                let cell = &mut Arc::make_mut(&mut self.map).cells[index];
                cell.ground = if matches!(cell.ground, 0x72 | 0x73) {
                    cell.ground
                } else if radius < 4.5 {
                    0xbc
                } else if radius < 10. {
                    0x73
                } else if radius < 12. {
                    0xbd
                } else {
                    0xbe
                };
                cell.object = None;
                cell.config.clear();
            }
        }
        let root = self.insert_entity(
            crate::dungeon::trench::HERMIT,
            Pos {
                x: area.x as f32 + 16.,
                y: area.y as f32 + 13.5,
            },
            vec![],
        )?;
        let e = self.entities.get_mut(&root).unwrap();
        e.quest = Some(root);
        e.stage = Some(0);
        e.condition |= crate::condition::bit(crate::condition::INVINCIBLE);
        e.extra_stats = vec![(61, 2)];
        for (x, y) in [
            (13, 8),
            (20, 17),
            (16, 8),
            (18, 8),
            (10, 13),
            (21, 13),
            (11, 17),
            (15, 18),
        ] {
            let id = self
                .spawn_entity(
                    0xd65,
                    Pos {
                        x: (area.x + x) as f32 + 0.5,
                        y: (area.y + y) as f32 + 0.5,
                    },
                    vec![],
                )
                .with_context(|| format!("Hermit spawner placement failed at offset {x},{y}"))?;
            let e = self.entities.get_mut(&id).unwrap();
            e.parent.set(Some(root), id, &mut self.timed_ids);
            e.quest = Some(root);
            e.condition |= crate::condition::bit(crate::condition::INVINCIBLE);
        }
        for (x, y) in [(11, 7), (20, 7), (8, 13), (20, 19)] {
            let pos = Pos {
                x: (area.x + x) as f32 + 0.5,
                y: (area.y + y) as f32 + 0.5,
            };
            if self.walkable(pos) {
                self.spawn_entity(0x17b, pos, vec![])?;
            }
        }
        self.setpieces.push(area);
        tracing::info!(
            root,
            x = area.x,
            y = area.y,
            "inland Hermit landmark placed"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermit_lakes_use_inland_event_terrain_on_every_realm_layout() {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        for source in crate::terrain::realms::sources(std::path::Path::new("data")).unwrap() {
            let map = source.open(&content).unwrap();
            for seed in [0, 12351] {
                let mut w = World::new(content.clone(), map.clone()).unwrap();
                w.rng = rand::rngs::StdRng::seed_from_u64(seed);
                w.populate_coil_snakes().unwrap();
                w.populate_realm_heroes().unwrap();
                w.populate_named_quests().unwrap();
                w.populate_realm_events().unwrap();
                assert_eq!(
                    w.entities
                        .values()
                        .filter(|e| e.kind == crate::dungeon::jungle::COIL)
                        .count(),
                    2
                );
                let original = w.map.clone();
                w.populate_hermit().unwrap();
                let area = w.setpieces.last().unwrap();
                let x = area.x + 16;
                let y = area.y + 14;
                let terrain = original.cells[(y * original.width + x) as usize].terrain;
                eprintln!(
                    "{} seed {seed}: Hermit site ({x},{y}), original terrain {terrain}",
                    source.name()
                );
                assert!(
                    (1..=7).contains(&terrain),
                    "Hermit belongs in Mountains..MidForest, not coast/lowlands: {terrain}"
                );
                let roots: Vec<_> = w
                    .entities
                    .values()
                    .filter(|e| e.kind == crate::dungeon::trench::HERMIT)
                    .collect();
                assert_eq!(roots.len(), 1);
                assert_eq!(content.objects[&roots[0].kind].name, "Hermit God");
                assert_eq!(
                    w.entities
                        .values()
                        .filter(|e| e.kind == 0xd65 && e.parent == Some(roots[0].id))
                        .count(),
                    8
                );
                assert!(
                    original
                        .spawns
                        .iter()
                        .all(|&(x, y)| area.distance(Pos { x, y }) >= 64.)
                );
                assert!(
                    w.setpieces[..w.setpieces.len() - 1]
                        .iter()
                        .all(|p| area.separate(p))
                );
            }
        }
        let mut shore = super::super::tests::arena(11);
        let entities = shore.entities.len();
        let pieces = shore.setpieces.len();
        assert!(
            shore
                .populate_hermit()
                .unwrap_err()
                .to_string()
                .contains("no unoccupied inland site")
        );
        assert_eq!(
            (shore.entities.len(), shore.setpieces.len()),
            (entities, pieces)
        );
    }
}
