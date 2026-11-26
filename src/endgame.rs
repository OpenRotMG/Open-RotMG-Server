use crate::content::Content;
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

// The January 2012 FAQ specifies a two-minute lockout, not later forks' 22 seconds: https://web.archive.org/web/20120106105406id_/http://forums.wildshadow.com/wiki/realm-mad-god/faq
pub const CLOSE_DELAY_MS: u64 = 120_000;
pub const EARTHQUAKE_MS: u64 = 8_000;
pub const REPLACEMENT_DELAY_MS: u64 = 2_000;
pub const CASTLE: &str = "oryx-castle";
#[cfg(test)]
pub const CHAMBER: &str = "oryx-chamber";
pub const GUARDIANS: [u16; 2] = [0xd78, 0xd79];
pub const ORYX: u16 = 0x1740;
pub const ORYX2: u16 = 0x932;
pub const LOCKED_CELLAR: u16 = 0x721;
pub const INCANTATION: u16 = 0x722;
pub const HEROES: [u16; 2] = [0x668, 0x67d];
/// Realm-cycle credit for a committed defeat (base 79a4c92b src/world.rs:6283-6293). A quest line ends when its
/// last line carrier leaves no successor inheriting the quest (base realm_quests.rs:418 transforms kept it).
pub fn critical_credit(
    hero: bool,
    id: i32,
    quest: Option<i32>,
    successor: bool,
    carrier: bool,
) -> Option<i32> {
    if hero {
        Some(id)
    } else if carrier && !successor {
        quest
    } else {
        None
    }
}
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
