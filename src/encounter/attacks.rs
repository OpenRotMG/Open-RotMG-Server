#[derive(Clone, Copy, Debug)]
pub(crate) enum Aim {
    Target,
    Fixed(f32),
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Prediction {
    None,
    Lead(f32),
    Chance(f64),
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Alternative {
    pub projectile: u8,
    pub count: usize,
    pub gap: f32,
    pub offset: f32,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Action {
    Shoot {
        projectile: u8,
        choice: Option<Alternative>,
    },
    Grenade {
        radius: f32,
        damage: i32,
    },
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Attack {
    pub action: Action,
    pub aim: Aim,
    pub count: usize,
    pub gap: f32,
    pub offset: f32,
    pub interval: Option<u64>,
    pub delay: u64,
    pub range: f32,
    pub predict: Prediction,
    pub enabled: bool,
}
impl Attack {
    pub(super) fn fan(projectile: u8, count: usize, gap: f32, interval: u64, range: f32) -> Self {
        Self {
            action: Action::Shoot {
                projectile,
                choice: None,
            },
            aim: Aim::Target,
            count,
            gap: gap.to_radians(),
            offset: 0.,
            interval: Some(interval),
            delay: 0,
            range,
            predict: Prediction::None,
            enabled: true,
        }
    }
    pub(super) fn ring(projectile: u8, count: usize, interval: u64, range: f32) -> Self {
        Self::fan(projectile, count, 360. / count as f32, interval, range)
            .offset(180. - 180. / count as f32)
    }
    pub(super) fn grenade(radius: f32, damage: i32, interval: u64, range: f32) -> Self {
        Self {
            action: Action::Grenade { radius, damage },
            ..Self::fan(0, 1, 0., interval, range)
        }
    }
    pub(super) fn offset(mut self, degrees: f32) -> Self {
        self.offset += degrees.to_radians();
        self
    }
    pub(super) fn delay(mut self, ms: u64) -> Self {
        self.delay = ms;
        self
    }
    pub(super) fn predict(mut self, factor: f32) -> Self {
        self.predict = Prediction::Lead(factor);
        self
    }
    pub(super) fn chance(mut self, chance: f64) -> Self {
        self.predict = Prediction::Chance(chance);
        self
    }
    pub(super) fn fixed(mut self, angle: f32) -> Self {
        self.aim = Aim::Fixed(angle);
        self
    }
    pub(super) fn once(mut self) -> Self {
        self.interval = None;
        self
    }
    pub(super) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    fn choice(mut self, index: u8) -> Self {
        let Action::Shoot { choice, .. } = &mut self.action else {
            unreachable!()
        };
        *choice = Some(Alternative {
            projectile: index,
            count: self.count,
            gap: self.gap,
            offset: self.offset,
        });
        self
    }
    pub(super) fn alternate(mut self, projectile: u8, count: usize, gap: f32, offset: f32) -> Self {
        let Action::Shoot { choice, .. } = &mut self.action else {
            panic!("grenade cannot have an alternate volley")
        };
        *choice = Some(Alternative {
            projectile,
            count,
            gap: gap.to_radians(),
            offset: offset.to_radians(),
        });
        self
    }
    pub fn shots(self, projectile: u8, heading: f32) -> Vec<(u8, f32)> {
        let heading = match self.aim {
            Aim::Target => heading,
            Aim::Fixed(angle) => angle,
        };
        (0..self.count)
            .map(|i| {
                (
                    projectile,
                    heading + self.offset + (i as f32 - (self.count - 1) as f32 / 2.) * self.gap,
                )
            })
            .collect()
    }
}
#[derive(Debug)]
pub(crate) struct Schedule {
    pub state: u64,
    pub age: u64,
    pub attacks: Vec<Attack>,
}
impl Schedule {
    pub fn snapshot(&self, heading: f32) -> Vec<(u8, f32)> {
        self.attacks
            .iter()
            .filter(|a| {
                a.enabled
                    && self.age >= a.delay
                    && match a.interval {
                        Some(ms) => (self.age - a.delay).is_multiple_of(ms),
                        None => self.age == a.delay,
                    }
            })
            .flat_map(|a| match a.action {
                Action::Shoot { projectile, .. } => a.shots(projectile, heading),
                Action::Grenade { .. } => vec![],
            })
            .collect()
    }
}
pub(crate) fn abyss_schedule(kind: u16, elapsed: u64) -> Schedule {
    use crate::dungeon::abyss::{self, IDOL, MALPHAS};
    let fan = Attack::fan;
    let mut state = 0;
    let mut age = elapsed;
    let attacks = match kind {
        0x66d => vec![fan(0, 5, 9., 1000, 20.)],
        0x66e => vec![fan(0, 3, 7.2, 1000, 15.)],
        0x66f | 0x670 => vec![fan(0, 3, 9., 1000, 15.)],
        0x671 | 0x672 => vec![fan(0, 3, 6., 500, 4.8)],
        0x908 => vec![fan(0, 3, 5., 1200, 5.).chance(0.45)],
        0x909 => vec![fan(0, 8, 45., 1, 0.).fixed(0.).delay(600).once()],
        0x90b => vec![
            fan(0, 1, 0., 200, 4.),
            fan(0, 8, 45., 1, 0.).fixed(0.).once().enabled(false),
        ],
        0x651 => vec![fan(0, 3, 15., 500, 10.)],
        MALPHAS => {
            let phase = abyss::phase(elapsed);
            state = elapsed / 50200 * 7 + phase;
            age = elapsed % 50200 - [0, 9000, 11500, 23500, 36500, 39000, 47000][phase as usize];
            let large = phase == 3;
            let rotation = age.saturating_sub(1800) / 1400 % 3;
            // Rotation must not rearm the other weapons, including the continuously firing radial slot.
            vec![
                fan(
                    if large { 3 } else { 0 },
                    1,
                    0.,
                    if phase == 5 { 2100 } else { 900 },
                    8.,
                )
                .offset(match phase {
                    0 => 1.,
                    2 => 0.6,
                    _ => 0.,
                })
                .chance(match phase {
                    0 | 2 => 0.15,
                    3 => 0.2,
                    5 => 0.25,
                    _ => 0.,
                })
                .delay(if large { 1800 } else { 0 })
                .enabled(matches!(phase, 0 | 2 | 3 | 5)),
                fan(2, 6, 60., 1200, 0.).fixed(0.).enabled(phase == 2),
                fan(4, 3, 120., 700, 0.)
                    .fixed((rotation as f32 * 40.).to_radians())
                    .offset(0.7)
                    .delay(1800)
                    .enabled(large),
                fan(3, 1, 0., 2000, 8.)
                    .chance(0.25)
                    .delay(3200)
                    .enabled(large && age >= 3200 && rotation == 1),
            ]
        }
        IDOL => vec![
            fan(0, 6, 60., 400, 0.).fixed(age as f32 / 1500.),
            fan(2, 4, 90., 1200, 0.).fixed(0.),
            fan(3, 3, 10., 1400, 20.),
            fan(3, 3, 10., 1400, 20.).delay(400),
        ],
        _ => unreachable!("unregistered Abyss schedule {kind:#x}"),
    };
    Schedule {
        state,
        age,
        attacks,
    }
}
fn manor_schedule(kind: u16, hp: i32, elapsed: u64) -> Schedule {
    use crate::dungeon::manor::*;
    let mut state = 0;
    let mut age = elapsed;
    let fan = Attack::fan;
    let attacks = match kind {
        BOSS => {
            let (phase, t) = crate::dungeon::manor::phase(elapsed);
            age = t;
            state = phase;
            match phase % 3 {
                // A volley every ≈0.45 s, often 90-180° away from the player: the direction rule is unknown, so each volley takes a
                // pseudo-random bearing (the daggers' count and spacing follow his HP, see `daggers`).
                0 => {
                    let (count, gap) = daggers(hp);
                    let bearing = (phase * 7919 + t / 450 * 104_729) % 360;
                    vec![fan(0, count, gap, 450, 30.).fixed((bearing as f32).to_radians())]
                }
                // Four rings of 24 Ruthven Shots 15° apart, one shot due east, from 0.2 s after the sprite change, ≈0.4 s apart.
                1 => (0..4)
                    .map(|k| {
                        Attack::ring(1, 24, 1, 30.)
                            .fixed(0.)
                            .delay(200 + 400 * k)
                            .once()
                    })
                    .collect(),
                2 => vec![],
                _ => unreachable!(),
            }
        }
        BAT | ESCORT | CHASER => vec![fan(0, 1, 0., 900, 1.)],
        0x1727 => vec![fan(0, 1, 0., 100, 1.)],
        // A landed coffin opens (red eyes) 0.4-0.8 s after landing, then fires single aimed Coffin Bullets about every 0.2 s.
        CREATURE => vec![fan(0, 1, 0., 200, 4.).delay(600)],
        FERATU => vec![fan(0, 5, 9., 1200, 8.)],
        // Bone Swipe: 3 shots 25° apart, aimed, ≈2 s apart (two videos; the interval LOW); Vampire Magic 5 side by side.
        0x1728 => vec![fan(0, 3, 25., 2000, 8.), fan(1, 5, 5., 1700, 9.)],
        0x1729 => vec![fan(0, 2, 180., 1500, 10.), fan(1, 1, 0., 750, 8.)],
        // A landed RockBomb bursts red and is gone ≈0.5 s later (RouEGVH0_eQ 2012-08-05): its spikes fire on landing (count unknown).
        ROCK => vec![Attack::ring(0, 8, 50, 0.).fixed(0.).delay(50).once()],
        // Fans ≈10.5° apart (at least 3 shots seen; count and cadence unmeasured).
        0x172b => vec![fan(0, 5, 10.5, 1500, 10.)],
        0x172c => vec![fan(0, 3, 5., 1500, 9.)],
        COFFIN | SWARM | DECOY | KAGE | MINE | GRAVE => vec![],
        _ => unreachable!("unregistered Manor attack"),
    };
    Schedule {
        state,
        age,
        attacks,
    }
}
pub(crate) fn manor_entry_schedule(kind: u16, stage: u64, elapsed: u64) -> Schedule {
    use crate::dungeon::manor::{KAGE, MINE};
    let mut state = stage << 32;
    let mut age = elapsed;
    let attacks = match (kind, stage) {
        // Bursts of 4 volleys 0.2 s apart, each 2 Specter Shots 180° apart, turning 45° per volley from due east; a burst every ≈2.45 s
        // (tH9A8yA4XQI 2012-04-03 and jhp6TWhv_44, both checked at 1.0×). No aimed pair was ever seen.
        (KAGE, 2) => {
            let t = elapsed % 2450;
            state += elapsed / 2450;
            age = t;
            vec![
                Attack::fan(1, 2, 180., 200, 0.)
                    .fixed((t / 200) as f32 * 45f32.to_radians())
                    .enabled(t < 800),
            ]
        }
        (KAGE, 3) => vec![Attack::fan(0, 2, 50., 400, 20.)],
        // A mine bursts into 4 Specter Spikes 90° apart, one of them at the nearest player.
        (MINE, 1) => vec![Attack::fan(0, 4, 90., 50, 60.).offset(45.).once()],
        (KAGE, 0 | 1) | (MINE, 0) => vec![],
        _ => unreachable!("invalid Manor entrance phase"),
    };
    Schedule {
        state,
        age,
        attacks,
    }
}

pub(crate) fn schedule(kind: u16, hp: i32, elapsed: u64) -> Option<Schedule> {
    if kind == 0x646 {
        return Some(super::minotaur::schedule(elapsed));
    }
    if super::tutorial::ROSTER.contains(&kind) {
        let (stage, age) = if kind != super::tutorial::CHEF {
            (0, elapsed)
        } else if hp < 90 {
            (3, 0)
        } else if elapsed % 9700 < 4500 {
            (1 + elapsed / 9700 * 4, elapsed % 9700)
        } else {
            (2 + elapsed / 9700 * 4, elapsed % 9700 - 4500)
        };
        return Some(super::tutorial::schedule(kind, stage, age));
    }
    if crate::dungeon::abyss::ROSTER.contains(&kind) {
        return Some(abyss_schedule(kind, elapsed));
    }
    if crate::dungeon::manor::ROSTER.contains(&kind) {
        return Some(manor_schedule(kind, hp, elapsed));
    }
    if crate::dungeon::davy::profile(kind).is_some() {
        return Some(davy_schedule(kind, 1, elapsed));
    }
    if super::landmarks::ROSTER.contains(&kind) {
        return Some(super::landmarks::schedule(kind, hp, 0, 0, elapsed));
    }
    if super::fauna::ROSTER.contains(&kind) {
        return Some(super::fauna::schedule(kind, hp, elapsed));
    }
    let fan = Attack::fan;
    let ring = Attack::ring;
    let grenade = Attack::grenade;
    let mut state = 0;
    let mut age = elapsed;
    let attacks = match kind {
        // Prefer the compiled primary cycle over Trapped's earlier one-second alternation: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Midland.cs#L682-L699
        0x6c6 => {
            let phase = elapsed % 5300;
            let (projectile, count, interval, entry) = if phase < 3100 {
                (0, 3, 500, 0)
            } else {
                (1, 1, 700, 3100)
            };
            state = elapsed / 5300 * 2 + u64::from(projectile);
            age = phase - entry;
            vec![fan(projectile, count, 11., interval, 10.).delay(interval)]
        }
        0x21b => vec![],
        // Straight at the target, no lead seen: hatchlings every ≈0.9-1 s within 9 tiles; the adults every ≈0.4 s
        // (medians 0.37-0.38 s over 18 intervals), the Black Den Spider only within 3 tiles; Brown 3 shots 9° apart (24 pairs, 9.17 ± 0.70°).
        0x21c => vec![fan(0, 1, 0., 1000, 9.)],
        0x21d => vec![fan(0, 1, 0., 400, 3.)],
        0x21e | 0x21f => vec![fan(0, 1, 0., 400, 9.)],
        0x220 => vec![fan(0, 3, 9., 400, 9.)],
        // From ≈3.0 s after the toss: a ring of 12 Green Magic at fixed 0°, 30° … 330° every ≈3.2 s (2.6-3.8 s), a Green Blob (Weak) at the
        // nearest player every ≈1.5 s and single Green Magic between (LOW); aim lead unknown, so none.
        0x936 => {
            let start = crate::dungeon::spider::ATTACK_MS;
            state = u64::from(elapsed >= start);
            age = elapsed.saturating_sub(start);
            vec![
                fan(0, 12, 30., 3200, 3000.)
                    .fixed(0.)
                    .offset(15.)
                    .enabled(state == 1),
                fan(1, 1, 0., 1500, 20.).enabled(state == 1),
                fan(0, 1, 0., 750, 20.).enabled(state == 1),
            ]
        }
        0x937..=0x93f => {
            let center: f32 = match kind {
                0x937 | 0x93d => 180.,
                0x938 => 240.,
                0x939 | 0x93e => 300.,
                0x93a => 360.,
                0x93b | 0x93f => 60.,
                0x93c => 120.,
                _ => unreachable!(),
            };
            // Equivalent wrapped angles keep all three threads in one native volley/ACK.
            vec![
                fan(0, 3, 60., crate::dungeon::spider::WEB_INTERVAL, 0.).fixed(center.to_radians()),
            ]
        }
        0x954 => vec![fan(0, 1, 0., 2000, 8.).choice(1)],
        0x60c => vec![fan(0, 1, 0., 5000, 10.), fan(1, 1, 0., 400, 5.)],
        0x953 => vec![fan(0, 1, 0., 1000, 8.), grenade(2., 12, 2500, 5.)],
        0x614 => {
            state = u64::from(hp < 182);
            let prediction = if state == 1 { 0.25 } else { 0.35 };
            vec![
                fan(0, 1, 0., 1050, 8.).predict(prediction),
                fan(1, 1, 0., if state == 1 { 1050 } else { 1350 }, 8.).predict(prediction),
            ]
        }
        0x609 => {
            state = elapsed / 2000;
            age = elapsed % 2000;
            match state % 3 {
                0 => vec![fan(0, 3, 14., 1000, 10.).chance(0.3)],
                1 => vec![fan(0, 3, 10., 1000, 10.).chance(0.5)],
                _ => vec![],
            }
        }
        0x617 | 0x61a => {
            let mut attacks = (0..3)
                .map(|i| {
                    ring(i, 15, 1200, 25.)
                        .fixed(i as f32 * 8f32.to_radians())
                        .delay(i as u64 * 400)
                })
                .collect::<Vec<_>>();
            if kind == 0x61a {
                attacks.push(fan(3, 1, 0., 1000, 10.));
            }
            attacks
        }
        0x632 => vec![
            fan(0, 3, 15., 2000, 10.),
            fan(1, 1, 0., 2000, 10.).delay(1000),
        ],
        0x633 => {
            state = elapsed / 5000;
            age = elapsed % 5000;
            vec![fan(
                (state % 2) as u8,
                1,
                0.,
                if state.is_multiple_of(2) { 250 } else { 500 },
                10.,
            )]
        }
        0x640 => vec![
            fan(0, 3, 7., 1000, 10.),
            fan(1, 1, 0., 1000, 10.).delay(500).predict(1.),
        ],
        0x645 => vec![
            fan(0, 1, 0., 1000, 10.),
            grenade(2., 55, 1000, 5.).delay(500),
        ],
        0x649 => vec![
            fan(0, 1, 0., 2000, 10.).predict(0.5),
            fan(1, 1, 0., 2000, 10.).delay(1000).predict(1.),
        ],
        // Dungeon Quiet also predicts; its purple fan does not: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/db/BehaviorDb.SpriteWorld.cs#L65-L70
        0xd05 => vec![
            fan(0, 4, 10., 1000, 12.),
            fan(1, 1, 0., 1000, 12.).predict(1.),
        ],
        // Only Quiet leads movement: https://steamcommunity.com/sharedfiles/filedetails/?id=181796604
        0x652 => vec![
            fan(0, 4, 10., 1000, 12.),
            fan(1, 1, 0., 1000, 10.).predict(1.),
        ],
        0x656 => vec![
            ring(0, 5, 750, 12.).predict(0.5),
            fan(1, 1, 0., 1000, 10.).predict(1.),
        ],
        0x658 => vec![
            fan(0, 5, 10., 1000, 12.).predict(1.),
            fan(1, 1, 0., 650, 10.).predict(1.),
        ],
        0x91a if hp >= 400 => {
            let phase = elapsed % 3750;
            state = elapsed / 3750 * 3
                + if phase < 2500 {
                    0
                } else if phase > 3500 {
                    2
                } else {
                    1
                };
            if phase < 2500 {
                age = phase;
                vec![]
            } else {
                age = phase - 2500;
                let step = (age.saturating_sub(200) / 200).min(4) as f32;
                if age > 1000 {
                    vec![]
                } else {
                    vec![
                        ring(0, 4, 200, 10.)
                            .fixed((step * 10.).to_radians())
                            .delay(200),
                        ring(0, 4, 200, 10.)
                            .fixed((90. - step * 10.).to_radians())
                            .delay(200),
                    ]
                }
            }
        }
        0x6d8 => {
            state = elapsed / 1500;
            age = elapsed % 1500;
            match state % 3 {
                0 => (0..3)
                    .map(|i| fan(0, 3, 10., 300, 15.).offset(i as f32 * 120.))
                    .collect(),
                1 => (0..3)
                    .map(|i| fan(0, 4, 10., 300, 15.).offset(60. + i as f32 * 120.))
                    .collect(),
                // Blue pairs are not a faster phase in the older table: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.Mountain.cs#L320-L343
                _ => vec![fan(1, 2, 15., 300, 15.).predict(1.)],
            }
        }
        0x906 | 0x907 if hp < 400 => vec![ring(0, 8, 50, 0.).fixed(0.)],
        crate::dungeon::snake::GUARD => {
            state = u64::from(hp < crate::dungeon::snake::GUARD_RAGE_HP);
            vec![
                fan(0, 3, 25., 1000, 25.),
                fan(1, 6, 60., 1000, 10.),
                fan(2, 3, 120., 2000, 15.).enabled(state == 1),
            ]
        }
        crate::dungeon::snake::DART => vec![fan(0, 1, 0., 2000, 25.)],
        crate::dungeon::snake::BUTTON | crate::dungeon::snake::SPAWNER => vec![],
        0x917 => {
            (state, age) = crate::dungeon::snake::phase(elapsed);
            match state % 3 {
                0 => vec![
                    fan(0, 2, 10., 1000, 10.).offset(45.),
                    fan(0, 1, 0., 1000, 10.).offset(135.),
                    fan(0, 1, 0., 1000, 10.).offset(225.),
                    fan(0, 2, 10., 1000, 10.).offset(315.),
                ],
                1 => vec![
                    fan(0, 1, 0., 1000, 10.).enabled(age < 16000),
                    fan(0, 1, 0., 1000, 10.).offset(90.).enabled(age < 16000),
                    fan(0, 1, 0., 1000, 10.).offset(270.).enabled(age < 16000),
                ],
                _ => vec![ring(1, 6, 500, 10.), Attack::grenade(2.5, 100, 1000, 10.)],
            }
        }
        0xd06 => {
            let t = elapsed % 43300;
            let cycle = elapsed / 43300 * 6;
            match t {
                0..2000 => {
                    state = cycle;
                    age = t;
                    vec![]
                }
                2000..2300 => {
                    state = cycle + 1;
                    age = t - 2000;
                    vec![]
                }
                2300..12300 => {
                    state = cycle + 2;
                    age = t - 2300;
                    vec![
                        fan(0, 2, 10., 400, 8.).offset(0.7).chance(0.4),
                        fan(0, 2, 180., 400, 8.).offset(0.7).chance(0.4),
                        fan(0, 18, 20., 1500, 8.)
                            .offset(0.4)
                            .chance(0.4)
                            .enabled(false),
                    ]
                }
                12300..32300 => {
                    let local = (t - 12300) % 5000;
                    if local < 1500 {
                        state = cycle + 3;
                        age = local;
                        vec![
                            fan(0, 1, 0., 1000, 8.).chance(0.1),
                            fan(0, 3, 120., 500, 8.).offset(0.3).chance(0.1),
                        ]
                    } else {
                        state = cycle + 4;
                        age = local - 1500;
                        vec![fan(0, 3, 120., 800, 8.).offset(0.3).chance(0.2)]
                    }
                }
                _ => {
                    state = cycle + 5;
                    age = t - 32300;
                    vec![fan(0, 3, 15., 1300, 8.)]
                }
            }
        }
        0xd07..=0xd0a => {
            let corner: f32 = match kind {
                0xd07 => 90.,
                0xd08 => 0.,
                0xd09 => 270.,
                0xd0a => 180.,
                _ => unreachable!(),
            };
            if elapsed < 2000 {
                vec![]
            } else {
                state = if elapsed < 8000 {
                    1
                } else if elapsed < 14000 {
                    2
                } else {
                    3
                };
                age = elapsed - [0, 2000, 8000, 14000][state as usize];
                let mut attacks = vec![
                    fan(0, 1, 0., 300, 0.).fixed(corner.to_radians()),
                    fan(0, 1, 0., 300, 0.).fixed((corner + 90.).to_radians()),
                ];
                if state == 2 {
                    attacks.push(fan(0, 1, 0., 300, 0.).fixed((corner + 45.).to_radians()));
                }
                attacks
            }
        }
        0xd90 => {
            let phase = elapsed / 13000;
            let local = elapsed % 13000;
            state = phase * 2 + u64::from(local >= 3000);
            if local < 3000 {
                age = local;
                vec![]
            } else {
                age = local - 3000;
                match phase % 4 {
                    0 => (0..5)
                        .map(|i| {
                            fan(0, 3, 120., 1000, 0.)
                                .fixed((i as f32 * 24.).to_radians())
                                .delay(i * 200)
                        })
                        .collect(),
                    1 => vec![fan(4, 12, 30., 2000, 10.)],
                    2 => vec![
                        fan(1, 8, 45., 1000, 10.),
                        fan(1, 8, 45., 1000, 10.).offset(22.5).delay(500),
                        fan(2, 3, 20., 2000, 8.),
                    ],
                    _ => vec![fan(1, 3, 10., 1000, 8.)],
                }
            }
        }
        0xd99 => vec![fan(0, 3, 20., 1000, 10.), fan(1, 1, 0., 1000, 10.)],
        0xd9a => vec![
            fan(0, 3, 20., 1000, 10.),
            fan(1, 3, 20., 2000, 10.).delay(100),
        ],
        0xda5 | 0xda6 => vec![fan(0, 1, 0., 500, 10.), fan(0, 1, 0., 1000, 3.)],
        _ => return None,
    };
    Some(Schedule {
        state,
        age,
        attacks,
    })
}

pub(crate) fn davy_schedule(kind: u16, stage: u64, age: u64) -> Schedule {
    use crate::dungeon::davy::{BOSS, LIT};
    let fan = Attack::fan;
    let attacks = match kind {
        BOSS if stage == 1 => vec![fan(0, 5, 10., 2000, 10.), fan(1, 1, 0., 4000, 10.)],
        LIT if stage == 2 => vec![fan(0, 6, 60., 1000, 10.).once().delay(100)],
        0xe31 => vec![fan(0, 1, 0., 2000, 10.)],
        0xe50 => vec![fan(0, 3, 10., 2000, 10.)],
        0xe3a => vec![fan(0, 3, 10., 1750, 10.)],
        0xe44 => vec![fan(0, 6, 60., 4000, 10.)],
        0xe2f => vec![fan(0, 1, 0., 1750, 10.)],
        0xe42 => vec![fan(0, 3, 10., 2500, 10.)],
        0xe43 => vec![fan(0, 2, 10., 2000, 10.)],
        0xe45 => vec![
            fan(0, 1, 0., 1100, 13.),
            fan(0, 1, 0., 1100, 13.).delay(50),
            fan(0, 1, 0., 1100, 13.).delay(100),
            fan(1, 5, 10., 2000, 7.),
        ],
        0xe48 if stage == 2 => vec![fan(0, 6, 60., 1000, 10.).once()],
        _ => vec![],
    };
    Schedule {
        state: stage,
        age,
        attacks,
    }
}

pub(crate) fn armor_schedule(stage: u64, age: u64) -> Schedule {
    let attacks = match stage {
        0 => vec![],
        // Awake: 2 Armor Sword Attacks per volley (Japanese wiki "otherwise x2"); spread and cadence are reconstruction.
        1 => vec![Attack::fan(0, 2, 15., 600, 10.)],
        // Regen: one 6-shot ring on entry and one ≈1.3 s later (bBQRbAoRP_g 2012-04-01 t=168.0-172.4).
        2 => vec![
            Attack::fan(0, 6, 60., 1, 0.).fixed(0.).once(),
            Attack::fan(0, 6, 60., 1, 0.).fixed(0.).delay(1300).once(),
        ],
        // "Afterwards its Armor Sword Attack fires faster" (Japanese wiki); the rate is reconstruction.
        3 => vec![Attack::fan(0, 2, 15., 400, 10.)],
        _ => panic!("invalid Suit of Armor phase"),
    };
    Schedule {
        state: stage,
        age,
        attacks,
    }
}
/// One Stone Guardian block. Angles are world radians (y grows south, so a clockwise screen turn adds); `age` counts from the block start.
pub(crate) fn guardian_schedule(
    block: crate::endgame::GuardianBlock,
    serial: u64,
    right: bool,
    age: u64,
) -> Schedule {
    use crate::endgame::{
        GUARDIAN_RUN_MS, GUARDIAN_SPIN_FIRST_MS, GUARDIAN_SPLIT_MS, GuardianBlock,
    };
    let fan = Attack::fan;
    // A volley fires on the tick its delay passes; re-entering a block (the survivor greys mid-block) must not replay earlier volleys.
    let at = |attack: Attack, delay: u64| attack.delay(delay).once().enabled(age <= delay + 100);
    let attacks = match block {
        // 10 Small White Blasts 36° apart at fixed angles, one due east, every ≈2.0 s from 0.2 s into the walk (HLraOCBJO94 t=637.77-657.6, bBQRbAoRP_g t=341.5-375.5).
        GuardianBlock::Ring => vec![fan(2, 10, 36., 2000, 20.).fixed(0.).offset(18.).delay(200)],
        // 5 Blasts ≈10° apart at the player every ≈0.5-0.6 s while charging (5ZRsiTUcKts 2012-04-04 t=131.1-132.8).
        GuardianBlock::Fan => vec![fan(0, 5, 10., 550, 20.)],
        // 2 Blasts 180° apart, first at 0°/180°, turning 40° clockwise every 0.20 s on a world 20° grid; both guardians at the same angles (HL t=642.17-647.14, fzg t=252.04-256.7).
        GuardianBlock::Spin => (0..27u64)
            .map(|k| {
                at(
                    fan(0, 2, 180., 1, 20.).fixed((40. * k as f32 + 90.).to_radians()),
                    GUARDIAN_SPIN_FIRST_MS + k * 200,
                )
            })
            .collect(),
        // 4 fixed directions × 2 Blasts ≈10° apart every 1.0 s, the Right ≈0.2 s first (bBQRbAoRP_g t=344.3-349.0, fzg t=232.75-234.78).
        GuardianBlock::Quad => (0..5u64)
            .flat_map(|k| {
                (0..4).map(move |d| {
                    at(
                        fan(0, 2, 10., 1, 20.).fixed((-(90. * d as f32 + 5.)).to_radians()),
                        GUARDIAN_SPLIT_MS + k * 1000 + if right { 0 } else { 200 },
                    )
                })
            })
            .collect(),
        // Each 1.0 s cycle: Right fan A (7 × P1, 26° apart, centred east) at 0, Right fan B (8 × P1, 22° apart from -66° to +88°) and Left fan A'
        // (centred west) at 0.20 s, Left fan B' (B rotated 180°) at 0.38 s; every pass is simultaneous (5ZRsiTUcKts t=144.55-148.56, HL t=659.30-663.34).
        GuardianBlock::Centre => (0..5u64)
            .flat_map(|cycle| {
                let t = GUARDIAN_RUN_MS + cycle * 1000;
                let (a, b, base): (u64, u64, f32) = if right {
                    (0, 200, 0.)
                } else {
                    (200, 380, 180.)
                };
                [
                    at(fan(1, 7, 26., 1, 20.).fixed(base.to_radians()), t + a),
                    at(
                        fan(1, 8, 22., 1, 20.).fixed((base - 11.).to_radians()),
                        t + b,
                    ),
                ]
            })
            .collect(),
        // From its side wall, 2 Blasts mirrored about the inward normal, starting ±120° and closing 10° every 0.20 s to 0° (13 volleys, world-fixed;
        // 5ZRsiTUcKts t=114.4-117.1, 136.15-138.9; fzg t=226.5-228.9). Only the Left's west wall was filmed; the Right mirrors it.
        GuardianBlock::Side => {
            let inward: f32 = if right { 180. } else { 0. };
            (0..13u64)
                .map(|k| {
                    let spread = 2. * (120. - 10. * k as f32);
                    at(
                        fan(0, 2, spread, 1, 20.).fixed(inward.to_radians()),
                        GUARDIAN_RUN_MS + k * 200,
                    )
                })
                .collect()
        }
    };
    Schedule {
        state: serial * 2 + 1,
        age,
        attacks,
    }
}
/// Insect Minion: aimed Stings plus a 6-shot Sting ring 60° apart (bBQRbAoRP_g 2012-04-01 t=285.75, 294.9); once its Commander is hit it turns red and adds
/// rings of Sting Status (t=299.3-299.6; "If the Commander is attacked, they will turn red and add in frequent rings", RealmEye 2017). Cadences are reconstruction.
pub(crate) fn insect_schedule(red: bool, age: u64) -> Schedule {
    let fan = Attack::fan;
    let mut attacks = vec![fan(0, 1, 0., 250, 10.), Attack::ring(0, 6, 1500, 10.)];
    if red {
        attacks.push(Attack::ring(1, 6, 1500, 10.).delay(750));
    }
    Schedule {
        state: u64::from(red),
        age,
        attacks,
    }
}
/// The Oryx Brute alternates RING and CHARGE states of random length (bBQRbAoRP_g 2012-04-01 t=37-102).
pub(crate) fn brute_schedule(charge: bool, serial: u64, age: u64) -> Schedule {
    let fan = Attack::fan;
    let attacks = if charge {
        // Each 2.5 s dash: 0.45 s after it starts, Rush volleys at the player every ≈0.2 s for ≈1.6 s (fzgFSFVN8nY 2012-08-10; second dash 2.45 s after the first).
        // A volley spans about ±50°: 3 shots 50° apart is the best estimate (LOW).
        (0..2u64)
            .flat_map(|dash| {
                (0..8u64).map(move |k| {
                    fan(0, 3, 50., 1, 20.)
                        .delay(dash * 2500 + 450 + k * 200)
                        .once()
                })
            })
            .collect()
    } else {
        // 12 Knight Blast Sm 30° apart, then 12 Brute Lines 0.39-0.45 s later, every ≈2.2 s (4 bursts); a 1.3 s RING had none, so the first comes later.
        vec![
            fan(2, 12, 30., 2200, 20.).delay(1500),
            fan(1, 12, 30., 2200, 20.).delay(1920),
        ]
    };
    Schedule {
        state: serial * 2 + u64::from(charge),
        age,
        attacks,
    }
}
pub(crate) fn endgame_schedule(kind: u16, hp: i32, max_hp: i32, elapsed: u64) -> Option<Schedule> {
    let fan = Attack::fan;
    let ring = Attack::ring;
    let mut state = 0;
    let age = elapsed;
    let attacks = match kind {
        0x668 => vec![
            fan(0, 5, 5., 1000, 10.).predict(1.),
            fan(1, 1, 0., 500, 10.),
        ],
        0x66a | 0x66b => vec![fan(0, 3, 5., 1000, 10.).predict(0.5)],
        0xd7e => {
            return Some(armor_schedule(
                if hp == max_hp {
                    0
                } else if hp < max_hp / 5 {
                    2
                } else {
                    1
                },
                elapsed,
            ));
        }
        // Five 72°-spaced Lightning Spirals at two fixed sets 36° apart: set A, set B 0.6 s later, then 1.4 s quiet; nothing aims or turns (5ZRsiTUcKts, HLraOCBJO94, bBQRbAoRP_g).
        // Below half health it bursts 8 Lightning Spirals 45° apart and is removed (exMHglbJ98E 2012-08-15 HP overlay; 5ZRsiTUcKts t=197.9-198.1).
        0xd7c if hp * 2 < max_hp => {
            state = 1;
            vec![fan(0, 8, 45., 1, 0.).fixed(0.).offset(22.5).once()]
        }
        0xd7c => vec![
            fan(0, 5, 72., 2000, 0.).fixed(36f32.to_radians()),
            fan(0, 5, 72., 2000, 0.).fixed(0.).delay(600),
        ],
        // Swipe Spin (Slowed) bursts: 3 aimed shots, then 2 at the same outer angles 0.14-0.21 s later; spacing 8.5-18.7° (fzgFSFVN8nY, bBQRbAoRP_g).
        // The burst period is unresolved (3.0 s and 0.45 s readings, both LOW). Bubble, Line and a Knight Blast Sm ring were never seen in ≈38 s of 2012 Knight footage.
        0xd83 => vec![
            fan(0, 3, 15., 3000, 10.),
            fan(0, 2, 30., 3000, 10.).delay(170),
        ],
        0x1741..=0x174e => return Some(oryx1_summon_schedule(kind, elapsed)),
        // Each Goo fires its selected speed-0, 6000 ms MultiHit projectile once: the puddle is the hazard.
        0xdf0 => vec![fan(0, 1, 0., 1, 0.).fixed(0.).once()],
        // A landed Quiet Bomb rests 0.8 s, then fires one circle of 18 Mute Spells (7 instances, e.g. bBQRbAoRP_g 2012-04-01 t=256.4; HLraOCBJO94 t=616.2-617.0).
        0xd86 => vec![ring(0, 18, 1, 0.).fixed(0.).delay(800).once()],
        0x944 => vec![
            fan(0, 1, 0., 1500, 8.).chance(1.),
            fan(1, 3, 20., 1500, 8.).delay(500),
        ],
        _ => return None,
    };
    Some(Schedule {
        state,
        age,
        attacks,
    })
}
pub(crate) fn oryx2_schedule(phase: crate::endgame::OryxPhase, age: u64) -> Schedule {
    use crate::endgame::OryxPhase;
    let fan = Attack::fan;
    if phase == OryxPhase::PrepareRage {
        return Schedule {
            state: 1,
            age,
            attacks: vec![
                fan(13, 1, 0., 4000, 0.).fixed(0.).delay(4000),
                fan(14, 1, 0., 4000, 0.)
                    .fixed(30f32.to_radians())
                    .delay(4000),
            ],
        };
    }
    assert!(
        matches!(phase, OryxPhase::Opening | OryxPhase::Rage),
        "invalid Oryx2 attack phase"
    );
    let mut attacks = vec![Attack::ring(0, 8, 1500, 25.)];
    attacks.extend(
        [(1, 3), (2, 3), (3, 2), (4, 3), (5, 2), (6, 3)]
            .into_iter()
            .map(|(index, n)| fan(index, n, 10., 1000, 25.)),
    );
    if phase == OryxPhase::Rage {
        attacks.push(fan(13, 1, 0., 50, 25.).delay(8000).once());
        attacks.push(fan(14, 1, 0., 50, 25.).delay(8500).once());
    }
    Schedule {
        state: if phase == OryxPhase::Rage { 2 } else { 0 },
        age,
        attacks,
    }
}

#[derive(Clone, Copy, Debug)]
enum Deadline {
    Unarmed,
    At(u64),
    Finished,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Clocks {
    state: Option<u64>,
    deadlines: Vec<Deadline>,
}
impl Clocks {
    pub fn due(&mut self, schedule: &Schedule, now: u64) -> Vec<usize> {
        if self.state != Some(schedule.state) {
            self.state = Some(schedule.state);
            self.deadlines = vec![Deadline::Unarmed; schedule.attacks.len()];
        }
        assert_eq!(
            self.deadlines.len(),
            schedule.attacks.len(),
            "attack channels changed without a phase transition"
        );
        self.deadlines
            .iter_mut()
            .enumerate()
            .filter_map(|(i, deadline)| {
                let attack = schedule.attacks[i];
                if !attack.enabled {
                    *deadline = Deadline::Unarmed;
                    return None;
                }
                if matches!(deadline, Deadline::Unarmed) {
                    *deadline = Deadline::At(
                        now.checked_sub(schedule.age)
                            .expect("attack phase age exceeds world time")
                            + attack.delay,
                    );
                }
                let Deadline::At(at) = *deadline else {
                    return None;
                };
                if now < at {
                    return None;
                }
                *deadline = match attack.interval {
                    Some(ms) => Deadline::At(now + ms),
                    None => Deadline::Finished,
                };
                Some(i)
            })
            .collect()
    }
    pub fn suspend(&mut self, delta: u64) {
        for clock in &mut self.deadlines {
            if let Deadline::At(at) = clock {
                *at += delta;
            }
        }
    }
}

/// Oryx the Mad God 1 by phase (endgame::oryx1). `age` runs from the phase taunt
/// (from "All who look…" in the Gaze), `serial` restarts every clock per phase instance, and nothing fires during the Gaze lead-in.
/// Fixed angles are world radians with y south, so clockwise on screen raises the angle.
pub(crate) fn oryx1_schedule(
    phase: crate::endgame::oryx1::Phase,
    serial: u64,
    age: u64,
    hp: i32,
    lead_in: bool,
) -> Schedule {
    use crate::endgame::oryx1::{GRID_MS, LOW_HP, Phase::*, RAGE_HEAL_MS};
    let fan = Attack::fan;
    let once = |attack: Attack, at: u64| attack.delay(at).once().enabled(age <= at + 100);
    let between =
        |attack: Attack, from: u64, to: u64| attack.delay(from).enabled((from..to).contains(&age));
    // The ring angle between instances is unmeasured; each instance turns its fixed rings differently.
    let turn = (serial * 13 % 36) as f32;
    if lead_in {
        return Schedule {
            state: serial * 2,
            age,
            attacks: vec![],
        };
    }
    let attacks = match phase {
        // Alone, he answers the arrivals with an orange fan of ≈8-9 Fire Bolts (M3Of4Q7Ag7Q 2012-06-28 t=396.5) and single Green Stars
        // (Japanese wiki: Fire Bolt ×9, Green Star ×1; old wiki "wide Fire Bolt shotguns and single Green Stars"). Cadence and spread unmeasured.
        Alone => vec![fan(3, 9, 10., 1000, 20.), fan(4, 1, 0., 1000, 16.)],
        // Minion phases: aimed Green Star fans (Japanese wiki ×4, old wiki "4-star shotgun"; green stars at ZdjeB9t-n5w t=144); the later ones
        // add three armour-piercing White Bolts in scattered directions (Japanese wiki; old wiki "armor-piercing shots and slow stars").
        Opening => vec![fan(4, 4, 10., 1000, 16.)],
        Minions => vec![
            fan(4, 4, 10., 1000, 16.),
            fan(2, 3, 120., 1000, 0.).fixed((turn * 10. + age as f32 * 0.137).to_radians()),
        ],
        // A pulse every ≈2.0 s: 10 White Superblasts exactly 36° apart at a constant angle with a 24-ball White Ball torus; the first pulse of
        // each run adds 36 Quiet spinners; between the runs a 4-arm White Blue Bubble spiral turns 12.5° clockwise per 0.2 s volley for ≈7 s
        // (tg7tAkfKGUE 2012-09-12, which plays 1.78× fast, and exMHglbJ98E 2012-08-15).
        Planets => {
            let spiral = (age.saturating_sub(7_000) / 200) as f32 * 12.5;
            vec![
                between(
                    fan(16, 10, 36., 2000, 0.).fixed(turn.to_radians()),
                    2_800,
                    7_000,
                ),
                between(fan(6, 24, 15., 2000, 0.).fixed(0.), 2_800, 7_000),
                once(fan(13, 36, 10., 1, 0.).fixed(0.), 2_800),
                between(
                    fan(5, 4, 90., 200, 0.).fixed((turn + spiral).to_radians()),
                    7_000,
                    14_000,
                ),
                between(
                    fan(16, 10, 36., 2000, 0.).fixed(turn.to_radians()),
                    14_400,
                    u64::MAX,
                ),
                between(fan(6, 24, 15., 2000, 0.).fixed(0.), 14_400, u64::MAX),
                once(fan(13, 36, 10., 1, 0.).fixed(0.), 14_400),
            ]
        }
        // From ≈4 s a Quiet spinner ring (≈36) then a Weak one 0.55-0.8 s later, every ≈7.2 s; from ≈5 s a ring of ≈18 White Balls every 0.60 s
        // (M3Of4Q7Ag7Q t=449.2-466.4; exMHglbJ98E t≈156.4). The bombs are thrown by World::oryx_step.
        Silence => vec![
            fan(13, 36, 10., 7200, 0.).fixed(0.).delay(4_000),
            fan(14, 36, 10., 7200, 0.)
                .fixed(5f32.to_radians())
                .delay(4_700),
            fan(6, 18, 20., 600, 0.).fixed(0.).delay(5_000),
        ],
        // Three P9 balls 120° apart every ≈0.92 s (alternating ≈0.82 / 1.03 s; the triangle turns between volleys, so aimed), a Blind ring of
        // 8 every ≈1.5 s and a Silver Shield ring hugging him (count and cadence unmeasured: 10 per 0.5 s is an estimate). No bombs
        // (ZdjeB9t-n5w t=0.0-26.6, M3Of4Q7Ag7Q t=468-481).
        Artifacts => vec![
            fan(9, 3, 120., 920, 20.).delay(5_300),
            fan(8, 8, 45., 1500, 0.)
                .fixed(turn.to_radians())
                .delay(5_300),
            fan(7, 10, 36., 500, 0.).fixed(0.).delay(5_300),
        ],
        // Two White Flames 10° apart at a player every ≈0.5 s (27 pairs); below ≈half HP also one White Blue Bubble every ≈0.2 s through 8 fixed
        // directions 45° apart in a fixed scrambled order (absent at ≥55.6 % and 63 %; ZdjeB9t-n5w t=124.5-137.0).
        Gaze => {
            const ORDER: [f32; 8] = [90., 270., 315., 135., 0., 180., 45., 225.];
            vec![
                fan(1, 2, 10., 500, 20.),
                fan(5, 1, 0., 200, 0.)
                    .fixed(ORDER[(age / 200 % 8) as usize].to_radians())
                    .enabled(hp < LOW_HP),
            ]
        }
        // Not in view in any real-time dance: the 2012 wiki, the guide and the Japanese wiki give white bullets in scattered directions inside
        // the ring (Japanese wiki: P12 ×4) plus the same spinners and bombs as BE SILENT. Cadences are estimates.
        Dance => vec![
            fan(13, 36, 10., 7200, 0.).fixed(0.).delay(4_000),
            fan(14, 36, 10., 7200, 0.)
                .fixed(5f32.to_radians())
                .delay(4_700),
            fan(12, 4, 90., 400, 0.)
                .fixed((age as f32 * 0.31).to_radians())
                .delay(2_000),
        ],
        // 3 White Superblasts 120° apart every ≈0.6 s (LOW) and Parametric White Balls looping close around him (2-3 seen) (ZdjeB9t-n5w
        // t=229.9-231.1).
        Cosmic => vec![
            fan(16, 3, 120., 600, 0.)
                .fixed(-90f32.to_radians())
                .delay(2_800),
            fan(15, 3, 120., 2000, 0.).fixed(0.).delay(2_800),
        ],
        // After ≈3.5 s standing he chases with spreads of 4-6 Dazing White Star Stunners and 2-3 White Ball Blasts at the target
        // (ZdjeB9t-n5w t=350.5-353.2); cadences unmeasured.
        Grid => vec![
            fan(18, 5, 15., 1000, 12.).delay(GRID_MS),
            fan(17, 3, 8., 1500, 25.).delay(GRID_MS),
        ],
        // After the heal a ring of 12 White Star Stunners 30° apart (ZdjeB9t-n5w t=413.6), then stars and White Ball Blast fans at the chased
        // player plus the Gaze's twin White Flames (Japanese wiki P18 ×6, P1 ×2, P17 ×3; old-wiki guide "every bullet from" the Gaze).
        Rage => vec![
            once(fan(18, 12, 30., 1, 0.).fixed(0.), RAGE_HEAL_MS),
            fan(18, 6, 12., 1000, 12.).delay(RAGE_HEAL_MS + 1000),
            fan(1, 2, 10., 1000, 8.).delay(RAGE_HEAL_MS + 1000),
            fan(17, 3, 8., 1500, 25.).delay(RAGE_HEAL_MS + 1000),
        ],
    };
    Schedule {
        state: serial * 2 + 1,
        age,
        attacks,
    }
}
/// Oryx's summons (endgame::oryx1). `elapsed` runs from landing.
pub(crate) fn oryx1_summon_schedule(kind: u16, elapsed: u64) -> Schedule {
    use crate::endgame::oryx1::lifetime;
    let fan = Attack::fan;
    let end = lifetime(kind).saturating_sub(100);
    let burst = |projectile: u8, count: usize| {
        fan(projectile, count, 360. / count as f32, 1, 0.)
            .fixed(0.)
            .delay(end)
            .once()
            .enabled(elapsed <= end + 100)
    };
    let attacks = match kind {
        // Assassin Swipe ×3 and Assassin Slash ×4 (Japanese wiki, ≤2012-07-22); "fans of white swipes" in footage. Cadence unmeasured.
        0x1741 => vec![fan(0, 3, 15., 1000, 7.8), fan(1, 4, 20., 1500, 4.8)],
        // Fire Bolt ×3 in a narrow fan and one Green Star, both aimed (−32 / −31 on DEF 43-44 = P1's 75); Fire Bolt every 1 s, Green Star every
        // 1.2 s (Japanese wiki, ≤2012-10-19).
        0x1749 => vec![fan(0, 3, 12., 1000, 9.1), fan(1, 1, 0., 1200, 9.6)],
        // Ring Elements pulse together every 0.84 s (22 intervals), each a ring reaching ≈1.9 tiles; 12 per ring per the Japanese wiki.
        0x1748 => vec![fan(0, 12, 30., 840, 0.).fixed(0.)],
        // Rings of 10 Violet Missiles (two bursts counted) every ≈0.6-0.7 s while an orb is in view (ZdjeB9t-n5w t=99.8-113.7).
        0x1747 => vec![fan(0, 10, 36., 650, 0.).fixed(0.)],
        // Artifact flames fly outward from Oryx, not at players (61 % within 30° of radial; Zdje t=6.65-10.5); World points them outward.
        // On despawn the near ones burst ≈8-12 white balls (Zdje t=26.6-27.8); the far ones just vanish (Japanese wiki). Cadence unmeasured.
        0x174a..=0x174d => vec![fan(0, 1, 0., 750, 0.), burst(1, 8)],
        0x174e => vec![fan(0, 3, 10., 1000, 0.)],
        // White Planet Blast rings (10 per the Japanese wiki; 10-16 seen), then a burst of White Orbs when the phase ends (8-10 seen, LOW).
        0x1743 => vec![fan(0, 10, 36., 1000, 0.).fixed(0.), burst(1, 10)],
        // Black Planets: rings of 10 Planet Shots 36° apart (ZdjeB9t-n5w t=230.3), White Orbs at the end (Japanese wiki).
        0x1746 => vec![fan(0, 10, 36., 1000, 0.).fixed(0.), burst(1, 10)],
        // Black Moons: three Dazing Moon Shots at the player (Japanese wiki; Dazed labels in footage).
        0x1745 => vec![fan(0, 3, 12., 1000, 4.5)],
        // Chase Elements: a Quiet disc every ≈0.73 s (10 per the Japanese wiki) and Quiet bullets on the diagonals toward their neighbours.
        0x1744 => vec![
            fan(0, 10, 36., 750, 0.).fixed(0.),
            fan(1, 4, 90., 1500, 0.).fixed(45f32.to_radians()),
        ],
        _ => vec![],
    };
    Schedule {
        state: 0,
        age: elapsed,
        attacks,
    }
}
