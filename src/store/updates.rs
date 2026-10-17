use super::*;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePost {
    pub id: String,
    pub content: String,
    pub published_at: i64,
    pub edited_at: Option<i64>,
    #[serde(default)]
    pub attachments: Vec<UpdateAttachment>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateAttachment {
    pub filename: String,
    pub url: String,
    pub content_type: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSnapshot {
    pub guild_id: String,
    pub channel_id: String,
    pub posts: Vec<UpdatePost>,
}
#[derive(Debug, Serialize)]
pub struct UpdateFeed {
    pub synced_at: Option<i64>,
    pub source: Option<UpdateSnapshot>,
}
pub fn discord_id(id: &str) -> bool {
    (17..=20).contains(&id.len())
        && !id.starts_with('0')
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<u64>().is_ok()
}
fn preview(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}
impl UpdatePost {
    pub fn title(&self) -> String {
        // The unmodified client does not clip titles before the right-aligned date.
        preview(
            self.content
                .lines()
                .next()
                .expect("validated update content"),
            32,
        )
    }
    fn tagline(&self) -> String {
        preview(
            &self
                .content
                .lines()
                .skip(1)
                .flat_map(str::split_whitespace)
                .collect::<Vec<_>>()
                .join(" "),
            60,
        )
    }
}
impl UpdateSnapshot {
    pub fn validate(&mut self) -> Result<()> {
        ensure!(
            discord_id(&self.guild_id) && discord_id(&self.channel_id),
            "Invalid update source IDs"
        );
        ensure!(
            self.posts.len() <= 100,
            "Update feed is limited to 100 posts"
        );
        let mut ids = HashSet::new();
        for post in &self.posts {
            ensure!(
                discord_id(&post.id) && ids.insert(&post.id),
                "Invalid or duplicate update ID"
            );
            ensure!(
                !post.content.trim().is_empty()
                    && post.content == post.content.trim()
                    && post.content.encode_utf16().count() <= 8000
                    && !post
                        .content
                        .chars()
                        .any(|c| (c.is_control() && c != '\n' && c != '\t')
                            || matches!(c, '\u{fffe}' | '\u{ffff}')),
                "Invalid update content"
            );
            ensure!(post.attachments.len() <= 10, "Too many update attachments");
            for attachment in &post.attachments {
                ensure!(
                    !attachment.filename.trim().is_empty()
                        && attachment.filename.encode_utf16().count() <= 256
                        && attachment.url.len() <= 2048
                        && [
                            "https://cdn.discordapp.com/attachments/",
                            "https://media.discordapp.net/attachments/"
                        ]
                        .iter()
                        .any(|prefix| attachment
                            .url
                            .strip_prefix(prefix)
                            .is_some_and(|path| !path.is_empty()))
                        && !attachment
                            .url
                            .chars()
                            .any(|c| c.is_whitespace() || c.is_control() || c == '#')
                        && attachment
                            .content_type
                            .as_ref()
                            .is_none_or(|mime| !mime.is_empty()
                                && mime.len() <= 128
                                && mime.bytes().all(|b| (32..=126).contains(&b))),
                    "Invalid update attachment"
                );
            }
            ensure!(
                (1..=i32::MAX as i64).contains(&post.published_at)
                    && post
                        .edited_at
                        .is_none_or(|t| t >= post.published_at && t <= i32::MAX as i64),
                "Invalid update timestamp"
            );
        }
        self.posts
            .sort_by_key(|p| std::cmp::Reverse(p.id.parse::<u64>().expect("validated Discord ID")));
        Ok(())
    }
}
impl UpdateFeed {
    pub fn news_xml(&self, origin: &str) -> String {
        let mut xml = String::new();
        if let Some(source) = &self.source {
            for post in source.posts.iter().take(10) {
                xml += &format!(
                    "<Item><Icon>news</Icon><Title>{}</Title><TagLine>{}</TagLine><Link>{}/updates/</Link><Date>{}</Date></Item>",
                    xml_escape(&post.title()),
                    xml_escape(&post.tagline()),
                    xml_escape(origin),
                    post.published_at
                );
            }
        }
        xml
    }
}
impl Store {
    pub async fn sync_updates(&self, mut snapshot: UpdateSnapshot) -> Result<UpdateFeed> {
        snapshot.validate()?;
        self.call(move |db| {
            let tx = db.transaction()?;
            let previous: Option<String> = tx.query_row("SELECT snapshot FROM update_feed WHERE id=1", [], |r| r.get(0)).optional()?;
            if let Some(raw) = previous {
                let previous: UpdateSnapshot = serde_json::from_str(&raw)?;
                ensure!(previous.guild_id == snapshot.guild_id && previous.channel_id == snapshot.channel_id,
                    "Update source differs from the published feed; refusing to replace it");
            }
            tx.execute("INSERT INTO update_feed(id,snapshot,synced_at) VALUES(1,?1,unixepoch()) ON CONFLICT(id) DO UPDATE SET snapshot=excluded.snapshot,synced_at=excluded.synced_at",
                [serde_json::to_string(&snapshot)?])?;
            let synced_at = tx.query_row("SELECT synced_at FROM update_feed WHERE id=1", [], |r| r.get(0))?;
            tx.commit()?;
            Ok(UpdateFeed {synced_at: Some(synced_at), source: Some(snapshot)})
        }).await
    }
    pub async fn updates(&self) -> Result<UpdateFeed> {
        self.call(|db| {
            let row: Option<(String, i64)> = db
                .query_row(
                    "SELECT snapshot,synced_at FROM update_feed WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match row {
                Some((raw, at)) => {
                    let mut source: UpdateSnapshot = serde_json::from_str(&raw)?;
                    source.validate()?;
                    Ok(UpdateFeed {
                        synced_at: Some(at),
                        source: Some(source),
                    })
                }
                None => Ok(UpdateFeed {
                    synced_at: None,
                    source: None,
                }),
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(crate) fn snapshot() -> UpdateSnapshot {
        UpdateSnapshot {
            guild_id: "12345678901234567".into(),
            channel_id: "23456789012345678".into(),
            posts: vec![UpdatePost {
                id: "34567890123456789".into(),
                content: "Fountains & <friends>\nHeal together <script>\nSecond paragraph".into(),
                published_at: 1700000000,
                edited_at: None,
                attachments: vec![UpdateAttachment {
                    filename: "demo.mp4".into(),
                    url: "https://cdn.discordapp.com/attachments/23456789012345678/34567890123456789/demo.mp4?ex=abc&hm=def".into(),
                    content_type: Some("video/mp4".into()),
                }],
            }],
        }
    }
    #[tokio::test]
    async fn updates_survive_restart_reconcile_edits_and_deletions_and_escape_native_news() {
        let content = Content::load(Path::new("data")).unwrap();
        let dir = std::env::temp_dir().join(format!("updates-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("game.db");
        let store = Store::open(&path, &content).unwrap();
        assert!(store.updates().await.unwrap().synced_at.is_none());
        let mut snapshot = snapshot();
        for n in 1..=11 {
            let mut post = snapshot.posts[0].clone();
            post.id = (34567890123456789u64 + n).to_string();
            snapshot.posts.push(post);
        }
        let feed = store.sync_updates(snapshot.clone()).await.unwrap();
        let xml = format!("<News>{}</News>", feed.news_xml("https://openrotmg.org"));
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(
            parsed
                .descendants()
                .filter(|n| n.has_tag_name("Item"))
                .count(),
            10
        );
        assert!(xml.contains("Fountains &amp; &lt;friends&gt;"));
        assert!(xml.contains("<Link>https://openrotmg.org/updates/</Link>"));
        drop(store);
        let store = Store::open(&path, &content).unwrap();
        assert_eq!(
            store.updates().await.unwrap().source.unwrap().posts.len(),
            12
        );
        assert_eq!(
            store.updates().await.unwrap().source.unwrap().posts[0].attachments[0]
                .content_type
                .as_deref(),
            Some("video/mp4")
        );
        snapshot.posts.truncate(1);
        snapshot.posts[0].content = "An edited update\nNew paragraph".into();
        snapshot.posts[0].edited_at = Some(1700000100);
        store.sync_updates(snapshot.clone()).await.unwrap();
        let feed = store.updates().await.unwrap();
        let posts = &feed.source.unwrap().posts;
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].title(), "An edited update");
        let mut wrong_source = snapshot.clone();
        wrong_source.channel_id = "45678901234567890".into();
        assert!(store.sync_updates(wrong_source).await.is_err());
        let mut invalid = snapshot.clone();
        invalid.posts.push(invalid.posts[0].clone());
        assert!(store.sync_updates(invalid).await.is_err());
        assert_eq!(
            store.updates().await.unwrap().source.unwrap().posts.len(),
            1
        );
        snapshot.posts.clear();
        assert!(
            store
                .sync_updates(snapshot)
                .await
                .unwrap()
                .source
                .unwrap()
                .posts
                .is_empty()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn updates_migrate_schema_six_and_reject_missing_schema_seven_state() {
        let content = Content::load(Path::new("data")).unwrap();
        let dir = std::env::temp_dir().join(format!("updates-migration-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("game.db");
        let store = Store::open(&path, &content).unwrap();
        let account = store.list("existing-schema-six".into()).await.unwrap();
        drop(store);
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE guild_members; DROP TABLE guilds; DROP TABLE update_feed; PRAGMA user_version=6")
            .unwrap();
        let store = Store::open(&path, &content).unwrap();
        assert_eq!(
            store.list("existing-schema-six".into()).await.unwrap().id,
            account.id
        );
        assert!(store.updates().await.unwrap().synced_at.is_none());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            10
        );
        store.sync_updates(snapshot()).await.unwrap();
        let mut invalid = snapshot();
        invalid.posts[0].content.clear();
        db.execute(
            "UPDATE update_feed SET snapshot=?1",
            [serde_json::to_string(&invalid).unwrap()],
        )
        .unwrap();
        assert!(store.updates().await.is_err());
        drop(store);
        db.execute("DROP TABLE update_feed", []).unwrap();
        assert!(
            Store::open(&path, &content)
                .err()
                .unwrap()
                .to_string()
                .contains("schema-7 update feed missing")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn old_snapshots_load_and_attachment_urls_are_restricted() {
        let mut raw = serde_json::to_value(snapshot()).unwrap();
        raw["posts"][0]
            .as_object_mut()
            .unwrap()
            .remove("attachments");
        let mut old: UpdateSnapshot = serde_json::from_value(raw).unwrap();
        old.validate().unwrap();
        assert!(old.posts[0].attachments.is_empty());
        for url in [
            "javascript:alert(1)",
            "https://cdn.discordapp.com.evil.test/attachments/x",
            "https://example.org/a",
            "https://cdn.discordapp.com/attachments/",
        ] {
            let mut s = snapshot();
            s.posts[0].attachments[0].url = url.into();
            assert!(s.validate().is_err());
        }
        let mut s = snapshot();
        s.posts[0].attachments = vec![s.posts[0].attachments[0].clone(); 11];
        assert!(s.validate().is_err());
    }
    #[test]
    fn updates_reject_invalid_bounded_input() {
        for id in [
            "",
            "01",
            "00000000000000000",
            "18446744073709551616",
            "<script>0000000000",
        ] {
            assert!(!discord_id(id));
        }
        for content in [
            " ".into(),
            " x".into(),
            "a\0b".into(),
            "a\u{ffff}b".into(),
            "x".repeat(8001),
            "😀".repeat(4001),
        ] {
            let mut s = snapshot();
            s.posts[0].content = content;
            assert!(s.validate().is_err());
        }
        let mut s = snapshot();
        s.posts[0].edited_at = Some(1);
        assert!(s.validate().is_err());
        let mut s = snapshot();
        s.posts = vec![s.posts[0].clone(); 101];
        assert!(s.validate().is_err());
        let mut s = snapshot();
        s.posts[0].content = "😀".repeat(80);
        s.validate().unwrap();
        assert_eq!(s.posts[0].title().chars().count(), 32);
        assert!(s.posts[0].title().ends_with('…'));
        assert_eq!(preview("Short title", 32), "Short title");
        assert_eq!(preview(&"x".repeat(100), 60).chars().count(), 60);
    }
}
