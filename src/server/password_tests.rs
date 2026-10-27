use super::*;
use rusqlite::{Connection, types::Value};

const EMAIL: &str = "password-test@example.invalid";
const OLD: &str = "old-fixture-password";
const NEW: &str = "new-fixture-password";
const DISCORD: &str = "100000000000000001";

async fn serve(app: &App) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/legacy-api/account/{operation}", post(account_request))
        .route("/legacy-api/char/list", post(chars))
        .route("/api/account/recover", post(reset_password))
        .with_state(app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (url, server)
}
async fn change(client: &reqwest::Client, url: &str, fields: &[(&str, &str)]) -> String {
    let response = client
        .post(format!("{url}/legacy-api/account/changePassword"))
        .form(fields)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/xml");
    assert_eq!(response.headers()["cache-control"], "no-store");
    response.text().await.unwrap()
}
fn form<'a>(old: &'a str, new: &'a str) -> [(&'a str, &'a str); 3] {
    [("guid", EMAIL), ("password", old), ("newPassword", new)]
}
fn snapshot(db: &Connection) -> Vec<(String, Vec<Vec<Value>>)> {
    let tables = db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap()
        .query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    tables
        .into_iter()
        .filter(|t| !["password_resets", "recovery_codes", "discord_codes"].contains(&t.as_str()))
        .map(|table| {
            let columns = if table == "credentials" {
                "account,email"
            } else {
                "*"
            };
            let mut query = db
                .prepare(&format!("SELECT {columns} FROM {table} ORDER BY rowid"))
                .unwrap();
            let count = query.column_count();
            let rows = query
                .query_map([], |r| {
                    (0..count)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<Value>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            (table, rows)
        })
        .collect()
}
fn pending(db: &Connection, account: i32) -> i32 {
    ["password_resets", "recovery_codes", "discord_codes"]
        .iter()
        .map(|table| {
            db.query_row(
                &format!("SELECT count(*) FROM {table} WHERE account=?1"),
                [account],
                |r| r.get::<_, i32>(0),
            )
            .unwrap()
        })
        .sum()
}
async fn recovery(app: &App, account: i32) -> String {
    let code = app.store.issue_discord_code(account).await.unwrap();
    app.store
        .redeem_discord_code(code, DISCORD.into())
        .await
        .unwrap();
    let consent = app.store.issue_recovery_code(account).await.unwrap();
    app.store
        .issue_reset(DISCORD.into(), Some(consent))
        .await
        .unwrap()
}

#[tokio::test]
async fn native_password_change_is_durable_atomic_and_preserves_account_state() {
    let (dir, app) = account_actions::tests::fixture();
    let account = app
        .store
        .register("password-fixture".into(), EMAIL.into(), OLD.into())
        .await
        .unwrap();
    app.store
        .choose_name(account, "Keeper".into())
        .await
        .unwrap();
    let foreign = app
        .store
        .register(
            "foreign-fixture".into(),
            "foreign@example.invalid".into(),
            OLD.into(),
        )
        .await
        .unwrap();
    let mut funding = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    funding.fame = 5000;
    funding.hp = 0;
    funding.alive = false;
    app.store
        .death(funding, "prepared password fixture funding".into())
        .await
        .unwrap();
    let mut character = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    character.exp = 7;
    app.store.progress(character.clone()).await.unwrap();
    app.store.vault(account, 0).await.unwrap();
    app.store.compete(account).await.unwrap();
    let token = recovery(&app, account).await;
    app.store.issue_recovery_code(account).await.unwrap();
    let foreign_code = app.store.issue_discord_code(foreign).await.unwrap();
    let db = Connection::open(dir.join("game.db")).unwrap();
    db.execute("UPDATE accounts SET credits=1200 WHERE id=?1", [account])
        .unwrap();
    db.execute(
        "UPDATE vaults SET items='[2820,-1,-1,-1,-1,-1,-1,-1]' WHERE account=?1",
        [account],
    )
    .unwrap();
    app.store
        .change_guild(account, crate::store::GuildAction::Create("Keepers".into()))
        .await
        .unwrap();
    app.store
        .purchase_character_slot(account, -42)
        .await
        .unwrap();
    // A stale pending link must be revoked even when a persistent binding already exists.
    db.execute("INSERT INTO discord_codes(account,hash,issued_at,expires_at) VALUES(?1,'stale-fixture-hash',0,2147483647)", [account]).unwrap();
    let before = snapshot(&db);
    let before_xml = app
        .store
        .characters(EMAIL.into(), OLD.into())
        .await
        .unwrap()
        .xml("", 0.0);
    let (url, server) = serve(&app).await;
    let client = reqwest::Client::new();
    assert_eq!(pending(&db, account), 3);
    let mut fields = form(OLD, NEW).to_vec();
    let target = foreign.to_string();
    fields.extend([("ignore", "-2147483648"), ("accountId", target.as_str())]);
    assert_eq!(
        change(&client, &url, &fields).await,
        "<Success/>",
        "native current-password change must commit"
    );
    assert!(
        snapshot(&db) == before,
        "password change touched unrelated account/save state"
    );
    assert_eq!(pending(&db, account), 0);
    assert_eq!(pending(&db, foreign), 1);
    assert!(app.store.reset_account(token).await.is_err());
    assert!(
        app.store
            .authenticate(EMAIL.into(), OLD.into())
            .await
            .is_err()
    );
    assert_eq!(
        app.store
            .authenticate(EMAIL.into(), NEW.into())
            .await
            .unwrap(),
        account
    );
    assert_eq!(
        app.store
            .authenticate("foreign@example.invalid".into(), OLD.into())
            .await
            .unwrap(),
        foreign
    );
    assert!(
        app.store
            .redeem_discord_code(foreign_code, "100000000000000002".into())
            .await
            .is_ok()
    );
    assert!(change(&client, &url, &fields).await.starts_with("<Error>"));
    let restarted = Store::open(&dir.join("game.db"), &app.content).unwrap();
    assert!(
        restarted
            .authenticate(EMAIL.into(), OLD.into())
            .await
            .is_err()
    );
    assert!(
        restarted
            .characters(EMAIL.into(), NEW.into())
            .await
            .unwrap()
            .xml("", 0.0)
            == before_xml
    );
    for (password, expected) in [(OLD, "<Error>"), (NEW, "<Chars")] {
        let response = client
            .post(format!("{url}/legacy-api/char/list"))
            .form(&[("guid", EMAIL), ("password", password)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().await.unwrap().starts_with(expected));
    }
    db.execute("INSERT INTO discord_codes(account,hash,issued_at,expires_at) VALUES(?1,'same-password-fixture-hash',0,2147483647)", [account]).unwrap();
    assert_eq!(change(&client, &url, &form(NEW, NEW)).await, "<Success/>");
    assert_eq!(pending(&db, account), 0);
    // Fail after the credential UPDATE, proving revocation failure rolls that update back too.
    db.execute("INSERT INTO discord_codes(account,hash,issued_at,expires_at) VALUES(?1,'rollback-fixture-hash',0,2147483647)", [account]).unwrap();
    db.execute_batch("CREATE TRIGGER reject_password_revocation BEFORE DELETE ON discord_codes BEGIN SELECT RAISE(ABORT,'fixture revocation failure'); END;").unwrap();
    let before_failure = snapshot(&db);
    let verifier: String = db
        .query_row(
            "SELECT password_hash FROM credentials WHERE account=?1",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        change(&client, &url, &form(NEW, "never-committed-password"))
            .await
            .contains("fixture revocation failure")
    );
    assert_eq!(pending(&db, account), 1);
    assert!(
        db.query_row(
            "SELECT password_hash FROM credentials WHERE account=?1",
            [account],
            |r| r.get::<_, String>(0)
        )
        .unwrap()
            == verifier
    );
    assert_eq!(
        app.store
            .authenticate(EMAIL.into(), NEW.into())
            .await
            .unwrap(),
        account
    );
    assert!(
        app.store
            .authenticate(EMAIL.into(), "never-committed-password".into())
            .await
            .is_err()
    );
    assert!(
        snapshot(&db) == before_failure,
        "failed change touched unrelated durable state"
    );
    assert_eq!(app.owners.lock().unwrap().len(), 1);
    assert!(app.owners.lock().unwrap().contains_key(&account));
    assert!(
        app.claim(account).is_err(),
        "SQL uncertainty must retain its account claim"
    );
    db.execute_batch("DROP TRIGGER reject_password_revocation")
        .unwrap();
    assert!(
        app.claim(account).is_err(),
        "repairing the SQL trigger must not clear the fence"
    );
    server.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
