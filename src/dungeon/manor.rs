use crate::{
    content::{Cell, Content, Map},
    encounter::{Anchor, Motion, Profile},
};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::{collections::BTreeMap, sync::LazyLock};

const MAX_ROOMS: usize = 30;
// Skipped bat rooms retain their broods; leave headroom for Ruthven's overlapping cohorts.
pub const SUMMON_LIMIT: usize = (MAX_ROOMS + 2) * 4 * 20;

pub const BOSS: u16 = 0x1720;
pub const BAT: u16 = 0x1721;
pub const CREATURE: u16 = 0x1722;
pub const SWARM: u16 = 0x1723;
pub const ESCORT: u16 = 0x1724;
pub const DECOY: u16 = 0x1725;
pub const CHASER: u16 = 0x1726;
pub const COFFIN: u16 = 0x1737;
pub const FERATU: u16 = 0x1738;
pub const BOOKCASE: u16 = 0x181a;
pub const ROCK: u16 = 0x172a;
pub const KAGE: u16 = 0x6db;
pub const MINE: u16 = 0x6da;
pub const GRAVE: u16 = 0x181f;
pub const ROSTER: [u16; 18] = [
    BOSS, BAT, CREATURE, SWARM, ESCORT, DECOY, CHASER, 0x1727, 0x1728, 0x1729, ROCK, 0x172b,
    0x172c, COFFIN, FERATU, KAGE, MINE, GRAVE,
];

// Phoenix supplies reconstruction timers; the 2012 guide establishes timed return, not a coffin kill gate.
// https://web.archive.org/web/20120516045129id_/http://forums.wildshadow.com/wiki/realm-mad-god/manor-immortals
pub fn phase(elapsed: u64) -> (u64, u64) {
    let t = elapsed % 21750;
    let (part, start) = if t < 9100 {
        (0, 0)
    } else if t < 16750 {
        (1, 9100)
    } else {
        (2, 16750)
    };
    (elapsed / 21750 * 3 + part, t - start)
}
pub fn blades(hp: i32) -> usize {
    (3 + (15000 - hp).max(0) as usize / 1000).min(18)
}
pub fn initial_condition(kind: u16) -> i32 {
    use crate::condition::{ARMORED, INVINCIBLE, bit};
    match kind {
        0x1729 => bit(ARMORED),
        // Size-zero anchors must not absorb native shots or show protection icons.
        SWARM | DECOY => bit(INVINCIBLE),
        _ => 0,
    }
}
pub fn profile(kind: u16) -> Profile {
    use Motion::*;
    let (motion, speed, range) = match kind {
        BOSS => (WanderHome { radius: 1. }, 1., 30.),
        BAT | CHASER | 0x1727 => (
            Chase {
                sight: 20.,
                stop: if kind == BAT { 2. } else { 0. },
            },
            8.,
            1.,
        ),
        ESCORT => (
            Protect {
                anchor: Anchor::Parent,
                distance: 4.,
            },
            8.,
            1.,
        ),
        SWARM => (
            Protect {
                anchor: Anchor::Parent,
                distance: 0.3,
            },
            8.,
            0.,
        ),
        DECOY => (
            Chase {
                sight: 20.,
                stop: 2.,
            },
            6.,
            0.,
        ),
        CREATURE | COFFIN | ROCK => (Stationary, 0., 10.),
        KAGE | MINE | GRAVE => (Stationary, 0., 20.),
        0x1728 => (
            Chase {
                sight: 10.,
                stop: 1.,
            },
            2.,
            10.,
        ),
        0x1729 => (
            Chase {
                sight: 10.,
                stop: 1.,
            },
            2.,
            10.,
        ),
        0x172b | 0x172c => (
            Charge {
                sight: 6.,
                cooldown: 0,
                delay: 0,
                stop: 1.,
                idle: 0.,
            },
            6.,
            if kind == 0x172b { 10. } else { 9. },
        ),
        FERATU => (WanderHome { radius: 1. }, 1., 8.),
        _ => unreachable!("unregistered Manor actor {kind:#x}"),
    };
    Profile {
        motion,
        // Phoenix integrates legacy speed/1.5; these are not NR-Core movement rates.
        speed: speed / 1.5,
        range,
        interval: 100,
    }
}

/// Every layout tunable. TKR = the live-instance capture behind data/manor-templates.json (data/README.md); "maps" =
/// TKR plus six independent full-map minimap screenshots (2013-2023), checked against 154 period runs on video.
pub mod tuning {
    use std::ops::RangeInclusive;
    /// Rooms strictly between start and boss: always 9 (JP wiki; all 7 maps, plus the runner hall in the boss).
    pub const PATH_ROOMS: usize = 9;
    /// Side branches off the start or path rooms (maps: 2-4) sharing 14-18 ordinary rooms (maps: 14-18).
    pub const BRANCHES: RangeInclusive<usize> = 2..=4;
    pub const BRANCH_ROOMS: RangeInclusive<usize> = 14..=18;
    /// Rooms in one branch: maps 1-11.
    pub const BRANCH_MAX: usize = 11;
    /// Chance a branch room hangs off an earlier room of its branch, not the last (maps: about 8 of 113 rooms).
    pub const FORK: f64 = 0.07;
    /// Corridor tiles between two joined sockets: 1-9, near uniform in the five tile-exact maps (124 corridors).
    pub const CORRIDOR: RangeInclusive<i32> = 1..=9;
    /// Least empty tiles between templates that are not joined: TKR minimum 1.
    pub const CLEARANCE: i32 = 1;
    /// Template (weight, most copies per run): instance counts in the five tile-exact layouts (TKR capture,
    /// 2014/2015/2017/2023 minimaps) and the most copies in any one of them. `l_room_b` is `l_room` turned 180
    /// degrees; templates below `cell_block` are shape-exact minimap cuts.
    pub const ROOM_WEIGHTS: [(&str, f64, usize); 30] = [
        ("l_room", 4.0, 3),
        ("l_room_b", 3.0, 2),
        ("pillared", 10.0, 3),
        ("portrait", 3.0, 2),
        ("gallery", 6.0, 2),
        ("gallery_plain", 5.0, 1),
        ("library_ring", 3.0, 1),
        ("cross_library", 3.0, 1),
        ("alcove", 4.0, 2),
        ("colonnade", 9.0, 3),
        ("big_rug", 3.0, 1),
        ("long_hall", 5.0, 2),
        ("t_room", 5.0, 1),
        ("brick_hall", 4.0, 2),
        ("closet_hall", 4.0, 2),
        ("cell_block", 3.0, 2),
        ("chamber", 3.0, 2),
        ("cross_small", 3.0, 2),
        ("gallery_16x13", 2.0, 2),
        ("gallery_long", 2.0, 1),
        ("hall_12x20", 6.0, 2),
        ("hall_16x17", 3.0, 2),
        ("hall_16x22", 5.0, 4),
        ("notched", 5.0, 3),
        ("octagon", 1.0, 1),
        ("pillared_wide", 6.0, 5),
        ("purple_20x20", 3.0, 1),
        ("rug_16x16", 6.0, 3),
        ("rug_16x16b", 2.0, 2),
        ("rug_18x14", 5.0, 3),
    ];
    /// Octagon ("bottle") vs moat start: 23 vs 9 of the Apr-Dec 2012 runs on video.
    pub const START_WEIGHTS: [(&str, f64); 2] = [("start", 23.0), ("start_moat", 9.0)];
    /// Hidden rooms behind a bookshelf door on an unused socket: 1-2 per map (TKR: one unopened door). The 12x10 and
    /// 13x14 shapes each appear in 4 of 6 maps.
    pub const VAULTS: RangeInclusive<usize> = 1..=2;
    pub const VAULT_WEIGHTS: [(&str, f64, usize); 2] = [("vault", 1.0, 1), ("vault_b", 1.0, 1)];
    /// Largest layout: the five tile-exact maps span 161-237 x 168-238 tiles; larger plans are redrawn.
    pub const MAX_EXTENT: i32 = 240;
    /// The layout's bounding box is centred in this square world.
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
    rows: Vec<String>,
}
impl Template {
    fn size(&self) -> (i32, i32) {
        let w = self.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        (w as i32, self.rows.len() as i32)
    }
}
static LIBRARY: LazyLock<Library> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/manor-templates.json"))
        .expect("data/manor-templates.json is a valid template library")
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
    secret: bool,
}
struct Node {
    name: &'static str,
    t: &'static Template,
    rect: Rect,
    link: Option<Link>,
}
impl Node {
    /// The corridor between this node and its parent (Bricks columns included).
    fn corridor(&self, nodes: &[Node]) -> Option<Rect> {
        let l = self.link.as_ref()?;
        let p = &nodes[l.parent];
        let (side, along) = p.t.ports[l.port];
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
    (0..nodes[p].t.ports.len()).filter(|&i| !used(i)).collect()
}

/// Hang a template from `pool` (with at least `need` ports) off a free port of `p` facing one of `sides`, through a
/// straight 4-wide corridor, keeping `CLEARANCE` from every other template and corridor.
fn attach(
    nodes: &mut Vec<Node>,
    p: usize,
    pool: &[(&'static str, f64, usize)],
    need: usize,
    sides: &str,
    secret: bool,
    rng: &mut StdRng,
) -> Option<usize> {
    use tuning::*;
    let ports: Vec<_> = free(nodes, p)
        .into_iter()
        .map(|i| (i, nodes[p].t.ports[i]))
        .filter(|(_, (side, _))| sides.contains(*side))
        .collect();
    let fits = |t: &Template, side: char| {
        t.ports.len() >= need && t.ports.iter().any(|q| q.0 == opposite(side))
    };
    // The L-room is seen turned both ways (footage, TKR): a draw may place either orientation.
    let twin = |name: &str| match name {
        "l_room" => Some("l_room_b"),
        "l_room_b" => Some("l_room"),
        _ => None,
    };
    let fit = |name: &str| ports.iter().any(|q| fits(&LIBRARY.templates[name], q.1.0));
    // Template first, among those that fit a free port, so weights hold; then a few placements of it (or its twin).
    for _ in 0..16 {
        let usable: Vec<_> = pool
            .iter()
            .filter(|e| nodes.iter().filter(|n| n.name == e.0).count() < e.2)
            .filter(|e| fit(e.0) || twin(e.0).is_some_and(fit))
            .collect();
        let &&(picked, _, _) = usable.choose_weighted(rng, |e| e.1).ok()?;
        for name in std::iter::once(picked).chain(twin(picked)) {
            let Some(&(_, _, cap)) = pool.iter().find(|e| e.0 == name) else {
                continue;
            };
            if nodes.iter().filter(|n| n.name == name).count() >= cap {
                continue;
            }
            let t = &LIBRARY.templates[name];
            let fitting: Vec<_> = ports.iter().filter(|q| fits(t, q.1.0)).collect();
            for _ in 0..8 {
                let Some(&&(port, (side, along))) = fitting.choose(rng) else {
                    break;
                };
                let mine: Vec<_> = (0..t.ports.len())
                    .filter(|&j| t.ports[j].0 == opposite(side))
                    .collect();
                let &own = mine.choose(rng)?;
                let len = rng.gen_range(CORRIDOR);
                let (w, h) = t.size();
                let (tx, ty) = nodes[p].rect.at(side, along, len + 1);
                let (ax, ay) = Rect { x: 0, y: 0, w, h }.at(opposite(side), t.ports[own].1, 0);
                let rect = Rect {
                    x: tx - ax,
                    y: ty - ay,
                    w,
                    h,
                };
                let node = Node {
                    name,
                    t,
                    rect,
                    link: Some(Link {
                        parent: p,
                        port,
                        own,
                        len,
                        secret,
                    }),
                };
                let hall = node.corridor(nodes)?;
                let crowded = nodes.iter().any(|n| {
                    n.rect.grow(CLEARANCE).overlaps(rect)
                        || n.rect.overlaps(hall)
                        || n.corridor(nodes)
                            .is_some_and(|c| c.overlaps(hall) || c.overlaps(rect.grow(CLEARANCE)))
                });
                if !crowded {
                    nodes.push(node);
                    return Some(nodes.len() - 1);
                }
            }
        }
    }
    None
}

/// Grow the template tree: start, nine path rooms, the boss on a north port of the last, side chains, vaults.
fn plan(rng: &mut StdRng, name: &'static str) -> Option<(Vec<Node>, Vec<usize>)> {
    use tuning::*;
    const ANY: &str = "NESW";
    let t = &LIBRARY.templates[name];
    let (w, h) = t.size();
    let mut nodes = vec![Node {
        name,
        t,
        rect: Rect { x: 0, y: 0, w, h },
        link: None,
    }];
    let mut path = vec![0];
    for _ in 0..PATH_ROOMS {
        path.push(attach(
            &mut nodes,
            *path.last()?,
            &ROOM_WEIGHTS,
            2,
            ANY,
            false,
            rng,
        )?);
    }
    path.push(attach(
        &mut nodes,
        *path.last()?,
        &[("boss", 1.0, 1)],
        1,
        "N",
        false,
        rng,
    )?);
    // Split the branch rooms into chains; a chain may fork off an earlier room of its own.
    let rooms = rng.gen_range(BRANCH_ROOMS);
    let mut cuts = vec![0, rooms];
    while cuts.windows(2).any(|w| w[1] - w[0] > BRANCH_MAX) {
        cuts = (1..rooms).collect();
        cuts.shuffle(rng);
        cuts.truncate(rng.gen_range(BRANCHES) - 1);
        cuts.extend([0, rooms]);
        cuts.sort();
    }
    for len in cuts.windows(2).map(|w| w[1] - w[0]) {
        let roots: Vec<_> = path[..=PATH_ROOMS]
            .iter()
            .copied()
            .filter(|&p| !free(&nodes, p).is_empty())
            .collect();
        let mut branch = vec![*roots.choose(rng)?];
        for k in 0..len {
            let need = if k + 1 < len { 2 } else { 1 };
            let open: Vec<_> = branch[1..]
                .iter()
                .copied()
                .filter(|&b| !free(&nodes, b).is_empty())
                .collect();
            let at = match open.choose(rng) {
                Some(&b) if rng.gen_bool(FORK) => b,
                _ => *branch.last()?,
            };
            branch.push(attach(
                &mut nodes,
                at,
                &ROOM_WEIGHTS,
                need,
                ANY,
                false,
                rng,
            )?);
        }
    }
    // Hidden rooms hang north or west of a room (their doors face S or E; TKR's shelf door is on a north wall).
    for _ in 0..rng.gen_range(VAULTS) {
        for _ in 0..8 {
            let hosts: Vec<_> = (1..nodes.len())
                .filter(|&i| nodes[i].name != "boss" && !nodes[i].name.starts_with("vault"))
                .filter(|&i| {
                    free(&nodes, i)
                        .iter()
                        .any(|&q| "NW".contains(nodes[i].t.ports[q].0))
                })
                .collect();
            let &host = hosts.choose(rng)?;
            if attach(&mut nodes, host, &VAULT_WEIGHTS, 1, "NW", true, rng).is_some() {
                break;
            }
        }
    }
    let (x0, y0, x1, y1) = bounds(&nodes);
    (x1 - x0 <= MAX_EXTENT && y1 - y0 <= MAX_EXTENT).then_some((nodes, path))
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

// Templates, sockets and corridors follow one live 2012-kit instance (TKR capture, data/README.md); the JP wiki
// gives the nine path rooms, the hidden rooms behind bookshelves and the hollow-shelf coffin/Nosferatu:
// https://wikiwiki.jp/rmd/Manor%20of%20the%20Immortals
fn build(content: &Content, seed: u64) -> Result<(Map, Vec<Node>, Vec<usize>)> {
    use tuning::*;
    const EMPTY: u8 = 0xff;
    const THRESHOLD: u8 = 0x6c;
    const CORRIDOR_FLOOR: u8 = 0xce;
    const BRICKS: u16 = 0x180a;
    const PILLAR: u16 = 0x180b;
    const SECRET: u8 = 250;
    let mut rng = StdRng::seed_from_u64(seed);
    // The start is drawn once, so redrawn plans keep the start weights.
    let &(start, _) = START_WEIGHTS.choose_weighted(&mut rng, |e| e.1)?;
    let (attempts, (mut nodes, path)) = (1..=1000)
        .find_map(|n| plan(&mut rng, start).map(|p| (n, p)))
        .context("Manor template tree did not fit in 1000 attempts")?;
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
            .with_context(|| format!("unknown Manor ground {name}"))
    };
    let object = |name: &str| {
        content
            .objects
            .values()
            .find(|o| o.name == name)
            .map(|o| o.kind)
            .with_context(|| format!("unknown Manor object {name}"))
    };
    let tiles = LIBRARY
        .legend
        .iter()
        .map(|(&c, (g, o))| {
            let g = if g.is_empty() { EMPTY } else { ground(g)? };
            let kind = match o.as_str() {
                "" => None,
                "spawn" => Some(0x703),
                "secret shelf" => Some(BOOKCASE),
                o => Some(object(o)?),
            };
            Ok((c, (g, kind, o == "secret shelf")))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
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
    let mut floors = vec![vec![]; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        for (y, row) in n.t.rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate().filter(|(_, c)| *c != ' ') {
                let p = (n.rect.x + x as i32, n.rect.y + y as i32);
                let &(ground, object, secret) = tiles
                    .get(&c)
                    .context("Manor template uses an unknown legend character")?;
                map.cells[at(p)] = Cell {
                    ground,
                    object,
                    region: if secret { SECRET } else { 0 },
                    ..blank.clone()
                };
                if object == Some(0x703) {
                    map.cells[at(p)].region = 1;
                    map.spawns.push((p.0 as f32 + 0.5, p.1 as f32 + 0.5));
                } else if object.is_none() && ground != THRESHOLD && content.grounds[&ground] > 0. {
                    floors[i].push(p);
                }
            }
        }
    }
    // Used sockets open as Pillar jambs around 4 Big Tile; corridors are 4 Slabs 2 between Bricks. A hidden room's
    // door keeps the host's jambs and stands 4 shelves on the threshold (TKR gallery north wall); its passage and
    // door are 2 Big Tile wide (2013-2023 minimaps).
    for n in &nodes {
        let Some(l) = &n.link else { continue };
        let p = &nodes[l.parent];
        let (side, along) = p.t.ports[l.port];
        for k in -1..=4 {
            let mut put = |q: (i32, i32), object: Option<u16>, ground: u8, region: u8| {
                map.cells[at(q)] = Cell {
                    ground,
                    object,
                    region,
                    ..blank.clone()
                };
            };
            let (jamb, inner) = if l.secret {
                (k == 0 || k == 3, k == 1 || k == 2)
            } else {
                (k == -1 || k == 4, (0..4).contains(&k))
            };
            let door = p.rect.at(side, along + k, 0);
            let own = n.rect.at(opposite(side), n.t.ports[l.own].1 + k, 0);
            if !l.secret {
                put(
                    door,
                    jamb.then_some(PILLAR),
                    if jamb { EMPTY } else { THRESHOLD },
                    0,
                );
            } else if (0..4).contains(&k) {
                put(door, Some(BOOKCASE), THRESHOLD, SECRET);
            }
            if jamb || inner {
                put(
                    own,
                    jamb.then_some(PILLAR),
                    if jamb { EMPTY } else { THRESHOLD },
                    0,
                );
            }
            for out in 1..=l.len {
                let q = p.rect.at(side, along + k, out);
                if inner {
                    put(
                        q,
                        None,
                        if l.secret { THRESHOLD } else { CORRIDOR_FLOOR },
                        0,
                    );
                } else if jamb || !l.secret {
                    put(q, Some(BRICKS), EMPTY, 0);
                }
            }
        }
    }
    for c in &mut map.cells {
        if c.object == Some(BOOKCASE) && c.region != SECRET {
            c.config = format!(
                "eff:{}",
                crate::condition::bit(crate::condition::INVINCIBLE)
            );
        }
    }
    // One pack per room, as before; the first room past the start holds Hellhounds (Bleeding within 12-25 s in all
    // three 2012 runs; seven Hellhounds in TKR's first room).
    for (i, n) in nodes.iter().enumerate() {
        if i == 0 || n.name == "boss" || n.name.starts_with("vault") {
            continue;
        }
        let family = if i == path[1] {
            0x172b
        } else if rng.gen_bool(0.3) {
            0x1727
        } else {
            *[0x1728, 0x1729, 0x172b, 0x172c].choose(&mut rng).unwrap()
        };
        for p in floors[i].choose_multiple(&mut rng, 4) {
            map.cells[at(*p)].object = Some(family);
        }
    }
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Manor ground {:#x}",
            c.ground
        );
        if let Some(k) = c.object {
            content.get(k)?;
        }
    }
    tracing::info!(
        seed,
        attempts,
        start = nodes[0].name,
        rooms = nodes.len(),
        vaults = nodes.iter().filter(|n| n.name == "vault").count(),
        width = x1 - x0,
        height = y1 - y0,
        "generated Manor template tree"
    );
    Ok((map, nodes, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Cells reachable from spawn; destructible shelves count as passable.
    fn reach(content: &Content, map: &Map) -> Vec<bool> {
        let walk = |c: &Cell| {
            content.grounds[&c.ground] > 0.
                && c.object
                    .is_none_or(|k| !content.objects[&k].blocks || content.objects[&k].enemy)
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
    fn manor_tree_follows_the_capture_rules() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, nodes, path) = build(&content, seed).unwrap();
            // Nine rooms between start and boss, the boss hanging off a north-facing port (entered from the south).
            assert_eq!(path.len(), tuning::PATH_ROOMS + 2, "seed {seed}");
            let boss = *path.last().unwrap();
            let l = nodes[boss].link.as_ref().unwrap();
            assert!(nodes[boss].name == "boss" && nodes[l.parent].t.ports[l.port].0 == 'N');
            for w in path.windows(2) {
                assert_eq!(
                    nodes[w[1]].link.as_ref().unwrap().parent,
                    w[0],
                    "seed {seed}"
                );
            }
            // A tree without overlaps: every node but the start hangs off an earlier one.
            for (i, n) in nodes.iter().enumerate().skip(1) {
                assert!(n.link.as_ref().unwrap().parent < i);
                for o in &nodes[..i] {
                    assert!(!o.rect.overlaps(n.rect), "seed {seed}: overlap");
                }
            }
            // Every floor cell is reachable; the walk from spawn reaches Ruthven.
            let all = reach(&content, &map);
            let walkable = |c: &Cell| {
                content.grounds[&c.ground] > 0.
                    && c.object.is_none_or(|k| !content.objects[&k].blocks)
            };
            assert!(
                map.cells.iter().zip(&all).all(|(c, r)| *r || !walkable(c)),
                "seed {seed}: unreachable floor"
            );
            let ruthven = map
                .cells
                .iter()
                .position(|c| c.object == Some(BOSS))
                .unwrap();
            assert!(all[ruthven], "seed {seed}");
        }
    }
}
