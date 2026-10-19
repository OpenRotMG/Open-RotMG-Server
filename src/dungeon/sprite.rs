use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

const COLORS: [u8; 5] = [0xa0, 0xa1, 0xa2, 0xa3, 0xa4];
const TREES: [u16; 5] = [0x175, 0x176, 0x177, 0x178, 0x179];
const SPRITES: [u16; 5] = [0xd00, 0xd01, 0xd02, 0xd03, 0xd04];

// Period guides describe conveyor floors, a tiled/solid superstition, and zero to five gods.
// No official generator was recovered, so each seed builds that variation explicitly.
pub fn generate(content: &Content, seed: u64) -> Result<Map> {
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
    let tiled_world = rng.gen_bool(0.5);
    let mut room_color = Vec::new();
    for (i, &(cx, cy)) in centers.iter().enumerate() {
        let primary = COLORS[rng.gen_range(0..COLORS.len())];
        let secondary = COLORS[rng.gen_range(0..COLORS.len())];
        let tiled = if tiled_world {
            rng.gen_bool(0.8)
        } else {
            rng.gen_bool(0.2)
        };
        room_color.push((primary, secondary, tiled));
        let radius = if i == 0 || i == boss {
            8
        } else {
            rng.gen_range(5..=7)
        };
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                if (x - cx) * (x - cx) + (y - cy) * (y - cy) > radius * radius {
                    continue;
                }
                paint(&mut map, x, y, primary, secondary, tiled);
            }
        }
    }
    for &(a, b) in &edges {
        let ((x0, y0), (x1, y1)) = (centers[a], centers[b]);
        let (primary, secondary, tiled) = room_color[b];
        for y in y0.min(y1) - 1..=y0.max(y1) + 1 {
            for x in x0.min(x1) - 1..=x0.max(x1) + 1 {
                paint(&mut map, x, y, primary, secondary, tiled);
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
        tiled_world,
        width,
        height,
        "generated varied Sprite World"
    );
    Ok(map)
}

fn paint(map: &mut Map, x: i32, y: i32, primary: u8, secondary: u8, tiled: bool) {
    let cell = &mut map.cells[(y * map.width + x) as usize];
    cell.ground = if tiled && (x + y) % 2 == 1 {
        secondary
    } else {
        primary
    };
}

fn place(map: &mut Map, x: i32, y: i32, kind: u16) {
    let cell = &mut map.cells[(y * map.width + x) as usize];
    if (0xa0..=0xa4).contains(&cell.ground) && cell.object.is_none() {
        cell.object = Some(kind);
    }
}
