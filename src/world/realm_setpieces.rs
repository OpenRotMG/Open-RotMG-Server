// GPL-3.0 reference; data/REALM-SRC-LICENSE: moistosaurus/realm-src@1855c017eab870d13f0b61b1cf0b9e66a01922ff
// wServer/realm/setpieces/{Grove,LichyTemple,Tower,Pyre,Castle,LavaFissure,SkullShrine,CubeGod,Pentaract,TempleA,TempleB}.cs.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Area {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}
impl Area {
    pub(super) fn distance(&self, p: Pos) -> f32 {
        let dx = (self.x as f32 - p.x)
            .max(p.x - (self.x + self.width) as f32)
            .max(0.);
        let dy = (self.y as f32 - p.y)
            .max(p.y - (self.y + self.height) as f32)
            .max(0.);
        dx.hypot(dy)
    }
    pub(super) fn separate(&self, other: &Self) -> bool {
        self.x + self.width + 2 <= other.x
            || other.x + other.width + 2 <= self.x
            || self.y + self.height + 2 <= other.y
            || other.y + other.height + 2 <= self.y
    }
}
struct Piece {
    width: usize,
    height: usize,
    cells: Vec<u8>,
    boss: (usize, usize),
}
impl Piece {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![0; width * height],
            boss: (width / 2, height / 2),
        }
    }
    fn set(&mut self, x: usize, y: usize, value: u8) {
        self.cells[y * self.width + x] = value;
    }
    fn rotate(self) -> Self {
        let mut out = Self::new(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(self.height - 1 - y, x, self.cells[y * self.width + x]);
            }
        }
        out.boss = (self.height - 1 - self.boss.1, self.boss.0);
        out
    }
    fn build(kind: u16, rng: &mut rand::rngs::StdRng) -> Self {
        let mut p = match kind {
            crate::dungeon::manor::KAGE => {
                let mut p = Self::new(65, 65);
                for y in 1..64 {
                    for x in 1..64 {
                        let radius = (x as f32 - 32.).hypot(y as f32 - 32.);
                        if radius < 28. + rng.gen_range(0.0..4.) {
                            let mark = if radius > 19. && rng.gen_ratio(1, 7) {
                                3
                            } else if rng.gen_ratio(1, 15) {
                                4
                            } else if rng.gen_ratio(1, 30) {
                                5
                            } else if radius > 24. {
                                2
                            } else {
                                1
                            };
                            p.set(x, y, mark);
                        }
                    }
                }
                for n in 10..55 {
                    p.set(32, n, 6);
                    p.set(n, 32, 6);
                }
                for y in (20..=44).step_by(6) {
                    for x in (20..=44).step_by(6) {
                        p.set(x, y, 7);
                    }
                }
                p.boss = (20 + rng.gen_range(0..5) * 6, 20 + rng.gen_range(0..5) * 6);
                p
            }
            0xdc2 => {
                let mut p = Self::new(60, 60);
                for y in 0..60 {
                    for x in 0..60 {
                        let (dx, dy) = ((x as f32 - 30.).abs() / 30., (y as f32 - 30.).abs() / 30.);
                        if dx + rng.gen_range(0.0..0.3) < 0.9
                            && dy + rng.gen_range(0.0..0.3) < 0.9
                            && !rng.gen_ratio(1, 50)
                        {
                            p.set(
                                x,
                                y,
                                if rng.r#gen::<f32>() < (1. - dx.hypot(dy)).powi(2) {
                                    1
                                } else {
                                    5
                                },
                            );
                        }
                    }
                }
                if rng.gen_bool(0.5) {
                    for n in 0..20 {
                        p.set(17 + n, 17, if n == 19 { 3 } else { 2 });
                        p.set(17 + n, 18, 3);
                        p.set(17, 17 + n, if n == 19 { 3 } else { 2 });
                        if n > 0 {
                            p.set(18, 17 + n, 3);
                        }
                    }
                    for n in 0..19 {
                        p.set(25 + n, 42, 3);
                        p.set(25 + n, 43, if n == 0 { 3 } else { 2 });
                        if n < 18 {
                            p.set(42, 25 + n, 3);
                        }
                        p.set(43, 25 + n, if n == 0 { 3 } else { 2 });
                    }
                    for (x, y) in [
                        (22, 22),
                        (38, 22),
                        (22, 38),
                        (38, 38),
                        (26, 26),
                        (34, 26),
                        (26, 34),
                        (34, 34),
                    ] {
                        p.set(x, y, 4);
                    }
                } else {
                    for n in 0..23 {
                        if (10..=12).contains(&n) {
                            continue;
                        }
                        for t in [16, 17, 37, 38] {
                            p.set(16 + n, t, 3);
                            p.set(t, 16 + n, 3);
                        }
                    }
                    for n in [7, 8, 9, 13, 14, 15] {
                        for t in [15, 39] {
                            p.set(16 + n, t, 2);
                            p.set(t, 16 + n, 2);
                        }
                    }
                    for y in 0..4 {
                        for x in 0..4 {
                            p.set(21 + x * 4, 21 + y * 4, 4);
                        }
                    }
                    p.boss = (27, 27);
                }
                for y in 6..54 {
                    for x in 6..54 {
                        if p.cells[y * 60 + x] == 5
                            && (!(17..=43).contains(&x) || !(17..=43).contains(&y))
                        {
                            let r: f32 = rng.r#gen();
                            if r > 0.33 {
                                p.set(
                                    x,
                                    y,
                                    if r > 0.6 {
                                        6
                                    } else if r > 0.35 {
                                        7
                                    } else {
                                        8
                                    },
                                );
                            }
                        }
                    }
                }
                p
            }
            0x91f => {
                let mut p = Self::new(25, 25);
                let radius = rng.gen_range(20..=25) / 2;
                let mut border = Vec::new();
                for y in 0..25 {
                    for x in 0..25 {
                        let r = (x as f32 - 12.5).hypot(y as f32 - 12.5);
                        if r <= radius as f32 {
                            p.set(x, y, 1);
                            if radius as f32 - r < 1.5 {
                                border.push((x, y));
                            }
                        }
                    }
                }
                border.shuffle(rng);
                for &(x, y) in border.iter().take(border.len().div_ceil(2)) {
                    p.set(x, y, 2);
                }
                p
            }
            0x91b => {
                let mut p = Self::new(25, 26);
                for x in 2..23 {
                    for y in 1..24 {
                        p.set(x, y, u8::from(!rng.gen_ratio(1, 10)));
                    }
                }
                for y in 1..24 {
                    p.set(2, y, 2);
                    p.set(22, y, 2);
                }
                for x in 2..23 {
                    p.set(x, 23, 2);
                }
                for x in 0..3 {
                    for y in 0..3 {
                        p.set(x + 1, y, 2);
                        p.set(x + 21, y, 2);
                    }
                }
                for x in 0..5 {
                    for y in 0..5 {
                        if (x == 0 || x == 4) && (y == 0 || y == 4) {
                            continue;
                        }
                        p.set(x, y + 21, 2);
                        p.set(x + 20, y + 21, 2);
                    }
                }
                for y in 0..6 {
                    p.set(9, 4 + 3 * y, 4);
                    p.set(15, 4 + 3 * y, 4);
                }
                for cell in &mut p.cells {
                    if *cell < 2 {
                        continue;
                    }
                    let n: f32 = rng.r#gen();
                    if n < 0.1 {
                        *cell = 1;
                    } else if n < 0.4 {
                        *cell += 1;
                    }
                }
                p
            }
            0x928 => {
                let mut p = Self::new(27, 27);
                let quarter = [
                    "............XX",
                    "........XXXXXX",
                    "......XXXXXXXX",
                    ".....XXXX=====",
                    "....XXX=======",
                    "...XXX========",
                    "..XXX=========",
                    "..XX==========",
                    ".XXX==========",
                    ".XX===========",
                    ".XX===========",
                    ".XX===========",
                    "XXX===========",
                    "XXX===========",
                ];
                for y in 0..27 {
                    for x in 0..27 {
                        let c = quarter[y.min(26 - y)].as_bytes()[x.min(26 - x)];
                        p.set(
                            x,
                            y,
                            match c {
                                b'X' => 2,
                                b'=' => 1,
                                _ => 0,
                            },
                        );
                    }
                }
                for y in 1..4 {
                    for x in 8..19 {
                        p.set(x, y, 1);
                    }
                }
                for x in 12..15 {
                    p.set(x, 0, 1);
                }
                p
            }
            0x675 => {
                let mut p = Self::new(30, 30);
                for y in 0..30 {
                    for x in 0..30 {
                        let r = (x as f32 - 15.).hypot(y as f32 - 15.) + rng.gen_range(-2.0..2.0);
                        if r <= 10. {
                            p.set(x, y, 1);
                        }
                    }
                }
                p
            }
            0x67d => {
                let mut p = Self::new(31, 40);
                for (ox, oy) in [(0, 0), (18, 0), (0, 27), (18, 27)] {
                    for x in 0..13 {
                        for y in 0..13 {
                            if ((x == 0 || x == 12) && !(3..=9).contains(&y))
                                || ((y == 0 || y == 12) && !(3..=9).contains(&x))
                            {
                                continue;
                            }
                            p.set(ox + x, oy + y, 2);
                        }
                    }
                }
                for x in 3..28 {
                    for y in 3..37 {
                        if !(6..=24).contains(&x) || !(6..=33).contains(&y) {
                            p.set(x, y, 2);
                        }
                    }
                }
                for x in 7..24 {
                    for y in 7..33 {
                        p.set(x, y, u8::from(!rng.gen_ratio(1, 3)));
                    }
                }
                for (ox, oy) in [(3, 3), (21, 3), (3, 30), (21, 30)] {
                    for x in 0..7 {
                        for y in 0..7 {
                            if ((x == 0 || x == 6) && y != 3) || ((y == 0 || y == 6) && x != 3) {
                                continue;
                            }
                            p.set(ox + x, oy + y, 4);
                        }
                    }
                }
                for x in 6..25 {
                    p.set(x, 6, 4);
                    p.set(x, 33, 4);
                }
                for y in 6..34 {
                    p.set(6, y, 4);
                    p.set(24, y, 4);
                }
                for x in 13..18 {
                    for y in 3..7 {
                        p.set(x, y, 6);
                    }
                }
                for c in &mut p.cells {
                    if *c < 2 {
                        continue;
                    }
                    let n: f32 = rng.r#gen();
                    if *c == 6 {
                        if n < 0.4 {
                            *c = 0;
                        }
                    } else if n < 0.1 {
                        *c = 1;
                    } else if n < 0.4 {
                        *c += 1;
                    }
                }
                p.set(15, 27, 7);
                p.boss = (15, 20);
                p
            }
            0x668 => {
                let mut p = Self::new(40, 40);
                for x in 0..40 {
                    let t = x as f32 / 40. * std::f32::consts::PI;
                    let low =
                        ((t - 2. * t.sin() / 5.5) / std::f32::consts::PI * 40.).ceil() as usize;
                    let high = ((t + t.sin() / 5.5) / std::f32::consts::PI * 40.).floor() as usize;
                    for y in low..high {
                        p.set(x, y, if rng.gen_ratio(1, 5) { 2 } else { 1 });
                    }
                }
                p
            }
            0xd56 => {
                let mut p = Self::new(33, 33);
                for x in 0..33 {
                    for y in 0..33 {
                        if (x as f32 - 16.).abs() / 16.5 + rng.gen_range(0.0..0.3) < 0.95
                            && (y as f32 - 16.).abs() / 16.5 + rng.gen_range(0.0..0.3) < 0.95
                        {
                            p.set(x, y, 1);
                        }
                    }
                }
                for _ in 0..2 {
                    for x in 12..21 {
                        for y in 4..29 {
                            p.set(x, y, 2);
                        }
                    }
                    p = p.rotate();
                }
                for _ in 0..2 {
                    for x in 13..20 {
                        for y in 5..28 {
                            p.set(x, y, 4);
                        }
                    }
                    p = p.rotate();
                }
                for _ in 0..4 {
                    for x in 13..20 {
                        for y in 5..7 {
                            p.set(x, y, 3);
                        }
                    }
                    p = p.rotate();
                }
                for _ in 0..4 {
                    for (x, y) in [(13, 7), (19, 7), (13, 10), (19, 10)] {
                        p.set(x, y, if rng.gen_ratio(1, 3) { 6 } else { 5 });
                    }
                    p = p.rotate();
                }
                let seed = rng.r#gen();
                for x in 0..33 {
                    for y in 0..33 {
                        if crate::terrain::noise(seed, x as f64 / 33. * 8., y as f64 / 33. * 8.)
                            < 0.2
                        {
                            p.set(x, y, 0);
                        }
                    }
                }
                p
            }
            0xd59 => Self::new(5, 5),
            0xd5f => {
                let mut p = Self::new(41, 41);
                for n in 0..5 {
                    let a = n as f32 * std::f32::consts::TAU / 5.;
                    let ox = (a.cos() * 15. + 17.) as usize;
                    let oy = (a.sin() * 15. + 17.) as usize;
                    for x in 0..7 {
                        for y in 0..7 {
                            if !(matches!(x, 0 | 6) && matches!(y, 0 | 1 | 5 | 6)
                                || matches!(x, 1 | 5) && matches!(y, 0 | 6))
                            {
                                p.set(ox + x, oy + y, 1);
                            }
                        }
                    }
                    p.set(ox + 3, oy + 3, 2);
                }
                p
            }
            _ => unreachable!("named setpiece kind validated by caller"),
        };
        if matches!(kind, 0x91b | 0x928 | 0x67d | 0x668 | 0xdc2) {
            for _ in 0..rng.gen_range(0..4) {
                p = p.rotate();
            }
        }
        if kind == 0x668 {
            p.boss = (20, 20);
        }
        if kind != 0xd59 {
            p.set(p.boss.0, p.boss.1, if kind == 0x668 { 2 } else { 1 });
        }
        p
    }
    fn tile(kind: u16, mark: u8) -> (u8, Option<u16>) {
        match (kind, mark) {
            (crate::dungeon::manor::KAGE, 1) => (0x57, None),
            (crate::dungeon::manor::KAGE, 2) => (0x56, None),
            (crate::dungeon::manor::KAGE, 3) => (0x57, Some(0x181e)),
            (crate::dungeon::manor::KAGE, 4) => (0x57, Some(0x181b)),
            (crate::dungeon::manor::KAGE, 5) => (0x57, Some(0x181c)),
            (crate::dungeon::manor::KAGE, 6) => (0xf1, None),
            (crate::dungeon::manor::KAGE, 7) => (0x57, Some(crate::dungeon::manor::GRAVE)),
            (0xdc2, 1) => (0xc5, None),
            (0xdc2, 2) => (0xc5, Some(0x1f4)),
            (0xdc2, 3) => (0xc5, Some(0x1f2)),
            (0xdc2, 4) => (0xc5, Some(0x1f3)),
            (0xdc2, 5) => (0x56, None),
            (0xdc2, 6) => (0x56, Some(0xdd4)),
            (0xdc2, 7) => (0x56, Some(0xdd5)),
            (0xdc2, 8) => (0x56, Some(0xdda)),
            (0x91f, 1) => (0x46, None),
            (0x91f, 2) => (0x46, Some(0x194)),
            (0x91b, 1) => (0xad, None),
            (0x91b, 2) => (0xad, Some(0x160)),
            (0x91b, 3) => (0xad, Some(0x162)),
            (0x91b, 4) => (0xad, Some(0x17d)),
            (0x91b, 5) => (0xad, Some(0x182)),
            (0x928, 1) => (0x60, None),
            (0x928, 2) => (0x60, Some(0x120)),
            (0x675 | 0xd5f, 1) | (0xd5f, 2) => (0xf2, None),
            (0x67d, 1 | 7) => (0x60, None),
            (0x67d, 2) => (0x73, None),
            (0x67d, 3) => (0xbc, None),
            (0x67d, 4) => (0x60, Some(0x120)),
            (0x67d, 5) => (0x60, Some(0x122)),
            (0x67d, 6) => (0xcb, None),
            (0x668, 1) => (0xf4, None),
            (0x668, 2) => (0xf4, Some(0x153)),
            (0xd56, 1) => (0x57, None),
            (0xd56, 2) => (0x19, None),
            (0xd56, 3) => (0x18, None),
            (0xd56, 4) => (0xd6, None),
            (0xd56, 5) => (0xd6, Some(0x17d)),
            (0xd56, 6) => (0xd6, Some(0x182)),
            (0xd59, 1) => (0x60, None),
            _ => unreachable!("setpiece marker has no selected tile"),
        }
    }
}
impl World {
    pub(super) fn outside_setpieces(&self, pos: Pos) -> bool {
        self.setpieces.iter().all(|a| a.distance(pos) >= 4.)
    }
    pub(super) fn place_named_setpiece(&mut self, kind: u16, terrains: &[u8]) -> Result<Pos> {
        ensure!(
            self.players.is_empty() && self.now == 0,
            "setpieces require an unentered Realm"
        );
        let piece = Piece::build(kind, &mut self.rng);
        for mark in piece.cells.iter().copied().filter(|m| *m != 0) {
            let (ground, object) = Piece::tile(kind, mark);
            ensure!(
                self.content.grounds.contains_key(&ground),
                "setpiece {kind:#x} missing selected ground {ground:#x}"
            );
            if let Some(object) = object {
                ensure!(
                    self.content.get(object)?.static_object,
                    "setpiece scenery must be static"
                );
            }
        }
        // Reserved camp positions must remain usable on later reproduction/replacement cycles.
        let protected: Vec<_> = self
            .population
            .iter()
            .map(|s| s.pos)
            .chain(
                self.entities
                    .values()
                    .filter(|e| {
                        let o = &self.content.objects[&e.kind];
                        o.enemy || !o.static_object
                    })
                    .map(|e| e.pos),
            )
            .collect();
        let quest_positions: Vec<_> = self
            .entities
            .values()
            .filter_map(|e| e.quest.map(|_| e.pos))
            .collect();
        let mut candidates: Vec<_> = self
            .map
            .cells
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                terrains
                    .contains(&c.terrain)
                    .then_some((i % self.map.width as usize, i / self.map.width as usize))
            })
            .collect();
        candidates.shuffle(&mut self.rng);
        let area = candidates
            .into_iter()
            .map(|(x, y)| Area {
                x: x as i32 - (piece.width / 2) as i32,
                y: y as i32 - (piece.height / 2) as i32,
                width: piece.width as i32,
                height: piece.height as i32,
            })
            .find(|a| {
                a.x > 0
                    && a.y > 0
                    && a.x + a.width < self.map.width
                    && a.y + a.height < self.map.height
                    && (kind != 0xd59
                        || self.walkable(Pos {
                            x: (a.x + a.width / 2) as f32 + 0.5,
                            y: (a.y + a.height / 2) as f32 + 0.5,
                        }))
                    && self.setpieces.iter().all(|p| a.separate(p))
                    && self
                        .map
                        .spawns
                        .iter()
                        .all(|(x, y)| a.distance(Pos { x: *x, y: *y }) >= 64.)
                    && protected.iter().all(|p| a.distance(*p) >= 4.)
                    && quest_positions.iter().all(|p| {
                        p.distance(Pos {
                            x: (a.x + piece.boss.0 as i32) as f32 + 0.5,
                            y: (a.y + piece.boss.1 as i32) as f32 + 0.5,
                        }) >= 32.
                    })
                    && (a.y..a.y + a.height).all(|y| {
                        (a.x..a.x + a.width).all(|x| {
                            let c = &self.map.cells[(y * self.map.width + x) as usize];
                            let untouched = kind == 0xdc2
                                && piece.cells
                                    [(y - a.y) as usize * piece.width + (x - a.x) as usize]
                                    == 0;
                            untouched
                                || c.region == 0
                                    && self.content.grounds[&c.ground] > 0.
                                    && !matches!(c.ground, 0x72 | 0x73)
                        })
                    })
            })
            .with_context(|| {
                format!(
                    "Realm cannot place {kind:#x} setpiece {}x{} in its selected habitat",
                    piece.width, piece.height
                )
            })?;
        for y in 0..piece.height {
            for x in 0..piece.width {
                let mark = piece.cells[y * piece.width + x];
                if mark == 0 {
                    continue;
                }
                let (ground, object) = Piece::tile(kind, mark);
                let pos = Pos {
                    x: area.x as f32 + x as f32 + 0.5,
                    y: area.y as f32 + y as f32 + 0.5,
                };
                let index = pos.y as usize * self.map.width as usize + pos.x as usize;
                let old: Vec<_> = self.cells.ids(index).to_vec();
                for id in old {
                    self.remove_entity(id);
                }
                let cell = &mut Arc::make_mut(&mut self.map).cells[index];
                cell.ground = ground;
                cell.object = object;
                cell.config.clear();
                if mark == 7 && kind == 0x67d || (x, y) == piece.boss && kind == 0x668 {
                    let items = crate::loot::landmark_chest(&self.content, kind, &mut self.rng)?;
                    self.spawn_entity(0x501, pos, items)?;
                }
                if let Some(object) = object {
                    let id = self.spawn_entity(object, pos, vec![])?;
                    if object == 0x194 {
                        self.entities.get_mut(&id).unwrap().size =
                            if self.rng.gen_bool(0.5) { 120 } else { 140 };
                    }
                }
            }
        }
        let pos = Pos {
            x: (area.x + piece.boss.0 as i32) as f32 + 0.5,
            y: (area.y + piece.boss.1 as i32) as f32 + 0.5,
        };
        tracing::info!(
            kind,
            x = area.x,
            y = area.y,
            width = area.width,
            height = area.height,
            "named Realm boss setpiece placed"
        );
        self.setpieces.push(area);
        Ok(pos)
    }
}
