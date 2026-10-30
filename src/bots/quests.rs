//! Why a level-20 player follows the quest arrow: drops worth having, a crowd already there, or habits that ignore the target.
use super::*;
use std::sync::{LazyLock, Mutex};

const SAMPLES: u32 = 400;
// Catalog units are twelfths of a Defense potion; four Speed potions buy one Defense.
const SPEED_POTION_UNITS: f32 = 3.;
/// Below this, a quest is not worth a detour for someone with reasons to choose.
const INTERESTING: f32 = 50.;
const BORED_MS: u64 = 5 * 60_000;

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

pub(super) struct Appeal {
    pub score: f32,
    pub value: f32,
    pub crowd: usize,
    pub habit: Option<&'static str>,
}
impl Appeal {
    pub fn interesting(&self) -> bool {
        self.score >= INTERESTING
    }
}

pub(super) fn appeal(
    p: &Profile,
    c: &Content,
    kind: u16,
    crowd: usize,
    last_progress: u64,
) -> Appeal {
    // Veterans who recognize valuable bags also know what a quest drops; newcomers judge by the arrow and the crowd.
    let value = if p.knows_blue {
        loot_value(c, kind)
    } else {
        0.
    };
    let habit = if p.role == persona::Role::Newbie {
        Some("follows_arrow")
    } else if persona::seed(p.identity).is_multiple_of(5) {
        Some("closes_realms")
    } else if plan::now().saturating_sub(last_progress) >= BORED_MS {
        Some("bored")
    } else {
        None
    };
    // Realm events are the quest gods; heroes such as Ent Ancients are quests but not events.
    let event = c.objects[&kind].god;
    let score = 40.
        + (value * 2.).min(40.)
        + if event { 20. } else { 0. }
        + crowd.min(8) as f32 * p.companions / 2.
        + if habit.is_some() { 25. } else { 0. };
    Appeal {
        score,
        value,
        crowd,
        habit,
    }
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
