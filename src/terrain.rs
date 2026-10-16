use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, ensure};

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}
fn noise(seed: u64, x: f64, y: f64) -> f64 {
    let (ix, iy) = (x.floor() as u64, y.floor() as u64);
    let sample = |x: u64, y: u64| {
        (mix(seed ^ x.wrapping_mul(0x9e3779b97f4a7c15) ^ y.wrapping_mul(0xd1b54a32d192ed03)) >> 11)
            as f64
            / ((1u64 << 53) as f64)
    };
    let smooth = |v: f64| v * v * (3. - 2. * v);
    let (fx, fy) = (smooth(x.fract()), smooth(y.fract()));
    let a = sample(ix, iy) * (1. - fx) + sample(ix + 1, iy) * fx;
    let b = sample(ix, iy + 1) * (1. - fx) + sample(ix + 1, iy + 1) * fx;
    a * (1. - fy) + b * fy
}
fn biome(depth: f64, moisture: f64, radius: f64) -> (u8, u8) {
    if depth < -4. {
        return (0xbc, 0);
    }
    if depth < 0. {
        return (0x72, 0);
    }
    if depth < 4. {
        return (0x73, 0);
    }
    if depth < 12. {
        return (0xbe, 11);
    }
    if depth < 20. && moisture > 3. / 7. {
        return (0x48, 12);
    }
    let elevation = depth / radius;
    let variant = if moisture > 5. / 7. {
        2
    } else if moisture > 3. / 7. {
        1
    } else {
        0
    };
    if elevation >= 0.5 {
        (if variant == 2 { 0x6a } else { 0x60 }, 1)
    } else if elevation > 0.35 {
        ([0xbd, 0x23, 0x47][variant], [2, 3, 4][variant])
    } else if elevation > 0.15 {
        ([0xbd, 0x46, 0x56][variant], [5, 6, 7][variant])
    } else {
        ([0xbd, 0x48, 0x56][variant], [8, 9, 10][variant])
    }
}

pub fn generate(content: &Content, seed: u64, size: i32) -> Result<Map> {
    ensure!(
        (192..=768).contains(&size),
        "Realm size must be 192–768 tiles"
    );
    let started = std::time::Instant::now();
    let center = size as f64 / 2.;
    let radius = size as f64 * 0.44;
    let phase = (mix(seed) >> 11) as f64 / ((1u64 << 53) as f64) * std::f64::consts::TAU;
    let mut cells = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f64 + 0.5 - center, y as f64 + 0.5 - center);
            let angle = dy.atan2(dx);
            let coast = radius
                * (1. + 0.045 * (angle * 3. + phase).sin() + 0.025 * (angle * 5. - phase).cos());
            let depth = coast - dx.hypot(dy);
            let moisture = 0.7 * noise(seed, x as f64 / 48., y as f64 / 48.)
                + 0.3 * noise(seed ^ 0x6a09e667f3bcc909, x as f64 / 96., y as f64 / 96.);
            let (mut ground, terrain) = biome(depth, moisture, radius);
            let ring = [0.10, 0.25, 0.42]
                .iter()
                .any(|h| (depth - radius * h).abs() < 1.3);
            let spoke = dx.abs() < 1.3 || dy.abs() < 1.3;
            let road = terrain != 0 && (ring || spoke);
            if road {
                ground = 0xd0;
            }
            let decoration = noise(seed ^ 0xbb67ae8584caa73b, x as f64, y as f64);
            let object = if !road && decoration < 0.035 {
                match terrain {
                    10 | 7 | 4 => Some(0x100),
                    9 | 6 | 3 | 12 => Some(0x101),
                    8 | 5 | 2 => Some(0x103),
                    _ => None,
                }
            } else {
                None
            };
            cells.push(Cell {
                ground,
                object,
                config: String::new(),
                terrain,
                region: 0,
            });
        }
    }
    let mut spawns = Vec::new();
    // Road ends give each coast an arrival connected to the rest of the island.
    for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
        let (x, y) = (2..size / 2 - 2)
            .rev()
            .map(|offset| (size / 2 + dx * offset, size / 2 + dy * offset))
            .find(|&(x, y)| {
                (y - 1..=y + 1)
                    .all(|y| (x - 1..=x + 1).all(|x| cells[(y * size + x) as usize].terrain == 11))
            })
            .with_context(|| format!("generated Realm has no coastal spawn toward {dx},{dy}"))?;
        for y in y - 1..=y + 1 {
            for x in x - 1..=x + 1 {
                let cell = &mut cells[(y * size + x) as usize];
                cell.object = None;
                cell.region = 1;
            }
        }
        spawns.push((x as f32 + 0.5, y as f32 + 0.5));
    }
    let map = Map {
        width: size,
        height: size,
        cells,
        spawns,
    };
    for cell in &map.cells {
        ensure!(
            content.grounds.contains_key(&cell.ground),
            "generated Realm uses unknown ground {:#x}",
            cell.ground
        );
        if let Some(object) = cell.object {
            content.get(object)?;
        }
    }
    tracing::info!(
        seed,
        size,
        spawn_points = map.spawns.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "generated Realm terrain; original generator unverified"
    );
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, VecDeque};
    #[test]
    fn seeds_are_reproducible_and_coast_roads_reach_every_biome() {
        let c = Content::load(std::path::Path::new("data")).unwrap();
        for seed in [0, 1, 42, u64::MAX] {
            let m = generate(&c, seed, 192).unwrap();
            assert_eq!(
                serde_json::to_vec(&m).unwrap(),
                serde_json::to_vec(&generate(&c, seed, 192).unwrap()).unwrap()
            );
            assert_ne!(
                serde_json::to_vec(&m).unwrap(),
                serde_json::to_vec(&generate(&c, seed.wrapping_add(1), 192).unwrap()).unwrap()
            );
            assert_eq!(m.spawns.len(), 4);
            for &(sx, sy) in &m.spawns {
                assert_eq!(m.cell(sx, sy).unwrap().ground, 0xd0);
                for y in sy as i32 - 1..=sy as i32 + 1 {
                    for x in sx as i32 - 1..=sx as i32 + 1 {
                        let cell = m.cell(x as f32, y as f32).unwrap();
                        assert_eq!((cell.terrain, cell.region), (11, 1));
                        assert!(cell.object.is_none());
                    }
                }
            }
            let (sx, sy) = m.spawns[0];
            let start = (sy as i32 * m.width + sx as i32) as usize;
            let mut seen = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            let mut terrains = BTreeSet::new();
            while let Some(i) = queue.pop_front() {
                let cell = &m.cells[i];
                terrains.insert(cell.terrain);
                let (x, y) = (i as i32 % m.width, i as i32 / m.width);
                for (x, y) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if x < 0 || y < 0 || x >= m.width || y >= m.height {
                        continue;
                    }
                    let j = (y * m.width + x) as usize;
                    let next = &m.cells[j];
                    if next.terrain != 0
                        && c.grounds[&next.ground] > 0.
                        && next.object.is_none_or(|id| !c.objects[&id].blocks)
                        && seen.insert(j)
                    {
                        queue.push_back(j);
                    }
                }
            }
            assert!(terrains.contains(&1));
            assert!(terrains.contains(&11));
            for range in [2..=4, 5..=7, 8..=10] {
                assert!(range.into_iter().any(|t| terrains.contains(&t)));
            }
            for (i, cell) in m
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| cell.ground == 0xd0 || cell.region == 1)
            {
                assert!(seen.contains(&i), "road or spawn unreachable at {i}");
                assert!(cell.object.is_none());
            }
            for x in 0..m.width {
                assert_eq!(m.cells[x as usize].ground, 0xbc);
                assert_eq!(
                    m.cells[((m.height - 1) * m.width + x) as usize].ground,
                    0xbc
                );
            }
        }
        assert!(generate(&c, 0, 191).is_err());
        assert!(generate(&c, 0, 769).is_err());
    }
}
