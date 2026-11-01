use super::*;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

pub(super) fn now() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?)
}
pub(super) fn discord_id(id: &str) -> Result<()> {
    ensure!(
        (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()),
        "invalid Discord identity"
    );
    Ok(())
}
pub(super) fn code_hash(code: &str) -> Result<String> {
    ensure!(
        code.len() == 32 && code.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid or expired link code"
    );
    Ok(format!("{:x}", Sha256::digest(code.as_bytes())))
}
impl Store {
    pub async fn compete(&self, account: i32) -> Result<()> {
        self.call(move |db| {
            db.execute("INSERT OR IGNORE INTO competitors(account) SELECT account FROM credentials WHERE account=?1", [account])?;
            Ok(())
        }).await
    }
    pub async fn issue_discord_code(&self, account: i32) -> Result<String> {
        let time = now()?;
        let code = format!("{:032x}", rand::random::<u128>());
        let hash = code_hash(&code)?;
        self.call(move |db| {
            let tx = db.transaction()?;
            let registered: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM credentials WHERE account=?1)", [account], |r| r.get(0))?;
            ensure!(registered, Rejected("register and sign in before linking Discord"));
            let linked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM discord_links WHERE account=?1)", [account], |r| r.get(0))?;
            ensure!(!linked, Rejected("this game account is already linked"));
            let previous: Option<i64> = tx.query_row("SELECT issued_at FROM discord_codes WHERE account=?1", [account], |r| r.get(0)).optional()?;
            ensure!(previous.is_none_or(|t| time-t >= 60), Rejected("wait one minute before requesting another link code"));
            tx.execute("DELETE FROM discord_codes WHERE expires_at<=?1", [time])?;
            tx.execute("INSERT INTO discord_codes(account,hash,issued_at,expires_at) VALUES(?1,?2,?3,?4) ON CONFLICT(account) DO UPDATE SET hash=excluded.hash,issued_at=excluded.issued_at,expires_at=excluded.expires_at", params![account,hash,time,time+300])?;
            tx.commit()?;
            Ok(())
        }).await?;
        tracing::info!(account, "private Discord link code issued");
        Ok(code)
    }
    pub async fn redeem_discord_code(
        &self,
        code: String,
        discord: String,
    ) -> Result<(i32, String)> {
        discord_id(&discord)?;
        let hash = code_hash(&code)?;
        let time = now()?;
        self.call(move |db| {
            let tx = db.transaction()?;
            let account: Option<i32> = tx.query_row("SELECT account FROM discord_codes WHERE hash=?1 AND expires_at>?2", params![hash,time], |r| r.get(0)).optional()?;
            let account = account.context("invalid or expired link code")?;
            let name: String = tx.query_row("SELECT name FROM accounts JOIN credentials ON credentials.account=accounts.id WHERE id=?1", [account], |r| r.get(0))?;
            let conflict: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM discord_links WHERE account=?1 OR discord_id=?2)", params![account,discord], |r| r.get(0))?;
            ensure!(!conflict, "a game or Discord account is already linked; links are never silently replaced");
            tx.execute("INSERT INTO discord_links(account,discord_id) VALUES(?1,?2)", params![account,discord])?;
            tx.execute("DELETE FROM discord_codes WHERE account=?1", [account])?;
            tx.execute("INSERT INTO discord_recovery(account,enabled_at,last_issued_at) VALUES(?1,?2,0) ON CONFLICT(account) DO NOTHING", params![account,time])?;
            let rewarded = gold::grant(&tx, account, gold::DISCORD)?;
            tx.commit()?;
            tracing::info!(account, rewarded, reward=gold::DISCORD, "Discord account linked");
            Ok((account, name))
        }).await
    }
    pub async fn hiscores(&self, living: Vec<Character>) -> Result<serde_json::Value> {
        let base = self.legacy_base;
        let classes = self.classes.clone();
        self.call(move |db| {
            let tx = db.transaction()?;
            let mut records = std::collections::BTreeMap::<i32, serde_json::Value>::new();
            {
                let mut q=tx.prepare("SELECT a.id,a.name,l.discord_id,f.balance FROM accounts a JOIN competitors c ON c.account=a.id LEFT JOIN account_fame f ON f.account=a.id LEFT JOIN discord_links l ON l.account=a.id WHERE NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=a.id) ORDER BY a.id")?;
                for row in q.query_map([], |r| Ok((r.get::<_,i32>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,i64>(3)?)))? {
                    let (id,name,discord,balance)=row?;
                    ensure!(balance >= 0, "negative account fame");
                    // Account stars sum class bests, not spendable fame: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/appengine/SavedCharactersList.as#L127-L139
                    let (presentation, _) = read_presentation(&tx, id, base, &classes)?;
                    records.insert(id,serde_json::json!({"account":id,"name":name,"discord_id":discord,"base_fame":0,"total_fame":balance,"collection":0,"quests":0,"stars":presentation.stars()}));
                }
            }
            let mut characters = std::collections::BTreeMap::new();
            {
                // Tombstones prevent stale world snapshots from restoring dead or deleted scores.
                // Bot characters were 96% of rows and made each call cost ~0.7 s of CPU beside the simulation.
                let mut q=tx.prepare("SELECT c.id,c.alive,c.stats FROM characters c JOIN competitors k ON k.account=c.account WHERE NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=c.account)")?;
                for row in q.query_map([],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,bool>(1)?,r.get::<_,String>(2)?)))? {
                    let (id,alive,raw)=row?;
                    let mut c=decode_character(&raw,base)?; c.id=id; c.alive=alive;
                    characters.insert(id,c);
                }
            }
            for c in living {
                if characters.get(&c.id).is_some_and(|saved| !saved.alive) { continue; }
                characters.insert(c.id,c);
            }
            for c in characters.values() {
                // Quests Completed is each character's assigned-quest kill count: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/FameCounter.cs#L81-L82
                if let Some(row)=records.get_mut(&c.account) {
                    let total=row["quests"].as_u64().context("invalid quest total")?;
                    row["quests"]=total.checked_add(c.lifetime.qualified_quests).context("quest total overflow")?.into();
                }
            }
            let mut living = Vec::new();
            for c in characters.values().filter(|c| c.alive) {
                if let Some(row)=records.get_mut(&c.account) {
                    ensure!(c.fame >= 0, "negative persisted fame for character {}", c.id);
                    let best=row["base_fame"].as_i64().context("invalid best fame")?;
                    if i64::from(c.fame)>best {row["base_fame"]=c.fame.into();}
                    // Equipment only: inventories stay private.
                    living.push(serde_json::json!({"account":c.account,"id":c.id,"class":c.class,"level":c.level,"exp":c.exp,"fame":c.fame,"stats":c.base,"equipment":c.items.get(..4).context("character without equipment slots")?,"tex1":c.tex1,"tex2":c.tex2,"created_at":c.created_at}));
                }
            }
            {
                let mut q=tx.prepare("SELECT account,count(*) FROM collections GROUP BY account")?;
                for row in q.query_map([],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,i64>(1)?)))? {
                    let (id,count)=row?;if let Some(row)=records.get_mut(&id) {row["collection"]=count.into();}
                }
            }
            // Guilds are public in the game (the name over each member's head, the guild hall), so
            // each player's guild and every guild's standing are shared; the guild board stays
            // private. Only guilds with a competing player in them are listed, like players.
            let mut guilds = Vec::new();
            {
                let mut members = std::collections::BTreeMap::<i32, u32>::new();
                let mut q=tx.prepare("SELECT m.account,g.id,g.name,m.rank,m.fame FROM guild_members m JOIN guilds g ON g.id=m.guild")?;
                for row in q.query_map([],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,i32>(1)?,r.get::<_,String>(2)?,r.get::<_,i32>(3)?,r.get::<_,i32>(4)?)))? {
                    let (account,id,name,rank,fame)=row?;
                    if let Some(row)=records.get_mut(&account) {
                        row["guild"]=serde_json::json!({"id":id,"name":name,"rank":rank,"fame":fame});
                        *members.entry(id).or_default()+=1;
                    }
                }
                drop(q);
                let mut q=tx.prepare("SELECT id,name,level,fame,earned FROM guilds ORDER BY id")?;
                for row in q.query_map([],|r|Ok((r.get::<_,i32>(0)?,r.get::<_,String>(1)?,r.get::<_,i32>(2)?,r.get::<_,i32>(3)?,r.get::<_,i32>(4)?)))? {
                    let (id,name,level,fame,earned)=row?;
                    if let Some(&count)=members.get(&id) {
                        guilds.push(serde_json::json!({"id":id,"name":name,"level":level,"fame":fame,"earned":earned,"members":count}));
                    }
                }
            }
            tx.commit()?;
            Ok(serde_json::json!({"updated_at":now()?,"players":records.into_values().collect::<Vec<_>>(),"characters":living,"guilds":guilds}))
        }).await
    }
    /// Player deaths from `since` (inclusive), oldest first, at most 500; bots and non-competitors never appear.
    pub async fn graveyard(&self, since: i64) -> Result<serde_json::Value> {
        let legacy_base = self.legacy_base;
        self.call(move |db| {
            let tx=db.transaction()?;
            let mut deaths=Vec::new();
            {
                let mut q=tx.prepare("SELECT d.snapshot,d.receipt,d.killed_by,d.died_at FROM deaths d JOIN characters c ON c.id=d.character JOIN competitors k ON k.account=c.account WHERE d.died_at>=?1 AND NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=c.account) ORDER BY d.died_at,d.character LIMIT 500")?;
                for row in q.query_map([since],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?)))? {
                    let (raw,paid,killed_by,died_at)=row?;
                    let receipt=death_awards::decode_receipt(&paid)?;
                    let c=receipt.character(&raw,legacy_base)?;
                    deaths.push(serde_json::json!({"account":c.account,"id":c.id,"name":c.name,"class":c.class,"level":c.level,"base_fame":receipt.base,"total_fame":receipt.total,"died_at":died_at,"killed_by":killed_by,"equipment":c.items.get(..4).context("death without equipment slots")?,"stats":c.base,"tex1":c.tex1,"tex2":c.tex2}));
                }
            }
            tx.commit()?;
            Ok(serde_json::json!({"deaths":deaths}))
        }).await
    }
    pub async fn collection(&self, account: i32) -> Result<serde_json::Value> {
        self.call(move |db| {
            let tx=db.transaction()?;
            let name:String=tx.query_row("SELECT name FROM accounts JOIN competitors ON competitors.account=accounts.id WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM shore_bots WHERE shore_bots.account=accounts.id)",[account],|r|r.get(0)).context("player has no competitive profile")?;
            let mut q=tx.prepare("SELECT item,found_at FROM collections WHERE account=?1 ORDER BY item")?;
            let items=q.query_map([account],|r|Ok(serde_json::json!({"item":r.get::<_,i32>(0)?,"found_at":r.get::<_,i64>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            drop(q);
            let players:i64=tx.query_row("SELECT count(*) FROM competitors c WHERE NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=c.account)",[],|r|r.get(0))?;
            let mut q=tx.prepare("SELECT x.item,count(*) FROM collections x JOIN competitors c ON c.account=x.account WHERE NOT EXISTS(SELECT 1 FROM shore_bots b WHERE b.account=x.account) GROUP BY x.item ORDER BY x.item")?;
            let owners=q.query_map([],|r|Ok(serde_json::json!({"item":r.get::<_,i32>(0)?,"owners":r.get::<_,i64>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            drop(q);tx.commit()?;
            Ok(serde_json::json!({"account":account,"name":name,"items":items,"ownership":{"players":players,"items":owners}}))
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn account_presentation_reads_characters_by_index() {
        let content = Content::load(Path::new("data")).unwrap();
        let path =
            std::env::temp_dir().join(format!("presentation-plan-{}.db", rand::random::<u64>()));
        let store = Store::open(&path, &content).unwrap();
        let plan: Vec<String> = store
            .call(|db| {
                let mut q = db.prepare(&format!("EXPLAIN QUERY PLAN {PRESENTATION_CHARACTERS}"))?;
                let plan = q
                    .query_map([1], |r| r.get(3))?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(plan)
            })
            .await
            .unwrap();
        // Hiscores run this per competitor; a table scan cost ~0.7 s of CPU per call in production.
        assert!(
            plan.iter().all(|step| !step.starts_with("SCAN")),
            "{plan:?}"
        );
    }

    #[tokio::test]
    async fn collection_ownership_counts_all_competitors_once_and_excludes_bots_and_outsiders() {
        let content = Content::load(Path::new("data")).unwrap();
        let path =
            std::env::temp_dir().join(format!("collection-owners-{}.db", rand::random::<u64>()));
        let store = Store::open(&path, &content).unwrap();
        let mut owners = Vec::new();
        for name in ["Player", "Empty", "Shorebot", "Outsider"] {
            let id = store
                .register(
                    format!("ownership-{name}"),
                    format!("{name}@example.invalid"),
                    "fixture-password".into(),
                )
                .await
                .unwrap();
            if name != "Outsider" {
                store.compete(id).await.unwrap();
            }
            owners.push(id);
        }
        let ids = owners.clone();
        store
            .call(move |db| {
                db.execute(
                    "INSERT INTO shore_bots(name,account) VALUES('fixture',?1)",
                    [ids[2]],
                )?;
                for id in [ids[0], ids[2], ids[3]] {
                    db.execute(
                        "INSERT INTO collections(account,item,found_at) VALUES(?1,2594,123)",
                        [id],
                    )?;
                }
                db.execute(
                    "INSERT INTO collections(account,item,found_at) VALUES(?1,2595,456)",
                    [ids[0]],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let collection = store.collection(owners[0]).await.unwrap();
        assert_eq!(collection["items"][0]["found_at"], 123);
        assert_eq!(
            collection["ownership"],
            serde_json::json!({"players":2,"items":[{"item":2594,"owners":1},{"item":2595,"owners":1}]})
        );
        assert_eq!(
            store.collection(owners[1]).await.unwrap()["ownership"],
            collection["ownership"]
        );
        assert!(store.collection(owners[2]).await.is_err());
        assert!(store.collection(owners[3]).await.is_err());
    }

    #[tokio::test]
    async fn characters_and_graveyard_list_only_competing_players() {
        let content = Content::load(Path::new("data")).unwrap();
        let path = std::env::temp_dir().join(format!(
            "graveyard-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let store = Store::open(&path, &content).unwrap();
        let mut owners = Vec::new();
        for name in ["Player", "Shorebot", "Outsider"] {
            let id = store
                .register(
                    format!("graveyard-{name}"),
                    format!("{name}@example.invalid"),
                    "fixture-password".into(),
                )
                .await
                .unwrap();
            store.choose_name(id, name.into()).await.unwrap();
            if name != "Outsider" {
                store.compete(id).await.unwrap();
            }
            owners.push(id);
        }
        let bot = owners[1];
        store
            .call(move |db| {
                db.execute(
                    "INSERT INTO shore_bots(name,account) VALUES('fixture',?1)",
                    [bot],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let mut live = Vec::new();
        for owner in owners.iter().copied() {
            // Each account has one slot, so the death comes first and frees it.
            for alive in [false, true] {
                let mut ch = store
                    .create(owner, content.get(WIZARD).unwrap())
                    .await
                    .unwrap();
                ch.level = 20;
                ch.exp = 20000;
                ch.fame = 70;
                store.progress(ch.clone()).await.unwrap();
                if alive {
                    ch.fame = 90;
                    live.push(ch);
                } else {
                    ch.alive = false;
                    ch.hp = 0;
                    store.death(ch, "fixture".into()).await.unwrap();
                }
            }
        }
        // Hiscores never decode bot characters; production keeps tens of thousands of them.
        store
            .call(move |db| {
                db.execute("UPDATE characters SET stats='{' WHERE account=?1", [bot])?;
                Ok(())
            })
            .await
            .unwrap();
        // Live world state wins over saved fame, and bots stay out even when online.
        let scores = store.hiscores(live.clone()).await.unwrap();
        let characters = scores["characters"].as_array().unwrap();
        assert_eq!(characters.len(), 1);
        assert_eq!(characters[0]["id"], live[0].id);
        assert_eq!(characters[0]["fame"], 90);
        assert_eq!(characters[0]["equipment"].as_array().unwrap().len(), 4);
        let deaths = store.graveyard(0).await.unwrap();
        let deaths = deaths["deaths"].as_array().unwrap();
        assert_eq!(deaths.len(), 1);
        assert_eq!(deaths[0]["account"], owners[0]);
        assert_eq!(deaths[0]["base_fame"], 70);
        assert_eq!(deaths[0]["killed_by"], "fixture");
        assert_eq!(deaths[0]["stats"].as_array().unwrap().len(), 8);
        let later = store.graveyard(i64::MAX).await.unwrap();
        assert!(later["deaths"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn hiscores_share_guild_standing_and_death_fame() {
        let content = Content::load(Path::new("data")).unwrap();
        let path = std::env::temp_dir().join(format!(
            "guild-hiscores-{}-{}.db",
            std::process::id(),
            rand::random::<u64>()
        ));
        let store = Store::open(&path, &content).unwrap();
        let mut owners = Vec::new();
        for name in ["Founder", "Loner", "Outsider"] {
            let id = store
                .register(
                    format!("guild-{name}"),
                    format!("{name}@example.invalid"),
                    "fixture-password".into(),
                )
                .await
                .unwrap();
            store.choose_name(id, name.into()).await.unwrap();
            if name != "Outsider" {
                store.compete(id).await.unwrap();
            }
            owners.push(id);
        }
        for owner in [owners[0], owners[2]] {
            store
                .call(move |db| {
                    db.execute(
                        "UPDATE account_fame SET balance=1000,earned=1000 WHERE account=?1",
                        [owner],
                    )?;
                    Ok(())
                })
                .await
                .unwrap();
        }
        store
            .change_guild(owners[0], GuildAction::Create("Mad Gods".into()))
            .await
            .unwrap();
        store
            .change_guild(owners[2], GuildAction::Create("Hidden".into()))
            .await
            .unwrap();
        // A member's death pays its full fame to the guild, as it does to the account.
        let mut ch = store
            .create(owners[0], content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.level = 20;
        ch.exp = 20000;
        ch.fame = 70;
        store.progress(ch.clone()).await.unwrap();
        ch.alive = false;
        ch.hp = 0;
        store.death(ch, "fixture".into()).await.unwrap();
        let total = store.graveyard(0).await.unwrap()["deaths"][0]["total_fame"]
            .as_i64()
            .unwrap();
        assert!(total >= 70);
        let scores = store.hiscores(Vec::new()).await.unwrap();
        let guilds = scores["guilds"].as_array().unwrap();
        assert_eq!(
            guilds.len(),
            1,
            "guilds without a competing player stay out"
        );
        assert_eq!(guilds[0]["name"], "Mad Gods");
        assert_eq!(guilds[0]["level"], 1);
        assert_eq!(guilds[0]["members"], 1);
        assert_eq!(guilds[0]["earned"], total);
        assert_eq!(guilds[0]["fame"], total);
        let players = scores["players"].as_array().unwrap();
        let founder = players.iter().find(|p| p["account"] == owners[0]).unwrap();
        assert_eq!(founder["guild"]["name"], "Mad Gods");
        assert_eq!(founder["guild"]["rank"], 40);
        assert_eq!(founder["guild"]["fame"], total);
        let loner = players.iter().find(|p| p["account"] == owners[1]).unwrap();
        assert!(loner.get("guild").is_none());
    }
}
