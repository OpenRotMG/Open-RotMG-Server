// GPL-3.0 reference; data/REALM-SRC-LICENSE: moistosaurus/realm-src@1855c017eab870d13f0b61b1cf0b9e66a01922ff
// wServer/realm/setpieces/{Grove,LichyTemple,Tower,Pyre}.cs; selected client tiles/objects remain authoritative.
use super::*;

pub(super) struct Area {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
impl Area {
    fn distance(&self, p: Pos) -> f32 {
        let dx = (self.x as f32 - p.x)
            .max(p.x - (self.x + self.width) as f32)
            .max(0.);
        let dy = (self.y as f32 - p.y)
            .max(p.y - (self.y + self.height) as f32)
            .max(0.);
        dx.hypot(dy)
    }
    fn separate(&self, other: &Self) -> bool {
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
}
impl Piece {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![0; width * height],
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
        out
    }
    fn build(kind: u16, rng: &mut rand::rngs::StdRng) -> Self {
        let mut p = match kind {
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
            _ => unreachable!("named setpiece kind validated by caller"),
        };
        if matches!(kind, 0x91b | 0x928) {
            for _ in 0..rng.gen_range(0..4) {
                p = p.rotate();
            }
        }
        p.set(p.width / 2, p.height / 2, 1);
        p
    }
    fn tile(kind: u16, mark: u8) -> (u8, Option<u16>) {
        match (kind, mark) {
            (0x91f, 1) => (0x46, None),
            (0x91f, 2) => (0x46, Some(0x194)),
            (0x91b, 1) => (0xad, None),
            (0x91b, 2) => (0xad, Some(0x160)),
            (0x91b, 3) => (0xad, Some(0x162)),
            (0x91b, 4) => (0xad, Some(0x17d)),
            (0x91b, 5) => (0xad, Some(0x182)),
            (0x928, 1) => (0x60, None),
            (0x928, 2) => (0x60, Some(0x120)),
            (0x675, 1) => (0xf2, None),
            _ => unreachable!("setpiece marker has no selected tile"),
        }
    }
}
impl World {
    pub(super) fn place_named_setpiece(
        &mut self,
        kind: u16,
        terrains: &[u8],
        placed: &[Area],
    ) -> Result<(Pos, Area)> {
        ensure!(
            self.players.is_empty() && self.now == 0,
            "setpieces require an unentered Realm"
        );
        let piece = Piece::build(kind, &mut self.rng);
        for mark in piece.cells.iter().copied().filter(|m| *m != 0) {
            let (ground, object) = Piece::tile(kind, mark);
            ensure!(
                self.content.grounds.get(&ground).is_some_and(|s| *s > 0.),
                "setpiece {kind:#x} missing walkable ground {ground:#x}"
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
                    && placed.iter().all(|p| a.separate(p))
                    && self
                        .map
                        .spawns
                        .iter()
                        .all(|(x, y)| a.distance(Pos { x: *x, y: *y }) >= 64.)
                    && protected.iter().all(|p| a.distance(*p) >= 4.)
                    && quest_positions.iter().all(|p| {
                        p.distance(Pos {
                            x: (a.x + a.width / 2) as f32 + 0.5,
                            y: (a.y + a.height / 2) as f32 + 0.5,
                        }) >= 32.
                    })
                    && (a.y..a.y + a.height).all(|y| {
                        (a.x..a.x + a.width).all(|x| {
                            let c = &self.map.cells[(y * self.map.width + x) as usize];
                            c.region == 0
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
                let old: Vec<_> = self.cells[index].iter().copied().collect();
                for id in old {
                    self.remove_entity(id);
                }
                let cell = &mut Arc::make_mut(&mut self.map).cells[index];
                cell.ground = ground;
                cell.object = object;
                cell.config.clear();
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
            x: (area.x + area.width / 2) as f32 + 0.5,
            y: (area.y + area.height / 2) as f32 + 0.5,
        };
        tracing::info!(
            kind,
            x = area.x,
            y = area.y,
            width = area.width,
            height = area.height,
            "named Realm boss setpiece placed"
        );
        Ok((pos, area))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_arenas_preserve_generated_waterways() {
        let content = super::super::tests::world().content;
        for seed in [0, 12351, u64::MAX] {
            let d = Directory::prepared(std::path::Path::new("data"), content.clone(), 4, seed)
                .unwrap();
            let raw = &d.templates["realm-shore"];
            let live = &d.worlds[&-3].map;
            for (i, (before, after)) in raw.cells.iter().zip(&live.cells).enumerate() {
                if matches!(before.ground, 0x72 | 0x73) {
                    assert_eq!(
                        after.ground,
                        before.ground,
                        "seed {seed}: arena erased water at {},{}",
                        i % 512,
                        i / 512
                    );
                }
            }
        }
    }
}
