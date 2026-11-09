use crate::{
    content::{Cell, Content, Map},
    encounter::{Motion, Profile, nr_speed},
};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::{collections::BTreeMap, sync::LazyLock};

pub const NAME: &str = "Mad Lab";
pub const BOSS: u16 = 0x976;
pub const HEXXER: u16 = 0x1e15;
pub const DEHEXXER: u16 = 0x1e16;
/// Lab Bad Vat Rug (Hexxer vats) and Lab Good Vat Rug (DeHexxer vats).
pub const BAD_VAT: u8 = 0xa9;
pub const GOOD_VAT: u8 = 0xa7;
pub const CAGE: u16 = 0x892;
pub const BUBBLE: u16 = 0x1e22;
pub const TESLA: u16 = 0x188a;
pub const EMERGENCY: u16 = 0x18c2;
pub const CREATION: u16 = 0x5e1c;
/// Lab Table: the destructible chemistry table (a flask on a bench, 100 HP, no XP). Players broke "desk objects" in
/// the Lab: https://web.archive.org/web/20261025211742/https://wikiwiki.jp/rmd/Mad%20Lab (comment 2013-04-22).
pub const TABLE: u16 = 0x188b;
pub const ROSTER: &[u16] = &[
    BOSS, 0x978, 0x979, 0x980, 0x981, 0x982, 0x983, 0x990, 0x991, 0x993, 0x994, 0x995, 0x996,
    0x12aa, 0x1e23, 0x1e24, 0x1e25, HEXXER, DEHEXXER, BUBBLE, TESLA, CREATION, TABLE,
];

pub fn initial_condition(kind: u16) -> i32 {
    if matches!(
        kind,
        HEXXER | DEHEXXER | BUBBLE | CREATION | 0x990 | 0x991 | 0x993..=0x996
    ) {
        crate::condition::bit(crate::condition::INVINCIBLE)
    } else {
        0
    }
}

pub fn profile(kind: u16) -> Profile {
    let (motion, speed, interval, range) = match kind {
        HEXXER | DEHEXXER | BUBBLE | TESLA | TABLE => (Motion::Stationary, 0., 0, 0.),
        // H Creation Wave range 11.25: https://web.archive.org/web/20261025211809/https://wikiwiki.jp/rmd/Horrific%20Creation; wander, speed and cadence are reconstruction.
        CREATION => (
            Motion::WanderHome { radius: 3. },
            nr_speed(0.5),
            1000,
            11.25,
        ),
        BOSS => (Motion::WanderHome { radius: 5. }, nr_speed(0.5), 0, 12.),
        0x978 => (Motion::Stationary, 0., 50, 30.),
        // Turrets fire "constantly" (常時): https://web.archive.org/web/20261025211742/https://wikiwiki.jp/rmd/Mad%20Lab. The
        // acid-room screenshot on the wiki page last updated for 123.5.1 (image 9ol81, `tuning` link) holds 20 Green Stars
        // in 12 turret rows; a 4 tiles/s star crosses its 10-tile view in 2.5 s from either wall, so each turret fires
        // about every 3 s (estimate from one frame). Each keeps its own beat: `turret_beat`.
        0x990 | 0x991 => (Motion::Stationary, 0., 3000, 32.),
        0x993..=0x996 => (Motion::Stationary, 0., 1000, 30.),
        0x12aa => (Motion::Stationary, 0., 1000, 10.),
        0x980 | 0x1e24 => (
            Motion::Chase {
                sight: 10.,
                stop: 1.,
            },
            nr_speed(0.6),
            800,
            10.,
        ),
        0x981 | 0x1e23 => (Motion::Wander, nr_speed(0.5), 800, 10.),
        0x979 | 0x1e25 | 0x982 | 0x983 => (Motion::Wander, nr_speed(0.5), 1000, 10.),
        _ => panic!("unregistered Mad Lab actor {kind:#x}"),
    };
    Profile {
        motion,
        speed,
        interval,
        range,
    }
}

/// When in its `interval` a Defense Turret at row `y` fires. On the 123.5.1 wiki screenshot (`profile`) neighbouring
/// turrets fire at different times and only a few at once, so the room shows a stream rather than walls of stars.
/// Rows step 11 of 27 slots (any 27 rows of a wall take distinct slots), the east wall 13 slots later (estimate).
pub fn turret_beat(kind: u16, y: i32, interval: u64) -> u64 {
    let slot = (i64::from(y) * 11 + if kind == 0x991 { 13 } else { 0 }).rem_euclid(27);
    slot as u64 * interval / 27
}

pub fn validate(content: &Content, kind: u16) -> Result<()> {
    ensure!(
        ROSTER.contains(&kind),
        "unregistered Mad Lab actor {kind:#x}"
    );
    let indices: &[u8] = match kind {
        HEXXER | DEHEXXER | BUBBLE | TESLA | TABLE => &[],
        CREATION => &[1, 4, 5, 6],
        0x980 | 0x1e24 => &[0, 1, 2],
        0x982 => &[0, 1],
        0x983 => &[0, 1, 2, 3],
        _ => &[0],
    };
    for index in indices {
        ensure!(
            content.get(kind)?.projectiles.contains_key(index),
            "Mad Lab actor {kind:#x} lacks selected projectile {index}"
        );
    }
    Ok(())
}

/// Every layout tunable. "Wiki map" = the example layout (4 px per tile) on the Wild Shadow wiki page last updated
/// for 123.5.1: https://web.archive.org/web/20130508122310/http://forums.wildshadow.com/wiki/realm-mad-god/mad-lab;
/// "2012-10-09" = the zoomed-out minimap of a whole run: https://www.youtube.com/watch?v=s9XOYh8DJQ8&t=300.
pub mod tuning {
    use std::ops::RangeInclusive;
    /// Rooms strictly between start and boss: 10 on both maps.
    pub const PATH_ROOMS: usize = 10;
    /// Path slot of the Hexxer L room: first on the wiki map, second on 2012-10-09.
    pub const L_ROOM: RangeInclusive<usize> = 0..=1;
    /// Side branches (2 on both maps) sharing 3-5 rooms (4 on both), at most 3 in one branch.
    pub const BRANCHES: RangeInclusive<usize> = 2..=3;
    pub const BRANCH_ROOMS: RangeInclusive<usize> = 3..=5;
    pub const BRANCH_MAX: usize = 3;
    /// Rooms per map, cluster cells and the Horrific Creation's room included: wiki map 21, 2012-10-09 about 20.
    pub const ROOMS: RangeInclusive<usize> = 19..=22;
    /// Corridor tiles between joined doors (wiki map: 5-9). Corridors are 4 tiles wide inside (16 px).
    pub const CORRIDOR: RangeInclusive<i32> = 5..=9;
    /// Least empty tiles between rooms and corridors that are not joined (estimate).
    pub const CLEARANCE: i32 = 2;
    /// Template (weight, most copies per map): instances on the two maps; 2012-10-09 identifications are approximate.
    /// The L room, the acid room and the cluster stand on the path exactly once (both maps).
    pub const ROOM_WEIGHTS: [(&str, f64, usize); 7] = [
        ("vat_hall", 5.0, 3),
        ("walkway_cross", 4.0, 3),
        ("table_room", 4.0, 3),
        ("cross_hollow", 3.0, 2),
        ("big_room", 3.0, 2),
        ("pair", 1.0, 1),
        ("maze", 1.0, 1),
    ];
    /// Lab Tables per table room, drawn from its table slots: tables were rare, as 2012 players remember it (estimate;
    /// no 2012 frame examined shows one).
    pub const TABLES: RangeInclusive<usize> = 0..=2;
    /// Enemies per room, as in the previous layout.
    pub const PACK: usize = 4;
    /// Largest layout: the wiki map spans 218x189 tiles, 2012-10-09 about 270x255; larger plans are redrawn.
    pub const MAX_EXTENT: i32 = 280;
    /// The layout's bounding box is centred in this square world.
    pub const WORLD: i32 = 288;
}

#[derive(serde::Deserialize)]
struct Library {
    legend: BTreeMap<char, (String, String)>,
    templates: BTreeMap<String, Template>,
}
#[derive(serde::Deserialize)]
struct Template {
    ports: Vec<(char, i32)>,
    rows: Vec<String>,
    /// Start, boss and acid room keep the wiki map's orientation; other rooms turn in quarter steps.
    #[serde(default)]
    fixed: bool,
    /// Rooms in the template (the cluster's and the pair's cells).
    #[serde(default = "one")]
    rooms: usize,
}
fn one() -> usize {
    1
}
static LIBRARY: LazyLock<Library> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/lab-templates.json"))
        .expect("data/lab-templates.json is a valid template library")
});

/// A template turned clockwise by quarter steps: rooms recur turned on the two maps (the wiki's north-south walkway
/// cross runs west-east on 2012-10-09).
#[derive(Clone)]
struct Shape {
    rows: Vec<Vec<char>>,
    ports: Vec<(char, i32)>,
    /// Quarter turns: catwalk slats turn with the room (they run along a 2012 walkway).
    turns: usize,
}
impl Shape {
    fn new(t: &Template, turns: usize) -> Shape {
        let mut rows: Vec<Vec<char>> = t.rows.iter().map(|r| r.chars().collect()).collect();
        let mut ports = t.ports.clone();
        for _ in 0..turns {
            let (w, h) = (rows[0].len(), rows.len() as i32);
            rows = (0..w)
                .map(|x| rows.iter().rev().map(|row| row[x]).collect())
                .collect();
            ports = ports
                .into_iter()
                .map(|(side, along)| match side {
                    'N' => ('E', along),
                    'E' => ('S', h - 4 - along),
                    'S' => ('W', along),
                    _ => ('N', h - 4 - along),
                })
                .collect();
        }
        Shape { rows, ports, turns }
    }
    fn size(&self) -> (i32, i32) {
        (self.rows[0].len() as i32, self.rows.len() as i32)
    }
}

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
    fn span(a: (i32, i32), b: (i32, i32)) -> Rect {
        Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs() + 1,
            h: (a.1 - b.1).abs() + 1,
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

struct Link {
    parent: usize,
    port: usize,
    own: usize,
    len: i32,
}
struct Node {
    name: &'static str,
    rooms: usize,
    shape: Shape,
    rect: Rect,
    link: Option<Link>,
}
impl Node {
    /// The corridor between this node and its parent, walls included.
    fn corridor(&self, nodes: &[Node]) -> Option<Rect> {
        let l = self.link.as_ref()?;
        let p = &nodes[l.parent];
        let (side, along) = p.shape.ports[l.port];
        Some(Rect::span(
            p.rect.at(side, along - 1, 1),
            p.rect.at(side, along + 4, l.len),
        ))
    }
}
/// Ports of node `p` that no link uses.
fn free(nodes: &[Node], p: usize) -> Vec<usize> {
    let used = |i: usize| {
        nodes[p].link.as_ref().is_some_and(|l| l.own == i)
            || nodes.iter().any(|n| {
                n.link
                    .as_ref()
                    .is_some_and(|l| (l.parent, l.port) == (p, i))
            })
    };
    (0..nodes[p].shape.ports.len())
        .filter(|&i| !used(i))
        .collect()
}

/// Hang a template from `pool` off a free port of `p` facing one of `sides`, through a straight 4-wide corridor,
/// keeping `CLEARANCE` from every other room and corridor. With `exit`, the new room keeps a port facing that side.
fn attach(
    nodes: &mut Vec<Node>,
    p: usize,
    pool: &[(&'static str, f64, usize)],
    sides: &str,
    exit: Option<char>,
    rng: &mut StdRng,
) -> Option<usize> {
    use tuning::*;
    let ports: Vec<_> = free(nodes, p)
        .into_iter()
        .map(|i| (i, nodes[p].shape.ports[i]))
        .filter(|(_, (side, _))| sides.contains(*side))
        .collect();
    // Template first, so weights hold; then a few turns and placements of it.
    for _ in 0..16 {
        let usable: Vec<_> = pool
            .iter()
            .filter(|e| nodes.iter().filter(|n| n.name == e.0).count() < e.2)
            .collect();
        let &&(name, _, _) = usable.choose_weighted(rng, |e| e.1).ok()?;
        let t = &LIBRARY.templates[name];
        for _ in 0..8 {
            let &(port, (side, along)) = ports.choose(rng)?;
            let shape = Shape::new(t, if t.fixed { 0 } else { rng.gen_range(0..4) });
            let mine: Vec<_> = (0..shape.ports.len())
                .filter(|&j| shape.ports[j].0 == opposite(side))
                .collect();
            let Some(&own) = mine.choose(rng) else {
                continue;
            };
            let onward = |s| (0..shape.ports.len()).any(|j| j != own && shape.ports[j].0 == s);
            if exit.is_some_and(|s| !onward(s)) {
                continue;
            }
            let len = rng.gen_range(CORRIDOR);
            let (w, h) = shape.size();
            let (tx, ty) = nodes[p].rect.at(side, along, len + 1);
            let (ax, ay) = Rect { x: 0, y: 0, w, h }.at(opposite(side), shape.ports[own].1, 0);
            let rect = Rect {
                x: tx - ax,
                y: ty - ay,
                w,
                h,
            };
            let node = Node {
                name,
                rooms: t.rooms,
                shape,
                rect,
                link: Some(Link {
                    parent: p,
                    port,
                    own,
                    len,
                }),
            };
            let hall = node.corridor(nodes)?;
            let crowded = nodes.iter().any(|n| {
                n.rect.grow(CLEARANCE).overlaps(rect)
                    || n.rect.overlaps(hall)
                    || n.corridor(nodes).is_some_and(|c| {
                        c.grow(CLEARANCE).overlaps(hall) || c.overlaps(rect.grow(CLEARANCE))
                    })
            });
            if !crowded {
                nodes.push(node);
                return Some(nodes.len() - 1);
            }
        }
    }
    None
}

/// Grow the template tree: start, ten path rooms, the boss on a north port of the last, then dead-end branches.
fn plan(rng: &mut StdRng) -> Option<(Vec<Node>, Vec<usize>)> {
    use tuning::*;
    const ANY: &str = "NESW";
    let shape = Shape::new(&LIBRARY.templates["start"], 0);
    let (w, h) = shape.size();
    let mut nodes = vec![Node {
        name: "start",
        rooms: 1,
        shape,
        rect: Rect { x: 0, y: 0, w, h },
        link: None,
    }];
    let mut once = [None; PATH_ROOMS];
    let l = rng.gen_range(L_ROOM);
    once[l] = Some("l_room");
    let mut rest: Vec<_> = (0..PATH_ROOMS).filter(|&k| k != l).collect();
    rest.shuffle(rng);
    once[rest[0]] = Some("acid");
    once[rest[1]] = Some("cluster");
    let mut path = vec![0];
    for (k, name) in once.into_iter().enumerate() {
        let single;
        let pool: &[_] = match name {
            Some(name) => {
                single = [(name, 1.0, 1)];
                &single
            }
            None => &ROOM_WEIGHTS,
        };
        let exit = (k + 1 == PATH_ROOMS).then_some('N');
        path.push(attach(&mut nodes, *path.last()?, pool, ANY, exit, rng)?);
    }
    // The boss is always entered from the south (wiki map, 2012-09-08/09-27/10-09 minimaps).
    path.push(attach(
        &mut nodes,
        *path.last()?,
        &[("boss", 1.0, 1)],
        "N",
        None,
        rng,
    )?);
    let rooms = rng.gen_range(BRANCH_ROOMS);
    let branches = rng.gen_range(BRANCHES).min(rooms);
    let mut cuts = vec![0, rooms];
    while cuts.len() != branches + 1 || cuts.windows(2).any(|w| w[1] - w[0] > BRANCH_MAX) {
        cuts = (1..rooms).collect();
        cuts.shuffle(rng);
        cuts.truncate(branches - 1);
        cuts.extend([0, rooms]);
        cuts.sort();
    }
    for len in cuts.windows(2).map(|w| w[1] - w[0]) {
        let roots: Vec<_> = path[1..=PATH_ROOMS]
            .iter()
            .copied()
            .filter(|&p| !free(&nodes, p).is_empty())
            .collect();
        let mut at = *roots.choose(rng)?;
        for _ in 0..len {
            at = attach(&mut nodes, at, &ROOM_WEIGHTS, ANY, None, rng)?;
        }
    }
    let (x0, y0, x1, y1) = bounds(&nodes);
    let rooms = nodes.iter().map(|n| n.rooms).sum::<usize>() + 1;
    (x1 - x0 <= MAX_EXTENT && y1 - y0 <= MAX_EXTENT && ROOMS.contains(&rooms))
        .then_some((nodes, path))
}
fn bounds(nodes: &[Node]) -> (i32, i32, i32, i32) {
    let rects: Vec<_> = nodes.iter().map(|n| n.rect).collect();
    (
        rects.iter().map(|r| r.x).min().unwrap_or(0),
        rects.iter().map(|r| r.y).min().unwrap_or(0),
        rects.iter().map(|r| r.x + r.w).max().unwrap_or(0),
        rects.iter().map(|r| r.y + r.h).max().unwrap_or(0),
    )
}

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    Ok(build(content, seed)?.0)
}

// Rooms are cut from the wiki map (shapes tile-exact, decor partly estimated; data/README.md). Both 2012 maps share
// the start, the boss room and the kit but differ in arrangement, so the tree is drawn per run.
fn build(content: &Content, seed: u64) -> Result<(Map, Vec<Node>, Vec<usize>)> {
    use tuning::*;
    const EMPTY: u8 = 0xff;
    const WALL: u16 = 0x188c;
    let mut rng = StdRng::seed_from_u64(seed);
    let (attempts, (mut nodes, path)) = (1..=1000)
        .find_map(|n| plan(&mut rng).map(|p| (n, p)))
        .context("Mad Lab template tree did not fit in 1000 attempts")?;
    let (x0, y0, x1, y1) = bounds(&nodes);
    let (dx, dy) = ((WORLD - (x1 - x0)) / 2 - x0, (WORLD - (y1 - y0)) / 2 - y0);
    for n in &mut nodes {
        (n.rect.x, n.rect.y) = (n.rect.x + dx, n.rect.y + dy);
    }
    let ground = |name: &str| {
        content
            .ground_names
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(k, _)| *k)
            .with_context(|| format!("unknown Mad Lab ground {name}"))
    };
    let object = |name: &str| {
        content
            .objects
            .values()
            .find(|o| o.name == name)
            .map(|o| o.kind)
            .with_context(|| format!("unknown Mad Lab object {name}"))
    };
    let tiles = LIBRARY
        .legend
        .iter()
        .map(|(&c, (g, o))| {
            let g = if g.is_empty() { EMPTY } else { ground(g)? };
            let marker = matches!(o.as_str(), "" | "table slot" | "vat field" | "spawn");
            let kind = if marker { None } else { Some(object(o)?) };
            Ok((c, (g, kind, o.as_str())))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let floor = ground("Lab Floor")?;
    let plain = [floor, ground("Lab Grate Floor")?, ground("Metal Plates")?];
    let vats = [ground("Lab Bad Vat Rug")?, ground("Lab Good Vat Rug")?];
    let slats = [
        object("Partial Horizontal Catwalk")?,
        object("Partial Vertical Catwalk")?,
    ];
    let blank = Cell {
        ground: EMPTY,
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
    let mut field = vec![false; map.cells.len()];
    let (mut slots, mut floors, mut wet) = (
        vec![vec![]; nodes.len()],
        vec![vec![]; nodes.len()],
        vec![vec![]; nodes.len()],
    );
    for (i, n) in nodes.iter().enumerate() {
        for (y, row) in n.shape.rows.iter().enumerate() {
            for (x, c) in row.iter().enumerate().filter(|(_, c)| **c != ' ') {
                let p = (n.rect.x + x as i32, n.rect.y + y as i32);
                let &(ground, object, name) = tiles
                    .get(c)
                    .context("Mad Lab template uses an unknown legend character")?;
                let object = object.map(|k| match slats.iter().position(|&s| s == k) {
                    Some(i) => slats[(i + n.shape.turns) % 2],
                    None => k,
                });
                map.cells[at(p)] = Cell {
                    ground,
                    object,
                    ..blank.clone()
                };
                match name {
                    "table slot" => slots[i].push(p),
                    "vat field" => {
                        field[at(p)] = true;
                        wet[i].push(p);
                    }
                    "spawn" => {
                        map.cells[at(p)].region = 1;
                        map.spawns.push((p.0 as f32 + 0.5, p.1 as f32 + 0.5));
                    }
                    _ if object.is_none() && plain.contains(&ground) => floors[i].push(p),
                    _ if object.is_none() && vats.contains(&ground) => wet[i].push(p),
                    _ => {}
                }
            }
        }
    }
    // Joined doors open in both walls; corridors are 4 Lab Floor tiles between Lab Walls (wiki map).
    for n in &nodes {
        let Some(l) = &n.link else { continue };
        let p = &nodes[l.parent];
        let (side, along) = p.shape.ports[l.port];
        let own = n.shape.ports[l.own].1;
        let open = Cell {
            ground: floor,
            ..blank.clone()
        };
        for k in -1..=4 {
            let inner = (0..4).contains(&k);
            if inner {
                map.cells[at(p.rect.at(side, along + k, 0))] = open.clone();
                map.cells[at(n.rect.at(opposite(side), own + k, 0))] = open.clone();
            }
            for out in 1..=l.len {
                map.cells[at(p.rect.at(side, along + k, out))] = if inner {
                    open.clone()
                } else {
                    Cell {
                        object: Some(WALL),
                        ..blank.clone()
                    }
                };
            }
        }
    }
    // Every open vat and acid pool is framed on the floor round it by MadLab Warning pieces whose stripes face it:
    // https://www.youtube.com/watch?v=t0f0_BWpBtw&t=155 (2012-09-08) t=155-163 (south pool), t=946-990 (vat rooms);
    // the room screenshot on the wiki page (`tuning`), image sAihx. Catwalks cover their tile, so a catwalk is floor.
    // Vat fields (the start's Hexxer ring, the L room) have none: https://www.youtube.com/watch?v=s9XOYh8DJQ8 (2012-10-09)
    // t=0-2, t=21.5-23.5. Each gas spawner stands in a frame of eight pieces: t0f0_BWpBtw t=940.
    let pools = [vats[0], vats[1], ground("Liquid Evil")?];
    let pieces = [
        "U", "R", "D", "L", "Inner UR", "Inner UL", "Inner DR", "Inner DL", "UR", "UL", "DR", "DL",
    ]
    .iter()
    .map(|piece| object(&format!("MadLab Warning {piece}")))
    .collect::<Result<Vec<_>>>()?;
    let pool = |map: &Map, (x, y): (i32, i32)| {
        let c = &map.cells[at((x, y))];
        pools.contains(&c.ground) && c.object.is_none() && !field[at((x, y))]
            || c.object.is_some_and(|k| (0x993..=0x996).contains(&k))
    };
    let mut borders = vec![];
    for y in 1..WORLD - 1 {
        for x in 1..WORLD - 1 {
            let c = &map.cells[at((x, y))];
            if c.object.is_some() || pools.contains(&c.ground) || content.grounds[&c.ground] <= 0. {
                continue;
            }
            let near = |dx, dy| pool(&map, (x + dx, y + dy));
            // Stripes on the side (or the two sides, or the corner) that touches the pool.
            let piece = match (near(0, -1), near(1, 0), near(0, 1), near(-1, 0)) {
                (true, false, false, false) => 0,
                (false, true, false, false) => 1,
                (false, false, true, false) => 2,
                (false, false, false, true) => 3,
                (true, true, false, false) => 4,
                (true, false, false, true) => 5,
                (false, true, true, false) => 6,
                (false, false, true, true) => 7,
                (false, false, false, false) => {
                    match (near(1, -1), near(-1, -1), near(1, 1), near(-1, 1)) {
                        (true, false, false, false) => 8,
                        (false, true, false, false) => 9,
                        (false, false, true, false) => 10,
                        (false, false, false, true) => 11,
                        _ => continue,
                    }
                }
                _ => continue,
            };
            borders.push(((x, y), piece));
        }
    }
    for ((x, y), piece) in borders {
        map.cells[at((x, y))].object = Some(pieces[piece]);
    }
    for list in slots.iter_mut().chain(&mut floors).chain(&mut wet) {
        list.retain(|&q| map.cells[at(q)].object.is_none());
    }
    // A few Lab Tables on the table rooms' table slots; the other slots stay floor. Then the previous rule's four
    // random enemies per room: dry floor first, vat-filled rooms (the L room) fall back to vat tiles. The acid room
    // holds only its turrets: no enemy stands in it on the 123.5.1 wiki screenshot (image 9ol81, `tuning` link) or in
    // https://www.youtube.com/watch?v=xyUiQewDLg8 (2012-09-16) t=58-92.
    for (i, n) in nodes.iter().enumerate() {
        if matches!(n.name, "start" | "boss") {
            continue;
        }
        for list in [&mut slots[i], &mut floors[i], &mut wet[i]] {
            list.shuffle(&mut rng);
        }
        let tables = rng.gen_range(TABLES).min(slots[i].len());
        for &q in &slots[i][..tables] {
            map.cells[at(q)].object = Some(TABLE);
        }
        let spots: Vec<_> = floors[i]
            .iter()
            .chain(&slots[i][tables..])
            .chain(&wet[i])
            .copied()
            .take(if n.name == "acid" { 0 } else { PACK * n.rooms })
            .collect();
        for q in spots {
            map.cells[at(q)].object = Some(
                *[0x979, 0x980, 0x981, 0x982, 0x983]
                    .choose(&mut rng)
                    .unwrap(),
            );
        }
    }
    // A pool actor acts on the 3x3 tiles round it (world::lab_step): cover every open vat tile, preferring actors
    // whose 3x3 stays inside the vat. Catwalk tiles are floor and need none.
    let around =
        |(x, y): (i32, i32)| (-1..=1).flat_map(move |a| (-1..=1).map(move |b| (x + a, y + b)));
    for (vat, actor) in [
        (ground("Lab Bad Vat Rug")?, HEXXER),
        (ground("Lab Good Vat Rug")?, DEHEXXER),
    ] {
        let mut covered = vec![false; map.cells.len()];
        for i in 0..map.cells.len() {
            if map.cells[i].ground != vat
                || covered[i]
                || map.cells[i]
                    .object
                    .is_some_and(|k| content.objects[&k].protects_ground)
            {
                continue;
            }
            let t = (i as i32 % WORLD, i as i32 / WORLD);
            let score = |q: (i32, i32)| {
                around(q)
                    .map(|r| match map.cells[at(r)].ground {
                        g if g == vat => i32::from(!covered[at(r)]),
                        EMPTY => 0,
                        _ => -2,
                    })
                    .sum::<i32>()
            };
            let Some(best) = around(t)
                .filter(|&q| map.cells[at(q)].ground == vat && map.cells[at(q)].object.is_none())
                .max_by_key(|&q| score(q))
            else {
                continue;
            };
            map.cells[at(best)].object = Some(actor);
            for r in around(best) {
                covered[at(r)] = true;
            }
        }
    }
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Mad Lab ground {:#x}",
            c.ground
        );
        if let Some(kind) = c.object {
            content.get(kind)?;
        }
    }
    ensure!(map.spawns.len() == 1, "Mad Lab start lacks its spawn");
    tracing::info!(
        seed,
        attempts,
        templates = nodes.len(),
        rooms = nodes.iter().map(|n| n.rooms).sum::<usize>() + 1,
        width = x1 - x0,
        height = y1 - y0,
        "generated Mad Lab template tree"
    );
    Ok((map, nodes, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn inside(r: Rect, (x, y): (i32, i32)) -> bool {
        (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y)
    }
    /// Cells reachable from spawn; enemies (coils included) count as passable, Emergency Walls only when `open`.
    fn reach(content: &Content, map: &Map, open: bool) -> Vec<bool> {
        let walk = |c: &Cell| {
            content.grounds[&c.ground] > 0.
                && c.object.is_none_or(|k| {
                    !content.objects[&k].blocks
                        || content.objects[&k].enemy
                        || (open && k == EMERGENCY)
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
    fn find(map: &Map, kind: u16) -> Vec<(i32, i32)> {
        (0..map.cells.len() as i32)
            .filter(|&i| map.cells[i as usize].object == Some(kind))
            .map(|i| (i % map.width, i / map.width))
            .collect()
    }

    #[test]
    fn lab_tree_follows_the_2012_maps() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, path) = build(&content, seed).unwrap();
            let rooms = nodes.iter().map(|n| n.rooms).sum::<usize>() + 1;
            assert!(tuning::ROOMS.contains(&rooms), "seed {seed}: {rooms} rooms");
            assert!(nodes[0].name == "start" && nodes[0].link.is_none());
            // Ten rooms between start and boss; the boss hangs off a north-facing port (entered from the south).
            assert_eq!(path.len(), tuning::PATH_ROOMS + 2, "seed {seed}");
            for w in path.windows(2) {
                assert_eq!(nodes[w[1]].link.as_ref().unwrap().parent, w[0]);
            }
            let boss = *path.last().unwrap();
            let l = nodes[boss].link.as_ref().unwrap();
            assert!(nodes[boss].name == "boss" && nodes[l.parent].shape.ports[l.port].0 == 'N');
            let count = |name: &str| nodes.iter().filter(|n| n.name == name).count();
            assert!(
                ["acid", "cluster", "l_room", "boss"]
                    .iter()
                    .all(|n| count(n) == 1)
            );
            assert!(path[1..=2].iter().any(|&i| nodes[i].name == "l_room"));
            // The L room's catwalk walkway joins its two doors without touching an open green vat.
            let l = nodes.iter().find(|n| n.name == "l_room").unwrap();
            let r = l.rect;
            let cell = |x: i32, y: i32| &map.cells[(y * map.width + x) as usize];
            let free = |x: i32, y: i32| {
                cell(x, y)
                    .object
                    .is_none_or(|k| !content.objects[&k].blocks)
            };
            let doors: Vec<_> = (r.y..r.y + r.h)
                .flat_map(|y| (r.x..r.x + r.w).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    (x == r.x || y == r.y || x == r.x + r.w - 1 || y == r.y + r.h - 1)
                        && free(x, y)
                        && cell(x, y).ground != 0xff
                })
                .collect();
            let dry = |x: i32, y: i32| {
                (r.x..r.x + r.w).contains(&x)
                    && (r.y..r.y + r.h).contains(&y)
                    && free(x, y)
                    && !matches!(cell(x, y).ground, 0xa9 | 0xff)
            };
            let mut seen = vec![doors[0]];
            let mut queue = vec![doors[0]];
            while let Some((x, y)) = queue.pop() {
                for q in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if dry(q.0, q.1) && !seen.contains(&q) {
                        seen.push(q);
                        queue.push(q);
                    }
                }
            }
            assert!(
                doors.iter().all(|d| seen.contains(d)),
                "seed {seed}: L room walkway"
            );
            assert!(
                path[1..=tuning::PATH_ROOMS]
                    .iter()
                    .any(|&i| nodes[i].name == "acid")
            );
            // A tree without overlaps: every node but the start hangs off an earlier one; side branches dead-end.
            for (i, n) in nodes.iter().enumerate().skip(1) {
                assert!(n.link.as_ref().unwrap().parent < i);
                for o in &nodes[..i] {
                    assert!(!o.rect.overlaps(n.rect), "seed {seed}: overlap");
                }
            }
            // Every floor cell is reachable once the Emergency Walls fall; Dr Terrible and the Creation too.
            let all = reach(&content, &map, true);
            let walkable = |c: &Cell| {
                content.grounds[&c.ground] > 0.
                    && c.object.is_none_or(|k| !content.objects[&k].blocks)
            };
            assert!(
                map.cells.iter().zip(&all).all(|(c, r)| *r || !walkable(c)),
                "seed {seed}: unreachable floor"
            );
            let at = |(x, y): (i32, i32)| (y * map.width + x) as usize;
            let (dr, creation) = (find(&map, BOSS)[0], find(&map, CREATION)[0]);
            assert!(all[at(dr)] && all[at(creation)], "seed {seed}");
            // The Horrific Creation waits north of Dr Terrible behind the walls.
            let sealed = reach(&content, &map, false);
            assert!(
                sealed[at(dr)] && !sealed[at(creation)],
                "seed {seed}: H room open"
            );
            assert!(creation.1 < dr.1 - 11 && (creation.0 - dr.0).abs() <= 1);
            let walls = find(&map, EMERGENCY);
            assert!(
                walls.len() == 4
                    && walls
                        .iter()
                        .all(|&(x, y)| y == dr.1 - 11 && (dr.0 - 1..=dr.0 + 2).contains(&x)),
                "seed {seed}: neck"
            );
            // A Tesla Coil on every Lab Floor Power tile but the gas spawners' and nowhere else: as many per room as its
            // template has dots, none in the start, L room and acid room.
            let power = content
                .ground_names
                .iter()
                .find(|(_, n)| *n == "Lab Floor Power")
                .map(|(k, _)| *k)
                .unwrap();
            let coils = find(&map, TESLA);
            assert!(coils.iter().all(|&(x, y)| cell(x, y).ground == power));
            assert!(
                (0..map.cells.len())
                    .filter(|&i| map.cells[i].ground == power)
                    .all(|i| matches!(map.cells[i].object, Some(TESLA | 0x993..=0x996))),
                "seed {seed}: bare Lab Floor Power"
            );
            for n in &nodes {
                let dots: usize = LIBRARY.templates[n.name]
                    .rows
                    .iter()
                    .map(|r| r.matches(['o', 'T']).count())
                    .sum();
                let mine = coils.iter().filter(|&&q| inside(n.rect, q)).count();
                assert_eq!(mine, dots, "seed {seed}: {}", n.name);
                match n.name {
                    "start" | "l_room" | "acid" => assert_eq!(mine, 0),
                    "boss" => {
                        // Four under the north wall, two each side of the neck (x dr-1..dr+2), mirrored about it and
                        // evenly spaced; none south of the room, in the entrance pool section.
                        let mut xs: Vec<_> = coils
                            .iter()
                            .filter(|&&q| inside(n.rect, q))
                            .map(|&(x, y)| {
                                assert_eq!(y, dr.1 - 10, "seed {seed}: boss coil row");
                                x - dr.0
                            })
                            .collect();
                        xs.sort();
                        assert_eq!(xs, [-8, -4, 5, 9], "seed {seed}: boss coils");
                    }
                    _ => assert!(mine > 0, "seed {seed}: {}", n.name),
                }
            }
        }
    }

    #[test]
    fn lab_acid_room_holds_only_its_turrets() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            let acid = nodes.iter().find(|n| n.name == "acid").unwrap().rect;
            let enemies: Vec<_> = (0..map.cells.len() as i32)
                .map(|i| (i % map.width, i / map.width))
                .filter(|&q| inside(acid, q))
                .filter_map(|(x, y)| map.cells[(y * map.width + x) as usize].object)
                .filter(|&k| content.objects[&k].enemy && !matches!(k, 0x990 | 0x991))
                .collect();
            assert!(
                enemies.is_empty(),
                "seed {seed}: {enemies:x?} in the acid room"
            );
            // Every other room still gets its pack.
            for n in nodes
                .iter()
                .filter(|n| !matches!(n.name, "start" | "boss" | "acid"))
            {
                let pack = (0..map.cells.len() as i32)
                    .map(|i| (i % map.width, i / map.width))
                    .filter(|&q| inside(n.rect, q))
                    .filter(|&(x, y)| {
                        matches!(
                            map.cells[(y * map.width + x) as usize].object,
                            Some(0x979..=0x983)
                        )
                    })
                    .count();
                assert_eq!(pack, tuning::PACK * n.rooms, "seed {seed}: {}", n.name);
            }
        }
    }

    #[test]
    fn lab_boss_room_follows_the_contract() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, ..) = build(&content, seed).unwrap();
            let boss = find(&map, BOSS);
            assert_eq!(boss.len(), 1, "seed {seed}");
            let (cx, cy) = boss[0];
            assert_eq!(find(&map, BUBBLE), [(cx, cy - 1)]);
            let mut cages = find(&map, CAGE);
            cages.sort();
            let mut expected = vec![
                (cx - 9, cy - 1),
                (cx - 9, cy + 1),
                (cx + 9, cy - 1),
                (cx + 9, cy + 1),
            ];
            expected.sort();
            assert_eq!(cages, expected, "seed {seed}");
            for (kind, offset) in [
                (0x993, (-6, -5)),
                (0x994, (6, -5)),
                (0x995, (-6, 5)),
                (0x996, (6, 5)),
            ] {
                assert_eq!(find(&map, kind), [(cx + offset.0, cy + offset.1)]);
            }
            // Open floor the encounter tests stand on, offsets from Dr Terrible's position (cell centre).
            for (dx, dy) in [
                (0., 9.),
                (-6., 0.),
                (7.5, 7.5),
                (-8., 3.),
                (-9., 3.),
                (3., 0.),
                (4., 0.),
            ] {
                let x = (cx as f32 + 0.5 + dx).floor() as i32;
                let y = (cy as f32 + 0.5 + dy).floor() as i32;
                let c = &map.cells[(y * map.width + x) as usize];
                assert!(
                    content.grounds[&c.ground] > 0. && c.object.is_none(),
                    "seed {seed}: floor at {dx},{dy}"
                );
            }
            let (sx, sy) = map.spawns[0];
            assert!((sx - cx as f32).hypot(sy - cy as f32) >= 20.);
            // 22x21 room: walls 11-12 tiles out; the south entrance corridor holds the DeHexxer pool.
            let wall = |x, y| map.cells[(y * map.width + x) as usize].object == Some(0x188c);
            assert!(wall(cx - 12, cy) && wall(cx + 11, cy) && wall(cx - 5, cy - 11));
            assert!(wall(cx - 5, cy + 11) && !wall(cx, cy + 11));
            assert!(
                find(&map, DEHEXXER)
                    .iter()
                    .any(|&(x, y)| (y - cy - 14).abs() <= 1 && (x - cx).abs() <= 3),
                "seed {seed}: south pool"
            );
        }
    }

    #[test]
    fn lab_has_one_acid_room_with_full_turret_columns_and_covered_pools() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            let acid: Vec<_> = nodes.iter().filter(|n| n.name == "acid").collect();
            assert_eq!(acid.len(), 1);
            let r = acid[0].rect;
            let kind = |x, y| map.cells[(y * map.width + x) as usize].object;
            for y in r.y + 1..r.y + r.h - 1 {
                assert_eq!(kind(r.x + 1, y), Some(0x990), "seed {seed}");
                assert_eq!(kind(r.x + r.w - 2, y), Some(0x991), "seed {seed}");
            }
            assert_eq!(find(&map, 0x990).len(), r.h as usize - 2);
            assert_eq!(find(&map, 0x991).len(), r.h as usize - 2);
            // Every vat tile without an object lies in the 3x3 of a matching pool actor.
            for (vat, actor) in [(0xa9, HEXXER), (0xa7, DEHEXXER)] {
                let actors = find(&map, actor);
                for (i, c) in map.cells.iter().enumerate() {
                    let (x, y) = (i as i32 % map.width, i as i32 / map.width);
                    if c.ground == vat && c.object.is_none() {
                        assert!(
                            actors
                                .iter()
                                .any(|&(a, b)| (a - x).abs() <= 1 && (b - y).abs() <= 1),
                            "seed {seed}: uncovered vat at {x},{y}"
                        );
                    }
                }
            }
            // The Creation's room: two Hexxer vats west of it, two DeHexxer vats east.
            let (hx, hy) = find(&map, CREATION)[0];
            let near = |kind| {
                find(&map, kind)
                    .into_iter()
                    .filter(|&(x, y)| (x - hx).abs() <= 9 && (y - hy).abs() <= 7)
                    .collect::<Vec<_>>()
            };
            assert!(near(HEXXER).len() >= 2 && near(HEXXER).iter().all(|q| q.0 < hx));
            assert!(near(DEHEXXER).len() >= 2 && near(DEHEXXER).iter().all(|q| q.0 > hx));
        }
    }

    /// A vat or acid tile no catwalk covers.
    fn open(content: &Content, c: &Cell) -> bool {
        matches!(c.ground, BAD_VAT | GOOD_VAT | 0xa8)
            && c.object
                .is_none_or(|k| !content.objects[&k].protects_ground)
    }

    #[test]
    fn lab_frames_every_open_pool_with_hazard_borders() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            let cell = |x: i32, y: i32| &map.cells[(y * map.width + x) as usize];
            // Green vat in the start and the L room is a field: no stripe faces it (touch leaves it out).
            let field = |x, y| {
                cell(x, y).ground == BAD_VAT
                    && nodes
                        .iter()
                        .any(|n| matches!(n.name, "start" | "l_room") && inside(n.rect, (x, y)))
            };
            let open = |x, y| {
                open(&content, cell(x, y)) && !field(x, y)
                    || matches!(cell(x, y).object, Some(0x993..=0x996))
            };
            let mut framed = vec![0; nodes.len()];
            for y in 1..map.height - 1 {
                for x in 1..map.width - 1 {
                    let c = cell(x, y);
                    if open(x, y) || content.grounds[&c.ground] <= 0. {
                        continue;
                    }
                    let touch: String = [('U', 0, -1), ('R', 1, 0), ('D', 0, 1), ('L', -1, 0)]
                        .into_iter()
                        .filter(|&(_, dx, dy)| open(x + dx, y + dy))
                        .map(|(side, ..)| side)
                        .collect();
                    let piece = c
                        .object
                        .and_then(|k| content.objects[&k].name.strip_prefix("MadLab Warning "));
                    let Some(piece) = piece else {
                        // A free floor tile beside a pool on one side or two adjacent sides is always framed.
                        assert!(
                            c.object.is_some()
                                || !matches!(
                                    touch.as_str(),
                                    "U" | "R" | "D" | "L" | "UR" | "UL" | "RD" | "DL"
                                ),
                            "seed {seed}: unframed floor at {x},{y} beside {touch}"
                        );
                        continue;
                    };
                    // Stripes face exactly the pools: sides for side and inner pieces, the diagonal for corners.
                    match piece.strip_prefix("Inner ") {
                        Some(sides) => {
                            let mut want: Vec<char> = sides.chars().collect();
                            let mut got: Vec<char> = touch.chars().collect();
                            want.sort();
                            got.sort();
                            assert_eq!(want, got, "seed {seed}: {piece} at {x},{y}");
                        }
                        None if piece.len() == 1 => {
                            assert_eq!(touch, piece, "seed {seed}: {piece} at {x},{y}")
                        }
                        None => {
                            let dx = if piece.ends_with('R') { 1 } else { -1 };
                            let dy = if piece.starts_with('U') { -1 } else { 1 };
                            assert!(
                                touch.is_empty() && open(x + dx, y + dy),
                                "seed {seed}: {piece} at {x},{y}"
                            );
                        }
                    }
                    if let Some(i) = nodes.iter().position(|n| inside(n.rect, (x, y))) {
                        framed[i] += 1;
                    }
                }
            }
            // Each gas spawner stands in a full frame of eight pieces.
            for (x, y) in (0x993..=0x996).flat_map(|k| find(&map, k)) {
                let pieces = (-1..=1)
                    .flat_map(|a| (-1..=1).map(move |b| (x + a, y + b)))
                    .filter(|&(a, b)| {
                        cell(a, b).object.is_some_and(|k| {
                            content.objects[&k].name.starts_with("MadLab Warning ")
                        })
                    })
                    .count();
                assert_eq!(pieces, 8, "seed {seed}: gas spawner at {x},{y}");
            }
            // Every room with an open pool beside free floor shows a border, the boss room's south pool included
            // (2012-09-08 t=155-163); the others show none.
            for (i, n) in nodes.iter().enumerate() {
                let r = n.rect;
                let free = |x: i32, y: i32| {
                    cell(x, y)
                        .object
                        .is_none_or(|k| content.objects[&k].name.starts_with("MadLab Warning "))
                        && content.grounds[&cell(x, y).ground] > 0.
                        && !open(x, y)
                };
                let pooled = (r.y..r.y + r.h).any(|y| {
                    (r.x..r.x + r.w).any(|x| {
                        open(x, y)
                            && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                                .iter()
                                .any(|(a, b)| free(x + a, y + b))
                    })
                });
                assert_eq!(framed[i] > 0, pooled, "seed {seed}: {}", n.name);
            }
        }
    }

    #[test]
    fn lab_cluster_and_pair_cells_hold_framed_vats_in_catwalk_floor() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            let cell = |x: i32, y: i32| &map.cells[(y * map.width + x) as usize];
            let pools: Vec<_> = find(&map, HEXXER)
                .into_iter()
                .chain(find(&map, DEHEXXER))
                .collect();
            for n in nodes
                .iter()
                .filter(|n| matches!(n.name, "cluster" | "pair"))
            {
                let r = n.rect;
                let (mut open_vat, mut walkable) = (0, 0);
                for y in r.y..r.y + r.h {
                    for x in r.x..r.x + r.w {
                        let c = cell(x, y);
                        let floor = content.grounds[&c.ground] > 0.
                            && c.object.is_none_or(|k| {
                                !content.objects[&k].blocks || content.objects[&k].enemy
                            });
                        walkable += i32::from(floor);
                        if open(&content, c) {
                            // Open vats sit inside metal frames: never beside a wall or a catwalk.
                            open_vat += 1;
                            for (a, b) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                                assert!(
                                    open(&content, cell(a, b)) || cell(a, b).ground == 0x31,
                                    "seed {seed}: {} vat at {x},{y} not framed",
                                    n.name
                                );
                            }
                        } else if c
                            .object
                            .is_some_and(|k| content.objects[&k].protects_ground)
                        {
                            // Catwalk over the vat outside the frames stays dry: no pool actor reaches it.
                            assert!(
                                pools
                                    .iter()
                                    .all(|&(a, b)| (a - x).abs() > 1 || (b - y).abs() > 1),
                                "seed {seed}: wet catwalk at {x},{y}"
                            );
                        }
                    }
                }
                // Walkable catwalk and plates round framed vats, as on 2012-09-08 t=946-990: open vat is a quarter of
                // the cluster (it was three quarters with the vat left bare outside the frames).
                assert!(
                    open_vat * 3 < walkable,
                    "seed {seed}: {} has {open_vat} open vat tiles of {walkable}",
                    n.name
                );
            }
        }
    }

    #[test]
    fn lab_catwalks_neither_slow_nor_hurt_nor_hex() {
        // The 123.5.1 client moves a player at the ground's Speed whatever object stands on the tile (Player.as reads
        // only the square's ground: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as),
        // so a catwalk lies on a ground of normal speed with no damage that no pool acts on: the vat look-alikes Lab
        // Bad/Good Vat No Slow under catwalks over vats, Bright Grass under the acid-room lattice, grate or floor under
        // the walkway cross's plates.
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, ..) = build(&content, seed).unwrap();
            let catwalks: Vec<_> = map
                .cells
                .iter()
                .filter(|c| {
                    c.object
                        .is_some_and(|k| content.objects[&k].protects_ground)
                })
                .collect();
            assert!(catwalks.len() > 100, "seed {seed}");
            for c in catwalks {
                let name = &content.ground_names[&c.ground];
                assert!(
                    content.grounds[&c.ground] == 1.
                        && !content.hazards.contains_key(&c.ground)
                        && !matches!(c.ground, BAD_VAT | GOOD_VAT | 0xa8),
                    "seed {seed}: catwalk on {name}"
                );
            }
        }
    }

    #[test]
    fn lab_walkways_are_2012_catwalks_whose_slats_turn_with_the_room() {
        // The L room's walkway (https://www.youtube.com/watch?v=s9XOYh8DJQ8 2012-10-09 t=21.5-23.5): plates along its
        // edges and 3x3 blocks of slats, the slats running along the walkway; the walkway cross follows it.
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let kind = |name: &str| {
            content
                .objects
                .values()
                .find(|o| o.name == name)
                .unwrap()
                .kind
        };
        let (plate, h, v) = (
            kind("Partial Catwalk"),
            kind("Partial Horizontal Catwalk"),
            kind("Partial Vertical Catwalk"),
        );
        let mut turned = [false; 2];
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            for n in nodes
                .iter()
                .filter(|n| matches!(n.name, "l_room" | "walkway_cross" | "cluster" | "pair"))
            {
                turned[n.shape.turns % 2] = true;
                for (y, row) in n.shape.rows.iter().enumerate() {
                    for (x, c) in row.iter().enumerate() {
                        let cell = &map.cells
                            [((n.rect.y + y as i32) * map.width + n.rect.x + x as i32) as usize];
                        let want = match c {
                            'c' | 'n' | '=' | '_' | 'M' => plate,
                            'v' => [v, h][n.shape.turns % 2],
                            'q' | 'p' => [h, v][n.shape.turns % 2],
                            _ => continue,
                        };
                        assert_eq!(
                            cell.object,
                            Some(want),
                            "seed {seed}: {} {c} at {x},{y}",
                            n.name
                        );
                    }
                }
            }
        }
        assert_eq!(turned, [true, true]);
        // The L room has no grate, no bare plates and a field all round its walkway; its DeHexxer pool at the bend is
        // ringed by catwalk only, so no hazard border (t0f0_BWpBtw 2012-09-08 t=40.5-41).
        let rows = &LIBRARY.templates["l_room"].rows;
        assert!(rows.iter().all(|r| !r.contains([':', 'g', 'm'])));
    }

    #[test]
    fn lab_table_rooms_hold_a_rare_chemistry_table_and_no_plain_tables() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let mut per_room = vec![];
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            // The purple Lab Table Plain columns are gone; the wiki map's brown in-room tiles are equipment walls.
            assert!(find(&map, 0x18ac).is_empty(), "seed {seed}");
            let tables = find(&map, TABLE);
            let rooms: Vec<_> = nodes.iter().filter(|n| n.name == "table_room").collect();
            assert!(
                tables
                    .iter()
                    .all(|&q| rooms.iter().any(|r| inside(r.rect, q))),
                "seed {seed}: table outside a table room"
            );
            for room in rooms {
                let mine: Vec<_> = tables.iter().filter(|&&q| inside(room.rect, q)).collect();
                assert!(tuning::TABLES.contains(&mine.len()), "seed {seed}");
                per_room.push(mine.len());
                // Each stands on a table slot, one tile inside the room's wall.
                for &&(x, y) in &mine {
                    let r = room.rect;
                    assert!(
                        x == r.x + 1 || y == r.y + 1 || x == r.x + r.w - 2 || y == r.y + r.h - 2,
                        "seed {seed}: table at {x},{y}"
                    );
                }
            }
        }
        // Rare: some table rooms have none, none has more than two, about one per room on average.
        let total: usize = per_room.iter().sum();
        assert!(per_room.contains(&0) && per_room.iter().any(|&n| n > 0));
        assert!(
            total * 2 <= per_room.len() * 3,
            "{total} tables in {} rooms",
            per_room.len()
        );
    }

    #[test]
    fn lab_start_holds_the_portal_of_cowardice() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, _) = build(&content, seed).unwrap();
            let (sx, sy) = map.spawns[0];
            let spawn = (sx as i32, sy as i32);
            assert!(inside(nodes[0].rect, spawn));
            assert_eq!(
                map.cells[(spawn.1 * map.width + spawn.0) as usize].region,
                1
            );
            let portals = find(&map, 0x703);
            assert!(
                portals.len() == 1
                    && inside(nodes[0].rect, portals[0])
                    && (portals[0].0 - spawn.0).abs() + (portals[0].1 - spawn.1).abs() <= 3
            );
        }
    }

    #[test]
    fn lab_layout_generates_quickly() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let started = std::time::Instant::now();
            generate(&content, seed).unwrap();
            assert!(
                started.elapsed() < std::time::Duration::from_secs(2),
                "seed {seed}"
            );
        }
    }

    #[test]
    #[ignore = "timing report: cargo test --release lab_generation_time -- --ignored --nocapture"]
    fn lab_generation_time_over_1000_seeds() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let mut times: Vec<_> = (0..1000)
            .map(|seed| {
                let started = std::time::Instant::now();
                generate(&content, seed).unwrap();
                started.elapsed().as_secs_f64() * 1000.
            })
            .collect();
        times.sort_by(f64::total_cmp);
        println!(
            "Mad Lab generation over 1000 seeds: median {:.2} ms, p99 {:.2} ms, max {:.2} ms",
            times[500], times[990], times[999]
        );
    }
}
