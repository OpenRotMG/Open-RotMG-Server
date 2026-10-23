use crate::{content::Content, dungeon::beach};
use anyhow::{Result, ensure};
use rand::{Rng, seq::SliceRandom};
#[derive(Clone, Copy)]
pub(crate) enum Pick {
    Item(u16),
    Tier(u8, &'static [i32]),
}
pub(crate) const WEAPONS: &[i32] = &[1, 2, 3, 8, 17];
pub(crate) const ABILITIES: &[i32] = &[4, 5, 11, 12, 13, 15, 16, 18, 19, 20, 21, 22, 23];
pub(crate) const ARMOR: &[i32] = &[6, 7, 14];
pub(crate) const RINGS: &[i32] = &[9];
fn fauna_private_table(kind: u16) -> &'static [(f64, Pick)] {
    use Pick::Tier;
    match kind {
        0x20d => &[(0.005, Pick::Item(0xa51))],
        0x952 => &[(0.03, Tier(2, RINGS))],
        0x949 => &[(0.03, Tier(6, WEAPONS)), (0.03, Tier(6, ARMOR))],
        _ => &[],
    }
}
fn table(kind: u16) -> &'static [(f64, Pick)] {
    use Pick::{Item, Tier};
    match kind {
        beach::BOSS => &[
            (0.1, Item(beach::DRINKS[0])),
            (0.1, Item(beach::DRINKS[1])),
            (0.1, Item(beach::DRINKS[2])),
            (0.1, Item(beach::DRINKS[3])),
        ],
        crate::dungeon::beach::BUM => &[(1., Item(0xc2e))],
        crate::dungeon::manor::BOSS | crate::dungeon::manor::COFFIN => &[
            (0.5, Item(0xc21)),
            (0.01, Item(0xc1d)),
            (0.01, Item(0xc1f)),
            (0.05, Item(0xc20)),
            (0.005, Item(0xc09)),
            (0.01, Item(0xc22)),
            (0.005, Item(0x722)),
            (0.03, Tier(9, WEAPONS)),
            (0.03, Tier(4, ABILITIES)),
        ],
        crate::dungeon::manor::KAGE => &[
            (0.1, Tier(6, WEAPONS)),
            (0.1, Tier(7, WEAPONS)),
            (0.05, Tier(8, WEAPONS)),
            (0.1, Tier(6, ARMOR)),
            (0.1, Tier(7, ARMOR)),
            (0.05, Tier(8, ARMOR)),
            (0.1, Tier(3, ABILITIES)),
            (0.05, Tier(4, ABILITIES)),
            (0.1, Tier(3, RINGS)),
            (0.05, Tier(4, RINGS)),
            (0.1, Item(0xa22)),
            (0.1, Item(0xa23)),
            (0.005, Item(0x722)),
        ],
        0x1728 => &[(0.01, Item(0xc22)), (0.03, Tier(8, WEAPONS))],
        0x1729 => &[(0.05, Tier(6, WEAPONS))],
        kind if crate::dungeon::manor::ROSTER.contains(&kind)
            || kind == crate::dungeon::manor::BOOKCASE =>
        {
            &[]
        }
        crate::dungeon::davy::BOSS => &[
            (0.2, Tier(3, RINGS)),
            (0.2, Tier(7, ARMOR)),
            (0.2, Tier(8, WEAPONS)),
            (0.1, Tier(4, ABILITIES)),
            (0.1, Tier(8, ARMOR)),
            (0.05, Tier(4, RINGS)),
            (0.03, Tier(9, ARMOR)),
            (0.03, Tier(5, ABILITIES)),
            (0.03, Tier(9, WEAPONS)),
            (0.02, Tier(10, ARMOR)),
            (0.02, Tier(10, WEAPONS)),
            (0.01, Tier(11, ARMOR)),
            (0.01, Tier(11, WEAPONS)),
            (0.01, Tier(5, RINGS)),
            (0.01, Item(0xc29)),
            (0.01, Item(0xc28)),
            (0.01, Item(0xc2a)),
            (0.01, Item(0xc27)),
            (0.01, Item(0x722)),
            (0.5, Item(0xa35)),
        ],
        0xe31 | 0xe3a | 0xe42 => &[
            (0.03, Item(0xa22)),
            (0.03, Item(0xa23)),
            (0.01, Item(0xc25)),
        ],
        0xe43 => &[
            (0.05, Tier(7, WEAPONS)),
            (0.05, Tier(7, ARMOR)),
            (0.02, Tier(3, RINGS)),
            (0.03, Item(0xa23)),
            (0.01, Item(0xc25)),
        ],
        0xe44 => &[
            (0.05, Tier(7, WEAPONS)),
            (0.03, Tier(8, WEAPONS)),
            (0.05, Tier(7, ARMOR)),
            (0.02, Tier(3, RINGS)),
            (0.03, Item(0xa23)),
        ],
        0xe45 => &[
            (0.03, Tier(4, RINGS)),
            (0.05, Tier(8, WEAPONS)),
            (0.05, Tier(7, ARMOR)),
            (0.03, Tier(3, ABILITIES)),
            (0.03, Tier(3, RINGS)),
            (0.03, Item(0xa23)),
            (0.01, Item(0xc25)),
        ],
        kind if crate::dungeon::davy::ROSTER.contains(&kind) || kind == 0x182e => &[],
        // Trapped's ordered boss pool is reconstruction evidence, not official 123.5.1 RNG.
        crate::dungeon::tomb::BES => &[
            (0.009, Item(0xba2)),
            (0.005, Item(0xc1e)),
            (0.005, Item(crate::endgame::INCANTATION)),
            (1., Item(0xae9)),
        ],
        crate::dungeon::tomb::GEB => &[
            (0.05, Item(0xba1)),
            (0.015, Item(crate::endgame::INCANTATION)),
            (1., Item(0xae9)),
        ],
        crate::dungeon::tomb::NUT => &[
            (0.009, Item(0xba0)),
            (0.005, Item(crate::endgame::INCANTATION)),
            (1., Item(0xae9)),
        ],
        crate::dungeon::tomb::SARCOPHAGUS => &[
            (0.15, Item(0xa22)),
            (0.15, Item(0xa23)),
            (0.15, Item(0xb0e)),
            (0.15, Item(0xb0d)),
            (0.15, Item(0xb0c)),
        ],
        crate::dungeon::tomb::TREASURE => &[
            (0.3, Item(0xa1f)),
            (0.3, Item(0xa21)),
            (0.3, Item(0xa20)),
            (0.08, Tier(9, ARMOR)),
            (0.08, Tier(9, WEAPONS)),
            (0.06, Tier(10, ARMOR)),
            (0.06, Tier(10, WEAPONS)),
            (0.08, Tier(4, RINGS)),
        ],
        0xd2a | 0xd2b => &[(0.2, Item(0xa22)), (0.2, Item(0xa23))],
        0xd2d..=0xd30 => &[(0.3, Item(0xa22)), (0.3, Item(0xa23))],
        0xd10..=0xd1b | 0xd3a..=0xd3c => &[(0.01, Item(0xa22))],
        kind if crate::dungeon::tomb::contains(kind) => &[],
        0x6c4 => &[
            (0.10, Tier(2, WEAPONS)),
            (0.10, Tier(2, ARMOR)),
            (0.10, Tier(1, RINGS)),
            (0.10, Tier(1, ABILITIES)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        crate::dungeon::lab::BOSS => &[
            (0.005, Item(0xc30)),
            (0.005, Item(0xc32)),
            (0.01, Item(0xc31)),
            (0.005, Item(crate::endgame::INCANTATION)),
            (0.2, Tier(8, WEAPONS)),
            (0.15, Tier(9, WEAPONS)),
            (0.1, Tier(10, WEAPONS)),
            (0.05, Tier(11, WEAPONS)),
            (0.2, Tier(8, ARMOR)),
            (0.15, Tier(9, ARMOR)),
            (0.1, Tier(10, ARMOR)),
            (0.05, Tier(11, ARMOR)),
            (0.15, Tier(4, ABILITIES)),
            (0.1, Tier(5, ABILITIES)),
        ],
        kind if crate::dungeon::lab::ROSTER.contains(&kind) => &[],
        crate::dungeon::jungle::BOSS => &[
            (1., Item(0xc1a)),
            (0.4, Item(0xc17)),
            (0.25, Item(0xc15)),
            (0.3, Item(0xc16)),
            (0.4, Item(0xc14)),
            (0.003, Item(0x722)),
        ],
        crate::dungeon::jungle::SHAMAN
        | crate::dungeon::jungle::WARRIOR
        | crate::dungeon::jungle::HUNTER => &[(0.1, Item(0xc1a))],
        kind if crate::dungeon::jungle::ROSTER.contains(&kind) => &[],
        crate::dungeon::spider::BOSS => {
            &[(0.2, Item(0xa16)), (0.2, Item(0x708)), (0.2, Item(0x709))]
        }
        0x21d..=0x220 => &[(0.2, Item(0x707))],
        0x21b | 0x21c | 0x937..=0x93f => &[],
        crate::dungeon::trench::HERMIT => {
            &[(0.1, Item(0xa34)), (0.1, Item(0xa4c)), (0.004, Item(0xc08))]
        }
        0xd62 => &[(0.1, Item(0xa22)), (0.1, Item(0xa23))],
        0x1705 | crate::dungeon::trench::THESSAL => &[
            (0.3, Item(0xc12)),
            (0.01, Item(0xc10)),
            (0.03, Item(0xc1c)),
            (0.02, Item(0x722)),
            (0.04, Item(0xc18)),
            (0.04, Item(0xc13)),
        ],
        0xd63..=0xd65 | 0x1700..=0x1704 | 0x1707..=0x170e => &[],
        0x623 => &[
            (0.1, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.1, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
            (0.05, Tier(1, ABILITIES)),
            (0.05, Tier(1, RINGS)),
        ],
        // Current reconstruction from Trapped's older Midland table, not 123.5.1 rate fidelity.
        0x945 => &[
            (0.10, Tier(5, WEAPONS)),
            (0.10, Tier(5, ARMOR)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.05, Tier(2, ABILITIES)),
        ],
        0x955 => &[
            (0.15, Tier(3, WEAPONS)),
            (0.15, Tier(3, ARMOR)),
            (0.15, Tier(1, ABILITIES)),
            (0.15, Tier(1, RINGS)),
        ],
        // Trapped 593e490 engineering tables, not authenticated 123.5.1 probabilities.
        0x952 => &[(0.05, Tier(5, WEAPONS)), (0.05, Tier(5, ARMOR))],
        0x949 => &[
            (0.10, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.10, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
        ],
        0x205 | 0x20d | 0x20f | 0x202 => &[(0.03, Item(0xa22)), (0.02, Item(0xa23))],
        0x204 => &[
            (0.01, Tier(2, ARMOR)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        // Behavior coverage alone does not establish new drop probabilities.
        kind if crate::encounter::fauna::ROSTER.contains(&kind)
            || crate::encounter::landmarks::ROSTER.contains(&kind) =>
        {
            &[]
        }
        // Tombstone rewards are a no-drop reconstruction; selected XML establishes zero XP, not loot.
        0x181f => &[],
        0x122 | 0x162 | 0x163 | 0xd57 | 0xd58 | 0xd5b..=0xd5f => &[],
        0xd5a => &[(0.05, Item(0xa3b))],
        0xd56 | 0xd59 | 0xd60 => &[
            (0.15, Tier(8, WEAPONS)),
            (0.1, Tier(9, WEAPONS)),
            (0.07, Tier(10, WEAPONS)),
            (0.05, Tier(11, WEAPONS)),
            (0.15, Tier(4, ABILITIES)),
            (0.07, Tier(5, ABILITIES)),
            (0.2, Tier(8, ARMOR)),
            (0.15, Tier(9, ARMOR)),
            (0.1, Tier(10, ARMOR)),
            (0.07, Tier(11, ARMOR)),
            (0.04, Tier(12, ARMOR)),
            (0.15, Tier(3, RINGS)),
            (0.07, Tier(4, RINGS)),
            (0.03, Tier(5, RINGS)),
            (0.1, Item(0xa1f)),
            (0.1, Item(0xa20)),
            (0.1, Item(0xa21)),
            (0.1, Item(0xa34)),
            (0.1, Item(0xa35)),
            (0.1, Item(0xa4c)),
        ],
        0x91b | 0x91f | 0x928 | 0x675 | 0x6c2 | 0x923 => &[],
        0x676 => &[(0.05, Item(0xa23))],
        0x677 => &[(0.05, Item(0xa22))],
        0x65f | 0x660 | 0x661 | 0x922 => &[(0.02, Item(0xa23))],
        0x921 | 0x91d | 0x91e | 0x929 | 0x92a | 0x92b | 0x92c | 0x924 => &[(0.03, Item(0xa23))],
        0x6c1 => &[(0.0001, Item(crate::endgame::INCANTATION))],
        0x920 => &[
            (0.3, Tier(6, WEAPONS)),
            (0.1, Tier(7, WEAPONS)),
            (0.3, Tier(6, ARMOR)),
            (0.1, Tier(7, ARMOR)),
            (0.95, Tier(1, ABILITIES)),
            (0.25, Tier(2, ABILITIES)),
            (0.05, Tier(3, ABILITIES)),
            (0.15, Tier(2, RINGS)),
            (0.04, Tier(3, RINGS)),
            (0.7, Item(0xa22)),
            (0.7, Item(0xa23)),
        ],
        0x91c => &[
            (0.3, Tier(5, WEAPONS)),
            (0.2, Tier(6, WEAPONS)),
            (0.05, Tier(7, WEAPONS)),
            (0.3, Tier(5, ARMOR)),
            (0.2, Tier(6, ARMOR)),
            (0.05, Tier(7, ARMOR)),
            (0.9, Tier(1, ABILITIES)),
            (0.15, Tier(2, ABILITIES)),
            (0.02, Tier(3, ABILITIES)),
            (0.11, Tier(2, RINGS)),
            (0.01, Tier(3, RINGS)),
            (0.4, Item(0xa22)),
            (0.4, Item(0xa23)),
        ],
        0x92d => &[
            (0.3, Tier(7, WEAPONS)),
            (0.1, Tier(8, WEAPONS)),
            (0.3, Tier(7, ARMOR)),
            (0.1, Tier(8, ARMOR)),
            (0.7, Tier(2, ABILITIES)),
            (0.16, Tier(3, ABILITIES)),
            (0.02, Tier(4, ABILITIES)),
            (0.25, Tier(2, RINGS)),
            (0.08, Tier(3, RINGS)),
            (0.7, Item(0xa22)),
            (0.7, Item(0xa23)),
        ],
        0x600 => &[(0.2, Tier(1, WEAPONS)), (0.03, Item(0xa22))],
        0x601 => &[(0.2, Tier(1, ARMOR)), (0.03, Item(0xa22))],
        0x602 | 0x604 => &[(0.03, Item(0xa22)), (0.02, Item(0xa23))],
        0x603 | 0x954 => &[],
        0x6d2..=0x6d7 => &[],
        0x62d | 0x631 => &[(0.03, Tier(5, WEAPONS)), (0.03, Item(0xa22))],
        0x62e => &[(0.03, Tier(5, ARMOR)), (0.03, Item(0xa22))],
        0x62f => &[(0.03, Item(0xa22)), (0.01, Tier(2, RINGS))],
        0x629 | 0x62b => &[(0.03, Item(0xa22))],
        0x62a | 0x647 | 0x648 => &[(0.03, Item(0xa23))],
        0x62c => &[
            (0.2, Tier(3, WEAPONS)),
            (0.1, Tier(4, WEAPONS)),
            (0.05, Tier(5, WEAPONS)),
            (0.2, Tier(3, ARMOR)),
            (0.1, Tier(4, ARMOR)),
            (0.05, Tier(5, ARMOR)),
            (0.05, Tier(2, ABILITIES)),
            (0.03, Item(0xa22)),
            (0.01, Tier(2, RINGS)),
        ],
        0x6c6 => &[
            (0.1, Tier(4, WEAPONS)),
            (0.05, Tier(3, WEAPONS)),
            (0.1, Tier(4, ARMOR)),
            (0.05, Tier(3, ARMOR)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x900 => &[
            (0.04, Item(0xa22)),
            (0.01, Item(0xa23)),
            (0.01, Tier(3, ARMOR)),
        ],
        0x901 => &[
            (0.04, Item(0xa22)),
            (0.01, Item(0xa23)),
            (0.01, Tier(4, ABILITIES)),
        ],
        0x902 => &[
            (0.04, Item(0xa22)),
            (0.01, Item(0xa23)),
            (0.01, Tier(2, RINGS)),
        ],
        0x612 | 0x613 | 0x618 => &[(0.03, Item(0xa22))],
        0x619 => &[(0.03, Item(0xa23))],
        0x953 => &[
            (0.2, Tier(1, WEAPONS)),
            (0.1, Tier(2, WEAPONS)),
            (0.2, Tier(1, ARMOR)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x614 => &[
            (0.2, Tier(3, WEAPONS)),
            (0.2, Tier(3, ARMOR)),
            (0.2, Tier(1, RINGS)),
            (0.2, Tier(1, ABILITIES)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x61a => &[
            (0.2, Tier(2, WEAPONS)),
            (0.1, Tier(3, WEAPONS)),
            (0.2, Tier(2, ARMOR)),
            (0.1, Tier(3, ARMOR)),
            (0.1, Tier(1, RINGS)),
            (0.1, Tier(1, ABILITIES)),
            (0.03, Item(0xa23)),
        ],
        0x60e..=0x610 => &[(0.03, Item(0xa22)), (0.02, Item(0xa23))],
        0x687..=0x689 => &[(0.01, Item(0xa22))],
        0x68a..=0x68e => &[(0.1, Tier(1, ABILITIES))],
        0x68f => &[(0.1, Tier(1, WEAPONS))],
        0x683..=0x686 => &[
            (0.3, Tier(1, ARMOR)),
            (0.3, Tier(1, RINGS)),
            (0.2, Tier(2, WEAPONS)),
            (0.1, Tier(3, WEAPONS)),
            (0.01, Item(0xa50)),
        ],
        0x927 => &[
            (0.01, Item(0xa50)),
            (0.15, Tier(1, ARMOR)),
            (0.5, Tier(1, RINGS)),
            (0.2, Tier(2, ARMOR)),
            (0.2, Tier(2, WEAPONS)),
            (0.25, Tier(3, ARMOR)),
            (0.3, Tier(3, WEAPONS)),
            (0.3, Tier(4, ARMOR)),
            (0.4, Tier(4, WEAPONS)),
        ],
        0x172 | 0x173 | 0x175..=0x179 | 0x653 | 0xd07..=0xd0a | 0xda2 | 0xda3 | 0x918 | 0x919 => {
            &[]
        }
        0xdaa..=0xdaf => &[(0.25, Item(0xa22)), (0.25, Item(0xa23))],
        0x605..=0x607
        | 0x60a
        | 0x60c
        | 0x615
        | 0x616
        | 0x61c..=0x61e
        | 0x624..=0x626
        | 0x63a..=0x63c
        | 0x63f
        | 0x642..=0x644
        | 0x64a..=0x64d
        | 0xd91..=0xd94
        | 0xd96
        | 0xd97
        | 0xd9b..=0xd9f
        | 0xda4..=0xda6
        | 0xdb0..=0xdb2 => &[(0.05, Item(0xa22)), (0.03, Item(0xa23))],
        0x609 | 0x617 | 0x60d => &[
            (0.2, Tier(2, WEAPONS)),
            (0.2, Tier(2, ARMOR)),
            (0.2, Tier(1, RINGS)),
            (0.2, Tier(1, ABILITIES)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x61f | 0x632 | 0x633 | 0x63e => &[
            (0.1, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.1, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
            (0.05, Tier(1, RINGS)),
            (0.05, Tier(1, ABILITIES)),
        ],
        0x640 | 0x645 | 0x646 | 0x649 | 0x64f | 0x650 | 0x905 => &[
            (0.16, Tier(5, WEAPONS)),
            (0.08, Tier(6, WEAPONS)),
            (0.16, Tier(4, ARMOR)),
            (0.08, Tier(5, ARMOR)),
            (0.04, Tier(6, ARMOR)),
            (0.05, Tier(3, RINGS)),
            (0.03, Tier(3, ABILITIES)),
        ],
        0x906 | 0x907 => &[
            (0.04, Item(0xa22)),
            (0.01, Item(0xa23)),
            (0.01, Tier(6, WEAPONS)),
        ],
        0x651..=0x659 | 0xd05 | 0x91a | 0x6d8 => &[
            (0.02, Tier(6, WEAPONS)),
            (0.01, Tier(7, WEAPONS)),
            (0.005, Tier(8, WEAPONS)),
            (0.02, Tier(6, ARMOR)),
            (0.01, Tier(7, ARMOR)),
            (0.005, Tier(8, ARMOR)),
            (0.015, Tier(3, RINGS)),
            (0.02, Tier(4, ABILITIES)),
        ],
        0x223..=0x229 => &[
            (0.1, Item(0xa3f)),
            (0.05, Tier(3, RINGS)),
            (0.05, Tier(6, WEAPONS)),
        ],
        0xd00..=0xd04 => &[(0.02, Tier(5, WEAPONS)), (0.05, Item(0xa23))],
        0xd95 | 0xd98..=0xd9a => &[
            (0.2, Tier(5, ARMOR)),
            (0.2, Tier(6, WEAPONS)),
            (0.1, Tier(7, WEAPONS)),
            (0.1, Tier(6, ARMOR)),
            (0.1, Tier(3, ABILITIES)),
            (0.1, Tier(3, RINGS)),
        ],
        0x917 => &[
            (1., Item(0xa21)),
            (0.05, Item(crate::endgame::INCANTATION)),
            (0.05, Item(0xc04)),
            (0.05, Item(0xa3e)),
            (0.05, Item(0xa40)),
            (0.05, Item(0xa41)),
            (0.2, Tier(9, WEAPONS)),
            (0.3, Tier(8, ARMOR)),
        ],
        0xd06 => &[
            (1., Item(0xa4c)),
            (0.01, Item(0xa4b)),
            (0.01, Item(0xc03)),
            (0.01, Item(0xa5a)),
            (0.15, Tier(7, ARMOR)),
            (0.124, Tier(4, ABILITIES)),
            (0.11, Tier(3, RINGS)),
        ],
        0xd90 => &[
            (1., Item(0xa35)),
            (0.005, Item(crate::endgame::INCANTATION)),
            (0.009, Item(0xc02)),
            (0.3, Tier(7, WEAPONS)),
            (0.2, Tier(8, WEAPONS)),
            (0.2, Tier(4, ABILITIES)),
            (0.3, Tier(3, RINGS)),
        ],
        0x66d => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa59)),
            (0.01, Item(0xaa7)),
        ],
        0x66e => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa1c)),
            (0.01, Item(0xa12)),
        ],
        0x66f => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa3b)),
            (0.01, Item(0xa0a)),
        ],
        0x670 => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa2f)),
            (0.01, Item(0xa3d)),
            (0.01, Item(0xa9d)),
            (0.01, Item(0xa5f)),
            (0.01, Item(0xaae)),
            (0.01, Item(0xab5)),
        ],
        0x671 => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa17)),
            (0.01, Item(0xa6a)),
        ],
        0x672 => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa3c)),
            (0.01, Item(0xa4e)),
            (0.01, Item(0xa64)),
            (0.01, Item(0xa32)),
        ],
        0x908 => &[(0.04, Item(0xa22)), (0.06, Item(0xa23))],
        0x90b => &[(0.1, Item(0xa22)), (0.1, Item(0xa23))],
        0x909 => &[],
        crate::dungeon::abyss::MALPHAS => &[
            (1., Item(0xa34)),
            (0.005, Item(crate::endgame::INCANTATION)),
            (0.001, Item(0xc01)),
            (0.8, Tier(7, WEAPONS)),
            (0.11, Tier(9, WEAPONS)),
            (0.05, Tier(10, WEAPONS)),
            (0.8, Tier(6, ARMOR)),
            (0.4, Tier(8, ARMOR)),
            (0.11, Tier(9, ARMOR)),
            (0.5, Tier(3, ABILITIES)),
            (0.06, Tier(4, ABILITIES)),
            (0.5, Tier(3, RINGS)),
            (0.06, Tier(4, RINGS)),
        ],
        crate::dungeon::abyss::IDOL => &[
            (1., Item(0xa34)),
            (0.005, Item(crate::endgame::INCANTATION)),
            (0.001, Item(0xc01)),
            (0.8, Tier(7, WEAPONS)),
            (0.4, Tier(8, WEAPONS)),
            (0.11, Tier(9, WEAPONS)),
            (0.05, Tier(10, WEAPONS)),
            (0.8, Tier(7, ARMOR)),
            (0.4, Tier(8, ARMOR)),
            (0.11, Tier(9, ARMOR)),
            (0.05, Tier(10, ARMOR)),
            (0.5, Tier(3, ABILITIES)),
            (0.06, Tier(4, ABILITIES)),
            (0.5, Tier(3, RINGS)),
            (0.06, Tier(4, RINGS)),
        ],
        kind if crate::endgame::ROSTER.contains(&kind) => crate::endgame::loot(kind),
        _ => panic!("loot requested without encounter profile"),
    }
}
fn candidates(content: &Content, pick: Pick) -> Vec<u16> {
    match pick {
        Pick::Item(kind) => vec![kind],
        Pick::Tier(tier, slots) => content
            .objects
            .values()
            .filter(|o| o.tier == Some(tier) && slots.contains(&o.slot_type))
            .map(|o| o.kind)
            .collect(),
    }
}
pub fn collectibles(content: &Content) -> std::collections::BTreeSet<u16> {
    content
        .objects
        .keys()
        .copied()
        .filter(|kind| crate::encounter::profile(*kind).is_some())
        .flat_map(|kind| {
            table(kind)
                .iter()
                .chain(fauna_private_table(kind))
                .flat_map(|(_, pick)| candidates(content, *pick))
                .chain(bonus(kind).iter().map(|(_, item)| *item))
        })
        .chain([0x67d, 0x668].into_iter().flat_map(|kind| {
            chest_table(kind)
                .into_iter()
                .flat_map(|(_, pick)| candidates(content, pick))
        }))
        .collect()
}
fn bonus(kind: u16) -> &'static [(f64, u16)] {
    match kind {
        0xd56 => &[
            (0.001, 0xc0b),
            (0.01, 0x121e),
            (0.01, 0x131e),
            (0.01, 0x1241),
            (0.01, 0x1341),
            (0.01, 0x120e),
            (0.01, 0x130e),
        ],
        0xd59 => &[(0.001, 0xc0a)],
        0xd60 => &[(0.004, 0xc06)],
        0x1705 => &[(0.04, 0xaea)],
        0x651 | 0x652 | 0x657 | 0x905..=0x907 => &[(0.015, 0xa1f)],
        0x654 | 0x659 | 0x91a => &[(0.015, 0xa21)],
        0x655 | 0x656 | 0x658 | 0x6d8 => &[(0.015, 0xa20)],
        _ => &[],
    }
}
pub fn validate(content: &Content, kind: u16) -> Result<()> {
    for (_, item) in bonus(kind) {
        let item = content.get(*item)?;
        ensure!(
            matches!(item.class.as_str(), "Equipment" | "Dye"),
            "invalid bonus loot {:#x}: {}",
            item.kind,
            item.class
        );
        if matches!(item.kind, 0xa1f | 0xa20 | 0xa21 | 0xaea) {
            ensure!(
                item.stat_increase.is_some(),
                "bonus potion is not a selected stat increase"
            );
        }
    }
    for (_, pick) in table(kind).iter().chain(fauna_private_table(kind)) {
        let items = candidates(content, *pick);
        ensure!(
            !items.is_empty(),
            "encounter {kind:#x} has an empty selected-XML loot tier"
        );
        for item in items {
            let o = content.get(item)?;
            ensure!(
                o.class == "Equipment" && o.bag_type <= 4,
                "invalid encounter loot {item:#x}"
            );
        }
    }
    Ok(())
}
pub fn roll(content: &Content, kind: u16, rng: &mut impl Rng) -> Option<u16> {
    roll_table(content, table(kind), rng)
}
fn roll_table(content: &Content, picks: &[(f64, Pick)], rng: &mut impl Rng) -> Option<u16> {
    for (chance, pick) in picks {
        if rng.gen_bool(*chance) {
            return Some(
                *candidates(content, *pick)
                    .choose(rng)
                    .expect("validated nonempty loot candidates"),
            );
        }
    }
    None
}
pub fn rewards(content: &Content, kind: u16, rng: &mut impl Rng) -> Vec<u16> {
    let mut items = if kind == crate::dungeon::tomb::TREASURE
        || crate::quest::final_root(kind)
        || matches!(kind, 0x66d..=0x672 | 0x908 | 0x90b)
        || matches!(kind, 0xd60 | 0x1705 | crate::dungeon::trench::THESSAL)
        || matches!(
            kind,
            0x917
                | 0xd06
                | 0xd90
                | 0xd78
                | 0xd79
                | crate::dungeon::spider::BOSS
                | crate::dungeon::lab::BOSS
                | crate::dungeon::jungle::BOSS
                | crate::dungeon::beach::BOSS
                | crate::dungeon::manor::BOSS
                | crate::dungeon::manor::COFFIN
                | crate::dungeon::manor::KAGE
                | crate::dungeon::davy::BOSS
                | crate::dungeon::abyss::MALPHAS
                | crate::dungeon::abyss::IDOL
                | crate::endgame::ORYX
                | crate::endgame::ORYX2
        ) {
        let mut items = Vec::new();
        for (chance, pick) in table(kind) {
            if rng.gen_bool(*chance) {
                items.push(
                    *candidates(content, *pick)
                        .choose(rng)
                        .expect("validated loot candidates"),
                );
            }
        }
        items
    } else {
        roll(content, kind, rng).into_iter().collect()
    };
    for &(chance, item) in bonus(kind) {
        if rng.gen_bool(chance) {
            items.push(item);
        }
    }
    items
}
pub fn fauna_private_rewards(
    content: &Content,
    kind: u16,
    contributors_by_damage: &[(i32, i32)],
    rng: &mut impl Rng,
) -> Vec<(i32, u16)> {
    let multiplier = match kind {
        0x20d | 0x952 => 1,
        0x949 => 2,
        _ => unreachable!("unsupported dual-channel fauna kind {kind:#x}"),
    };
    let owners: Vec<_> = contributors_by_damage
        .iter()
        .filter(|(_, damage)| *damage > 1)
        .map(|(account, _)| *account)
        .collect();
    let mut rewards = Vec::new();
    let mut successes = 0;
    for _ in 0..owners.len() * multiplier {
        if let Some(item) = roll_table(content, fauna_private_table(kind), rng) {
            // Allocation and the cap count successful rolls, including deduplicated items.
            let reward = (owners[successes % owners.len()], item);
            if !rewards.contains(&reward) {
                rewards.push(reward);
            }
            successes += 1;
            if successes == 8 {
                break;
            }
        }
    }
    rewards
}
pub fn private(kind: u16) -> bool {
    crate::dungeon::tomb::BOSSES.contains(&kind)
        || matches!(
            kind,
            crate::dungeon::tomb::TREASURE | crate::dungeon::tomb::SARCOPHAGUS
        )
        || crate::quest::final_root(kind)
        || kind == crate::dungeon::davy::BOSS
        || matches!(
            kind,
            0xd5a | 0xd60 | 0x1705 | crate::dungeon::trench::THESSAL
        )
        || matches!(kind, 0x649 | 0x91a | 0x6d8 | 0x905..=0x907 | 0x683..=0x686 | 0x927 | 0x640 | 0x645 | 0x646 | 0x64f | 0x650..=0x659
        | 0xd05 | 0x917 | 0xd06 | 0xd90 | 0xd95 | 0xd98..=0xd9a
        | crate::dungeon::jungle::BOSS | crate::dungeon::beach::BOSS
        | crate::dungeon::manor::BOSS | crate::dungeon::manor::COFFIN | crate::dungeon::manor::KAGE
        | crate::dungeon::abyss::MALPHAS | crate::dungeon::abyss::IDOL
        | 0x668 | 0x67d | 0xd78 | 0xd79 | 0xd7f | crate::endgame::ORYX | crate::endgame::ORYX2)
}
fn chest_table(kind: u16) -> Vec<(f64, Pick)> {
    use Pick::Tier;
    if kind == crate::dungeon::jungle::COIL {
        return vec![
            (0.3, Tier(4, WEAPONS)),
            (0.2, Tier(5, WEAPONS)),
            (0.3, Tier(4, ARMOR)),
            (0.2, Tier(5, ARMOR)),
            (0.25, Tier(1, ABILITIES)),
            (0.15, Tier(2, ABILITIES)),
            (0.3, Tier(2, RINGS)),
            (0.2, Tier(3, RINGS)),
            (0.5, Tier(1, &[10])),
            (0.5, Tier(1, &[10])),
            (0.5, Tier(1, &[10])),
        ];
    }
    let (weapon, armor) = match kind {
        0x67d => (6, 5),
        0x668 => (7, 6),
        _ => unreachable!("selected landmark chest"),
    };
    [
        (0.3, Tier(weapon, WEAPONS)),
        (0.2, Tier(weapon + 1, WEAPONS)),
        (0.1, Tier(weapon + 2, WEAPONS)),
        (0.3, Tier(armor, ARMOR)),
        (0.2, Tier(armor + 1, ARMOR)),
        (0.1, Tier(armor + 2, ARMOR)),
        (0.3, Tier(2, ABILITIES)),
        (0.2, Tier(3, ABILITIES)),
        (0.1, Tier(4, ABILITIES)),
        (0.25, Tier(2, RINGS)),
        (0.15, Tier(3, RINGS)),
        (0.5, Tier(1, &[10])),
    ]
    .to_vec()
}
pub fn landmark_chest(content: &Content, kind: u16, rng: &mut impl Rng) -> Result<Vec<i32>> {
    let picks = chest_table(kind);
    for (_, pick) in &picks {
        ensure!(
            !candidates(content, *pick).is_empty(),
            "landmark {kind:#x} chest has an empty selected tier"
        );
    }
    let maximum = rng.gen_range(5..8);
    let mut items = Vec::new();
    for (chance, pick) in picks {
        if rng.gen_bool(chance) {
            items.push(i32::from(
                *candidates(content, pick)
                    .choose(rng)
                    .expect("selected landmark chest tier"),
            ));
            if items.len() == maximum {
                break;
            }
        }
    }
    items.resize(8, -1);
    Ok(items)
}
#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    fn approved_abyss_rows(kind: u16) -> Vec<(f64, Vec<u16>)> {
        match kind {
            0x66d => vec![
                (0.03, vec![0xa22]),
                (0.02, vec![0xa23]),
                (0.01, vec![0xa59]),
                (0.01, vec![0xaa7]),
            ],
            0x66e => vec![
                (0.03, vec![0xa22]),
                (0.02, vec![0xa23]),
                (0.01, vec![0xa1c]),
                (0.01, vec![0xa12]),
            ],
            0x66f => vec![
                (0.03, vec![0xa22]),
                (0.02, vec![0xa23]),
                (0.01, vec![0xa3b]),
                (0.01, vec![0xa0a]),
            ],
            0x670 => vec![
                (0.03, vec![0xa22]),
                (0.02, vec![0xa23]),
                (0.01, vec![0xa2f]),
                (0.01, vec![0xa3d]),
                (0.01, vec![0xa9d]),
                (0.01, vec![0xa5f]),
                (0.01, vec![0xaae]),
                (0.01, vec![0xab5]),
            ],
            0x671 => vec![
                (0.03, vec![0xa22]),
                (0.02, vec![0xa23]),
                (0.01, vec![0xa17]),
                (0.01, vec![0xa6a]),
            ],
            0x672 => vec![
                (0.03, vec![0xa22]),
                (0.02, vec![0xa23]),
                (0.01, vec![0xa3c]),
                (0.01, vec![0xa4e]),
                (0.01, vec![0xa64]),
                (0.01, vec![0xa32]),
            ],
            0x908 => vec![(0.04, vec![0xa22]), (0.06, vec![0xa23])],
            0x90b => vec![(0.1, vec![0xa22]), (0.1, vec![0xa23])],
            0x909 => vec![],
            0x90a => vec![
                (1.0, vec![0xa34]),
                (0.005, vec![0x722]),
                (0.001, vec![0xc01]),
                (0.8, vec![0xa02, 0xa18, 0xa3a, 0xa9e, 0xae0]),
                (0.11, vec![0xa83, 0xa85, 0xa88, 0xa8b, 0xaa0]),
                (0.05, vec![0xa84, 0xa86, 0xa89, 0xa8c, 0xaa1]),
                (0.8, vec![0xa11, 0xa5e, 0xad1]),
                (0.4, vec![0xa0e, 0xa12, 0xadc]),
                (0.11, vec![0xa13, 0xa60, 0xad3]),
                (
                    0.5,
                    vec![
                        0xa44, 0xa69, 0xaa6, 0xaad, 0xab4, 0xacf, 0xad5, 0xad8, 0xada, 0xadb,
                        0xade, 0xb1e, 0xb30,
                    ],
                ),
                (
                    0.06,
                    vec![
                        0xa0b, 0xa33, 0xa45, 0xa54, 0xa59, 0xa64, 0xa6a, 0xaa7, 0xaae, 0xab5,
                        0xad6, 0xb1f, 0xb31,
                    ],
                ),
                (
                    0.5,
                    vec![0xab7, 0xab8, 0xab9, 0xaba, 0xabb, 0xabc, 0xabd, 0xabe],
                ),
                (
                    0.06,
                    vec![0xabf, 0xac0, 0xac1, 0xac2, 0xac3, 0xac4, 0xac5, 0xac6],
                ),
            ],
            0x0e1d => vec![
                (1.0, vec![0xa34]),
                (0.005, vec![0x722]),
                (0.001, vec![0xc01]),
                (0.8, vec![0xa02, 0xa18, 0xa3a, 0xa9e, 0xae0]),
                (0.4, vec![0xa07, 0xa19, 0xa1e, 0xa82, 0xa9f]),
                (0.11, vec![0xa83, 0xa85, 0xa88, 0xa8b, 0xaa0]),
                (0.05, vec![0xa84, 0xa86, 0xa89, 0xa8c, 0xaa1]),
                (0.8, vec![0xa5f, 0xad2, 0xadf]),
                (0.4, vec![0xa0e, 0xa12, 0xadc]),
                (0.11, vec![0xa13, 0xa60, 0xad3]),
                (0.05, vec![0xa8e, 0xa91, 0xa94]),
                (
                    0.5,
                    vec![
                        0xa44, 0xa69, 0xaa6, 0xaad, 0xab4, 0xacf, 0xad5, 0xad8, 0xada, 0xadb,
                        0xade, 0xb1e, 0xb30,
                    ],
                ),
                (
                    0.06,
                    vec![
                        0xa0b, 0xa33, 0xa45, 0xa54, 0xa59, 0xa64, 0xa6a, 0xaa7, 0xaae, 0xab5,
                        0xad6, 0xb1f, 0xb31,
                    ],
                ),
                (
                    0.5,
                    vec![0xab7, 0xab8, 0xab9, 0xaba, 0xabb, 0xabc, 0xabd, 0xabe],
                ),
                (
                    0.06,
                    vec![0xabf, 0xac0, 0xac1, 0xac2, 0xac3, 0xac4, 0xac5, 0xac6],
                ),
            ],
            _ => panic!("not an approved Abyss reward identity"),
        }
    }
    fn selected_rows(content: &Content, kind: u16) -> Vec<(f64, Vec<u16>)> {
        table(kind)
            .iter()
            .map(|(chance, pick)| (*chance, candidates(content, *pick)))
            .collect()
    }
    #[test]
    fn abyss_loot_ordinary_candidate_sets_match_selected_census() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let gear = [
            (0x66d, 0xa59, "Cloak of the Red Agent", 4, 13),
            (0x66d, 0xaa7, "Felwasp Toxin", 4, 18),
            (0x66e, 0xa1c, "Fire Bow", 5, 3),
            (0x66e, 0xa12, "Mithril Armor", 8, 7),
            (0x66f, 0xa3b, "Fire Sword", 5, 1),
            (0x66f, 0xa0a, "Steel Shield", 2, 5),
            (0x670, 0xa2f, "Fire Nova Spell", 2, 11),
            (0x670, 0xa3d, "Wand of Dark Magic", 6, 8),
            (0x670, 0xa9d, "Avenger Staff", 6, 17),
            (0x670, 0xa5f, "Robe of the Invoker", 7, 14),
            (0x670, 0xaae, "Essence Tap Skull", 4, 19),
            (0x670, 0xab5, "Demonhunter Trap", 4, 20),
            (0x671, 0xa17, "Obsidian Dagger", 6, 2),
            (0x671, 0xa6a, "Steel Helm", 4, 16),
            (0x672, 0xa3c, "Glass Sword", 6, 1),
            (0x672, 0xa4e, "Ring of Greater Dexterity", 2, 9),
            (0x672, 0xa64, "Magesteel Quiver", 4, 15),
            (0x672, 0xa32, "Spirit Salve Tome", 2, 4),
        ];
        assert_eq!(gear.len(), 18);
        for (kind, item, name, tier, slot) in gear {
            let object = content.get(item).unwrap();
            assert_eq!(
                (object.name.as_str(), object.tier, object.slot_type),
                (name, Some(tier), slot)
            );
            assert!(
                approved_abyss_rows(kind)
                    .iter()
                    .any(|(_, items)| items == &[item])
            );
        }
        for kind in 0x66d..=0x672 {
            let expected = approved_abyss_rows(kind);
            assert_eq!(
                selected_rows(&content, kind),
                expected,
                "species {kind:#x}: no cross-species or generic tier substitutes"
            );
            for (_, items) in expected {
                let item = content.get(items[0]).unwrap();
                assert!(
                    !item.soulbound && item.bag_type <= 4,
                    "selected public packaging for {:#x}",
                    item.kind
                );
            }
            assert!(!private(kind));
            assert!(bonus(kind).is_empty());
        }
        assert!(selected_rows(&content, 0x672).contains(&(0.01, vec![0xa3c])));
    }
    #[test]
    fn abyss_loot_minion_and_boss_candidates_match_approved_pools() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let catalogue = collectibles(&content);
        for kind in [0x908, 0x909, 0x90b, 0x90a, 0xe1d] {
            assert_eq!(
                selected_rows(&content, kind),
                approved_abyss_rows(kind),
                "pool {kind:#x}"
            );
            validate(&content, kind).unwrap();
            assert_eq!(private(kind), matches!(kind, 0x90a | 0xe1d));
            assert!(bonus(kind).is_empty());
            for (_, pick) in table(kind) {
                for item in candidates(&content, *pick) {
                    assert!(catalogue.contains(&item), "unlisted {item:#x}");
                    assert_ne!(
                        item, 0xa1f,
                        "Idol DEF is held: omit the row, not a zero chance"
                    );
                }
            }
        }
        for kind in 0x66d..=0x672 {
            validate(&content, kind).unwrap();
            for (_, pick) in table(kind) {
                assert!(
                    candidates(&content, *pick)
                        .iter()
                        .all(|item| catalogue.contains(item))
                );
            }
        }
    }
    #[test]
    fn abyss_loot_rolls_match_approved_rates_and_distribution() {
        use rand::RngCore;
        struct BoundaryRng {
            draws: std::collections::VecDeque<u64>,
        }
        impl RngCore for BoundaryRng {
            fn next_u32(&mut self) -> u32 {
                0
            }
            fn next_u64(&mut self) -> u64 {
                self.draws.pop_front().expect("unexpected reward roll")
            }
            fn fill_bytes(&mut self, _: &mut [u8]) {
                panic!("unexpected byte RNG");
            }
            fn try_fill_bytes(&mut self, _: &mut [u8]) -> std::result::Result<(), rand::Error> {
                panic!("unexpected byte RNG");
            }
        }
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for kind in (0x66d..=0x672).chain([0x908, 0x909, 0x90b, 0x90a, 0xe1d]) {
            let expected = approved_abyss_rows(kind);
            for selected in 0..=expected.len() {
                for hit in [false, true] {
                    let mut rng = BoundaryRng {
                        draws: expected
                            .iter()
                            .enumerate()
                            .filter(|(_, (p, _))| *p < 1.)
                            .map(|(index, (p, _))| {
                                let boundary = (p * (u64::MAX as f64 + 1.)) as u64;
                                if hit && index == selected {
                                    boundary - 1
                                } else {
                                    boundary
                                }
                            })
                            .collect(),
                    };
                    let actual = rewards(&content, kind, &mut rng);
                    let wanted: Vec<_> = expected
                        .iter()
                        .enumerate()
                        .filter(|(index, (p, _))| *p == 1. || (hit && *index == selected))
                        .map(|(_, (_, items))| items[0])
                        .collect();
                    assert_eq!(actual, wanted, "kind {kind:#x}, row {selected}, hit {hit}");
                    assert!(rng.draws.is_empty(), "each optional row rolls exactly once");
                }
            }
            let mut hits = rand::rngs::mock::StepRng::new(0, 0);
            assert_eq!(
                rewards(&content, kind, &mut hits),
                expected
                    .iter()
                    .map(|(_, items)| items[0])
                    .collect::<Vec<_>>()
            );
            let mut actual = StdRng::seed_from_u64(12351);
            let mut reference = actual.clone();
            for _ in 0..1024 {
                let mut wanted = Vec::new();
                for (chance, choices) in &expected {
                    if reference.gen_bool(*chance) {
                        wanted.push(*choices.choose(&mut reference).unwrap());
                    }
                }
                assert_eq!(rewards(&content, kind, &mut actual), wanted);
            }
        }
        // Eligibility, per-account groups and duplicate collapse belong to World, not this row roller.
    }
    #[test]
    fn abyss_loot_non_abyss_tables_routing_and_rolls_remain_unchanged() {
        use sha2::{Digest, Sha256};
        let content = Content::load(std::path::Path::new("data")).unwrap();
        // Frozen non-Abyss rows and rolls, including upstream's guaranteed Limon DEX drop.
        for (kind, expected) in [
            (
                0x651,
                "503f03dbf9cd88c75b6a87263743cf61a3fd546ff0bc8f48371a4268fb322db8",
            ),
            (
                0x657,
                "503f03dbf9cd88c75b6a87263743cf61a3fd546ff0bc8f48371a4268fb322db8",
            ),
            (
                0x900,
                "bf5338634bb4f6880db5803f043b3ad3f8ce644efe8a3151de5e9b851067ca06",
            ),
            (
                0x901,
                "0eddb8d16f6722e29c6a5a2c6787bf14f6fe9d44c4b91d8593c136c33fe73c88",
            ),
            (
                0x902,
                "a3ae3df816fb79027f1129c69bd707eb4a7b74629721c69032156517de0edf2a",
            ),
            (
                0x927,
                "562be985949a9d637cb5e5cd890ba22716d48deaf8171a57b5fa5f6540e308d4",
            ),
            (
                0xd06,
                "59e81eba88c6ed76e5d5a2a859758f8c09565a90f201745f890fb48701d00d93",
            ),
            (
                0xd90,
                "405e7f8749f1f0ceeec5c74a7dfa8b6daa14a8f652c4b4b289ce35bb9a6435e1",
            ),
            (
                0xd62,
                "88fc781beed1aa09fc1c5e108ecb3ca890017c61ca887df7ec6972dbda8549ad",
            ),
            (
                0x1705,
                "3d1e5d08f976180504964e859ac20b918b083d561b59861a8c5f1f95b8f592c8",
            ),
            (
                0x1740,
                "9dbfe60ded9445048768639b0b57bd1ee7089f5ec2f6c6c9f8e57bef9726ac9b",
            ),
            (
                0x932,
                "e3094c6b7327c04fa469ae614e83359200c3efc32fe4870467032408adf657ea",
            ),
        ] {
            let mut rng = StdRng::seed_from_u64(12351);
            let rolls: Vec<_> = (0..32)
                .map(|_| {
                    (
                        roll(&content, kind, &mut rng),
                        rewards(&content, kind, &mut rng),
                    )
                })
                .collect();
            let signature = format!(
                "{:?}|{:?}|{}|{:?}",
                selected_rows(&content, kind),
                bonus(kind),
                private(kind),
                rolls
            );
            let hash = format!("{:x}", Sha256::digest(signature.as_bytes()));
            assert_eq!(hash, expected, "unrelated encounter {kind:#x}");
        }
    }
    struct Branch(Vec<u64>);
    impl rand::RngCore for Branch {
        fn next_u32(&mut self) -> u32 {
            0
        }
        fn next_u64(&mut self) -> u64 {
            self.0.remove(0)
        }
        fn fill_bytes(&mut self, _: &mut [u8]) {
            panic!("unexpected byte draw")
        }
        fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand::Error> {
            panic!("unexpected byte draw")
        }
    }
    #[test]
    fn midland_great_lizard_and_warrior_wasp_ordered_public_tables() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, name, expected) in [
            (
                0x952,
                "Great Lizard",
                vec![
                    (0.05, vec![0xa06, 0xa16, 0xa1c, 0xa3b, 0xa9c]),
                    (0.05, vec![0xa10, 0xa80, 0xad0]),
                ],
            ),
            (
                0x949,
                "Warrior Wasp",
                vec![
                    (0.10, vec![0xa01, 0xa05, 0xa73, 0xa76, 0xa9a]),
                    (0.05, vec![0xa6e, 0xa71, 0xa74, 0xa77, 0xa9b]),
                    (0.10, vec![0xa5d, 0xa79, 0xa7c]),
                    (0.05, vec![0xa7a, 0xa7d, 0xa7f]),
                ],
            ),
        ] {
            assert_eq!(content.get(kind).unwrap().name, name);
            assert_eq!(
                table(kind)
                    .iter()
                    .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
                    .collect::<Vec<_>>(),
                expected
            );
            for (branch, (_, items)) in expected.iter().enumerate() {
                let mut rng = Branch(vec![u64::MAX; branch]);
                rng.0.push(0);
                let result = rewards(&content, kind, &mut rng);
                assert_eq!(result.len(), 1);
                assert!(items.contains(&result[0]) && rng.0.is_empty());
            }
            let mut misses = Branch(vec![u64::MAX; expected.len()]);
            assert!(rewards(&content, kind, &mut misses).is_empty() && misses.0.is_empty());
            assert!(!private(kind) && bonus(kind).is_empty());
            validate(&content, kind).unwrap();
            assert!(
                expected
                    .iter()
                    .flat_map(|(_, items)| items)
                    .all(|item| collectibles(&content).contains(item))
            );
        }
    }
    #[test]
    #[should_panic(expected = "unsupported dual-channel fauna kind 0x947")]
    fn midland_private_rewards_reject_unsupported_enemy_kind() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        fauna_private_rewards(&content, 0x947, &[], &mut Branch(vec![]));
    }
    #[test]
    fn midland_private_tables_use_strict_threshold_ordered_success_allocation_and_dedup_cap() {
        let mut content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, expected) in [
            (
                0x952,
                vec![(
                    0.03,
                    vec![0xa29, 0xa2a, 0xa2b, 0xa2c, 0xa2d, 0xa38, 0xa39, 0xa4e],
                )],
            ),
            (
                0x949,
                vec![
                    (0.03, vec![0xa17, 0xa1d, 0xa3c, 0xa3d, 0xa9d]),
                    (0.03, vec![0xa11, 0xa5e, 0xad1]),
                ],
            ),
        ] {
            assert_eq!(
                fauna_private_table(kind)
                    .iter()
                    .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
                    .collect::<Vec<_>>(),
                expected
            );
            for (branch, (_, items)) in expected.iter().enumerate() {
                let mut rng = Branch(vec![u64::MAX; branch]);
                rng.0.push(0);
                if kind == 0x949 {
                    rng.0.extend([u64::MAX; 2]);
                }
                let rewards = fauna_private_rewards(&content, kind, &[(9, 2), (8, 1)], &mut rng);
                assert_eq!(rewards.len(), 1);
                assert_eq!(rewards[0].0, 9);
                assert!(items.contains(&rewards[0].1) && rng.0.is_empty());
            }
            let mut misses = Branch(vec![
                u64::MAX;
                expected.len() * if kind == 0x949 { 2 } else { 1 }
            ]);
            assert!(
                fauna_private_rewards(&content, kind, &[(9, 2)], &mut misses).is_empty()
                    && misses.0.is_empty()
            );
            let mut no_draws = Branch(vec![]);
            assert!(fauna_private_rewards(&content, kind, &[(9, 1)], &mut no_draws).is_empty());
            for item in expected.iter().flat_map(|(_, items)| items) {
                assert!(collectibles(&content).contains(item));
                assert!(!content.get(*item).unwrap().soulbound);
            }
        }
        let mut delayed = Branch(vec![u64::MAX, u64::MAX, u64::MAX, 0, 0, u64::MAX, u64::MAX]);
        assert_eq!(
            fauna_private_rewards(&content, 0x949, &[(9, 3), (8, 2), (7, 1)], &mut delayed),
            vec![(9, 0xa11), (8, 0xa17)]
        );
        assert!(delayed.0.is_empty());
        let owners: Vec<_> = (1..=6).map(|owner| (owner, 2)).collect();
        let mut eight_successes = Branch(vec![0; 8]);
        assert_eq!(
            fauna_private_rewards(&content, 0x949, &owners, &mut eight_successes),
            (1..=6).map(|owner| (owner, 0xa17)).collect::<Vec<_>>()
        );
        assert!(
            eight_successes.0.is_empty(),
            "cap eight successes before per-owner dedup; not twelve attempts"
        );
        content.objects.get_mut(&0xa29).unwrap().bag_type = 5;
        assert!(validate(&content, 0x952).is_err());
        validate(&content, 0x949).unwrap();
        content.objects.get_mut(&0xa17).unwrap().bag_type = 5;
        assert!(validate(&content, 0x949).is_err());
    }
    #[test]
    fn starter_gray_private_rolls_use_strict_threshold_success_order_and_eight_success_cap() {
        let mut content = Content::load(std::path::Path::new("data")).unwrap();
        assert_eq!(
            fauna_private_table(0x20d)
                .iter()
                .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
                .collect::<Vec<_>>(),
            vec![(0.005, vec![0xa51])]
        );
        let mut second_roll = Branch(vec![u64::MAX, 0]);
        assert_eq!(
            fauna_private_rewards(&content, 0x20d, &[(1, 3), (2, 2), (3, 1)], &mut second_roll),
            vec![(1, 0xa51)]
        );
        assert!(second_roll.0.is_empty());
        let owners: Vec<_> = (1..=12).map(|account| (account, 2)).collect();
        let mut hits = rand::rngs::mock::StepRng::new(0, 0);
        assert_eq!(
            fauna_private_rewards(&content, 0x20d, &owners, &mut hits),
            (1..=8).map(|account| (account, 0xa51)).collect::<Vec<_>>()
        );
        let mut misses = rand::rngs::mock::StepRng::new(u64::MAX, 0);
        assert!(fauna_private_rewards(&content, 0x20d, &owners, &mut misses).is_empty());
        content.objects.get_mut(&0xa51).unwrap().bag_type = 5;
        assert!(validate(&content, 0x20d).is_err());
        validate(&content, 0x205).unwrap();
    }
    #[test]
    fn starter_enemy_tables_use_the_reconstructed_ordered_public_rewards() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, name) in [
            (0x205, "Pink Blob"),
            (0x20d, "Gray Blob"),
            (0x20f, "Little Green Slime"),
            (0x202, "Red Spider"),
            (0x204, "Black Bat"),
        ] {
            assert_eq!(content.get(kind).unwrap().name, name);
            let mut expected = vec![(0.03, vec![0xa22]), (0.02, vec![0xa23])];
            if kind == 0x204 {
                expected.insert(0, (0.01, vec![0xa0d, 0xa0f, 0xa7e]));
            }
            let actual: Vec<_> = table(kind)
                .iter()
                .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
                .collect();
            assert_eq!(actual, expected, "{name}");
            for (branch, (_, items)) in expected.iter().enumerate() {
                let mut rng = Branch(vec![u64::MAX; branch]);
                rng.0.push(0);
                let result = rewards(&content, kind, &mut rng);
                assert_eq!(result.len(), 1);
                assert!(items.contains(&result[0]));
                assert!(rng.0.is_empty(), "first success ends the attempt");
            }
            let mut rng = Branch(vec![u64::MAX; expected.len()]);
            assert!(rewards(&content, kind, &mut rng).is_empty());
            assert!(rng.0.is_empty());
            assert!(!private(kind) && bonus(kind).is_empty());
            validate(&content, kind).unwrap();
        }
        assert!(collectibles(&content).contains(&0xa51));
        let mushroom = content.get(0xa51).unwrap();
        assert_eq!(
            (
                mushroom.name.as_str(),
                mushroom.bag_type,
                mushroom.soulbound
            ),
            ("Magic Mushroom", 1, false)
        );
        for kind in [0x20e, 0x206, 0x620, 0x622, 0x947, 0x948] {
            assert!(table(kind).is_empty() && bonus(kind).is_empty() && !private(kind));
        }
    }
    #[test]
    fn tomb_life_potions_do_not_shadow_the_correct_boss_rings_or_white_bag() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (boss, ring) in [
            (crate::dungeon::tomb::NUT, 0xba0),
            (crate::dungeon::tomb::GEB, 0xba1),
            (crate::dungeon::tomb::BES, 0xba2),
        ] {
            let mut observed = std::collections::BTreeSet::new();
            for seed in 0..4096 {
                let items = rewards(&content, boss, &mut StdRng::seed_from_u64(seed));
                assert_eq!(items.len(), 1, "one ordered Tomb pool roll");
                observed.extend(items);
            }
            assert!(
                observed.contains(&ring),
                "boss {boss:#x} never drops its ring"
            );
            assert!(
                [0xba0, 0xba1, 0xba2]
                    .into_iter()
                    .all(|item| item == ring || !observed.contains(&item))
            );
            assert!(observed.contains(&0xae9));
            assert!(observed.contains(&crate::endgame::INCANTATION));
            if boss == crate::dungeon::tomb::BES {
                assert!(observed.contains(&0xc1e));
            }
        }
        let sarc_drops: Vec<_> = table(crate::dungeon::tomb::SARCOPHAGUS)
            .iter()
            .map(|(_, pick)| match pick {
                Pick::Item(item) => *item,
                _ => panic!("Sarcophagus must use selected consumable identities"),
            })
            .collect();
        assert_eq!(sarc_drops, [0xa22, 0xa23, 0xb0e, 0xb0d, 0xb0c]);
        assert!((0..64).any(|seed| {
            rewards(
                &content,
                crate::dungeon::tomb::TREASURE,
                &mut StdRng::seed_from_u64(seed),
            )
            .len()
                > 1
        }));
    }
    #[test]
    fn abyss_bosses_guarantee_only_vitality() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for kind in [crate::dungeon::abyss::MALPHAS, crate::dungeon::abyss::IDOL] {
            for seed in 0..64 {
                let mut rng = StdRng::seed_from_u64(seed);
                let items = rewards(&content, kind, &mut rng);
                assert!(items.contains(&0xa34), "kind {kind:#x} seed {seed}");
                assert!(!items.contains(&0xa20), "kind {kind:#x} seed {seed}");
            }
        }
    }
    #[test]
    fn desert_werewolf_uses_only_existing_ordered_public_tiers_without_a_minimum() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let expected = [
            (0.1, 3, WEAPONS),
            (0.05, 4, WEAPONS),
            (0.1, 3, ARMOR),
            (0.05, 4, ARMOR),
            (0.05, 1, ABILITIES),
            (0.05, 1, RINGS),
        ];
        assert_eq!(table(0x623).len(), expected.len());
        let existing: std::collections::BTreeSet<_> = [0x614, 0x62c]
            .into_iter()
            .flat_map(|kind| {
                table(kind)
                    .iter()
                    .flat_map(|(_, pick)| candidates(&content, *pick))
            })
            .collect();
        for ((chance, pick), (expected_chance, tier, slots)) in table(0x623).iter().zip(expected) {
            let Pick::Tier(actual_tier, actual_slots) = pick else {
                panic!("ordinary selected tier")
            };
            assert_eq!(
                (*chance, *actual_tier, *actual_slots),
                (expected_chance, tier, slots)
            );
            for item in candidates(&content, *pick) {
                assert!(existing.contains(&item));
                let item = content.get(item).unwrap();
                assert_eq!(item.class, "Equipment");
                assert!(!item.soulbound && item.bag_type == 0);
            }
        }
        let mut misses = rand::rngs::mock::StepRng::new(u64::MAX, 0);
        assert!(rewards(&content, 0x623, &mut misses).is_empty());
        let mut hits = rand::rngs::mock::StepRng::new(0, 0);
        let first = rewards(&content, 0x623, &mut hits);
        assert_eq!(first.len(), 1);
        let first = content.get(first[0]).unwrap();
        assert_eq!(first.tier, Some(3));
        assert!(WEAPONS.contains(&first.slot_type));
        for kind in [0x620, 0x622, 0x20e] {
            assert!(rewards(&content, kind, &mut hits).is_empty());
        }
        assert!(!private(0x623));
    }
    #[test]
    fn swarm_uses_only_the_reconstructed_ordered_public_pool_without_a_minimum() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let expected = [
            vec![0xa01, 0xa05, 0xa73, 0xa76, 0xa9a],
            vec![0xa5d, 0xa79, 0xa7c],
            vec![
                0xa09, 0xa42, 0xa57, 0xa62, 0xa67, 0xaa4, 0xaab, 0xab2, 0xad4, 0xad7, 0xad9, 0xb1c,
                0xb2e,
            ],
            vec![0xa24, 0xa25, 0xa26, 0xa27, 0xa28, 0xa36, 0xa37, 0xa4d],
        ];
        let actual: Vec<_> = table(0x955)
            .iter()
            .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
            .collect();
        assert_eq!(actual, expected.clone().map(|items| (0.15, items)));
        let existing: std::collections::BTreeSet<_> = table(0x614)
            .iter()
            .flat_map(|(_, pick)| candidates(&content, *pick))
            .collect();
        let catalogue = collectibles(&content);
        for item in expected.into_iter().flatten() {
            let object = content.get(item).unwrap();
            assert_eq!(object.class, "Equipment");
            assert!(!object.soulbound && object.bag_type == 0);
            assert!(existing.contains(&item) && catalogue.contains(&item));
        }
        let mut misses = rand::rngs::mock::StepRng::new(u64::MAX, 0);
        assert!(rewards(&content, 0x955, &mut misses).is_empty());
        let mut hits = rand::rngs::mock::StepRng::new(0, 0);
        let first = rewards(&content, 0x955, &mut hits);
        assert_eq!(first.len(), 1);
        assert!(candidates(&content, Pick::Tier(3, WEAPONS)).contains(&first[0]));
        assert!(!private(0x955));
        assert!(bonus(0x955).is_empty());
        for distinct in [0x919, 0x954] {
            assert!(table(distinct).is_empty());
        }
    }
    #[test]
    fn horned_drake_uses_only_the_reconstructed_ordered_public_pool_without_a_minimum() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let expected = [
            (0.10, vec![0xa06, 0xa16, 0xa1c, 0xa3b, 0xa9c]),
            (0.10, vec![0xa10, 0xa80, 0xad0]),
            (0.03, vec![0xa22]),
            (0.02, vec![0xa23]),
            (
                0.05,
                vec![
                    0xa0a, 0xa2f, 0xa32, 0xa43, 0xa53, 0xa58, 0xa63, 0xa68, 0xaa5, 0xaac, 0xab3,
                    0xb1d, 0xb2f,
                ],
            ),
        ];
        let actual: Vec<_> = table(0x945)
            .iter()
            .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
            .collect();
        assert_eq!(actual, expected);
        let catalogue = collectibles(&content);
        for (_, items) in expected {
            for item in items {
                let object = content.get(item).unwrap();
                assert_eq!(object.class, "Equipment");
                assert!(!object.soulbound && object.bag_type == 0);
                assert!(catalogue.contains(&item));
            }
        }
        let mut misses = rand::rngs::mock::StepRng::new(u64::MAX, 0);
        assert!(rewards(&content, 0x945, &mut misses).is_empty());
        let mut hits = rand::rngs::mock::StepRng::new(0, 0);
        let first = rewards(&content, 0x945, &mut hits);
        assert_eq!(first.len(), 1);
        assert!(candidates(&content, Pick::Tier(5, WEAPONS)).contains(&first[0]));
        assert!(!private(0x945));
        assert!(bonus(0x945).is_empty());
        assert!(table(0x946).is_empty());
    }
    #[test]
    fn collection_catalogue_covers_every_encounter_reward() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let catalogue = collectibles(&content);
        for item in [0xa32, 0xa3a, 0xa6a, 0xa80, 0xa1f, 0xc0b, 0xc10] {
            assert!(catalogue.contains(&item), "missing collectible {item:#x}");
        }
        let mut rng = StdRng::seed_from_u64(127);
        for kind in [0x67d, 0x668] {
            for _ in 0..100 {
                for item in landmark_chest(&content, kind, &mut rng)
                    .unwrap()
                    .into_iter()
                    .filter(|item| *item >= 0)
                {
                    assert!(
                        catalogue.contains(&(item as u16)),
                        "unlisted chest item {item:#x}"
                    );
                }
            }
        }
        for kind in content
            .objects
            .keys()
            .copied()
            .filter(|kind| crate::encounter::profile(*kind).is_some())
        {
            validate(&content, kind).unwrap();
            for _ in 0..100 {
                for item in rewards(&content, kind, &mut rng) {
                    assert!(
                        catalogue.contains(&item),
                        "encounter {kind:#x} dropped unlisted {item:#x}"
                    );
                }
            }
        }
    }
    #[test]
    fn giant_crab_uses_only_the_approved_ordered_normal_pool() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let expected = [
            (0.10, vec![0xa1b, 0xa6d, 0xa70, 0xa72, 0xa99]),
            (0.10, vec![0xa0d, 0xa0f, 0xa7e]),
            (
                0.10,
                vec![0xa24, 0xa25, 0xa26, 0xa27, 0xa28, 0xa36, 0xa37, 0xa4d],
            ),
            (
                0.10,
                vec![
                    0xa09, 0xa42, 0xa57, 0xa62, 0xa67, 0xaa4, 0xaab, 0xab2, 0xad4, 0xad7, 0xad9,
                    0xb1c, 0xb2e,
                ],
            ),
            (0.03, vec![0xa22]),
            (0.02, vec![0xa23]),
        ];
        let actual: Vec<_> = table(0x6c4)
            .iter()
            .map(|(chance, pick)| (*chance, candidates(&content, *pick)))
            .collect();
        assert_eq!(
            actual, expected,
            "ordinary Giant Crab has no approved item table"
        );
        assert!(!private(0x6c4));
        assert!(bonus(0x6c4).is_empty());
        for (_, items) in expected {
            for item in items {
                let object = content.get(item).unwrap();
                assert_eq!(object.class, "Equipment");
                assert_eq!(object.bag_type, 0);
                assert!(!object.soulbound);
                assert!(collectibles(&content).contains(&item));
            }
        }
        for neighbor in [0x6c7, 0x6c8] {
            assert!(table(neighbor).is_empty());
        }
        assert_eq!(table(crate::dungeon::trench::HERMIT).len(), 3);
    }
    #[test]
    fn every_candidate_loot_tier_resolves_and_rolls_selected_items() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let mut rng = StdRng::seed_from_u64(17);
        for kind in [
            0x600, 0x60e, 0x60f, 0x610, 0x683, 0x684, 0x685, 0x686, 0x687, 0x688, 0x689, 0x68a,
            0x68b, 0x68c, 0x68d, 0x68e, 0x68f, 0x927,
        ] {
            validate(&content, kind).unwrap();
            let mut dropped = false;
            for _ in 0..1000 {
                if let Some(item) = roll(&content, kind, &mut rng) {
                    dropped = true;
                    assert_eq!(content.get(item).unwrap().class, "Equipment");
                }
            }
            assert!(dropped);
        }
        assert!(!private(0x600));
        assert!(private(0x927));
    }
}
