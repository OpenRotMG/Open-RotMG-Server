use super::*;

#[tokio::test]
async fn every_selected_class_requires_all_lifetime_level_records_after_restart() {
    let content = Content::load(Path::new("data")).unwrap();
    let dir = std::env::temp_dir().join(format!("class-unlocks-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let tree = [
        (0x300, vec![(0x307, 5)]),
        (0x307, vec![(0x310, 5)]),
        (0x30e, vec![]),
        (0x310, vec![(0x30e, 5)]),
        (0x31d, vec![(0x300, 5)]),
        (0x31e, vec![(0x31d, 20)]),
        (0x31f, vec![(0x31e, 20), (0x310, 20)]),
        (0x320, vec![(0x300, 20), (0x30e, 20)]),
        (0x321, vec![(0x30e, 20), (0x310, 20)]),
        (0x322, vec![(0x300, 20), (0x307, 20)]),
        (0x323, vec![(0x322, 20), (0x321, 20)]),
        (0x324, vec![(0x320, 20), (0x31f, 20)]),
        (0x325, vec![(0x321, 20), (0x320, 20)]),
    ];
    assert_eq!(content.classes.len(), tree.len());
    for (kind, requires) in tree {
        assert_eq!(content.classes[&kind].requires, requires);
        for short in 0..=requires.len() {
            let guid = format!("class-{kind}-{short}");
            let account = store.list(guid.clone()).await.unwrap();
            assert!(account.best_levels.is_empty());
            assert_eq!(account.character_slots, 1);
            assert_eq!(
                content.unlocked(kind, &account.best_levels).unwrap(),
                requires.is_empty()
            );
            let mut retired = Vec::new();
            if !requires.is_empty() {
                // Selected isAvailable checks every prerequisite even with target ClassStats: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
                let prior = store
                    .create_prepared(account.id, content.get(kind).unwrap(), None)
                    .await
                    .unwrap();
                store.delete_character(account.id, prior.id).await.unwrap();
                retired.push(prior.id);
                assert_eq!(
                    store.list(guid.clone()).await.unwrap().best_levels[&kind],
                    1
                );
                assert_eq!(
                    store
                        .create(account.id, content.get(kind).unwrap())
                        .await
                        .unwrap_err()
                        .downcast_ref::<Rejected>()
                        .unwrap()
                        .0,
                    "class is locked"
                );
            }
            for (index, &(parent, level)) in requires.iter().enumerate() {
                let mut ch = store
                    .create_prepared(account.id, content.get(parent).unwrap(), None)
                    .await
                    .unwrap();
                ch.gain_experience(
                    &content,
                    level_start(level - i32::from(index == short)).unwrap(),
                    &mut rand::thread_rng(),
                )
                .unwrap();
                store.progress(ch.clone()).await.unwrap();
                assert_eq!(
                    Store::open(&path, &content)
                        .unwrap()
                        .list(guid.clone())
                        .await
                        .unwrap()
                        .best_levels[&parent],
                    ch.level
                );
                if index % 2 == 0 {
                    store.delete_character(account.id, ch.id).await.unwrap();
                } else {
                    ch.hp = 0;
                    ch.alive = false;
                    store
                        .death(ch.clone(), "unlock fixture".into())
                        .await
                        .unwrap();
                }
                retired.push(ch.id);
            }
            let reopened = Store::open(&path, &content).unwrap();
            let listed = reopened.list(guid.clone()).await.unwrap();
            assert!(listed.characters.is_empty());
            for id in retired {
                assert!(reopened.load(account.id, id).await.is_err());
            }
            let eligible = short == requires.len();
            assert_eq!(
                content.unlocked(kind, &listed.best_levels).unwrap(),
                eligible
            );
            assert!(listed.xml("", 0.0).contains("<BestLevel>") || requires.is_empty());
            let result = reopened
                .create(account.id, content.get(kind).unwrap())
                .await;
            if !eligible {
                assert_eq!(
                    result.unwrap_err().downcast_ref::<Rejected>().unwrap().0,
                    "class is locked"
                );
                assert!(reopened.list(guid).await.unwrap().characters.is_empty());
                continue;
            }
            let ch = result.unwrap();
            let fresh = Character::fresh(content.get(kind).unwrap()).unwrap();
            assert_eq!(
                (ch.level, ch.exp, ch.base, &ch.items),
                (1, 0, fresh.base, &fresh.items)
            );
            assert_eq!(
                reopened
                    .create(account.id, content.get(kind).unwrap())
                    .await
                    .unwrap_err()
                    .downcast_ref::<Rejected>()
                    .unwrap()
                    .0,
                "No free character slots."
            );
            assert!(reopened.load(account.id + 10000, ch.id).await.is_err());
            let cold = Store::open(&path, &content).unwrap();
            assert_eq!(
                cold.load(account.id, ch.id).await.unwrap().items,
                fresh.items
            );
        }
    }
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
