pub mod abyss;
pub mod snake;
pub mod spider;
pub mod sprite;
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
        0x653 => Some((0x70c, 0.25)),
        0x659 => Some((0x71a, 0.17)),
        _ => None,
    }
}

pub use snake::generate as snake_pit;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::content::{Content, Map};
    use rand::SeedableRng;
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
            } else if name == "sprite-world" {
                sprite::generate(&content, 12351).unwrap()
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
    fn snake_generation_is_seeded_connected_and_uses_the_selected_tiles() {
        let content = Content::load(Path::new("data")).unwrap();
        let map = snake_pit(&content, 7).unwrap();
        assert_eq!(
            serde_json::to_vec(&map).unwrap(),
            serde_json::to_vec(&snake_pit(&content, 7).unwrap()).unwrap()
        );
        for seed in 0..64 {
            let map = snake_pit(&content, seed).unwrap();
            let reached = reachable(&map, &content);
            let boss = map
                .cells
                .iter()
                .position(|c| c.object == Some(snake::BOSS))
                .unwrap();
            assert!(reached.contains(&boss), "disconnected boss for seed {seed}");
            let mut floors: BTreeSet<_> = map
                .cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| matches!(cell.ground, 0xca | 0x1e | 0x1f | 0x32))
                .map(|(i, _)| i)
                .collect();
            while let Some(start) = floors.pop_first() {
                let mut room = vec![start];
                let mut queue = VecDeque::from([start]);
                while let Some(i) = queue.pop_front() {
                    for next in [i - 1, i + 1, i - map.width as usize, i + map.width as usize] {
                        if floors.remove(&next) {
                            room.push(next);
                            queue.push_back(next);
                        }
                    }
                }
                let xs: Vec<_> = room.iter().map(|i| *i as i32 % map.width).collect();
                let ys: Vec<_> = room.iter().map(|i| *i as i32 / map.width).collect();
                let size = (
                    xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1,
                    ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1,
                );
                if room.contains(&boss) {
                    assert_eq!(size, (33, 33), "boss arena for seed {seed}");
                } else {
                    assert!(
                        (15..=19).contains(&size.0) && (15..=19).contains(&size.1),
                        "oversized ordinary room {size:?} for seed {seed}"
                    );
                }
            }
            assert!(
                map.cells.iter().all(|cell| {
                    cell.object.is_none_or(|kind| !content.objects[&kind].enemy)
                        || content.grounds[&cell.ground] > 0.
                }),
                "enemy outside room floor for seed {seed}"
            );
            let (bx, by) = (
                (boss as i32 % map.width) as f32 + 0.5,
                (boss as i32 / map.width) as f32 + 0.5,
            );
            for i in 0..64 {
                let angle = i as f32 * std::f32::consts::TAU / 64.;
                let (x, y) = (
                    (bx + 10. * angle.cos()) as i32,
                    (by + 10. * angle.sin()) as i32,
                );
                let cell = &map.cells[(y * map.width + x) as usize];
                assert!(
                    content.grounds[&cell.ground] > 0.
                        && cell.object.is_none_or(|k| !content.objects[&k].blocks),
                    "guardian orbit blocked for seed {seed}"
                );
            }
            assert!(map.cells.iter().filter(|c| c.object == Some(0x172)).count() > 100);
            for ground in [0xca, 0x1e, 0x1f, 0x16, 0x32] {
                assert!(map.cells.iter().any(|c| c.ground == ground));
            }
            assert!(map.cells.iter().filter(|c| c.object == Some(0x173)).count() >= 16);
        }
    }

    #[test]
    fn sprite_worlds_vary_by_seed_and_keep_the_documented_route() {
        let content = Content::load(Path::new("data")).unwrap();
        let mut layouts = BTreeSet::new();
        let mut gods = BTreeSet::new();
        let (mut tiled, mut solid) = (0, 0);
        for seed in 0..48 {
            let map = sprite::generate(&content, seed).unwrap();
            assert_eq!(
                serde_json::to_vec(&map).unwrap(),
                serde_json::to_vec(&sprite::generate(&content, seed).unwrap()).unwrap()
            );
            layouts.insert(serde_json::to_vec(&map).unwrap());
            let reached = reachable(&map, &content);
            let boss = map
                .cells
                .iter()
                .position(|c| c.object == Some(0xd06))
                .unwrap();
            assert!(reached.contains(&boss), "disconnected boss for seed {seed}");
            assert_eq!(map.spawns.len(), 1);
            let entrance = map.spawns[0].1 as i32 * map.width + map.spawns[0].0 as i32;
            assert!(reached.contains(&(entrance as usize)));
            let god_count = map.cells.iter().filter(|c| c.object == Some(0xd05)).count();
            assert!(god_count <= 5, "seed {seed}");
            gods.insert(god_count);
            let mut changes = 0;
            let mut pairs = 0;
            for y in 0..map.height - 1 {
                for x in 0..map.width - 1 {
                    let a = map.cells[(y * map.width + x) as usize].ground;
                    let b = map.cells[(y * map.width + x + 1) as usize].ground;
                    if (0xa0..=0xa4).contains(&a) && (0xa0..=0xa4).contains(&b) {
                        pairs += 1;
                        changes += usize::from(a != b);
                    }
                }
            }
            if changes * 4 > pairs {
                tiled += 1;
            } else {
                solid += 1;
            }
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            assert!(crate::loot::rewards(&content, 0xd06, &mut rng).contains(&0xa4c));
        }
        assert!(layouts.len() > 40, "seeds collapsed to one Sprite World");
        assert!(gods.contains(&0) && gods.contains(&5), "god count {gods:?}");
        assert!(tiled > 0 && solid > 0, "tiled={tiled} solid={solid}");
    }
}
