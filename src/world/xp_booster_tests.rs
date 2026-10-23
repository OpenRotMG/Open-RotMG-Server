use super::{tests, *};
use crate::store::{InventoryWrite, Store};

#[tokio::test]
async fn all_death_fame_phase1_causal_levels_settled_inventory_boost() {
    for fail in [false, true] {
        let mut w = tests::world();
        let dir = std::env::temp_dir().join(format!("fame-boost-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(&dir.join("game.db"), &w.content).unwrap();
        let owner = store.list("fame-boost".into()).await.unwrap();
        let mut ch = store
            .create(owner.id, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.items[4] = 0xc42;
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        ch.exp = 45;
        store.progress(ch.clone()).await.unwrap();
        let id = w.enter(ch.clone(), None).unwrap();
        let pos = w.players[&id].pos;
        let plan = w
            .use_item(
                id,
                Slot {
                    object: id,
                    slot: 4,
                    kind: 0xc42,
                },
                pos,
            )
            .unwrap()
            .unwrap();
        w.players
            .get_mut(&id)
            .unwrap()
            .queue_experience(3, Some(99), true)
            .unwrap();
        assert!(w.pending_progress().is_empty());
        let q = w.players[&id].experience_queue.as_ref().unwrap();
        assert!(q.assists[0].is_empty());
        assert_eq!(
            q.assists[1],
            [LevelAssist {
                reached_level: 2,
                giver: 99
            }]
        );
        let result = store
            .consume_xp_booster(
                InventoryWrite {
                    id: ch.id,
                    account: ch.account,
                    revision: plan.revision + i32::from(fail),
                    items: plan.items.clone(),
                },
                Some(4),
                plan.vaults.clone(),
            )
            .await;
        assert_eq!(result.is_err(), fail);
        assert_eq!(w.finish_inventory(plan, result).is_err(), fail);
        if fail {
            assert!(w.pending_progress().is_empty());
            assert!(w.players[&id].persistence_failure.is_some());
            assert_eq!(store.load(ch.account, ch.id).await.unwrap().exp, 45);
            assert!(!store.load(ch.account, ch.id).await.unwrap().xp_boost.active);
        } else {
            let plans = w.pending_progress();
            assert_eq!(
                (
                    plans[0].1.level,
                    plans[0].1.exp,
                    plans[0].1.xp_boost.bonus_exp
                ),
                (2, 51, 3)
            );
            assert_eq!(
                w.players[&id].level_assists_prepared,
                [LevelAssist {
                    reached_level: 2,
                    giver: 99
                }]
            );
            store.progress(plans[0].1.clone()).await.unwrap();
            w.finish_progress(id, plans[0].1.clone()).unwrap();
            assert!(w.players[&id].level_assists_prepared.is_empty());
        }
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
