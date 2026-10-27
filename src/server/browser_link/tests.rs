use super::*;

async fn value(response: Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap()
}
fn headers(a: &App, cookie: Option<&str>) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("origin", a.origin.parse().unwrap());
    if let Some(cookie) = cookie {
        h.insert("cookie", cookie.parse().unwrap());
    }
    h
}
async fn browser_tab(a: &App, old: Option<&str>) -> (String, String) {
    let tab = random();
    let response = super::super::chars(
        State(a.clone()),
        ConnectInfo("127.0.0.1:1".parse().unwrap()),
        headers(a, old),
        Query(HashMap::from([("browser".into(), tab.clone())])),
        Form(HashMap::from([
            (
                "guid".into(),
                format!("browser-{:016x}", rand::random::<u64>()),
            ),
            ("password".into(), String::new()),
        ])),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = if let Some(cookie) = old {
        cookie.to_owned()
    } else {
        let header = response.headers()["set-cookie"].to_str().unwrap();
        assert!(
            header.contains("HttpOnly")
                && header.contains("SameSite=Lax")
                && header.contains("Path=/")
        );
        header.split(';').next().unwrap().to_owned()
    };
    (cookie, tab)
}
async fn native_login(a: &App, cookie: &str, tab: &str, email: &str, password: &str) -> Response {
    super::super::chars(
        State(a.clone()),
        ConnectInfo("127.0.0.1:1".parse().unwrap()),
        headers(a, Some(cookie)),
        Query(HashMap::from([("browser".into(), tab.into())])),
        Form(HashMap::from([
            ("guid".into(), email.into()),
            ("password".into(), password.into()),
        ])),
    )
    .await
}
fn configure(a: &mut App, provider: &str) {
    let mut oauth = OAuth::new("12345678901234567".into(), "fixture-secret".into()).unwrap();
    oauth.authorize = format!("{provider}/authorize");
    oauth.exchange = format!("{provider}/token");
    oauth.user = format!("{provider}/user");
    a.browser_links = Arc::new(Service {
        oauth: Some(oauth),
        ..Service::default()
    });
}
fn private_headers(a: &App) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        "authorization",
        format!("Bearer {}", a.discord_link_token.as_ref().unwrap())
            .parse()
            .unwrap(),
    );
    h
}

async fn grant(a: &App, discord: &str) -> String {
    let h = private_headers(a);
    let v = value(
        issue(
            State(a.clone()),
            ConnectInfo("127.0.0.1:1".parse().unwrap()),
            h,
            Json(DiscordRequest {
                discord_id: discord.into(),
            }),
        )
        .await,
    )
    .await;
    v["url"].as_str().unwrap().split('#').nth(1).unwrap().into()
}
async fn begin(a: &App, cookie: &str, token: &str) -> serde_json::Value {
    value(
        start(
            State(a.clone()),
            headers(a, Some(cookie)),
            Json(StartRequest {
                token: token.into(),
            }),
        )
        .await,
    )
    .await
}
async fn authorization_response(a: &App, cookie: &str, auth: &serde_json::Value) -> Response {
    let url = reqwest::Url::parse(auth["url"].as_str().unwrap()).unwrap();
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();
    callback(
        State(a.clone()),
        headers(a, Some(cookie)),
        Query(Callback {
            state: Some(state),
            code: Some("fixture-code".into()),
            iss: Some("https://discord.com".into()),
            error: None,
        }),
    )
    .await
}
async fn finish(a: &App, cookie: &str, auth: &serde_json::Value) -> serde_json::Value {
    let response = authorization_response(a, cookie, auth).await;
    let mut location =
        reqwest::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
    let fragment = location.fragment().unwrap().to_owned();
    let Some(receipt) = fragment.strip_prefix("result=") else {
        location.set_query(Some(&fragment));
        let error = location
            .query_pairs()
            .find(|(key, _)| key == "error")
            .unwrap()
            .1
            .into_owned();
        return serde_json::json!({"error":error});
    };
    value(
        complete(
            State(a.clone()),
            headers(a, Some(cookie)),
            Json(StartRequest {
                token: receipt.into(),
            }),
        )
        .await,
    )
    .await
}
async fn provider(discord: &str) -> (String, tokio::task::JoinHandle<()>) {
    let discord = discord.to_string();
    let router=Router::new().route("/token",post(|Form(form):Form<HashMap<String,String>>|async move {
        assert_eq!(form["client_secret"],"fixture-secret");assert_eq!(form["code"],"fixture-code");
        Json(serde_json::json!({"access_token":"fixture-access","token_type":"Bearer","scope":"identify"}))
    })).route("/user",get(move|headers:HeaderMap|{let id=discord.clone();async move{assert_eq!(headers["authorization"],"Bearer fixture-access");Json(serde_json::json!({"id":id}))}}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (origin, task)
}

#[tokio::test]
async fn forwarded_link_cannot_give_the_issuers_discord_recovery_of_a_victims_game() {
    let (dir, mut a) = super::super::account_actions::tests::fixture();
    let (origin, provider) = provider("12345678901234567").await;
    configure(&mut a, &origin);
    a.store
        .register(
            "victim-browser".into(),
            "victim@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let token = grant(&a, "12345678901234568").await;
    let (cookie, tab) = browser_tab(&a, None).await;
    native_login(&a, &cookie, &tab, "victim@example.test", "test-password").await;
    let auth = begin(&a, &cookie, &token).await;
    assert!(finish(&a, &cookie, &auth).await["error"].is_string());
    assert!(
        a.store
            .linked_account("12345678901234568".into())
            .await
            .is_err()
    );
    assert!(
        a.store
            .issue_reset("12345678901234568".into(), None)
            .await
            .is_err()
    );
    provider.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn logout_reset_cookie_state_and_challenge_rotation_invalidate_approval() {
    let (dir, mut a) = super::super::account_actions::tests::fixture();
    let (origin, provider) = provider("12345678901234567").await;
    configure(&mut a, &origin);
    let account = a
        .store
        .register(
            "race-browser".into(),
            "race@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let token = grant(&a, "12345678901234567").await;
    let (cookie, tab) = browser_tab(&a, None).await;
    native_login(&a, &cookie, &tab, "race@example.test", "test-password").await;
    let auth = begin(&a, &cookie, &token).await;
    let (other_cookie, _) = browser_tab(&a, None).await;
    assert!(finish(&a, &other_cookie, &auth).await["error"].is_string());
    native_login(&a, &cookie, &tab, "logout-guest", "").await;
    assert!(finish(&a, &cookie, &auth).await["error"].is_string());
    native_login(&a, &cookie, &tab, "race@example.test", "test-password").await;
    let auth = begin(&a, &cookie, &token).await;
    let replacement = grant(&a, "12345678901234567").await;
    assert!(finish(&a, &cookie, &auth).await["error"].is_string());
    let auth = begin(&a, &cookie, &replacement).await;
    let verifier = a
        .store
        .prepare_password("new-password".into())
        .await
        .unwrap();
    a.store.change_password(account, verifier).await.unwrap();
    assert!(finish(&a, &cookie, &auth).await["error"].is_string());
    assert!(
        a.store
            .linked_account("12345678901234567".into())
            .await
            .is_err()
    );
    provider.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
