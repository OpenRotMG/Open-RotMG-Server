use super::*;
use crate::store::{Character, experience_goal, level_start};
use rand::seq::SliceRandom;
use serde::Deserialize;

use std::sync::LazyLock;

// Class quest thresholds; each reached threshold is one account star.
const THRESHOLDS: [i32; 5] = [20, 150, 400, 800, 2000];
// Potions raise attack and dexterity first, as players usually did.
const MAX_ORDER: [usize; 8] = [2, 5, 4, 3, 6, 7, 0, 1];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Newbie,
    Grinder,
    Trader,
    Rusher,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    stars: i32,
    role: Role,
}

/// Referenced names; careers, current lives, gear and wealth are synthetic, not those players' histories.
pub(crate) static PERSONAS: LazyLock<Vec<Profile>> = LazyLock::new(|| {
    let entries: Vec<Entry> = serde_json::from_str(include_str!("../../data/bot-personas.json"))
        .expect("data/bot-personas.json is a valid persona list");
    entries.into_iter().map(profile).collect()
});

fn profile(e: Entry) -> Profile {
    assert!(
        (0..=65).contains(&e.stars),
        "{} has impossible stars",
        e.name
    );
    // Skill follows how much someone has played; risk and wants follow what they play for.
    let skill = e.stars as f32 / 65.;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed(&e.name));
    let mut jitter = |spread: f32| rng.gen_range(-spread..=spread);
    let (recovery, resources, companions) = match e.role {
        Role::Newbie => (0.5, 3., 7.),
        Role::Grinder => (0.48, 4., 6.),
        Role::Trader => (0.55, 8., 3.),
        Role::Rusher => (0.36, 6., 5.),
    };
    Profile {
        identity: Box::leak(format!("persona:v1:{}", e.name).into_boxed_str()),
        name: Box::leak(e.name.into_boxed_str()),
        recovery: (recovery + jitter(0.06)).clamp(0.3, 0.7),
        resources: resources + jitter(1.),
        companions: companions + jitter(1.),
        reaction: (6. - 3. * skill + jitter(0.6)).round().clamp(3., 7.) as u64,
        attention: (2. + 6. * skill + jitter(1.)).round().clamp(2., 8.) as usize,
        velocity_step: (2. - 1.75 * skill).clamp(0.25, 2.),
        directions: if skill > 0.5 {
            16
        } else if skill > 0.15 {
            8
        } else {
            4
        },
        pickup_clearance: (1.5 - 0.85 * skill + jitter(0.1)).clamp(0.65, 1.5),
        knows_blue: skill > 0.4,
        knows_minimap: e.role != Role::Newbie,
        stars: e.stars,
        role: e.role,
    }
}

pub(super) fn seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// The characters and vault a persona owns at this point of its 2012 life.
pub(super) fn life(
    p: &Profile,
    content: &Content,
    rng: &mut impl Rng,
) -> Result<(Vec<Character>, Vec<[i32; 8]>)> {
    // Keep synthetic account history independent of current-life gear sampling.
    let mut history = rand::rngs::StdRng::seed_from_u64(seed(p.identity));
    let mut classes: Vec<u16> = content.classes.keys().copied().collect();
    classes.shuffle(&mut history);
    let mut roster = Vec::new();
    if p.stars == 0 || p.role == Role::Trader {
        let level = if p.role == Role::Trader {
            1
        } else {
            rng.gen_range(1..=19)
        };
        let exp = level_start(level)? + rng.gen_range(0..experience_goal(level)?);
        roster.push(character(content, classes[0], level, exp, p.stars, rng)?);
    }
    let mut left = p.stars;
    let count = classes.len() as i32;
    for (index, class) in classes.into_iter().enumerate() {
        if left == 0 {
            break;
        }
        // Leave no more stars than the remaining classes can hold.
        let fewest = (left - 5 * (count - 1 - index as i32)).max(1);
        let stars = history.gen_range(fewest..=left.min(5));
        left -= stars;
        let floor = THRESHOLDS[stars as usize - 1];
        let ceiling = THRESHOLDS.get(stars as usize).copied().unwrap_or(4000);
        let fame = history.gen_range(floor..ceiling);
        let exp = fame * 1000 + history.gen_range(0..1000);
        roster.push(character(content, class, 20, exp, p.stars, rng)?);
    }
    ensure!(left == 0, "{} cannot hold {} stars", p.name, p.stars);
    // Account fame must not force every adventurer's current character to be level 20.
    if p.stars > 0 && p.role != Role::Trader && seed(p.identity) % 2 == 1 {
        let class = roster[0].class;
        for ch in &mut roster {
            ch.alive = false;
            ch.hp = 0;
            ch.items.fill(-1);
        }
        let level = rng.gen_range(1..=19);
        let exp = level_start(level)? + rng.gen_range(0..experience_goal(level)?);
        roster.insert(0, character(content, class, level, exp, p.stars, rng)?);
    }
    let mut needed = BTreeMap::new();
    if p.stars >= 26 {
        // Veteran presets own an unlocked roster, not fabricated kills or deaths.
        for &class in content.classes.keys() {
            prerequisites(content, class, &mut needed);
        }
    } else {
        for ch in &roster {
            prerequisites(content, ch.class, &mut needed);
        }
    }
    for (class, level) in needed {
        if roster
            .iter()
            .any(|ch| ch.class == class && ch.level >= level)
        {
            continue;
        }
        let mut archived = character(content, class, level, level_start(level)?, 0, rng)?;
        archived.alive = false;
        archived.hp = 0;
        archived.items.fill(-1);
        roster.push(archived);
    }
    Ok((roster, vault(p, content, rng)?))
}

pub(super) fn starting_stock(p: &Profile, content: &Content) -> Result<Option<i32>> {
    (p.role == Role::Trader)
        .then(|| {
            named(
                content,
                ["Potion of Speed", "Potion of Defense", "Potion of Life"]
                    [(seed(p.identity) % 3) as usize],
            )
        })
        .transpose()
}

fn prerequisites(content: &Content, class: u16, needed: &mut BTreeMap<u16, i32>) {
    for &(required, level) in &content.classes[&class].requires {
        let prior = needed.entry(required).or_insert(0);
        if *prior < level {
            *prior = level;
            prerequisites(content, required, needed);
        }
    }
}

fn character(
    content: &Content,
    class: u16,
    level: i32,
    exp: i32,
    stars: i32,
    rng: &mut impl Rng,
) -> Result<Character> {
    let definition = content.get(class)?;
    let rules = &content.classes[&class];
    let mut ch = Character::fresh(definition)?;
    ch.lifetime = crate::store::FameLifetime::known_only();
    ch.level = level;
    ch.exp = exp;
    ch.fame = exp / 1000;
    for (i, (min, max)) in rules.growth.iter().enumerate() {
        for _ in 1..level {
            ch.base[i] += rng.gen_range(*min..=*max);
        }
        ch.base[i] = ch.base[i].min(rules.caps[i]);
    }
    let maxed = if level == 20 {
        (stars * 8 / 45).min(8)
    } else {
        0
    };
    for i in MAX_ORDER.into_iter().take(maxed as usize) {
        ch.base[i] = rules.caps[i];
    }
    // Experienced accounts can equip a leveling character without granting maxed stats.
    let veteran = level == 20 || stars >= 26;
    let tiers = [
        if veteran {
            8 + stars / 13
        } else {
            (level - 1) / 3
        },
        if veteran {
            2 + stars / 16
        } else {
            (level - 1) / 6
        },
        if veteran {
            9 + stars / 13
        } else {
            (level - 1) / 3
        },
        if veteran {
            1 + stars / 16
        } else {
            (level - 10) / 4 + 1
        },
    ];
    for (slot, tier) in tiers.into_iter().enumerate() {
        ch.items[slot] = if slot == 2 && tier == 0 || slot == 3 && !veteran && level < 10 {
            -1
        } else {
            i32::from(equipment(content, definition.slots[slot], tier)?)
        };
    }
    let health = named(content, "Health Potion")?;
    let magic = named(content, "Magic Potion")?;
    ch.items[4..8].copy_from_slice(&[health, health, magic, -1]);
    let effective = ch.effective(content)?;
    (ch.hp, ch.mp) = (effective[0], effective[1]);
    Ok(ch)
}

/// The best tiered item for `slot_type` not above `tier`, matching the 2012 tier caps.
fn equipment(content: &Content, slot_type: i32, tier: i32) -> Result<u16> {
    let cap = match slot_type {
        6 | 7 | 14 => 13,
        1 | 2 | 3 | 8 | 17 => 12,
        _ => 5,
    };
    let tier = tier.clamp(0, cap);
    content
        .objects
        .values()
        .filter(|o| o.slot_type == slot_type && !o.unsupported_equipment)
        .filter_map(|o| o.tier.map(|t| (i32::from(t), o.kind)))
        .filter(|(t, _)| *t <= tier)
        .max()
        .map(|(_, kind)| kind)
        .with_context(|| format!("no tiered equipment for slot type {slot_type}"))
}

fn named(content: &Content, name: &str) -> Result<i32> {
    content
        .objects
        .values()
        .find(|o| o.name == name)
        .map(|o| i32::from(o.kind))
        .with_context(|| format!("{name} missing from objects.xml"))
}

fn vault(p: &Profile, content: &Content, rng: &mut impl Rng) -> Result<Vec<[i32; 8]>> {
    let mut items = Vec::new();
    if p.stars >= 13 {
        let wealth = p.stars as usize * if p.role == Role::Trader { 2 } else { 1 };
        for (name, per) in [
            ("Potion of Life", 12),
            ("Potion of Defense", 5),
            ("Potion of Speed", 4),
        ] {
            items.extend(std::iter::repeat_n(named(content, name)?, wealth / per));
        }
        for name in [
            "Potion of Attack",
            "Potion of Dexterity",
            "Potion of Vitality",
            "Potion of Wisdom",
        ] {
            items.extend(std::iter::repeat_n(
                named(content, name)?,
                rng.gen_range(0..=wealth / 8),
            ));
        }
    }
    let mut chests: Vec<[i32; 8]> = items
        .chunks(8)
        .map(|chunk| {
            let mut chest = [-1; 8];
            chest[..chunk.len()].copy_from_slice(chunk);
            chest
        })
        .collect();
    if chests.is_empty() {
        chests.push([-1; 8]);
    }
    ensure!(
        chests.len() <= 20,
        "{} owns more than the 20 purchasable vault chests",
        p.name
    );
    Ok(chests)
}
