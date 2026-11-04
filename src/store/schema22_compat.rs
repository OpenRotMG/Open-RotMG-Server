use super::*;

#[cfg(test)]
mod tests;

// Recovery must retain the exact known newer history, not recreate or downgrade it.
pub(super) const GOLD: &str = "CREATE TABLE gold_rewards(account INTEGER NOT NULL REFERENCES accounts(id),reward TEXT NOT NULL CHECK(reward IN ('discord_link','collection_top_half','first_bug_report','class_star_blue','class_star_red','class_star_orange','class_star_yellow','class_star_white','achievement_not_even_close','achievement_nexus_after_death','achievement_friends','achievement_potential','achievement_trader','achievement_hit_by_truck','achievement_realm_closer','achievement_baby_max','achievement_merchant','achievement_dungeon_master','achievement_max','achievement_no')),awarded_at INTEGER NOT NULL CHECK(awarded_at>0),notified_at INTEGER CHECK(notified_at>0),PRIMARY KEY(account,reward));";
pub(super) const GOLD23: &str = "CREATE TABLE gold_rewards(account INTEGER NOT NULL REFERENCES accounts(id),reward TEXT NOT NULL CHECK(reward IN ('discord_link','collection_top_half','first_bug_report','class_star_blue','class_star_red','class_star_orange','class_star_yellow','class_star_white','achievement_not_even_close','achievement_nexus_after_death','achievement_friends','achievement_potential','achievement_trader','achievement_hit_by_truck','achievement_realm_closer','achievement_baby_max','achievement_merchant','achievement_dungeon_master','achievement_max','achievement_no','achievement_ah_7uck')),awarded_at INTEGER NOT NULL CHECK(awarded_at>0),notified_at INTEGER CHECK(notified_at>0),PRIMARY KEY(account,reward));";
pub(super) const TABLES: [(&str, &str); 3] = [
    (
        "achievement_partners",
        "CREATE TABLE achievement_partners(account INTEGER NOT NULL REFERENCES accounts(id),peer INTEGER NOT NULL REFERENCES accounts(id),PRIMARY KEY(account,peer),CHECK(account<>peer))",
    ),
    (
        "achievement_dungeons",
        "CREATE TABLE achievement_dungeons(account INTEGER NOT NULL REFERENCES accounts(id),dungeon TEXT NOT NULL CHECK(dungeon IN ('pirate-cave','snake-pit','sprite-world','undead-lair','abyss-of-demons','ocean-trench','spider-den','mad-lab','forbidden-jungle','manor-of-the-immortals','beachzone','davy-jones-locker','tomb-of-the-ancients')),PRIMARY KEY(account,dungeon))",
    ),
    (
        "achievement_realm_credits",
        "CREATE TABLE achievement_realm_credits(account INTEGER NOT NULL REFERENCES accounts(id),realm TEXT NOT NULL CHECK(length(realm)=32),root INTEGER NOT NULL CHECK(root>0),PRIMARY KEY(account,realm,root))",
    ),
];

pub(super) fn check(db: &Connection, version: i32) -> Result<()> {
    for (name, expected) in TABLES {
        let sql: Option<String> = db
            .query_row("SELECT sql FROM sqlite_schema WHERE name=?1", [name], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(
            if matches!(version, 22 | 23) {
                sql.as_deref() == Some(expected)
            } else {
                sql.is_none()
            },
            "schema-{version} incompatible achievement metadata {name}; preserve state for review"
        );
    }
    Ok(())
}
