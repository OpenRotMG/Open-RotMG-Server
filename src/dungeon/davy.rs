use crate::{
    content::{Cell, Content, Map},
    encounter::{Motion, Profile, nr_speed},
};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::{collections::BTreeMap, sync::LazyLock};

pub const NAME: &str = "Davy Jones's Locker";
pub const TEMPLATE: &str = "davy-jones-locker";
pub const BOSS: u16 = 0xe32;
pub const UNLIT: u16 = 0xe35;
pub const LIT: u16 = 0xe36;
pub const KEYS: [u16; 4] = [0xe3e, 0xe3f, 0xe40, 0xe41];
pub const COLORS: [&str; 4] = ["purple", "green", "red", "yellow"];
pub const DOORS: [u16; 4] = [0x186a, 0x186c, 0x1868, 0x186e];
pub const ROSTER: [u16; 12] = [
    BOSS, UNLIT, LIT, 0xe50, 0xe48, 0xe31, 0xe3a, 0xe44, 0xe2f, 0xe45, 0xe42, 0xe43,
];

pub fn door_color(kind: u16) -> Option<usize> {
    DOORS
        .iter()
        .position(|door| kind == *door || kind == *door + 1)
}
pub fn profile(kind: u16) -> Option<Profile> {
    let follow = |rate, interval| {
        (
            Motion::Follow {
                sight: 8.,
                stop: 1.,
                idle: nr_speed(0.1),
                duration: 0,
                cooldown: 0,
            },
            rate,
            interval,
            10.,
        )
    };
    let (motion, rate, interval, range) = match kind {
        BOSS => (Motion::WanderHome { radius: 3. }, 0.2, 50, 12.),
        UNLIT | 0x182e => (Motion::Stationary, 0., 0, 0.),
        LIT => (Motion::Stationary, 0., 50, 10.),
        0xe44 => (
            Motion::Chase {
                sight: 8.,
                stop: 2.,
            },
            0.35,
            4000,
            10.,
        ),
        0xe48 => (
            Motion::Orbit {
                anchor: crate::encounter::Anchor::Parent,
                radius: 3.,
            },
            0.3,
            50,
            10.,
        ),
        0xe45 => (
            Motion::Chase {
                sight: 13.,
                stop: 6.,
            },
            0.2,
            50,
            13.,
        ),
        0xe31 | 0xe50 => follow(0.6, 2000),
        0xe3a => follow(0.25, 1750),
        0xe2f => follow(0.55, 1750),
        0xe42 => follow(0.4, 2500),
        0xe43 => follow(0.3, 2000),
        _ => return None,
    };
    Some(Profile {
        motion,
        speed: if rate == 0. { 0. } else { nr_speed(rate) },
        interval,
        range,
    })
}
pub fn initial_condition(kind: u16) -> i32 {
    if kind == 0xe48 {
        return crate::condition::bit(crate::condition::INVINCIBLE);
    }
    if matches!(kind, BOSS | LIT | 0xe50) || KEYS.contains(&kind) {
        crate::condition::bit(crate::condition::INVULNERABLE)
    } else {
        0
    }
}

/// Every layout tunable; sources are in data/README.md and the PR description.
pub mod tuning {
    use std::ops::RangeInclusive;
    /// Halls per run (captures: 10-12).
    pub const HALLS: RangeInclusive<usize> = 10..=12;
    /// Rooms besides start and boss (captures: 15-16).
    pub const ROOMS: RangeInclusive<usize> = 15..=16;
    /// Halls between start and boss (every capture: 3).
    pub const BOSS_HALLS: usize = 3;
    /// Template weights: instance counts in nilly and the 2013/2014/2018 minimaps.
    pub const HALL_WEIGHTS: [(&str, f64); 6] = [
        ("H24", 12.0),
        ("H36", 4.0),
        ("H48", 5.0),
        ("V24", 11.0),
        ("V36", 8.0),
        ("V48", 3.0),
    ];
    /// `start` hangs below a hall, `start_b` east of one.
    pub const START_WEIGHTS: [(&str, f64); 2] = [("start", 8.0), ("start_b", 2.0)];
    /// Chance that `start` hangs below a horizontal hall (about 13 of 15 sightings).
    pub const START_FLAT: f64 = 0.85;
    /// s*/e*/w* rooms are minimap shapes with approximate decor; w16x10 was seen once.
    pub const ROOM_WEIGHTS: [(&str, f64); 19] = [
        ("closet", 6.0),
        ("rug", 8.0),
        ("map", 4.0),
        ("chapel", 3.0),
        ("table_rug", 3.0),
        ("armoury", 2.0),
        ("puddle", 6.0),
        ("table", 6.0),
        ("brig", 2.0),
        ("s12x13", 4.0),
        ("e22x12", 3.0),
        ("s10x16", 3.0),
        ("e16x10", 3.0),
        ("e19x12", 3.0),
        ("e20x14", 1.0),
        ("s12x22", 2.0),
        ("s14x20", 1.0),
        ("s8x12", 1.0),
        ("w16x10", 0.25),
    ];
    /// Hall ports that take rooms; a room's fixed door picks the side (only w16x10 uses E).
    pub const ROOM_SIDES: [char; 3] = ['N', 'W', 'E'];
    /// End-to-end, 4-way crossing and room-beside-room chances, calibrated on footage run/crossing/comb rates.
    pub const END_TO_END: f64 = 0.2;
    pub const CROSSING: f64 = 0.1;
    pub const COMB: f64 = 0.6;
    /// Longest straight run in halls and tiles (footage: up to ~150 tiles).
    pub const RUN_HALLS: usize = 4;
    pub const RUN_TILES: i32 = 150;
    /// Most rooms in a row on one wall (footage: 6 plus the boss neck).
    pub const COMB_MAX: usize = 6;
    /// Whether same-orientation halls may join side by side (never seen).
    pub const PARALLEL_HALLS: bool = false;
    /// Least empty tiles beside unjoined halls, side-by-side halls and rooms (nilly and footage minima).
    pub const HALL_CLEARANCE: i32 = 1;
    pub const PARALLEL_CLEARANCE: i32 = 3;
    pub const ROOM_CLEARANCE: i32 = 0;
    /// nilly's coloured room doors besides start (purple) and boss (yellow); one video may show a second red.
    pub const GREEN: usize = 5;
    pub const RED: usize = 1;
    pub const YELLOW: usize = 3;
    /// Enemy packs per 12-tile hall module or room (a guess).
    pub const PACKS: i32 = 1;
    /// Square world the layout is centred in, as in nilly.
    pub const WORLD: i32 = 256;
}

#[derive(serde::Deserialize)]
struct Library {
    legend: BTreeMap<char, (String, String)>,
    templates: BTreeMap<String, Template>,
}
#[derive(serde::Deserialize)]
struct Template {
    ports: Vec<(char, i32)>,
    #[serde(default)]
    pack: Vec<String>,
    rows: Vec<String>,
}
impl Template {
    fn hall(&self) -> bool {
        self.ports.len() > 1
    }
    fn size(&self) -> (i32, i32) {
        let w = self
            .rows
            .iter()
            .map(|r| r.chars().count())
            .max()
            .unwrap_or(0);
        (w as i32, self.rows.len() as i32)
    }
}
static LIBRARY: LazyLock<Library> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/davy-templates.json"))
        .expect("data/davy-templates.json is a valid template library")
});

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
impl Rect {
    /// The cell `along` this rect's `side` edge, `out` cells outside it.
    fn at(self, side: char, along: i32, out: i32) -> (i32, i32) {
        match side {
            'N' => (self.x + along, self.y - out),
            'S' => (self.x + along, self.y + self.h - 1 + out),
            'W' => (self.x - out, self.y + along),
            _ => (self.x + self.w - 1 + out, self.y + along),
        }
    }
    fn grow(self, g: i32) -> Rect {
        Rect {
            x: self.x - g,
            y: self.y - g,
            w: self.w + 2 * g,
            h: self.h + 2 * g,
        }
    }
    fn overlaps(self, o: Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}
fn opposite(side: char) -> char {
    match side {
        'N' => 'S',
        'S' => 'N',
        'W' => 'E',
        _ => 'W',
    }
}

struct Node {
    name: &'static str,
    t: &'static Template,
    rect: Rect,
    /// Parent node, the parent's port and this template's port.
    link: Option<(usize, usize, usize)>,
    color: Option<usize>,
}

/// The node on the far side of port `i` of node `p` (its parent for the port it hangs from), if any.
fn joined(nodes: &[Node], p: usize, i: usize) -> Option<usize> {
    match nodes[p].link {
        Some((parent, _, own)) if own == i => Some(parent),
        _ => nodes
            .iter()
            .position(|n| n.link.is_some_and(|l| (l.0, l.1) == (p, i))),
    }
}
/// Unused ports of `parents` that face one of `sides`.
fn free(nodes: &[Node], parents: &[usize], sides: &[char]) -> Vec<(usize, usize)> {
    parents
        .iter()
        .flat_map(|&p| (0..nodes[p].t.ports.len()).map(move |i| (p, i)))
        .filter(|&(p, i)| sides.contains(&nodes[p].t.ports[i].0) && joined(nodes, p, i).is_none())
        .collect()
}
fn flat(t: &Template) -> bool {
    let (w, h) = t.size();
    w > h
}
/// Halls and tiles in the straight run through hall `p`: same-orientation halls joined end to end (+1 tile) or
/// across one lattice point of a perpendicular hall (+12: its 10-tile width and two gaps).
fn run(nodes: &[Node], p: usize) -> (usize, i32) {
    let next = |i: usize| {
        let ends = (0..nodes[i].t.ports.len())
            .filter(move |&e| flat(nodes[i].t) == "WE".contains(nodes[i].t.ports[e].0));
        let halls = ends
            .filter_map(move |e| joined(nodes, i, e))
            .filter(move |&j| nodes[j].t.hall());
        halls.filter_map(move |j| {
            if flat(nodes[j].t) == flat(nodes[i].t) {
                return Some((j, 1));
            }
            let k = (0..nodes[j].t.ports.len()).find(|&k| joined(nodes, j, k) == Some(i))?;
            let (s, a) = nodes[j].t.ports[k];
            let twin = nodes[j]
                .t
                .ports
                .iter()
                .position(|&q| q == (opposite(s), a))?;
            let m = joined(nodes, j, twin)?;
            (nodes[m].t.hall() && flat(nodes[m].t) == flat(nodes[i].t)).then_some((m, 12))
        })
    };
    let (mut members, mut tiles, mut k) = (vec![p], 0, 0);
    while let Some(&i) = members.get(k) {
        k += 1;
        tiles += nodes[i].rect.w.max(nodes[i].rect.h);
        for (j, gap) in next(i).collect::<Vec<_>>() {
            if !members.contains(&j) {
                members.push(j);
                tiles += gap;
            }
        }
    }
    (members.len(), tiles)
}

/// Least empty tiles between a new structure and an unjoined one.
fn clearance(t: &Template, r: Rect, o: &Node) -> i32 {
    if !t.hall() || !o.t.hall() {
        return tuning::ROOM_CLEARANCE;
    }
    let (a, b) = (o.rect, flat(t));
    let beside = flat(o.t) == b
        && if b {
            r.x < a.x + a.w && a.x < r.x + r.w
        } else {
            r.y < a.y + a.h && a.y < r.y + r.h
        };
    if beside {
        tuning::PARALLEL_CLEARANCE
    } else {
        tuning::HALL_CLEARANCE
    }
}
/// Attach a template from `pool` at one of `ports` (node, port), never overlapping earlier rects.
fn attach(
    nodes: &mut Vec<Node>,
    gaps: &mut Vec<Rect>,
    ports: &[(usize, usize)],
    pool: &[(&str, f64)],
    aligned: bool,
    rng: &mut StdRng,
) -> Option<()> {
    use tuning::*;
    for _ in 0..64 {
        // Template first, so weights hold; it can only take ports facing its own doors.
        let (name, t) = LIBRARY
            .templates
            .get_key_value(pool.choose_weighted(rng, |e| e.1).ok()?.0)?;
        let fits = |&&(p, i): &&(usize, usize)| {
            t.ports
                .iter()
                .any(|q| q.0 == opposite(nodes[p].t.ports[i].0))
        };
        let Some(&(p, i)) = ports
            .iter()
            .filter(fits)
            .collect::<Vec<_>>()
            .choose(rng)
            .copied()
        else {
            continue;
        };
        let parent = nodes[p].t;
        let (side, along) = parent.ports[i];
        let mine: Vec<_> = (0..t.ports.len())
            .filter(|&j| t.ports[j].0 == opposite(side))
            .collect();
        let Some(&j) = mine.choose(rng) else { continue };
        // Halls meet through a one-tile gap; rooms carry their own two-tile neck.
        let gap = i32::from(t.hall() && parent.hall());
        let collinear = gap == 1 && flat(t) == flat(parent);
        if collinear && flat(t) == "NS".contains(side) && !PARALLEL_HALLS || aligned && !collinear {
            continue;
        }
        let (w, h) = t.size();
        let ((tx, ty), (ax, ay)) = (
            nodes[p].rect.at(side, along, 1 + gap),
            Rect { x: 0, y: 0, w, h }.at(opposite(side), t.ports[j].1, 0),
        );
        let rect = Rect {
            x: tx - ax,
            y: ty - ay,
            w,
            h,
        };
        let (a, b) = (
            nodes[p].rect.at(side, along - 1, 1),
            nodes[p].rect.at(side, along + 4, 1),
        );
        let joint = Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs() + 1,
            h: (a.1 - b.1).abs() + 1,
        };
        let crowded = (0..nodes.len()).any(|k| {
            let need = if k == p {
                0
            } else {
                clearance(t, rect, &nodes[k])
            };
            nodes[k].rect.grow(need).overlaps(rect)
        });
        let mut taken = nodes.iter().map(|n| n.rect).chain(gaps.iter().copied());
        if crowded || taken.any(|r| r.overlaps(rect) || (gap == 1 && r.overlaps(joint))) {
            continue;
        }
        nodes.push(Node {
            name,
            t,
            rect,
            link: Some((p, i, j)),
            color: None,
        });
        let (halls, tiles) = run(nodes, nodes.len() - 1);
        if t.hall() && (halls > RUN_HALLS || tiles > RUN_TILES) {
            nodes.pop();
            continue;
        }
        if gap == 1 {
            gaps.push(joint);
        }
        return Some(());
    }
    None
}

/// Grow the template tree: start, three halls to the boss, more halls, then leaf rooms on N/W ports.
fn plan(rng: &mut StdRng) -> Option<(Vec<Node>, Vec<Rect>)> {
    use tuning::*;
    const ANY: [char; 4] = ['N', 'E', 'S', 'W'];
    let (name, t) = LIBRARY
        .templates
        .get_key_value(START_WEIGHTS.choose_weighted(rng, |e| e.1).ok()?.0)?;
    let (w, h) = t.size();
    let rect = Rect { x: 0, y: 0, w, h };
    let mut nodes = vec![Node {
        name,
        t,
        rect,
        link: None,
        color: Some(0),
    }];
    let mut gaps = vec![];
    let horizontal = rng.gen_bool(START_FLAT);
    let first: Vec<_> = HALL_WEIGHTS
        .into_iter()
        .filter(|(n, _)| nodes[0].name != "start" || flat(&LIBRARY.templates[*n]) == horizontal)
        .collect();
    for k in 0..=BOSS_HALLS {
        let (pool, sides): (&[_], &[_]) = if k == 0 {
            (&first, &ANY)
        } else if k < BOSS_HALLS {
            (&HALL_WEIGHTS, &ANY)
        } else {
            (&[("boss", 1.0)], &ROOM_SIDES)
        };
        let ports = free(&nodes, &[k], sides);
        attach(&mut nodes, &mut gaps, &ports, pool, false, rng)?;
    }
    nodes.last_mut()?.color = Some(3);
    for _ in BOSS_HALLS..rng.gen_range(HALLS) {
        let halls: Vec<_> = (0..nodes.len()).filter(|&i| nodes[i].t.hall()).collect();
        let any = free(&nodes, &halls, &ANY);
        let port = |&(p, i): &(usize, usize)| (nodes[p].t, nodes[p].t.ports[i]);
        // End-to-end runs extend a hall from an end; crossings answer a vertical hall across a horizontal one.
        let ends: Vec<_> = any
            .iter()
            .copied()
            .filter(|q| matches!(port(q), (t, (s, _)) if flat(t) == "WE".contains(s)))
            .collect();
        let crossings: Vec<_> = any
            .iter()
            .copied()
            .filter(|&(p, i)| {
                let (t, (s, a)) = port(&(p, i));
                let twin = t.ports.iter().position(|&q| q == (opposite(s), a));
                flat(t)
                    && "NS".contains(s)
                    && twin
                        .and_then(|j| joined(&nodes, p, j))
                        .is_some_and(|c| nodes[c].t.hall())
            })
            .collect();
        let roll: f64 = rng.gen_range(0.0..1.0);
        let special = match () {
            _ if roll < END_TO_END => Some((ends, true)),
            _ if roll < END_TO_END + CROSSING => Some((crossings, false)),
            _ => None,
        };
        let placed = special.and_then(|(ports, aligned)| {
            attach(&mut nodes, &mut gaps, &ports, &HALL_WEIGHTS, aligned, rng)
        });
        placed.or_else(|| attach(&mut nodes, &mut gaps, &any, &HALL_WEIGHTS, false, rng))?;
    }
    for _ in 0..rng.gen_range(ROOMS) {
        let halls: Vec<_> = (0..nodes.len()).filter(|&i| nodes[i].t.hall()).collect();
        // Rooms on one wall line 12 tiles apart (13 across an end-to-end join) form a comb; the boss bridges one.
        let door = |&(p, i): &(usize, usize)| {
            let (side, along) = nodes[p].t.ports[i];
            (side, nodes[p].rect.at(side, along, 0))
        };
        let doors: Vec<_> = nodes
            .iter()
            .filter(|n| !n.t.hall())
            .filter_map(|n| n.link.map(|(p, i, _)| (door(&(p, i)), n.name != "boss")))
            .collect();
        let row = |q: &(usize, usize)| {
            let (s, (x, y)) = door(q);
            let along = |&((t, (u, v)), counts): &((char, (i32, i32)), bool)| match "NS".contains(s)
            {
                true => (t == s && v == y).then_some((u, counts)),
                false => (t == s && u == x).then_some((v, counts)),
            };
            let mut at: Vec<_> = doors.iter().filter_map(along).collect();
            let me = if "NS".contains(s) { x } else { y };
            at.push((me, true));
            at.sort();
            let k = at.iter().position(|a| a.0 == me).unwrap_or(0);
            let lo = (0..k)
                .rev()
                .take_while(|&i| at[i + 1].0 - at[i].0 <= 13)
                .last()
                .unwrap_or(k);
            let hi = (k..at.len() - 1)
                .take_while(|&i| at[i + 1].0 - at[i].0 <= 13)
                .last()
                .map_or(k, |i| i + 1);
            at[lo..=hi].iter().filter(|a| a.1).count()
        };
        let any: Vec<_> = free(&nodes, &halls, &ROOM_SIDES)
            .into_iter()
            .filter(|q| row(q) <= COMB_MAX)
            .collect();
        let comb: Vec<_> = any.iter().copied().filter(|q| row(q) > 1).collect();
        let placed = rng
            .gen_bool(COMB)
            .then(|| attach(&mut nodes, &mut gaps, &comb, &ROOM_WEIGHTS, false, rng))
            .flatten();
        placed.or_else(|| attach(&mut nodes, &mut gaps, &any, &ROOM_WEIGHTS, false, rng))?;
    }
    let (x0, y0, x1, y1) = bounds(nodes.iter().map(|n| n.rect).chain(gaps.iter().copied()));
    (x1 - x0 < WORLD - 2 && y1 - y0 < WORLD - 2).then_some((nodes, gaps))
}
fn bounds(rects: impl Iterator<Item = Rect> + Clone) -> (i32, i32, i32, i32) {
    let min = |f: fn(&Rect) -> i32| rects.clone().map(|r| f(&r)).min().unwrap_or(0);
    let max = |f: fn(&Rect) -> i32| rects.clone().map(|r| f(&r)).max().unwrap_or(0);
    (
        min(|r| r.x),
        min(|r| r.y),
        max(|r| r.x + r.w),
        max(|r| r.y + r.h),
    )
}

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    Ok(build(content, seed)?.0)
}

fn build(content: &Content, seed: u64) -> Result<(Map, Vec<Node>)> {
    use tuning::*;
    const DARK: u8 = 0x7e;
    const SPACE: u8 = 0xfe;
    const WALL: u16 = 0x1826;
    let mut rng = StdRng::seed_from_u64(seed);
    let (attempts, (mut nodes, gaps)) = (1..=50)
        .find_map(|n| plan(&mut rng).map(|p| (n, p)))
        .context("Davy template tree did not fit in 50 attempts")?;
    let mut rooms: Vec<_> = (1..nodes.len())
        .filter(|&i| !nodes[i].t.hall() && nodes[i].color.is_none())
        .collect();
    rooms.shuffle(&mut rng);
    let colors = [vec![2; RED], vec![1; GREEN], vec![3; YELLOW]].concat();
    ensure!(
        rooms.len() > colors.len(),
        "Davy layout lacks a doorless room for the green key"
    );
    for (&i, c) in rooms.iter().zip(colors) {
        nodes[i].color = Some(c);
    }
    // Purple in the start, green in a doorless room, red behind green, yellow in the red room.
    let holders = [0, rooms[RED + GREEN + YELLOW], rooms[RED], rooms[0]];

    let (x0, y0, x1, y1) = bounds(nodes.iter().map(|n| n.rect).chain(gaps.iter().copied()));
    let (dx, dy) = (
        (WORLD - (x1 - x0) + 1) / 2 - x0,
        (WORLD - (y1 - y0) + 1) / 2 - y0,
    );
    for n in &mut nodes {
        (n.rect.x, n.rect.y) = (n.rect.x + dx, n.rect.y + dy);
    }
    let ground = |name: &str| {
        content
            .ground_names
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(k, _)| *k)
            .with_context(|| format!("unknown Davy ground {name}"))
    };
    let object = |name: &str| {
        content
            .objects
            .values()
            .find(|o| o.name == name)
            .map(|o| o.kind)
            .with_context(|| format!("unknown Davy object {name}"))
    };
    let marker = |o: &str| matches!(o, "" | "door slot" | "key slot" | "spawn");
    let tiles = LIBRARY
        .legend
        .iter()
        .map(|(&c, (g, o))| {
            let ground = if g.is_empty() { SPACE } else { ground(g)? };
            Ok((
                c,
                (
                    ground,
                    o.as_str(),
                    if marker(o) { None } else { Some(object(o)?) },
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let blank = Cell {
        ground: SPACE,
        object: None,
        config: String::new(),
        terrain: 0,
        region: 0,
    };
    let mut map = Map {
        width: WORLD,
        height: WORLD,
        cells: vec![blank.clone(); (WORLD * WORLD) as usize],
        spawns: vec![],
    };
    let at = |(x, y): (i32, i32)| (y * WORLD + x) as usize;
    let (mut doors, mut keys, mut floors) = (
        vec![vec![]; nodes.len()],
        vec![None; nodes.len()],
        vec![vec![]; nodes.len()],
    );
    for (i, n) in nodes.iter().enumerate() {
        for (y, row) in n.t.rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate().filter(|(_, c)| *c != ' ') {
                let p = (n.rect.x + x as i32, n.rect.y + y as i32);
                let (ground, name, kind) = *tiles
                    .get(&c)
                    .context("Davy template uses an unknown legend character")?;
                map.cells[at(p)] = Cell {
                    ground,
                    ..blank.clone()
                };
                match name {
                    "" => floors[i].push(p),
                    "door slot" => doors[i].push(p),
                    "key slot" => keys[i] = Some(p),
                    "spawn" => map.spawns.push((p.0 as f32 + 0.5, p.1 as f32 + 0.5)),
                    _ => map.cells[at(p)].object = kind,
                }
            }
        }
    }
    for n in &nodes {
        let Some((p, i, j)) = n.link else { continue };
        let (parent, (side, along)) = (&nodes[p], nodes[p].t.ports[i]);
        let mut open = |p: (i32, i32), object: Option<u16>| {
            map.cells[at(p)] = Cell {
                ground: if object.is_some() { SPACE } else { DARK },
                object,
                ..blank.clone()
            };
        };
        for k in 0..4 {
            if parent.t.hall() {
                open(parent.rect.at(side, along + k, 0), None);
            }
            if n.t.hall() {
                open(n.rect.at(opposite(side), n.t.ports[j].1 + k, 0), None);
            }
        }
        if parent.t.hall() && n.t.hall() {
            for k in -1..=4 {
                open(
                    parent.rect.at(side, along + k, 1),
                    (k == -1 || k == 4).then_some(WALL),
                );
            }
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        if let Some(c) = n.color {
            // Lf/Rt order varies on nilly's side-by-side pairs; its three stacked pairs all put Lf on top.
            let lf_first = doors[i][0].0 == doors[i][1].0 || rng.gen_bool(0.5);
            for (k, &p) in doors[i].iter().enumerate() {
                map.cells[at(p)].object = Some(DOORS[c] + u16::from((k == 0) == lf_first));
            }
        }
        let modules = if n.t.hall() {
            n.rect.w.max(n.rect.h) / 12
        } else {
            1
        };
        for _ in 0..PACKS * modules {
            for kind in &n.t.pack {
                if !floors[i].is_empty() {
                    let k = rng.gen_range(0..floors[i].len());
                    let p = floors[i].swap_remove(k);
                    map.cells[at(p)].object = Some(object(kind)?);
                }
            }
        }
    }
    for (color, &i) in holders.iter().enumerate() {
        map.cells[at(keys[i].context("Davy key room lacks a key slot")?)].object =
            Some(KEYS[color]);
    }
    let &(sx, sy) = map
        .spawns
        .first()
        .context("Davy start template lacks a spawn")?;
    map.cells[at((sx as i32, sy as i32))].region = 1;
    tracing::info!(
        seed,
        attempts,
        start = nodes[0].name,
        halls = nodes.iter().filter(|n| n.t.hall()).count(),
        rooms = nodes.iter().filter(|n| !n.t.hall()).count(),
        width = x1 - x0,
        height = y1 - y0,
        "generated Davy template tree"
    );
    Ok((map, nodes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, VecDeque};

    /// Cells reachable from spawn with the doors in `open` passable; destructible barrels count as passable.
    fn reach(content: &Content, map: &Map, open: u8) -> Vec<bool> {
        let walk = |c: &Cell| {
            content.grounds[&c.ground] > 0.
                && c.object.is_none_or(|k| match door_color(k) {
                    Some(d) => open & 1 << d != 0,
                    None => !content.objects[&k].blocks || content.objects[&k].enemy,
                })
        };
        let (sx, sy) = map.spawns[0];
        let mut seen = vec![false; map.cells.len()];
        let mut queue = VecDeque::from([(sy as i32 * map.width + sx as i32) as usize]);
        while let Some(i) = queue.pop_front() {
            if !seen[i] && walk(&map.cells[i]) {
                seen[i] = true;
                let w = map.width as usize;
                queue.extend([i - 1, i + 1, i - w, i + w]);
            }
        }
        seen
    }

    #[test]
    fn davy_tree_follows_2012_rules() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..48 {
            let (map, nodes) = build(&content, seed).unwrap();
            let halls = nodes.iter().filter(|n| n.t.hall()).count();
            assert!(
                tuning::HALLS.contains(&halls)
                    && tuning::ROOMS.contains(&(nodes.len() - halls - 2))
            );
            assert!(nodes[0].name.starts_with("start") && nodes[0].link.is_none());
            let mut owner = vec![None; map.cells.len()];
            for (i, n) in nodes.iter().enumerate() {
                for (y, row) in n.t.rows.iter().enumerate() {
                    for (x, _) in row.chars().enumerate().filter(|(_, c)| *c != ' ') {
                        owner[((n.rect.y + y as i32) * map.width + n.rect.x + x as i32) as usize] =
                            Some(i);
                    }
                }
                let Some((p, port, own)) = n.link else {
                    continue;
                };
                let (side, along) = nodes[p].t.ports[port];
                assert!(p < i && n.t.ports[own].0 == opposite(side));
                if !n.t.hall() {
                    assert!(
                        nodes[p].t.hall() && tuning::ROOM_SIDES.contains(&side),
                        "seed {seed}: room on {side}"
                    );
                }
                if p == 0 {
                    assert!(
                        i == 1 && n.t.hall() && matches!(n.t.ports[own].0, 'S' | 'E'),
                        "seed {seed}: start side"
                    );
                } else if n.t.hall() && nodes[p].t.hall() {
                    for k in 0..4 {
                        let (x, y) = nodes[p].rect.at(side, along + k, 1);
                        owner[(y * map.width + x) as usize] = Some(i);
                    }
                }
            }
            let boss = nodes.iter().position(|n| n.name == "boss").unwrap();
            let route: Vec<_> =
                std::iter::successors(Some(boss), |&i| nodes[i].link.map(|l| l.0)).collect();
            assert_eq!(
                route.len(),
                5,
                "seed {seed}: boss is four structures from the start"
            );
            assert!(
                route[1..4].iter().all(|&i| nodes[i].t.hall()) && nodes[boss].t.ports[0].0 == 'S'
            );
            // Openings between structures are exactly the tree's links, and every floor cell is reachable.
            let all = reach(&content, &map, 0b1111);
            let mut joins = BTreeSet::new();
            for i in (0..map.cells.len()).filter(|&i| all[i]) {
                for j in [i + 1, i + map.width as usize]
                    .into_iter()
                    .filter(|&j| all[j])
                {
                    if let (Some(a), Some(b)) = (owner[i], owner[j]) {
                        joins.extend((a != b).then(|| (a.min(b), a.max(b))));
                    }
                }
            }
            let links: BTreeSet<_> = (1..nodes.len())
                .map(|i| (nodes[i].link.unwrap().0, i))
                .collect();
            assert_eq!(joins, links, "seed {seed}");
            let walkable = |c: &Cell| {
                content.grounds[&c.ground] > 0.
                    && c.object.is_none_or(|k| !content.objects[&k].blocks)
            };
            assert!(
                map.cells.iter().zip(&all).all(|(c, r)| *r || !walkable(c)),
                "seed {seed}: unreachable floor"
            );
            // Key chain: each key needs exactly the colours before it; Davy needs all four.
            let find = |kind: u16| {
                map.cells
                    .iter()
                    .position(|c| c.object == Some(kind))
                    .unwrap()
            };
            for (c, target) in KEYS.iter().chain([&BOSS]).map(|&k| find(k)).enumerate() {
                assert!(
                    reach(&content, &map, (1u8 << c) - 1)[target],
                    "seed {seed}: key {c} unreachable"
                );
                assert!(
                    c == 0 || !reach(&content, &map, (1u8 << (c - 1)) - 1)[target],
                    "seed {seed}: key {c} early"
                );
            }
            let purple: Vec<_> = (0..map.cells.len())
                .filter(|&i| map.cells[i].object.and_then(door_color) == Some(0))
                .collect();
            assert!(
                purple.len() == 2 && purple.iter().all(|&i| owner[i] == Some(0)),
                "seed {seed}: purple door"
            );
            let (davy, w) = (find(BOSS) as i32, map.width);
            let lamps: Vec<_> = (0..map.cells.len() as i32)
                .filter(|&i| map.cells[i as usize].object == Some(UNLIT))
                .collect();
            let near = |&i: &i32| (i % w - davy % w).pow(2) + (i / w - davy / w).pow(2) < 121;
            assert!(
                lamps.len() == 8 && lamps.iter().all(near),
                "seed {seed}: lanterns"
            );
        }
    }

    #[test]
    fn davy_templates_sit_on_the_hall_lattice() {
        for (name, t) in &LIBRARY.templates {
            let (w, h) = t.size();
            let edge = |side, k| {
                let (x, y) = Rect { x: 0, y: 0, w, h }.at(side, k, 0);
                LIBRARY
                    .legend
                    .get(&t.rows[y as usize].chars().nth(x as usize).unwrap_or(' '))
                    .map(|l| l.1.as_str())
            };
            for &(side, along) in &t.ports {
                if t.hall() {
                    let lengthwise = (w > h) == matches!(side, 'N' | 'S');
                    assert!(
                        w.min(h) == 10 && w.max(h) % 12 == 0 && w.max(h) <= 48,
                        "{name}"
                    );
                    assert!(
                        if lengthwise {
                            along % 12 == 4
                        } else {
                            along == 3
                        },
                        "{name} port off the lattice"
                    );
                } else {
                    let slot = [1, 2]
                        .iter()
                        .all(|&k| edge(side, along + k) == Some("door slot"));
                    assert!(t.ports.len() == 1 && slot, "{name} door slot");
                }
            }
        }
    }
}
