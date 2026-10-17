use crate::content::{Cell, Content, Map};
use anyhow::{Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

pub const MALPHAS: u16 = 0x90a;
pub const IDOL: u16 = 0xe1d;
pub const ROSTER: &[u16] = &[
    0x66d, 0x66e, 0x66f, 0x670, 0x671, 0x672, 0x908, 0x909, MALPHAS, 0x90b, IDOL,
];
pub const LAVA: u8 = 0x70;
pub const FLOOR: u8 = 0x3c;
pub const BRIDGE: u16 = 0x153;

#[derive(Clone, Copy)]
struct Room {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    depth: usize,
    tag: u8,
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
#[derive(Clone, Copy)]
struct Edge {
    parent: usize,
    child: usize,
    a: (i32, i32),
    b: (i32, i32),
}

fn attach(
    rooms: &[Room],
    parent: usize,
    size: (i32, i32, u8),
    door: (bool, bool, i32),
) -> (Room, Edge) {
    let (w, h, tag) = size;
    let (horizontal, positive, offset) = door;
    let p = rooms[parent];
    let inset = if horizontal { h / 2 } else { w / 2 };
    let mut r = Room {
        x: p.x,
        y: p.y,
        w,
        h,
        depth: p.depth + 1,
        tag,
    };
    let (a, b) = if horizontal {
        r.x = if positive { p.x + p.w + 2 } else { p.x - w - 2 };
        r.y = p.y + offset - inset;
        (
            (if positive { p.x + p.w - 1 } else { p.x }, p.y + offset),
            (if positive { r.x } else { r.x + w - 1 }, p.y + offset),
        )
    } else {
        r.y = if positive { p.y + p.h + 2 } else { p.y - h - 2 };
        r.x = p.x + offset - inset;
        (
            (p.x + offset, if positive { p.y + p.h - 1 } else { p.y }),
            (p.x + offset, if positive { r.y } else { r.y + h - 1 }),
        )
    };
    (
        r,
        Edge {
            parent,
            child: rooms.len(),
            a,
            b,
        },
    )
}
fn index(map: &Map, x: i32, y: i32) -> usize {
    (y * map.width + x) as usize
}
fn path(map: &mut Map, mut a: (i32, i32), b: (i32, i32)) {
    loop {
        let i = index(map, a.0, a.1);
        if map.cells[i].ground == LAVA {
            map.cells[i].object = Some(BRIDGE);
        }
        if a == b {
            break;
        }
        if a.0 != b.0 {
            a.0 += (b.0 - a.0).signum();
        } else {
            a.1 += (b.1 - a.1).signum();
        }
    }
}

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut rooms = vec![Room {
        x: 0,
        y: 0,
        w: 42,
        h: 42,
        depth: 0,
        tag: 0,
    }];
    let mut edges = Vec::new();
    let count = rng.gen_range(36..=52);
    let mut treasures = 0;
    while treasures < 3 && rng.gen_bool(0.2) {
        treasures += 1;
    }
    // The 2012 footage establishes the form, not the lost server's seed distribution.
    for number in 1..count + treasures + 1 {
        let (w, h, tag) = if number == count + treasures {
            (16, 16, 1)
        } else if number >= count {
            (15, 21, 16 + (number - count) as u8)
        } else {
            (8, 8, 0)
        };
        let mut candidates = Vec::new();
        for parent in 0..rooms.len() {
            if rooms[parent].tag != 0 || parent == 0 && rooms.len() > 1 {
                continue;
            }
            for horizontal in [true, false] {
                let offsets = if parent == 0 {
                    12..30
                } else {
                    2..(if horizontal {
                        rooms[parent].h
                    } else {
                        rooms[parent].w
                    }) - 2
                };
                for offset in offsets {
                    for positive in [true, false] {
                        let (r, edge) =
                            attach(&rooms, parent, (w, h, tag), (horizontal, positive, offset));
                        if rooms.iter().any(|other| r.overlaps(*other)) {
                            continue;
                        }
                        let corridor = Room {
                            x: edge.a.0.min(edge.b.0) - 1,
                            y: edge.a.1.min(edge.b.1) - 1,
                            w: (edge.a.0 - edge.b.0).abs() + 3,
                            h: (edge.a.1 - edge.b.1).abs() + 3,
                            depth: 0,
                            tag: 0,
                        };
                        if rooms
                            .iter()
                            .enumerate()
                            .any(|(i, room)| i != parent && corridor.overlaps(*room))
                        {
                            continue;
                        }
                        candidates.push((r, edge));
                    }
                }
            }
        }
        if tag == 1
            && let Some(depth) = candidates.iter().map(|(r, _)| r.depth).max()
        {
            candidates.retain(|(r, _)| r.depth == depth);
        }
        let (r, edge) = *candidates
            .choose_weighted(&mut rng, |(_, e)| {
                if tag == 0 && e.parent + 1 == rooms.len() {
                    32
                } else {
                    1
                }
            })
            .map_err(|error| {
                anyhow::anyhow!("Abyss seed {seed}: room {number} has no valid frontier: {error}")
            })?;
        rooms.push(r);
        edges.push(edge);
    }
    let min_x = rooms.iter().map(|r| r.x).min().unwrap() - 3;
    let min_y = rooms.iter().map(|r| r.y).min().unwrap() - 3;
    for r in &mut rooms {
        r.x -= min_x;
        r.y -= min_y;
    }
    for e in &mut edges {
        e.a.0 -= min_x;
        e.b.0 -= min_x;
        e.a.1 -= min_y;
        e.b.1 -= min_y;
    }
    let width = rooms.iter().map(|r| r.x + r.w).max().unwrap() + 3;
    let height = rooms.iter().map(|r| r.y + r.h).max().unwrap() + 3;
    ensure!(
        width <= 1024 && height <= 1024,
        "Abyss seed {seed} exceeds bounded map dimensions"
    );
    let empty = Cell {
        ground: 0xfe,
        object: None,
        config: String::new(),
        terrain: 0,
        region: 0,
    };
    let mut map = Map {
        width,
        height,
        cells: vec![empty; (width * height) as usize],
        spawns: vec![],
    };
    let phase = rng.gen_range(0f32..std::f32::consts::TAU);
    for (number, room) in rooms.iter().enumerate() {
        for y in room.y..room.y + room.h {
            for x in room.x..room.x + room.w {
                let i = index(&map, x, y);
                let band =
                    (x as f32 * 0.29 + y as f32 * 0.21 + (y as f32 * 0.13 + phase).sin() * 1.8)
                        .sin();
                map.cells[i].ground = if room.tag >= 16 {
                    0x69
                } else if band > 0.35 {
                    LAVA
                } else {
                    FLOOR
                };
                map.cells[i].region = if room.tag >= 16 { room.tag } else { 0 };
            }
        }
        if number == 0 {
            let (cx, cy) = room.center();
            for y in room.y..room.y + room.h {
                for x in room.x..room.x + room.w {
                    let i = index(&map, x, y);
                    let (dx, dy) = (
                        (x as f32 - cx as f32 + 0.5).abs(),
                        (y as f32 - cy as f32 + 0.5).abs(),
                    );
                    let radius = dx.max(dy);
                    let c = &mut map.cells[i];
                    if dx > 14.5 && dy > 14.5 || dx > 11.5 && dy > 17.5 || dy > 11.5 && dx > 17.5 {
                        c.ground = 0xfe;
                        c.object = None;
                        continue;
                    }
                    if c.ground != LAVA {
                        c.ground = if radius < 12. { 0x69 } else { 0x36 };
                    }
                    // Period footage and Build 122 establish the two squared bridge rings, not the seed distribution.
                    if (4.5..=5.5).contains(&radius) || (12.5..=13.5).contains(&radius) {
                        c.ground = LAVA;
                        c.object = Some(BRIDGE);
                    }
                }
            }
        }
        if room.tag >= 16 {
            let (cx, cy) = room.center();
            for y in room.y..room.y + room.h {
                for x in room.x..room.x + room.w {
                    let i = index(&map, x, y);
                    if (x - cx).abs() > 4 || (y - cy).abs() > 5 {
                        map.cells[i].ground = LAVA;
                    }
                }
            }
        }
    }
    for edge in &edges {
        for y in edge.a.1.min(edge.b.1)..=edge.a.1.max(edge.b.1) {
            for x in edge.a.0.min(edge.b.0)..=edge.a.0.max(edge.b.0) {
                for side in -1..=1 {
                    let (x, y) = if edge.a.0 == edge.b.0 {
                        (x + side, y)
                    } else {
                        (x, y + side)
                    };
                    let i = index(&map, x, y);
                    map.cells[i].ground = FLOOR;
                }
            }
        }
        path(&mut map, rooms[edge.parent].center(), edge.a);
        path(&mut map, edge.a, edge.b);
        path(&mut map, edge.b, rooms[edge.child].center());
    }
    for room in &rooms {
        let (cx, cy) = room.center();
        if room.tag == 1 {
            for y in cy - 2..=cy + 2 {
                for x in cx - 2..=cx + 2 {
                    let i = index(&map, x, y);
                    map.cells[i].ground = FLOOR;
                    map.cells[i].object = None;
                }
            }
            let i = index(&map, cx, cy);
            map.cells[i].object = Some(0x703);
            map.cells[i].region = 1;
            map.spawns.push((cx as f32 + 0.5, cy as f32 + 0.5));
        } else if room.tag >= 16 {
            let i = index(&map, cx, cy);
            map.cells[i].object = Some(IDOL);
        } else if room.depth > 0 {
            for _ in 0..rng.gen_range(4..=7) {
                let x = rng.gen_range(room.x..room.x + room.w);
                let y = rng.gen_range(room.y..room.y + room.h);
                let i = index(&map, x, y);
                if map.cells[i].object.is_none() {
                    map.cells[i].object = Some(rng.gen_range(0x66d..=0x672));
                }
            }
            for _ in 0..2 {
                let i = index(
                    &map,
                    rng.gen_range(room.x..room.x + room.w),
                    rng.gen_range(room.y..room.y + room.h),
                );
                if map.cells[i].ground != LAVA && map.cells[i].object.is_none() {
                    map.cells[i].object = Some(0x1fa);
                }
            }
        }
    }
    let (cx, cy) = rooms[0].center();
    let i = index(&map, cx, cy);
    map.cells[i].ground = 0x69;
    map.cells[i].object = Some(MALPHAS);
    for (dx, dy) in [(-4, -9), (5, -9), (-9, 0), (10, 0), (-4, 8), (5, 8)]
        .into_iter()
        .take(rng.gen_range(3..=6))
    {
        let i = index(&map, cx + dx, cy + dy);
        map.cells[i].object = Some(0x651);
    }
    let mut walls = vec![];
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = index(&map, x, y);
            if map.cells[i].ground == 0xfe
                && (-1..=1).any(|dy| {
                    (-1..=1).any(|dx| map.cells[index(&map, x + dx, y + dy)].ground != 0xfe)
                })
            {
                walls.push(i);
            }
        }
    }
    for i in walls {
        map.cells[i].object = Some(if rng.gen_bool(0.1) { 0x151 } else { 0x150 });
    }
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Abyss ground {:#x}",
            c.ground
        );
        if let Some(k) = c.object {
            content.get(k)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        treasures,
        depth = rooms.last().unwrap().depth,
        width,
        height,
        "generated reconstructed 123.5.1 Abyss"
    );
    Ok(map)
}

pub fn initial_condition(kind: u16) -> i32 {
    if kind == MALPHAS {
        crate::condition::bit(crate::condition::INVULNERABLE)
    } else {
        0
    }
}

pub fn profile(kind: u16) -> crate::encounter::Profile {
    use crate::encounter::{Anchor, Motion::*, Profile};
    let (motion, speed, interval, range) = match kind {
        0x66d => (Wander, 12., 1000, 20.),
        0x66e..=0x670 => (
            Chase {
                sight: 11.,
                stop: 5.,
            },
            16.,
            1000,
            15.,
        ),
        0x671 | 0x672 => (
            Chase {
                sight: 11.,
                stop: 1.,
            },
            16.,
            500,
            4.8,
        ),
        0x908 => (
            Orbit {
                anchor: Anchor::Parent,
                radius: 9.,
            },
            6.,
            1200,
            5.,
        ),
        0x909 => (
            Chase {
                sight: 20.,
                stop: 0.,
            },
            6.,
            100,
            100.,
        ),
        0x90b => (
            Chase {
                sight: 15.,
                stop: 1.,
            },
            6.,
            200,
            4.,
        ),
        MALPHAS | IDOL => (Stationary, 0., 100, 20.),
        _ => unreachable!("unregistered Abyss enemy"),
    };
    Profile {
        motion,
        speed,
        interval,
        range,
    }
}
pub fn movement(kind: u16, elapsed: u64) -> (crate::encounter::Motion, f32) {
    use crate::encounter::Motion::*;
    if kind != MALPHAS {
        let p = profile(kind);
        return (p.motion, p.speed);
    }
    match phase(elapsed) {
        0 => (
            Chase {
                sight: 8.,
                stop: 2.,
            },
            3.,
        ),
        2 => (Wander, 8.),
        3 => (Wander, 2.),
        5 => (
            Chase {
                sight: 9.,
                stop: 2.,
            },
            4.,
        ),
        6 => (Retreat { distance: 4. }, 4.),
        _ => (Stationary, 0.),
    }
}
pub fn attacks(kind: u16, heading: f32, elapsed: u64) -> Vec<(u8, f32)> {
    let mut shots = Vec::new();
    let mut fan = |index, count, gap: f32, angle| {
        for i in 0..count {
            shots.push((
                index,
                angle + (i as f32 - (count - 1) as f32 / 2.) * gap.to_radians(),
            ));
        }
    };
    let due = |period| elapsed % period < 100;
    match kind {
        0x66d => fan(0, 5, 9., heading),
        0x66e..=0x672 | 0x908 => fan(0, 3, 6., heading),
        0x909 if (5000..5100).contains(&elapsed) => fan(0, 8, 45., 0.),
        0x909 => {}
        0x90b => fan(0, 1, 0., heading),
        MALPHAS => {
            let stage = phase(elapsed);
            let big = elapsed % 50200 >= 25300 && stage == 3;
            if (matches!(stage, 0 | 2 | 5) || big) && due(if stage == 5 { 2100 } else { 900 }) {
                fan(if big { 3 } else { 0 }, 1, 0., heading);
            }
            if stage == 2 && due(1200) {
                fan(2, 6, 60., 0.);
            }
            if big && due(700) {
                fan(
                    4,
                    3,
                    120.,
                    ((elapsed % 50200 - 25300) / 1400 % 3) as f32 * 40f32.to_radians(),
                );
            }
        }
        IDOL => {
            if due(400) {
                fan(0, 6, 60., elapsed as f32 / 1500.);
            }
            if due(1200) {
                fan(2, 4, 90., 0.);
            }
            if elapsed % 1400 < 100 || (400..500).contains(&(elapsed % 1400)) {
                fan(3, 3, 10., heading);
            }
        }
        _ => unreachable!("unregistered Abyss attack"),
    }
    shots
}

pub fn phase(elapsed: u64) -> u64 {
    match elapsed % 50200 {
        0..9000 => 0,
        9000..11500 => 1,
        11500..23500 => 2,
        23500..36500 => 3,
        36500..39000 => 4,
        39000..47000 => 5,
        _ => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::{BTreeSet, VecDeque},
        path::Path,
    };
    #[test]
    fn generated_abysses_have_seeded_complete_lava_safe_routes_and_optional_treasures() {
        let content = Content::load(Path::new("data")).unwrap();
        let mut variants = BTreeSet::new();
        for seed in 0..256 {
            let map = generate(&content, seed).unwrap();
            if seed == 7 {
                assert_eq!(
                    serde_json::to_vec(&map).unwrap(),
                    serde_json::to_vec(&generate(&content, seed).unwrap()).unwrap()
                );
            }
            let (x, y) = map.spawns[0];
            let start = index(&map, x as i32, y as i32);
            let mut seen = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            while let Some(i) = queue.pop_front() {
                let (x, y) = (i as i32 % map.width, i as i32 / map.width);
                for (x, y) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if x < 0 || y < 0 || x >= map.width || y >= map.height {
                        continue;
                    }
                    let n = index(&map, x, y);
                    let c = &map.cells[n];
                    if content.grounds[&c.ground] > 0.
                        && (c.ground != LAVA || c.object == Some(BRIDGE))
                        && c.object.is_none_or(|k| !content.objects[&k].blocks)
                        && seen.insert(n)
                    {
                        queue.push_back(n);
                    }
                }
            }
            let mut idols = 0;
            for (i, c) in map.cells.iter().enumerate() {
                if matches!(c.object, Some(MALPHAS | IDOL)) {
                    assert!(seen.contains(&i), "seed {seed}: encounter disconnected");
                }
                if c.object == Some(IDOL) {
                    idols += 1;
                    assert!(c.region >= 16);
                }
            }
            variants.insert(idols);
            assert!(
                map.cells
                    .iter()
                    .filter(|c| c.object.is_some_and(|k| (0x66d..=0x672).contains(&k)))
                    .count()
                    > 100
            );
            assert_eq!(
                map.cells
                    .iter()
                    .filter(|c| c.object == Some(MALPHAS))
                    .count(),
                1
            );
        }
        assert_eq!(variants, BTreeSet::from([0, 1, 2, 3]));
    }
}
