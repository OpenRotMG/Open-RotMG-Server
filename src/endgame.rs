pub mod oryx1;

use crate::{
    content::Content,
    encounter::{Motion, Profile},
};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::TAU,
};

// The January 2012 FAQ specifies a two-minute lockout, not later forks' 22 seconds: https://web.archive.org/web/20120106105406id_/http://forums.wildshadow.com/wiki/realm-mad-god/faq
pub const CLOSE_DELAY_MS: u64 = 120_000;
pub const EARTHQUAKE_MS: u64 = 8_000;
pub const REPLACEMENT_DELAY_MS: u64 = 2_000;
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
    0xd79, 0xd7a, 0xd7c, 0xd7d, 0xd7e, 0xd7f, 0xd80, 0xd81, 0xd82, 0xd83, 0xd84, 0xd86, 0xd87,
    0x1740, 0x1741, 0x1742, 0x1743, 0x1744, 0x1745, 0x1746, 0x1747, 0x1748, 0x1749, 0x174a, 0x174b,
    0x174c, 0x174d, 0x174e, 0x932, 0x944, 0xde0, 0xde1, 0xde2, 0xde3, 0xde4, 0xde5, 0xdf0, 0xdf1,
    0xdf2,
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
                deadline: now + CLOSE_DELAY_MS,
            };
        }
        true
    }
    pub fn advance(&mut self, now: u64) -> bool {
        match self.stage {
            RealmStage::Closing { deadline } if now >= deadline => {
                self.stage = RealmStage::Sealed {
                    deadline: deadline + EARTHQUAKE_MS,
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
        0xd70 | 0xd71 | 0xd7a | 0xd7d | 0xde5 => (Motion::Stationary, 0., 0, 0.),
        // A Quiet Bomb only fires its scheduled landing burst.
        0xd86 => (Motion::Stationary, 0., 1000, 0.),
        0xdf0 => (Motion::Stationary, 0., 6000, 0.),
        // Bile "moves around randomly" (https://web.archive.org/web/20160522054322/http://www.realmeye.com/wiki/bile-of-oryx); Wander(.5) from a later comparator: https://github.com/Omniraptyr/Valor-Server/blob/40fab9e80d8a44e922036d178b9d1f03aca66a44/VotR-Server/wServer/logic/db/BehaviorDb.Oryx.cs#L493-L501
        0xdf1 => (Motion::Wander, crate::encounter::nr_speed(0.5), 0, 0.),
        // Awake Guardians leave the dais and follow players around the hall (bBQRbAoRP_g t=345-440, HLraOCBJO94 t=638-656); sight and stop are reconstruction.
        0xd78 | 0xd79 => (
            Motion::Chase {
                sight: 10.,
                stop: 3.,
            },
            6.,
            200,
            20.,
        ),
        // The sword stands in the ring core (bBQRbAoRP_g t=453); its 200 ms cadence is reconstruction.
        0xd7c => (Motion::Stationary, 0., 200, 12.),
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
        // A fresh Eye Warrior lunged at ≈7.5 tiles/s (peaks 10-13) and stopped ≈1.5 tiles from the player (fzgFSFVN8nY 2012-08-10).
        0xd81 => (
            Motion::Chase {
                sight: 10.,
                stop: 1.5,
            },
            tiles_per_s(7.5),
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
        // Holds its spot while a player closes from 6-7 tiles, then lunges ≈2-2.4 tiles and stops 1.1-1.6 tiles short: ≈10 tiles/s
        // (fzgFSFVN8nY 2012-08-10) and 5.9 average / 7.8 peak (wpi7Ff-mOD4 2012-06-18 t=81-82.8). The cooldown is not on film (2013 text: ≈5 s).
        0xd83 => (
            Motion::Charge {
                sight: 3.5,
                cooldown: 3000,
                delay: 0,
                stop: 1.2,
                idle: 0.,
            },
            tiles_per_s(8.),
            350,
            10.,
        ),
        // One tracked Pet dropped a bubble every 0.3-0.5 s (bBQRbAoRP_g 2012-04-01 t=59.4-60.6).
        0xd84 => (
            Motion::Chase {
                sight: 10.,
                stop: 0.,
            },
            4.,
            400,
            10.,
        ),
        // "If a player gets closer, it will fire rings of mute spells" (2013 wiki: https://www.realmeye.com/wiki/oryx-s-living-floor/1; footage HLraOCBJO94 t=624).
        // Rings every ≈0.8-1.0 s (HLraOCBJO94 2012-03-19 t=624-625.2, bBQRbAoRP_g t=318-336). Both floors of a gate pair rang with the player 5.8-6.0 and
        // 6.9-7.2 tiles away (fzgFSFVN8nY 2012-08-10 t=95.00), so the trigger reaches about 7 tiles, not the 5 of a later comparator.
        0xd87 => (Motion::Stationary, 0., 900, 7.),
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
        // Elements stand or orbit Oryx per World::finale_movement; they fire from schedules.
        0x1742..=0x1748 | 0x174a..=0x174e => (Motion::Stationary, 0., 200, 25.),
        // Assassins chase to 1-3 tiles in 2012 footage; "approximately matching the player's speed" (RealmEye rev 1, old wiki).
        0x1741 => (
            Motion::Chase {
                sight: 15.,
                stop: 1.5,
            },
            tiles_per_s(oryx1::ASSASSIN_SPEED),
            200,
            8.,
        ),
        // Minions move toward the nearest player and wander when none is visible (2012 footage, LOW).
        0x1749 => (
            Motion::Follow {
                sight: 10.,
                stop: 3.,
                idle: tiles_per_s(1.5),
                duration: 0,
                cooldown: 0,
            },
            tiles_per_s(oryx1::MINION_SPEED),
            200,
            10.,
        ),
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
    PrepareRage,
    Rage,
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
            kind == ORYX2 && hp > 0 && max_hp > 0,
            "invalid live Oryx 2 phase input"
        );
        ensure!(
            self.last_elapsed.is_none_or(|last| elapsed >= last),
            "Oryx phase clock regressed"
        );
        self.last_elapsed = Some(elapsed);
        if hp <= max_hp / 5 {
            self.rage_at.get_or_insert(elapsed);
        }
        let phase = if let Some(start) = self.rage_at {
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
        if elapsed >= self.next_summon && phase == OryxPhase::Opening {
            waves.push(Wave {
                kind: 0x944,
                count: 5,
                radius: 3.,
                lifetime: 60_000,
            });
            self.next_summon = elapsed + 5000;
        }
        Ok(BossFacts {
            phase,
            changed,
            invulnerable: phase == OryxPhase::PrepareRage,
            waves,
        })
    }
}
/// Stat 61 for an awake guardian: the spin toggles AltTexture 3 ↔ 4 on every volley, starting with 3; the client's 300 ms attack frames of those
/// textures then show back, side, front, side every 0.40 s (HLraOCBJO94 t=642.28-647.16 at 30 fps, bBQRbAoRP_g t=428.08-429.75). Every other block uses 0.
pub fn guardian_texture(block: GuardianBlock, age: u64) -> i32 {
    match block {
        GuardianBlock::Spin
            if (GUARDIAN_SPIN_FIRST_MS..GUARDIAN_SPIN_FIRST_MS + 27 * 200).contains(&age) =>
        {
            if ((age - GUARDIAN_SPIN_FIRST_MS) / 200).is_multiple_of(2) {
                3
            } else {
                4
            }
        }
        _ => 0,
    }
}
/// One Stone Guardian attack block. The 2013 Japanese wiki lists six phases chosen at random; all six are in 2012 footage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardianBlock {
    /// Per guardian: walk toward players firing the fixed 10-shot white ring.
    Ring,
    /// Per guardian: charge the nearest player firing aimed 5-shot fans.
    Fan,
    /// Joint: "Sword Dance: Spins while shooting out bullets at a rapid rate" (Wild Shadow wiki, 2012-03-18): two arms turning 40° per 0.2 s.
    Spin,
    /// Joint: four fixed directions × 2 shots every second.
    Quad,
    /// Joint: back to back in the ring core, two P1 fans per guardian per second.
    Centre,
    /// Joint: "Throw and Blow: Runs to the side and shoots bullets from the right and the left leading into the middle while throwing bombs" (Wild Shadow wiki, 2012-03-18).
    Side,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardianClock {
    pub block: GuardianBlock,
    pub serial: u64,
    pub start: u64,
    pub end: u64,
}
/// Engine speed for a measured ground speed: an NPC moves `speed / 1.5` tiles per second (World::npc_step, charge_goal).
pub const fn tiles_per_s(measured: f32) -> f32 {
    measured * 1.5
}
// Movement measured from the never-rotated 123.5.1 minimap (4 px per tile) in HLraOCBJO94, bBQRbAoRP_g and wpi7Ff-mOD4.
// Runs always go to fixed points (core, ring points, side walls, dais) at ≈10 tiles/s (7-13); the core and wall runs end before the first shot.
pub const GUARDIAN_RUN_MS: u64 = 1500;
pub const GUARDIAN_RUN_SPEED: f32 = tiles_per_s(10.);
// Only the chase-fan chases: ≈5-6 tiles/s, trailing the player by 2-5 tiles (never closer than 2.2).
pub const GUARDIAN_CHASE_SPEED: f32 = tiles_per_s(5.5);
// Four-way pairs: first volley 0.5 s into the block.
pub const GUARDIAN_SPLIT_MS: u64 = 500;
// Spin: run to the core (≈1.3 s before the first volley), split to the ring's east (Right) / west (Left) point, the first volley fired during the split.
pub const GUARDIAN_SPIN_CORE_MS: u64 = 1000;
pub const GUARDIAN_SPIN_FIRST_MS: u64 = 1100;
pub const GUARDIAN_SPIN_ORBIT_MS: u64 = 1300;
// Both orbit the core counter-clockwise (north up) at 4.0 tiles, 180° apart: 28.5°/s in the spin (8 blocks, 3 fights), 51°/s in the four-way pairs (2 blocks, 1 fight).
pub const GUARDIAN_ORBIT_RADIUS: f32 = 4.;
pub const GUARDIAN_SPIN_DEG_S: f32 = 28.5;
pub const GUARDIAN_QUAD_DEG_S: f32 = 51.;
// Side block: along the core's row to just inside the armour strip.
pub const GUARDIAN_WALL_X: f32 = 10.3;
// The dais centre, where the survivor throws from: 10 tiles north of the core (bBQRbAoRP_g t=450.6-451.6).
pub const GUARDIAN_DAIS_Y: f32 = 10.;
/// Joint blocks start together at random, about every 9.5 s (5-17 s, four 2012 fights; no fixed order, repeats allowed).
/// Between them each guardian ring-walks and sometimes breaks into a chase-fan on its own (four cases of one in F while the other ring-walks).
pub struct GuardianDuo {
    joint: Option<GuardianClock>,
    next_joint: u64,
    solo: BTreeMap<i32, GuardianClock>,
    serial: u64,
}
impl GuardianDuo {
    pub fn new(now: u64, rng: &mut impl rand::Rng) -> Self {
        // The first joint block came 3.3-8.9 s after waking (bB 3.3, HL 5.2, 5Z 8.9).
        Self {
            joint: None,
            next_joint: now + rng.gen_range(3500..=9000),
            solo: BTreeMap::new(),
            serial: 0,
        }
    }
    fn update(&mut self, now: u64, rng: &mut impl rand::Rng) {
        if self.joint.is_some_and(|joint| now >= joint.end) {
            self.joint = None;
        }
        if self.joint.is_some() || now < self.next_joint {
            return;
        }
        // Weighted by how often each joint block appeared in four 2012 fights: spin 8, centre 6, four-way 4, side 4.
        let block = match rng.gen_range(0..22) {
            0..8 => GuardianBlock::Spin,
            8..14 => GuardianBlock::Centre,
            14..18 => GuardianBlock::Quad,
            _ => GuardianBlock::Side,
        };
        let length = match block {
            // 27 volleys = 3 turns (HL ≈5.4 s; bB 4.8 s); the orbit stops with the last one.
            GuardianBlock::Spin => GUARDIAN_SPIN_FIRST_MS + 27 * 200,
            // 5 volleys (bB 4.7 and 4.5 s; fzg 3.8 s).
            GuardianBlock::Quad => GUARDIAN_SPLIT_MS + 4500,
            // 5 cycles of 1.0 s (5Z, HL, fzg).
            GuardianBlock::Centre => GUARDIAN_RUN_MS + 5000,
            // 13 stream volleys, then bombs up to ≈1 s after (5Z 117.1-117.8, 138.9-140.4).
            _ => GUARDIAN_RUN_MS + 13 * 200 + 800,
        };
        self.serial += 1;
        self.joint = Some(GuardianClock {
            block,
            serial: self.serial,
            start: now,
            end: now + length,
        });
        // Start to start 5-17 s, median ≈9.5 s; the next block waits for a short ring walk.
        self.next_joint = now + rng.gen_range(6000..=14_000).max(length + 1500);
    }
    /// The block this guardian is in now.
    pub fn clock(&mut self, id: i32, now: u64, rng: &mut impl rand::Rng) -> GuardianClock {
        self.update(now, rng);
        if let Some(joint) = self.joint {
            return joint;
        }
        let serial = &mut self.serial;
        let solo = self.solo.entry(id).or_insert_with(|| {
            *serial += 1;
            GuardianClock {
                block: GuardianBlock::Ring,
                serial: *serial,
                start: now,
                end: now + 2000,
            }
        });
        if now >= solo.end {
            // F lasted 1.4-3.9 s and appeared about once per ring-walk gap (9 of ≈28 comet blocks); 1 in 3 every 2-4 s is an estimate.
            let (block, length) = if solo.block == GuardianBlock::Ring && rng.gen_ratio(1, 3) {
                (GuardianBlock::Fan, rng.gen_range(2000..=4000))
            } else {
                (GuardianBlock::Ring, rng.gen_range(2000..=4000))
            };
            *serial += 1;
            *solo = GuardianClock {
                block,
                serial: *serial,
                start: now,
                end: now + length,
            };
        }
        *solo
    }
    pub fn current(&self, id: i32) -> Option<GuardianClock> {
        self.joint.or_else(|| self.solo.get(&id).copied())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardianMode {
    /// Grey stone right after the partner's death; heals.
    Stone { heal: bool },
    /// Fights with the shared blocks.
    Active,
    /// Still fighting, but walking back to the dais to throw.
    Returning,
    /// Grey at the dais from just before the throw until the sword is gone; `toss` on the release tick.
    Waiting { toss: bool },
}
/// The survivor's cycle once its peer dies (HLraOCBJO94, bBQRbAoRP_g, 5ZRsiTUcKts, wpi7Ff-mOD4, fzgFSFVN8nY, exMHglbJ98E).
#[derive(Default)]
pub struct GuardianBrain {
    alone: Option<u64>,
    heals: u8,
    next_heal: u64,
    next_throw: Option<u64>,
    grey: Option<u64>,
    tossed: bool,
    sword_seen: bool,
    sword_gone: Option<u64>,
    mode: Option<GuardianMode>,
}
// Haunted Armor regen: invulnerable and still for ≈4.0 s, +200 every ≈0.65 s (6 heals), entered at most once at ≤1350 HP
// (bBQRbAoRP_g 2012-04-01 t=168.0-172.4; threshold and "once per armor" from the Japanese wiki, whose 6 heals and 6-shot ring match the footage).
pub const ARMOR_REGEN_MS: u64 = 4000;
pub const ARMOR_REGEN_HP: i32 = 1350;
pub const ARMOR_HEAL_MS: u64 = 650;
// One undamaged armor woke with the player ≈1.3-2.4 tiles away (bBQRbAoRP_g t=150-166, at 60 px per tile); the Japanese wiki: "starts moving when a player approaches or attacks it".
pub const ARMOR_WAKE_RANGE: f32 = 2.;
/// Oryx Brute state: CHARGE one or two 2.5 s dashes, RING 1.3-6.6 s (bBQRbAoRP_g 2012-04-01: Rush 4.3, 2.5, ≥2.3, ≥2.2 s; gaps 2.2, 1.3, ≥7 s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BruteState {
    pub charge: bool,
    pub serial: u64,
    pub since: u64,
    pub until: u64,
}
impl BruteState {
    pub fn new(now: u64, rng: &mut impl rand::Rng) -> Self {
        Self {
            charge: false,
            serial: 0,
            since: now,
            until: now + rng.gen_range(1300..=6600),
        }
    }
    pub fn update(&mut self, now: u64, rng: &mut impl rand::Rng) -> bool {
        if now < self.until {
            return false;
        }
        self.charge = !self.charge;
        self.serial += 1;
        self.since = now;
        self.until = now
            + if self.charge {
                // One or two 2.5 s dashes (Rush windows 2.2-4.3 s; second dash 2.45 s after the first).
                2500 * rng.gen_range(1..=2)
            } else {
                rng.gen_range(1300..=6600)
            };
        true
    }
}
// Grey from ≈0.6 s after the death for ≈5.0 s; active again 5.3-5.8 s after the death (5ZRsiTUcKts, HLraOCBJO94, bBQRbAoRP_g).
pub const GUARDIAN_STONE_DELAY_MS: u64 = 600;
pub const GUARDIAN_STONE_MS: u64 = 5000;
// "+2400" steps ≈0.4 s apart from 0.5 s into the stone, the last one partial (HLraOCBJO94 t=684.4-685.6, 5ZRsiTUcKts t=159.8-162).
// The Left (red) heals +1600 per step (0k8IXj3k3yw 2012-05-06 t=335.5-340.5, ≈11-12 steps); "Red stone guardian heals less than blue one" (guide published 2012-04-24).
// Steps continue to full health or the end of the stone; the Japanese wiki's "× 8" is not a cap in the footage.
pub const GUARDIAN_HEAL_STEPS: u8 = 12;
pub fn guardian_heal(kind: u16) -> i32 {
    if kind == GUARDIANS[1] { 1600 } else { 2400 }
}
// The survivor returns with a red nova ≈1.9 s after its sword dies (Right, 5 instances, 1.7-2.1 s).
pub const GUARDIAN_RETURN_MS: u64 = 1900;
// Every sword was shot down 1.4-6.3 s after landing and no timeout was ever seen; this bound only keeps an unattended sword finite.
pub const SWORD_MS: u64 = 120_000;
impl GuardianBrain {
    pub fn alone(&mut self, now: u64) {
        if self.alone.is_none() {
            self.alone = Some(now);
            self.next_heal = now + GUARDIAN_STONE_DELAY_MS + 500;
        }
    }
    pub fn mode(&self) -> Option<GuardianMode> {
        self.mode
    }
    /// `sword`: its thrown sword still exists; `home`: it stands at its dais.
    pub fn update(
        &mut self,
        now: u64,
        sword: bool,
        home: bool,
        rng: &mut impl rand::Rng,
    ) -> Result<GuardianMode> {
        let mode = self.step(now, sword, home, rng)?;
        self.mode = Some(mode);
        Ok(mode)
    }
    fn step(
        &mut self,
        now: u64,
        sword: bool,
        home: bool,
        rng: &mut impl rand::Rng,
    ) -> Result<GuardianMode> {
        let Some(alone) = self.alone else {
            return Ok(GuardianMode::Active);
        };
        ensure!(now >= alone, "Guardian phase clock regressed");
        let stone = alone + GUARDIAN_STONE_DELAY_MS;
        if now < stone {
            return Ok(GuardianMode::Active);
        }
        if now < stone + GUARDIAN_STONE_MS {
            let heal = now >= self.next_heal && self.heals < GUARDIAN_HEAL_STEPS;
            if heal {
                self.heals += 1;
                self.next_heal = now + 400;
            }
            return Ok(GuardianMode::Stone { heal });
        }
        // The first throw left 1.4-2.0 s after the stone ended, from at or beside the dais (5 fights).
        let next = *self.next_throw.get_or_insert(now);
        if let Some(grey) = self.grey {
            // It greys 0.1-0.4 s before letting go.
            let toss = !self.tossed && now >= grey + 200;
            if toss {
                self.tossed = true;
            } else if self.tossed {
                self.sword_seen |= sword;
                // A blocked landing never produces a sword; treat it as gone once the lob would have landed.
                if !sword
                    && self.sword_gone.is_none()
                    && (self.sword_seen || now >= grey + 200 + 2000)
                {
                    self.sword_gone = Some(now);
                }
                if self
                    .sword_gone
                    .is_some_and(|gone| now >= gone + GUARDIAN_RETURN_MS)
                {
                    // Next throw after a return: 2.2-24.7 s and once none for 33 s (throw-to-throw 11-49 s, median ≈24).
                    // The Left uses the same timers: a second Left survivor (0k8IXj3k3yw) did not repeat exMHglbJ98E's 3-8 s throws.
                    self.next_throw = Some(now + rng.gen_range(2000..=30_000));
                    self.grey = None;
                    self.tossed = false;
                    self.sword_seen = false;
                    self.sword_gone = None;
                    return Ok(GuardianMode::Active);
                }
            }
            return Ok(GuardianMode::Waiting { toss });
        }
        if now < next {
            return Ok(GuardianMode::Active);
        }
        if home {
            self.grey = Some(now);
            return Ok(GuardianMode::Waiting { toss: false });
        }
        Ok(GuardianMode::Returning)
    }
}
pub fn attacks(kind: u16, hp: i32, max_hp: i32, heading: f32, elapsed: u64) -> Vec<(u8, f32)> {
    if let Some(schedule) = crate::encounter::attacks::endgame_schedule(kind, hp, max_hp, elapsed) {
        return schedule.snapshot(heading);
    }
    match kind {
        0x669 | 0x67e..=0x682 | 0xd84 | 0xde2 | 0xdf2 => vec![(0, heading)],
        // 18 Quiet shots before the 2017-06-20 note "Projectile Amount: 18 -> 15": https://www.realmeye.com/wiki/release-history-2017
        0xd87 => ring(0, 18, 0.),
        0x67d if elapsed % 10_000 < 5000 => fan(10, 3, 10., heading),
        0x67d => (0..4).map(|index| (index, heading)).collect(),
        0xd70 | 0xd71 | 0xd7a | 0xd7d | 0xde1 | 0xde3 | 0xde5 | 0xdf1 => vec![],
        // Guardians, their sword, Insect Minions and the Brute fire from stateful schedules (World::npc_attack_schedule).
        0xd78 | 0xd79 | 0xd7c | 0xd80 | 0xd82 => vec![],
        0xd7f => vec![(0, heading)],
        0xd81 => {
            if elapsed % 1000 < 500 {
                // Ring of 8 Swipes 45° apart (bBQRbAoRP_g 2012-04-01 t=51.6, 120.6); the cadence is reconstruction.
                ring(0, 8, heading)
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
    if kind == ORYX {
        for phase in oryx1::PHASES {
            for hp in [object.hp, oryx1::LOW_HP - 1] {
                for age in (0..45_000).step_by(100) {
                    let schedule =
                        crate::encounter::attacks::oryx1_schedule(phase, 1, age, hp, false);
                    for (index, angle) in schedule.snapshot(0.) {
                        ensure!(
                            angle.is_finite() && object.projectiles.contains_key(&index),
                            "Oryx {phase:?} emits absent selected projectile {index}"
                        );
                    }
                }
            }
        }
        return Ok(());
    }
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
        // Hall armor respawns in its slot (https://wikiwiki.jp/rmd/version/build123.4.4); no source limits its life, so u64::MAX never expires.
        0xd7d => &[Wave {
            kind: 0xd7e,
            count: 1,
            radius: 1.,
            lifetime: u64::MAX,
        }],
        // Counts are maxima; the world draws 8-15 every 3 s and 3-5 every 7 s from the 2012 wiki: https://web.archive.org/web/20120528162631id_/http://forums.wildshadow.com/wiki/realm-mad-god/oryxs-castle-and-chamber
        // About 11-12 minions stood around one Commander before contact, and new ones appear on its sprite (bBQRbAoRP_g 2012-04-01 t=24.2, 294-298).
        0xd7f => &[Wave {
            kind: 0xd80,
            count: 15,
            radius: 0.5,
            lifetime: 60_000,
        }],
        // New Eye Warriors appear on the Brute's own sprite (bBQRbAoRP_g 2012-04-01 t=47.9-48.3, 102.7).
        0xd82 => &[Wave {
            kind: 0xd81,
            count: 5,
            radius: 0.5,
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
            // Henchman spawns Bile (period JP wiki): https://web.archive.org/web/20261025114348/https://wikiwiki.jp/rmd/Wine%20Cellar
            Wave {
                kind: 0xdf1,
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
        // Each guardian gave every qualifier a DEF potion (9 of 9 clear 2012 kills). The 2012 Wild Shadow pages list for both: Wine Cellar Incantation,
        // Ancient Stone Sword, T7-T10 weapons and armor (wiki armor tiers are one lower than the client's), a T3 ability and a T3 ring.
        // No 2012 source gives rates. The Incantation is 1 in 9 observed kills; the equipment rates are estimates that give a qualifier at least one extra
        // piece about half the time, inside the 3-of-4 post-"Castle drop tweaks" (2012-04-22) footage range.
        0xd78 | 0xd79 => &[
            (1., Item(0xa20)),
            (0.1, Item(INCANTATION)),
            (0.003, Item(0xc05)),
            (0.1, Tier(7, WEAPONS)),
            (0.08, Tier(8, WEAPONS)),
            (0.05, Tier(9, WEAPONS)),
            (0.03, Tier(10, WEAPONS)),
            (0.1, Tier(8, ARMOR)),
            (0.08, Tier(9, ARMOR)),
            (0.05, Tier(10, ARMOR)),
            (0.03, Tier(11, ARMOR)),
            (0.08, Tier(3, ABILITIES)),
            (0.08, Tier(3, RINGS)),
        ],
        // Att, Def, T9-T11 weapons and armour, T4-T5 rings and abilities, no T12 or UT in three dated 2012 sources; rates are one player's
        // 100 logged Build 123 kills, 2012-04-23 .. 05-02, posted on the Wild Shadow forums.
        ORYX => &[
            (0.23, Item(0xa1f)),
            (0.2, Item(0xa20)),
            (0.02, Tier(9, WEAPONS)),
            (0.13, Tier(10, WEAPONS)),
            (0.04, Tier(11, WEAPONS)),
            (0.08, Tier(9, ARMOR)),
            (0.1, Tier(10, ARMOR)),
            (0.04, Tier(11, ARMOR)),
            (0.12, Tier(4, ABILITIES)),
            (0.03, Tier(5, ABILITIES)),
            (0.06, Tier(4, RINGS)),
            (0.07, Tier(5, RINGS)),
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
        // Compiled Kithio keeps these shared equipment pools separate from boss/chest loot: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.RedDemon.cs#L44-L85
        0x669 => &[(0.02, Item(0xa05)), (0.02, Item(0xa9a)), (0.02, Item(0xa1c))],
        0x66a => &[(0.03, Item(0xa1c))],
        0x66b => &[(0.03, Item(0xa17)), (0.02, Item(0xa0a))],
        // Insect Minions, Haunted Armor and the Guardian Sword dropped nothing in 2012 footage.
        0x67e..=0x682
        | 0xd7f
        | 0xd81..=0xd84
        | 0x944
        | 0xde0
        | 0xdf2 => &[(0.03, Item(0xa22)), (0.02, Item(0xa23))],
        0x67d
        | 0xd70
        | 0xd71
        | 0xd7a
        | 0xd7c
        | 0xd7d
        | 0xd7e
        | 0xd80
        | 0xd86
        | 0xd87
        | 0x1741..=0x1748
        | 0x174a..=0x174e
        | 0xde1..=0xde4
        // A 2012 player reports the barrels empty: https://web.archive.org/web/20130627235632/http://gaming.stackexchange.com/questions/92083/where-is-the-wine
        | 0xde5
        | 0xdf0
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
