use super::*;

fn window(span: &str) -> Result<i64> {
    match span {
        "week" => Ok(7 * 86400),
        "month" => Ok(30 * 86400),
        "all" => Ok(i64::MAX),
        _ => anyhow::bail!("timespan must be week, month or all"),
    }
}
fn entry(
    tag: &str,
    character: &Character,
    receipt: &death_awards::DeathFameReceipt,
) -> Result<String> {
    ensure!(
        character.fame >= 0 && character.items.len() == 12 && !character.alive && character.hp == 0,
        "invalid Legends death snapshot"
    );
    Ok(format!(
        "<{tag} accountId=\"{}\" charId=\"{}\"><Name>{}</Name><ObjectType>{}</ObjectType><Tex1>{}</Tex1><Tex2>{}</Tex2><Equipment>{}</Equipment><TotalFame>{}</TotalFame></{tag}>",
        character.account,
        character.id,
        xml_escape(&character.name),
        character.class,
        character.tex1,
        character.tex2,
        character
            .items
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(","),
        receipt.total
    ))
}
impl Store {
    pub async fn legends(&self, span: String, account: i32, character: i32) -> Result<String> {
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        self.legends_at(span, account, character, now).await
    }
    async fn legends_at(
        &self,
        span: String,
        account: i32,
        character: i32,
        now: i64,
    ) -> Result<String> {
        // Rolling 7/30-day windows are reconstruction, not recovered 123.5.1 policy: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/common/DbModels.cs
        let cutoff = now.saturating_sub(window(&span)?).max(0);
        let legacy_base = self.legacy_base;
        self.call(move |db| {
            let tx = db.transaction()?;
            let mut xml = format!("<FameList timespan=\"{span}\">");
            let mut mine_listed = false;
            {
                let mut q = tx.prepare("SELECT snapshot,receipt FROM deaths WHERE died_at>?1 AND died_at<=?2 ORDER BY json_extract(receipt,'$.total') DESC,died_at ASC,character ASC LIMIT 20")?;
                for row in q.query_map(params![cutoff,now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
                    let (raw, paid) = row?;
                    let receipt=death_awards::decode_receipt(&paid)?;
                    let c = receipt.character(&raw,legacy_base)?;
                    mine_listed |= c.account == account && c.id == character;
                    xml.push_str(&entry("FameListElem",&c,&receipt)?);
                }
            }
            if !mine_listed && account > 0 && character > 0 {
                let mut q = tx.prepare("SELECT snapshot,receipt FROM deaths JOIN characters ON characters.id=deaths.character WHERE characters.account=?1 AND deaths.character=?2 AND died_at>?3 AND died_at<=?4")?;
                for row in q.query_map(params![account,character,cutoff,now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {
                    let (raw, paid) = row?;
                    let receipt=death_awards::decode_receipt(&paid)?;
                    xml.push_str(&entry("MyFameListElem",&receipt.character(&raw,legacy_base)?,&receipt)?);
                }
            }
            xml.push_str("</FameList>");
            tx.commit()?;
            Ok(xml)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture() -> (Store, Content, i32) {
        let content = Content::load(Path::new("data")).unwrap();
        let path = std::env::temp_dir().join(format!("legends-{}.db", rand::random::<u64>()));
        let store = Store::open(&path, &content).unwrap();
        let account = store.list("legends-fixture".into()).await.unwrap().id;
        (store, content, account)
    }

    async fn death(store: &Store, content: &Content, account: i32, time: i64) -> Character {
        let mut ch = store
            .create(account, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.fame = 100;
        ch.name = "A<&".into();
        ch.alive = false;
        ch.hp = 0;
        store.death(ch.clone(), "fixture".into()).await.unwrap();
        let id = ch.id;
        store
            .call(move |db| {
                db.execute(
                    "UPDATE deaths SET died_at=?1 WHERE character=?2",
                    params![time, id],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        ch
    }

    fn ids(xml: &str, tag: &str) -> Vec<i32> {
        roxmltree::Document::parse(xml)
            .unwrap()
            .descendants()
            .filter(|n| n.has_tag_name(tag))
            .map(|n| n.attribute("charId").unwrap().parse().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn legends_windows_exclude_cutoff_and_future_deaths_but_retain_archives() {
        let (store, content, account) = fixture().await;
        let now = 2_000_000_000;
        let mut dead = Vec::new();
        for time in [
            now - 31 * 86400,
            now - 30 * 86400,
            now - 30 * 86400 + 1,
            now - 7 * 86400,
            now - 7 * 86400 + 1,
            now,
            now + 1,
        ] {
            dead.push(death(&store, &content, account, time).await.id);
        }
        let live = store
            .create(account, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        for (span, expected) in [
            ("week", vec![dead[4], dead[5]]),
            ("month", dead[2..6].to_vec()),
            ("all", dead[..6].to_vec()),
        ] {
            let xml = store
                .legends_at(span.into(), account, dead[0], now)
                .await
                .unwrap();
            assert_eq!(ids(&xml, "FameListElem"), expected);
            assert!(ids(&xml, "MyFameListElem").is_empty());
            assert!(!ids(&xml, "FameListElem").contains(&live.id));
        }
        assert!(store.fame(account, dead[0]).await.is_ok());
        assert!(
            store
                .legends_at("year".into(), account, 0, now)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn legends_cap_ties_own_row_and_details_use_committed_death_receipts() {
        let (store, content, account) = fixture().await;
        let now = 2_000_000_000;
        let mut dead = Vec::new();
        for _ in 0..21 {
            dead.push(death(&store, &content, account, now).await);
        }
        let mine = dead.last().unwrap();
        let xml = store
            .legends_at("all".into(), account, mine.id, now)
            .await
            .unwrap();
        assert_eq!(
            ids(&xml, "FameListElem"),
            dead[..20].iter().map(|c| c.id).collect::<Vec<_>>()
        );
        assert_eq!(ids(&xml, "MyFameListElem"), vec![mine.id]);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let row = doc
            .descendants()
            .find(|n| n.has_tag_name("MyFameListElem"))
            .unwrap();
        for (tag, value) in [
            ("Name", "A<&".to_owned()),
            ("ObjectType", WIZARD.to_string()),
            ("Tex1", mine.tex1.to_string()),
            ("Tex2", mine.tex2.to_string()),
            (
                "Equipment",
                mine.items
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ] {
            assert_eq!(
                row.children().find(|n| n.has_tag_name(tag)).unwrap().text(),
                Some(value.as_str())
            );
        }
        let fame = store.fame(account, mine.id).await.unwrap();
        let detail = roxmltree::Document::parse(&fame).unwrap();
        assert_eq!(
            row.children()
                .find(|n| n.has_tag_name("TotalFame"))
                .unwrap()
                .text(),
            detail
                .descendants()
                .find(|n| n.has_tag_name("TotalFame"))
                .unwrap()
                .text()
        );
        assert!(
            ids(
                &store
                    .legends_at("all".into(), account, dead[0].id, now)
                    .await
                    .unwrap(),
                "MyFameListElem"
            )
            .is_empty()
        );
        assert!(
            ids(
                &store
                    .legends_at("all".into(), account + 1, mine.id, now)
                    .await
                    .unwrap(),
                "MyFameListElem"
            )
            .is_empty()
        );
        store
            .call(move |db| {
                db.execute(
                    "UPDATE characters SET stats=json_set(stats,'$.fame',999999)",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            store
                .legends_at("all".into(), account, mine.id, now)
                .await
                .unwrap(),
            xml
        );
        let tied = dead[1].id;
        store
            .call(move |db| {
                db.execute(
                    "UPDATE deaths SET died_at=died_at+1 WHERE character=?1",
                    [tied],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let xml = store
            .legends_at("all".into(), account, 0, now + 1)
            .await
            .unwrap();
        assert_eq!(
            ids(&xml, "FameListElem"),
            std::iter::once(dead[0].id)
                .chain(dead[2..].iter().map(|c| c.id))
                .collect::<Vec<_>>()
        );
    }
}
