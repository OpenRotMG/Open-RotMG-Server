use crate::content::{Condition, ConditionEffect, Object, Projectile};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, f64::consts::PI};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConditionMask(u32);
impl ConditionMask {
    pub fn from_bits(bits: u32) -> Result<Self> {
        ensure!(bits < (1 << 21), "condition mask contains non-July bits");
        Ok(Self(bits))
    }
    pub fn bit(condition: Condition) -> u32 {
        if condition == Condition::Nothing {
            0
        } else {
            1 << (condition as u8 - 1)
        }
    }
    pub fn bits(self) -> u32 {
        self.0
    }
    pub fn contains(self, condition: Condition) -> bool {
        self.0 & Self::bit(condition) != 0
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionsSnapshot {
    pub now_seconds: f64,
    pub expiries: [Option<f64>; 21],
}

#[derive(Clone, Debug, Default)]
pub struct Conditions {
    expiries: [Option<f64>; 21],
    last_seconds: f64,
}
impl Conditions {
    pub fn restore(snapshot: &ConditionsSnapshot) -> Result<Self> {
        nonnegative(snapshot.now_seconds, "saved condition time")?;
        for end in snapshot.expiries.iter().flatten() {
            ensure!(
                end.is_finite() && *end > snapshot.now_seconds,
                "saved condition expiry is invalid or expired"
            );
        }
        Ok(Self {
            expiries: snapshot.expiries,
            last_seconds: snapshot.now_seconds,
        })
    }
    pub fn snapshot(&self) -> ConditionsSnapshot {
        ConditionsSnapshot {
            now_seconds: self.last_seconds,
            expiries: self.expiries,
        }
    }
    // All times are caller monotonic seconds; XML duration_seconds needs no ms truncation.
    pub fn expire(&mut self, now_seconds: f64) -> Result<ConditionMask> {
        nonnegative(now_seconds, "condition time")?;
        ensure!(
            now_seconds >= self.last_seconds,
            "condition time moved backwards"
        );
        self.last_seconds = now_seconds;
        let mut bits = 0;
        for (i, expiry) in self.expiries.iter_mut().enumerate() {
            if expiry.is_some_and(|end| now_seconds >= end) {
                *expiry = None;
            }
            if expiry.is_some() {
                bits |= 1 << i;
            }
        }
        ConditionMask::from_bits(bits)
    }
    pub fn refresh(&mut self, effect: &ConditionEffect, now_seconds: f64) -> Result<ConditionMask> {
        nonnegative(f64::from(effect.duration_seconds), "condition duration")?;
        nonnegative(now_seconds, "condition time")?;
        let end = now_seconds + f64::from(effect.duration_seconds);
        ensure!(
            end.is_finite() && (effect.duration_seconds == 0.0 || end > now_seconds),
            "condition expiry overflow or lost duration precision"
        );
        self.expire(now_seconds)?;
        if effect.condition != Condition::Nothing && end > now_seconds {
            let expiry = &mut self.expiries[effect.condition as usize - 1];
            *expiry = Some(expiry.map_or(end, |old| old.max(end)));
        }
        self.expire(now_seconds)
    }
    pub fn next_expiry_seconds(&self) -> Option<f64> {
        self.expiries
            .iter()
            .flatten()
            .copied()
            .min_by(f64::total_cmp)
    }
}

fn nonnegative(value: f64, domain: &str) -> Result<()> {
    ensure!(
        value.is_finite() && value >= 0.0,
        "invalid {domain}: expected finite nonnegative value"
    );
    Ok(())
}

pub fn movement_tiles_per_second(speed: i32, conditions: ConditionMask) -> Result<f64> {
    ensure!(speed >= 0, "negative movement speed stat");
    if conditions.contains(Condition::Paralyzed) || conditions.contains(Condition::Paused) {
        return Ok(0.0);
    }
    // Slowed returns before Berserk in Player._-Cd; update applies .8 to input velocity.
    let rate = if conditions.contains(Condition::Slowed) {
        0.005
    } else {
        (0.005 + f64::from(speed) / 75.0 * 0.007)
            * if conditions.contains(Condition::Berserk) {
                1.5
            } else {
                1.0
            }
    };
    Ok(rate * 800.0)
}

pub fn attack_factor(attack: i32, conditions: ConditionMask) -> Result<f64> {
    ensure!(attack >= 0, "negative attack stat");
    // Weak's early return bypasses Damaging in Player._-N0.
    if conditions.contains(Condition::Weak) {
        return Ok(0.5);
    }
    Ok((0.5 + f64::from(attack) / 75.0 * 1.5)
        * if conditions.contains(Condition::Damaging) {
            2.0
        } else {
            1.0
        })
}

pub fn weapon_interval_ms(item: &Object, dexterity: i32, conditions: ConditionMask) -> Result<f64> {
    ensure!(dexterity >= 0, "{}: negative dexterity", item.name);
    let rof = f64::from(
        item.rate_of_fire
            .with_context(|| format!("{} missing root RateOfFire", item.name))?,
    );
    ensure!(
        rof.is_finite() && rof > 0.0,
        "{}: invalid root RateOfFire",
        item.name
    );
    let rate = (0.0015 + f64::from(dexterity) / 75.0 * (0.008 - 0.0015))
        * if conditions.contains(Condition::Berserk) {
            1.5
        } else {
            1.0
        };
    // Player.shoot assigns this Number expression to its int attack-period property.
    let interval = (1.0 / rate) * (1.0 / rof);
    ensure!(
        interval.is_finite() && interval >= 1.0 && interval <= f64::from(i32::MAX),
        "{}: unsupported native int weapon period",
        item.name
    );
    Ok(interval.trunc())
}

pub fn cooldown_ready(now_ms: f64, previous_ms: Option<f64>, interval_ms: f64) -> Result<bool> {
    nonnegative(now_ms, "cooldown time")?;
    ensure!(
        interval_ms.is_finite() && interval_ms > 0.0,
        "invalid cooldown interval"
    );
    if let Some(previous) = previous_ms {
        nonnegative(previous, "previous cooldown time")?;
        ensure!(previous <= now_ms, "cooldown time moved backwards");
        return Ok(now_ms - previous >= interval_ms);
    }
    Ok(true)
}

pub fn ability_ready(
    item: &Object,
    mp: i32,
    max_mp: i32,
    conditions: ConditionMask,
    now_ms: f64,
    previous_ms: Option<f64>,
) -> Result<bool> {
    ensure!(
        max_mp >= 0 && (0..=max_mp).contains(&mp),
        "ability mana outside effective maximum"
    );
    let cost = item
        .mp_cost
        .with_context(|| format!("{} missing root MpCost", item.name))?;
    ensure!(cost >= 0, "{}: negative root MpCost", item.name);
    let ready = cooldown_ready(now_ms, previous_ms, 500.0)?;
    // _-It/UseItem differs from shoot: Stunned is not a blanket ability gate.
    Ok(ready
        && mp >= cost
        && !conditions.contains(Condition::Quiet)
        && !conditions.contains(Condition::Paused))
}

pub enum DamageSource<'a> {
    Root(&'a Object),
    Projectile(&'a Projectile),
}
pub enum AttackScaling {
    Weapon {
        attack: i32,
        conditions: ConditionMask,
    },
    Unscaled,
}
pub fn continuous_damage(
    source: DamageSource<'_>,
    scaling: AttackScaling,
    random: f64,
) -> Result<i32> {
    ensure!(
        random.is_finite() && (0.0..1.0).contains(&random),
        "damage RNG must be in [0,1)"
    );
    let (min, max) = match source {
        DamageSource::Root(item) => item
            .root_damage
            .with_context(|| format!("{} missing root damage", item.name))?,
        DamageSource::Projectile(p) => (p.min, p.max),
    };
    ensure!(
        0 <= min && min <= max && max <= i32::from(i16::MAX),
        "invalid combat damage range"
    );
    let factor = match scaling {
        AttackScaling::Weapon { attack, conditions } => attack_factor(attack, conditions)?,
        AttackScaling::Unscaled => 1.0,
    };
    let damage = (f64::from(min) + f64::from(max - min) * random) * factor;
    ensure!(
        damage.is_finite() && damage <= f64::from(i16::MAX),
        "combat damage exceeds build100 wire range"
    );
    Ok(damage as i32)
}

pub fn projectile_defended_damage(raw: i32, defense: i32, projectile: &Projectile) -> Result<i32> {
    ensure!(defense >= 0, "negative projectile defense");
    super::defended_damage(
        raw,
        if projectile.armor_piercing() {
            0
        } else {
            defense
        },
    )
}

pub fn shot_angles(item: &Object, angle_radians: f64) -> Result<Vec<f64>> {
    ensure!(
        angle_radians.is_finite(),
        "{}: invalid shot angle",
        item.name
    );
    let gap = f64::from(item.arc_gap_radians);
    nonnegative(gap, "shot arc gap radians")?;
    ensure!(
        item.num_projectiles > 0,
        "{}: zero projectile count",
        item.name
    );
    let start = angle_radians - gap * f64::from(item.num_projectiles - 1) / 2.0;
    let mut angle = start;
    (0..item.num_projectiles)
        .map(|_| {
            ensure!(angle.is_finite(), "{}: shot angle overflow", item.name);
            let result = angle;
            angle += gap;
            Ok(result)
        })
        .collect()
}

// Closed bounds of f64 values that round to this f32; odd significands exclude midpoint ties.
pub fn f32_rounding_cell(value: f32) -> Result<(f64, f64)> {
    ensure!(value.is_finite(), "nonfinite shot rounding cell");
    let v = f64::from(value);
    if value == 0.0 {
        // Signed zero has identical shot geometry; include both signs' rounding cells.
        let half = f64::from(f32::from_bits(1)) / 2.0;
        return Ok((-half, half));
    }
    let below = if value == -f32::MAX {
        v - (f64::from(value.next_up()) - v)
    } else {
        f64::from(value.next_down())
    };
    let above = if value == f32::MAX {
        v + (v - f64::from(value.next_down()))
    } else {
        f64::from(value.next_up())
    };
    let (mut low, mut high) = ((below + v) / 2.0, (v + above) / 2.0);
    if value.to_bits() & 1 != 0 {
        low = low.next_up();
        high = high.next_down();
    }
    Ok((low, high))
}

pub fn weapon_burst_angles(item: &Object, angles: &[f32]) -> Result<()> {
    ensure!(
        !angles.is_empty() && item.num_projectiles > 0,
        "empty weapon burst"
    );
    let gap = f64::from(item.arc_gap_radians);
    let center = f64::from(angles[0]) + gap * f64::from(item.num_projectiles - 1) / 2.0;
    let nominal = shot_angles(item, center)?;
    ensure!(angles.len() <= nominal.len(), "excess weapon burst count");
    if gap == 0.0 || nominal.len() == 1 {
        // These source shapes have no spread accumulation or lost ArcGap precision.
        ensure!(
            angles
                .iter()
                .zip(nominal)
                .all(|(&a, expected)| f64::from(a) == expected),
            "weapon burst angle order"
        );
        return Ok(());
    }
    let (gap_low, gap_high) = f32_rounding_cell(item.arc_gap_radians)?;
    let (mut low, mut high) = f32_rounding_cell(angles[0])?;
    for &angle in angles {
        let cell = f32_rounding_cell(angle)?;
        low = low.max(cell.0);
        high = high.min(cell.1);
        ensure!(
            low <= high,
            "weapon burst angle order outside f32 rounding cells"
        );
        // Outward rounding encloses each source Number += operation, not a timing/angle epsilon.
        low = (low + gap_low).next_down();
        high = (high + gap_high).next_up();
        ensure!(
            low.is_finite() && high.is_finite(),
            "weapon burst angle bound overflow"
        );
    }
    Ok(())
}

// Absolute age in seconds reproduces cumulative update time, not frame-by-frame Euler integration.
// None means expired BEFORE movement (including equality); collision/Continues are not executed here.
pub fn projectile_position(
    projectile: &Projectile,
    origin: (f64, f64),
    angle: f64,
    bullet_id: u8,
    age_seconds: f64,
) -> Result<Option<(f64, f64)>> {
    nonnegative(age_seconds, "projectile age")?;
    ensure!(
        origin.0.is_finite() && origin.1.is_finite() && angle.is_finite(),
        "invalid projectile origin/angle"
    );
    ensure!(
        projectile.speed.is_finite() && projectile.speed > 0.0,
        "invalid projectile speed"
    );
    nonnegative(f64::from(projectile.lifetime), "projectile lifetime")?;
    ensure!(
        projectile.amplitude.is_finite() && projectile.frequency.is_finite(),
        "invalid projectile wave"
    );
    ensure!(
        projectile.features.iter().all(|f| [
            "Wavy",
            "Continues",
            "ConditionEffect",
            "ActivateOnEquip"
        ]
        .contains(&f.as_str())),
        "unsupported projectile geometry metadata"
    );
    let lifetime = f64::from(projectile.lifetime);
    if age_seconds >= lifetime {
        return Ok(None);
    }
    let phase = if bullet_id.is_multiple_of(2) { 0.0 } else { PI };
    let distance = f64::from(projectile.speed) * age_seconds;
    // Original update tests Wavy first, then amplitude; Wavy wins when both are declared.
    let direction = if projectile.wavy {
        angle + PI / 64.0 * (phase + 6.0 * PI * age_seconds).sin()
    } else {
        angle
    };
    let wave = if !projectile.wavy && projectile.amplitude != 0.0 {
        f64::from(projectile.amplitude)
            * (phase + age_seconds / lifetime * f64::from(projectile.frequency) * 2.0 * PI).sin()
    } else {
        0.0
    };
    let x = origin.0 + distance * direction.cos() + wave * (angle + PI / 2.0).cos();
    let y = origin.1 + distance * direction.sin() + wave * (angle + PI / 2.0).sin();
    ensure!(
        x.is_finite() && y.is_finite(),
        "projectile position overflow"
    );
    Ok(Some((x, y)))
}

const STAT_IDS: [u8; 8] = [0, 3, 20, 21, 22, 26, 27, 28];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats([i32; 8]);
impl Stats {
    pub fn from_base(base: &BTreeMap<u8, i32>) -> Result<Self> {
        ensure!(
            base.keys().all(|id| STAT_IDS.contains(id)),
            "unknown permanent stat ID"
        );
        let mut values = [0; 8];
        for (i, id) in STAT_IDS.iter().enumerate() {
            values[i] = *base
                .get(id)
                .with_context(|| format!("missing permanent stat {id}"))?;
        }
        let stats = Self(values);
        stats.validate()?;
        Ok(stats)
    }
    fn validate(&self) -> Result<()> {
        for (id, value) in STAT_IDS.iter().zip(self.0) {
            ensure!(value >= 0, "negative effective/base stat {id}: {value}");
        }
        ensure!(self.0[0] > 0, "effective/base maxHP must be positive");
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = (u8, i32)> + '_ {
        STAT_IDS.into_iter().zip(self.0)
    }
    pub fn get(&self, id: u8) -> Result<i32> {
        let index = STAT_IDS
            .iter()
            .position(|key| *key == id)
            .with_context(|| format!("unknown July stat {id}"))?;
        Ok(self.0[index])
    }
}

pub fn equipment_stats(
    base: &Stats,
    slots: &[i32],
    inventory: &[Option<&Object>],
) -> Result<Stats> {
    ensure!(
        slots.len() == inventory.len() && !slots.is_empty(),
        "equipment slot/inventory length mismatch"
    );
    base.validate()?;
    let mut effective = base.clone();
    for (i, (&slot, item)) in slots.iter().zip(inventory).enumerate() {
        ensure!(
            (0..=17).contains(&slot),
            "unknown July slot type {slot} at {i}"
        );
        if let Some(item) = item {
            ensure!(
                item.class == "Equipment",
                "{}: non-equipment in slot {i}",
                item.name
            );
            let item_slot = item
                .slot_type
                .with_context(|| format!("{} missing SlotType", item.name))?;
            ensure!(
                (1..=17).contains(&item_slot) && (slot == 0 || slot == item_slot),
                "{}: wrong item slot {i} (requires {slot}, got {item_slot})",
                item.name
            );
            for &(id, amount) in &item.bonuses {
                let index = STAT_IDS
                    .iter()
                    .position(|key| *key == id)
                    .with_context(|| format!("{}: unknown equip stat {id}", item.name))?;
                if slot != 0 {
                    effective.0[index] = effective.0[index]
                        .checked_add(amount)
                        .with_context(|| format!("{}: equipment stat {id} overflow", item.name))?;
                }
            }
        }
    }
    effective.validate()?;
    Ok(effective)
}

fn sum_with_roundoff(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    let b_part = sum - a;
    (sum, (a - (sum - b_part)) + (b - b_part))
}

// Two-word carry preserves sub-ULP contributions instead of snapping near integers.
fn vital_delta(remainder: f64, roundoff: f64, rate: f64, seconds: f64) -> Result<(f64, f64, f64)> {
    let increment = rate * seconds;
    ensure!(increment.is_finite(), "vital regeneration overflow");
    let product_roundoff = rate.mul_add(seconds, -increment);
    let (sum, error) = sum_with_roundoff(remainder, increment);
    let (sum, mut tail) = sum_with_roundoff(sum, error + roundoff + product_roundoff);
    let mut whole = sum.trunc();
    let mut fraction = sum - whole;
    // A rounded integral high word is not an integer crossing if its low word points back.
    if fraction == 0.0 && sum > 0.0 && tail < 0.0 {
        whole -= 1.0;
        fraction = 1.0;
    } else if fraction == 0.0 && sum < 0.0 && tail > 0.0 {
        whole += 1.0;
        fraction = -1.0;
    }
    (fraction, tail) = sum_with_roundoff(fraction, tail);
    if fraction == 1.0 && tail < 0.0 {
        fraction = 1.0_f64.next_down();
        tail += 1.0 - fraction;
    } else if fraction == -1.0 && tail > 0.0 {
        fraction = (-1.0_f64).next_up();
        tail += -1.0 - fraction;
    }
    Ok((whole, fraction, tail))
}

fn validate_fraction(remainder: f64, roundoff: f64) -> Result<()> {
    ensure!(
        remainder.is_finite() && remainder.abs() < 1.0 && roundoff.is_finite(),
        "invalid fractional vital remainder"
    );
    let ulp = (remainder.next_up() - remainder).max(remainder - remainder.next_down());
    ensure!(roundoff.abs() <= ulp, "invalid fractional vital roundoff");
    let (sum, tail) = sum_with_roundoff(remainder, roundoff);
    ensure!(
        sum.abs() < 1.0 || (sum == 1.0 && tail < 0.0) || (sum == -1.0 && tail > 0.0),
        "fractional vital carry reaches integer boundary"
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    pub hp: i32,
    pub mp: i32,
    pub hp_remainder: f64,
    pub mp_remainder: f64,
    // These low words must travel with their remainders; dropping them restores cadence drift.
    pub hp_roundoff: f64,
    pub mp_roundoff: f64,
}
impl Vitals {
    pub(crate) fn validate(self, stats: &Stats) -> Result<()> {
        stats.validate()?;
        ensure!(
            (0..=stats.get(0)?).contains(&self.hp) && (0..=stats.get(3)?).contains(&self.mp),
            "HP/MP outside effective maxima"
        );
        validate_fraction(self.hp_remainder, self.hp_roundoff)?;
        validate_fraction(self.mp_remainder, self.mp_roundoff)?;
        ensure!(
            self.mp_remainder >= 0.0 && (self.mp_remainder != 0.0 || self.mp_roundoff >= 0.0),
            "negative fractional MP"
        );
        ensure!(
            self.hp != 0 || (self.hp_remainder == 0.0 && self.hp_roundoff == 0.0),
            "dead HP has fractional remainder"
        );
        ensure!(
            self.hp != stats.get(0)?
                || (self.hp_remainder <= 0.0
                    && (self.hp_remainder != 0.0 || self.hp_roundoff <= 0.0)),
            "full HP has positive remainder"
        );
        ensure!(
            self.mp != stats.get(3)? || (self.mp_remainder == 0.0 && self.mp_roundoff == 0.0),
            "full MP has remainder"
        );
        Ok(())
    }
    pub fn rebase(self, old: &Stats, new: &Stats) -> Result<Self> {
        self.validate(old)?;
        new.validate()?;
        let hp = self.hp.min(new.get(0)?);
        let mp = self.mp.min(new.get(3)?);
        let hp_clamped = self.hp > new.get(0)?
            || (hp == new.get(0)?
                && (self.hp_remainder > 0.0
                    || (self.hp_remainder == 0.0 && self.hp_roundoff > 0.0)));
        let mp_clamped = mp == new.get(3)?;
        Ok(Self {
            hp,
            mp,
            hp_remainder: if hp_clamped { 0.0 } else { self.hp_remainder },
            hp_roundoff: if hp_clamped { 0.0 } else { self.hp_roundoff },
            mp_remainder: if mp_clamped { 0.0 } else { self.mp_remainder },
            mp_roundoff: if mp_clamped { 0.0 } else { self.mp_roundoff },
        })
    }
    pub fn heal(self, amount: i32, stats: &Stats, conditions: ConditionMask) -> Result<Self> {
        self.validate(stats)?;
        ensure!(amount >= 0, "negative HP healing");
        if self.hp == 0 || amount == 0 || conditions.contains(Condition::Sick) {
            return Ok(self);
        }
        let hp = i64::from(self.hp) + i64::from(amount);
        let max = stats.get(0)?;
        Ok(Self {
            hp: hp.min(i64::from(max)) as i32,
            hp_remainder: if hp >= i64::from(max) {
                0.0
            } else {
                self.hp_remainder
            },
            hp_roundoff: if hp >= i64::from(max) {
                0.0
            } else {
                self.hp_roundoff
            },
            ..self
        })
    }
    // R: net HP rate, signed remainder; caller must partition steps at condition-expiry boundaries.
    pub fn regenerate(
        self,
        elapsed_seconds: f64,
        stats: &Stats,
        conditions: ConditionMask,
    ) -> Result<Self> {
        self.validate(stats)?;
        nonnegative(elapsed_seconds, "regeneration elapsed seconds")?;
        let hp_rate = if conditions.contains(Condition::Sick) {
            0.0
        } else {
            1.0 + f64::from(stats.get(26)?) / 8.0
                + if conditions.contains(Condition::Healing) {
                    20.0
                } else {
                    0.0
                }
        } - if conditions.contains(Condition::Bleeding) {
            20.0
        } else {
            0.0
        };
        let (hp_whole, mut hp_remainder, mut hp_roundoff) = vital_delta(
            self.hp_remainder,
            self.hp_roundoff,
            hp_rate,
            elapsed_seconds,
        )?;
        let hp_total = f64::from(self.hp) + hp_whole;
        let hp = if self.hp == 0 {
            0
        } else {
            hp_total.clamp(0.0, f64::from(stats.get(0)?)) as i32
        };
        if hp == 0
            || hp_total > f64::from(stats.get(0)?)
            || (hp == stats.get(0)?
                && (hp_remainder > 0.0 || (hp_remainder == 0.0 && hp_roundoff > 0.0)))
        {
            hp_remainder = 0.0;
            hp_roundoff = 0.0;
        }
        let (mp_whole, mut mp_remainder, mut mp_roundoff) = vital_delta(
            self.mp_remainder,
            self.mp_roundoff,
            0.8 + f64::from(stats.get(27)?) / 16.7,
            elapsed_seconds,
        )?;
        let mp_total = f64::from(self.mp) + mp_whole;
        let mp = if conditions.contains(Condition::Quiet) {
            0
        } else {
            mp_total.min(f64::from(stats.get(3)?)) as i32
        };
        if conditions.contains(Condition::Quiet) || mp_total >= f64::from(stats.get(3)?) {
            mp_remainder = 0.0;
            mp_roundoff = 0.0;
        }
        Ok(Self {
            hp,
            mp,
            hp_remainder,
            mp_remainder,
            hp_roundoff,
            mp_roundoff,
        })
    }
}

#[cfg(test)]
#[path = "combat_tests.rs"]
mod tests;
