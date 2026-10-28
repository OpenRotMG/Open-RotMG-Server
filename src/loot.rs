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
// The period wiki calls Wolfskin/Neophyte/Iron Mail T0; the selected client calls them T1.
// Armor rates below use selected-client tiers (wiki tier + 1), verified through all 39 named armors.
pub(crate) const ARMOR: &[i32] = &[6, 7, 14];
pub(crate) const RINGS: &[i32] = &[9];
pub(crate) fn fauna_private_table(kind: u16) -> &'static [(f64, Pick)] {
    use Pick::Tier;
    match kind {
        0x20d => &[(0.005, Pick::Item(0xa51))],
        0x952 => &[(0.03, Tier(2, RINGS))],
        0x949 | 0x950 => &[(0.03, Tier(6, WEAPONS)), (0.03, Tier(6, ARMOR))],
        0x945 => &[
            (0.05, Tier(6, WEAPONS)),
            (0.05, Tier(6, ARMOR)),
            (0.05, Tier(2, RINGS)),
            (0.03, Tier(3, RINGS)),
            (0.03, Tier(3, ABILITIES)),
        ],
        0x903 => &[(0.01, Tier(2, RINGS))],
        // Trapped's ordered first-success soul bag: Wand, Sword, then Att/Def/Dex/Spd/Vit/Wis potions: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.Crystal.cs#L674-L684
        0x941 => &[
            (0.02, Pick::Item(0xb3f)),
            (0.02, Pick::Item(0xa03)),
            (0.5, Pick::Item(0xa1f)),
            (0.5, Pick::Item(0xa20)),
            (0.5, Pick::Item(0xa4c)),
            (0.5, Pick::Item(0xa21)),
            (0.5, Pick::Item(0xa34)),
            (0.5, Pick::Item(0xa35)),
        ],
        0x924 => &[(0.001, Pick::Item(0xc07))],
        0x957 => &[
            (0.02, Tier(5, WEAPONS)),
            (0.01, Tier(6, WEAPONS)),
            (0.005, Tier(7, WEAPONS)),
            (0.02, Tier(5, ARMOR)),
            (0.01, Tier(6, ARMOR)),
            (0.005, Tier(7, ARMOR)),
            (0.015, Tier(3, RINGS)),
            (0.015, Tier(3, ABILITIES)),
        ],
        _ => &[],
    }
}
pub(crate) fn period_dungeon(kind: u16) -> bool {
    matches!(
        kind,
        0x21d..=0x220 | 0x936 | 0x683..=0x68f | 0x927 | 0xd01..=0xd04
            | 0xd06 | 0x917 | 0xd90 | 0x66d..=0x672 | crate::dungeon::abyss::MALPHAS
    )
}
pub(crate) fn period_surface(kind: u16) -> bool {
    matches!(
        kind,
        0x202 | 0x204 | 0x205 | 0x209 | 0x20d | 0x20f
            | 0x600..=0x602 | 0x604..=0x607 | 0x609 | 0x60a | 0x60c..=0x610
            | 0x612..=0x615 | 0x617 | 0x618 | 0x61a | 0x61c..=0x61f
            | 0x624..=0x626 | 0x629..=0x62f | 0x631..=0x633
            | 0x63b | 0x63c | 0x63e..=0x640 | 0x642..=0x645 | 0x647..=0x64d
            | 0x64f | 0x650 | 0x65f..=0x661 | 0x669..=0x66b | 0x6c6 | 0x900..=0x902 | 0x91d | 0x91e
    )
}
fn table(kind: u16) -> &'static [(f64, Pick)] {
    use Pick::{Item, Tier};
    match kind {
        0x6af..=0x6b1 | 0x6b3 => &[],
        0x6b2 => &[(1., Item(0xab0))],
        0x6b4 => &[(0.1, Item(0xab0)), (0.1, Item(0xadd))],
        crate::encounter::tutorial::CHEF => &[(1., Item(0xab0)), (1., Item(0xadd))],
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
        // https://web.archive.org/web/20120201110941/http://forums.wildshadow.com/wiki/realm-mad-god/arachna-spider-queen
        crate::dungeon::spider::BOSS => &[
            (0.99, Item(0x707)),
            (0.25, Item(0x707)),
            (0.23, Item(0x708)),
            (0.23, Item(0x709)),
            (0.99, Item(0xa16)),
        ],
        // https://web.archive.org/web/20120216023204/http://forums.wildshadow.com/wiki/realm-mad-god/brown-den-spider
        0x21d..=0x220 => &[(0.15, Item(0x707))],
        0x21b | 0x21c | 0x937..=0x93f => &[],
        crate::dungeon::trench::HERMIT => {
            &[(0.1, Item(0xa34)), (0.1, Item(0xa4c)), (0.004, Item(0xc08))]
        }
        // BehaviorDb.Sphinx.cs / Oasis.cs, realm-src@1855c017 (GPL-3.0; data/REALM-SRC-LICENSE), NR-Core and Skilly.
        0xd54 => &[(0.1, Item(0xa34)), (0.1, Item(0xa35)), (0.004, Item(0xc08))],
        0xd62 => &[(0.1, Item(0xa22)), (0.1, Item(0xa23))],
        0x679 | 0x67c => &[(0.05, Item(0xa23))],
        0x67a | 0x67b => &[(0.05, Item(0xa22))],
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
        0x949 | 0x950 => &[
            (0.10, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.10, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
        ],
        0x637 => &[
            (0.10, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.03, Tier(5, WEAPONS)),
            (0.10, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
            (0.03, Tier(5, ARMOR)),
            (0.05, Tier(1, RINGS)),
            (0.05, Tier(1, ABILITIES)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x205 | 0x20d | 0x20f | 0x202 => &[(0.03, Item(0xa22)), (0.03, Item(0xa23))],
        0x635 | 0x636 | 0x639 | 0x6c5 | 0x7f22 | 0x6c7 | 0x6c9 | 0x6d1 | 0x951 => {
            &[(0.03, Item(0xa22)), (0.02, Item(0xa23))]
        }
        0x903 => &[(0.04, Item(0xa22)), (0.01, Item(0xa23))],
        0x209 => &[
            (0.03, Item(0xa22)),
            (0.03, Item(0xa23)),
            (0.01, Item(0xa54)),
        ],
        0x904 => &[(0.03, Item(0xa23)), (0.01, Tier(5, WEAPONS))],
        0x204 => &[
            (0.01, Tier(2, ARMOR)),
            (0.01, Item(0xa22)),
            (0.01, Item(0xa23)),
        ],
        // GPL-3.0 realm-src@1855c017, BehaviorDb.{LotLL,GhostShip}.cs; NR-Core and Skilly.
        0xd50 => &[
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
            (0.1, Item(0xa20)),
            (0.1, Item(0xa1f)),
            (0.1, Item(0xa34)),
            (0.1, Item(0xa35)),
            (0.1, Item(0xa21)),
            (0.1, Item(0xa4c)),
            (0.004, Item(0xc0f)),
        ],
        0xd52 | 0xd53 => &[(0.1, Item(0xa22)), (0.1, Item(0xa23))],
        0xe37 => &[
            (0.1, Item(0xa35)),
            (0.12, Tier(8, WEAPONS)),
            (0.1, Tier(9, WEAPONS)),
            (0.12, Tier(8, ARMOR)),
            (0.1, Tier(9, ARMOR)),
            (0.12, Tier(3, ABILITIES)),
            (0.1, Tier(4, ABILITIES)),
            (0.07, Tier(4, RINGS)),
            (0.06, Tier(10, WEAPONS)),
            (0.04, Tier(11, WEAPONS)),
            (0.06, Tier(10, ARMOR)),
            (0.04, Tier(11, ARMOR)),
            (0.04, Tier(5, ABILITIES)),
            (0.04, Tier(5, RINGS)),
            (0.001, Item(crate::endgame::INCANTATION)),
        ],
        // Item lists match the May-June 2012 Wild Shadow skeleton pages, e.g. https://web.archive.org/web/20120608231314/http://forums.wildshadow.com/wiki/realm-mad-god/skeleton-mage
        // Rates are realm-src@1855c017 community values (data/REALM-SRC-LICENSE), whose Mage lists Comet Staff twice; kept once:
        // https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/db/BehaviorDb.Deathmage.cs
        0x65a => &[(0.02, Item(0xa01)), (0.02, Item(0xa15))],
        0x65b => &[
            (0.03, Item(0xa01)),
            (0.02, Item(0xa0a)),
            (0.02, Item(0xa67)),
        ],
        0x65c => &[
            (0.03, Item(0xa01)),
            (0.02, Item(0xa0b)),
            (0.01, Item(0xa57)),
            (0.01, Item(0xaa4)),
        ],
        0x65d => &[
            (0.02, Item(0xa05)),
            (0.02, Item(0xa99)),
            (0.02, Item(0xa2f)),
        ],
        // Trapped 593e490 engineering tables, not authenticated 123.5.1 probabilities. Kithio lists only potions; the May 2012
        // wiki pool (wiki T2/T3 armor, T1 rings, HP/MP) omits Trapped's T2 weapon and adds a Master HP potion:
        // https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.Lowland.cs#L153-L196
        // https://web.archive.org/web/20120509105306/http://forums.wildshadow.com:80/wiki/realm-mad-god/sumo-master
        0x7f00 => &[
            (0.10, Tier(2, WEAPONS)),
            (0.05, Tier(3, ARMOR)),
            (0.03, Tier(4, ARMOR)),
            (0.10, Tier(1, RINGS)),
            (0.03, Item(0xa23)),
        ],
        0x7f01 => &[(0.03, Item(0xa22)), (0.02, Item(0xa23))],
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
        // Wild Shadow wiki rates before the March 2012 edits removed them (Pentaract is per tower):
        // https://web.archive.org/web/20120117000123/http://forums.wildshadow.com:80/wiki/realm-mad-god/cube-god
        0xd56 | 0xd59 => &[
            (0.4, Tier(8, WEAPONS)),
            (0.2, Tier(9, WEAPONS)),
            (0.1, Tier(10, WEAPONS)),
            (0.05, Tier(11, WEAPONS)),
            (0.4, Tier(8, ARMOR)),
            (0.2, Tier(9, ARMOR)),
            (0.1, Tier(10, ARMOR)),
            (0.05, Tier(11, ARMOR)),
            (0.025, Tier(12, ARMOR)),
            (0.4, Tier(3, RINGS)),
            (0.1, Tier(4, RINGS)),
            (0.025, Tier(5, RINGS)),
            (0.2, Tier(4, ABILITIES)),
            (0.05, Tier(5, ABILITIES)),
            (0.5, Tier(2, &[10])),
            (0.5, Tier(2, &[10])),
            (0.5, Tier(2, &[10])),
        ],
        0xd60 => &[
            (0.08, Tier(8, WEAPONS)),
            (0.04, Tier(9, WEAPONS)),
            (0.02, Tier(10, WEAPONS)),
            (0.01, Tier(11, WEAPONS)),
            (0.08, Tier(8, ARMOR)),
            (0.04, Tier(9, ARMOR)),
            (0.02, Tier(10, ARMOR)),
            (0.01, Tier(11, ARMOR)),
            (0.005, Tier(12, ARMOR)),
            (0.08, Tier(3, RINGS)),
            (0.02, Tier(4, RINGS)),
            (0.005, Tier(5, RINGS)),
            (0.04, Tier(4, ABILITIES)),
            (0.01, Tier(5, ABILITIES)),
            (0.1, Tier(2, &[10])),
            (0.1, Tier(2, &[10])),
            (0.1, Tier(2, &[10])),
        ],
        0x91b | 0x91f | 0x928 | 0x675 | 0x6c2 | 0x923 => &[],
        0x676 => &[(0.05, Item(0xa23))],
        0x677 => &[(0.05, Item(0xa22))],
        // Ents drop a tincture, Saplings nothing: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.EntAncient.cs#L219-L244
        0x921 => &[(0.02, Item(0xb0c))],
        0x922 => &[],
        // Distinct follower pools, not generic potions: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Lich.cs#L321-L415
        0x65f | 0x660 => &[(0.02, Item(0xa23)), (0.02, Item(0xa32))],
        0x661 => &[
            (0.02, Item(0xa06)),
            (0.02, Item(0xa9c)),
            (0.02, Item(0xa02)),
            (0.02, Item(0xa16)),
        ],
        0x91d => &[
            (0.02, Item(0xb0f)),
            (0.06, Item(0xaf1)),
            (0.03, Item(0xa23)),
        ],
        0x91e => &[
            (0.02, Tier(8, WEAPONS)),
            (0.02, Item(0xa23)),
            (0.02, Item(0xa28)),
            (0.02, Item(0xa24)),
            (0.06, Item(0xb0c)),
            (0.09, Item(0xb0e)),
            (0.04, Item(0xb0d)),
        ],
        0x929 | 0x92a | 0x92b | 0x92c | 0x924 => &[(0.03, Item(0xa23))],
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
        0x624 => &[(0.02, Tier(5, WEAPONS)), (0.05, Item(0xa23))],
        0x625 => &[(0.04, Tier(2, ABILITIES)), (0.05, Item(0xa23))],
        0x626 => &[(0.01, Tier(6, ARMOR)), (0.05, Item(0xa23))],
        0x62d => &[(0.04, Tier(5, WEAPONS)), (0.03, Item(0xa22))],
        0x631 => &[
            (0.04, Tier(5, WEAPONS)),
            (0.03, Item(0xa22)),
            (0.03, Item(0xa23)),
        ],
        0x62e => &[(0.04, Tier(6, ARMOR)), (0.03, Item(0xa22))],
        0x632 => &[
            (0.1, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.04, Tier(6, ARMOR)),
            (0.05, Tier(1, RINGS)),
            (0.05, Tier(1, ABILITIES)),
            (0.03, Item(0xa23)),
        ],
        0x62f => &[(0.03, Item(0xa22)), (0.05, Tier(2, RINGS))],
        0x633 => &[
            (0.1, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.1, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
            (0.05, Tier(2, RINGS)),
            (0.05, Tier(1, ABILITIES)),
            (0.03, Item(0xa23)),
        ],
        0x629 | 0x62b => &[(0.03, Item(0xa22))],
        0x62a | 0x647 | 0x648 => &[(0.03, Item(0xa23))],
        // Orc King: Wild Shadow wiki, snapshot 20120207192955 (portals unchanged).
        0x62c => &[
            (0.16, Tier(3, WEAPONS)),
            (0.08, Tier(4, WEAPONS)),
            (0.04, Tier(5, WEAPONS)),
            (0.16, Tier(4, ARMOR)),
            (0.08, Tier(5, ARMOR)),
            (0.04, Tier(6, ARMOR)),
            (0.10, Tier(2, ABILITIES)),
            (0.03, Item(0xa22)),
            (0.05, Tier(2, RINGS)),
        ],
        0x6c6 => &[(0.04, Item(0xa23))],
        0x900 => &[
            (0.04, Item(0xa22)),
            (0.01, Item(0xa23)),
            (0.01, Tier(4, ARMOR)),
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
        0x612 | 0x613 => &[(0.04, Item(0xa22))],
        0x618 => &[(0.04, Item(0xa22))],
        0x619 => &[(0.03, Item(0xa23))],
        0x953 => &[
            (0.2, Tier(1, WEAPONS)),
            (0.1, Tier(2, WEAPONS)),
            (0.2, Tier(1, ARMOR)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x614 => &[
            (0.30, Tier(3, WEAPONS)),
            (0.30, Tier(3, ARMOR)),
            (0.09, Tier(1, RINGS)),
            (0.38, Tier(1, ABILITIES)),
            (0.02, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x61a => &[
            (0.30, Tier(3, WEAPONS)),
            (0.30, Tier(3, ARMOR)),
            (0.12, Tier(1, RINGS)),
            (0.39, Tier(1, ABILITIES)),
            (0.03, Item(0xa23)),
        ],
        0x60e..=0x610 => &[(0.04, Item(0xa22)), (0.04, Item(0xa23))],
        // https://web.archive.org/web/20111126104739/http://forums.wildshadow.com/wiki/realm-mad-god/cave-pirate-brawler
        0x687..=0x689 => &[(0.03, Item(0xa22))],
        // https://web.archive.org/web/20111126104744/http://forums.wildshadow.com/wiki/realm-mad-god/cave-pirate-cabin-boy
        0x68a..=0x68e => &[(0.2, Tier(1, ABILITIES))],
        0x68f => &[(0.1, Tier(1, WEAPONS)), (0.2, Tier(1, ABILITIES))],
        // https://web.archive.org/web/20111126105041/http://forums.wildshadow.com/wiki/realm-mad-god/pirate-lieutenant
        0x683..=0x686 => &[
            (0.2, Tier(1, ARMOR)),
            (0.1, Tier(2, ARMOR)),
            (0.05, Tier(1, RINGS)),
            (0.2, Tier(2, WEAPONS)),
            (0.1, Tier(3, WEAPONS)),
            (0.005, Item(0xa50)),
        ],
        // https://web.archive.org/web/20120217160929/http://forums.wildshadow.com/wiki/realm-mad-god/dreadstump-pirate-king
        0x927 => &[
            (0.05, Item(0xa50)),
            (0.4, Tier(1, ARMOR)),
            (0.1, Tier(1, RINGS)),
            (0.2, Tier(2, ARMOR)),
            (0.4, Tier(2, WEAPONS)),
            (0.1, Tier(3, ARMOR)),
            (0.2, Tier(3, WEAPONS)),
            (0.05, Tier(4, ARMOR)),
            (0.1, Tier(4, WEAPONS)),
        ],
        0x172
        | 0x173
        | 0x175..=0x179
        | 0x653
        | 0xd07..=0xd0a
        | 0xda2
        | 0xda3
        | 0x918
        | 0x919
        | crate::dungeon::snake::BUTTON
        | crate::dungeon::snake::SPAWNER
        | crate::dungeon::snake::DART => &[],
        0xdaa..=0xdaf => &[(0.25, Item(0xa22)), (0.25, Item(0xa23))],
        0x605 | 0x606 => &[(0.04, Item(0xa22))],
        0x607 => &[(0.03, Item(0xa23))],
        0x615 => &[(0.04, Item(0xa22)), (0.03, Item(0xa23))],
        0x61c | 0x61e | 0x63b => &[(0.03, Item(0xa22)), (0.03, Item(0xa23))],
        // The King is an ordinary elf, not an equipment-bearing lesser god: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Highland.cs#L201-L256
        0x64a..=0x64d | 0x64f => &[(0.03, Item(0xa22))],
        0x60a | 0x60c => &[(0.03, Item(0xa23))],
        0x61d | 0x63c | 0x63f | 0x642..=0x644 => &[(0.05, Item(0xa22)), (0.03, Item(0xa23))],
        0x616
        | 0x63a
        | 0xd91..=0xd94
        | 0xd96
        | 0xd97
        | 0xd9b..=0xd9f
        | 0xda4..=0xda6
        | 0xdb0..=0xdb2 => &[(0.05, Item(0xa22)), (0.03, Item(0xa23))],
        0x60d => &[
            (0.30, Tier(3, WEAPONS)),
            (0.30, Tier(3, ARMOR)),
            (0.11, Tier(1, RINGS)),
            (0.39, Tier(1, ABILITIES)),
            (0.04, Item(0xa22)),
        ],
        0x609 => &[
            (0.36, Tier(2, WEAPONS)),
            (0.10, Tier(3, WEAPONS)),
            (0.36, Tier(2, ARMOR)),
            (0.11, Tier(1, RINGS)),
            (0.39, Tier(1, ABILITIES)),
            (0.02, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        0x617 => &[
            (0.30, Tier(2, WEAPONS)),
            (0.30, Tier(2, ARMOR)),
            (0.11, Tier(1, RINGS)),
            (0.39, Tier(1, ABILITIES)),
            (0.02, Item(0xa22)),
            (0.02, Item(0xa23)),
        ],
        // Dwarf King has no monster-page rates; tier pages fill this pool.
        0x61f => &[
            (0.20, Tier(2, WEAPONS)),
            (0.10, Tier(3, WEAPONS)),
            (0.50, Tier(4, WEAPONS)),
            (0.32, Tier(1, ARMOR)),
            (0.16, Tier(2, ARMOR)),
            (0.08, Tier(3, ARMOR)),
            (0.04, Tier(4, ARMOR)),
            (0.10, Tier(1, RINGS)),
            (0.10, Tier(1, ABILITIES)),
        ],
        0x63e => &[
            (0.1, Tier(3, WEAPONS)),
            (0.05, Tier(4, WEAPONS)),
            (0.1, Tier(3, ARMOR)),
            (0.05, Tier(4, ARMOR)),
            (0.05, Tier(1, RINGS)),
            (0.05, Tier(1, ABILITIES)),
            (0.03, Item(0xa22)),
        ],
        0x645 => &[
            (0.16, Tier(5, WEAPONS)),
            (0.08, Tier(6, WEAPONS)),
            (0.16, Tier(4, ARMOR)),
            (0.08, Tier(5, ARMOR)),
            (0.04, Tier(6, ARMOR)),
            (0.05, Tier(3, RINGS)),
            (0.03, Tier(3, ABILITIES)),
            (0.03, Item(0xa23)),
        ],
        0x640 | 0x649 | 0x650 => &[
            (0.16, Tier(5, WEAPONS)),
            (0.08, Tier(6, WEAPONS)),
            (0.04, Tier(7, WEAPONS)),
            (0.16, Tier(5, ARMOR)),
            (0.08, Tier(6, ARMOR)),
            (0.04, Tier(7, ARMOR)),
            (0.05, Tier(3, RINGS)),
            (0.10, Tier(3, ABILITIES)),
            (0.005, Item(0xaa9)),
        ],
        0x646 | 0x905 => &[
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
        // Leviathan's monster page overrides the general Realm Gods tier-page rates:
        // https://web.archive.org/web/20120215050418/http://forums.wildshadow.com:80/wiki/realm-mad-god/weapons
        0x6d8 => &[
            (0.02, Tier(6, WEAPONS)),
            (0.01, Tier(7, WEAPONS)),
            (0.005, Tier(8, WEAPONS)),
            (0.02, Tier(6, ARMOR)),
            (0.02, Tier(7, ARMOR)),
            (0.01, Tier(8, ARMOR)),
            (0.005, Tier(9, ARMOR)),
            (0.0075, Tier(3, RINGS)),
            (0.005, Tier(4, RINGS)),
            (0.01, Tier(4, ABILITIES)),
        ],
        0x651 | 0x652 | 0x654..=0x659 | 0xd05 | 0x91a => &[
            (0.04, Tier(6, WEAPONS)),
            (0.02, Tier(7, WEAPONS)),
            (0.01, Tier(8, WEAPONS)),
            (0.08, Tier(6, ARMOR)),
            (0.04, Tier(7, ARMOR)),
            (0.02, Tier(8, ARMOR)),
            (0.01, Tier(9, ARMOR)),
            (0.015, Tier(3, RINGS)),
            (0.005, Tier(4, RINGS)),
            (0.02, Tier(4, ABILITIES)),
        ],
        0x223..=0x229 => &[
            (0.1, Item(0xa3f)),
            (0.05, Tier(3, RINGS)),
            (0.05, Tier(6, WEAPONS)),
        ],
        // https://web.archive.org/web/20120127132732/http://forums.wildshadow.com/wiki/realm-mad-god/sprites
        // Keep existing unlisted candidates; absence from the wiki is not disproof.
        0xd00 => &[(0.02, Tier(5, WEAPONS)), (0.05, Item(0xa23))],
        0xd01 => &[
            (0.02, Tier(5, WEAPONS)),
            (0.05, Item(0xa23)),
            (0.04, Tier(2, ABILITIES)),
        ],
        0xd02 => &[
            (0.02, Tier(5, WEAPONS)),
            (0.05, Item(0xa23)),
            (0.01, Tier(6, ARMOR)),
        ],
        0xd03 => &[
            (0.02, Tier(5, WEAPONS)),
            (0.03, Item(0xa23)),
            (0.03, Item(0xa4b)),
            (0.02, Item(0xa2d)),
        ],
        0xd04 => &[
            (0.02, Tier(5, WEAPONS)),
            (0.05, Item(0xa23)),
            (0.03, Item(0xa22)),
            (0.02, Item(0xa4d)),
        ],
        0xd95 | 0xd98..=0xd9a => &[
            (0.2, Tier(5, ARMOR)),
            (0.2, Tier(6, WEAPONS)),
            (0.1, Tier(7, WEAPONS)),
            (0.1, Tier(6, ARMOR)),
            (0.1, Tier(3, ABILITIES)),
            (0.1, Tier(3, RINGS)),
        ],
        // https://web.archive.org/web/20120118035706/http://forums.wildshadow.com/wiki/realm-mad-god/stheno-snake-queen
        0x917 => &[
            (1., Item(0xa21)),
            (0.006, Item(crate::endgame::INCANTATION)),
            (0.001, Item(0xc04)),
            (0.45, Item(0xa3e)),
            (0.45, Item(0xa40)),
            (0.45, Item(0xa41)),
            (0.3, Tier(9, WEAPONS)),
            (0.3, Tier(9, ARMOR)),
            (0.05, Tier(10, WEAPONS)),
            (0.05, Tier(10, ARMOR)),
        ],
        crate::dungeon::snake::GUARD => &[
            (1., Item(0xa21)),
            (0.05, Item(crate::endgame::INCANTATION)),
            (0.05, Item(0xc04)),
            (0.05, Item(0xa3e)),
            (0.05, Item(0xa40)),
            (0.05, Item(0xa41)),
            (0.2, Tier(9, WEAPONS)),
            (0.3, Tier(8, ARMOR)),
        ],
        // https://web.archive.org/web/20120114025209/http://forums.wildshadow.com/wiki/realm-mad-god/limon-sprite-god
        // The cloak rate is marked "???"; preserve its existing reconstruction.
        0xd06 => &[
            (1., Item(0xa4c)),
            (0.1, Item(0xa4b)),
            (0.006, Item(crate::endgame::INCANTATION)),
            (0.001, Item(0xc03)),
            (0.01, Item(0xa5a)),
            (0.05, Tier(7, ARMOR)),
            (0.05, Tier(3, ABILITIES)),
            (0.01, Tier(5, ABILITIES)),
            (0.024, Tier(4, ABILITIES)),
            (0.05, Tier(3, RINGS)),
            // Later comparator's optional bonus, not recovered 123.5.1 odds: https://github.com/ossimc82/fabiano-swagger-of-doom/blob/6fd20aad4a7905b13f25389c68368a942a2b68cb/wServer/logic/db/BehaviorDb.SpriteWorld.cs#L143-L145
            (0.05, Item(0xa20)),
        ],
        // https://web.archive.org/web/20120114021352/http://forums.wildshadow.com/wiki/realm-mad-god/septavius-ghost-god
        0xd90 => &[
            (1., Item(0xa35)),
            (0.006, Item(crate::endgame::INCANTATION)),
            (0.001, Item(0xc02)),
            (0.4, Tier(7, WEAPONS)),
            (0.2, Tier(8, WEAPONS)),
            (0.4, Tier(3, ABILITIES)),
            (0.15, Tier(4, ABILITIES)),
            (0.05, Tier(5, ABILITIES)),
            (0.4, Tier(3, RINGS)),
            (0.12, Tier(4, RINGS)),
        ],
        // https://web.archive.org/web/20120209020521/http://forums.wildshadow.com/wiki/realm-mad-god/imp-abyss
        0x66d => &[
            (0.02, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.01, Item(0xa59)),
            (0.01, Item(0xaa7)),
        ],
        // https://web.archive.org/web/20120216011240/http://forums.wildshadow.com/wiki/realm-mad-god/demon-abyss
        0x66e => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.02, Item(0xa1c)),
            (0.02, Item(0xa12)),
        ],
        // https://web.archive.org/web/20120216044619/http://forums.wildshadow.com/wiki/realm-mad-god/demon-warrior-abyss
        0x66f => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.02, Item(0xa3b)),
            (0.02, Item(0xa0a)),
        ],
        // https://web.archive.org/web/20120215021713/http://forums.wildshadow.com/wiki/realm-mad-god/demon-mage-abyss
        0x670 => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.02, Item(0xa2f)),
            (0.02, Item(0xa3d)),
            (0.02, Item(0xa9d)),
            (0.02, Item(0xa5f)),
            (0.02, Item(0xaae)),
            (0.01, Item(0xab5)),
        ],
        // https://web.archive.org/web/20120215010915/http://forums.wildshadow.com/wiki/realm-mad-god/brute-abyss
        0x671 => &[
            (0.02, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.02, Item(0xa17)),
            (0.01, Item(0xa6a)),
        ],
        // https://web.archive.org/web/20120214232201/http://forums.wildshadow.com/wiki/realm-mad-god/brute-warrior-abyss
        0x672 => &[
            (0.03, Item(0xa22)),
            (0.02, Item(0xa23)),
            (0.02, Item(0xa3c)),
            (0.01, Item(0xa4e)),
            (0.01, Item(0xa64)),
            (0.02, Item(0xa32)),
        ],
        0x908 => &[(0.04, Item(0xa22)), (0.06, Item(0xa23))],
        0x90b => &[(0.1, Item(0xa22)), (0.1, Item(0xa23))],
        0x909 => &[],
        // https://web.archive.org/web/20120207191548/http://forums.wildshadow.com/wiki/realm-mad-god/archdemon-malphas
        // Preserve #244's paired private-potion contract; historical allocation is separate: https://github.com/OpenRotMG/Open-RotMG-Server/issues/244
        crate::dungeon::abyss::MALPHAS => &[
            (1., Item(0xa34)),
            (1., Item(0xa20)),
            (0.006, Item(crate::endgame::INCANTATION)),
            (0.001, Item(0xc01)),
            (0.7, Tier(7, WEAPONS)),
            (0.4, Tier(9, WEAPONS)),
            (0.15, Tier(10, WEAPONS)),
            (0.7, Tier(7, ARMOR)),
            (0.4, Tier(9, ARMOR)),
            (0.15, Tier(10, ARMOR)),
            (0.5, Tier(3, ABILITIES)),
            (0.3, Tier(4, ABILITIES)),
            (0.5, Tier(3, RINGS)),
            (0.3, Tier(4, RINGS)),
        ],
        crate::dungeon::abyss::IDOL => &[
            (1., Item(0xa34)),
            // Restore Trapped's DEF candidate using our VIT guarantee, not its ordered weights: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.Abyss.cs#L224-L235
            (1., Item(0xa20)),
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
        .chain([0x678, 0x67d, 0x668, 0x925].into_iter().flat_map(|kind| {
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
        // Kithio keeps White Demon below the ordinary gods: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Mountain.cs#L15-L35
        0x651 => &[(0.01, 0xa1f)],
        // Native's 1.5% follows period testimony, not the later 2% table: https://discord.com/channels/1558662335076376578/1558662336439386185/1563889316432973895
        0x652 | 0x657 | 0xd05 | 0x905..=0x907 => &[(0.015, 0xa1f)],
        // Djinn uses Kithio's qualified 1.5% roll, not the reconstructed 2%: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Mountain.cs#L334-L343
        0x659 | 0x91a => &[(0.015, 0xa21)],
        0x654 => &[(0.01, 0xa21)],
        0x655 | 0x658 => &[(0.015, 0xa20)],
        0x656 => &[(0.01, 0xa20)],
        0x6d8 => &[(0.025, 0xa20)],
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
pub(crate) fn period_boss(kind: u16) -> bool {
    matches!(kind, 0xd56 | 0xd59 | 0xd60)
}
fn independent(kind: u16) -> bool {
    period_dungeon(kind)
        || period_surface(kind)
        || crate::encounter::tutorial::ROSTER.contains(&kind)
        || kind == crate::dungeon::tomb::TREASURE
        || crate::quest::final_root(kind)
        || matches!(kind, crate::dungeon::trench::HERMIT | 0xd50 | 0xd52 | 0xd53)
        || matches!(kind, 0x66d..=0x672 | 0x908 | 0x90b | 0x65a..=0x65d)
        || matches!(kind, 0xd60 | 0x1705 | crate::dungeon::trench::THESSAL)
        || matches!(
            kind,
            0x917
                | crate::dungeon::snake::GUARD
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
        )
        || matches!(kind, 0x651..=0x659 | 0xd05 | 0x91a | 0x6d8 | 0xd56 | 0xd59)
}
/// Realm god potions need 18% of selected max HP; other god loot needs any hit.
pub(crate) fn potion_damage(kind: u16) -> Option<i32> {
    match kind {
        0x651 | 0x655 | 0x657..=0x659 | 0x91a => Some(180),
        0x656 => Some(270),
        0x652 | 0x654 | 0x6d8 | 0xd05 => Some(360),
        _ => None,
    }
}
pub(crate) fn item_qualified(kind: u16, item: u16, damage: i32, max_hp: i32) -> bool {
    // Kithio LootDefs.Threshold uses >=, not the generic world's strict > gate.
    kind != 0xd90 || item != 0xc02 || i64::from(damage) * 100 >= i64::from(max_hp) * 15
}

fn god_equipment(content: &Content, kind: u16, rng: &mut impl Rng) -> Vec<u16> {
    let mut items = Vec::new();
    for (chance, pick) in table(kind) {
        let pool = candidates(content, *pick);
        assert!(!pool.is_empty(), "validated god loot candidates");
        let probability = chance / pool.len() as f64;
        for item in pool {
            if rng.gen_bool(probability) {
                items.push(item);
            }
        }
    }
    items
}

pub(crate) struct GodLoot {
    pub shared: Vec<u16>,
    pub shared_owners: Vec<i32>,
    pub private: std::collections::BTreeMap<i32, Vec<u16>>,
}

pub(crate) fn god_loot(
    content: &Content,
    kind: u16,
    contributors: &[(i32, i32)],
    rng: &mut impl Rng,
) -> GodLoot {
    let required = potion_damage(kind);
    assert!(!contributors.is_empty(), "god loot needs contributors");
    let mut loot = GodLoot {
        shared: Vec::new(),
        // Private potions must not erase successful gear: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/loot/Loots.cs
        shared_owners: contributors.iter().map(|&(owner, _)| owner).collect(),
        private: std::collections::BTreeMap::new(),
    };
    for item in god_equipment(content, kind, rng) {
        if content.objects[&item].soulbound {
            let owner = contributors.choose(rng).expect("nonempty contributors").0;
            // Do not retain Kithio Loots.cs's duplicate soulbound entry in shared loot.
            loot.private.entry(owner).or_default().push(item);
        } else {
            loot.shared.push(item);
        }
    }
    for &(owner, damage) in contributors {
        if required.is_some_and(|required| damage >= required) {
            for &(chance, item) in bonus(kind) {
                if rng.gen_bool(chance) {
                    loot.private.entry(owner).or_default().push(item);
                }
            }
        }
    }
    loot
}

pub fn rewards(content: &Content, kind: u16, rng: &mut impl Rng) -> Vec<u16> {
    // Sprite tier candidates roll independently, not as an exclusive choice: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/loot/LootDefs.cs#L77-L86
    let per_item = potion_damage(kind).is_some() || matches!(kind, 0x624..=0x626 | 0x91d | 0x91e);
    let mut items = if per_item {
        god_equipment(content, kind, rng)
    } else if independent(kind) {
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
        0x20d | 0x952 | 0x903 | 0x924 => 1,
        0x941 => 8,
        0x949 | 0x950 | 0x945 | 0x957 => 2,
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
        || matches!(kind, crate::dungeon::trench::HERMIT | 0xd50)
        || matches!(
            kind,
            0x679..=0x67c | 0xd5a | 0xd60 | 0x1705 | crate::dungeon::trench::THESSAL
        )
        || matches!(kind, 0x649 | 0x91a | 0x6d8 | 0x905..=0x907 | 0x683..=0x686 | 0x927 | 0x640 | 0x645 | 0x646 | 0x650..=0x659
        | 0xd05 | 0x917 | crate::dungeon::snake::GUARD | 0xd06 | 0xd90 | 0xd95 | 0xd98..=0xd9a
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
    // The June 2012 wiki's Deathmage Chest lists these tiers without rates; rates are realm-src Graveyard.cs:
    // https://web.archive.org/web/20120610073636id_/http://forums.wildshadow.com:80/wiki/realm-mad-god/deathmage
    // https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/realm/setpieces/Graveyard.cs
    if kind == 0x925 {
        return vec![
            (0.3, Tier(4, WEAPONS)),
            (0.2, Tier(5, WEAPONS)),
            (0.1, Tier(6, WEAPONS)),
            (0.3, Tier(3, ARMOR)),
            (0.2, Tier(4, ARMOR)),
            (0.1, Tier(5, ARMOR)),
            (0.3, Tier(1, ABILITIES)),
            (0.2, Tier(2, ABILITIES)),
            (0.2, Tier(3, ABILITIES)),
            (0.25, Tier(1, RINGS)),
            (0.15, Tier(2, RINGS)),
            (0.5, Tier(1, &[10])),
        ];
    }
    // Monster-page chest rates override the generic community-server pools.
    // https://web.archive.org/web/20120217151558/http://forums.wildshadow.com:80/wiki/realm-mad-god/oasis-giant
    let (weapon, armor, tier_rates, ring, ring_rates, ability_rates) = match kind {
        0x675 => (5, 5, [0.75, 0.375, 0.167], 1, [0.6, 0.15], [0.3, 0.075, 0.]),
        0x678 => (5, 4, [0.75, 0.375, 0.167], 1, [0.6, 0.15], [0.3, 0.075, 0.]),
        0x67d => (6, 6, [0.5, 0.25, 0.125], 2, [0.2, 0.05], [0.4, 0.1, 0.025]),
        0x668 => (7, 7, [0.5, 0.25, 0.125], 2, [0.4, 0.1], [0.8, 0.2, 0.05]),
        _ => unreachable!("selected landmark chest"),
    };
    [
        (tier_rates[0], Tier(weapon, WEAPONS)),
        (tier_rates[1], Tier(weapon + 1, WEAPONS)),
        (tier_rates[2], Tier(weapon + 2, WEAPONS)),
        (tier_rates[0], Tier(armor, ARMOR)),
        (tier_rates[1], Tier(armor + 1, ARMOR)),
        (tier_rates[2], Tier(armor + 2, ARMOR)),
        (ability_rates[0], Tier(2, ABILITIES)),
        (ability_rates[1], Tier(3, ABILITIES)),
        (ability_rates[2], Tier(4, ABILITIES)),
        (ring_rates[0], Tier(ring, RINGS)),
        (ring_rates[1], Tier(ring + 1, RINGS)),
        (1., Tier(1, &[10])),
        (1., Tier(1, &[10])),
    ]
    .into_iter()
    .filter(|(chance, _)| *chance > 0.)
    .collect()
}
pub fn landmark_chest(content: &Content, kind: u16, rng: &mut impl Rng) -> Result<Vec<i32>> {
    if !matches!(kind, crate::dungeon::jungle::COIL | 0x925) {
        let mut items = Vec::new();
        for (chance, pick) in chest_table(kind) {
            let pool = candidates(content, pick);
            ensure!(
                !pool.is_empty(),
                "landmark {kind:#x} chest has an empty selected tier"
            );
            if rng.gen_bool(chance) {
                items.push(i32::from(*pool.choose(rng).expect("validated chest tier")));
            }
        }
        return Ok(items);
    }
    let mut consideration = Vec::new();
    for (chance, pick) in chest_table(kind) {
        let pool = candidates(content, pick);
        ensure!(
            !pool.is_empty(),
            "landmark {kind:#x} chest has an empty selected tier"
        );
        // MobDrops.cs expands p/n per item; selected BTree order is not authenticated source order.
        let probability = chance / pool.len() as f64;
        consideration.extend(pool.into_iter().map(|id| (probability, id)));
    }
    // TempleA/B.cs and Graveyard.cs (realm-src@1855c017; data/REALM-SRC-LICENSE) use GetLoots(3, 8) as a success ceiling.
    let maximum = rng.gen_range(3..8);
    let mut items = Vec::new();
    for (chance, id) in consideration {
        if rng.gen_bool(chance) {
            items.push(i32::from(id));
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

    #[test]
    fn realm_sprites_keep_distinct_independent_public_rewards() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, tier, slots, chance) in [
            (0x624, 5, WEAPONS, 0.02),
            (0x625, 2, ABILITIES, 0.04),
            (0x626, 6, ARMOR, 0.01),
        ] {
            assert!(
                matches!(table(kind), [(p, Pick::Tier(t, s)), (q, Pick::Item(0xa23))]
                if *p == chance && *t == tier && *s == slots && *q == 0.05)
            );
            assert!(period_surface(kind) && !private(kind));
            let pool = candidates(&content, Pick::Tier(tier, slots));
            assert!(!pool.is_empty());
            let all = rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0));
            assert_eq!(all.len(), pool.len() + 1);
            assert!(pool.iter().all(|item| all.contains(item)) && all.contains(&0xa23));
            assert!(
                rewards(
                    &content,
                    kind,
                    &mut rand::rngs::mock::StepRng::new(u64::MAX, 0)
                )
                .is_empty()
            );
        }
    }

    #[test]
    fn nomad_has_one_public_four_percent_magic_potion_roll() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        assert!(matches!(table(0x6c6), [(p, Pick::Item(0xa23))] if *p == 0.04));
        assert!(period_surface(0x6c6) && !private(0x6c6));
        assert_eq!(
            rewards(&content, 0x6c6, &mut rand::rngs::mock::StepRng::new(0, 0)),
            vec![0xa23]
        );
        assert!(
            rewards(
                &content,
                0x6c6,
                &mut rand::rngs::mock::StepRng::new(u64::MAX, 0)
            )
            .is_empty()
        );
    }

    #[test]
    fn oasis_chest_armor_keeps_period_tiers() {
        let armor: Vec<_> = chest_table(0x678)
            .into_iter()
            .filter_map(|(chance, pick)| match pick {
                Pick::Tier(tier, slots) if slots == ARMOR => Some((tier, chance)),
                _ => None,
            })
            .collect();
        assert_eq!(armor, [(4, 0.75), (5, 0.375), (6, 0.167)]);
        assert!(table(0x678).is_empty());
    }

    #[test]
    fn shore_cubes_keep_independent_public_potions_and_pirate_cave_eligibility() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for kind in 0x60e..=0x610 {
            assert!(
                matches!(table(kind), [(hp, Pick::Item(0xa22)), (mp, Pick::Item(0xa23))] if *hp == 0.04 && *mp == 0.04)
            );
            assert!(period_surface(kind) && !private(kind));
            assert_eq!(
                rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0)),
                vec![0xa22, 0xa23]
            );
            assert!(
                rewards(
                    &content,
                    kind,
                    &mut rand::rngs::mock::StepRng::new(u64::MAX, 0)
                )
                .is_empty()
            );
            assert_eq!(crate::dungeon::portal_drop(kind), Some((0x717, 0.01)));
        }
    }

    #[test]
    fn night_elves_only_roll_one_public_health_potion() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for kind in [0x64a, 0x64b, 0x64c, 0x64d, 0x64f] {
            assert!(
                matches!(table(kind), [(p, Pick::Item(0xa22))] if *p == 0.03),
                "{kind:#x}"
            );
            assert!(period_surface(kind) && !private(kind));
            assert_eq!(
                rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0)),
                vec![0xa22]
            );
        }
    }

    #[test]
    fn goblin_and_elf_escorts_keep_their_public_potion_rolls() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, chance, item) in [
            (0x605, 0.04, 0xa22),
            (0x606, 0.04, 0xa22),
            (0x607, 0.03, 0xa23),
            (0x612, 0.04, 0xa22),
            (0x613, 0.04, 0xa22),
        ] {
            assert!(matches!(table(kind), [(p, Pick::Item(id))] if *p == chance && *id == item));
            assert!(period_surface(kind) && !private(kind));
            assert_eq!(
                rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0)),
                vec![item]
            );
        }
    }

    #[test]
    fn sandsman_escorts_only_roll_magic_potions() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for kind in [0x60a, 0x60c] {
            assert!(matches!(table(kind), [(chance, Pick::Item(0xa23))] if *chance == 0.03));
            assert_eq!(
                rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0)),
                vec![0xa23]
            );
        }
    }

    #[test]
    fn red_demon_minions_have_separate_public_equipment_rolls() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, expected) in [
            (0x669, vec![(0.02, 0xa05), (0.02, 0xa9a), (0.02, 0xa1c)]),
            (0x66a, vec![(0.03, 0xa1c)]),
            (0x66b, vec![(0.03, 0xa17), (0.02, 0xa0a)]),
        ] {
            let actual: Vec<_> = table(kind)
                .iter()
                .map(|(p, pick)| (*p, candidates(&content, *pick)[0]))
                .collect();
            assert_eq!(actual, expected, "{kind:#x}");
            assert!(!private(kind));
            assert!(period_surface(kind), "one public attempt per death");
            assert_eq!(
                rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0)),
                expected.iter().map(|(_, item)| *item).collect::<Vec<_>>()
            );
        }
        assert!(private(0x668));
    }

    #[test]
    fn lich_followers_keep_distinct_independent_public_rewards() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, expected) in [
            (0x65f, vec![(0.02, 0xa23), (0.02, 0xa32)]),
            (0x660, vec![(0.02, 0xa23), (0.02, 0xa32)]),
            (
                0x661,
                vec![(0.02, 0xa06), (0.02, 0xa9c), (0.02, 0xa02), (0.02, 0xa16)],
            ),
            (0x91d, vec![(0.02, 0xb0f), (0.06, 0xaf1), (0.03, 0xa23)]),
            (
                0x91e,
                vec![
                    (0.02, 0xa23),
                    (0.02, 0xa28),
                    (0.02, 0xa24),
                    (0.06, 0xb0c),
                    (0.09, 0xb0e),
                    (0.04, 0xb0d),
                ],
            ),
        ] {
            validate(&content, kind).unwrap();
            let fixed: Vec<_> = table(kind)
                .iter()
                .filter_map(|(p, pick)| match pick {
                    Pick::Item(item) => Some((*p, *item)),
                    Pick::Tier(..) => None,
                })
                .collect();
            assert_eq!(fixed, expected, "{kind:#x}");
            assert!(!private(kind));
            assert!(period_surface(kind));
            let all = rewards(&content, kind, &mut rand::rngs::mock::StepRng::new(0, 0));
            assert!(expected.iter().all(|(_, item)| all.contains(item)));
            let tier_count = if kind == 0x91e {
                candidates(&content, Pick::Tier(8, WEAPONS)).len()
            } else {
                0
            };
            assert_eq!(all.len(), expected.len() + tier_count);
        }
        assert!(matches!(table(0x91e).first(), Some((p, Pick::Tier(8, WEAPONS))) if *p == 0.02));
    }

    #[test]
    fn ent_follower_rewards_do_not_use_generic_potions() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let mut rng = rand::rngs::mock::StepRng::new(0, 0);
        let observed = [0x921, 0x922].map(|kind| rewards(&content, kind, &mut rng));
        assert_eq!(observed, [vec![0xb0c], vec![]]);
        assert!(matches!(table(0x921), [(chance, Pick::Item(0xb0c))] if *chance == 0.02));
    }

    #[test]
    fn realm_god_potion_boundaries_preserve_equipment() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        for (kind, threshold, chance, potion) in [
            (0x651, 180, 0.01, 0xa1f),
            (0x655, 180, 0.015, 0xa20),
            (0x656, 270, 0.01, 0xa20),
            (0x657, 180, 0.015, 0xa1f),
            (0x658, 180, 0.015, 0xa20),
            (0x6d8, 360, 0.025, 0xa20),
            (0x91a, 180, 0.015, 0xa21),
        ] {
            assert_eq!(bonus(kind), &[(chance, potion)]);
            let owners = [(10, threshold - 1), (20, threshold), (30, threshold + 1)];
            let loot = god_loot(
                &content,
                kind,
                &owners,
                &mut rand::rngs::mock::StepRng::new(0, 0),
            );
            assert!(!loot.private.contains_key(&10), "{kind:#x}");
            for owner in [20, 30] {
                assert_eq!(loot.private[&owner], [potion], "{kind:#x}");
            }
            assert_eq!(loot.shared_owners, [10, 20, 30]);
            let expected: Vec<_> = table(kind)
                .iter()
                .flat_map(|(_, pick)| candidates(&content, *pick))
                .collect();
            assert_eq!(loot.shared, expected, "{kind:#x}");
            assert!(!loot.shared.contains(&potion));
            let empty = god_loot(
                &content,
                kind,
                &owners,
                &mut rand::rngs::mock::StepRng::new(u64::MAX, 0),
            );
            assert!(empty.shared.is_empty() && empty.private.is_empty());
        }
        assert_eq!(crate::dungeon::portal_drop(0x6d8), None);
    }

    #[test]
    fn god_bound_equipment_is_allocated_once_and_never_in_shared_loot() {
        let mut content = Content::load(std::path::Path::new("data")).unwrap();
        let pool = candidates(&content, Pick::Tier(6, WEAPONS));
        for kind in &pool {
            content.objects.get_mut(kind).unwrap().soulbound = true;
        }
        let owners = [(10, 1), (20, 1), (30, 1), (40, 1), (50, 1)];
        let mut bound_success = false;
        for seed in 0..1024 {
            let loot = god_loot(&content, 0x651, &owners, &mut StdRng::seed_from_u64(seed));
            assert!(loot.shared.iter().all(|kind| !pool.contains(kind)));
            let mut allocated = std::collections::BTreeSet::new();
            for (owner, items) in loot.private {
                assert!(owners.iter().any(|(account, _)| *account == owner));
                assert!(loot.shared_owners.contains(&owner));
                for item in items {
                    assert!(pool.contains(&item));
                    assert!(allocated.insert(item), "one allocation per shared success");
                    bound_success = true;
                }
            }
        }
        assert!(bound_success);
    }
}
