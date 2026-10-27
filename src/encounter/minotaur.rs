use super::{
    Motion,
    attacks::{Attack, Schedule},
    nr_speed,
};

// Kithio's charge is fast Follow, not locked Charge; transitions run the old state for one more tick: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Highland.cs#L15-L57
pub(super) fn phase(elapsed: u64) -> (u64, u64) {
    let age = elapsed % 9350;
    let (phase, start) = match age {
        0..50 => (0, 0),
        50..300 => (1, 50),
        300..4750 => (2, 300),
        4750..8300 => (3, 4750),
        _ => (4, 8300),
    };
    (elapsed / 9350 * 5 + phase, age - start)
}

pub(super) fn movement(elapsed: u64) -> (Motion, f32) {
    match phase(elapsed).0 % 5 {
        0 => (Motion::Stationary, 0.),
        phase @ (1 | 3) => (
            Motion::Follow {
                sight: if phase == 1 { 11. } else { 10. },
                stop: if phase == 1 { 1.6 } else { 1. },
                idle: if phase == 1 { 0. } else { nr_speed(0.6) },
                duration: 0,
                cooldown: 0,
            },
            nr_speed(if phase == 1 { 6. } else { 0.6 }),
        ),
        _ => (Motion::Wander, nr_speed(0.6)),
    }
}

pub(super) fn schedule(elapsed: u64) -> Schedule {
    let (state, age) = phase(elapsed);
    let attacks = match state % 5 {
        2 => [
            (1000, 1, 0., 0.),
            (1200, 2, 16., 0.),
            (1600, 3, 120., 0.2),
            (2200, 1, 0., 0.),
            (2800, 2, 180., 0.2),
            (3200, 3, 16., 0.),
        ]
        .into_iter()
        .map(|(delay, count, gap, predict)| {
            Attack::fan(0, count, gap, 50, 8.)
                .predict(predict)
                .delay(delay)
                .once()
        })
        .collect(),
        3 => vec![
            Attack::fan(1, 12, 30., 850, 0.).fixed(0.).delay(600),
            Attack::fan(2, 6, 60., 850, 0.)
                .fixed(15f32.to_radians())
                .delay(1000),
        ],
        _ => vec![],
    };
    Schedule {
        state,
        age,
        attacks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encounter::attacks::{Action, Clocks};

    #[test]
    fn minotaur_repertoire_repeats_without_rearming_volley_channels() {
        let expected = [
            (1300, 0, 1),
            (1500, 0, 2),
            (1900, 0, 3),
            (2500, 0, 1),
            (3100, 0, 2),
            (3500, 0, 3),
            (5350, 1, 12),
            (5750, 2, 6),
            (6200, 1, 12),
            (6600, 2, 6),
            (7050, 1, 12),
            (7450, 2, 6),
            (7900, 1, 12),
        ];
        let mut clocks = Clocks::default();
        let mut observed = vec![];
        for elapsed in (0..18700).step_by(50) {
            let schedule = schedule(elapsed);
            for slot in clocks.due(&schedule, elapsed) {
                let attack = schedule.attacks[slot];
                let Action::Shoot { projectile, .. } = attack.action else {
                    panic!("unexpected grenade")
                };
                observed.push((elapsed, projectile, attack.count));
            }
        }
        assert_eq!(
            observed,
            expected
                .iter()
                .copied()
                .chain(expected.iter().map(|&(t, p, n)| (t + 9350, p, n)))
                .collect::<Vec<_>>()
        );
        let ring = schedule(5350).snapshot(1.);
        assert_eq!(ring.len(), 12);
        assert!((ring[0].1 - (-165f32).to_radians()).abs() < 1e-6);
        let ring = schedule(5750).snapshot(1.);
        assert_eq!(ring.len(), 6);
        assert!((ring[0].1 - (-135f32).to_radians()).abs() < 1e-6);
        assert_eq!(movement(0), (Motion::Stationary, 0.));
        assert!(
            matches!(movement(50), (Motion::Follow { sight: 11., stop: 1.6, .. }, speed) if speed == nr_speed(6.))
        );
        assert_eq!(phase(8300), (4, 0));
        assert_eq!(phase(9350), (5, 0));
    }
}
