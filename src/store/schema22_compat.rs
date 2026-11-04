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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KnownStoreLayout {
    Legacy,
    Achievement22,
    Portable22,
    Achievement23,
    Combined24,
}
impl KnownStoreLayout {
    pub(super) fn achievements(self) -> bool {
        matches!(
            self,
            Self::Achievement22 | Self::Achievement23 | Self::Combined24
        )
    }
    pub(super) fn portable(self) -> bool {
        matches!(self, Self::Portable22 | Self::Combined24)
    }
    fn check_objects(self, db: &Connection, version: i32) -> Result<()> {
        let mut canonical = super::character_slots::TRIGGERS
            .iter()
            .map(|sql| (12, *sql))
            .chain(std::iter::once((9, super::guilds::CAPACITY_TRIGGER)))
            .filter(|(minimum, _)| version >= *minimum)
            .map(|(_, sql)| {
                (
                    sql.split_whitespace().nth(2).unwrap(),
                    sql.trim_end_matches(';'),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        // Rebuilding Gold must not drop or rewrite SQL dependencies outside the known layouts.
        let mut objects=db.prepare("SELECT type,name,sql FROM sqlite_schema WHERE type IN ('view','trigger') OR (type='index' AND tbl_name='gold_rewards') ORDER BY name")?;
        let objects = objects.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        for object in objects {
            let (kind, name, sql) = object?;
            let supported =
                kind == "index" && name == "sqlite_autoindex_gold_rewards_1" && sql.is_none()
                    || kind == "trigger"
                        && canonical
                            .remove(name.as_str())
                            .is_some_and(|expected| Some(expected) == sql.as_deref());
            ensure!(
                supported,
                "unsupported {kind} {name} in {self:?} schema-{version}; preserve input for review"
            );
        }
        ensure!(
            canonical.is_empty(),
            "missing canonical trigger {:?} in {self:?} schema-{version}; preserve input for review",
            canonical.keys().next()
        );
        let child:Option<String>=db.query_row("SELECT s.name FROM sqlite_schema s JOIN pragma_foreign_key_list(s.name) fk WHERE s.type='table' AND fk.[table] COLLATE NOCASE='gold_rewards' LIMIT 1",[],|r|r.get(0)).optional()?;
        ensure!(
            child.is_none(),
            "unsupported foreign key to Gold from table {child:?} in {self:?} schema-{version}; preserve input for review"
        );
        Ok(())
    }
    pub(super) fn read(db: &Connection, version: i32) -> Result<Self> {
        fn group(
            db: &Connection,
            definitions: impl IntoIterator<Item = (&'static str, &'static str)>,
        ) -> Result<bool> {
            let mut count = 0;
            let mut total = 0;
            for (name, expected) in definitions {
                total += 1;
                let sql: Option<String> = db
                    .query_row("SELECT sql FROM sqlite_schema WHERE name=?1", [name], |r| {
                        r.get(0)
                    })
                    .optional()?;
                if let Some(sql) = sql {
                    ensure!(
                        sql == expected,
                        "incompatible store metadata {name}; preserve input for review"
                    );
                    count += 1;
                }
            }
            ensure!(
                count == 0 || count == total,
                "partial store feature metadata; preserve input for review"
            );
            Ok(count == total)
        }
        let achievements = group(db, TABLES)?;
        let portable = group(
            db,
            super::account_save::SCHEMA
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    (
                        s.split_whitespace()
                            .nth(2)
                            .unwrap()
                            .split('(')
                            .next()
                            .unwrap(),
                        s,
                    )
                }),
        )?;
        let layout = match (version, achievements, portable) {
            (0 | 5..=21, false, false) => Self::Legacy,
            (22, true, false) => Self::Achievement22,
            (22, false, true) => Self::Portable22,
            (23, true, false) => Self::Achievement23,
            (24, true, true) => Self::Combined24,
            _ => anyhow::bail!(
                "unsupported schema-{version} feature layout; preserve input for review"
            ),
        };
        if version >= 18 {
            let sql: String = db.query_row(
                "SELECT sql FROM sqlite_schema WHERE name='gold_rewards'",
                [],
                |r| r.get(0),
            )?;
            let expected = match layout {
                Self::Achievement22 => GOLD,
                Self::Achievement23 | Self::Combined24 => GOLD23,
                _ => super::gold::SCHEMA18,
            };
            ensure!(
                sql == expected.trim_end_matches(';'),
                "schema-{version} gold reward metadata mismatch; restore the reviewed ledger"
            );
        }
        layout.check_objects(db, version)?;
        if portable {
            super::account_save::check_schema(db)?;
        }
        Ok(layout)
    }
}
pub(super) fn check(db: &Connection, version: i32) -> Result<()> {
    KnownStoreLayout::read(db, version).map(|_| ())
}
