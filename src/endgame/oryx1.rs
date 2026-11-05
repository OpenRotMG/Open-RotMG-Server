//! Oryx the Mad God 1 as he fought in Builds 123.3-123.5.1 (Jun-Sep 2012). Build 123 (2012-04-22) replaced the fixed cycle with
//! "New randomized set of attacks in Oryx's Chamber"; five whole 2012-06-28 .. 09-13 fights give the phase pools, lengths, rage and
//! taunts below (M3Of4Q7Ag7Q + ZdjeB9t-n5w 06-28, fzgFSFVN8nY 08-10, exMHglbJ98E 08-15,
//! tg7tAkfKGUE 09-12, w5koziG9P1M 09-13).
use rand::{Rng, seq::SliceRandom};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Awake but alone, before the wake burst.
    Alone,
    /// The wake burst: Minions of Oryx, then a minion-end line.
    Opening,
    /// "I control your fate!": White Planets and Moons.
    Planets,
    /// "BE SILENT!!!"
    Silence,
    /// "My artifacts will protect me!"
    Artifacts,
    /// "All who look upon my face shall die."
    Gaze,
    /// Assassins and Minions of Oryx.
    Minions,
    /// "Time for more dancing! Hahahaha!!"
    Dance,
    /// "Tremble before my cosmic might!!!": Black Planets and Moons.
    Cosmic,
    /// The chase grid.
    Grid,
    Rage,
}
use Phase::*;
pub const PHASES: [Phase; 11] = [
    Alone, Opening, Planets, Silence, Artifacts, Gaze, Minions, Dance, Cosmic, Grid, Rage,
];

/// Every fight's first HP line reads 81.5-82.3k, so the wake burst fires once he has lost ≈2.6k (≥ 82345 observed at the switch).
pub const WAKE_HP: i32 = 82_400;
/// Silence and Artifacts never run after the first Dance/Cosmic/Grid, which never run before it; the switch falls at ≈47-50k (LOW,
/// interpolated between HP lines).
pub const LOW_HP: i32 = 48_000;
/// HP lines at 5237 and 5397 came without rage; rage struck at 3305 (exMHglbJ98E HP overlay 5311 → 3305). The Japanese wiki and the
/// early-2013 old-wiki guide both say 5000.
pub const RAGE_HP: i32 = 5_000;
/// Stacked "+2500" floaters; the overlay read 3305 → 20805, seven steps (the Japanese wiki says eight), over ≈2.6-3.6 s of red flashing.
pub const HEAL: i32 = 2_500;
const HEAL_STEPS: u64 = 7;
const HEAL_FIRST_MS: u64 = 400;
const HEAL_STEP_MS: u64 = 450;
pub const RAGE_HEAL_MS: u64 = HEAL_FIRST_MS + HEAL_STEPS * HEAL_STEP_MS;
/// ≈1.5-1.6× (exMHglbJ98E t=311 / 318) and 1.46× (ZdjeB9t-n5w 2012-06-28 sprite widths 1.83 → 2.67 tiles); he grows while he heals.
pub const RAGE_SIZE: i32 = 150;
pub const GROWTH: i32 = 8;
/// Net chase speed ≈1.8-2 tiles/s in the chase grid and in rage (ZdjeB9t-n5w t=350.5-357.5, 425.5-430.5; LOW, floor registration).
pub const SPEED: f32 = 2.;
/// Minions move toward the nearest player and drift ≈1-2 tiles/s (LOW); Assassins keep up with players (old-wiki text, no measurement).
pub const MINION_SPEED: f32 = 2.;
pub const ASSASSIN_SPEED: f32 = 6.;
/// Opening: ≈5 Minions at once on Oryx; later minion phases ≈9 Minions and Assassins at once (split unknown; the Japanese wiki says 4 + 4).
/// No re-summon inside a minion phase (tg7tAkfKGUE, exMHglbJ98E).
pub const OPENING_MINIONS: usize = 5;
pub const PHASE_MINIONS: usize = 5;
pub const PHASE_ASSASSINS: usize = 4;
/// Rage: ≥ 2 Assassins and 2-3 Minions within 2 s of the taunt (tg7tAkfKGUE t=579-580); "a ton of minions behind him" (forum). The top-up
/// cadence and caps are estimates.
pub const RAGE_ASSASSINS: usize = 3;
pub const RAGE_MINIONS: usize = 5;
pub const RAGE_SUMMON_MS: u64 = 6_000;
/// Three red bombs every ≈1.3 s, radius ≈3 (10 nova spokes; the client draws 4 + 2 × radius); damage per the Japanese wiki (texts 200-250).
pub const BOMB_MS: u64 = 1_300;
pub const BOMB_RADIUS: f32 = 3.;
pub const BOMB_DAMAGE: i32 = 220;
/// Ring Elements stand 8.8 tiles out (median of 337 detections, tg7tAkfKGUE t=166-175.5; 8.84 in M3Of4Q7Ag7Q).
pub const RING_RADIUS: f32 = 8.8;
/// Every summon is tossed from Oryx (yellow trails, the client's fixed 1.5 s throw) and lands at its slot: Ring Elements ≈1.9 s after
/// "BE SILENT!!!" (M3Of4Q7Ag7Q 2012-06-28 t=445.1 → 447.0), the artifacts ≈6.9 s after their taunt (468.0 → 474.9), White Planets by the
/// first pulse ≈2.8 s in, Black Planets ≈8 s in (ZdjeB9t-n5w t=222-224), the grid ≈3.5 s in (t=347.0 → 350.5).
pub const TOSS_MS: u64 = 1_500;
fn toss_at(phase: Phase) -> Option<u64> {
    match phase {
        Silence | Dance => Some(0),
        Planets => Some(1_300),
        Artifacts => Some(5_300),
        Cosmic => Some(6_500),
        Grid => Some(GRID_MS - TOSS_MS),
        _ => None,
    }
}
/// Oryx stands while the grid lands, then chases.
pub const GRID_MS: u64 = 3_500;
/// How long a tossed summon lasts after landing: to its phase's end, except Ring Elements ≈19.5 s (t=447.0-466.4) and the artifacts ≈33 s.
pub fn lifetime(kind: u16) -> u64 {
    match kind {
        0x1743 => 26_000 - 2_800,
        0x1745 | 0x1746 => 30_000 - 8_000,
        0x1744 => 29_000 - GRID_MS,
        0x1747 | 0x1748 => 19_500,
        0x174a..=0x174e => 32_000,
        _ => 20_000,
    }
}
/// Wake → minion-end line 14.0-14.8 s, then 6.0 s to the first phase taunt (three fights).
const OPENING_MS: u64 = 14_500;
const GAP_MS: u64 = 6_000;
/// The Gaze lead-in comes 4-6 s before "All who look…"; after a minion-end line Oryx goes straight to "All who look…".
const LEAD_IN_MS: u64 = 5_000;
/// Planets: the second taunt comes 12-14 s in.
const PLANETS_MID_MS: u64 = 13_000;
/// Taunt to taunt, so each includes the transition: Planets 24-28, Silence 22-24, Artifacts 38-40, Gaze ≈10-14 after "All who look…",
/// Dance 24-30, Cosmic 29-40, Grid 26-32, Minions 12-18 to the end line then 6.
fn length(phase: Phase) -> u64 {
    match phase {
        Opening => OPENING_MS + GAP_MS,
        Planets => 26_000,
        Silence => 23_000,
        Artifacts => 39_000,
        Gaze => 12_000,
        Minions => 15_000 + GAP_MS,
        Dance => 25_000,
        Cosmic => 30_000,
        Grid => 29_000,
        Alone | Rage => u64::MAX,
    }
}
const HIGH: [Phase; 5] = [Planets, Silence, Artifacts, Gaze, Minions];
const LOW: [Phase; 6] = [Planets, Gaze, Minions, Dance, Cosmic, Grid];
const ORDER: [Phase; 8] = [
    Planets, Silence, Artifacts, Gaze, Minions, Dance, Cosmic, Grid,
];
/// Transitions counted in the five fights (row = from Opening then ORDER, column = to ORDER). No phase ever followed itself (0 of 110).
/// The next phase is drawn with weight count + 1 over the HP pool, so unseen pairs stay possible (an estimate; weights unmeasured).
const SEEN: [[u8; 8]; 9] = [
    [3, 2, 0, 0, 0, 0, 0, 0],
    [0, 4, 2, 2, 2, 2, 2, 4],
    [1, 0, 3, 3, 0, 0, 0, 1],
    [0, 2, 0, 4, 0, 0, 0, 0],
    [1, 0, 0, 0, 9, 4, 0, 3],
    [8, 0, 1, 4, 0, 2, 2, 1],
    [4, 0, 0, 2, 2, 0, 2, 1],
    [0, 0, 0, 2, 2, 1, 0, 1],
    [1, 0, 0, 2, 4, 2, 2, 0],
];
/// Phase taunts were missing for 2 of 8 Cosmic starts, 1 of 7 Artifacts starts and 3 of 25 Minions HP lines.
const TAUNT_CHANCE: f64 = 0.9;

const WAKE: &str = "Go forth minions! Crush these weaklings!!!";
const HP_LINES: [&str; 5] = [
    "Puny mortals! My {} hitpoints are more than enough!",
    "Insignificant peons! I have {} hitpoints!",
    "Fools! I still have {} hitpoints!",
    "You cosmic peasants! My {} hitpoints make me mightier than you!",
    "I have {} hitpoints and I shall destroy you!",
];
const MINIONS_END: [&str; 5] = [
    "I will finish you off myself!",
    "Bah! I need no minions to destroy you!!",
    "These minions will only get in my way!!!",
    "Leave me my minions... I wish to crush them PERSONALLY!",
    "Fools! I will deal with you PERSONALLY!",
];
const PLANETS_START: [&str; 3] = [
    "I am the master of this existence!!!",
    "I control your fate!",
    "The universe bends to my will alone!!!",
];
const PLANETS_MID: [&str; 4] = [
    "You have spoken your last!",
    "Speak no more, fools!",
    "SILENCE!",
    "You have uttered your last pathetic whimper!",
];
const GAZE_LEAD: &str = "Insufferable fools! If I see you, I will kill you!";
const GAZE: &str = "All who look upon my face shall die.";
const GRID: [&str; 4] = [
    "You WILL fear me!!!",
    "Yes, run for your worthless lives!!!",
    "You shall never see the light of day!",
    "FOOL! Flee while you can!!!",
];
const RAGE: [&str; 3] = [
    "I HAVE HAD ENOUGH OF YOU!!!",
    "THIS SHALL BE YOUR GRAVE!!!",
    "ENOUGH!!!",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Say(String),
    /// A new phase began: spawn its summons and restart its attack clock.
    Begin(Phase),
    /// Throw the phase's summons; they land TOSS_MS later.
    Toss(Phase),
    /// The minion-end line: the leftover Minions vanish.
    Dismiss,
    /// Oryx turns solid green for ≈3-4 s at phase changes and minion-end lines (w5koziG9P1M t=162.4-165.4); meaning unproven.
    Green,
    /// Red flashing while he heals at the rage.
    Red,
    Heal(i32),
    /// The heal is done: grow, armour up and send the rage summons.
    Unleash,
}

#[derive(Clone, Debug)]
pub struct Brain {
    phase: Phase,
    previous: Option<Phase>,
    serial: u64,
    start: u64,
    lead_in: bool,
    said: u8,
    tossed: bool,
    healed: u64,
}
impl Brain {
    pub fn new(now: u64) -> Self {
        Self {
            phase: Alone,
            previous: None,
            serial: 0,
            start: now,
            lead_in: false,
            said: 0,
            tossed: false,
            healed: 0,
        }
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    /// Distinct per phase instance, for the attack clock.
    pub fn serial(&self) -> u64 {
        self.serial
    }
    /// Milliseconds into the phase's own attacks: the Gaze lead-in is not part of them.
    pub fn age(&self, now: u64) -> u64 {
        let age = now.saturating_sub(self.start);
        if self.phase == Gaze && self.lead_in {
            age.saturating_sub(LEAD_IN_MS)
        } else {
            age
        }
    }
    pub fn lead_in(&self, now: u64) -> bool {
        self.phase == Gaze && self.lead_in && now < self.start + LEAD_IN_MS
    }
    pub fn healing(&self) -> bool {
        self.phase == Rage && self.healed < HEAL_STEPS
    }
    /// The Japanese wiki has him invulnerable while he heals; otherwise footage shows damage numbers in every phase it could check.
    pub fn invulnerable(&self) -> bool {
        self.healing()
    }
    pub fn update(&mut self, hp: i32, now: u64, rng: &mut impl Rng) -> Vec<Event> {
        let mut events = vec![];
        if self.phase != Rage && hp <= RAGE_HP {
            // Rage cuts into whatever phase runs (29 s into Cosmic, 4 s after "All who look…"); no phase or taunt follows it.
            self.enter(Rage, now);
            events.push(Event::Say(RAGE.choose(rng).unwrap().to_string()));
            events.push(Event::Begin(Rage));
            events.push(Event::Red);
            return events;
        }
        let age = now.saturating_sub(self.start);
        match self.phase {
            Rage => {
                while self.healed < HEAL_STEPS && age >= HEAL_FIRST_MS + self.healed * HEAL_STEP_MS
                {
                    self.healed += 1;
                    events.push(Event::Heal(HEAL));
                    if self.healed == HEAL_STEPS {
                        events.push(Event::Unleash);
                    }
                }
                return events;
            }
            Alone => {
                if hp < WAKE_HP {
                    self.enter(Opening, now);
                    events.push(Event::Say(WAKE.into()));
                    events.push(Event::Say(hp_line(hp, rng)));
                    events.push(Event::Begin(Opening));
                }
                return events;
            }
            Opening | Minions if self.said == 0 && age >= length(self.phase) - GAP_MS => {
                self.said = 1;
                events.push(Event::Say(MINIONS_END.choose(rng).unwrap().to_string()));
                events.push(Event::Dismiss);
                events.push(Event::Green);
            }
            Planets if self.said == 0 && age >= PLANETS_MID_MS => {
                self.said = 1;
                events.push(Event::Say(PLANETS_MID.choose(rng).unwrap().to_string()));
            }
            Gaze if self.lead_in && self.said == 0 && age >= LEAD_IN_MS => {
                self.said = 1;
                events.push(Event::Say(GAZE.into()));
            }
            _ => {}
        }
        if !self.tossed && toss_at(self.phase).is_some_and(|at| age >= at) {
            self.tossed = true;
            events.push(Event::Toss(self.phase));
        }
        let total = length(self.phase)
            + if self.phase == Gaze && self.lead_in {
                LEAD_IN_MS
            } else {
                0
            };
        if age < total {
            return events;
        }
        let next = self.pick(hp, rng);
        // A Gaze straight after a minion-end line skips the lead-in (fzgFSFVN8nY t=692.3, tg7tAkfKGUE t=423).
        let lead_in = next == Gaze && !matches!(self.phase, Opening | Minions);
        self.enter(next, now);
        self.lead_in = lead_in;
        let taunt = rng.gen_bool(TAUNT_CHANCE);
        match next {
            Planets if taunt => {
                events.push(Event::Say(PLANETS_START.choose(rng).unwrap().to_string()))
            }
            Silence if taunt => events.push(Event::Say("BE SILENT!!!".into())),
            Artifacts if taunt => events.push(Event::Say("My artifacts will protect me!".into())),
            Gaze if lead_in => events.push(Event::Say(GAZE_LEAD.into())),
            Gaze => events.push(Event::Say(GAZE.into())),
            Minions if taunt => events.push(Event::Say(hp_line(hp, rng))),
            Dance if taunt => events.push(Event::Say("Time for more dancing! Hahahaha!!".into())),
            Cosmic if taunt => events.push(Event::Say("Tremble before my cosmic might!!!".into())),
            Grid if taunt => events.push(Event::Say(GRID.choose(rng).unwrap().to_string())),
            _ => {}
        }
        events.push(Event::Begin(next));
        events.push(Event::Green);
        events
    }
    fn enter(&mut self, phase: Phase, now: u64) {
        self.previous = Some(self.phase);
        self.phase = phase;
        self.serial += 1;
        self.start = now;
        self.lead_in = false;
        self.said = 0;
        self.tossed = false;
        self.healed = 0;
    }
    fn pick(&self, hp: i32, rng: &mut impl Rng) -> Phase {
        let pool: &[Phase] = if hp >= LOW_HP { &HIGH } else { &LOW };
        let row = match self.phase {
            Opening => 0,
            from => {
                1 + ORDER
                    .iter()
                    .position(|p| *p == from)
                    .expect("phase outside the random set")
            }
        };
        let choices: Vec<(Phase, u32)> = pool
            .iter()
            .filter(|p| **p != self.phase)
            .map(|p| {
                (
                    *p,
                    u32::from(SEEN[row][ORDER.iter().position(|q| q == p).unwrap()]) + 1,
                )
            })
            .collect();
        choices
            .choose_weighted(rng, |c| c.1)
            .expect("empty Oryx phase pool")
            .0
    }
}
fn hp_line(hp: i32, rng: &mut impl Rng) -> String {
    HP_LINES.choose(rng).unwrap().replace("{}", &hp.to_string())
}

/// Every element Oryx places is invulnerable (Japanese wiki element pages, 2012-06-26); Minions and Assassins are not.
pub fn shielded(kind: u16) -> bool {
    matches!(kind, 0x1742..=0x1748 | 0x174a..=0x174e)
}
/// (radius, tiles/s along the circle, direction) for summons that circle Oryx; direction -1 is counter-clockwise on screen.
pub fn orbit(kind: u16) -> Option<(f32, f32, f32)> {
    match kind {
        // Anti-Spectators sweep counter-clockwise at 19-20 tiles/s on an arc of ≈6.8 tiles (ZdjeB9t-n5w t=101.0-105.9; radius LOW).
        0x1747 => Some((6.8, 19.5, -1.)),
        // The four near artifacts at 1.9 tiles, counter-clockwise ≈220°/s (Zdje t=7.64-9.04 at 30 fps); the far ones ≈8 tiles, ≈110°/s.
        0x174a..=0x174d => Some((1.9, 7.3, -1.)),
        0x174e => Some((8., 15.4, -1.)),
        // Four White Planets 90° apart, counter-clockwise at ≈25-31°/s ≈ 4 tiles/s, radius ≈8.5 (tg7tAkfKGUE t=133-136, 204-210 ×1.78).
        0x1743 => Some((8.5, 4., -1.)),
        // Black Planets ≈9.6 tiles out (ZdjeB9t-n5w t=232.9); "circle him at low speed" (2012 wiki). Speed and direction unmeasured.
        0x1746 => Some((9.6, 1., -1.)),
        // Black Moons 6.2-7.7 tiles out, between two planets (t=226-235). Speed unmeasured.
        0x1745 => Some((7., 2., -1.)),
        _ => None,
    }
}
/// The White Planets come in to ≈2.8 tiles in the second half of the spiral and go back out ≈4 s later, then in again (≈9.5 s and ≈19.5 s
/// into the phase; tg7tAkfKGUE t=134.8-135.6, 203.6-210.3 ×1.78; the second cycle is seen once).
pub fn orbit_radius(kind: u16, age: u64) -> Option<f32> {
    let (radius, ..) = orbit(kind)?;
    // `age` runs from landing, ≈2.8 s into the phase.
    Some(
        if kind == 0x1743 && ((6_700..10_500).contains(&age) || (16_700..20_500).contains(&age)) {
            2.8
        } else {
            radius
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn oryx_runs_random_pools_and_rages_at_5000() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut brain = Brain::new(0);
        let (mut now, mut hp, mut seen) = (0, 85_000, vec![]);
        assert!(brain.update(hp, now, &mut rng).is_empty());
        hp = 82_000;
        let wake = brain.update(hp, now, &mut rng);
        assert_eq!(wake[0], Event::Say(WAKE.into()));
        assert!(matches!(&wake[1], Event::Say(s) if s.contains("82000")));
        while seen.last().is_none_or(|s: &(Phase, i32)| s.0 != Rage) {
            now += 100;
            hp -= 7;
            for event in brain.update(hp, now, &mut rng) {
                if let Event::Begin(p) = event {
                    seen.push((p, hp));
                }
            }
        }
        assert!(brain.invulnerable());
        for pair in seen.windows(2) {
            assert_ne!(pair[0].0, pair[1].0, "a phase followed itself");
        }
        for (phase, hp) in &seen {
            match phase {
                Silence | Artifacts => assert!(*hp >= LOW_HP),
                Dance | Cosmic | Grid => assert!(*hp < LOW_HP),
                _ => {}
            }
        }
        assert_eq!(seen.last().unwrap().0, Rage);
        assert!(seen.iter().filter(|s| s.0 == Rage).count() == 1);
        // Seven +2500 steps, then the unleash.
        let rage = now;
        let mut heal = 0;
        let mut unleashed = false;
        while now < rage + 5000 {
            now += 100;
            for event in brain.update(hp, now, &mut rng) {
                match event {
                    Event::Heal(n) => heal += n,
                    Event::Unleash => unleashed = true,
                    Event::Begin(_) => panic!("a phase followed the rage"),
                    _ => {}
                }
            }
        }
        assert_eq!(heal, 17_500);
        assert!(unleashed && !brain.invulnerable());
    }
}
