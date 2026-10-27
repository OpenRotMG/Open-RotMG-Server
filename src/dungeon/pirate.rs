use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

pub const BOSS: u16 = 0x927;
const WALL: u16 = 0x1ce;
const PALM: u16 = 0x18e;
const EXIT: u16 = 0x703;
const FLOOR: u8 = 0x0c;
const SAND: u8 = 0xbd;
const WATER: u8 = 0x73;
const COMPOSITE: u8 = 0xfd;
const SPACE: u8 = 0xfe;
const MAX_SIDE: i32 = 336;
const MAX_ACTORS: usize = 103;
const OFFICERS: &[u16] = &[0x683, 0x684, 0x685, 0x686];
const PIRATES: &[u16] = &[0x687, 0x688, 0x689];
const PETS: &[u16] = &[0x68a, 0x68b, 0x68c, 0x68d, 0x68e, 0x68f];
const DIRECTIONS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

// Reconstruction, not official RNG/distributions. Comparator: realm-src 1855c017,
// DungeonGen/Templates/PirateCave, Copyright (C) 2015 creepylava, AGPL-3.0-or-later.
// Uses this server's Rust room/corridor and wall-configuration patterns, not a C# port or assets.
pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    build(content, seed).with_context(|| format!("Pirate Cave generation seed {seed}"))
}

fn build(content: &Content, seed: u64) -> Result<Map> {
    for ground in [FLOOR, SAND, WATER, COMPOSITE, SPACE] {
        ensure!(
            content.grounds.contains_key(&ground),
            "unknown ground {ground:#x}"
        );
    }
    for kind in [BOSS, WALL, PALM, EXIT]
        .iter()
        .chain(OFFICERS)
        .chain(PIRATES)
        .chain(PETS)
    {
        content.get(*kind)?;
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let steps = rng.gen_range(5..=7);
    let rotation = rng.gen_range(0..4);
    let mut sites = vec![(0, 0)];
    let mut edges = Vec::new();
    // A monotone spine cannot trap itself; finite frontier enumeration needs no retries.
    for parent in 0..steps {
        let (dx, dy) = DIRECTIONS[(rotation + rng.gen_range(0..2)) % 4];
        let (x, y) = sites[parent];
        sites.push((x + dx, y + dy));
        edges.push((parent, parent + 1));
    }
    let boss_room = steps;
    for _ in 0..rng.gen_range(3..=5) {
        let candidates: Vec<_> = sites
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0 && *i != boss_room)
            .flat_map(|(parent, &(x, y))| {
                DIRECTIONS
                    .iter()
                    .map(move |&(dx, dy)| (parent, (x + dx, y + dy)))
            })
            .filter(|&(_, (x, y))| {
                !sites.contains(&(x, y))
                    && sites
                        .iter()
                        .filter(|&&(sx, sy)| (sx - x).abs() + (sy - y).abs() == 1)
                        .count()
                        == 1
            })
            .collect();
        let &(parent, site) = candidates.choose(&mut rng).context("no room frontier")?;
        edges.push((parent, sites.len()));
        sites.push(site);
    }
    let spacing = rng.gen_range(21..=25);
    let min_x = sites.iter().map(|p| p.0).min().unwrap();
    let min_y = sites.iter().map(|p| p.1).min().unwrap();
    let centers: Vec<_> = sites
        .iter()
        .map(|&(x, y)| ((x - min_x) * spacing + 14, (y - min_y) * spacing + 14))
        .collect();
    let width = centers.iter().map(|p| p.0).max().unwrap() + 15;
    let height = centers.iter().map(|p| p.1).max().unwrap() + 15;
    ensure!(
        width <= MAX_SIDE && height <= MAX_SIDE,
        "map {width}x{height} exceeds {MAX_SIDE}x{MAX_SIDE}"
    );
    let mut map = Map {
        width,
        height,
        cells: vec![
            Cell {
                ground: SPACE,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            (width * height) as usize
        ],
        spawns: vec![],
    };
    let index = |x: i32, y: i32| (y * width + x) as usize;
    let mut populations = Vec::new();
    for (room, &(cx, cy)) in centers.iter().enumerate() {
        let round = room == 0 || room == boss_room;
        let (w, h) = if round {
            (21, 21)
        } else {
            (rng.gen_range(8..=14), rng.gen_range(8..=14))
        };
        let mut population = Vec::new();
        for y in cy - h / 2..cy + (h + 1) / 2 {
            for x in cx - w / 2..cx + (w + 1) / 2 {
                let (dx, dy) = (x - cx, y - cy);
                if round && dx * dx + dy * dy > 100 {
                    continue;
                }
                let cell = &mut map.cells[index(x, y)];
                cell.ground = if room == 0 { SAND } else { FLOOR };
                cell.region = room as u8 + 2;
                // Reserve the central cross for unobstructed two-tile doors and the boss.
                if room != 0
                    && dx.abs() > 1
                    && dy.abs() > 1
                    && if round {
                        dx * dx + dy * dy <= 64
                    } else {
                        x > cx - w / 2
                            && x < cx + (w + 1) / 2 - 1
                            && y > cy - h / 2
                            && y < cy + (h + 1) / 2 - 1
                    }
                {
                    population.push(index(x, y));
                }
            }
        }
        population.shuffle(&mut rng);
        populations.push(population);
    }
    for &(a, b) in &edges {
        let ((x0, y0), (x1, y1)) = (centers[a], centers[b]);
        for y in y0.min(y1) - i32::from(y0 == y1)..=y0.max(y1) {
            for x in x0.min(x1) - i32::from(x0 == x1)..=x0.max(x1) {
                let cell = &mut map.cells[index(x, y)];
                if cell.ground == SPACE {
                    cell.ground = FLOOR;
                }
            }
        }
    }
    // Snapshot each dilation so scan order cannot turn a margin into unbounded water.
    for _ in 0..2 {
        let previous = map.cells.clone();
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let i = index(x, y);
                if previous[i].ground == SPACE
                    && DIRECTIONS.iter().any(|&(dx, dy)| {
                        matches!(previous[index(x + dx, y + dy)].ground, FLOOR | SAND | WATER)
                    })
                {
                    map.cells[i].ground = WATER;
                }
            }
        }
    }
    let shoreline = map.cells.clone();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            if shoreline[index(x, y)].ground == SPACE
                && (-1..=1)
                    .any(|dy| (-1..=1).any(|dx| shoreline[index(x + dx, y + dy)].ground != SPACE))
            {
                let cell = &mut map.cells[index(x, y)];
                cell.ground = COMPOSITE;
                cell.object = Some(WALL);
            }
        }
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            if map.cells[index(x, y)].object == Some(WALL) {
                let connection = DIRECTIONS.iter().fold(0_u32, |conn, &(dx, dy)| {
                    (conn << 8)
                        | if map.cells[index(x + dx, y + dy)].object == Some(WALL) {
                            1
                        } else {
                            2
                        }
                });
                map.cells[index(x, y)].config = format!("conn:{connection}");
            }
        }
    }
    for (room, population) in populations.iter_mut().enumerate().skip(1) {
        let boss = room == boss_room;
        for (family, count) in [
            (OFFICERS, rng.gen_range(if boss { 4..=7 } else { 0..=1 })),
            (PIRATES, rng.gen_range(if boss { 4..=7 } else { 3..=5 })),
            (PETS, if boss { 0 } else { rng.gen_range(0..=2) }),
        ] {
            for _ in 0..count {
                let i = population.pop().context("room has no population site")?;
                map.cells[i].object = Some(*family.choose(&mut rng).unwrap());
            }
        }
    }
    let (sx, sy) = centers[0];
    for (dx, dy) in [(-7, 0), (7, 0), (0, -7), (0, 7)] {
        map.cells[index(sx + dx, sy + dy)].object = Some(PALM);
    }
    let spawn = &mut map.cells[index(sx, sy)];
    spawn.object = Some(EXIT);
    spawn.region = 1;
    map.spawns.push((sx as f32 + 0.5, sy as f32 + 0.5));
    let (bx, by) = centers[boss_room];
    map.cells[index(bx, by)].object = Some(BOSS);
    let actors = map
        .cells
        .iter()
        .filter(|c| c.object.is_some_and(|k| content.objects[&k].enemy))
        .count();
    ensure!(
        actors <= MAX_ACTORS,
        "population {actors} exceeds {MAX_ACTORS}"
    );
    tracing::info!(
        seed,
        width,
        height,
        cells = map.cells.len(),
        rooms = centers.len(),
        actors,
        boss_x = bx,
        boss_y = by,
        spawn_x = sx,
        spawn_y = sy,
        "generated reconstructed Pirate Cave"
    );
    Ok(map)
}
