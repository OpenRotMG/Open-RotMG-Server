use crate::content::{Content, Loot, Object};
use anyhow::{Context, Result, ensure};
use rand::Rng;
use std::collections::{BTreeMap, BTreeSet};

const PERMANENT_STATS: [u8; 8] = [0, 3, 20, 21, 22, 26, 27, 28];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub level: u8,
    pub xp: i32,
    // Equipment-effective stats must never be persisted or capped here.
    pub base: BTreeMap<u8, i32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrowthAward {
    pub progress: Progress,
    pub max_hp_delta: i32,
    pub max_mp_delta: i32,
}

pub fn xp_goal(level: u8) -> Result<i32> {
    ensure!((1..=20).contains(&level), "invalid July level {level}");
    Ok(50 + 100 * (i32::from(level) - 1))
}

impl Progress {
    pub fn validate(&self, class: &Object) -> Result<()> {
        let goal = xp_goal(self.level)?;
        ensure!(
            self.xp >= 0
                && if self.level == 20 {
                    self.xp == 0
                } else {
                    self.xp < goal
                },
            "{}: invalid level {} progress {}",
            class.name,
            self.level,
            self.xp
        );
        ensure!(
            class.class == "Player",
            "{}: growth requires Player class",
            class.name
        );
        ensure!(
            self.base.len() == 8 && self.base.keys().all(|id| PERMANENT_STATS.contains(id)),
            "{}: expected exactly eight permanent base stats",
            class.name
        );
        let mut seen = BTreeSet::new();
        for &(id, min, max, cap) in &class.growth {
            ensure!(
                PERMANENT_STATS.contains(&id) && seen.insert(id),
                "{}: unknown or duplicate growth stat {id}",
                class.name
            );
            let initial = class
                .stats
                .get(&id)
                .with_context(|| format!("{}: missing initial stat {id}", class.name))?;
            ensure!(
                min >= 0 && min <= max && cap >= 0 && *initial >= 0 && *initial <= cap,
                "{}: invalid growth range/cap/initial stat {id}",
                class.name
            );
            let base = self.base[&id];
            ensure!(
                (0..=cap).contains(&base) && (id != 0 || (base > 0 && *initial > 0)),
                "{}: permanent stat {id} outside class cap {cap}",
                class.name
            );
        }
        ensure!(seen.len() == 8, "{}: incomplete class growth", class.name);
        Ok(())
    }
}

// Frozen R: per-level progress, inclusive XML-order rolls even at cap, no refill or fame.
pub fn award_xp(
    current: &Progress,
    class: &Object,
    amount: i32,
    rng: &mut impl Rng,
) -> Result<GrowthAward> {
    current.validate(class)?;
    ensure!(amount >= 0, "{}: negative XP award {amount}", class.name);
    let mut progress = current.clone();
    let mut remaining = i64::from(current.xp) + i64::from(amount);
    while progress.level < 20 && remaining >= i64::from(xp_goal(progress.level)?) {
        remaining -= i64::from(xp_goal(progress.level)?);
        progress.level += 1;
        for &(id, min, max, cap) in &class.growth {
            let roll = rng.gen_range(min..=max);
            let value = i64::from(progress.base[&id]) + i64::from(roll);
            progress
                .base
                .insert(id, i32::try_from(value.min(i64::from(cap)))?);
        }
    }
    progress.xp = if progress.level == 20 {
        0
    } else {
        i32::try_from(remaining)?
    };
    Ok(GrowthAward {
        max_hp_delta: progress.base[&0] - current.base[&0],
        max_mp_delta: progress.base[&3] - current.base[&3],
        progress,
    })
}

pub fn enemy_xp(enemy: &Object) -> Result<i32> {
    let hp = *enemy
        .stats
        .get(&0)
        .with_context(|| format!("{}: missing XP MaxHitPoints", enemy.name))?;
    let multiplier = f64::from(enemy.xp_multiplier);
    ensure!(hp > 0, "{}: invalid XP MaxHitPoints {hp}", enemy.name);
    ensure!(
        multiplier.is_finite() && multiplier >= 0.0,
        "{}: invalid XpMult",
        enemy.name
    );
    if multiplier == 0.0 {
        return Ok(0);
    }
    let amount = (f64::from(hp) / 10.0 * multiplier).floor().max(1.0);
    ensure!(
        amount.is_finite() && amount <= f64::from(i32::MAX),
        "{}: XP award exceeds build100 i32 wire range",
        enemy.name
    );
    Ok(amount as i32)
}

#[derive(Clone, Copy, Debug)]
pub struct RewardRecipient {
    pub instance: i32,
    pub position: (f32, f32),
    pub living: bool,
    pub paused: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct EnemyDeath {
    pub instance: i32,
    pub position: (f32, f32),
    pub ordinary_enemy: bool,
    pub player_attributed: bool,
    pub summoned: bool,
}

// Attribution and once-only death ownership are the caller's transaction, not inferred here.
pub fn reward_eligible(player: RewardRecipient, death: EnemyDeath) -> Result<bool> {
    ensure!(
        [
            player.position.0,
            player.position.1,
            death.position.0,
            death.position.1
        ]
        .into_iter()
        .all(f32::is_finite),
        "nonfinite reward position"
    );
    let dx = f64::from(player.position.0) - f64::from(death.position.0);
    let dy = f64::from(player.position.1) - f64::from(death.position.1);
    let distance_squared = dx * dx + dy * dy;
    ensure!(distance_squared.is_finite(), "reward distance overflow");
    Ok(player.instance == death.instance
        && player.living
        && !player.paused
        && death.ordinary_enemy
        && death.player_attributed
        && !death.summoned
        && distance_squared < 16.0 * 16.0)
}

pub type LootBag = [Option<i16>; 8];

pub fn roll_loot(source: &Object, content: &Content, rng: &mut impl Rng) -> Result<Vec<LootBag>> {
    let count = source
        .loot
        .iter()
        .filter(|entry| matches!(entry, Loot::Item { .. }))
        .count();
    ensure!(
        count <= 64,
        "{}: {count} declared loot items exceeds 64",
        source.name
    );
    let mut entries = Vec::with_capacity(count);
    for (index, entry) in source.loot.iter().enumerate() {
        if let Loot::Item { name, probability } = entry {
            ensure!(
                probability.is_finite() && (0.0..=1.0).contains(probability),
                "{}: Loot {index} {name}: invalid probability",
                source.name
            );
            let item = content
                .named(name)
                .with_context(|| format!("{}: Loot {index} {name}", source.name))?;
            ensure!(
                item.class == "Equipment" && item.id >= 0,
                "{}: Loot {index} {name}: expected nonnegative Equipment ID",
                source.name
            );
            entries.push((item.id, f64::from(*probability)));
        }
    }
    let mut bags: Vec<LootBag> = Vec::new();
    let mut rolled = 0;
    for (id, probability) in entries {
        // Even p0/p1 consume one f64 uniform; empty markers consume none.
        if rng.r#gen::<f64>() < probability {
            if rolled % 8 == 0 {
                bags.push([None; 8]);
            }
            bags[rolled / 8][rolled % 8] = Some(id);
            rolled += 1;
        }
    }
    Ok(bags)
}

#[cfg(test)]
#[path = "progression_tests.rs"]
mod tests;
