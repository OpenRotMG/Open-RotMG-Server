use crate::{
    content::{Cell, Content, Map},
    encounter::{Motion, Profile},
};
use anyhow::{Result, ensure};
use rand::{Rng, SeedableRng};

pub const BOSS: u16 = 0x936;
pub const EGG: u16 = 0x21b;
pub const HATCHLING: u16 = 0x21c;
pub const ROSTER: [u16; 16] = [
    EGG, HATCHLING, 0x21d, 0x21e, 0x21f, 0x220, BOSS, 0x937, 0x938, 0x939, 0x93a, 0x93b, 0x93c,
    0x93d, 0x93e, 0x93f,
];

pub fn generate(content: &Content, seed: u64) -> Result<Map> {
    let size = 90;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut map = Map {
        width: size,
        height: size,
        cells: vec![
            Cell {
                ground: 0xff,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            (size * size) as usize
        ],
        spawns: vec![(12.5, 74.5)],
    };
    let rooms = [
        (12, 74),
        (12, 54),
        (32, 54),
        (32, 34),
        (52, 34),
        (72, 34),
        (72, 16),
    ];
    for (i, (cx, cy)) in rooms.iter().copied().enumerate() {
        let radius = if i == rooms.len() - 1 { 12 } else { 6 };
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                map.cells[(y * size + x) as usize].ground = 0x2f;
            }
        }
    }
    for pair in rooms.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        for y in y0.min(y1) - 1..=y0.max(y1) + 1 {
            for x in x0.min(x1) - 1..=x0.max(x1) + 1 {
                map.cells[(y * size + x) as usize].ground = 0x2f;
            }
        }
    }
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let i = (y * size + x) as usize;
            if map.cells[i].ground == 0xff
                && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                    .iter()
                    .any(|(x, y)| map.cells[(y * size + x) as usize].ground == 0x2f)
            {
                map.cells[i].object = Some(0x1e8);
            }
        }
    }
    for (cx, cy) in rooms.iter().copied().skip(1).take(rooms.len() - 2) {
        for (dx, dy) in [(-3, -3), (3, -3), (-3, 3), (3, 3)] {
            map.cells[((cy + dy) * size + cx + dx) as usize].object =
                Some(rng.gen_range(HATCHLING..=0x220));
        }
        map.cells[(cy * size + cx) as usize].object = Some(EGG);
        for (dx, dy) in [(-5, 0), (5, 0)] {
            map.cells[((cy + dy) * size + cx + dx) as usize].object =
                Some([0x1fc, 0x1fd, 0x1fe, 0x1800][rng.gen_range(0..4)]);
        }
    }
    map.cells[(16 * size + 72) as usize].object = Some(BOSS);
    let spawn = &mut map.cells[(74 * size + 12) as usize];
    spawn.region = 1;
    spawn.object = Some(0x703);
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Spider Den ground"
        );
        if let Some(k) = c.object {
            content.get(k)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        "generated reconstructed Spider Den layout"
    );
    Ok(map)
}

// The modified 7.0 realm-src 1855c017 SpiderDen table supplies behavior, not an authenticated 123.5.1 map.
pub fn initial_condition(kind: u16) -> i32 {
    if kind == BOSS || (0x937..=0x93f).contains(&kind) {
        crate::condition::bit(crate::condition::INVULNERABLE)
    } else {
        0
    }
}

pub fn profile(kind: u16) -> Profile {
    let (motion, speed, interval, range) = match kind {
        EGG => (Motion::Stationary, 0., 0, 0.),
        HATCHLING => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            9.6,
            1000,
            10.,
        ),
        0x21d | 0x21f => (
            Motion::Charge {
                sight: if kind == 0x21d { 20. } else { 40. },
                cooldown: 2000,
                delay: 0,
                stop: 0.1,
                idle: 9.6,
            },
            10.8,
            500,
            10.,
        ),
        0x21e => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            12.,
            500,
            10.,
        ),
        0x220 => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            9.6,
            500,
            10.,
        ),
        BOSS => (Motion::Wander, 12., 1000, 20.),
        // The source Shoot checks a positive 5ms timer before subtracting its 50ms tick: one volley every two ticks.
        0x937..=0x93f => (Motion::Stationary, 0., 100, 30.),
        _ => panic!("unregistered Spider Den actor {kind:#x}"),
    };
    Profile {
        motion,
        speed,
        interval,
        range,
    }
}

pub fn attacks(kind: u16, heading: f32, elapsed: u64) -> Vec<(u8, f32)> {
    match kind {
        EGG => vec![],
        BOSS if elapsed < 2000 => vec![],
        BOSS => {
            let mut shots: Vec<_> = (0..12)
                .map(|i| (0, (22.5 + i as f32 * 30.).to_radians()))
                .collect();
            shots.push((0, heading));
            if ((elapsed - 2000) / 1000).is_multiple_of(2) {
                shots.push((1, heading));
            }
            shots
        }
        0x937..=0x93f => {
            let start = match kind {
                0x937 | 0x93d => 120.,
                0x938 => 180.,
                0x939 | 0x93e => 240.,
                0x93a => 300.,
                0x93b | 0x93f => 0.,
                0x93c => 60.,
                _ => unreachable!(),
            };
            // Equivalent wrapped angles keep the fan in one native volley instead of flooding its ACK queue.
            (0..3)
                .map(|i| (0, (start + i as f32 * 60.).to_radians()))
                .collect()
        }
        0x220 => [-20f32, 0., 20.]
            .into_iter()
            .map(|d| (0, heading + d.to_radians()))
            .collect(),
        HATCHLING..=0x21f => vec![(0, heading)],
        _ => panic!("unregistered Spider Den attack {kind:#x}"),
    }
}
