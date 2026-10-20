pub mod abyss;
pub mod jungle;
pub mod manor;
pub mod snake;
pub mod spider;
pub mod sprite;
pub mod trench;
pub mod undead;

pub const DUNGEONS: [(u16, &str, &str, u16); 9] = [
    (0x717, "pirate-cave", "Pirate Cave", 0x927),
    (0x718, "snake-pit", "Snake Pit", 0x917),
    (0x70c, "sprite-world", "Sprite World", 0xd06),
    (0x71a, "undead-lair", "Undead Lair", 0xd90),
    (0x71b, "abyss-of-demons", "Abyss of Demons", abyss::MALPHAS),
    (0x730, "ocean-trench", "Ocean Trench", trench::THESSAL),
    (0x719, "spider-den", "Spider Den", spider::BOSS),
    (0x733, "forbidden-jungle", "Forbidden Jungle", jungle::BOSS),
    (
        0x739,
        "manor-of-the-immortals",
        "Manor of the Immortals",
        manor::BOSS,
    ),
];

pub fn portal_drop(enemy: u16) -> Option<(u16, f64)> {
    match enemy {
        trench::HERMIT => Some((0x730, 1.)),
        0xdc2 => Some((0x733, 0.2)),
        0x6db => Some((0x739, 0.5)),
        0x60e..=0x610 => Some((0x717, 0.01)),
        0x654 => Some((0x71b, 0.1)),
        0x62c | 0x637 => Some((0x719, 0.1)),
        0x640 | 0x645 | 0x646 | 0x649 | 0x650 => Some((0x718, 0.17)),
        0x653 => Some((0x70c, 0.25)),
        0x659 => Some((0x71a, 0.17)),
        _ => None,
    }
}

pub use snake::generate as snake_pit;
