// GPL-3.0 reference: realm-src@1855c017eab870d13f0b61b1cf0b9e66a01922ff, Hermit.cs/SP_Hermit.jm; data/REALM-SRC-LICENSE.
use super::realm_setpieces::Area;
use super::*;
use rand::seq::SliceRandom;

use crate::content::game::{IslandOwnership, IslandPlacement, NamedSite, SiteRole};

fn hermit_radius(x: i32, y: i32) -> f32 {
    ((x as f32 - 16.).powi(2) + ((y as f32 - 14.) * 1.1).powi(2)).sqrt()
}

impl World {
    pub(super) fn place_hermit_island(
        &mut self,
        rule: &NamedSite,
        placements: &[IslandPlacement],
    ) -> Result<()> {
        ensure!(
            self.players.is_empty() && self.now == 0,
            "Hermit landmark requires an unentered Realm"
        );
        let retain_authored_lava = self.retains_authored_lava();
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
        let mut candidates: Vec<_> = self
            .map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| rule.terrains.contains(&c.terrain))
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
                        (a.x..a.x + 32).all(|x| {
                            let c = &self.map.cells[(y * self.map.width + x) as usize];
                            c.region == 0
                                && !(retain_authored_lava && c.ground == 0xf4)
                                && (hermit_radius(x - a.x, y - a.y) > 15.
                                    || c.object
                                        .is_none_or(|kind| !self.content.objects[&kind].full_occupy))
                        })
                    })
            })
            .with_context(|| format!(
                "Realm has no unoccupied inland site for the Hermit landmark (layout={:?}, retain_authored_lava={retain_authored_lava})",
                self.realm_layout
            ))?;
        let width = self.map.width;
        let indices = (0..29).flat_map(|y| {
            (0..32).filter_map(move |x| {
                (hermit_radius(x, y) <= 15.).then_some(((area.y + y) * width + area.x + x) as usize)
            })
        });
        if rule.role == SiteRole::Event {
            let original = self.capture_event_site(area, indices)?;
            self.event_sites.push(original);
        }
        // Reconstruct the source island's rings without erasing generated waterways.
        for y in 0..29 {
            for x in 0..32 {
                let radius = hermit_radius(x, y);
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
                // Keep the southern portal approach shallow: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/common/resources/worlds/SP_Hermit.jm
                cell.ground = if matches!(cell.ground, 0x72 | 0x73) {
                    cell.ground
                } else if radius < 4.5 && (x, y) != (16, 18) {
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
        let root = self.spawn_site_actor(
            rule.kind,
            Pos {
                x: area.x as f32 + 16.,
                y: area.y as f32 + 13.5,
            },
            rule.role,
            false,
        )?;
        for &IslandPlacement {
            kind,
            x,
            y,
            ownership,
        } in placements
        {
            let pos = Pos {
                x: (area.x + x) as f32 + 0.5,
                y: (area.y + y) as f32 + 0.5,
            };
            // Pillars are optional dressing on the preserved shoreline, not quest dependents.
            if ownership == IslandOwnership::IndependentScenery && !self.walkable(pos) {
                continue;
            }
            let id = self.spawn_entity(kind, pos, vec![]).with_context(|| {
                format!("Island actor {kind:#x} placement failed at offset {x},{y}")
            })?;
            if ownership == IslandOwnership::Defender {
                self.set_parent(id, Some(root));
                let e = self.entities.get_mut(&id).unwrap();
                e.quest.set(Some(root), id, &mut self.quest_ids);
            }
        }
        self.setpieces.push(area);
        tracing::info!(
            root,
            layout = ?self.realm_layout,
            retain_authored_lava,
            x = area.x,
            y = area.y,
            "inland Hermit landmark placed"
        );
        Ok(())
    }
}
