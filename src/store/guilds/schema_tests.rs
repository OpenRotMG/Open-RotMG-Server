use super::*;

#[test]
fn guild_name_migration_preserves_roster_constraints_and_deleted_id_sequence() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "PRAGMA foreign_keys=ON;
CREATE TABLE accounts(id INTEGER PRIMARY KEY);
INSERT INTO accounts VALUES(1),(2),(3),(4);
",
    )
    .unwrap();
    migrate(&db, 0).unwrap();
    // Reconstruct the deployed schema before exercising its migration.
    db.execute_batch("DROP TABLE guilds;").unwrap();
    db.execute_batch(&GUILDS_SQL.replace("COLLATE NOCASE", "COLLATE NOCASE CHECK(length(name) BETWEEN 1 AND 20 AND name=trim(name) AND name NOT GLOB '*[^A-Za-z ]*')")).unwrap();
    db.execute_batch("INSERT INTO guilds(id,name,level,fame,earned,board) VALUES(2,'fifty',3,10,100,'Board & history'),(99,'Deleted',1,0,0,'');
DELETE FROM guilds WHERE id=99;
INSERT INTO guild_members VALUES(1,2,40,40),(2,2,30,30),(3,2,20,20),(4,2,0,10);
").unwrap();
    assert!(db.execute("UPDATE guilds SET name='50'", []).is_err());
    let roster = |db: &Connection| -> String {
        db.query_row("SELECT json_group_array(json_array(account,guild,rank,fame)) FROM guild_members ORDER BY account", [], |r| r.get(0)).unwrap()
    };
    let before = roster(&db);
    {
        let tx = db.transaction().unwrap();
        migrate_names(&tx, 23).unwrap();
        migrate(&tx, 24).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(roster(&db), before);
    db.execute("UPDATE guilds SET name='50' WHERE id=2", [])
        .unwrap();
    let guild: (i32, String, i32, i32, i32, String) = db
        .query_row("SELECT * FROM guilds WHERE id=2", [], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap();
    assert_eq!(
        guild,
        (2, "50".into(), 3, 10, 100, "Board & history".into())
    );
    for sql in [
        "INSERT INTO guilds(name) VALUES('50')",
        "INSERT INTO guilds(name,level) VALUES('Bad',5)",
        "UPDATE guild_members SET guild=999 WHERE account=1",
        "UPDATE guild_members SET rank=40 WHERE account=2",
    ] {
        assert!(db.execute(sql, []).is_err(), "{sql}");
    }
    db.execute("INSERT INTO guilds(name) VALUES('Next')", [])
        .unwrap();
    assert_eq!(db.last_insert_rowid(), 100);
    assert_eq!(
        db.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
    migrate_names(&db, 24).unwrap();
    assert_eq!(roster(&db), before);
}
