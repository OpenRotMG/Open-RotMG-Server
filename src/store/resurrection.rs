use super::*;

impl Store {
    pub async fn resurrect(
        &self,
        mut ch: Character,
        slot: usize,
        content: std::sync::Arc<Content>,
    ) -> Result<Character> {
        ensure!(
            !ch.alive && ch.hp == 0 && ch.items.len() == 12 && slot < 4,
            "resurrection requires a fatal snapshot and equipped item"
        );
        let item = content.get(u16::try_from(ch.items[slot])?)?;
        ensure!(item.resurrects, "item does not grant resurrection");
        ensure!(
            ch.revision >= 0 && ch.revision < i32::MAX,
            "invalid resurrection revision"
        );
        let previous_items = ch.items.clone();
        ch.items[slot] = -1;
        let stats = ch.effective(&content)?;
        ch.hp = stats[0];
        ch.mp = stats[1];
        ch.alive = true;
        ensure!(ch.hp > 0, "resurrection has no living health capacity");
        self.call(move |db| {
            let tx = db.transaction()?;
            let (raw, items): (String, String) = tx.query_row(
                "SELECT stats,items FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![ch.id, ch.account, ch.revision], |r| Ok((r.get(0)?, r.get(1)?)))
                .context("resurrection conflict: dead, foreign or stale character")?;
            let saved: Character = serde_json::from_str(&raw)?;
            ensure!(saved.schema == ch.schema && saved.class == ch.class && saved.base == ch.base
                && saved.level == ch.level && saved.exp == ch.exp && saved.fame == ch.fame
                && saved.pirate_caves == ch.pirate_caves && saved.dungeons == ch.dungeons && saved.abysses == ch.abysses && saved.trenches == ch.trenches && saved.spider_dens == ch.spider_dens && saved.mad_labs == ch.mad_labs && saved.jungles == ch.jungles && saved.manors == ch.manors && saved.beaches == ch.beaches && saved.davy_lockers == ch.davy_lockers && saved.tombs == ch.tombs
                && serde_json::from_str::<Vec<i32>>(&items)? == previous_items,
                "resurrection reservation changed");
            ch.lifetime.follows(&saved.lifetime)?;
            ch.revision += 1;
            let n = tx.execute(
                "UPDATE characters SET stats=?4,items=?5,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![ch.id, ch.account, ch.revision - 1, serde_json::to_string(&ch)?, serde_json::to_string(&ch.items)?])?;
            ensure!(n == 1, "resurrection commit lost its reservation");
            tx.commit()?;
            Ok(ch)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resurrection_preserves_progress_rejects_replays_and_later_death_pays_once() {
        let content = Content::load(Path::new("data")).unwrap();
        let dir = std::env::temp_dir().join(format!("resurrection-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("game.db");
        let store = Store::open(&path, &content).unwrap();
        let account = store.list("amulet-owner".into()).await.unwrap().id;
        let other = store.list("amulet-other".into()).await.unwrap().id;
        let mut ch = store
            .create(account, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.level = 20;
        ch.exp = 18050;
        ch.fame = 18;
        store.progress(ch.clone()).await.unwrap();
        ch.items[3] = 0xb3e;
        ch.items[4] = 0xb3e;
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        ch.alive = false;
        ch.hp = 0;
        ch.mp = 0;
        ch.lifetime.shots = 3;
        let fatal = ch.clone();
        assert!(
            store
                .resurrect(ch.clone(), 4, content.clone())
                .await
                .is_err()
        );
        assert!(
            store
                .resurrect(ch.clone(), 0, content.clone())
                .await
                .is_err()
        );
        ch.account = other;
        assert!(store.resurrect(ch, 3, content.clone()).await.is_err());
        let restored = store
            .resurrect(fatal.clone(), 3, content.clone())
            .await
            .unwrap();
        let mut expected = fatal.items.clone();
        expected[3] = -1;
        assert!(restored.alive);
        assert_eq!(
            (restored.id, restored.level, restored.exp, restored.fame),
            (fatal.id, 20, 18050, 18)
        );
        assert_eq!(
            [restored.hp, restored.mp],
            restored.effective(&content).unwrap()[..2]
        );
        assert_eq!(restored.items, expected);
        assert_eq!(restored.lifetime, fatal.lifetime);
        assert_eq!(restored.revision, fatal.revision + 1);
        assert!(
            store
                .resurrect(fatal.clone(), 3, content.clone())
                .await
                .is_err()
        );
        assert!(
            store
                .death(fatal, "stale fatal snapshot".into())
                .await
                .is_err()
        );
        assert!(store.fame(account, restored.id).await.is_err());
        let wallet = store.presentation(account).await.unwrap();
        assert_eq!(wallet.fame, 0);
        drop(store);
        let store = Store::open(&path, &content).unwrap();
        let mut next = store.load(account, restored.id).await.unwrap();
        assert_eq!(
            (next.hp, next.mp, &next.items, next.revision),
            (restored.hp, restored.mp, &expected, restored.revision)
        );
        assert_eq!(store.presentation(account).await.unwrap(), wallet);
        next.alive = false;
        next.hp = 0;
        assert!(
            store
                .resurrect(next.clone(), 3, content.clone())
                .await
                .is_err()
        );
        assert!(
            store
                .resurrect(next.clone(), 4, content.clone())
                .await
                .is_err()
        );
        store
            .death(next.clone(), "second fatal hit".into())
            .await
            .unwrap();
        let paid = store.presentation(account).await.unwrap();
        assert!(paid.fame > 0);
        let archive = store.fame(account, next.id).await.unwrap();
        assert!(
            store
                .death(next.clone(), "replayed hit".into())
                .await
                .is_err()
        );
        assert_eq!(store.presentation(account).await.unwrap(), paid);
        assert_eq!(store.fame(account, next.id).await.unwrap(), archive);
        assert!(store.load(account, next.id).await.is_err());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
