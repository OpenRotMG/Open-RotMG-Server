use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

const TREES: [u16; 5] = [0x175, 0x176, 0x177, 0x178, 0x179];
const SPRITES: [u16; 5] = [0xd00, 0xd01, 0xd02, 0xd03, 0xd04];

#[derive(Clone, Copy, Debug)]
enum FloorPattern {
    White,
    YellowChecker,
}

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    // Reconstructed per-world visual choice; do not consume layout/population RNG draws.
    let pattern = if seed & 1 == 0 {
        FloorPattern::White
    } else {
        FloorPattern::YellowChecker
    };
    generate_with_pattern(content, seed, pattern)
}

fn generate_with_pattern(content: &Content, seed: u64, pattern: FloorPattern) -> Result<Map> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let directions = [(1i32, 0), (-1, 0), (0, 1), (0, -1)];
    let mut rooms = vec![(0i32, 0i32)];
    let mut edges = Vec::new();
    let spine = rng.gen_range(5..=8);
    for parent in 0..spine {
        let (x, y) = rooms[parent];
        let choices: Vec<_> = directions
            .iter()
            .map(|(dx, dy)| (x + dx, y + dy))
            .filter(|p| !rooms.contains(p))
            .collect();
        rooms.push(
            *choices
                .choose(&mut rng)
                .context("Sprite World spine exhausted")?,
        );
        edges.push((parent, parent + 1));
    }
    let boss = rooms.len() - 1;
    for _ in 0..rng.gen_range(2..=5) {
        let choices: Vec<_> = rooms
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0 && *i != boss)
            .flat_map(|(i, (x, y))| directions.iter().map(move |(dx, dy)| (i, (x + dx, y + dy))))
            .filter(|(_, p)| !rooms.contains(p))
            .collect();
        let (parent, pos) = *choices
            .choose(&mut rng)
            .context("Sprite World branches exhausted")?;
        edges.push((parent, rooms.len()));
        rooms.push(pos);
    }
    let min_x = rooms.iter().map(|p| p.0).min().unwrap();
    let min_y = rooms.iter().map(|p| p.1).min().unwrap();
    let centers: Vec<_> = rooms
        .iter()
        .map(|(x, y)| ((x - min_x) * 16 + 12, (y - min_y) * 16 + 12))
        .collect();
    let width = centers.iter().map(|p| p.0).max().unwrap() + 14;
    let height = centers.iter().map(|p| p.1).max().unwrap() + 14;
    let mut map = Map {
        width,
        height,
        cells: vec![
            Cell {
                ground: 0xfe,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            (width * height) as usize
        ],
        spawns: vec![],
    };
    // Leave void between unlinked platforms; smaller rooms/two-tile bridges match the preserved map:
    // https://github.com/ossimc82/fabiano-swagger-of-doom/blob/6fd20aad4a7905b13f25389c68368a942a2b68cb/wServer/realm/worlds/maps/spriteworld.wmap
    for (i, &(cx, cy)) in centers.iter().enumerate() {
        let radius = if i == boss {
            8
        } else if i == 0 {
            6
        } else {
            rng.gen_range(3..=5)
        };
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                paint(&mut map, x, y, pattern);
            }
        }
    }
    for &(a, b) in &edges {
        let ((x0, y0), (x1, y1)) = (centers[a], centers[b]);
        for y in y0.min(y1)..=y0.max(y1) + 1 {
            for x in x0.min(x1)..=x0.max(x1) + 1 {
                paint(&mut map, x, y, pattern);
            }
        }
    }
    let (sx, sy) = centers[0];
    let spawn = &mut map.cells[(sy * width + sx) as usize];
    spawn.region = 1;
    spawn.object = Some(0x70d);
    map.spawns = vec![(sx as f32 + 0.5, sy as f32 + 0.5)];
    let (bx, by) = centers[boss];
    map.cells[(by * width + bx) as usize].object = Some(0xd06);
    map.cells[(by * width + bx + 3) as usize].object = Some(0x70e);
    let gods = rng.gen_range(0..=5);
    let mut hosts: Vec<_> = centers
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 0 && *i != boss)
        .map(|(_, center)| *center)
        .collect();
    hosts.shuffle(&mut rng);
    for (cx, cy) in hosts.into_iter().take(gods) {
        place(&mut map, cx + 2, cy - 2, 0xd05);
    }
    for (room, &(cx, cy)) in centers.iter().enumerate() {
        if room == 0 || room == boss {
            continue;
        }
        for (n, &(dx, dy)) in [(-2, 0), (2, 1), (0, 2), (-1, -2)].iter().enumerate() {
            place(&mut map, cx + dx, cy + dy, SPRITES[n % SPRITES.len()]);
        }
        for (dx, dy) in [(-3, -3), (3, -3), (-3, 3), (3, 3)] {
            place(
                &mut map,
                cx + dx,
                cy + dy,
                TREES[rng.gen_range(0..TREES.len())],
            );
        }
    }
    ensure!(map.spawns.len() == 1, "Sprite World lost its entrance");
    ensure!(
        map.cells.iter().filter(|c| c.object == Some(0xd06)).count() == 1
            && map.cells.iter().filter(|c| c.object == Some(0x70e)).count() == 1
            && map.cells.iter().filter(|c| c.object == Some(0x70d)).count() == 1,
        "Sprite World lost a required portal or boss"
    );
    for cell in &map.cells {
        ensure!(
            content.grounds.contains_key(&cell.ground),
            "unknown Sprite World ground"
        );
        if let Some(kind) = cell.object {
            content.get(kind)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        gods,
        floor_pattern = ?pattern,
        width,
        height,
        "generated varied Sprite World"
    );
    Ok(map)
}

fn paint(map: &mut Map, x: i32, y: i32, pattern: FloorPattern) {
    // Keep conveyor positions/vectors identical; white and yellow are both stationary.
    map.cells[(y * map.width + x) as usize].ground =
        match ((x - 12).rem_euclid(8), (y - 12).rem_euclid(8)) {
            (_, 2) => 0xa1,
            (_, 3) => 0xa2,
            (2, _) => 0xa3,
            (3, _) => 0xa4,
            _ if matches!(pattern, FloorPattern::YellowChecker) && (x + y) % 2 != 0 => 0xa5,
            _ => 0xa0,
        };
}

fn place(map: &mut Map, x: i32, y: i32, kind: u16) {
    let cell = &mut map.cells[(y * map.width + x) as usize];
    if (0xa0..=0xa5).contains(&cell.ground) && cell.object.is_none() {
        cell.object = Some(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn floor_patterns_only_recolor_stationary_tiles() {
        let content = Content::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data"
        )))
        .unwrap();
        assert_eq!(content.grounds[&0xa0], content.grounds[&0xa5]);
        assert_eq!(content.pushes[&0xa0], (0., 0.));
        assert_eq!(content.pushes[&0xa5], (0., 0.));
        assert!(!content.hazards.contains_key(&0xa0));
        assert!(!content.hazards.contains_key(&0xa5));
        for seed in [0, 1] {
            let white = generate_with_pattern(&content, seed, FloorPattern::White).unwrap();
            let checker =
                generate_with_pattern(&content, seed, FloorPattern::YellowChecker).unwrap();
            assert!(checker.cells.iter().any(|c| c.ground == 0xa5));
            for (index, (a, b)) in white.cells.iter().zip(&checker.cells).enumerate() {
                let (x, y) = (index as i32 % white.width, index as i32 / white.width);
                let expected = if a.ground == 0xa0 && (x + y) % 2 != 0 {
                    0xa5
                } else {
                    a.ground
                };
                assert_eq!(b.ground, expected, "seed {seed} at {x},{y}");
            }
            let selected = if seed & 1 == 0 { &white } else { &checker };
            assert_eq!(
                serde_json::to_vec(&generate(&content, seed).unwrap()).unwrap(),
                serde_json::to_vec(selected).unwrap()
            );
            let mut normalized = checker;
            for cell in &mut normalized.cells {
                if cell.ground == 0xa5 {
                    cell.ground = 0xa0;
                }
            }
            assert_eq!(
                serde_json::to_vec(&white).unwrap(),
                serde_json::to_vec(&normalized).unwrap(),
                "seed {seed}: layout, conveyors, trees, population or portals changed"
            );
        }
    }

    #[test]
    fn compact_platforms_have_only_two_tile_bridges() {
        let content = Content::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data"
        )))
        .unwrap();
        for seed in 0..48 {
            let map = generate(&content, seed).unwrap();
            assert_eq!(
                serde_json::to_vec(&map).unwrap(),
                serde_json::to_vec(&generate(&content, seed).unwrap()).unwrap()
            );
            let floor = |x: i32, y: i32| {
                (0xa0..=0xa5).contains(&map.cells[(y * map.width + x) as usize].ground)
            };
            let mut rooms = Vec::new();
            for cy in (12..map.height).step_by(16) {
                for cx in (12..map.width).step_by(16) {
                    if !floor(cx, cy) {
                        continue;
                    }
                    let radius = (1..=9).take_while(|r| floor(cx + r, cy + r)).count() as i32;
                    let kind = map.cells[(cy * map.width + cx) as usize].object;
                    match kind {
                        Some(0x70d) => assert_eq!(radius, 6, "seed {seed}: oversized entrance"),
                        Some(0xd06) => assert_eq!(radius, 8, "seed {seed}: boss arena changed"),
                        _ => assert!(
                            (3..=5).contains(&radius),
                            "seed {seed}: oversized ordinary platform radius {radius}"
                        ),
                    }
                    assert_eq!(map.cells[(cy * map.width + cx) as usize].ground, 0xa0);
                    rooms.push((cx, cy, radius));
                }
            }
            assert!((8..=14).contains(&rooms.len()));
            for &(x, y, radius) in &rooms {
                for (dx, dy) in [(1, 0), (0, 1)] {
                    let Some(&(_, _, other)) = rooms
                        .iter()
                        .find(|&&(cx, cy, _)| (cx, cy) == (x + 16 * dx, y + 16 * dy))
                    else {
                        continue;
                    };
                    assert!(
                        radius + other < 15,
                        "seed {seed}: unlinked platforms can touch"
                    );
                    let linked = floor(x + dx * (radius + 1), y + dy * (radius + 1));
                    for along in radius + 1..16 - other {
                        for across in -1..=2 {
                            assert_eq!(
                                floor(x + dx * along + dy * across, y + dy * along + dx * across),
                                linked && (0..=1).contains(&across),
                                "seed {seed}: bridge width or unintended room join"
                            );
                        }
                    }
                }
            }
            let start = (map.spawns[0].0 as i32, map.spawns[0].1 as i32);
            let mut seen = BTreeSet::from([start]);
            let mut todo = vec![start];
            while let Some((x, y)) = todo.pop() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let p = (x + dx, y + dy);
                    if floor(p.0, p.1) && seen.insert(p) {
                        todo.push(p);
                    }
                }
            }
            assert_eq!(
                seen.len(),
                map.cells
                    .iter()
                    .filter(|c| (0xa0..=0xa5).contains(&c.ground))
                    .count(),
                "seed {seed}: disconnected floor/boss"
            );
            assert!(map.cells.iter().filter(|c| c.object == Some(0xd05)).count() <= 5);
        }
    }
}
