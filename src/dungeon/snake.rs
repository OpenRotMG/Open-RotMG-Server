use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

pub const BOSS: u16 = 0x917;
pub const PET: u16 = 0x918;
pub const SWARM: u16 = 0x919;
pub const GUARD: u16 = 0xe26;
pub const BUTTON: u16 = 0xe27;
pub const SPAWNER: u16 = 0xe28;
pub const DART: u16 = 0xe25;
pub const GUARD_RAGE_HP: i32 = 3300;

// The modified 7.0 server provides explicit timings, not an authenticated 123.5.1 AI script.
pub fn phase(elapsed: u64) -> (u64, u64) {
    let cycle = elapsed / 31_000;
    let t = elapsed % 31_000;
    let (phase, age) = match t {
        0..8000 => (0, t),
        8000..25000 => (1, t - 8000),
        _ => (2, t - 25000),
    };
    (cycle * 3 + phase, age)
}

pub fn initial_condition(kind: u16) -> i32 {
    match kind {
        BOSS => crate::condition::bit(crate::condition::INVULNERABLE),
        BUTTON | SPAWNER | DART => crate::condition::bit(crate::condition::INVINCIBLE),
        _ => 0,
    }
}

pub fn shield(elapsed: u64) -> bool {
    let (stage, age) = phase(elapsed);
    match stage % 3 {
        0 => age % 3000 < 2000,
        1 => age >= 16000 || !(1000..2000).contains(&(age % 3000)),
        _ => (1500..3500).contains(&age) || age >= 5000,
    }
}

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let directions = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    let mut rooms = vec![(0i32, 0i32)];
    let mut edges = Vec::new();
    for parent in 0..6 {
        let (x, y) = rooms[parent];
        let choices: Vec<_> = directions
            .iter()
            .map(|(dx, dy)| (x + dx, y + dy))
            .filter(|p| !rooms.contains(p))
            .collect();
        rooms.push(
            *choices
                .choose(&mut rng)
                .context("Snake Pit spine exhausted")?,
        );
        edges.push((parent, parent + 1));
    }
    for _ in 0..rng.gen_range(3..=8) {
        let choices: Vec<_> = rooms
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 0 && *i != 6)
            .flat_map(|(i, (x, y))| directions.iter().map(move |(dx, dy)| (i, (x + dx, y + dy))))
            .filter(|(_, p)| !rooms.contains(p))
            .collect();
        let (parent, pos) = *choices
            .choose(&mut rng)
            .context("Snake Pit branches exhausted")?;
        edges.push((parent, rooms.len()));
        rooms.push(pos);
    }
    let ordinary = rooms.len();
    // Keep existing circular-room rolls independent of optional treasure placement.
    let mut treasure_rng = rand::rngs::StdRng::seed_from_u64(seed ^ 0x7472_6561_7375_7265);
    let mut candidates: Vec<_> = (1..ordinary)
        .filter(|&i| i != 6)
        .flat_map(|i| [-1, 1].map(|dx| (i, (rooms[i].0 + dx, rooms[i].1))))
        .filter(|&(parent, pos)| {
            !rooms.contains(&pos)
                && rooms
                    .iter()
                    .enumerate()
                    .all(|(i, &(x, y))| i == parent || (x - pos.0).abs() + (y - pos.1).abs() > 1)
        })
        .collect();
    candidates.shuffle(&mut treasure_rng);
    let mut spaces: Vec<(usize, (i32, i32))> = Vec::new();
    for candidate in candidates {
        if spaces
            .iter()
            .all(|&(_, (x, y))| (x - candidate.1.0).abs() + (y - candidate.1.1).abs() > 1)
        {
            spaces.push(candidate);
            if spaces.len() == 3 {
                break;
            }
        }
    }
    for &(parent, pos) in &spaces[..treasure_rng.gen_range(0..=spaces.len())] {
        edges.push((parent, rooms.len()));
        rooms.push(pos);
    }
    let min_x = rooms
        .iter()
        .map(|p| p.0)
        .min()
        .context("Snake Pit has no rooms")?;
    let min_y = rooms
        .iter()
        .map(|p| p.1)
        .min()
        .context("Snake Pit has no rooms")?;
    let centers: Vec<_> = rooms
        .iter()
        .map(|(x, y)| ((x - min_x) * 32 + 20, (y - min_y) * 32 + 20))
        .collect();
    let width = centers.iter().map(|p| p.0).max().unwrap() + 21;
    let height = centers.iter().map(|p| p.1).max().unwrap() + 21;
    let mut map = Map {
        width,
        height,
        cells: vec![
            Cell {
                ground: 0xff,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            (width * height) as usize
        ],
        spawns: vec![(centers[0].0 as f32 + 0.5, centers[0].1 as f32 + 0.5)],
    };
    for (i, &(cx, cy)) in centers.iter().enumerate() {
        if i >= ordinary {
            treasure_room(&mut map, cx, cy);
            continue;
        }
        let radius = if i == 6 { 16 } else { rng.gen_range(7..=9) };
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y > radius * radius {
                    continue;
                }
                let cell = &mut map.cells[((cy + y) * width + cx + x) as usize];
                cell.ground = if i == 6 && x * x + y * y > (radius - 1) * (radius - 1) {
                    0x32
                } else {
                    match rng.gen_range(0..10) {
                        0 => 0x1e,
                        1 => 0x1f,
                        _ => 0xca,
                    }
                };
            }
        }
        if i != 0 && i != 6 {
            for (dx, dy) in [(-4, 0), (4, 0)] {
                map.cells[((cy + dy) * width + cx + dx) as usize].object = Some(0x173);
            }
            for (dx, dy) in [(-4, -4), (4, -4), (-4, 4), (4, 4)] {
                map.cells[((cy + dy) * width + cx + dx) as usize].object =
                    Some(rng.gen_range(0x225..=0x229));
            }
        }
    }
    for &(a, b) in &edges {
        let ((x0, y0), (x1, y1)) = (centers[a], centers[b]);
        for y in
            y0.min(y1) - if y0 == y1 { 4 } else { 0 }..=y0.max(y1) + if y0 == y1 { 4 } else { 0 }
        {
            for x in x0.min(x1) - if x0 == x1 { 4 } else { 0 }
                ..=x0.max(x1) + if x0 == x1 { 4 } else { 0 }
            {
                let cell = &mut map.cells[(y * width + x) as usize];
                if cell.ground == 0xff {
                    cell.ground = 0x16;
                    cell.object = Some(0x172);
                }
            }
        }
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let index = (y * width + x) as usize;
            if map.cells[index].ground == 0xff
                && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                    .iter()
                    .any(|(x, y)| map.cells[(y * width + x) as usize].ground != 0xff)
            {
                map.cells[index].object = Some(match rng.gen_range(0..10) {
                    0 => 0x1ec,
                    1 => 0x1ed,
                    _ => 0x170,
                });
            }
        }
    }
    let (sx, sy) = centers[0];
    for y in -2i32..=2 {
        for x in -2i32..=2 {
            if x.abs() + y.abs() <= 2 {
                map.cells[((sy + y) * width + sx + x) as usize].ground = 0x32;
            }
        }
    }
    map.cells[(sy * width + sx) as usize].region = 1;
    map.cells[(sy * width + sx) as usize].object = Some(0x703);
    let (bx, by) = centers[6];
    // Boss cover is permanent, unlike tunnel blocks: https://bluenosersguide.weebly.com/snake-pit.html
    for (dx, dy) in [(-6, 0), (6, 0), (0, -6), (0, 6)] {
        for along in -2..=2 {
            for thick in 0..2 {
                let (x, y) = if dx == 0 {
                    (bx + along, by + dy + thick * dy.signum())
                } else {
                    (bx + dx + thick * dx.signum(), by + along)
                };
                map.cells[(y * width + x) as usize].ground = 0x16;
                map.cells[(y * width + x) as usize].object = Some(0x170);
            }
        }
    }
    map.cells[(by * width + bx) as usize].object = Some(BOSS);
    for cell in &map.cells {
        ensure!(
            content.grounds.contains_key(&cell.ground),
            "unknown Snake Pit ground"
        );
        if let Some(kind) = cell.object {
            content.get(kind)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        treasure_rooms = rooms.len() - ordinary,
        treasure_slots = spaces.len(),
        width,
        height,
        "generated branching Snake Pit with diggable tunnels"
    );
    Ok(map)
}

fn treasure_room(map: &mut Map, cx: i32, cy: i32) {
    for dy in -16i32..=16 {
        for dx in -7i32..=7 {
            let cell = &mut map.cells[((cy + dy) * map.width + cx + dx) as usize];
            cell.ground = 0xca;
            if dy.abs() == 7 && dx.abs() > 1 {
                cell.ground = 0x16;
                cell.object = Some(0x170);
            }
        }
    }
    map.cells[((cy - 12) * map.width + cx) as usize].object = Some(BUTTON);
    map.cells[(cy * map.width + cx) as usize].object = Some(SPAWNER);
    for dy in [-4, -2, 0, 2, 4] {
        for dx in [-7, 7] {
            map.cells[((cy + dy) * map.width + cx + dx) as usize].object = Some(DART);
        }
    }
}
