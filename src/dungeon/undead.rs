use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

#[derive(Clone, Copy)]
struct Room {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    depth: usize,
    round: bool,
}
impl Room {
    fn center(self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
    fn overlaps(self, b: Self) -> bool {
        self.x <= b.x + b.w
            && b.x <= self.x + self.w
            && self.y <= b.y + b.h
            && b.y <= self.y + self.h
    }
}

// A lair room holds one enemy group, as in the map this server shipped before the
// procedural generator (data/undead-lair.wmap): skeletons, mummies, constructs, slimes,
// reapers and vampires each keep their own rooms.
const FAMILIES: [&[u16]; 6] = [
    &[0xd91, 0xd92, 0xd93, 0xd94, 0xd95],
    &[0xd96, 0xd97, 0xd98],
    &[0xd99, 0xd9a],
    &[0xd9b, 0xd9e],
    &[0xda4],
    &[0xda5, 0xda6],
];

// Period guides establish the room shapes, not the lost official generator's distributions.
pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut rooms = vec![Room {
        x: 0,
        y: 0,
        w: 27,
        h: 27,
        depth: 0,
        round: true,
    }];
    let mut edges: Vec<Room> = vec![];
    let mut entrance = None;
    for number in 1..rng.gen_range(32..=40) {
        let round = rng.gen_bool(0.1);
        let w = if round { 13 } else { rng.gen_range(9..=13) };
        let h = if round { w } else { rng.gen_range(9..=13) };
        let mut candidates = vec![];
        for (parent, p) in rooms.iter().enumerate() {
            if p.depth == 10 || entrance == Some(parent) || parent == 0 && number > 1 {
                continue;
            }
            let (cx, cy) = p.center();
            for horizontal in [true, false] {
                for positive in [true, false] {
                    for offset in [-2, 0, 2] {
                        let mut r = Room {
                            x: cx - w / 2,
                            y: cy - h / 2,
                            w,
                            h,
                            depth: p.depth + 1,
                            round,
                        };
                        let corridor = if horizontal {
                            r.x = if positive { p.x + p.w + 1 } else { p.x - w - 1 };
                            r.y += offset;
                            Room {
                                x: if positive { p.x + p.w - 3 } else { r.x + w - 3 },
                                y: cy + offset - 1,
                                w: 7,
                                h: 3,
                                depth: 0,
                                round: false,
                            }
                        } else {
                            r.y = if positive { p.y + p.h + 1 } else { p.y - h - 1 };
                            r.x += offset;
                            Room {
                                x: cx + offset - 1,
                                y: if positive { p.y + p.h - 3 } else { r.y + h - 3 },
                                w: 3,
                                h: 7,
                                depth: 0,
                                round: false,
                            }
                        };
                        if rooms.iter().any(|o| r.overlaps(*o))
                            || edges.iter().any(|e| r.overlaps(*e))
                            || rooms
                                .iter()
                                .enumerate()
                                .any(|(i, o)| i != parent && corridor.overlaps(*o))
                        {
                            continue;
                        }
                        candidates.push((r, corridor));
                    }
                }
            }
        }
        if entrance.is_none() {
            let depth = candidates
                .iter()
                .map(|(r, _)| r.depth)
                .max()
                .context("Undead Lair has no frontier")?;
            candidates.retain(|(r, _)| r.depth == depth);
        }
        let (room, corridor) = *candidates
            .choose(&mut rng)
            .with_context(|| format!("Undead Lair seed {seed}: no placement for room {number}"))?;
        if room.depth == 10 && entrance.is_none() {
            entrance = Some(number);
        }
        rooms.push(room);
        edges.push(corridor);
    }
    let entrance = entrance.context("Undead Lair has no entrance ten rooms from boss")?;
    let min_x = rooms.iter().map(|r| r.x).min().unwrap() - 2;
    let min_y = rooms.iter().map(|r| r.y).min().unwrap() - 2;
    for room in rooms.iter_mut().chain(&mut edges) {
        room.x -= min_x;
        room.y -= min_y;
    }
    let width = rooms.iter().map(|r| r.x + r.w).max().unwrap() + 2;
    let height = rooms.iter().map(|r| r.y + r.h).max().unwrap() + 2;
    ensure!(
        width <= 512 && height <= 512,
        "Undead Lair seed {seed}: map exceeds bounds"
    );
    let mut map = Map {
        width,
        height,
        cells: vec![
            Cell {
                ground: 0xfe,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            (width * height) as usize
        ],
        spawns: vec![],
    };
    for (number, room) in rooms.iter().enumerate() {
        let (cx, cy) = room.center();
        let mut sites = vec![];
        for y in room.y..room.y + room.h {
            for x in room.x..room.x + room.w {
                let (dx, dy) = (x - cx, y - cy);
                if room.round && dx * dx + dy * dy > (room.w / 2).pow(2) {
                    continue;
                }
                let i = (y * width + x) as usize;
                map.cells[i].region = number as u8 + 2; // room identity, region 1 marks the spawn
                map.cells[i].ground = if number == 0 && (16..=25).contains(&(dx * dx + dy * dy)) {
                    0x61
                } else if rng.gen_ratio(1, 32) {
                    0xd4
                } else {
                    5
                };
                if number != 0 && number != entrance && dx.abs() > 2 && dy.abs() > 2 {
                    sites.push(i);
                }
            }
        }
        if number != 0 && number != entrance {
            sites.shuffle(&mut rng);
            let family = *FAMILIES.choose(&mut rng).unwrap();
            for _ in 0..rng.gen_range(6..=10) {
                let i = sites.pop().context("Undead Lair room has no enemy site")?;
                map.cells[i].object = Some(*family.choose(&mut rng).unwrap());
            }
            for kind in [0xda2, 0xda3, 0x1801, 0x1802, 0x1803] {
                if rng.gen_bool(0.5) {
                    let i = sites
                        .pop()
                        .context("Undead Lair room has no decoration site")?;
                    map.cells[i].object = Some(kind);
                }
            }
        }
    }
    for corridor in edges {
        for y in corridor.y..corridor.y + corridor.h {
            for x in corridor.x..corridor.x + corridor.w {
                let cell = &mut map.cells[(y * width + x) as usize];
                cell.ground = 5;
                cell.object = None;
            }
        }
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = (y * width + x) as usize;
            if map.cells[i].ground == 0xfe
                && (-1..=1).any(|dy| {
                    (-1..=1)
                        .any(|dx| map.cells[((y + dy) * width + x + dx) as usize].ground != 0xfe)
                })
            {
                map.cells[i].object = Some(if rng.gen_ratio(1, 20) { 0x1eb } else { 0x120 });
            }
        }
    }
    let (bx, by) = rooms[0].center();
    map.cells[(by * width + bx) as usize].object = Some(0xd90);
    let (sx, sy) = rooms[entrance].center();
    let cell = &mut map.cells[(sy * width + sx) as usize];
    cell.ground = 0x61;
    cell.object = Some(0x703);
    cell.region = 1;
    map.spawns.push((sx as f32 + 0.5, sy as f32 + 0.5));
    // Build 122 names endpoint candles; their count/spacing and blood frequency are reconstructed.
    for (endpoint, x, y) in [("entrance", sx, sy), ("boss", bx, by)] {
        for (dx, kind) in [(-2, 0x1801), (2, 0x1802)] {
            let x = x + dx;
            let cell = &mut map.cells[(y * width + x) as usize];
            ensure!(
                matches!(cell.ground, 5 | 0xd4) && cell.object.is_none(),
                "Undead Lair seed {seed}: {endpoint} candle requires empty floor at ({x}, {y}); ground={:#x}, object={:?}, region={}",
                cell.ground,
                cell.object,
                cell.region
            );
            cell.object = Some(kind);
        }
    }
    for cell in &map.cells {
        ensure!(
            content.grounds.contains_key(&cell.ground),
            "unknown Undead Lair ground"
        );
        if let Some(kind) = cell.object {
            content.get(kind)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        width,
        height,
        "generated reconstructed Undead Lair"
    );
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn content() -> Content {
        Content::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data"
        )))
        .unwrap()
    }

    #[test]
    fn endpoint_candles_preserve_routes_and_room_families() {
        let content = content();
        for kind in [0x1801, 0x1802] {
            let candle = content.get(kind).unwrap();
            assert!(candle.static_object && !candle.blocks && !candle.enemy);
        }
        for seed in 0..48 {
            let map = generate(&content, seed).unwrap();
            let cell = |x, y| &map.cells[(y * map.width + x) as usize];
            assert_eq!(map.spawns.len(), 1);
            let (sx, sy) = map.spawns[0];
            let spawn = (sx as i32, sy as i32);
            assert_eq!((sx, sy), (spawn.0 as f32 + 0.5, spawn.1 as f32 + 0.5));
            assert_eq!(cell(spawn.0, spawn.1).region, 1);
            assert_eq!(cell(spawn.0, spawn.1).ground, 0x61);
            let mut endpoints = vec![spawn];
            for kind in [0x703, 0xd90] {
                let sites: Vec<_> = map
                    .cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.object == Some(kind))
                    .map(|(i, _)| (i as i32 % map.width, i as i32 / map.width))
                    .collect();
                assert_eq!(sites.len(), 1, "seed {seed}: endpoint {kind:#x}");
                if kind == 0x703 {
                    assert_eq!(sites[0], spawn);
                } else {
                    endpoints.push(sites[0]);
                }
            }
            let mut expected = BTreeSet::new();
            for &(x, y) in &endpoints {
                for (dx, kind) in [(-2, 0x1801), (2, 0x1802)] {
                    let c = cell(x + dx, y);
                    assert_eq!(
                        c.object,
                        Some(kind),
                        "seed {seed}: candle at ({}, {y})",
                        x + dx
                    );
                    assert!(matches!(c.ground, 5 | 0xd4));
                    expected.insert((x + dx, y));
                }
            }
            let walkable = |x, y| {
                (0..map.width).contains(&x)
                    && (0..map.height).contains(&y)
                    && content.grounds[&cell(x, y).ground] > 0.
                    && cell(x, y)
                        .object
                        .is_none_or(|k| !content.objects[&k].blocks)
            };
            let mut reached = BTreeSet::from([spawn]);
            let mut todo = vec![spawn];
            while let Some((x, y)) = todo.pop() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let p = (x + dx, y + dy);
                    if walkable(p.0, p.1) && reached.insert(p) {
                        todo.push(p);
                    }
                }
            }
            assert_eq!(
                reached.len(),
                map.cells
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| walkable(*i as i32 % map.width, *i as i32 / map.width))
                    .count(),
                "seed {seed}: disconnected walkable floor"
            );
            for p in endpoints.into_iter().chain(expected) {
                assert!(
                    reached.contains(&p),
                    "seed {seed}: unreachable endpoint/candle {p:?}"
                );
            }
            let mut families = BTreeMap::new();
            for c in &map.cells {
                if let Some(family) = FAMILIES
                    .iter()
                    .position(|f| c.object.is_some_and(|k| f.contains(&k)))
                {
                    assert!(c.region > 2, "seed {seed}: enemy in endpoint/corridor");
                    if let Some(previous) = families.insert(c.region, family) {
                        assert_eq!(previous, family, "seed {seed}: mixed room family");
                    }
                }
            }
            assert!(
                (30..=38).contains(&families.len()),
                "seed {seed}: ordinary rooms"
            );
        }
    }

    #[test]
    fn bloody_floor_uses_selected_nonhazardous_ground() {
        let content = content();
        assert_eq!(content.grounds[&0xd4], content.grounds[&5]);
        assert_eq!(content.pushes[&0xd4], (0., 0.));
        assert!(!content.hazards.contains_key(&0xd4));
        for seed in 0..48 {
            let map = generate(&content, seed).unwrap();
            assert!(
                map.cells.iter().any(|c| c.ground == 0xd4),
                "seed {seed}: no bloody floor"
            );
            assert!(map.cells.iter().all(|c| c.ground != 6));
            assert!(
                map.cells.iter().any(|c| c.object == Some(0x1803)),
                "seed {seed}: blood pools lost"
            );
        }
    }
}
