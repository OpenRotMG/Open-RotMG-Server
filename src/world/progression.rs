use super::*;

pub const REALM_ROSTER: &[(u8, &[u16])] = &[
    (8, &[0x60d, 0x60a, 0x60c]),
    (9, &[0x617, 0x615, 0x616, 0x61a, 0x618, 0x619]),
    (10, &[0x609, 0x605, 0x606, 0x607, 0x614, 0x612, 0x613]),
    (5, &[0x632, 0x633, 0x6c6]),
    (6, &[0x624, 0x625, 0x626, 0x629, 0x62a, 0x62b, 0x62c]),
    (7, &[0x61f, 0x61c, 0x61d, 0x61e]),
    (2, &[0x646, 0x647, 0x648, 0x649]),
    (
        3,
        &[
            0x640, 0x63e, 0x63a, 0x63b, 0x63c, 0x63f, 0x900, 0x901, 0x902,
        ],
    ),
    (
        4,
        &[
            0x650, 0x64f, 0x64a, 0x64b, 0x64c, 0x64d, 0x645, 0x643, 0x644, 0x642,
        ],
    ),
    (
        1,
        &[
            0x651, 0x652, 0x653, 0x654, 0x655, 0x656, 0x657, 0x658, 0x659, 0x91a, 0x6d8, 0x905,
            0x906, 0x907,
        ],
    ),
];

impl World {
    pub(super) fn populate_inland(&mut self) -> Result<()> {
        for (terrain, roster) in REALM_ROSTER {
            for kind in *roster {
                ensure!(
                    self.content.get(*kind)?.terrain == *terrain,
                    "Realm roster {kind:#x} contradicts selected XML terrain {terrain}"
                );
            }
            let mut candidates: Vec<_> = self
                .map
                .cells
                .iter()
                .enumerate()
                .filter_map(|(i, cell)| {
                    let pos = Pos {
                        x: (i % self.map.width as usize) as f32 + 0.5,
                        y: (i / self.map.width as usize) as f32 + 0.5,
                    };
                    (cell.terrain == *terrain && cell.object.is_none() && self.walkable(pos))
                        .then_some(pos)
                })
                .collect();
            candidates.shuffle(&mut self.rng);
            if *terrain == 1 {
                self.populate_godlands(&candidates)?;
                continue;
            }
            let families: &[u16] = match terrain {
                2 => &[0x649, 0x649, 0x649],
                3 => &[0x640, 0x640, 0x900, 0x900],
                4 => &[0x650, 0x645, 0x645],
                6 => &[0x62c, 0x62c, 0x62c],
                7 => &[0x61f, 0x61f, 0x61f, 0x61f],
                8 => &[0x60d, 0x60d, 0x60d, 0x60d, 0x60d],
                9 => &[0x617, 0x61a, 0x617, 0x61a],
                10 => &[0x614, 0x609, 0x609],
                _ => &[],
            };
            if !families.is_empty() {
                self.populate_families(*terrain, &candidates, families)?;
                let solos: &[u16] = match terrain {
                    2 => &[0x646; 8],
                    6 => &[0x624, 0x625, 0x626, 0x624, 0x625, 0x626, 0x624, 0x625],
                    _ => &[],
                };
                self.populate_solos(*terrain, &candidates, solos)?;
                continue;
            }
            let mut count = 0;
            for pos in candidates {
                if self.population.iter().any(|s| s.pos.distance(pos) < 8.) {
                    continue;
                }
                let kind = roster[count % roster.len()];
                let entity = self.spawn_entity(kind, pos, vec![])?;
                self.population.push(Spawn {
                    kind,
                    pos,
                    entity: Some(entity),
                    ready: 0,
                    camp: None,
                });
                count += 1;
                if count == 32 {
                    break;
                }
            }
            tracing::info!(terrain, count, "bounded inland population allocated");
        }
        Ok(())
    }
}
