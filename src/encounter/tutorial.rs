use super::{
    Anchor, Motion, Profile,
    attacks::{Attack, Schedule},
    nr_speed,
};

pub(crate) const GOD: u16 = 0x6b1;
pub(crate) const MINION: u16 = 0x6b0;
pub(crate) const CHEF: u16 = 0x926;
pub(crate) const ROSTER: &[u16] = &[0x6af, MINION, GOD, 0x6b2, 0x6b3, 0x6b4, CHEF];
pub(crate) const REPRODUCE_MS: u64 = 60050;
pub(crate) const MINION_LIMIT: usize = 12;

// Compiled Kithio reconstruction; selected XML forbids chicken shots: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Tutorial.cs
pub(crate) fn movement(kind: u16, stage: u64) -> (Motion, f32) {
    let follow = |speed, stop, sight, idle| {
        (
            Motion::Follow {
                sight,
                stop,
                idle: nr_speed(idle),
                duration: 0,
                cooldown: 0,
            },
            nr_speed(speed),
        )
    };
    match kind {
        0x6af | 0x6b2 => (Motion::Wander, nr_speed(0.3)),
        GOD => follow(0.4, 5., 10., 0.3),
        MINION => (
            Motion::Protect {
                anchor: Anchor::Parent,
                distance: 1.,
            },
            nr_speed(0.3),
        ),
        0x6b3 => follow(0.6, 6., 10., 0.4),
        0x6b4 => follow(0.8, 1., 10., 0.4),
        CHEF => match stage % 4 {
            0 => (Motion::Wander, nr_speed(0.6)),
            1 | 2 => follow(0.6, 3., 9., 0.6),
            3 => (Motion::Retreat { distance: 6. }, nr_speed(0.5)),
            _ => panic!("invalid Kitchen boss phase {stage}"),
        },
        _ => panic!("unsupported Tutorial enemy {kind:#x}"),
    }
}
pub(crate) fn profile(kind: u16) -> Profile {
    let (motion, speed) = movement(kind, 0);
    Profile {
        motion,
        speed,
        interval: if matches!(kind, 0x6b3 | 0x6b4 | CHEF) {
            1050
        } else {
            0
        },
        range: 0.,
    }
}
pub(crate) fn schedule(kind: u16, stage: u64, age: u64) -> Schedule {
    let attacks = match kind {
        0x6b3 => vec![Attack::fan(0, 1, 0., 1050, 7.)],
        0x6b4 => vec![Attack::fan(0, 1, 0., 1050, 3.)],
        CHEF => match stage % 4 {
            0 | 3 => vec![],
            1 => vec![Attack::fan(0, 1, 0., 1450, 6.)],
            2 => (0..9)
                .map(|i| {
                    let delay = if i < 5 { i * 50 } else { 1250 + (i - 5) * 1050 };
                    let index = (stage / 4 * 4 + i) % 5;
                    Attack::fan(0, 4, 90., 1050, 0.)
                        .fixed(((index + 1) as f32 * 25.).to_radians())
                        .once()
                        .delay(delay)
                })
                .collect(),
            _ => panic!("invalid Kitchen attack phase {stage}"),
        },
        0x6af..=0x6b2 => vec![],
        _ => panic!("unsupported Tutorial attack {kind:#x}"),
    };
    Schedule {
        state: stage,
        age,
        attacks,
    }
}
