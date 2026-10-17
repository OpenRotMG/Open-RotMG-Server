use crate::{
    content::Content,
    encounter::{Anchor, Motion, Profile},
};
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, f32::consts::TAU};

pub const CASTLE: &str = "oryx-castle";
pub const CHAMBER: &str = "oryx-chamber";
pub const CELLAR: &str = "wine-cellar";
pub const GUARDIANS: [u16; 2] = [0xd78, 0xd79];
pub const ORYX: u16 = 0x1740;
pub const ORYX2: u16 = 0x932;
pub const LOCKED_CELLAR: u16 = 0x721;
pub const INCANTATION: u16 = 0x722;
pub const HEROES: [u16; 2] = [0x668, 0x67d];
pub const DUNGEONS: &[(u16, &str, &str, u16)] = &[
    (0xd7b, CHAMBER, "Oryx's Chamber", ORYX),
    (0x700, CELLAR, "Wine Cellar", ORYX2),
];
pub const ROSTER: &[u16] = &[
    0x668, 0x669, 0x66a, 0x66b, 0x67d, 0x67e, 0x67f, 0x680, 0x681, 0x682, 0xd70, 0xd71, 0xd78,
    0xd79, 0xd7a, 0xd7c, 0xd7d, 0xd7e, 0xd7f, 0xd80, 0xd81, 0xd82, 0xd83, 0xd84, 0xd87, 0x1740,
    0x1748, 0x1749, 0x174a, 0x174b, 0x932, 0x944, 0xde0, 0xde1, 0xde2, 0xde3, 0xde4, 0xdf1, 0xdf2,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealmStage {
    Open,
    Closing { deadline: u64 },
    Sealed { deadline: u64 },
    Transferring,
}
pub struct RealmCycle {
    pub remaining: BTreeSet<i32>,
    pub stage: RealmStage,
}
impl RealmCycle {
    pub fn new(heroes: BTreeSet<i32>) -> Result<Self> {
        ensure!(
            !heroes.is_empty(),
            "Realm closure needs a finite initial hero roster"
        );
        Ok(Self {
            remaining: heroes,
            stage: RealmStage::Open,
        })
    }
    pub fn defeated(&mut self, id: i32, now: u64) -> bool {
        if self.stage != RealmStage::Open || !self.remaining.remove(&id) {
            return false;
        }
        if self.remaining.is_empty() {
            self.stage = RealmStage::Closing {
                deadline: now + 60_000,
            };
        }
        true
    }
    pub fn advance(&mut self, now: u64) -> bool {
        match self.stage {
            RealmStage::Closing { deadline } if now >= deadline => {
                self.stage = RealmStage::Sealed {
                    deadline: deadline + 22_000,
                };
                true
            }
            RealmStage::Sealed { deadline } if now >= deadline => {
                self.stage = RealmStage::Transferring;
                true
            }
            _ => false,
        }
    }
    pub fn admits(&self) -> bool {
        self.stage == RealmStage::Open
    }
}

pub fn profile(kind: u16) -> Option<Profile> {
    let (motion, speed, interval, range) = match kind {
        0x668 => (Motion::WanderHome { radius: 1. }, 3., 500, 10.),
        0x669 => (Motion::Wander, 7., 250, 10.),
        0x66a | 0x66b => (Motion::Wander, 7., 1000, 10.),
        0x67d => (Motion::WanderHome { radius: 3. }, 3., 1000, 12.),
        0x67e..=0x682 => (
            Motion::Chase {
                sight: 10.,
                stop: 2.,
            },
            10.,
            1000,
            10.,
        ),
        0xd70 | 0xd71 | 0xd7a | 0xd7d | 0xdf1 => (Motion::Stationary, 0., 0, 0.),
        0xd78 | 0xd79 => (
            Motion::Orbit {
                anchor: Anchor::Home,
                radius: 3.,
            },
            6.,
            200,
            20.,
        ),
        0xd7c => (
            Motion::Orbit {
                anchor: Anchor::Parent,
                radius: 5.,
            },
            7.,
            900,
            12.,
        ),
        0xd7e => (
            Motion::Chase {
                sight: 10.,
                stop: 2.,
            },
            3.,
            600,
            10.,
        ),
        0xd7f => (Motion::Wander, 2., 900, 10.),
        0xd80 => (
            Motion::Chase {
                sight: 10.,
                stop: 1.,
            },
            5.,
            250,
            10.,
        ),
        0xd81 => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            4.,
            500,
            10.,
        ),
        0xd82 => (
            Motion::Chase {
                sight: 10.,
                stop: 1.,
            },
            4.,
            300,
            10.,
        ),
        0xd83 => (
            Motion::Chase {
                sight: 10.,
                stop: 3.,
            },
            4.,
            350,
            10.,
        ),
        0xd84 => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            4.,
            200,
            10.,
        ),
        0xd87 => (Motion::Stationary, 0., 500, 5.),
        ORYX => (Motion::Stationary, 0., 200, 25.),
        ORYX2 => (
            Motion::Chase {
                sight: 25.,
                stop: 3.,
            },
            2.,
            500,
            25.,
        ),
        0x1748 => (Motion::Stationary, 0., 700, 25.),
        0x174a => (
            Motion::Orbit {
                anchor: Anchor::Parent,
                radius: 1.,
            },
            3.,
            700,
            25.,
        ),
        0x174b => (
            Motion::Orbit {
                anchor: Anchor::Parent,
                radius: 9.,
            },
            3.,
            700,
            25.,
        ),
        0x1749 => (Motion::Wander, 3., 1000, 3.),
        0x944 => (Motion::Wander, 2., 2500, 15.),
        0xde0 => (
            Motion::Chase {
                sight: 12.,
                stop: 3.,
            },
            3.,
            1000,
            12.,
        ),
        0xde1 | 0xde3 => (Motion::Wander, 2., 0, 0.),
        0xde4 => (Motion::Stationary, 0., 1000, 3.),
        0xde2 => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            6.,
            1000,
            4.,
        ),
        0xdf2 => (Motion::Wander, 2., 800, 10.),
        _ => return None,
    };
    Some(Profile {
        motion,
        speed,
        interval,
        range,
    })
}
fn fan(index: u8, count: usize, gap: f32, heading: f32) -> Vec<(u8, f32)> {
    (0..count)
        .map(|i| {
            (
                index,
                heading + (i as f32 - (count - 1) as f32 / 2.) * gap.to_radians(),
            )
        })
        .collect()
}
fn ring(index: u8, count: usize, heading: f32) -> Vec<(u8, f32)> {
    (0..count)
        .map(|i| (index, heading + i as f32 * TAU / count as f32))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OryxPhase {
    Opening,
    Dance,
    Artifacts,
    Gaze,
    DanceTwo,
    PrepareRage,
    Rage,
}
pub fn oryx_phase(elapsed: u64) -> OryxPhase {
    if elapsed < 20_000 {
        return OryxPhase::Opening;
    }
    match (elapsed - 20_000) % 85_000 {
        0..25_000 => OryxPhase::Dance,
        25_000..50_000 => OryxPhase::Artifacts,
        50_000..60_000 => OryxPhase::Gaze,
        _ => OryxPhase::DanceTwo,
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Wave {
    pub kind: u16,
    pub count: usize,
    pub radius: f32,
    pub lifetime: u64,
}
pub struct BossFacts {
    pub phase: OryxPhase,
    pub changed: bool,
    pub invulnerable: bool,
    pub waves: Vec<Wave>,
}
#[derive(Default, Clone)]
pub struct BossBrain {
    phase: Option<OryxPhase>,
    rage_at: Option<u64>,
    next_summon: u64,
    last_elapsed: Option<u64>,
}
impl BossBrain {
    pub fn phase(&self) -> Option<OryxPhase> {
        self.phase
    }
    pub fn rage_elapsed(&self, elapsed: u64) -> Option<u64> {
        self.rage_at.map(|start| elapsed - start)
    }
    pub fn update(&mut self, kind: u16, hp: i32, max_hp: i32, elapsed: u64) -> Result<BossFacts> {
        ensure!(
            matches!(kind, ORYX | ORYX2) && hp > 0 && max_hp > 0,
            "invalid live Oryx phase input"
        );
        ensure!(
            self.last_elapsed.is_none_or(|last| elapsed >= last),
            "Oryx phase clock regressed"
        );
        self.last_elapsed = Some(elapsed);
        if hp <= max_hp / 5 {
            self.rage_at.get_or_insert(elapsed);
        }
        let phase = if kind == ORYX {
            if hp <= max_hp / 5 {
                OryxPhase::Rage
            } else {
                oryx_phase(elapsed)
            }
        } else if let Some(start) = self.rage_at {
            ensure!(elapsed >= start, "Oryx 2 phase clock regressed");
            if elapsed - start < 10_000 {
                OryxPhase::PrepareRage
            } else {
                OryxPhase::Rage
            }
        } else {
            OryxPhase::Opening
        };
        let changed = self.phase != Some(phase);
        self.phase = Some(phase);
        let mut waves = vec![];
        if changed && kind == ORYX {
            match phase {
                OryxPhase::Dance | OryxPhase::DanceTwo => waves.push(Wave {
                    kind: 0x1748,
                    count: 15,
                    radius: 9.,
                    lifetime: 20_000,
                }),
                OryxPhase::Artifacts => {
                    waves.push(Wave {
                        kind: 0x174a,
                        count: 4,
                        radius: 1.,
                        lifetime: 20_000,
                    });
                    waves.push(Wave {
                        kind: 0x174b,
                        count: 4,
                        radius: 9.,
                        lifetime: 20_000,
                    });
                }
                _ => {}
            }
        }
        if elapsed >= self.next_summon && matches!(phase, OryxPhase::Opening | OryxPhase::Rage) {
            if kind == ORYX {
                waves.push(Wave {
                    kind: 0x1749,
                    count: if phase == OryxPhase::Rage { 10 } else { 5 },
                    radius: 3.,
                    lifetime: 60_000,
                });
                self.next_summon = elapsed + 1500;
            } else if phase == OryxPhase::Opening {
                waves.push(Wave {
                    kind: 0x944,
                    count: 5,
                    radius: 3.,
                    lifetime: 60_000,
                });
                self.next_summon = elapsed + 5000;
            }
        }
        Ok(BossFacts {
            phase,
            changed,
            invulnerable: matches!(
                phase,
                OryxPhase::Dance | OryxPhase::DanceTwo | OryxPhase::PrepareRage
            ),
            waves,
        })
    }
}
#[derive(Default)]
pub struct GuardianBrain {
    recovery_at: Option<u64>,
    sword: Option<i32>,
    sword_cleared_at: Option<u64>,
}
pub struct GuardianFacts {
    pub invulnerable: bool,
    pub summon_sword: bool,
    pub heal: bool,
}
impl GuardianBrain {
    pub fn awaiting_sword(&self) -> Option<i32> {
        self.sword.filter(|_| self.sword_cleared_at.is_none())
    }
    pub fn update(&mut self, peer_alive: bool, now: u64) -> Result<GuardianFacts> {
        if peer_alive || self.sword_cleared_at.is_some_and(|t| now >= t + 1400) {
            return Ok(GuardianFacts {
                invulnerable: false,
                summon_sword: false,
                heal: false,
            });
        }
        let start = *self.recovery_at.get_or_insert(now);
        ensure!(now >= start, "Guardian phase clock regressed");
        let summon_sword = now >= start + 4000 && self.sword.is_none();
        Ok(GuardianFacts {
            invulnerable: true,
            summon_sword,
            heal: summon_sword,
        })
    }
    pub fn bind_sword(&mut self, id: i32) -> Result<()> {
        ensure!(
            self.sword.is_none() && self.recovery_at.is_some(),
            "Guardian sword already bound or recovery not started"
        );
        self.sword = Some(id);
        Ok(())
    }
    pub fn sword_defeated(&mut self, id: i32, now: u64) -> Result<()> {
        ensure!(
            self.sword == Some(id) && self.sword_cleared_at.is_none(),
            "foreign or replayed Guardian sword defeat"
        );
        ensure!(
            self.recovery_at.is_some_and(|start| now >= start + 4000),
            "Guardian sword defeat clock regressed"
        );
        self.sword_cleared_at = Some(now);
        Ok(())
    }
}
pub fn attacks(kind: u16, hp: i32, max_hp: i32, heading: f32, elapsed: u64) -> Vec<(u8, f32)> {
    if let Some(schedule) = crate::encounter::attacks::endgame_schedule(kind, hp, max_hp, elapsed) {
        return schedule.snapshot(heading);
    }
    match kind {
        0x669 | 0x67e..=0x682 | 0xd84 | 0xd87 | 0xde2 | 0xdf2 => vec![(0, heading)],
        0x67d if elapsed % 10_000 < 5000 => fan(10, 3, 10., heading),
        0x67d => (0..4).map(|index| (index, heading)).collect(),
        0xd70 | 0xd71 | 0xd7a | 0xd7d | 0xde1 | 0xde3 | 0xdf1 => vec![],
        0xd78 | 0xd79 => match elapsed % 20_000 {
            0..5200 => ring(0, 2, elapsed as f32 / 200. * 30f32.to_radians()),
            5200..10_000 => fan(0, 2, 180., (elapsed as f32 / 100. * 10.).to_radians()),
            10_000..14_000 => ring(1, 7, heading),
            14_000..18_000 => fan(0, 5, 10., heading),
            _ => ring(2, 10, heading),
        },
        0xd7c => ring(0, 6, heading),
        0xd7f => vec![(0, heading)],
        0xd80 => {
            if elapsed % 1500 < 250 {
                ring(0, 5, heading)
            } else {
                vec![(0, heading)]
            }
        }
        0xd81 => {
            if elapsed % 1000 < 500 {
                ring(0, 5, heading)
            } else {
                vec![(1, heading)]
            }
        }
        ORYX2 => oryx2_attacks(
            if hp <= max_hp / 5 {
                OryxPhase::Rage
            } else {
                OryxPhase::Opening
            },
            elapsed,
            heading,
        ),
        0x1749 => {
            let mut shots = fan(0, 3, 10., heading);
            shots.extend(fan(1, 3, 10., heading));
            shots
        }
        0xde4 => fan(0, 5, 7., heading),
        0xde0 => (0..5).map(|i| (i, heading + i as f32 * TAU / 5.)).collect(),
        _ => panic!("endgame attack requested for unselected object {kind:#x}"),
    }
}

pub fn oryx2_attacks(phase: OryxPhase, age: u64, heading: f32) -> Vec<(u8, f32)> {
    crate::encounter::attacks::oryx2_schedule(phase, age).snapshot(heading)
}
pub fn validate_kind(content: &Content, kind: u16) -> Result<()> {
    let object = content.get(kind)?;
    ensure!(
        profile(kind).is_some(),
        "unselected endgame encounter {kind:#x}"
    );
    for hp in [object.hp, object.hp / 2, (object.hp / 10).max(1)] {
        for elapsed in (0..110_000).step_by(100) {
            for (index, angle) in attacks(kind, hp, object.hp, 0., elapsed) {
                ensure!(
                    angle.is_finite() && object.projectiles.contains_key(&index),
                    "endgame {kind:#x} emits absent selected projectile {index}"
                );
            }
        }
    }
    Ok(())
}
pub fn summons(kind: u16) -> &'static [Wave] {
    match kind {
        0x668 => &[
            Wave {
                kind: 0x669,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0x66a,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0x66b,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
        ],
        0x67d => &[
            Wave {
                kind: 0x67e,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0x67f,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0x680,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0x681,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0x682,
                count: 3,
                radius: 3.,
                lifetime: 60_000,
            },
        ],
        0xd7d => &[Wave {
            kind: 0xd7e,
            count: 1,
            radius: 1.,
            lifetime: 60_000,
        }],
        0xd7f => &[Wave {
            kind: 0xd80,
            count: 20,
            radius: 3.,
            lifetime: 60_000,
        }],
        0xd82 => &[Wave {
            kind: 0xd81,
            count: 4,
            radius: 3.,
            lifetime: 60_000,
        }],
        0x944 => &[
            Wave {
                kind: 0xdf2,
                count: 1,
                radius: 2.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0xde0,
                count: 1,
                radius: 2.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0xde1,
                count: 1,
                radius: 2.,
                lifetime: 60_000,
            },
            Wave {
                kind: 0xde3,
                count: 1,
                radius: 2.,
                lifetime: 60_000,
            },
        ],
        0xde1 => &[Wave {
            kind: 0xde2,
            count: 1,
            radius: 1.,
            lifetime: 10_000,
        }],
        0xde3 => &[Wave {
            kind: 0xde4,
            count: 1,
            radius: 5.,
            lifetime: 10_000,
        }],
        kind if ROSTER.contains(&kind) => &[],
        _ => panic!("summons requested for unselected endgame object {kind:#x}"),
    }
}
pub(crate) fn loot(kind: u16) -> &'static [(f64, crate::loot::Pick)] {
    use crate::loot::{
        ABILITIES, ARMOR,
        Pick::{Item, Tier},
        RINGS, WEAPONS,
    };
    match kind {
        0x668 => &[
            (0.02, Item(0xa02)),
            (0.02, Item(0xa2a)),
            (0.02, Item(0xa6a)),
            (0.01, Item(0xa21)),
            (0.01, Item(0xa1f)),
        ],
        0xd78 => &[(1., Item(0xa20)), (0.003, Item(0xc05))],
        0xd79 => &[(1., Item(0xa20))],
        ORYX => &[
            (0.3, Item(0xa1f)),
            (0.3, Item(0xa20)),
            (0.07, Tier(10, WEAPONS)),
            (0.06, Tier(11, WEAPONS)),
            (0.07, Tier(5, ABILITIES)),
            (0.07, Tier(11, ARMOR)),
            (0.06, Tier(5, RINGS)),
        ],
        ORYX2 => &[
            (1., Item(0xa34)),
            (0.3, Item(0xa1f)),
            (0.3, Item(0xa20)),
            (0.3, Item(0xa35)),
            (0.07, Tier(10, WEAPONS)),
            (0.06, Tier(11, WEAPONS)),
            (0.05, Tier(12, WEAPONS)),
            (0.07, Tier(5, ABILITIES)),
            (0.05, Tier(6, ABILITIES)),
            (0.07, Tier(11, ARMOR)),
            (0.06, Tier(12, ARMOR)),
            (0.05, Tier(13, ARMOR)),
            (0.06, Tier(5, RINGS)),
        ],
        0x1749 => &[(0.2, Tier(7, WEAPONS)), (0.03, Item(0xa22))],
        0x669..=0x66b
        | 0x67e..=0x682
        | 0xd7c
        | 0xd7e
        | 0xd7f
        | 0xd80..=0xd84
        | 0x944
        | 0xde0
        | 0xdf2 => &[(0.03, Item(0xa22)), (0.02, Item(0xa23))],
        0x67d
        | 0xd70
        | 0xd71
        | 0xd7a
        | 0xd7d
        | 0xd87
        | 0x1748
        | 0x174a
        | 0x174b
        | 0xde1..=0xde4
        | 0xdf1 => &[],
        _ => panic!("unselected endgame loot {kind:#x}"),
    }
}
pub fn validate(content: &Content) -> Result<()> {
    for (kind, hp, defense) in [
        (ORYX, 85_000, 60),
        (ORYX2, 75_000, 60),
        (0xd78, 60_000, 100),
        (0xd79, 60_000, 100),
    ] {
        let o = content.get(kind)?;
        ensure!(
            o.hp == hp && o.defense == defense,
            "selected endgame strength changed for {kind:#x}"
        );
    }
    for kind in HEROES {
        ensure!(
            content.get(kind)?.quest,
            "closure hero {kind:#x} is not a selected-client quest"
        );
    }
    for kind in ROSTER {
        crate::loot::validate(content, *kind)?;
        validate_kind(content, *kind)?;
        for wave in summons(*kind) {
            ensure!(
                wave.count > 0 && wave.count <= 20 && wave.radius.is_finite() && wave.radius > 0.,
                "invalid bounded summon wave for {kind:#x}"
            );
            ensure!(
                content.get(wave.kind)?.enemy && ROSTER.contains(&wave.kind),
                "unhandled summon {:#x}",
                wave.kind
            );
        }
    }
    for kind in [LOCKED_CELLAR, 0x700, 0xd7b] {
        ensure!(
            content.get(kind)?.class == "Portal",
            "endgame portal contract mismatch {kind:#x}"
        );
    }
    let inc = content.get(INCANTATION)?;
    ensure!(
        inc.activate == "UnlockPortal",
        "Incantation activation contract mismatch"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Map;
    use std::{
        collections::{BTreeMap, VecDeque},
        path::Path,
    };
    #[test]
    fn finite_hero_credit_never_counts_summons_replays_or_despawns() {
        assert!(RealmCycle::new(BTreeSet::new()).is_err());
        let mut realm = RealmCycle::new(BTreeSet::from([7, 9])).unwrap();
        assert!(!realm.defeated(100, 0));
        assert!(realm.defeated(7, 100));
        assert!(!realm.defeated(7, 200));
        assert!(realm.admits());
        assert!(!realm.advance(1_000_000));
        assert!(realm.defeated(9, 200));
        assert!(!realm.admits());
        assert!(!realm.advance(60_199));
        assert!(realm.advance(60_200));
        assert!(!realm.advance(82_199));
        assert!(realm.advance(82_200));
        assert_eq!(realm.stage, RealmStage::Transferring);
        assert!(!realm.advance(200_000));
        assert!(!realm.defeated(9, 200_001));
    }
    #[test]
    fn selected_xml_covers_all_phase_projectiles_and_different_boss_patterns() {
        let content = Content::load(Path::new("data")).unwrap();
        validate(&content).unwrap();
        assert_eq!(oryx_phase(19_999), OryxPhase::Opening);
        assert_eq!(oryx_phase(20_000), OryxPhase::Dance);
        assert_eq!(oryx_phase(45_000), OryxPhase::Artifacts);
        assert_eq!(oryx_phase(70_000), OryxPhase::Gaze);
        assert_eq!(oryx_phase(80_000), OryxPhase::DanceTwo);
        assert_ne!(
            attacks(ORYX, 85_000, 85_000, 0., 0),
            attacks(ORYX2, 75_000, 75_000, 0., 0)
        );
        assert!(
            attacks(ORYX, 17_000, 85_000, 0., 2000)
                .iter()
                .any(|(i, _)| *i == 16)
        );
    }
    #[test]
    fn boss_waves_are_phase_bound_and_oryx_two_rage_has_a_real_protected_transition() {
        let mut brain = BossBrain::default();
        let initial = brain.update(ORYX, 85_000, 85_000, 0).unwrap();
        assert!(initial.changed && !initial.invulnerable);
        assert_eq!((initial.waves[0].kind, initial.waves[0].count), (0x1749, 5));
        let dance = brain.update(ORYX, 80_000, 85_000, 20_000).unwrap();
        assert!(dance.invulnerable && dance.changed);
        assert_eq!(
            (
                dance.waves[0].kind,
                dance.waves[0].count,
                dance.waves[0].lifetime
            ),
            (0x1748, 15, 20_000)
        );
        assert!(
            brain
                .update(ORYX, 80_000, 85_000, 20_050)
                .unwrap()
                .waves
                .is_empty()
        );
        let artifacts = brain.update(ORYX, 75_000, 85_000, 45_000).unwrap();
        assert!(!artifacts.invulnerable && artifacts.waves.len() == 2);
        assert_eq!(oryx_phase(105_000), OryxPhase::Dance);
        assert!(brain.update(ORYX, 75_000, 85_000, 44_999).is_err());
        let rage = brain.update(ORYX, 17_000, 85_000, 45_050).unwrap();
        assert_eq!(rage.phase, OryxPhase::Rage);
        assert!(!rage.invulnerable && rage.waves[0].count == 10);
        let mut brain = BossBrain::default();
        let prepare = brain.update(ORYX2, 15_000, 75_000, 500).unwrap();
        assert_eq!(prepare.phase, OryxPhase::PrepareRage);
        assert!(prepare.invulnerable && prepare.waves.is_empty());
        assert!(
            brain
                .update(ORYX2, 16_000, 75_000, 10_499)
                .unwrap()
                .invulnerable
        );
        let rage = brain.update(ORYX2, 16_000, 75_000, 10_500).unwrap();
        assert_eq!(rage.phase, OryxPhase::Rage);
        assert!(!rage.invulnerable && rage.waves.is_empty());
    }
    #[test]
    fn surviving_guardian_requires_a_real_bound_sword_defeat_and_recovery() {
        let mut brain = GuardianBrain::default();
        assert!(!brain.update(true, 100).unwrap().invulnerable);
        assert!(brain.bind_sword(50).is_err());
        assert!(brain.update(false, 200).unwrap().invulnerable);
        assert!(!brain.update(false, 4199).unwrap().summon_sword);
        let ready = brain.update(false, 4200).unwrap();
        assert!(ready.summon_sword && ready.heal);
        assert!(brain.update(false, 4250).unwrap().summon_sword);
        brain.bind_sword(50).unwrap();
        assert!(brain.bind_sword(51).is_err());
        assert!(brain.update(false, 20_000).unwrap().invulnerable);
        assert!(!brain.update(false, 20_000).unwrap().summon_sword);
        assert!(brain.sword_defeated(51, 20_000).is_err());
        brain.sword_defeated(50, 20_000).unwrap();
        assert!(brain.sword_defeated(50, 20_000).is_err());
        assert!(brain.update(false, 21_399).unwrap().invulnerable);
        assert!(!brain.update(false, 21_400).unwrap().invulnerable);
    }
    #[test]
    fn complete_maps_preserve_supported_rosters_and_connected_spawn_boss_routes() {
        let content = Content::load(Path::new("data")).unwrap();
        for (template, bosses) in [
            (CASTLE, GUARDIANS.as_slice()),
            (CHAMBER, &[ORYX][..]),
            (CELLAR, &[ORYX2][..]),
        ] {
            let map = Map::load(
                &Path::new("data").join(format!("{template}.wmap")),
                &content,
            )
            .unwrap();
            let mut counts = BTreeMap::new();
            for cell in &map.cells {
                if let Some(kind) = cell.object {
                    *counts.entry(kind).or_insert(0) += 1;
                }
            }
            for boss in bosses {
                assert_eq!(counts[boss], 1);
            }
            if template == CASTLE {
                assert_eq!(counts[&0xd83], 103);
                assert_eq!(counts[&0xd7f], 6);
            }
            if template == CELLAR {
                assert_eq!(counts[&0x944], 44);
            }
            for cell in &map.cells {
                if let Some(kind) = cell.object
                    && content.objects[&kind].enemy
                {
                    assert!(
                        profile(kind).is_some(),
                        "{template} contains unhandled enemy {kind:#x}"
                    );
                }
            }
            let (x, y) = map.spawns[0];
            let start = (y as i32 * map.width + x as i32) as usize;
            let mut visited = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            while let Some(i) = queue.pop_front() {
                let (x, y) = (i as i32 % map.width, i as i32 / map.width);
                for (x, y) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if x < 0 || y < 0 || x >= map.width || y >= map.height {
                        continue;
                    }
                    let n = (y * map.width + x) as usize;
                    let c = &map.cells[n];
                    if content.grounds[&c.ground] > 0.
                        && c.object.is_none_or(|k| {
                            !content.objects[&k].blocks || content.objects[&k].destructible
                        })
                        && visited.insert(n)
                    {
                        queue.push_back(n);
                    }
                }
            }
            for (i, c) in map.cells.iter().enumerate() {
                if c.object.is_some_and(|k| bosses.contains(&k)) {
                    assert!(visited.contains(&i), "{template} boss is unreachable");
                }
            }
        }
    }
}
