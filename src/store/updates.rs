use super::*;
use std::collections::HashSet;

#[cfg(test)]
mod tests;

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
fn news_item(icon: &str, title: &str, tagline: &str, link: &str, date: i64) -> String {
    format!(
        "<Item><Icon>{icon}</Icon><Title>{}</Title><TagLine>{}</TagLine><Link>{}</Link><Date>{date}</Date></Item>",
        xml_escape(title),
        xml_escape(tagline),
        xml_escape(link)
    )
}
impl Store {
    pub async fn news(&self, account: i32, origin: &str) -> Result<String> {
        let feed = self.updates().await?;
        let content = self.content.clone();
        let legacy_base = self.legacy_base;
        // Deriving news from committed deaths avoids duplicate events on retries: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/db/Database.cs#L42-L88
        let mut items = self.call(move |db| {
            let mut q = db.prepare("SELECT d.character,d.snapshot,d.receipt,d.died_at FROM deaths d JOIN characters c ON c.id=d.character WHERE c.account=?1 AND c.alive=0 ORDER BY d.died_at DESC,d.character DESC LIMIT 10")?;
            let mut items = Vec::new();
            for row in q.query_map([account], |r| Ok((r.get::<_, i32>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, i64>(3)?)))? {
                let (id, raw, paid, date) = row?;
                let receipt = death_awards::decode_receipt(&paid)?;
                let ch = receipt.character(&raw, legacy_base)?;
                ensure!(ch.account == account && ch.id == id && !ch.alive && ch.hp == 0 && ch.fame == receipt.base && (1..=i64::from(i32::MAX)).contains(&date), "invalid personal news death {id} for account {account}");
                let class = &content.get(ch.class)?.name;
                items.push((date, news_item("fame", &format!("Your {class} died at level {}",ch.level), &format!("You earned {} glorious Fame",receipt.total), &format!("fame:{id}"), date)));
            }
            Ok(items)
        }).await?;
        if let Some(source) = feed.source {
            for post in source.posts.into_iter().take(10) {
                items.push((
                    post.published_at,
                    news_item(
                        "news",
                        &preview(&format!("Discord: {}", post.title()), 32),
                        &post.tagline(),
                        &format!("{origin}/updates/"),
                        post.published_at,
                    ),
                ));
            }
        }
        items.sort_by_key(|(date, _)| std::cmp::Reverse(*date));
        Ok(items.into_iter().take(10).map(|(_, xml)| xml).collect())
    }
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
