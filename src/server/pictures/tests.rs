use super::*;
use crate::store::pictures::test_png;

async fn fixture() -> (PathBuf, App, String, tokio::task::JoinHandle<()>) {
    let (dir, a) = account_actions::tests::fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/legacy-api/picture/save",
            post(save).layer(DefaultBodyLimit::max(24 * 1024)),
        )
        .route(
            "/legacy-api/picture/get",
            post(get_picture).layer(DefaultBodyLimit::max(2048)),
        )
        .route(
            "/legacy-api/picture/{operation}",
            post(request).layer(DefaultBodyLimit::max(2048)),
        )
        .with_state(a.clone());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (dir, a, url, task)
}

fn multipart(guid: &str, password: &str, png: Vec<u8>) -> reqwest::multipart::Form {
    reqwest::multipart::Form::new()
        .text("guid", guid.to_owned())
        .text("password", password.to_owned())
        .text("secret", "")
        .text("name", "Native Art")
        .text("dataType", "2")
        .text("tags", "elf, wizard")
        .text("overwrite", "on")
        .part(
            "data",
            reqwest::multipart::Part::bytes(png)
                .file_name("Foo.png")
                .mime_str("image/png")
                .unwrap(),
        )
}

#[tokio::test]
async fn native_multipart_save_and_binary_reload_use_existing_authentication() {
    let (_dir, a, url, task) = fixture().await;
    a.store
        .register(
            "converted".into(),
            "owner@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let png = test_png(8, 8);
    for (guid, password) in [
        ("unknown-guest", ""),
        ("converted", ""),
        ("owner@example.org", ""),
        ("owner@example.org", "wrong-password"),
    ] {
        let response = client
            .post(format!("{url}/legacy-api/picture/save"))
            .multipart(multipart(guid, password, png.clone()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.text().await.unwrap().contains("<Success>"));
    }
    let response = client
        .post(format!("{url}/legacy-api/picture/save"))
        .multipart(multipart(
            "OWNER@EXAMPLE.ORG",
            "valid-password",
            png.clone(),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let text = response.text().await.unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let id = doc
        .descendants()
        .find(|n| n.has_tag_name("Id"))
        .unwrap()
        .text()
        .unwrap();
    let response = client
        .post(format!("{url}/legacy-api/picture/get"))
        .form(&[("id", id)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.bytes().await.unwrap(), png);
    // Native art must not provision an unauthenticated guest as a side effect.
    assert!(
        a.store
            .authenticate("unknown-guest".into(), "".into())
            .await
            .is_err()
    );
    let response = client
        .post(format!("{url}/legacy-api/picture/save"))
        .header("origin", "https://evil.invalid")
        .multipart(multipart(
            "owner@example.org",
            "valid-password",
            png.clone(),
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    for data in [vec![0; 32], vec![0; 24 * 1024 + 1]] {
        let response = client
            .post(format!("{url}/legacy-api/picture/save"))
            .multipart(multipart("owner@example.org", "valid-password", data))
            .send()
            .await
            .unwrap();
        assert!(!response.status().is_success());
    }
    let guest = a.store.list("known-guest".into()).await.unwrap();
    let response = client
        .post(format!("{url}/legacy-api/picture/save"))
        .multipart(multipart("known-guest", "", png.clone()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let db = rusqlite::Connection::open(_dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM pictures WHERE account=?1",
            [guest.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    task.abort();
}

#[test]
fn picture_read_budget_has_exact_expiry_ip_isolation_and_bounded_registry() {
    let now = std::time::Instant::now();
    let a = "192.0.2.1".parse().unwrap();
    let b = "192.0.2.2".parse().unwrap();
    let mut requests = HashMap::new();
    for _ in 0..120 {
        assert!(rate_allow_read(&mut requests, a, now));
    }
    assert!(!rate_allow_read(&mut requests, a, now));
    assert!(rate_allow_read(&mut requests, b, now));
    assert!(!rate_allow_read(
        &mut requests,
        a,
        now + std::time::Duration::from_secs(59)
    ));
    assert!(rate_allow_read(
        &mut requests,
        a,
        now + std::time::Duration::from_secs(60)
    ));
    assert_eq!(requests[&a].1, 1);
    requests.clear();
    for i in 0..4096_u32 {
        requests.insert(IpAddr::V4((0x0a000000 + i).into()), (now, 0));
    }
    let length = requests.len();
    assert!(!rate_allow_read(
        &mut requests,
        "203.0.113.1".parse().unwrap(),
        now
    ));
    assert_eq!(requests.len(), length);
}

#[tokio::test]
async fn native_page_24_gets_and_budget_exhaustion_do_not_consume_or_reset_mutations() {
    let (_dir, a, url, task) = fixture().await;
    let owner = a.store.list("artist".into()).await.unwrap().id;
    let id = a
        .store
        .save_picture(
            owner,
            "Native Art".into(),
            2,
            "elf".into(),
            test_png(8, 8),
            true,
        )
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let peer = "127.0.0.1:1234".parse().unwrap();
    for _ in 0..20 {
        assert!(admit(&a, &HeaderMap::new(), peer).unwrap());
    }
    let listed = client
        .post(format!("{url}/legacy-api/picture/list"))
        .form(&[("myGUID", "artist"), ("num", "24")])
        .send()
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    assert!(listed.text().await.unwrap().contains("<Mine/>"));
    for _ in 0..24 {
        let response = client
            .post(format!("{url}/legacy-api/picture/get"))
            .form(&[("id", id)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.bytes().await.unwrap(), test_png(8, 8));
    }
    assert_eq!(a.requests.lock().unwrap().values().next().unwrap().1, 20);
    for _ in 25..120 {
        let response = client
            .post(format!("{url}/legacy-api/picture/get"))
            .form(&[("id", id)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = client
        .post(format!("{url}/legacy-api/picture/get"))
        .form(&[("id", id)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(!admit(&a, &HeaderMap::new(), peer).unwrap());
    assert_eq!(
        a.picture_reads.lock().unwrap().values().next().unwrap().1,
        121
    );
    a.picture_reads
        .lock()
        .unwrap()
        .values_mut()
        .for_each(|(start, _)| {
            *start = std::time::Instant::now() - std::time::Duration::from_secs(60)
        });
    assert!(admit_read(&a, &HeaderMap::new(), peer).unwrap());
    assert!(!admit(&a, &HeaderMap::new(), peer).unwrap());
    let mut headers = HeaderMap::new();
    headers.insert("origin", "https://evil.invalid".parse().unwrap());
    assert!(admit_read(&a, &headers, peer).is_err());
    let mut proxy = a.clone();
    proxy.behind_proxy = true;
    assert!(admit_read(&proxy, &HeaderMap::new(), peer).is_err());
    headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "192.0.2.4, 192.0.2.5".parse().unwrap());
    assert!(admit_read(&proxy, &headers, peer).is_err());
    task.abort();
}

#[tokio::test]
async fn native_delete_authentication_cannot_be_spoofed_by_gallery_presentation_fields() {
    let (dir, a, url, task) = fixture().await;
    let owner = a
        .store
        .register(
            "revoked".into(),
            "owner@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    a.store
        .register(
            "other".into(),
            "other@example.org".into(),
            "other-password".into(),
        )
        .await
        .unwrap();
    let id = a
        .store
        .save_picture(
            owner,
            "Native Art".into(),
            2,
            "elf".into(),
            test_png(8, 8),
            true,
        )
        .await
        .unwrap();
    let client = reqwest::Client::new();
    for (guid, password) in [
        ("revoked", ""),
        ("owner@example.org", "wrong-password"),
        ("other@example.org", "other-password"),
    ] {
        let response = client
            .post(format!("{url}/legacy-api/picture/delete"))
            .form(&[
                ("guid", guid),
                ("password", password),
                ("id", &id.to_string()),
                ("myGUID", "owner@example.org"),
                ("Admin", "true"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(a.store.picture(id).await.is_ok());
    }
    let response = client
        .post(format!("{url}/legacy-api/picture/delete"))
        .form(&[
            ("guid", "OWNER@EXAMPLE.ORG"),
            ("password", "valid-password"),
            ("id", &id.to_string()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "<Success/>");
    assert!(a.store.picture(id).await.is_err());
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT next_id FROM picture_sequence", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        id + 1
    );
    task.abort();
}

fn native_multipart_body(fields: &[(&str, &str)], png: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend(b"--aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n");
        if *name == "data" {
            body.extend(b"Content-Disposition: form-data; name=\"data\"; filename=\"Foo.png\"\r\nContent-Type: application/octet-stream\r\n\r\n");
            body.extend(png);
        } else {
            body.extend(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}")
                    .as_bytes(),
            );
        }
        body.extend(b"\r\n");
    }
    body.extend(b"--aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa--");
    body
}

#[tokio::test]
async fn native_transport_metadata_exact_values_duplicates_unknowns_and_overwrite_are_atomic() {
    let (dir, a, url, task) = fixture().await;
    a.store.list("native-transport".into()).await.unwrap();
    let fields = vec![
        ("guid", "native-transport"),
        ("password", ""),
        ("secret", ""),
        ("name", "Native Wire"),
        ("dataType", "2"),
        ("tags", "native, wire"),
        ("overwrite", "on"),
        ("Filename", "Foo.png"),
        ("data", ""),
        ("Upload", "Submit Query"),
    ];
    let http = reqwest::Client::new();
    let send = |fields: Vec<(&str, &str)>, png: Vec<u8>| {
        http.post(format!("{url}/legacy-api/picture/save"))
            .header(
                "Content-Type",
                "multipart/form-data; boundary=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .body(native_multipart_body(&fields, &png))
            .send()
    };
    let mut identity = None;
    let replacement = test_png(16, 8);
    for png in [test_png(8, 8), replacement.clone()] {
        let response = send(fields.clone(), png.clone()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let text = response.text().await.unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();
        let id: i32 = doc
            .descendants()
            .find(|n| n.has_tag_name("Id"))
            .unwrap()
            .text()
            .unwrap()
            .parse()
            .unwrap();
        if let Some(previous) = identity {
            assert_eq!(id, previous);
        }
        identity = Some(id);
        assert_eq!(a.store.picture(id).await.unwrap(), png);
    }
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let before: (i64, i64) = db
        .query_row(
            "SELECT (SELECT count(*) FROM pictures),next_id FROM picture_sequence",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let oversized = "x".repeat(257);
    for (name, value, duplicate, expected) in [
        ("Filename", "../evil.png", false, "invalid picture Filename"),
        ("Upload", "Upload", false, "invalid picture Upload"),
        ("Filename", "Foo.png", true, "duplicate picture metadata"),
        ("Upload", "Submit Query", true, "duplicate picture metadata"),
        ("unexpected", "", true, "unsupported picture field"),
        (
            "Upload",
            oversized.as_str(),
            false,
            "picture metadata exceeds 256 bytes",
        ),
    ] {
        let mut malformed = fields.clone();
        if !duplicate {
            malformed.retain(|(key, _)| *key != name);
        }
        malformed.push((name, value));
        let response = send(malformed, test_png(8, 8)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.text().await.unwrap().contains(expected));
        assert_eq!(
            a.store.picture(identity.unwrap()).await.unwrap(),
            replacement
        );
        let after: (i64, i64) = db
            .query_row(
                "SELECT (SELECT count(*) FROM pictures),next_id FROM picture_sequence",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            db.query_row("SELECT name FROM pictures", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "Native Wire"
        );
    }
    task.abort();
}
