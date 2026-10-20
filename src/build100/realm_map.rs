use crate::content::{Content, Ground, Grounds};
use anyhow::{Context, Result, ensure};
use std::{collections::VecDeque, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Region {
    Sea,
    ShoreSand,
    LowPlains,
    LowForest,
    MidPlains,
    MidForest,
    HighPlains,
    HighForest,
    Mountains,
    Tutorial,
}
impl Region {
    pub fn terrain(self) -> Option<&'static str> {
        match self {
            Self::Sea => None,
            Self::ShoreSand => Some("ShoreSand"),
            Self::LowPlains => Some("LowPlains"),
            Self::LowForest => Some("LowForest"),
            Self::MidPlains => Some("MidPlains"),
            Self::MidForest => Some("MidForest"),
            Self::HighPlains => Some("HighPlains"),
            Self::HighForest => Some("HighForest"),
            Self::Mountains => Some("Mountains"),
            Self::Tutorial => Some("Tutorial"),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Realm,
    Tutorial,
}
impl Kind {
    pub fn dimensions(self) -> (u16, u16) {
        match self {
            Self::Realm => (256, 256),
            Self::Tutorial => (32, 32),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Realm => "Realm of the Mad God",
            Self::Tutorial => "Tutorial",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub ground: u8,
    pub region: Region,
    // Population/decoration placement must leave the shore landing and dry route clear.
    pub reserved: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub kind: i16,
    pub x: u16,
    pub y: u16,
}
impl Placement {
    pub fn position(self) -> (f32, f32) {
        (f32::from(self.x) + 0.5, f32::from(self.y) + 0.5)
    }
}
// Wrap once in Arc at world creation; no tile/object copies are needed during ticks.
pub struct Map {
    pub kind: Kind,
    pub seed: u64,
    pub spawn: (f32, f32),
    tiles: Box<[Tile]>,
    objects: Box<[Placement]>,
    grounds: Arc<Grounds>,
    occupied: Box<[bool]>,
    walls: Box<[bool]>,
}
impl Map {
    pub fn dimensions(&self) -> (u16, u16) {
        self.kind.dimensions()
    }
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }
    pub fn objects(&self) -> &[Placement] {
        &self.objects
    }
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let (w, h) = self.dimensions();
        (x >= 0 && y >= 0 && x < i32::from(w) && y < i32::from(h))
            .then(|| y as usize * usize::from(w) + x as usize)
    }
    pub fn tile(&self, x: i32, y: i32) -> Result<&Tile> {
        self.index(x, y).map(|i| &self.tiles[i]).with_context(|| {
            format!(
                "{:?} seed={} tile ({x},{y}) out of bounds",
                self.kind, self.seed
            )
        })
    }
    pub fn ground(&self, x: i32, y: i32) -> Result<&Ground> {
        self.grounds.get(self.tile(x, y)?.ground)
    }
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| !self.occupied[i])
    }
    fn wall(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_none_or(|i| self.walls[i])
    }
    fn step(&self, from: (f32, f32), to: (f32, f32)) -> bool {
        let (x, y) = to;
        if !self.walkable(x.floor() as i32, y.floor() as i32) {
            return false;
        }
        let cross_x = (from.0 * 2.).floor() != (x * 2.).floor();
        let cross_y = (from.1 * 2.).floor() != (y * 2.).floor();
        let near_x = if x.fract() < 0.5 {
            x.floor() as i32 - 1
        } else {
            x.floor() as i32 + 1
        };
        let near_y = if y.fract() < 0.5 {
            y.floor() as i32 - 1
        } else {
            y.floor() as i32 + 1
        };
        // Player._-IB checks neighboring Wall objects on half-cell crossings, not terrain speed.
        !(cross_x || cross_y)
            || (!self.wall(near_x, near_y)
                && (!cross_x || !self.wall(near_x, y.floor() as i32))
                && (!cross_y || !self.wall(x.floor() as i32, near_y)))
    }
    pub fn can_move(&self, from: (f32, f32), to: (f32, f32)) -> bool {
        if ![from.0, from.1, to.0, to.1].into_iter().all(f32::is_finite)
            || !self.walkable(from.0.floor() as i32, from.1.floor() as i32)
            || !self.walkable(to.0.floor() as i32, to.1.floor() as i32)
        {
            return false;
        }
        let steps = ((to.0 - from.0).hypot(to.1 - from.1) / 0.25).ceil().max(1.) as u32;
        let mut previous = from;
        for n in 1..=steps {
            let t = n as f32 / steps as f32;
            let next = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            if !self.step(previous, next) {
                return false;
            }
            previous = next;
        }
        true
    }
    fn assemble(
        kind: Kind,
        seed: u64,
        spawn: (f32, f32),
        tiles: Vec<Tile>,
        objects: Vec<Placement>,
        content: &Content,
        grounds: Arc<Grounds>,
    ) -> Result<Self> {
        let (w, h) = kind.dimensions();
        let len = usize::from(w) * usize::from(h);
        ensure!(
            tiles.len() == len,
            "{kind:?} seed={seed}: expected {len} tiles, got {}",
            tiles.len()
        );
        let mut occupied = Vec::with_capacity(len);
        let mut walls = vec![false; len];
        for (i, tile) in tiles.iter().enumerate() {
            let ground = grounds
                .get(tile.ground)
                .with_context(|| format!("{kind:?} seed={seed}: tile {i}"))?;
            occupied.push(ground.no_walk);
            let (x, y) = (i % usize::from(w), i / usize::from(w));
            ensure!(
                (tile.region != Region::Sea
                    && x != 0
                    && y != 0
                    && x + 1 != usize::from(w)
                    && y + 1 != usize::from(h))
                    || ground.no_walk,
                "{kind:?} seed={seed}: walkable map boundary at ({x},{y})"
            );
        }
        let mut placed = vec![false; len];
        for p in &objects {
            ensure!(
                p.x < w && p.y < h,
                "{kind:?} seed={seed}: object out of bounds {p:?}"
            );
            let i = usize::from(p.y) * usize::from(w) + usize::from(p.x);
            ensure!(
                !occupied[i] && !placed[i],
                "{kind:?} seed={seed}: blocked or duplicate placement {p:?}"
            );
            let object = content
                .get(p.kind)
                .with_context(|| format!("{kind:?} seed={seed}: placement {p:?}"))?;
            ensure!(
                ["GameObject", "Character", "Wall", "Portal", "Container"]
                    .contains(&object.class.as_str()),
                "{} is not a map object",
                object.name
            );
            placed[i] = true;
            // Square.obj_ is registered by Static, not by FullOccupy or OccupySquare alone.
            occupied[i] = object.is_static && object.occupy_square;
            walls[i] = object.is_static && object.class == "Wall";
        }
        let map = Self {
            kind,
            seed,
            spawn,
            tiles: tiles.into_boxed_slice(),
            objects: objects.into_boxed_slice(),
            grounds,
            occupied: occupied.into_boxed_slice(),
            walls: walls.into_boxed_slice(),
        };
        ensure!(
            [spawn.0, spawn.1].into_iter().all(f32::is_finite)
                && map.walkable(spawn.0.floor() as i32, spawn.1.floor() as i32),
            "{kind:?} seed={seed}: invalid spawn {spawn:?}"
        );
        let reached = map.reachable();
        for (i, tile) in map.tiles.iter().enumerate() {
            if !map.occupied[i] && map.grounds.get(tile.ground)?.damage.is_none() {
                ensure!(
                    reached[i],
                    "{kind:?} seed={seed}: unreachable safe tile ({},{})",
                    i % usize::from(w),
                    i / usize::from(w)
                );
            }
            if tile.reserved {
                let ground = map.grounds.get(tile.ground)?;
                ensure!(
                    !map.occupied[i] && !ground.sink && ground.damage.is_none(),
                    "{kind:?} seed={seed}: unsafe reserved tile {i}"
                );
            }
        }
        ensure!(
            map.ground(spawn.0 as i32, spawn.1 as i32)?.damage.is_none(),
            "{kind:?} seed={seed}: hazardous spawn"
        );
        Ok(map)
    }
    fn reachable(&self) -> Vec<bool> {
        let (w, _) = self.dimensions();
        let mut seen = vec![false; self.tiles.len()];
        let start = self
            .index(self.spawn.0 as i32, self.spawn.1 as i32)
            .expect("validated spawn");
        seen[start] = true;
        let mut queue = VecDeque::from([start]);
        while let Some(i) = queue.pop_front() {
            let (x, y) = ((i % usize::from(w)) as i32, (i / usize::from(w)) as i32);
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if let Some(j) = self.index(nx, ny)
                    && !seen[j]
                    && !self.occupied[j]
                    && self
                        .grounds
                        .get(self.tiles[j].ground)
                        .expect("validated ground")
                        .damage
                        .is_none()
                {
                    seen[j] = true;
                    queue.push_back(j);
                }
            }
        }
        seen
    }
}

fn qualify(grounds: &Grounds, expected: &[(u8, &str)]) -> Result<()> {
    for &(id, name) in expected {
        ensure!(
            grounds.get(id)?.name == name,
            "July ground {id:#x} must be {name}"
        );
    }
    Ok(())
}
fn placement(content: &Content, id: i16, name: &str, x: u16, y: u16) -> Result<Placement> {
    ensure!(
        content.get(id)?.name == name,
        "July object {id:#x} must be {name}"
    );
    Ok(Placement { kind: id, x, y })
}
fn hash(seed: u64, x: u16, y: u16) -> u64 {
    let mut n = seed ^ (u64::from(x) << 32) ^ u64::from(y);
    n = n.wrapping_add(0x9e3779b97f4a7c15);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}
fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
    (p.0 - a.0 - dx * t).hypot(p.1 - a.1 - dy * t)
}

// Approved reconstruction: coherent irregular coast and inland bands, not a recovered generator.
pub fn realm(seed: u64, content: &Content, grounds: Arc<Grounds>) -> Result<Map> {
    qualify(
        &grounds,
        &[
            (0xbc, "Dark Water"),
            (0xbd, "Light Sand"),
            (0x46, "Light Grass"),
            (0x47, "Yellow Grass"),
            (0x48, "Grass"),
            (0x56, "Dark Grass"),
            (0x60, "Rock"),
            (0x64, "Dark Rock"),
            (0x6a, "Snowy Rock"),
            (0x6b, "Ice"),
            (0x70, "Lava"),
            (0x72, "Water"),
            (0x73, "Shallow Water"),
        ],
    )?;
    let phase = (hash(seed, 0, 0) % 6283) as f32 / 1000.;
    let center = |y: f32| 128. + 10. * (y / 31. + phase).sin() + 5. * (y / 13. + phase).sin();
    let extent = |y: f32| {
        (1. - ((y - 128.) / 112.).powi(2)).max(0.).sqrt()
            * (94. + 7. * (y / 17. + phase).sin() + 4. * (y / 9. + phase).sin())
    };
    let spawn = ((center(190.) - extent(190.)).ceil() + 10.5, 190.5);
    let route = [
        spawn,
        (center(190.) - 43., 190.5),
        (center(160.) - 24., 160.5),
        (center(128.) + 0.5, 128.5),
        (center(100.) + 0.5, 100.5),
    ];
    let mut tiles = Vec::with_capacity(256 * 256);
    let mut objects = vec![];
    for y in 0..256u16 {
        for x in 0..256u16 {
            let (fx, fy) = (f32::from(x) + 0.5, f32::from(y) + 0.5);
            let depth = (extent(fy) - (fx - center(fy)).abs())
                .min(fy - 16.)
                .min(240. - fy);
            if depth < 0. {
                tiles.push(Tile {
                    ground: 0xbc,
                    region: Region::Sea,
                    reserved: false,
                });
                continue;
            }
            let spawn_distance = (fx - spawn.0).hypot(fy - spawn.1);
            let reserved = spawn_distance < 10.
                || route
                    .windows(2)
                    .any(|p| segment_distance((fx, fy), p[0], p[1]) < 2.5);
            let forest = (fx / 19. + (fy / 23.).sin() + phase).sin() > 0.;
            let (region, mut ground) = if depth < 6. || spawn_distance < 10. {
                (Region::ShoreSand, 0xbd)
            } else if depth < 22. {
                if forest {
                    (Region::LowForest, 0x48)
                } else {
                    (Region::LowPlains, 0x46)
                }
            } else if depth < 40. {
                if forest {
                    (Region::MidForest, 0x56)
                } else {
                    (Region::MidPlains, 0x47)
                }
            } else if depth < 56. {
                if forest {
                    (Region::HighForest, 0x56)
                } else {
                    (Region::HighPlains, 0x60)
                }
            } else {
                (Region::Mountains, 0x64)
            };
            if !reserved {
                let river = (fx - (center(fy) - 31. + 9. * (fy / 18. + phase).sin())).abs();
                let lake = ((fx - center(151.) - 29.) / 11.).powi(2) + ((fy - 151.) / 8.).powi(2);
                if river < 2.5 || lake < 1. {
                    ground = if river < 1.3 || lake < 0.65 {
                        0x72
                    } else {
                        0x73
                    };
                } else if region == Region::Mountains {
                    let lava =
                        ((fx - center(121.) - 17.) / 9.).powi(2) + ((fy - 121.) / 12.).powi(2);
                    ground = if lava < 1. {
                        0x70
                    } else if fy < 104. {
                        if (fx / 7. + fy / 11. + phase).sin() > 0.55 {
                            0x6b
                        } else {
                            0x6a
                        }
                    } else {
                        ground
                    };
                }
                if x % 4 == 0
                    && y % 4 == 0
                    && hash(seed, x, y).is_multiple_of(5)
                    && !grounds.get(ground)?.sink
                    && ground != 0x6b
                {
                    let (id, name) = match region {
                        Region::Mountains => (0x107, "Rock Grey"),
                        Region::ShoreSand => (0x18e, "Palm Tree 1"),
                        _ => (0x100, "Tree A"),
                    };
                    objects.push(placement(content, id, name, x, y)?);
                }
            }
            tiles.push(Tile {
                ground,
                region,
                reserved,
            });
        }
    }
    Map::assemble(Kind::Realm, seed, spawn, tiles, objects, content, grounds)
}

pub fn tutorial(seed: u64, content: &Content, grounds: Arc<Grounds>) -> Result<Map> {
    qualify(
        &grounds,
        &[(0x00, "Grey Wall"), (0x1c, "Dirt"), (0x46, "Light Grass")],
    )?;
    let mut tiles = vec![
        Tile {
            ground: 0x00,
            region: Region::Tutorial,
            reserved: false
        };
        32 * 32
    ];
    // Wide overlapping rooms leave a bypass: no tutorial action gates access to the exit.
    for (left, top, right, bottom) in [
        (2, 2, 13, 10),
        (10, 7, 24, 18),
        (4, 15, 18, 27),
        (14, 19, 30, 30),
        (17, 14, 23, 24),
    ] {
        for y in top..bottom {
            for x in left..right {
                tiles[y * 32 + x].ground = if y < 15 { 0x1c } else { 0x46 };
            }
        }
    }
    let mut objects = vec![];
    for (id, name, x, y) in [
        (0x816, "WASD Sign", 5, 4),
        (0x817, "QE Sign", 6, 7),
        (0x81b, "Minimap Sign", 9, 5),
        (0x81c, "Hitpoints Sign", 16, 11),
        (0x81d, "Dodge The Balls Sign", 13, 10),
        (0x802, "South Tutorial Gun", 19, 7),
        (0x803, "North Tutorial Gun", 13, 17),
        (0x804, "East Tutorial Gun", 10, 13),
        (0x805, "West Tutorial Gun", 22, 13),
        (0x821, "Shoot Some Chickens Sign", 6, 17),
        (0x800, "Evil Chicken Hole", 8, 21),
        (0x801, "Evil Chicken", 6, 20),
        (0x801, "Evil Chicken", 10, 22),
        (0x807, "Tutorial Loot Generator", 24, 23),
        (0x823, "Drag A Ring Sign", 23, 25),
        (0x825, "Enter The Portal Sign", 26, 28),
        (0x70e, "Glowing Realm Portal", 27, 27),
    ] {
        objects.push(placement(content, id, name, x, y)?);
    }
    Map::assemble(
        Kind::Tutorial,
        seed,
        (4.5, 5.5),
        tiles,
        objects,
        content,
        grounds,
    )
}

#[cfg(test)]
#[path = "realm_map_tests.rs"]
mod tests;
