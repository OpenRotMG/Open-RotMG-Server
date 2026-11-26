pub const ROOTS: [u16; 5] = [0x91f, 0x91b, 0x928, 0x675, 0x678];
pub const EVENT_BOSSES: [u16; 7] = [0xd56, 0xd59, 0xd5f, 0xd50, 0xd54, 0xd61, 0xe37];
pub fn root(kind: u16) -> bool {
    ROOTS.contains(&kind) || EVENT_BOSSES.contains(&kind)
}

/// Character level windows of the quest monsters, as (kind, first level, last level).
///
/// The February 2012 guide documents wandering +/-5 windows, not every boss bound:
/// https://web.archive.org/web/20120225084118id_/http://forums.wildshadow.com/wiki/realm-mad-god/quest-monsters
/// Boss bounds follow the later wiki; the pinned community tables differ.
pub const WINDOWS: &[(u16, u8, u8)] = &[
    // Wandering quest enemies: a quest level of five either side, as the reference states.
    (0x604, 1, 6),  // Scorpion Queen
    (0x953, 1, 7),  // Bandit Leader
    (0x617, 1, 8),  // Hobbit Mage
    (0x6c4, 1, 8),  // Giant Crab
    (0x609, 1, 8),  // Elf Wizard
    (0x61a, 1, 9),  // Undead Hobbit Mage
    (0x60d, 1, 9),  // Sandsman King
    (0x614, 1, 9),  // Goblin Mage
    (0x623, 1, 10), // Desert Werewolf
    (0x61f, 1, 10), // Dwarf King
    (0x955, 1, 10), // Swarm
    (0x950, 1, 11), // Shambling Sludge
    (0x952, 1, 11), // Great Lizard
    (0x947, 3, 20), // Wasp Queen
    (0x945, 3, 20), // Horned Drake
    // Setpiece bosses and Heroes of Oryx.
    (0x925, 1, 11),  // Deathmage
    (0xdc2, 6, 20),  // Great Coil Snake
    (0x91b, 5, 20),  // Lich
    (0x91c, 7, 20),  // Actual Lich
    (0x91f, 5, 20),  // Ent Ancient
    (0x920, 7, 20),  // Actual Ent Ancient
    (0x678, 8, 20),  // Oasis Giant
    (0x675, 9, 20),  // Phoenix Lord
    (0x928, 10, 20), // Ghost King
    (0x92d, 10, 20), // Actual Ghost King
    (0x67d, 10, 20), // Cyclops God
    (0x668, 15, 20), // Red Demon
    (0xd56, 15, 20), // Skull Shrine
    (0xd59, 15, 20), // Cube God
    (0xd5f, 15, 20), // Pentaract
    (0xd54, 15, 20), // Grand Sphinx
    (0xd50, 15, 20), // Lord of the Lost Lands
    (0xd61, 15, 20), // Hermit God
    (0xe37, 15, 20), // Ghost Ship
];
pub fn window(kind: u16) -> (u8, u8) {
    WINDOWS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map_or((1, 20), |(_, min, max)| (*min, *max))
}
pub fn final_root(kind: u16) -> bool {
    matches!(
        kind,
        0x920
            | 0x91c
            | 0x92d
            | 0x6c1
            | 0x678
            | 0xd54
            | 0xd56
            | 0xd59
            | 0xd5f
            | crate::dungeon::trench::HERMIT
            | 0xe37
    )
}
