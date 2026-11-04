use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Achievement {
    NotEvenClose,
    NexusAfterDeath,
    Friends,
    Potential,
    Trader,
    HitByTruck,
    RealmCloser,
    BabyMax,
    Merchant,
    DungeonMaster,
    Max,
    No,
}
use Achievement::*;

#[derive(Serialize)]
pub(crate) struct Definition {
    pub id: Achievement,
    pub name: &'static str,
    pub difficulty: &'static str,
    pub gold: i32,
    pub criterion: &'static str,
}
// These are new project achievements, not reconstructed historical rewards: https://github.com/OpenRotMG/Open-RotMG-Server/issues/1376
pub(crate) const CATALOG: [Definition; 12] = [
    Definition {
        id: NotEvenClose,
        name: "Not even close",
        difficulty: "Easy",
        gold: 10,
        criterion: "Escape to Nexus at level 20 with less than 30 HP after losing more than 200 HP in the last second.",
    },
    Definition {
        id: NexusAfterDeath,
        name: "Nexus after death",
        difficulty: "Easy",
        gold: 10,
        criterion: "Try to escape to Nexus after permanent death.",
    },
    Definition {
        id: Friends,
        name: "Friends? Friends!!!",
        difficulty: "Easy",
        gold: 10,
        criterion: "Successfully invite someone to a guild or join a guild.",
    },
    Definition {
        id: Potential,
        name: "So much potential..",
        difficulty: "Easy",
        gold: 10,
        criterion: "Die permanently between levels 17 and 19.",
    },
    Definition {
        id: Trader,
        name: "Trader",
        difficulty: "Easy",
        gold: 10,
        criterion: "Complete a trade.",
    },
    Definition {
        id: HitByTruck,
        name: "Hit by a truck",
        difficulty: "Medium",
        gold: 25,
        criterion: "Take at least 500 damage after defenses and protection within 0.25 seconds, including overkill.",
    },
    Definition {
        id: RealmCloser,
        name: "Realm Closer",
        difficulty: "Medium",
        gold: 25,
        criterion: "Defeat 15 distinct qualifying closure enemies in one Realm instance.",
    },
    Definition {
        id: BabyMax,
        name: "Baby Max",
        difficulty: "Medium",
        gold: 25,
        criterion: "Max at least 6 of 8 permanent stats on one character.",
    },
    Definition {
        id: Merchant,
        name: "Merchant",
        difficulty: "Medium",
        gold: 25,
        criterion: "Complete trades with 10 different human accounts.",
    },
    Definition {
        id: DungeonMaster,
        name: "Dungeon Master",
        difficulty: "Medium",
        gold: 25,
        criterion: "Complete each of the 13 ordinary dungeon types at least once across your account's characters.",
    },
    Definition {
        id: Max,
        name: "Max",
        difficulty: "Hard",
        gold: 100,
        criterion: "Max all 8 permanent stats on one character.",
    },
    Definition {
        id: No,
        name: "No!",
        difficulty: "Hard",
        gold: 100,
        criterion: "Be in Oryx's Castle, Oryx's Chamber or Wine Cellar when an intentional server update stops the server.",
    },
];
impl Achievement {
    pub fn reward(self) -> &'static str {
        match self {
            NotEvenClose => "achievement_not_even_close",
            NexusAfterDeath => "achievement_nexus_after_death",
            Friends => "achievement_friends",
            Potential => "achievement_potential",
            Trader => "achievement_trader",
            HitByTruck => "achievement_hit_by_truck",
            RealmCloser => "achievement_realm_closer",
            BabyMax => "achievement_baby_max",
            Merchant => "achievement_merchant",
            DungeonMaster => "achievement_dungeon_master",
            Max => "achievement_max",
            No => "achievement_no",
        }
    }
}
