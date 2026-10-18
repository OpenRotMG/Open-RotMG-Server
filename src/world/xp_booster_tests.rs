use super::{tests, *};
use crate::store::{InventoryWrite, Store};

#[tokio::test]
async fn xp_booster_sources_are_atomic_and_only_eligible_enemy_xp_is_doubled() {
    for source in 0..3 {
        for fail in [false, true] {
            let mut w = tests::world();
            let dir =
                std::env::temp_dir().join(format!("xp-booster-{:016x}", rand::random::<u64>()));
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("game.db");
            let store = Store::open(&path, &w.content).unwrap();
            let account = store.list("xp-source-owner".into()).await.unwrap().id;
            let mut ch = store
                .create(account, w.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            ch.items[4..].fill(0xa22);
            let vault = if source == 2 {
                let mut vault = store.vault(account, 0).await.unwrap();
                vault.items[0] = 0xc42;
                Some(vault)
            } else {
                if source == 0 {
                    ch.items[5] = 0xc42;
                }
                None
            };
            ch.revision = store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vault.into_iter().collect(),
                )
                .await
                .unwrap()[0];
            let before = ch.clone();
            let id = w.enter(ch, None).unwrap();
            let pos = w.players[&id].pos;
            let slot = if source == 0 {
                Slot {
                    object: id,
                    slot: 5,
                    kind: 0xc42,
                }
            } else {
                let mut items = vec![0xc42];
                items.resize(8, -1);
                let bag = w.spawn_entity(0x503, pos, items).unwrap();
                w.entities
                    .get_mut(&bag)
                    .unwrap()
                    .extra_stats
                    .push((54, account));
                if source == 2 {
                    w.entities.get_mut(&bag).unwrap().kind = 0x504;
                    w.owner = Some(account);
                    w.vaults.insert(bag, store.vault(account, 0).await.unwrap());
                }
                Slot {
                    object: bag,
                    slot: 0,
                    kind: 0xc42,
                }
            };
            let mut plan = w.use_item(id, slot, pos).unwrap().unwrap();
            assert!(!w.players[&id].character.xp_boost.active);
            if fail {
                if source == 2 {
                    plan.vaults[0].revision += 1;
                } else {
                    rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER fail_xp BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'injected XP booster save failure'); END;").unwrap();
                }
            }
            let result = store
                .consume_xp_booster(
                    InventoryWrite {
                        id: before.id,
                        account,
                        revision: plan.revision,
                        items: plan.items.clone(),
                    },
                    (source == 0).then_some(5),
                    plan.vaults.clone(),
                )
                .await;
            assert_eq!(result.is_err(), fail);
            assert_eq!(w.finish_inventory(plan, result).is_err(), fail);
            let saved = store.load(account, before.id).await.unwrap();
            assert_eq!(saved.xp_boost.active, !fail);
            assert_eq!(saved.revision, before.revision + i32::from(!fail));
            if fail {
                assert_eq!(saved.items, before.items);
                assert!(!w.players[&id].character.xp_boost.active);
                assert!(w.players[&id].persistence_failure.is_some());
            } else {
                assert!(w.players[&id].character.xp_boost.active);
                if source == 0 {
                    assert_eq!(saved.items[5], -1);
                } else {
                    assert_eq!(saved.items, before.items);
                }
                // XP witnesses belong in a shared world, not the source account's private Vault.
                let mut reward_world = tests::world();
                let id = reward_world
                    .enter(w.players[&id].character.clone(), None)
                    .unwrap();
                let w = &mut reward_world;
                let pos = w.players[&id].pos;
                let mut other = tests::ch(w);
                other.account = account + 1;
                other.id = before.id + 1;
                let witness = w
                    .enter_at(
                        other,
                        None,
                        Some(pos),
                        AccountPresentation {
                            guild: None,
                            credits: 0,
                            fame: 0,
                            best_fame: BTreeMap::new(),
                        },
                    )
                    .unwrap();
                let excluded = w.spawn_entity(0x600, pos, vec![]).unwrap();
                w.entities.get_mut(&excluded).unwrap().script = Some(landmark_ai::Script {
                    no_experience: true,
                    ..Default::default()
                });
                w.defeat_entity(excluded).unwrap();
                assert!(w.pending_progress().is_empty());
                let enemy = w.spawn_entity(0x600, pos, vec![]).unwrap();
                w.defeat_entity(enemy).unwrap();
                let rewards: BTreeMap<_, _> = w.pending_progress().into_iter().collect();
                assert!(rewards[&witness].exp > 0);
                assert_eq!(rewards[&id].exp, 2 * rewards[&witness].exp);
                assert_eq!(rewards[&id].fame, rewards[&witness].fame);
                assert_eq!(rewards[&id].xp_boost.bonus_exp, rewards[&witness].exp);
                store.progress(rewards[&id].clone()).await.unwrap();
                w.finish_progress(id, rewards[&id].clone()).unwrap();
            }
            if source != 0 {
                assert_eq!(
                    w.entities[&slot.object].items[0],
                    if fail { 0xc42 } else { -1 }
                );
            }
            if source == 2 {
                assert_eq!(
                    store.vault(account, 0).await.unwrap().items[0],
                    if fail { 0xc42 } else { -1 }
                );
            }
            drop(store);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

#[test]
fn xp_booster_atomic_level_twenty_crossing_preserves_fame_and_rejects_corruption() {
    let mut w = tests::world();
    let mut boosted = tests::ch(&w);
    boosted.level = 19;
    boosted.exp = crate::store::level_start(20).unwrap() - 3;
    boosted.fame = boosted.exp / 1000;
    boosted.xp_boost.active = true;
    let mut ordinary = boosted.clone();
    ordinary.xp_boost.active = false;
    for amount in [100, 752, 1, 1000] {
        boosted
            .gain_experience(&w.content, amount, &mut rand::thread_rng())
            .unwrap();
        ordinary
            .gain_experience(&w.content, amount, &mut rand::thread_rng())
            .unwrap();
        assert_eq!(boosted.level, 20);
        assert!(!boosted.xp_boost.active);
        assert_eq!(boosted.exp, ordinary.exp + 100);
        assert_eq!(boosted.xp_boost.bonus_exp, 100);
        assert_eq!(boosted.fame, ordinary.fame);
    }
    for bonus in [-1, boosted.exp + 1] {
        let mut invalid = boosted.clone();
        invalid.xp_boost.bonus_exp = bonus;
        let before = serde_json::to_value(&invalid).unwrap();
        assert!(
            invalid
                .gain_experience(&w.content, 1, &mut rand::thread_rng())
                .is_err()
        );
        assert_eq!(serde_json::to_value(invalid).unwrap(), before);
    }
    let before = serde_json::to_value(&boosted).unwrap();
    for amount in [-1, i32::MAX] {
        assert!(
            boosted
                .gain_experience(&w.content, amount, &mut rand::thread_rng())
                .is_err()
        );
        assert_eq!(serde_json::to_value(&boosted).unwrap(), before);
    }
    let mut legacy = serde_json::to_value(tests::ch(&w)).unwrap();
    legacy.as_object_mut().unwrap().remove("xp_boost");
    let decoded: Character = serde_json::from_value(legacy.clone()).unwrap();
    assert!(!decoded.xp_boost.active);
    assert_eq!(decoded.xp_boost.bonus_exp, 0);
    for malformed in [
        serde_json::Value::Null,
        serde_json::json!({"active": false}),
        serde_json::json!({"bonus_exp": 0}),
    ] {
        legacy["xp_boost"] = malformed;
        assert!(serde_json::from_value::<Character>(legacy.clone()).is_err());
    }
    boosted.items[5] = 0xc42;
    let id = w.enter(boosted, None).unwrap();
    let before = serde_json::to_value(&w.players[&id].character).unwrap();
    assert!(
        w.use_item(
            id,
            Slot {
                object: id,
                slot: 5,
                kind: 0xc42
            },
            w.players[&id].pos
        )
        .err()
        .unwrap()
        .to_string()
        .contains("below level 20")
    );
    assert_eq!(
        serde_json::to_value(&w.players[&id].character).unwrap(),
        before
    );
    assert!(!w.players[&id].pending);
}
