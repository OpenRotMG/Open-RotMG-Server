use super::*;

fn window(span: &str) -> Result<i64> {
    match span {
        "week" => Ok(7 * 86400),
        "month" => Ok(30 * 86400),
        "all" => Ok(i64::MAX),
        _ => anyhow::bail!("timespan must be week, month or all"),
    }
}
fn entry(tag: &str, character: &Character) -> Result<String> {
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
        character.fame
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
        let cutoff = now.saturating_sub(window(&span)?).max(0);
        let legacy_base = self.legacy_base;
        self.call(move |db| {
            let tx = db.transaction()?;
            let mut xml = format!("<FameList timespan=\"{span}\">");
            let mut mine_listed = false;
            {
                let mut q = tx.prepare("SELECT snapshot FROM deaths WHERE died_at>?1 AND died_at<=?2 ORDER BY json_extract(snapshot,'$.fame') DESC,died_at ASC,character ASC LIMIT 20")?;
                for row in q.query_map(params![cutoff,now],|r|r.get::<_,String>(0))? {
                    let c = decode_character(&row?,legacy_base)?;
                    mine_listed |= c.account == account && c.id == character;
                    xml.push_str(&entry("FameListElem",&c)?);
                }
            }
            if !mine_listed && account > 0 && character > 0 {
                let mut q = tx.prepare("SELECT snapshot FROM deaths JOIN characters ON characters.id=deaths.character WHERE characters.account=?1 AND deaths.character=?2 AND died_at>?3 AND died_at<=?4")?;
                for row in q.query_map(params![account,character,cutoff,now],|r|r.get::<_,String>(0))? {
                    xml.push_str(&entry("MyFameListElem",&decode_character(&row?,legacy_base)?)?);
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
    #[tokio::test]
    async fn periods_ties_personal_row_and_death_equipment_are_consistent() {
        let content = Content::load(Path::new("data")).unwrap();
        let path = std::env::temp_dir().join(format!(
            "legends-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let s = Store::open(&path, &content).unwrap();
        let now = 100 * 86400;
        let mut ids = vec![];
        for i in 0..23 {
            let a = s.list(format!("legend-{i}")).await.unwrap();
            let mut ch = s.create(a.id, content.get(WIZARD).unwrap()).await.unwrap();
            ch.name = "A<&>".into();
            ch.fame = if i < 2 { 1000 } else { 100 - i };
            ch.hp = 0;
            ch.alive = false;
            let equipment = ch.items.clone();
            s.death(ch.clone(), "Pirate".into()).await.unwrap();
            ids.push((a.id, ch.id, equipment));
        }
        s.call(move |db| {
            db.execute("UPDATE deaths SET died_at=?1", [now - 1])?;
            Ok(())
        })
        .await
        .unwrap();
        let xml = s
            .legends_at("week".into(), ids[22].0, ids[22].1, now)
            .await
            .unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let rows: Vec<_> = doc
            .root_element()
            .children()
            .filter(|n| n.has_tag_name("FameListElem"))
            .collect();
        assert_eq!(rows.len(), 20);
        assert_eq!(
            rows[0].attribute("charId"),
            Some(ids[0].1.to_string().as_str())
        );
        assert_eq!(
            rows[1].attribute("charId"),
            Some(ids[1].1.to_string().as_str())
        );
        assert_eq!(
            rows[0]
                .children()
                .find(|n| n.has_tag_name("Name"))
                .unwrap()
                .text(),
            Some("A<&>")
        );
        assert_eq!(
            rows[0]
                .children()
                .find(|n| n.has_tag_name("Equipment"))
                .unwrap()
                .text()
                .unwrap(),
            ids[0]
                .2
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(
            doc.root_element()
                .children()
                .filter(|n| n.has_tag_name("MyFameListElem"))
                .count(),
            1
        );
        assert!(
            !s.legends_at("week".into(), ids[0].0, ids[0].1, now)
                .await
                .unwrap()
                .contains("MyFameListElem")
        );
        s.rename(ids[0].0, "Changed".into()).await.unwrap();
        assert!(
            s.legends_at("all".into(), -1, -1, now)
                .await
                .unwrap()
                .contains("A&lt;&amp;&gt;")
        );
        s.call(move |db| {
            db.execute("UPDATE deaths SET died_at=?1", [now - 7 * 86400])?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(
            !s.legends_at("week".into(), -1, -1, now)
                .await
                .unwrap()
                .contains("FameListElem")
        );
        assert!(
            s.legends_at("month".into(), -1, -1, now)
                .await
                .unwrap()
                .contains("FameListElem")
        );
        s.call(move |db| {
            db.execute("UPDATE deaths SET died_at=?1", [now - 30 * 86400])?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(
            !s.legends_at("month".into(), -1, -1, now)
                .await
                .unwrap()
                .contains("FameListElem")
        );
        assert!(
            s.legends_at("all".into(), -1, -1, now)
                .await
                .unwrap()
                .contains("FameListElem")
        );
        assert!(s.legends_at("day".into(), -1, -1, now).await.is_err());
        let reopened = Store::open(&path, &content).unwrap();
        assert_eq!(
            s.legends_at("all".into(), -1, -1, now).await.unwrap(),
            reopened
                .legends_at("all".into(), -1, -1, now)
                .await
                .unwrap()
        );
    }
}
