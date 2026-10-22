use super::*;

#[test]
fn community_schema16_migrates_canonical15_without_recreating_committed_feature_tables() {
    let content = Content::load(Path::new("data")).unwrap();
    for missing in ["bot_minds", "trade_transactions"] {
        let dir = std::env::temp_dir().join(format!("community-schema-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("game.db");
        drop(Store::open(&path, &content).unwrap());
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            16
        );
        db.execute_batch(
            "DROP TABLE bot_minds; DROP TABLE trade_transactions; PRAGMA user_version=15;",
        )
        .unwrap();
        drop(db);
        drop(Store::open(&path, &content).unwrap());
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            16
        );
        for table in ["bot_minds", "trade_transactions"] {
            assert!(
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap()
            );
        }
        db.execute_batch(&format!("DROP TABLE {missing}")).unwrap();
        assert!(
            Store::open(&path, &content)
                .err()
                .unwrap()
                .to_string()
                .contains("schema-16")
        );
    }
}

#[test]
fn community_ambiguous_experimental13_and14_require_explicit_preserved_state_migration() {
    let content = Content::load(Path::new("data")).unwrap();
    for version in [13, 14] {
        let dir = std::env::temp_dir().join(format!(
            "community-ambiguous-{version}-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("game.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(&format!(
            "PRAGMA user_version={version}; CREATE TABLE bot_minds(account INTEGER PRIMARY KEY);"
        ))
        .unwrap();
        let before: String = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='bot_minds'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let error = Store::open(&path, &content).err().unwrap();
        assert!(error.to_string().contains("request an explicit migration"));
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            version
        );
        assert_eq!(
            db.query_row(
                "SELECT sql FROM sqlite_schema WHERE name='bot_minds'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            before
        );
    }
}
