use crate::content::{Cell, Content, Map};
use anyhow::{Result, ensure};
use rand::{Rng, SeedableRng};

pub mod abyss;
pub mod spider;
pub mod trench;

pub const DUNGEONS: [(u16, &str, &str, u16); 7] = [
    (0x717, "pirate-cave", "Pirate Cave", 0x927),
    (0x718, "snake-pit", "Snake Pit", 0x917),
    (0x70c, "sprite-world", "Sprite World", 0xd06),
    (0x71a, "undead-lair", "Undead Lair", 0xd90),
    (0x71b, "abyss-of-demons", "Abyss of Demons", abyss::MALPHAS),
    (0x730, "ocean-trench", "Ocean Trench", trench::THESSAL),
    (0x719, "spider-den", "Spider Den", spider::BOSS),
];

pub fn portal_drop(enemy: u16) -> Option<(u16, f64)> {
    match enemy {
        trench::HERMIT => Some((0x730, 1.)),
        0x60e..=0x610 => Some((0x717, 0.01)),
        0x651 => Some((0x71b, 0.17)),
        0x62c | 0x637 => Some((0x719, 0.1)),
        0x640 | 0x645 | 0x649 | 0x650 | 0x654 => Some((0x718, 0.17)),
        0x653 => Some((0x70c, 0.11)),
        0x659 => Some((0x71a, 0.17)),
        _ => None,
    }
}

pub fn snake_pit(content: &Content, seed: u64) -> Result<Map> {
    let size = 90;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut map = Map {
        width: size,
        height: size,
        cells: vec![
            Cell {
                ground: 0xff,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            (size * size) as usize
        ],
        spawns: vec![(15.5, 75.5)],
    };
    let rooms = [
        (15, 75),
        (15, 55),
        (35, 55),
        (35, 35),
        (55, 35),
        (55, 15),
        (75, 15),
    ];
    let floor = |map: &mut Map, x: i32, y: i32| {
        map.cells[(y * size + x) as usize].ground = 0x16;
    };
    for (i, (cx, cy)) in rooms.iter().enumerate() {
        let radius = if i == rooms.len() - 1 { 11 } else { 5 };
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                floor(&mut map, x, y);
            }
        }
    }
    for pair in rooms.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        for y in y0.min(y1) - 1..=y0.max(y1) + 1 {
            for x in x0.min(x1) - 1..=x0.max(x1) + 1 {
                floor(&mut map, x, y);
            }
        }
    }
    let mut walls = Vec::new();
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            if map.cells[(y * size + x) as usize].ground == 0xff
                && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                    .iter()
                    .any(|(x, y)| map.cells[(y * size + x) as usize].ground == 0x16)
            {
                walls.push((x, y));
            }
        }
    }
    for (x, y) in walls {
        map.cells[(y * size + x) as usize].object = Some(0x1ed);
    }
    for (i, (cx, cy)) in rooms.iter().enumerate().skip(1).take(rooms.len() - 2) {
        for (j, (dx, dy)) in [(-2, -2), (2, -2), (-2, 2), (2, 2)].iter().enumerate() {
            let kind = [0x223, 0x224, 0x225, 0x226, 0x227, 0x228, 0x229]
                [(i + j + rng.gen_range(0..7)) % 7];
            map.cells[((cy + dy) * size + cx + dx) as usize].object = Some(kind);
        }
        map.cells[((cy + 3) * size + cx) as usize].object = Some(0x173);
    }
    for pair in rooms.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        let (mx, my) = ((x0 + x1) / 2, (y0 + y1) / 2);
        let thickness = if pair[1] == rooms[rooms.len() - 1] {
            1
        } else {
            0
        };
        for t in -thickness..=thickness {
            for side in -1..=1 {
                let (x, y) = if x0 == x1 {
                    (mx + side, my + t)
                } else {
                    (mx + t, my + side)
                };
                map.cells[(y * size + x) as usize].object = Some(0x172);
            }
        }
    }
    for (dx, dy) in [(-6, 0), (6, 0), (0, -6), (0, 6)] {
        for side in -1..=1 {
            let (x, y) = if dx == 0 {
                (75 + side, 15 + dy)
            } else {
                (75 + dx, 15 + side)
            };
            map.cells[(y * size + x) as usize].object = Some(0x172);
        }
    }
    map.cells[(15 * size + 75) as usize].object = Some(0x917);
    let spawn = &mut map.cells[(75 * size + 15) as usize];
    spawn.region = 1;
    spawn.object = Some(0x703);
    for cell in &map.cells {
        ensure!(
            content.grounds.contains_key(&cell.ground),
            "unknown Snake Pit ground"
        );
        if let Some(kind) = cell.object {
            content.get(kind)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        "generated reconstructed Snake Pit layout"
    );
    Ok(map)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        collections::{BTreeSet, VecDeque},
        path::Path,
    };

    pub(crate) fn reachable(map: &Map, content: &Content) -> BTreeSet<usize> {
        let (x, y) = map.spawns[0];
        let start = (y as i32 * map.width + x as i32) as usize;
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i as i32 % map.width, i as i32 / map.width);
            for (x, y) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if x < 0 || y < 0 || x >= map.width || y >= map.height {
                    continue;
                }
                let next = (y * map.width + x) as usize;
                let cell = &map.cells[next];
                if content.grounds[&cell.ground] > 0.
                    && cell.object.is_none_or(|k| {
                        !content.objects[&k].blocks || content.objects[&k].destructible
                    })
                    && seen.insert(next)
                {
                    queue.push_back(next);
                }
            }
        }
        seen
    }

    #[test]
    fn all_dungeons_resolve_selected_portals_and_have_complete_connected_routes() {
        let content = Content::load(Path::new("data")).unwrap();
        for (portal, name, _, boss) in DUNGEONS {
            assert_eq!(content.objects[&portal].class, "Portal");
            let map = if name == "abyss-of-demons" {
                abyss::generate(&content, 12351).unwrap()
            } else if name == "ocean-trench" {
                trench::generate(&content, 12351).unwrap()
            } else if name == "spider-den" {
                spider::generate(&content, 12351).unwrap()
            } else if name == "snake-pit" {
                snake_pit(&content, 12351).unwrap()
            } else {
                Map::load(&Path::new("data").join(format!("{name}.wmap")), &content).unwrap()
            };
            let reached = reachable(&map, &content);
            let bosses: Vec<_> = map
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| c.object == Some(boss))
                .collect();
            assert_eq!(bosses.len(), 1, "{name}");
            assert!(reached.contains(&bosses[0].0), "{name} boss disconnected");
            assert!(
                map.cells
                    .iter()
                    .filter(|c| c.object.is_some_and(|k| content.objects[&k].enemy))
                    .count()
                    > 12,
                "{name} cannot be only a boss room"
            );
        }
    }

    #[test]
    fn snake_generation_is_seeded_and_includes_grates_walls_and_four_cover_walls() {
        let content = Content::load(Path::new("data")).unwrap();
        let map = snake_pit(&content, 7).unwrap();
        assert_eq!(
            serde_json::to_vec(&map).unwrap(),
            serde_json::to_vec(&snake_pit(&content, 7).unwrap()).unwrap()
        );
        assert_ne!(
            serde_json::to_vec(&map).unwrap(),
            serde_json::to_vec(&snake_pit(&content, 8).unwrap()).unwrap()
        );
        assert_eq!(
            map.cells.iter().filter(|c| c.object == Some(0x173)).count(),
            5
        );
        assert_eq!(
            map.cells.iter().filter(|c| c.object == Some(0x172)).count(),
            36
        );
        for (x, y) in [(69, 15), (81, 15), (75, 9), (75, 21)] {
            assert_eq!(map.cells[(y * map.width + x) as usize].object, Some(0x172));
        }
    }
}
