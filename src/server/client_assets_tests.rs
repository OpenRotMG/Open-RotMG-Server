use super::*;

#[tokio::test]
async fn named_editor_assets_remain_raw_and_do_not_replace_dynamic_gameplay_or_expose_source() {
    let (dir, mut app) = account_actions::tests::fixture();
    let client = dir.join("client");
    std::fs::create_dir_all(client.join("vendor")).unwrap();
    for (name, bytes) in [
        ("index.html", b"game entry".as_slice()),
        ("draw.html", b"editor entry"),
        ("launcher.js", b"guarded launcher"),
        (
            "native-texture-editor.swf",
            b"raw editor, never key-rewritten",
        ),
        ("client.swf", b"source gameplay, not the served key rewrite"),
        ("vendor/ruffle.js", b"pinned runtime"),
        ("secret.txt", b"must not be served"),
    ] {
        std::fs::write(client.join(name), bytes).unwrap();
    }
    app.swf = Arc::new(b"dynamic key-rewritten gameplay".to_vec());
    let router = client_assets(&client)
        .route("/client.swf", get(swf))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    for (path, expected) in [
        ("/", b"game entry".as_slice()),
        ("/index.html", b"game entry"),
        ("/draw.html", b"editor entry"),
        ("/launcher.js", b"guarded launcher"),
        (
            "/native-texture-editor.swf",
            b"raw editor, never key-rewritten",
        ),
        ("/client.swf", b"dynamic key-rewritten gameplay"),
        ("/vendor/ruffle.js", b"pinned runtime"),
    ] {
        let response = http.get(format!("{url}{path}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let mime = response.headers()["content-type"].to_str().unwrap();
        if path.ends_with(".swf") {
            assert!(
                [
                    "application/x-shockwave-flash",
                    "application/vnd.adobe.flash.movie"
                ]
                .contains(&mime)
            );
        } else if path.ends_with(".js") {
            assert!(["text/javascript", "application/javascript"].contains(&mime));
        } else {
            assert!(mime.starts_with("text/html"));
        }
        assert_eq!(response.bytes().await.unwrap(), expected, "{path}");
    }
    for path in [
        "/secret.txt",
        "/editor/lock.json",
        "/scripts/run.py",
        "/native-texture-editor.swf/secret.txt",
        "/../Cargo.toml",
    ] {
        assert_eq!(
            http.get(format!("{url}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    task.abort();
}

#[test]
fn deployed_proxy_routes_only_named_editor_assets_not_source_or_draw_prefixes() {
    let config = include_str!("../../scripts/deploy/Caddyfile");
    let patterns: Vec<_> = config
        .lines()
        .find(|line| line.trim().starts_with("@backend path "))
        .unwrap()
        .split_whitespace()
        .skip(2)
        .collect();
    let matches = |path: &str| {
        patterns
            .iter()
            .any(|pattern| match pattern.strip_suffix('*') {
                Some(prefix) => path.starts_with(prefix),
                None => path == *pattern,
            })
    };
    for path in [
        "/launcher.js",
        "/draw.html",
        "/native-texture-editor.swf",
        "/client.swf",
        "/game",
        "/legacy-api/picture/save",
        "/vendor/ruffle.js",
        "/sfx/error.mp3",
    ] {
        assert!(matches(path), "{path}");
    }
    for path in [
        "/draw/",
        "/draw-other",
        "/native-texture-editor.swf.bak",
        "/launcher.js/private",
        "/editor/lock.json",
        "/scripts/run.py",
        "/dev/testing",
        "/internal/discord/link",
    ] {
        assert!(!matches(path), "{path}");
    }
}
