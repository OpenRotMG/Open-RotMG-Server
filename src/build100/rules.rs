#[path = "combat.rs"]
pub mod combat;
#[path = "progression.rs"]
pub mod progression;

use anyhow::{Result, ensure};

// Player._-N0 and Projectile's hit branch truncate only after applying the formula.
pub fn attack_damage(base: i32, attack: i32) -> Result<i32> {
    ensure!(base >= 0 && attack >= 0, "negative attack input");
    let damage = f64::from(base) * (0.5 + f64::from(attack) / 75.0 * 1.5);
    ensure!(
        damage <= f64::from(i16::MAX),
        "attack damage exceeds build100 wire range"
    );
    Ok(damage as i32)
}

pub fn defended_damage(damage: i32, defense: i32) -> Result<i32> {
    ensure!(
        (0..=i32::from(i16::MAX)).contains(&damage) && defense >= 0,
        "invalid defense input"
    );
    Ok((f64::from(damage) * 0.25).max(f64::from(damage) - f64::from(defense)) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_contracts_attack_multiplier_and_truncation() {
        for (base, attack, expected) in [
            (100, 0, 50),
            (100, 25, 100),
            (100, 75, 200),
            (3, 0, 1),
            (100, 80, 210),
        ] {
            assert_eq!(attack_damage(base, attack).unwrap(), expected);
        }
        assert!(attack_damage(-1, 0).is_err());
        assert!(attack_damage(1, -1).is_err());
        assert!(attack_damage(i32::MAX, 75).is_err());
    }

    #[test]
    fn historical_contracts_quarter_floor_not_later_fifteen_percent() {
        for (raw, defense, expected) in [
            (100, 0, 100),
            (100, 74, 26),
            (100, 75, 25),
            (100, 76, 25),
            (100, 1000, 25),
            (7, 100, 1),
            (3, 100, 0),
            (0, 0, 0),
        ] {
            assert_eq!(defended_damage(raw, defense).unwrap(), expected);
        }
        assert!(defended_damage(-1, 0).is_err());
        assert!(defended_damage(1, -1).is_err());
        assert!(defended_damage(32768, 0).is_err());
    }
}
