//! Why a level-20 player leaves the Godlands for the quest arrow: an event worth joining, or a quest-hunting habit.
use super::*;
use std::sync::{LazyLock, Mutex};

const SAMPLES: u32 = 400;
// Catalog units are twelfths of a Defense potion; four Speed potions buy one Defense.
const SPEED_POTION_UNITS: f32 = 3.;
/// Below this, a quest is not worth a detour for someone with reasons to choose.
const INTERESTING: f32 = 50.;

static VALUES: LazyLock<Mutex<BTreeMap<u16, f32>>> = LazyLock::new(Default::default);

/// Expected priced drops in Speed potions. Unpriced items count nothing: the price table is all a bot knows of worth.
pub(super) fn loot_value(c: &Content, kind: u16) -> f32 {
    if crate::encounter::profile(kind).is_none() {
        return 0.;
    }
    *VALUES
        .lock()
        .expect("quest loot values poisoned")
        .entry(kind)
        .or_insert_with(|| {
            let mut rng = rand::rngs::StdRng::seed_from_u64(u64::from(kind));
            let units: u64 = (0..SAMPLES)
                .flat_map(|_| crate::loot::rewards(c, kind, &mut rng))
                .filter_map(|item| c.item_values.quote_range(i32::from(item)).ok())
                .map(|range| (range.min + range.max) / 2)
                .sum();
            units as f32 / SAMPLES as f32 / SPEED_POTION_UNITS
        })
}

/// Owner decision: about one level 20 in ten hunts quests; the rest farm the Godlands.
pub(super) fn hunts_quests(p: &Profile) -> bool {
    // High bits: the low bits already pick other persona traits.
    (persona::seed(p.identity) >> 32).is_multiple_of(10)
}

pub(super) struct Appeal {
    pub score: f32,
    pub value: f32,
    pub crowd: usize,
    pub event: bool,
    pub habit: Option<&'static str>,
}
impl Appeal {
    pub fn interesting(&self) -> bool {
        self.score >= INTERESTING
    }
    /// Quest hunters follow the arrow; Godlands farmers leave only for an event they or a crowd can take on.
    pub fn joins(&self, solo: bool) -> bool {
        self.interesting() && (self.habit.is_some() || self.event && (solo || self.crowd >= 3))
    }
}

pub(super) fn appeal(p: &Profile, c: &Content, kind: u16, crowd: usize) -> Appeal {
    // Veterans who recognize valuable bags also know what a quest drops; newcomers judge by the arrow and the crowd.
    let value = if p.knows_blue {
        loot_value(c, kind)
    } else {
        0.
    };
    let habit = hunts_quests(p).then_some("hunts_quests");
    // Live: 93 of 97 level-20 quest follows were Red Demon, a god-tagged hero; only Oryx's events draw crowds.
    let event = c.objects[&kind].encounter;
    // In a 2012 realm of 70 only 5-8 players did midland quests; most level 20s farmed the Godlands.
    let score = 40.
        + (value * 2.).min(40.)
        + if event { 20. } else { 0. }
        + crowd.min(8) as f32 * p.companions / 2.
        + if habit.is_some() { 25. } else { 0. };
    Appeal {
        score,
        value,
        crowd,
        event,
        habit,
    }
}

#[cfg(test)]
#[test]
fn red_demon_is_a_hero_not_an_event() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut p = super::ARCHETYPES[1];
    p.knows_blue = false;
    p.role = persona::Role::Grinder;
    let score = |kind| appeal(&p, &c, kind, 0).score;
    assert_eq!(
        score(0x668),
        score(0x678),
        "Red Demon scores like Oasis Giant"
    );
    assert_eq!(score(0xd59), score(0x678) + 20., "Cube God is an event");
}

#[cfg(test)]
#[test]
fn calibration_print() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    for kind in [
        0xd50u16, 0xd54, 0xe37, 0xd5f, 0xd59, 0xd56, 0xd61, 0x675, 0x91f, 0x91b, 0x928, 0x678,
        0x67d, 0x654, 0x655, 0x656, 0x91a,
    ] {
        println!(
            "CAL {:#x} {} value={:.2} god={} quest={}",
            kind,
            c.objects[&kind].name,
            loot_value(&c, kind),
            c.objects[&kind].god,
            c.objects[&kind].quest
        );
    }
}
