use super::*;
use crate::world::view::{VisibleObject, VisibleProjectile};

pub(super) fn predicted(b: &VisibleProjectile, seconds: f32) -> Option<Pos> {
    let direction = Pos {
        x: b.angle.cos(),
        y: b.angle.sin(),
    };
    // Wave displacement is perpendicular, so projection recovers age without the private birth clock.
    let age = ((b.position.x - b.start.x) * direction.x + (b.position.y - b.start.y) * direction.y)
        * 10000.
        / b.speed;
    let age = age.max(0.) + seconds * 1000.;
    if age >= b.lifetime_ms {
        return None;
    }
    let phase = if b.id.is_multiple_of(2) {
        0.
    } else {
        std::f32::consts::PI
    };
    let wave =
        b.amplitude * (phase + age / b.lifetime_ms * b.frequency * std::f32::consts::TAU).sin();
    let distance = age * b.speed / 10000.;
    Some(Pos {
        x: b.start.x + distance * direction.x - wave * direction.y,
        y: b.start.y + distance * direction.y + wave * direction.x,
    })
}

pub(super) fn speed(v: &PlayerView) -> f32 {
    let ground = v
        .tiles
        .iter()
        .find(|t| (t.x, t.y) == tile(v.position))
        .expect("own tile missing from discovered terrain")
        .ground_speed;
    let flags = v.character.conditions;
    let speed = if flags & crate::content::PARALYZED != 0 {
        0.
    } else if flags & crate::content::SLOWED != 0 {
        4.
    } else {
        (4. + 5.6 * v.character.stats[4] as f32 / 75.)
            * if flags & crate::content::SPEEDY != 0 {
                1.5
            } else {
                1.
            }
    };
    speed * ground
}

pub(super) fn clearance(v: &PlayerView, destination: Pos) -> f32 {
    let speed = speed(v);
    let distance = v.position.distance(destination);
    let mut minimum = 20_f32;
    for step in 0..=24 {
        let seconds = step as f32 * 0.025;
        let fraction = if distance > 0. {
            (seconds * speed / distance).min(1.)
        } else {
            0.
        };
        let player = Pos {
            x: v.position.x + (destination.x - v.position.x) * fraction,
            y: v.position.y + (destination.y - v.position.y) * fraction,
        };
        for bullet in v.projectiles.iter().filter(|b| hostile(v, b.owner)) {
            if let Some(pos) = predicted(bullet, seconds) {
                minimum = minimum.min(player.distance(pos));
            }
        }
    }
    minimum
}

pub(super) fn lead(
    v: &PlayerView,
    target: &VisibleObject,
    velocity: Pos,
    projectile: &crate::content::Projectile,
) -> Pos {
    let mut point = target.position;
    for _ in 0..3 {
        let seconds =
            (v.position.distance(point) * 10. / projectile.speed).min(projectile.lifetime / 1000.);
        point = Pos {
            x: target.position.x + velocity.x * seconds,
            y: target.position.y + velocity.y * seconds,
        };
    }
    point
}

pub(super) fn target<'a>(
    v: &PlayerView,
    content: &Content,
    grid: &Grid,
    enemies: &[&'a VisibleObject],
    range: f32,
    passes_cover: bool,
) -> Option<&'a VisibleObject> {
    enemies
        .iter()
        .copied()
        .filter(|enemy| {
            progression::can_fight(v, content, enemy)
                && progression::vulnerable(enemy)
                && v.position.distance(enemy.position) < range
                && shot_clear(v, grid, enemy.position, passes_cover)
        })
        .max_by(|a, b| {
            (a.max_hp as f32 / (1. + v.position.distance(a.position)))
                .total_cmp(&(b.max_hp as f32 / (1. + v.position.distance(b.position))))
        })
}

pub(super) fn attack(
    v: &PlayerView,
    content: &Content,
    grid: &Grid,
    enemy: &VisibleObject,
    velocity: Pos,
    decision: &mut Decision,
) -> Result<()> {
    if !progression::vulnerable(enemy) {
        return Ok(());
    }
    let weapon = content.get(v.character.items[0] as u16)?;
    let projectile = weapon
        .projectiles
        .get(&0)
        .context("weapon projectile missing")?;
    let point = lead(v, enemy, velocity, projectile);
    let range = projectile.speed * projectile.lifetime / 10000.;
    if v.character.conditions & crate::content::STUNNED == 0
        && v.position.distance(point) < range
        && shot_clear(v, grid, point, projectile.passes_cover)
    {
        decision.aim = Some(angle(v.position, point));
    }
    if decision.item.is_none()
        && v.character.items[1] >= 0
        && v.time_ms >= v.character.ability_ready_ms
    {
        let ability = content.get(v.character.items[1] as u16)?;
        if ability.activate == "BulletNova"
            && v.character.conditions & crate::condition::bit(crate::condition::QUIET) == 0
            && v.character.mp >= ability.mana_cost
            && (v.map.name != "Snake Pit"
                || v.quest == Some(enemy.id)
                || v.character.mp - ability.mana_cost >= ability.mana_cost)
            && enemy.hp > projectile.max * 2
            && v.position.distance(enemy.position) <= 15.
            && shot_clear(v, grid, enemy.position, false)
        {
            // Nova bullets originate at the cursor, not after the weapon's flight time.
            decision.item = Some(Action::Use {
                slot: 1,
                x: enemy.position.x,
                y: enemy.position.y,
            });
        }
    }
    Ok(())
}

pub(super) fn movement(
    v: &PlayerView,
    grid: &Grid,
    enemies: &[&VisibleObject],
    target: Option<Pos>,
    range: f32,
    retreat: bool,
) -> Option<Pos> {
    if v.character.conditions & crate::content::PARALYZED != 0 {
        return None;
    }
    let mut best = None;
    for index in 0..16 {
        let direction = index as f32 * std::f32::consts::TAU / 16.;
        let point = Pos {
            x: v.position.x + direction.cos() * 1.25,
            y: v.position.y + direction.sin() * 1.25,
        };
        if !grid.segment(v.position, point) {
            continue;
        }
        let safety = clearance(v, point);
        let separation = enemies
            .iter()
            .map(|e| point.distance(e.position))
            .fold(20., f32::min);
        let spacing = if retreat {
            separation.min(12.) * 3.
        } else {
            target.map_or(0., |p| -(point.distance(p) - range * 0.8).abs() * 3.)
        };
        let score = safety.min(4.) * 12. + separation.min(2.) * 5. + spacing;
        if best.is_none_or(|(prior, _)| score > prior) {
            best = Some((score, point));
        }
    }
    best.map(|(_, point)| point)
}
