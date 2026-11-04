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
                0 => vec![fan(0, blades(hp), 10., 700, 30.)],
                1 => [(750, 20), (1450, 6), (2250, 15), (2650, 25)]
                    .into_iter()
                    .map(|(delay, n)| Attack::ring(1, n, 50, 30.).fixed(0.).delay(delay).once())
                    .collect(),
                2 => vec![],
                _ => unreachable!(),
            }
        }
        BAT | ESCORT | CHASER => vec![fan(0, 1, 0., 900, 1.)],
        0x1727 => vec![fan(0, 1, 0., 100, 1.)],
        CREATURE => vec![fan(0, 1, 0., 200, 4.).delay(1500)],
        FERATU => vec![fan(0, 5, 9., 1200, 8.)],
        0x1728 => vec![fan(0, 3, 20., 1500, 8.), fan(1, 5, 5., 1700, 9.)],
        0x1729 => vec![fan(0, 2, 180., 1500, 10.), fan(1, 1, 0., 750, 8.)],
        ROCK => vec![Attack::ring(0, 8, 50, 0.).fixed(0.).delay(3000).once()],
        0x172b => vec![fan(0, 5, 10., 1500, 10.)],
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
        (KAGE, 2) => {
            let t = elapsed % 1400;
            let aimed = t >= 1000;
            state += elapsed / 1400 * 2 + u64::from(aimed);
            age = if aimed { t - 1000 } else { t };
            if aimed {
                vec![Attack::fan(1, 2, 180., 400, 20.)]
            } else {
                vec![
                    Attack::fan(1, 2, 180., 300, 0.)
                        .fixed((t / 300) as f32 * 30f32.to_radians())
                        .offset(90.),
                ]
            }
        }
        (KAGE, 3) => vec![Attack::fan(0, 2, 50., 400, 20.)],
        (MINE, 1) => vec![Attack::fan(0, 4, 45., 50, 60.).once()],
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
        0x21c => vec![fan(0, 1, 0., 1000, 10.).predict(1.)],
        0x21d..=0x21f => vec![fan(0, 1, 0., 500, 10.).predict(1.)],
        0x220 => vec![fan(0, 3, 20., 500, 10.)],
        0x936 => {
            state = u64::from(elapsed >= 2000);
            age = elapsed.saturating_sub(2000);
            vec![
                ring(0, 12, 1000, 3000.)
                    .fixed(22.5f32.to_radians())
                    .enabled(state == 1),
                fan(0, 1, 0., 1000, 10.).predict(1.).enabled(state == 1),
                fan(1, 1, 0., 2000, 10.).predict(1.).enabled(state == 1),
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
    use crate::endgame::{ORYX, OryxPhase, oryx_phase};
    let fan = Attack::fan;
    let ring = Attack::ring;
    let mut state = 0;
    let mut age = elapsed;
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
        0x1748 => vec![ring(0, 12, 700, 50.).delay(200)],
        0x174a if elapsed >= 10000 => {
            state = 1;
            age = elapsed - 10000;
            vec![fan(0, 1, 0., 700, 3.)]
        }
        0x174a | 0x174b => vec![fan(0, 3, 10., 1000, 25.)],
        // Each Goo fires its selected speed-0, 6000 ms MultiHit projectile once: the puddle is the hazard.
        0xdf0 => vec![fan(0, 1, 0., 1, 0.).fixed(0.).once()],
        // A landed Quiet Bomb rests 0.8 s, then fires one circle of 18 Mute Spells (7 instances, e.g. bBQRbAoRP_g 2012-04-01 t=256.4; HLraOCBJO94 t=616.2-617.0).
        0xd86 => vec![ring(0, 18, 1, 0.).fixed(0.).delay(800).once()],
        0x944 => vec![
            fan(0, 1, 0., 1500, 8.).chance(1.),
            fan(1, 3, 20., 1500, 8.).delay(500),
        ],
        ORYX if hp <= max_hp / 5 => {
            state = u64::MAX;
            vec![
                fan(1, 2, 10., 1500, 7.).delay(2000),
                fan(16, 5, 10., 1500, 7.).delay(2000),
            ]
        }
        ORYX => {
            let phase = oryx_phase(elapsed);
            if phase != OryxPhase::Opening {
                let t = (elapsed - 20000) % 85000;
                let (index, start) = match phase {
                    OryxPhase::Dance => (1, 0),
                    OryxPhase::Artifacts => (2, 25000),
                    OryxPhase::Gaze => (3, 50000),
                    OryxPhase::DanceTwo => (4, 60000),
                    _ => unreachable!(),
                };
                state = (elapsed - 20000) / 85000 * 4 + index;
                age = t - start;
            }
            match phase {
                OryxPhase::Opening => vec![fan(5, 4, 10., 1000, 25.)],
                OryxPhase::Dance => vec![ring(6, 8, 700, 50.).delay(200)],
                OryxPhase::Artifacts => vec![
                    ring(7, 10, 500, 50.).delay(200),
                    ring(9, 3, 1500, 50.).delay(200),
                    ring(8, 10, 2000, 50.).delay(200),
                ],
                OryxPhase::Gaze => vec![fan(1, 2, 10., 1000, 7.).delay(800)],
                OryxPhase::DanceTwo => vec![
                    ring(8, 4, 200, 0.)
                        .fixed([0f32, 30., 15., 45.][(age / 200 % 4) as usize].to_radians()),
                ],
                _ => unreachable!(),
            }
        }
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
