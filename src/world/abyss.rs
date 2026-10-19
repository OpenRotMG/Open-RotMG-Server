use super::*;
use crate::{
    condition,
    dungeon::abyss::{self, BRIDGE, IDOL, LAVA, MALPHAS},
};

impl World {
    fn change_ground(&mut self, i: usize, ground: u8) {
        Arc::make_mut(&mut self.map).cells[i].ground = ground;
        let tile = (
            (i % self.map.width as usize) as i16,
            (i / self.map.width as usize) as i16,
        );
        let mut out = Writer::new();
        out.i16(1).i16(tile.0).i16(tile.1).u8(ground).i16(0).i16(0);
        for p in self.players.values().filter(|p| p.tiles.contains(&tile)) {
            send(p, wire::UPDATE, out.0.clone());
        }
    }

    pub(super) fn idol_bridge(&mut self, home: Pos, restore: bool) -> Result<()> {
        let region = self
            .map
            .cell(home.x, home.y)
            .context("Idol home outside map")?
            .region;
        ensure!(region >= 16, "Idol has no generated treasure-room region");
        let bridges: Vec<_> = self
            .map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.region == region && c.object == Some(BRIDGE))
            .map(|(i, _)| i)
            .collect();
        ensure!(!bridges.is_empty(), "Idol treasure room has no bridge");
        for i in bridges {
            let existing = self.cells[i]
                .iter()
                .find(|id| self.entities[id].kind == BRIDGE)
                .copied();
            if restore {
                if existing.is_none() {
                    self.spawn_entity(
                        BRIDGE,
                        Pos {
                            x: (i % self.map.width as usize) as f32 + 0.5,
                            y: (i / self.map.width as usize) as f32 + 0.5,
                        },
                        vec![],
                    )?;
                }
            } else if let Some(id) = existing {
                self.remove_entity(id);
            }
        }
        if restore {
            let patches: Vec<_> = self
                .lava_patches
                .iter()
                .filter(|(i, _)| self.map.cells[**i].region == region)
                .map(|(i, (_, g))| (*i, *g))
                .collect();
            for (i, g) in patches {
                self.change_ground(i, g);
                self.lava_patches.get_mut(&i).unwrap().0 = self.now;
            }
        }
        tracing::info!(region, restore, "Abyss Idol treasure bridge changed");
        Ok(())
    }

    pub(super) fn abyss_step(&mut self) {
        let restored: Vec<_> = self
            .lava_patches
            .iter()
            .filter(|(i, (deadline, _))| {
                *deadline <= self.now && self.map.cells[**i].ground == LAVA
            })
            .map(|(i, (_, ground))| (*i, *ground))
            .collect();
        for (i, ground) in restored {
            self.change_ground(i, ground);
        }
    }

    fn abyss_flash(&self, id: i32, color: u32, period: f32, duration: f32) {
        let mut out = Writer::new();
        out.u8(15)
            .i32(id)
            .f32(period)
            .f32((duration / period).round())
            .f32(0.)
            .f32(0.)
            .u32(color);
        for p in self.players.values().filter(|p| p.known.contains(&id)) {
            send(p, wire::SHOW_EFFECT, out.0.clone());
        }
    }

    pub(super) fn abyss_mechanics(
        &mut self,
        id: i32,
        target: impl Into<Option<Pos>>,
        elapsed: u64,
    ) -> Result<()> {
        let target = target.into();
        let e = &self.entities[&id];
        let kind = e.kind;
        if kind == 0x909 {
            if e.stage.is_none()
                && (elapsed >= 5000
                    || e.hp < self.content.objects[&kind].hp
                    || target.is_some_and(|p| e.pos.distance(p) < 1.3))
            {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(1);
                e.next_aux = self.now + 600;
                e.next_shot = self.now + 600;
                e.expires.set(Some(self.now + 650), id, &mut self.timed_ids);
                self.abyss_flash(id, 0xffffffff, 0.1, 0.6);
            }
            return Ok(());
        }
        if kind != MALPHAS && kind != IDOL {
            return Ok(());
        }
        let stage = if kind == MALPHAS {
            abyss::phase(elapsed)
        } else {
            0
        };
        let changed = e.stage != Some(stage);
        let home = e.home;
        let e = self.entities.get_mut(&id).unwrap();
        e.stage = Some(stage);
        if kind == MALPHAS {
            e.condition &= !condition::bit(condition::INVULNERABLE);
            let clock = elapsed % 50200;
            let shield = matches!(stage, 1 | 4 | 5)
                || stage == 0 && !(2000..6000).contains(&clock)
                || stage == 3 && (clock < 25300 || (clock - 25300) / 1400 % 3 != 1);
            if shield {
                e.condition |= condition::bit(condition::INVULNERABLE);
            }
            let size = match stage {
                2 => 30,
                3 => 140,
                _ => 100,
            };
            e.size += (size - e.size).clamp(-11, 11);
        }
        if changed {
            tracing::info!(
                entity = id,
                kind,
                stage,
                time_ms = self.now,
                "Abyss encounter phase entered"
            );
            if kind == IDOL {
                self.idol_bridge(home, false)?;
            } else if matches!(stage, 1 | 4) {
                self.abyss_flash(id, 0xff00ff00, 0.25, 2.5);
            } else if stage == 5 {
                self.spawn_children(id, 0x90b, 4, 6.)?;
                self.spawn_children(id, 0x90b, 9, 0.7)?;
            } else if stage == 6 {
                let flamers: Vec<_> = self
                    .entities
                    .values()
                    .filter(|e| e.parent == Some(id) && e.kind == 0x90b)
                    .map(|e| e.id)
                    .collect();
                for child in flamers {
                    self.remove_entity(child);
                }
                self.abyss_flash(id, 0xff484848, 0.6, 3.2);
            }
        }
        if kind == MALPHAS && self.now >= self.entities[&id].next_aux {
            self.spawn_children(id, 0x908, 3, 9.)?;
            if stage == 0 && elapsed % 1800 < 500 {
                self.spawn_children(id, 0x909, 4, 1.)?;
                for child in self
                    .entities
                    .values_mut()
                    .filter(|e| e.parent == Some(id) && e.kind == 0x909 && e.activated.is_none())
                {
                    child.activated = Some(self.now);
                    child
                        .expires
                        .set(Some(self.now + 5650), child.id, &mut self.timed_ids);
                }
            }
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 500;
        }
        if kind == IDOL && self.now >= self.entities[&id].next_aux {
            let region = self.map.cell(home.x, home.y).unwrap().region;
            let mut candidates: Vec<_> = self
                .map
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| c.region == region && c.ground != LAVA && c.object.is_none())
                .map(|(i, _)| i)
                .collect();
            candidates.shuffle(&mut self.rng);
            for i in candidates.into_iter().take(6) {
                let old = self.map.cells[i].ground;
                self.lava_patches.insert(i, (self.now + 2000, old));
                self.change_ground(i, LAVA);
            }
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 2500;
        }
        Ok(())
    }
}
