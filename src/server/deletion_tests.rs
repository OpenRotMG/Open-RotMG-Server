use super::*;
use rusqlite::Connection;

async fn delete(url: &str, guid: &str, password: &str, id: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!("{url}/legacy-api/char/delete"))
        .form(&[
            ("guid", guid),
            ("password", password),
            ("charId", id),
            ("reason", "1"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/xml");
    assert_eq!(response.headers()["cache-control"], "no-store");
    response.text().await.unwrap()
}

#[tokio::test]
async fn character_deletion_http_is_owned_once_only_and_durable_without_death_rewards() {
    let (dir, app) = account_actions::tests::fixture();
    let account = app
        .store
        .register(
            "deletion-fixture".into(),
            "delete@example.invalid".into(),
            "fixture-password".into(),
        )
        .await
        .unwrap();
    let foreign = app.store.list("deletion-foreign".into()).await.unwrap();
    let other = app
        .store
        .create(foreign.id, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut dead = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    dead.alive = false;
    dead.hp = 0;
    dead.fame = 150;
    app.store
        .death(dead.clone(), "prepared deletion fixture".into())
        .await
        .unwrap();
    let death = app.store.fame(account, dead.id).await.unwrap();
    let mut character = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    character.level = 5;
    character.fame = 20;
    app.store.progress(character.clone()).await.unwrap();
    let before = app.store.presentation(account).await.unwrap();
    let vault = serde_json::to_value(app.store.vault(account, 0).await.unwrap()).unwrap();
    let id = character.id.to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/legacy-api/char/delete", post(delete_character))
        .with_state(app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    for (guid, password, target) in [
        ("delete@example.invalid", "wrong-password", id.as_str()),
        ("deletion-fixture", "", id.as_str()),
        ("deletion-foreign", "", id.as_str()),
        ("delete@example.invalid", "fixture-password", "0"),
        ("delete@example.invalid", "fixture-password", "2147483648"),
        ("delete@example.invalid", "fixture-password", "1.5"),
    ] {
        assert!(
            delete(&url, guid, password, target)
                .await
                .starts_with("<Error>")
        );
        assert_eq!(
            app.store.load(account, character.id).await.unwrap().items,
            character.items
        );
    }
    let active = app.claim_session(account).unwrap();
    assert!(
        delete(&url, "delete@example.invalid", "fixture-password", &id)
            .await
            .contains("active session")
    );
    assert!(app.store.load(account, character.id).await.is_ok());
    drop(active);
    // Kithio hard-deletes without Fame awards; retained rows preserve our lifetime unlocks: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/char/delete.cs
    let (first, second) = tokio::join!(
        delete(&url, "delete@example.invalid", "fixture-password", &id),
        delete(&url, "delete@example.invalid", "fixture-password", &id),
    );
    assert_eq!(
        [&first, &second]
            .iter()
            .filter(|xml| xml.as_str() == "<Success/>")
            .count(),
        1
    );
    assert!(
        [&first, &second]
            .iter()
            .any(|xml| xml.starts_with("<Error>"))
    );
    for target in [&id, &dead.id.to_string(), &other.id.to_string()] {
        assert!(
            delete(&url, "delete@example.invalid", "fixture-password", target)
                .await
                .starts_with("<Error>")
        );
    }
    assert!(app.store.load(account, character.id).await.is_err());
    assert!(app.store.progress(character.clone()).await.is_err());
    let mut stale = character.clone();
    stale.alive = false;
    stale.hp = 0;
    assert!(app.store.death(stale, "late death".into()).await.is_err());
    server.abort();
    let _ = server.await;
    let content = app.content.clone();
    drop(app);
    let store = Store::open(&dir.join("game.db"), &content).unwrap();
    assert_eq!(store.presentation(account).await.unwrap(), before);
    assert_eq!(store.fame(account, dead.id).await.unwrap(), death);
    assert!(store.fame(account, character.id).await.is_err());
    assert_eq!(
        serde_json::to_value(store.vault(account, 0).await.unwrap()).unwrap(),
        vault
    );
    assert!(store.load(foreign.id, other.id).await.is_ok());
    let listed = store
        .characters("delete@example.invalid".into(), "fixture-password".into())
        .await
        .unwrap();
    assert!(listed.characters.is_empty());
    assert_eq!(listed.best_levels[&WIZARD], 5);
    let db = Connection::open(dir.join("game.db")).unwrap();
    let retired: (bool, String, i32) = db
        .query_row(
            "SELECT alive,items,revision FROM characters WHERE id=?1",
            [character.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        retired,
        (
            false,
            serde_json::to_string(&vec![-1; 12]).unwrap(),
            character.revision + 1
        )
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    let replacement = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert!(replacement.id > character.id);
    assert!(store.load(account, replacement.id).await.is_ok());
    assert!(store.load(account, character.id).await.is_err());
    drop(db);
    drop(store);
}
