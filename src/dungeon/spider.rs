use crate::{
    content::{Cell, Content, Map},
    encounter::{Motion, Profile},
};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::{collections::BTreeMap, sync::LazyLock};

pub const BOSS: u16 = 0x936;
pub const EGG: u16 = 0x21b;
pub const HATCHLING: u16 = 0x21c;
// 2012 web threads are dashed with a 1.40-tile period at 7 tiles/s: one volley per 200 ms (8Uv7XGMabZI 2012-05-27, oMP3hUIl3qI 2012-11-13);
// the Japanese spoke page (last modified 2012-06-26) also says every 0.2 s.
pub const WEB_INTERVAL: u64 = 200;
/// Arachna wakes with a player within 10 tiles (one player stood 11.6 tiles away for 1.5 s without waking her).
pub const WAKE: f32 = 10.;
/// The spokes land 1500 ms after the toss; she stays invulnerable, still and silent until ≈3.0 s (2.55-3.25 s measured, 3 videos).
pub const SPOKE_MS: u64 = 1500;
pub const ATTACK_MS: u64 = 3000;
/// The web disappears 200 s after it is placed (Japanese spoke page, last modified 2012-06-26; dated comments 2012-03-06 and 2012-11-12).
pub const WEB_LIFETIME_MS: u64 = 200_000;
/// Engine speed for a measured ground speed: an NPC moves `speed / 1.5` tiles per second.
const fn tiles_per_s(measured: f32) -> f32 {
    measured * 1.5
}
const WALL: u16 = 0x1e8;
const WEB: u16 = 0x732;
pub const ROSTER: [u16; 16] = [
    EGG, HATCHLING, 0x21d, 0x21e, 0x21f, 0x220, BOSS, 0x937, 0x938, 0x939, 0x93a, 0x93b, 0x93c,
    0x93d, 0x93e, 0x93f,
];

/// Every layout tunable. "Capture" = the live instance behind data/spider-templates.json (data/README.md); "maps" =
/// the capture plus tile-exact RealmEye wiki minimaps of 2018-07-06 (rev 40) and 2020-05-01 (rev 52), and the 2010
/// Wild Shadow wiki "Example Layout" for counts. Radii, gaps and offsets are in tiles from cell centres.
pub mod tuning {
    use std::ops::RangeInclusive;
    /// Rooms strictly between start and arena: 9 in all four pre-rework maps, never a branch.
    pub const ROOMS: usize = 9;
    /// Disc radii in quarter tiles fitted to the 27 middle rooms and 3 start rooms of the three tile-exact maps.
    pub const RADIUS: [i32; 27] = [
        22, 36, 31, 31, 39, 33, 23, 35, 35, 23, 28, 39, 27, 30, 39, 21, 21, 29, 25, 31, 27, 39, 33,
        33, 36, 41, 22,
    ];
    pub const START_RADIUS: [i32; 3] = [26, 25, 21];
    /// The arena template's outline radius (capture: 18.5 for the 37-tile disc).
    pub const ARENA_RADIUS: f32 = 18.5;
    /// Centre distance along the join axis minus both radii, quarter tiles, for the same 27 joins; the arena's three
    /// joins (at its 18.5 outline) all leave a corridor.
    pub const GAP: [i32; 27] = [
        4, 8, -1, -2, 6, 6, 6, 10, 6, 10, 7, 11, 6, 9, 9, 10, 10, 10, 6, 6, 2, 10, 8, 6, 9, 7, 5,
    ];
    pub const ARENA_GAP: [i32; 3] = [9, 8, 4];
    /// The arena corridor's line runs this far beside the arena centre, between the pillars and the rim (maps: 13.5, 15.5
    /// and 12.5).
    /// 2012 footage: 4.5-16.5 tiles in 8 rebuilt layouts.
    pub const ARENA_SIDE: RangeInclusive<i32> = 4..=16;
    /// Corridors run straight along one axis, 6 tiles wide, inside both discs' widths (every join of the three maps).
    pub const CORRIDOR: i32 = 6;
    /// Chance a join turns 90 degrees from the last: 35 of 64 joins in 8 rebuilt 2012 layouts (2012-02-16 … 2012-11-13; the maps gave
    /// 10 of 24); joins never reverse.
    pub const TURN: f64 = 0.55;
    /// Least tiles between the outlines of rooms or corridors that are not joined (maps: 3.3 at the closest).
    pub const CLEARANCE: f32 = 3.;
    /// Largest layout including walls (capture 112 x 78; 2018 and 2020 maps 93 and 92 high); larger plans are redrawn.
    /// One 2012 layout's floor is 121 tiles tall.
    pub const MAX_EXTENT: i32 = 124;
    /// The layout's bounding box is centred in this square world (the capture's canvas).
    pub const WORLD: i32 = 128;
    /// Spider Dirt Hole speckle outside the arena: 174 of 2164 capture cells.
    pub const HOLES: f64 = 0.08;
    /// Wall webs: 31 of the capture's 406 floor cells beside a wall (another 6 webs stand off the wall, where the
    /// 123.5.1 client does not draw them).
    pub const WEBS: f64 = 0.076;
    /// Per-cell dressing of rooms and corridors past the start (capture: 2020 cells): Spider Ground Rocks 89, Spider
    /// Egg Prop 20, Spider Silk 3; live sacs 38 unhatched plus about 11 hatched (48 hatchlings at 2-7 each).
    pub const DRESSING: [(u16, f64); 3] = [(0x1fc, 0.044), (0x1fe, 0.0099), (0x1800, 0.0015)];
    /// Rock, egg-prop and silk density varied from room to room in 2012; the spread is an estimate.
    pub const DRESSING_SPREAD: std::ops::Range<f64> = 0.5..1.5;
    /// Egg sacs stood in rooms only (corridors 0-2 per run, arena none); about 35 % of rooms had none and the rest roughly 3 per 100
    /// room tiles (3-21 sacs), counted on arrival in 2012 footage.
    pub const SAC_FREE_ROOMS: f64 = 0.35;
    pub const SACS: f64 = 0.03;
    /// Adult spiders per middle room (capture rooms in path order); 2012 rooms mix one to three species.
    pub const SPIDERS: [usize; 9] = [2, 4, 2, 0, 0, 9, 10, 12, 4];
    pub const SPECIES: std::ops::RangeInclusive<usize> = 1..=3;
    pub const ADULTS: [u16; 4] = [0x21d, 0x21e, 0x21f, 0x220];
    /// The Portal of Cowardice carries the spawn point, 1-4 tiles from the start-room centre in 2012 footage (capture: 3.5).
    pub const SPAWN_OFFSET: std::ops::RangeInclusive<f32> = 1.0..=4.0;
    /// Each loose arena wall stood in 8 of 8 and 7 of 8 rebuilt 2012 arenas.
    pub const LOOSE_WALL: f64 = 0.9;
    /// Arachna's web spokes as (count, radius) rings from angle 0, world-fixed: outer 11.0, inner 6.5 in all five 2012 fights measured
    /// (Xl8e3X8bGEw 2012-03-03 … oMP3hUIl3qI 2012-11-13).
    pub const SPOKES: [(usize, f32); 2] = [(6, 11.), (3, 6.5)];
}

#[derive(serde::Deserialize)]
struct Library {
    legend: BTreeMap<char, (String, String)>,
    templates: BTreeMap<String, Template>,
}
#[derive(serde::Deserialize)]
struct Template {
    rows: Vec<String>,
}
static LIBRARY: LazyLock<Library> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../data/spider-templates.json"))
        .expect("data/spider-templates.json is a valid template library")
});
/// The arena template's cells as offsets from Arachna's cell.
fn arena() -> Vec<(i32, i32, char)> {
    let rows = &LIBRARY.templates["arena"].rows;
    let cells: Vec<_> = (rows.iter().enumerate())
        .flat_map(|(y, r)| {
            r.chars()
                .enumerate()
                .map(move |(x, c)| (x as i32, y as i32, c))
        })
        .filter(|c| c.2 != ' ')
        .collect();
    let &(ax, ay, _) = cells
        .iter()
        .find(|c| c.2 == 'A')
        .expect("arena has Arachna");
    cells.iter().map(|&(x, y, c)| (x - ax, y - ay, c)).collect()
}

/// A round room: centre cell and radius.
#[derive(Clone, Copy, Debug)]
struct Room {
    x: i32,
    y: i32,
    r: f32,
}
impl Room {
    fn contains(self, (x, y): (i32, i32)) -> bool {
        ((x - self.x).pow(2) + (y - self.y).pow(2)) as f32 <= self.r * self.r
    }
}
/// Inclusive cell bounds of a straight corridor.
#[derive(Clone, Copy, Debug)]
struct Band {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}
impl Band {
    fn gap(self, x: f32, y: f32) -> f32 {
        let d = |v: f32, lo: i32, hi: i32| (lo as f32 - v).max(v - hi as f32).max(0.);
        d(x, self.x0, self.x1).hypot(d(y, self.y0, self.y1))
    }
}
/// Start, nine rooms and the arena; `bands[i]` joins `rooms[i]` to `rooms[i + 1]` heading `dirs[i]` (0 = north).
struct Plan {
    rooms: Vec<Room>,
    bands: Vec<Band>,
    dirs: Vec<usize>,
}
const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// Chain discs: each next room lies along one axis from the last, a 6-wide corridor between them, clear of the rest.
fn plan(rng: &mut StdRng) -> Option<Plan> {
    use tuning::*;
    let quarter = |rng: &mut StdRng, q: &[i32]| *q.choose(rng).unwrap() as f32 / 4.;
    let mut rooms = vec![Room {
        x: 0,
        y: 0,
        r: quarter(rng, &START_RADIUS),
    }];
    let (mut bands, mut dirs) = (vec![], vec![rng.gen_range(0..4)]);
    for i in 0..=ROOMS {
        if i > 0 {
            let last = dirs[i - 1];
            dirs.push(match rng.gen_bool(TURN) {
                true => (last + [1, 3][rng.gen_range(0..2)]) % 4,
                false => last,
            });
        }
        let (a, (dx, dy)) = (rooms[i], DIRS[dirs[i]]);
        let r = if i == ROOMS {
            ARENA_RADIUS
        } else {
            quarter(rng, &RADIUS)
        };
        let gap = quarter(rng, if i == ROOMS { &ARENA_GAP } else { &GAP });
        let along = (a.r + r + gap).round() as i32;
        let half = |r: f32| r as i32 - CORRIDOR / 2;
        let c = rng.gen_range(-half(a.r)..=half(a.r));
        let s = c + match i == ROOMS {
            true => rng.gen_range(ARENA_SIDE) * [1, -1][rng.gen_range(0..2)],
            false => rng.gen_range(-half(r)..=half(r)),
        };
        let b = Room {
            x: a.x + dx * along - dy * s,
            y: a.y + dy * along + dx * s,
            r,
        };
        let ends = [
            (a.x - dy * (c - 3), a.y + dx * (c - 3)),
            (b.x - dy * (c + 2 - s), b.y + dx * (c + 2 - s)),
        ];
        let band = Band {
            x0: ends[0].0.min(ends[1].0),
            y0: ends[0].1.min(ends[1].1),
            x1: ends[0].0.max(ends[1].0),
            y1: ends[0].1.max(ends[1].1),
        };
        // The new disc keeps clear of every earlier room, its corridor of all but the room it leaves.
        let clear =
            |o: &Room| ((b.x - o.x) as f32).hypot((b.y - o.y) as f32) - b.r - o.r >= CLEARANCE;
        let wide = |o: &Room| band.gap(o.x as f32, o.y as f32) >= o.r + CLEARANCE;
        let old: &[Band] = &bands[..bands.len().saturating_sub(1)];
        if !rooms[..i].iter().all(clear)
            || !rooms[..i.saturating_sub(1)].iter().all(wide)
            || old.iter().any(|o| {
                o.gap(b.x as f32, b.y as f32) < b.r + CLEARANCE || band_gap(*o, band) < CLEARANCE
            })
            || bands
                .last()
                .is_some_and(|o: &Band| o.gap(b.x as f32, b.y as f32) <= b.r + 1.)
        {
            return None;
        }
        rooms.push(b);
        bands.push(band);
    }
    let (x0, y0, x1, y1) = bounds(&rooms);
    (x1 - x0 <= MAX_EXTENT && y1 - y0 <= MAX_EXTENT).then_some(Plan { rooms, bands, dirs })
}
fn band_gap(a: Band, b: Band) -> f32 {
    let d = |lo0: i32, hi0: i32, lo1: i32, hi1: i32| (lo1 - hi0).max(lo0 - hi1).max(0) as f32;
    d(a.x0, a.x1, b.x0, b.x1).hypot(d(a.y0, a.y1, b.y0, b.y1))
}
/// Bounding box of the discs and their walls.
fn bounds(rooms: &[Room]) -> (i32, i32, i32, i32) {
    let ext = |f: fn(&Room) -> f32| rooms.iter().map(f).fold(f32::NAN, f32::min).floor() as i32;
    let max = |f: fn(&Room) -> f32| rooms.iter().map(f).fold(f32::NAN, f32::max).ceil() as i32;
    (
        ext(|r| r.x as f32 - r.r) - 1,
        ext(|r| r.y as f32 - r.r) - 1,
        max(|r| r.x as f32 + r.r) + 1,
        max(|r| r.y as f32 + r.r) + 1,
    )
}

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    Ok(build(content, seed)?.0)
}

// The 2012 Wild Shadow guide: "a linear series of underground caverns"; Build 122 added aesthetic eggs and the boss
// chamber's pillars: https://web.archive.org/web/20120224174134id_/http://forums.wildshadow.com/wiki/realm-mad-god/spider-den
fn build(content: &Content, seed: u64) -> Result<(Map, Plan)> {
    use tuning::*;
    const EMPTY: u8 = 0xff;
    const DIRT: u8 = 0x2f;
    const HOLE: u8 = 0x30;
    let mut rng = StdRng::seed_from_u64(seed);
    let (attempts, mut plan) = (1..=1000)
        .find_map(|n| plan(&mut rng).map(|p| (n, p)))
        .context("Spider Den chain did not fit in 1000 attempts")?;
    let (x0, y0, x1, y1) = bounds(&plan.rooms);
    let (dx, dy) = ((WORLD - (x1 - x0)) / 2 - x0, (WORLD - (y1 - y0)) / 2 - y0);
    for r in &mut plan.rooms {
        (r.x, r.y) = (r.x + dx, r.y + dy);
    }
    for b in &mut plan.bands {
        (b.x0, b.y0, b.x1, b.y1) = (b.x0 + dx, b.y0 + dy, b.x1 + dx, b.y1 + dy);
    }
    let lookup = |g: &str, o: &str| -> Result<(u8, Option<u16>)> {
        let ground = match g {
            "" => EMPTY,
            g => {
                *content
                    .ground_names
                    .iter()
                    .find(|(_, n)| *n == g)
                    .with_context(|| format!("unknown Spider Den ground {g}"))?
                    .0
            }
        };
        let object = match o {
            "" | "loose wall" => None,
            o => Some(
                content
                    .objects
                    .values()
                    .find(|v| v.name == o)
                    .with_context(|| format!("unknown Spider Den object {o}"))?
                    .kind,
            ),
        };
        Ok((ground, object))
    };
    let legend = (LIBRARY.legend.iter())
        .map(|(&c, (g, o))| Ok((c, lookup(g, o)?)))
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
    // Room index of every cell dug from the plan (a corridor counts with the room it leads to).
    let mut owner = vec![usize::MAX; map.cells.len()];
    for (i, r) in plan.rooms[..=ROOMS].iter().enumerate() {
        let k = r.r as i32 + 1;
        for p in (r.y - k..=r.y + k).flat_map(|y| (r.x - k..=r.x + k).map(move |x| (x, y))) {
            if r.contains(p) {
                (map.cells[at(p)].ground, owner[at(p)]) = (DIRT, i);
            }
        }
    }
    for (i, b) in plan.bands.iter().enumerate() {
        for p in (b.y0..=b.y1).flat_map(|y| (b.x0..=b.x1).map(move |x| (x, y))) {
            if owner[at(p)] == usize::MAX {
                (map.cells[at(p)].ground, owner[at(p)]) = (DIRT, (i + 1).min(ROOMS));
            }
        }
    }
    // The arena is stamped over everything dug into its outline.
    let boss = plan.rooms[ROOMS + 1];
    for (x, y, c) in arena() {
        let p = (boss.x + x, boss.y + y);
        let (ground, object) = legend[&c];
        let loose = c == '?' && rng.gen_bool(LOOSE_WALL);
        map.cells[at(p)] = Cell {
            ground: if loose { EMPTY } else { ground },
            object: if loose { Some(WALL) } else { object },
            ..blank.clone()
        };
        owner[at(p)] = ROOMS + 1;
    }
    let floor = |c: &Cell| c.ground != EMPTY && c.object != Some(WALL);
    let walls: Vec<_> = (0..map.cells.len() as i32)
        .filter(|&i| {
            map.cells[i as usize].ground == EMPTY && map.cells[i as usize].object.is_none()
        })
        .filter(|&i| {
            let floor_at = |d: i32| map.cells.get((i + d) as usize).is_some_and(floor);
            (-1..=1).any(|y| (-1..=1).any(|x| floor_at(y * WORLD + x)))
        })
        .map(|i| i as usize)
        .collect();
    for i in walls {
        map.cells[i].object = Some(WALL);
    }
    // Dressing outside the arena; the start room gets only wall webs.
    let spread: Vec<f64> = (0..=ROOMS)
        .map(|_| rng.gen_range(DRESSING_SPREAD))
        .collect();
    for (i, &room) in owner.iter().enumerate() {
        if room > ROOMS || !floor(&map.cells[i]) {
            continue;
        }
        if rng.gen_bool(HOLES) {
            map.cells[i].ground = HOLE;
        }
        let walled = [1, -1, WORLD, -WORLD]
            .iter()
            .any(|d| map.cells[(i as i32 + d) as usize].object == Some(WALL));
        map.cells[i].object = if walled && rng.gen_bool(WEBS) {
            Some(WEB)
        } else if room == 0 {
            None
        } else {
            let (roll, mut sum) = (rng.gen_range(0.0..1.0), 0.);
            DRESSING
                .iter()
                .find(|d| {
                    sum += d.1 * spread[room];
                    roll < sum
                })
                .map(|d| d.0)
        };
    }
    for (i, r) in plan.rooms[1..=ROOMS].iter().enumerate() {
        let free: Vec<_> = (0..map.cells.len())
            .filter(|&j| owner[j] == i + 1 && map.cells[j].object.is_none() && floor(&map.cells[j]))
            .filter(|&j| r.contains((j as i32 % WORLD, j as i32 / WORLD)))
            .collect();
        if !rng.gen_bool(SAC_FREE_ROOMS) {
            for &j in &free {
                if rng.gen_bool(SACS) {
                    map.cells[j].object = Some(EGG);
                }
            }
        }
        let free: Vec<_> = free
            .into_iter()
            .filter(|&j| map.cells[j].object.is_none())
            .collect();
        let mixed = rng.gen_range(SPECIES);
        let species: Vec<u16> = ADULTS.choose_multiple(&mut rng, mixed).copied().collect();
        let n = *SPIDERS.choose(&mut rng).unwrap();
        for &j in free.choose_multiple(&mut rng, n) {
            map.cells[j].object = Some(*species.choose(&mut rng).unwrap());
        }
    }
    let start = plan.rooms[0];
    let spots: Vec<_> = (0..map.cells.len())
        .map(|i| (i as i32 % WORLD, i as i32 / WORLD))
        .filter(|&(x, y)| {
            SPAWN_OFFSET.contains(&((x - start.x) as f32).hypot((y - start.y) as f32))
                && map.cells[at((x, y))].object.is_none()
        })
        .collect();
    let &spawn = spots
        .choose(&mut rng)
        .context("Spider Den start has no spawn spot")?;
    (map.cells[at(spawn)].object, map.cells[at(spawn)].region) = (Some(0x703), 1);
    map.spawns
        .push((spawn.0 as f32 + 0.5, spawn.1 as f32 + 0.5));
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Spider Den ground {:#x}",
            c.ground
        );
        if let Some(k) = c.object {
            content.get(k)?;
        }
    }
    tracing::info!(
        seed,
        attempts,
        turns = plan.dirs.windows(2).filter(|w| w[0] != w[1]).count(),
        width = x1 - x0,
        height = y1 - y0,
        "generated Spider Den chain"
    );
    Ok((map, plan))
}

// The modified 7.0 realm-src 1855c017 SpiderDen table supplies behavior, not an authenticated 123.5.1 map.
pub fn initial_condition(kind: u16) -> i32 {
    if kind == BOSS || (0x937..=0x93f).contains(&kind) {
        crate::condition::bit(crate::condition::INVULNERABLE)
    } else {
        0
    }
}

/// 2012 movement (8Uv7XGMabZI 2012-05-27 solo, Build 123.0.0; oMP3hUIl3qI 2012-11-13, Build 123.5.1; 59.6 px per tile).
/// The current table was the 2011 wiki block divided by 10 with Charge's speed and range swapped; the wiki's speeds are about twice tiles/s.
pub fn profile(kind: u16) -> Profile {
    let (motion, speed, interval, range) = match kind {
        EGG => (Motion::Stationary, 0., 0, 0.),
        // Drifts ≈1.4 tiles/s without closing in while a player is within 8 tiles; the wiki's "Chase Range 8" closes to 8 from farther.
        HATCHLING => (
            Motion::Follow {
                sight: 20.,
                stop: 8.,
                idle: tiles_per_s(1.5),
                duration: 0,
                cooldown: 0,
            },
            tiles_per_s(4.),
            1000,
            9.,
        ),
        // Charges lock the player's position and run through it: Black 9-10 tiles/s, Black Spotted 16-20 (range 9, cooldown 2 s per the 2011 wiki).
        0x21d | 0x21f => (
            Motion::Charge {
                sight: 9.,
                cooldown: 2000,
                delay: 0,
                stop: 0.1,
                idle: tiles_per_s(if kind == 0x21d { 4. } else { 3.5 }),
            },
            tiles_per_s(if kind == 0x21d { 10. } else { 18. }),
            400,
            if kind == 0x21d { 3. } else { 9. },
        ),
        // Mostly ≤2 tiles/s; one approach at ≈7 to ≈1.5 tiles (LOW).
        0x21e => (
            Motion::Follow {
                sight: 9.,
                stop: 1.5,
                idle: tiles_per_s(2.),
                duration: 0,
                cooldown: 0,
            },
            tiles_per_s(7.),
            400,
            9.,
        ),
        // Approaches at ≈4 tiles/s and holds about 3 tiles out (wiki "Chase Range 3").
        0x220 => (
            Motion::Follow {
                sight: 9.,
                stop: 3.,
                idle: tiles_per_s(4.),
                duration: 0,
                cooldown: 0,
            },
            tiles_per_s(4.),
            400,
            9.,
        ),
        // She wanders the web at ≈5.5 tiles/s with frequent turns, usually 5-6 tiles from the centre and sometimes beyond 10; she never chases
        // (5 fights; "Arachna does run away from you if you get too close", Wild Shadow forum 2012-08-15).
        BOSS => (
            Motion::WanderHome { radius: 10. },
            tiles_per_s(5.5),
            750,
            20.,
        ),
        0x937..=0x93f => (Motion::Stationary, 0., WEB_INTERVAL, 30.),
        _ => panic!("unregistered Spider Den actor {kind:#x}"),
    };
    Profile {
        motion,
        speed,
        interval,
        range,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spider_den_fires_the_2012_patterns() {
        use crate::encounter::attacks::schedule;
        let degrees = |shots: Vec<(u8, f32)>| -> Vec<i32> {
            let mut out: Vec<i32> = shots
                .iter()
                .map(|s| s.1.to_degrees().rem_euclid(360.).round() as i32 % 360)
                .collect();
            out.sort();
            out
        };
        // Arachna is silent until the attack phase, then rings 12 Green Magic at fixed 0°, 30° … 330°.
        assert!(
            schedule(BOSS, 2700, ATTACK_MS - 50)
                .unwrap()
                .snapshot(1.)
                .is_empty()
        );
        let ring = degrees(
            schedule(BOSS, 2700, ATTACK_MS)
                .unwrap()
                .snapshot(1.)
                .into_iter()
                .filter(|s| s.0 == 0)
                .collect(),
        );
        assert!((0..12).all(|k| ring.contains(&(k * 30))));
        // Brown fires 3 shots 9° apart; the others one shot every 0.4 s.
        assert_eq!(
            degrees(schedule(0x220, 100, 0).unwrap().snapshot(0.)),
            vec![0, 9, 351]
        );
        assert_eq!(schedule(0x21e, 200, 400).unwrap().snapshot(0.).len(), 1);
        // The web: outer spokes at 11.0, inner at 6.5, a volley every 200 ms.
        assert_eq!(tuning::SPOKES, [(6, 11.), (3, 6.5)]);
        assert_eq!(WEB_INTERVAL, 200);
    }
    use std::collections::VecDeque;

    #[test]
    fn spider_chain_follows_the_capture_rules() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for seed in 0..32 {
            let (map, plan) = build(&content, seed).unwrap();
            let cell = |(x, y): (i32, i32)| &map.cells[(y * map.width + x) as usize];
            let floor = |p| cell(p).ground != 0xff && cell(p).object != Some(WALL);
            let rooms = &plan.rooms;
            let inb = |o: &Band, (x, y): (i32, i32)| {
                (o.x0..=o.x1).contains(&x) && (o.y0..=o.y1).contains(&y)
            };
            assert_eq!(rooms.len(), tuning::ROOMS + 2, "seed {seed}");
            // Discs are dug whole; rooms that are not joined keep their distance.
            for (i, r) in rooms[..=tuning::ROOMS].iter().enumerate() {
                let k = r.r as i32;
                let disc =
                    (r.y - k..=r.y + k).flat_map(|y| (r.x - k..=r.x + k).map(move |x| (x, y)));
                assert!(
                    disc.filter(|&p| r.contains(p)).all(floor),
                    "seed {seed}: disc {i}"
                );
                for o in &rooms[..i.saturating_sub(1)] {
                    let d = ((r.x - o.x) as f32).hypot((r.y - o.y) as f32);
                    assert!(d - r.r - o.r >= tuning::CLEARANCE, "seed {seed}: overlap");
                }
            }
            // Straight 6-wide corridors that never reverse, walled where they leave the rooms.
            for (i, b) in plan.bands.iter().enumerate() {
                let (w, h) = (b.x1 - b.x0 + 1, b.y1 - b.y0 + 1);
                let flat = plan.dirs[i] % 2 == 1;
                assert_eq!(if flat { h } else { w }, tuning::CORRIDOR, "seed {seed}");
                let flanks: Vec<_> = match flat {
                    true => (b.x0..=b.x1)
                        .flat_map(|x| [(x, b.y0 - 1), (x, b.y1 + 1)])
                        .collect(),
                    false => (b.y0..=b.y1)
                        .flat_map(|y| [(b.x0 - 1, y), (b.x1 + 1, y)])
                        .collect(),
                };
                // A flank may open into a room's rim or another corridor, never onto new floor.
                let inside = |p: (i32, i32)| {
                    let rim = |r: &Room| Room { r: r.r + 1., ..*r }.contains(p);
                    rooms.iter().any(rim) || plan.bands.iter().any(|o| inb(o, p))
                };
                assert!(
                    flanks.into_iter().all(|p| inside(p) || !floor(p)),
                    "seed {seed}: corridor width"
                );
                assert!(
                    i == 0 || plan.dirs[i] != (plan.dirs[i - 1] + 2) % 4,
                    "seed {seed}"
                );
            }
            // The arena is entered only through its own corridor.
            let boss = rooms[tuning::ROOMS + 1];
            let foot: Vec<_> = arena()
                .iter()
                .map(|&(x, y, _)| (boss.x + x, boss.y + y))
                .collect();
            let last = plan.bands[tuning::ROOMS];
            for &(x, y) in &foot {
                for q in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                    assert!(
                        foot.contains(&q) || !floor(q) || inb(&last, q),
                        "seed {seed}: arena door"
                    );
                }
            }
            // Every floor cell, Arachna included, is reachable from the spawn.
            let walk = |c: &Cell| {
                content.grounds[&c.ground] > 0.
                    && c.object.is_none_or(|k| !content.objects[&k].blocks)
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
            assert!(
                map.cells.iter().zip(&seen).all(|(c, s)| *s || !walk(c)),
                "seed {seed}: unreachable floor"
            );
            assert_eq!(cell((boss.x, boss.y)).object, Some(BOSS), "seed {seed}");
        }
    }
}
