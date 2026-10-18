use super::*;

pub(super) fn check_schema(db: &Connection, version: i32) -> Result<()> {
    let columns = {
        let mut q = db.prepare("SELECT name FROM pragma_table_xinfo('deaths') ORDER BY cid")?;
        q.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let expected: &[&str] = if version >= 14 {
        &[
            "character",
            "snapshot",
            "killed_by",
            "died_at",
            "equipment_fame",
        ]
    } else {
        &["character", "snapshot", "killed_by", "died_at"]
    };
    ensure!(
        (version == 0 && columns.is_empty())
            || columns
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied()),
        "death award schema/version/column disagreement; restore reviewed state"
    );
    if version >= 14 {
        let reference: bool = db.query_row("SELECT count(*)=1 AND count(*) FILTER(WHERE \"table\"='characters' AND \"from\"='character' AND \"to\"='id' AND seq=0 AND on_update='NO ACTION' AND on_delete='NO ACTION')=1 FROM pragma_foreign_key_list('deaths')",[],|r|r.get(0))?;
        ensure!(reference, "unsupported death character foreign key");
        for (column, kind, not_null, default, key) in [
            ("character", "INTEGER", false, None, 1),
            ("snapshot", "TEXT", true, None, 0),
            ("killed_by", "TEXT", true, None, 0),
            ("died_at", "INTEGER", true, Some("unixepoch()"), 0),
            ("equipment_fame", "INTEGER", true, None, 0),
        ] {
            let metadata: (String, bool, Option<String>, i32, i32) = db.query_row(
                "SELECT type,\"notnull\",dflt_value,pk,hidden FROM pragma_table_xinfo('deaths') WHERE name=?1", [column],
                |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            ensure!(
                metadata == (kind.into(), not_null, default.map(str::to_owned), key, 0),
                "unsupported canonical death metadata: {column}"
            );
        }
    }
    Ok(())
}

pub(super) fn migrate(db: &Connection, version: i32, legacy_base: [i32; 8]) -> Result<()> {
    if version < 14 {
        // Old deaths were paid base-only; their original snapshots and wallets must not change.
        db.execute_batch("ALTER TABLE deaths RENAME TO legacy_deaths;
CREATE TABLE deaths(character INTEGER PRIMARY KEY REFERENCES characters(id),snapshot TEXT NOT NULL,killed_by TEXT NOT NULL,died_at INTEGER NOT NULL DEFAULT(unixepoch()),equipment_fame INTEGER NOT NULL CHECK(typeof(equipment_fame)='integer' AND equipment_fame BETWEEN 0 AND 2147483647));
INSERT INTO deaths(character,snapshot,killed_by,died_at,equipment_fame) SELECT character,snapshot,killed_by,died_at,0 FROM legacy_deaths;
DROP TABLE legacy_deaths;
CREATE INDEX deaths_by_time ON deaths(died_at);")?;
    }
    check_schema(db, 14)?;
    let mut q = db.prepare("SELECT d.character,c.account,c.alive,d.snapshot,d.equipment_fame FROM deaths d JOIN characters c ON c.id=d.character")?;
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, i32>(0)?,
            r.get::<_, i32>(1)?,
            r.get::<_, bool>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i32>(4)?,
        ))
    })? {
        let (id, account, alive, raw, equipment) = row?;
        let ch = decode_character(&raw, legacy_base)?;
        ensure!(
            ch.id == id
                && ch.account == account
                && !alive
                && !ch.alive
                && ch.hp == 0
                && ch.items.len() == 12,
            "invalid archived death ownership or snapshot {id}"
        );
        total(&ch, equipment)?;
    }
    let orphan: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM deaths d LEFT JOIN characters c ON c.id=d.character WHERE c.id IS NULL)",[],|r|r.get(0))?;
    ensure!(!orphan, "orphaned death award");
    Ok(())
}

pub(super) fn validate_wallets(db: &Connection) -> Result<()> {
    let broken: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM account_fame f LEFT JOIN (SELECT c.account,sum(json_extract(d.snapshot,'$.fame')+d.equipment_fame) AS paid FROM deaths d JOIN characters c ON c.id=d.character GROUP BY c.account) d ON d.account=f.account WHERE f.earned<>coalesce(d.paid,0) OR f.balance<0 OR f.balance>f.earned OR f.earned>2147483647)",[],|r|r.get(0))?;
    ensure!(
        !broken,
        "archived death awards disagree with account fame wallets; restore reviewed state"
    );
    Ok(())
}

pub(super) fn equipment(ch: &Character, content: &Content) -> Result<i32> {
    ensure!(
        ch.items.len() == 12 && ch.fame >= 0,
        "invalid death equipment or base fame"
    );
    ch.effective(content)?;
    let mut percent = 0_u64;
    for kind in &ch.items[..4] {
        if *kind == -1 {
            continue;
        }
        let item = content.get(u16::try_from(*kind)?)?;
        ensure!(
            item.class == "Equipment" && !item.resurrects,
            "invalid permanent-death equipment {}",
            item.name
        );
        percent = percent
            .checked_add(u64::from(item.fame_bonus))
            .context("equipment percent overflow")?;
    }
    // The May2012 calculator applies the combined percent once to the running death subtotal.
    let bonus = u64::try_from(ch.fame)?
        .checked_mul(percent)
        .context("equipment fame overflow")?
        / 100;
    let bonus = i32::try_from(bonus).context("equipment fame exceeds wire range")?;
    total(ch, bonus)?;
    Ok(bonus)
}

pub(super) fn total(ch: &Character, equipment: i32) -> Result<i32> {
    ensure!(
        ch.fame >= 0 && equipment >= 0,
        "invalid archived death fame"
    );
    ch.fame
        .checked_add(equipment)
        .context("total death fame overflow")
}

#[cfg(test)]
mod tests;
