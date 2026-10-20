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
                    6
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
