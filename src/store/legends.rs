use super::*;

// Seeded/legacy bot lives lack a provable zero-fame baseline: https://github.com/OpenRotMG/Open-RotMG-Server/issues/1049
const ELIGIBLE: &str = "(NOT EXISTS(SELECT 1 FROM shore_bots WHERE account=json_extract(snapshot,'$.account')) OR json_extract(snapshot,'$.lifetime.completeness')='complete')";

fn now() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?)
}

fn ranked(db: &Connection, cutoff: i64, now: i64, limit: i32) -> Result<Vec<(String, String)>> {
    let mut q = db.prepare(&format!("SELECT snapshot,receipt FROM deaths WHERE died_at>?1 AND died_at<=?2 AND {ELIGIBLE} ORDER BY json_extract(receipt,'$.total') DESC,died_at ASC,character ASC LIMIT ?3"))?;
    Ok(
        q.query_map(params![cutoff, now, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?,
    )
}

pub(super) fn is_legend_at(
    db: &Connection,
    account: i32,
    now: i64,
    legacy_base: [i32; 8],
) -> Result<bool> {
    let mut legendary = false;
    for span in ["week", "month", "all"] {
        for (raw, paid) in ranked(db, now.saturating_sub(window(span)?).max(0), now, 10)? {
            let receipt = death_awards::decode_receipt(&paid)?;
            let ch = receipt.character(&raw, legacy_base)?;
            validate(&ch)?;
            legendary |= ch.account == account;
        }
    }
    Ok(legendary)
}

fn window(span: &str) -> Result<i64> {
    match span {
        "week" => Ok(7 * 86400),
        "month" => Ok(30 * 86400),
        "all" => Ok(i64::MAX),
        _ => anyhow::bail!("timespan must be week, month or all"),
    }
}
fn validate(character: &Character) -> Result<()> {
    ensure!(
        character.fame >= 0 && character.items.len() == 12 && !character.alive && character.hp == 0,
        "invalid Legends death snapshot"
    );
    Ok(())
}
fn entry(
    tag: &str,
    character: &Character,
    receipt: &death_awards::DeathFameReceipt,
) -> Result<String> {
    validate(character)?;
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
    pub async fn is_legend(&self, account: i32) -> Result<bool> {
        let now = now()?;
        let base = self.legacy_base;
        self.call(move |db| is_legend_at(db, account, now, base))
            .await
    }
    pub async fn legends(&self, span: String, account: i32, character: i32) -> Result<String> {
        self.legends_at(span, account, character, now()?).await
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
                for (raw, paid) in ranked(&tx, cutoff, now, 20)? {
                    let receipt=death_awards::decode_receipt(&paid)?;
                    let c = receipt.character(&raw,legacy_base)?;
                    mine_listed |= c.account == account && c.id == character;
                    xml.push_str(&entry("FameListElem",&c,&receipt)?);
                }
            }
            if !mine_listed && account > 0 && character > 0 {
                let mut q = tx.prepare(&format!("SELECT snapshot,receipt FROM deaths JOIN characters ON characters.id=deaths.character WHERE characters.account=?1 AND deaths.character=?2 AND died_at>?3 AND died_at<=?4 AND {ELIGIBLE}"))?;
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
    async fn legends_only_rank_bot_lives_with_complete_earned_history() {
        let (store, content, human) = fixture().await;
        let mut preset = Character::fresh(content.get(WIZARD).unwrap()).unwrap();
        preset.lifetime = FameLifetime::known_only();
        preset.exp = 1_000_000;
        preset.fame = 1000;
        let bot = store
            .persona_character(
                "persona:v1:LegendBot".into(),
                vec![preset],
                vec![[-1; 8]],
                None,
            )
            .await
            .unwrap()
            .account;
        let mut seeded = Vec::new();
        for _ in 0..21 {
            let mut ch = store
                .create(bot, content.get(WIZARD).unwrap())
                .await
                .unwrap();
            ch.lifetime = FameLifetime::known_only();
            ch.exp = 1_000_000;
            ch.fame = 1000;
            let raw = serde_json::to_string(&ch).unwrap();
            let id = ch.id;
            store
                .call(move |db| {
                    db.execute(
                        "UPDATE characters SET stats=?1 WHERE id=?2",
                        params![raw, id],
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
            ch.gain_experience(&content, 1000, &mut rand::thread_rng())
                .unwrap();
            ch.alive = false;
            ch.hp = 0;
            store.death(ch, "fixture".into()).await.unwrap();
            seeded.push(id);
        }
        let now = 2_000_000_000;
        let base = store.legacy_base;
        assert!(
            !store
                .call(move |db| is_legend_at(db, bot, now, base))
                .await
                .unwrap()
        );
        let mut earned = store
            .create(bot, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        earned
            .gain_experience(&content, 100_000, &mut rand::thread_rng())
            .unwrap();
        earned.alive = false;
        earned.hp = 0;
        store.death(earned.clone(), "fixture".into()).await.unwrap();
        let human = death(&store, &content, human, now).await;
        let human_id = human.id;
        store
            .call(move |db| {
                let raw: String = db.query_row(
                    "SELECT snapshot FROM deaths WHERE character=?1",
                    [human_id],
                    |r| r.get(0),
                )?;
                let mut ch: Character = serde_json::from_str(&raw)?;
                ch.lifetime = FameLifetime::known_only();
                let receipt = death_awards::DeathFameReceipt::paid(
                    &ch,
                    0,
                    death_awards::ReceiptPolicy::LegacyPaid,
                )?;
                db.execute(
                    "UPDATE deaths SET snapshot=?1,receipt=?2 WHERE character=?3",
                    params![
                        serde_json::to_string(&ch)?,
                        serde_json::to_string(&receipt)?,
                        human_id
                    ],
                )?;
                db.execute("UPDATE deaths SET died_at=?1", [now])?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            store
                .call(move |db| is_legend_at(db, bot, now, base))
                .await
                .unwrap()
        );
        assert!(
            store
                .call(move |db| is_legend_at(db, human.account, now, base))
                .await
                .unwrap()
        );
        let archived = store.fame(bot, seeded[0]).await.unwrap();
        for span in ["week", "month", "all"] {
            let xml = store
                .legends_at(span.into(), bot, seeded[0], now)
                .await
                .unwrap();
            let rows = ids(&xml, "FameListElem");
            assert_eq!(
                rows.len(),
                2,
                "{span}: seeded rows must not consume the top twenty"
            );
            assert!(rows.contains(&earned.id) && rows.contains(&human.id));
            assert!(
                ids(&xml, "MyFameListElem").is_empty(),
                "seeded own-row must also stay excluded"
            );
        }
        assert_eq!(store.fame(bot, seeded[0]).await.unwrap(), archived);
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
        let (highest, tied) = (dead[0].id, dead[1].id);
        store
            .call(move |db| {
                db.execute(
                    "UPDATE deaths SET died_at=died_at+1 WHERE character IN (?1,?2)",
                    params![highest, tied],
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
