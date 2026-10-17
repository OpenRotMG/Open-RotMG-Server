use super::hiscores::{json_error, private_denial};
use super::*;
use crate::store::UpdateSnapshot;

fn response(result: Result<crate::store::UpdateFeed>) -> Response {
    match result {
        Ok(feed) => ([("cache-control", "no-store")], Json(feed)).into_response(),
        Err(error) => {
            tracing::error!(%error, "update feed storage failed");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Update feed storage failed",
            )
        }
    }
}
pub(super) async fn feed(State(a): State<App>) -> Response {
    response(a.store.updates().await)
}
pub(super) async fn sync(
    State(a): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(snapshot): Json<UpdateSnapshot>,
) -> Response {
    if let Some(denial) = private_denial(&a, peer, &headers) {
        return denial;
    }
    let count = snapshot.posts.len();
    match a.store.sync_updates(snapshot).await {
        Ok(feed) => {
            tracing::info!(
                posts = count,
                synced_at = feed.synced_at,
                "update feed synced"
            );
            response(Ok(feed))
        }
        Err(error) => {
            tracing::warn!(%error, "update feed sync rejected");
            json_error(StatusCode::BAD_REQUEST, error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn private_updates_publish_to_public_feed_and_native_character_list() {
        let (dir, app) = super::super::account_actions::tests::fixture();
        let post = crate::store::UpdatePost {
            id: "34567890123456789".into(),
            content: "A new update\nRead the full post".into(),
            published_at: 1700000000,
            edited_at: None,
            attachments: vec![],
        };
        let snapshot = UpdateSnapshot {
            guild_id: "12345678901234567".into(),
            channel_id: "23456789012345678".into(),
            posts: vec![post],
        };
        let peer = "127.0.0.1:1234".parse().unwrap();
        assert_eq!(
            sync(
                State(app.clone()),
                ConnectInfo(peer),
                HeaderMap::new(),
                Json(snapshot.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
        );
        let mut forwarded = headers.clone();
        forwarded.insert("x-forwarded-for", "192.0.2.1".parse().unwrap());
        assert_eq!(
            sync(
                State(app.clone()),
                ConnectInfo(peer),
                forwarded,
                Json(snapshot.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            sync(
                State(app.clone()),
                ConnectInfo(peer),
                headers,
                Json(snapshot)
            )
            .await
            .status(),
            StatusCode::OK
        );
        let public = feed(State(app.clone())).await;
        assert_eq!(public.status(), StatusCode::OK);
        assert_eq!(public.headers()["cache-control"], "no-store");
        let public: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(public.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            public["source"]["posts"][0]["content"],
            "A new update\nRead the full post"
        );
        let form = HashMap::from([
            ("guid".into(), "news-guest".into()),
            ("password".into(), "".into()),
        ]);
        let native = chars(
            State(app.clone()),
            ConnectInfo(peer),
            HeaderMap::new(),
            Form(form),
        )
        .await;
        assert_eq!(native.status(), StatusCode::OK);
        let xml = String::from_utf8(
            axum::body::to_bytes(native.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let link = doc
            .descendants()
            .find(|n| n.has_tag_name("Link"))
            .unwrap()
            .text()
            .unwrap();
        assert_eq!(link, "http://127.0.0.1:1/updates/");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
