pub mod abyss;
pub mod beach;
pub mod davy;
pub mod jungle;
pub mod lab;
pub mod manor;
pub mod snake;
pub mod spider;
pub mod sprite;
pub mod tomb;
pub mod trench;
pub mod undead;

pub const DUNGEONS: [(u16, &str, &str, u16); 12] = [
    (0x717, "pirate-cave", "Pirate Cave", 0x927),
    (0x718, "snake-pit", "Snake Pit", 0x917),
    (0x70c, "sprite-world", "Sprite World", 0xd06),
    (0x71a, "undead-lair", "Undead Lair", 0xd90),
    (0x71b, "abyss-of-demons", "Abyss of Demons", abyss::MALPHAS),
    (0x730, "ocean-trench", "Ocean Trench", trench::THESSAL),
    (0x719, "spider-den", "Spider Den", spider::BOSS),
    (0x890, "mad-lab", "Mad Lab", lab::BOSS),
    (0x733, "forbidden-jungle", "Forbidden Jungle", jungle::BOSS),
    (
        0x739,
        "manor-of-the-immortals",
        "Manor of the Immortals",
        manor::BOSS,
    ),
    (0x742, "beachzone", "Beachzone", beach::BOSS),
    (0x741, davy::TEMPLATE, davy::NAME, davy::BOSS),
    (
        tomb::PORTAL,
        "tomb-of-the-ancients",
        "Tomb of the Ancients",
        tomb::ANCHOR,
    ),
];

pub fn portal_drop(enemy: u16) -> Option<(u16, f64)> {
    match enemy {
        beach::BUM => Some((0x742, 0.5)),
        trench::HERMIT => Some((0x730, 1.)),
        0xdc2 => Some((0x733, 0.2)),
        0x6db => Some((0x739, 0.5)),
        tomb::SPHINX => Some((tomb::PORTAL, 1.)),
        0x60e..=0x610 => Some((0x717, 0.01)),
        0x654 => Some((0x71b, 0.1)),
        0x62c | 0x637 => Some((0x719, 0.1)),
        // Classic sources are these five leaders, not their followers: https://wikiwiki.jp/rmd/Snake%20Pit
        0x640 | 0x645 | 0x646 | 0x649 | 0x650 => Some((0x718, 0.17)),
        0x653 => Some((0x70c, 0.25)),
        0x659 => Some((0x71a, 0.17)),
        // Flying Brain matches the retained 123.5.0 announcement; 17% is from the later pinned comparator.
        0x657 => Some((0x890, 0.17)),
        _ => None,
    }
}

pub use snake::generate as snake_pit;
#[cfg(test)]
pub(crate) mod tests {
    use crate::content::{Content, Map};
    use std::collections::{BTreeSet, VecDeque};

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
}
