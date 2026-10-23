use super::tests::content;
use super::*;

#[tokio::test]
async fn community_pursuit_pending_and_failed_results_do_not_complete_the_activity() {
    use crate::content::WIZARD;
    use crate::store::{InventoryWrite, Store};
    for lost_publication in [false, true] {
        let c = Arc::new(content());
        let dir = std::env::temp_dir().join(format!("community-pending-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let store = Store::open(&dir.join("game.db"), &c).unwrap();
        let account = store.list("community-pending".into()).await.unwrap();
        let mut ch = store
            .create(account.id, c.get(WIZARD).unwrap())
            .await
            .unwrap();
        ch.items[4..].fill(-1);
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
        let character = ch.id;
        let mut d = crate::world::Directory::test_with_seed(
            std::path::Path::new("data"),
            c.clone(),
            16,
            42,
        )
        .unwrap();
        let map = Arc::new(crate::content::Map {
            width: 32,
            height: 32,
            cells: vec![
                crate::content::Cell {
                    ground: 0x61,
                    object: None,
                    config: String::new(),
                    terrain: 0,
                    region: 0
                };
                32 * 32
            ],
            spawns: vec![(10.5, 10.5)],
        });
        let mut w = crate::world::World::new(c.clone(), map).unwrap();
        w.name = "Realm".into();
        w.spawn_entity(
            0x500,
            Pos { x: 10.5, y: 11.5 },
            vec![0xa22, 0xa22, -1, -1, -1, -1, -1, -1],
        )
        .unwrap();
        let (packets, _receiver) = tokio::sync::mpsc::channel(OUTPUT_CAPACITY);
        let (lost, _) = tokio::sync::watch::channel(false);
        let player = w
            .enter_autonomous(ch, crate::world::Output { packets, lost })
            .unwrap();
        d.worlds.insert(-3, w);
        let v = d.player_view(player).unwrap();
        let mut brain = Brain::new(PROFILES[1], &v);
        let decision = brain.decide(&v, &c).unwrap();
        let plan = d
            .worlds
            .get_mut(&-3)
            .unwrap()
            .autonomous_action(player, decision.item.unwrap())
            .unwrap()
            .unwrap();
        d.worlds.get_mut(&-3).unwrap().step();
        let pending = d.player_view(player).unwrap();
        assert!(pending.lifecycle.busy);
        assert_eq!(
            pending
                .character
                .items
                .iter()
                .filter(|k| **k == 0xa22)
                .count(),
            0
        );
        brain.decide(&pending, &c).unwrap();
        let revision = store
            .exchange(
                vec![InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                }],
                plan.vaults.clone(),
            )
            .await
            .unwrap()[0];
        d.worlds
            .get_mut(&-3)
            .unwrap()
            .finish_inventory(plan, Ok(revision))
            .unwrap();
        for _ in 0..4 {
            d.worlds.get_mut(&-3).unwrap().step();
        }
        let v = d.player_view(player).unwrap();
        let decision = brain.decide(&v, &c).unwrap();
        assert_eq!(brain.mind.history.back().unwrap().carried, 1);
        let plan = d
            .worlds
            .get_mut(&-3)
            .unwrap()
            .autonomous_action(player, decision.item.unwrap())
            .unwrap()
            .unwrap();
        brain.mind.revision = store.save_bot_mind(brain.mind.clone()).await.unwrap();
        let revision = store
            .exchange(
                vec![InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                }],
                plan.vaults.clone(),
            )
            .await
            .unwrap()[0];
        if lost_publication {
            assert!(
                d.worlds
                    .get_mut(&-3)
                    .unwrap()
                    .finish_inventory(
                        plan,
                        Err(anyhow::anyhow!(
                            "offline fixture: committed result lost before World publication"
                        ))
                    )
                    .is_err()
            );
        } else {
            d.worlds
                .get_mut(&-3)
                .unwrap()
                .finish_inventory(plan, Ok(revision))
                .unwrap();
            let reserved = d
                .worlds
                .get_mut(&-3)
                .unwrap()
                .drop_item(
                    player,
                    Slot {
                        object: player,
                        slot: 4,
                        kind: 0xa22,
                    },
                )
                .unwrap();
            for _ in 0..4 {
                d.worlds.get_mut(&-3).unwrap().step();
            }
            let pending = d.player_view(player).unwrap();
            assert!(pending.lifecycle.busy);
            assert_eq!(
                pending
                    .character
                    .items
                    .iter()
                    .filter(|k| **k == 0xa22)
                    .count(),
                2
            );
            assert_eq!(reserved.items.iter().filter(|k| **k == 0xa22).count(), 1);
            brain.decide(&pending, &c).unwrap();
            std::fs::write(
                dir.join("pending-view.json"),
                serde_json::to_vec_pretty(&pending).unwrap(),
            )
            .unwrap();
            std::fs::write(
                dir.join("pending-mind.json"),
                serde_json::to_vec_pretty(&brain.mind).unwrap(),
            )
            .unwrap();
            println!("community pending-result evidence: {}", dir.display());
            assert!(
                !brain
                    .mind
                    .history
                    .iter()
                    .any(|h| h.reason == "reserve_carried_satisfied"),
                "busy reservation completed the project before its result settled"
            );
            let error = store
                .exchange(
                    vec![InventoryWrite {
                        id: reserved.character,
                        account: reserved.account,
                        revision: reserved.revision - 1,
                        items: reserved.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap_err();
            std::fs::write(dir.join("failed-store-result.txt"), format!("{error:#}")).unwrap();
            assert!(
                d.worlds
                    .get_mut(&-3)
                    .unwrap()
                    .finish_inventory(reserved, Err(error))
                    .is_err()
            );
        }
        let failed = d.player_view(player).unwrap();
        assert!(failed.lifecycle.quarantined && failed.lifecycle.busy);
        brain.mind.reconcile(&failed).unwrap();
        let prior = brain.mind.clone();
        for _ in 0..2 {
            assert!(
                !brain
                    .receive(
                        Packet {
                            id: 4,
                            body: vec![0, 0, 0, 0]
                        },
                        &failed,
                        false
                    )
                    .unwrap()
            );
        }
        assert_eq!(brain.mind, prior);
        assert!(
            !brain
                .mind
                .history
                .iter()
                .any(|h| h.reason == "reserve_carried_satisfied")
        );
        assert_eq!(
            serde_json::to_value(&brain.mind).unwrap()["activity"]["carried"],
            1
        );
        let reopened = Store::open(&dir.join("game.db"), &c).unwrap();
        let committed = reopened.load(account.id, character).await.unwrap();
        assert_eq!(committed.items.iter().filter(|k| **k == 0xa22).count(), 2);
        assert_eq!(committed.revision, revision);
        assert_eq!(
            failed
                .character
                .items
                .iter()
                .filter(|k| **k == 0xa22)
                .count(),
            if lost_publication { 1 } else { 2 }
        );
        assert_eq!(
            failed.character.inventory_revision,
            if lost_publication {
                revision - 1
            } else {
                revision
            }
        );
        std::fs::write(
            dir.join("failed-view.json"),
            serde_json::to_vec_pretty(&failed).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("committed-character.json"),
            serde_json::to_vec_pretty(&committed).unwrap(),
        )
        .unwrap();
        let map = d.worlds[&-3].map.clone();
        drop(d);
        let mut d = crate::world::Directory::test_with_seed(
            std::path::Path::new("data"),
            c.clone(),
            16,
            101,
        )
        .unwrap();
        let mut w = crate::world::World::new(c.clone(), map).unwrap();
        w.name = "Realm".into();
        let (packets, _fresh_receiver) = tokio::sync::mpsc::channel(OUTPUT_CAPACITY);
        let (lost, _) = tokio::sync::watch::channel(false);
        let player = w
            .enter_autonomous(committed, crate::world::Output { packets, lost })
            .unwrap();
        d.worlds.insert(-3, w);
        let view = d.player_view(player).unwrap();
        let mut resumed = Brain::new(PROFILES[1], &view);
        resumed.mind = reopened.bot_mind(account.id).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&resumed.mind).unwrap()["activity"]["carried"],
            1
        );
        resumed.mind.bind(&view, 1).unwrap();
        assert!(
            resumed.destination.is_none()
                && resumed.tracks.is_empty()
                && resumed.last_tick.is_none()
        );
        assert!(serde_json::to_value(&resumed.mind).unwrap()["activity"].is_null());
        assert_eq!(
            resumed
                .mind
                .history
                .iter()
                .filter(|h| h.activity == 1
                    && h.reason == "reserve_carried_satisfied"
                    && h.carried == 2)
                .count(),
            1
        );
        resumed.mind.revision = reopened.save_bot_mind(resumed.mind.clone()).await.unwrap();
        let settled = resumed.mind.clone();
        for _ in 0..2 {
            assert!(
                !resumed
                    .receive(
                        Packet {
                            id: 4,
                            body: vec![0, 0, 0, 0]
                        },
                        &view,
                        false
                    )
                    .unwrap()
            );
        }
        assert_eq!(resumed.mind, settled);
        assert_eq!(
            reopened.load(account.id, character).await.unwrap().items,
            view.character.items
        );
        std::fs::write(
            dir.join("reconciled-mind.json"),
            serde_json::to_vec_pretty(&resumed.mind).unwrap(),
        )
        .unwrap();
        println!(
            "community failed-result reconciliation lost_publication={lost_publication}: {}",
            dir.display()
        );
    }
}
