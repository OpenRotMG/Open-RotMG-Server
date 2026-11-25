use super::*;
use rand::RngCore;

fn content() -> Arc<Content> {
    let mut c = Content::load(Path::new("data")).unwrap();
    let mut rules = authored(
        "Pirate",
        0x600,
        "<Behavior>RemainVisible</Behavior><SoulboundLoot prob=\"1\">Health Potion</SoulboundLoot>",
    );
    for (kind, service) in [
        (6, "projectile"),
        (0x30e, "player"),
        (0xa22, "inventory"),
        (0xb16, "inventory"),
        (0x500, "container"),
        (0x503, "container"),
        (0x507, "container"),
        (0x508, "container"),
        (0x509, "container"),
    ] {
        rules += &format!(
            r#"<Object type="{kind}" id="{}" disposition="native" nativeService="{service}" provenance="authored" reason="selected review counterexample dependency" origin="independent regression"/>"#,
            c.objects[&kind].name
        );
    }
    let inputs = BTreeMap::from([
        (
            "game/123.5.1/game.xml".into(),
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/></Game>"#
                .to_vec(),
        ),
        (
            "game/123.5.1/rules.xml".into(),
            format!("<Objects>{rules}</Objects>").into_bytes(),
        ),
        (
            "objects.xml".into(),
            std::fs::read("data/objects.xml").unwrap(),
        ),
        (
            "grounds.xml".into(),
            std::fs::read("data/grounds.xml").unwrap(),
        ),
    ]);
    c.game = Some(Arc::new(game::compile_fixture(&c, inputs).unwrap()));
    Arc::new(c)
}
pub(super) async fn setup(name: &str) -> (crate::store::Store, Directory, i32, i32) {
    let c = content();
    let path =
        Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join(format!("review-{name}.db"));
    let store = crate::store::Store::open(&path, &c).unwrap();
    let account = store.list(format!("review-{name}")).await.unwrap();
    let mut ch = store
        .create(account.id, c.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.items[4] = 0xb16;
    let revisions = store
        .exchange(
            vec![crate::store::InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap();
    ch.revision = revisions[0];
    let ch = store.load(ch.account, ch.id).await.unwrap();
    let mut d = admission_tests::isolated_directory(c.clone());
    let mut w = World::in_domain(c, Arc::new(flat_map(128)), WorldDomain::NonRealm).unwrap();
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 65.5, y: 64.5 };
    d.worlds.insert(17, w);
    (store, d, 17, p)
}
async fn reward(
    store: &crate::store::Store,
    d: &mut Directory,
    origin: i32,
    p: i32,
) -> (crate::store::world_events::Event, i32) {
    let w = d.worlds.get_mut(&origin).unwrap();
    let actor = spawn(w, 0x600, 65.5, 64.5);
    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
    let event = w.pending_terminal_events()[0].clone();
    let record = store.coordinate_terminal(event.clone()).await.unwrap();
    w.finish_terminal_event(record).unwrap();
    store.publish_world_event(event.clone()).await.unwrap();
    w.acknowledge_terminal_publication(event.id.clone());
    w.publish_entitlements(
        event.clone(),
        store.unpicked_entitlements(event.clone()).await.unwrap(),
    )
    .unwrap();
    let bag = *w.bag_expiry.keys().next().unwrap();
    (event, bag)
}
fn replacement(d: &mut Directory, origin: i32, p: i32, bag: i32) -> InventoryPlan {
    d.worlds
        .get_mut(&origin)
        .unwrap()
        .swap(
            p,
            Slot {
                object: bag,
                slot: 0,
                kind: 0xa22,
            },
            Slot {
                object: p,
                slot: 4,
                kind: 0xb16,
            },
        )
        .unwrap()
}
#[tokio::test]
async fn xml_rewards_portable_refuses_unsettled_terminals_with_or_without_loot() {
    for loot in [true, false] {
        let (store, mut d, origin, p) = setup(if loot { "a4" } else { "a4-progress" }).await;
        let w = d.worlds.get_mut(&origin).unwrap();
        let actor = spawn(w, 0x600, 65.5, 64.5);
        w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
        let mut event = w.pending_terminal_events()[0].clone();
        if !loot {
            let mut payload = event.terminal_payload().unwrap().clone();
            payload.allocation = None;
            payload.actions["loot"] = serde_json::json!(false);
            event = crate::store::world_events::Event::terminal(&w.xml_game().fingerprint, payload)
                .unwrap();
        }
        let account = w.players[&p].character.account;
        store.prepare_world_event(event.clone()).await.unwrap();
        assert!(
            store.export_account_save(account).await.is_err(),
            "PREPARED terminal custody must refuse portable export"
        );
        store.commit_terminal_event(event.clone()).await.unwrap();
        assert!(store.export_account_save(account).await.is_err());
        if loot {
            store.reconcile_lost_world_events(vec![]).await.unwrap();
        } else {
            store.publish_world_event(event).await.unwrap();
        }
        assert!(store.export_account_save(account).await.is_ok());
    }
}

#[tokio::test]
async fn xml_rewards_review_a3_replacement_pickup_claims_earned_item_not_displaced_item() {
    let (store, mut d, origin, p) = setup("a3").await;
    let (event, bag) = reward(&store, &mut d, origin, p).await;
    let stale = d.worlds[&origin].delivery_batch().pop().unwrap();
    let stale_rows = store.unpicked_entitlements(event.clone()).await.unwrap();
    assert_eq!(stale_rows.len(), 1);
    let plan = replacement(&mut d, origin, p, bag);
    let dropped = plan.dropped.unwrap();
    let write = crate::store::InventoryWrite {
        id: plan.character,
        account: plan.account,
        revision: plan.revision,
        items: plan.items.clone(),
    };
    let pickup = d.retain_inventory_event(plan).unwrap();
    store.prepare_world_event(pickup.clone()).await.unwrap();
    let receipt = store
        .commit_inventory_event(pickup.clone(), write)
        .await
        .unwrap();
    assert!(
        store
            .unpicked_entitlements(event.clone())
            .await
            .unwrap()
            .is_empty(),
        "earned item entered inventory but is still unpicked"
    );
    let db = rusqlite::Connection::open(
        Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("review-a3.db"),
    )
    .unwrap();
    let picked: (i32, String) = db
        .query_row(
            "SELECT status,pickup_operation FROM world_entitlements WHERE event=?1 AND ordinal=0",
            [&event.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(picked, (1, pickup.id.clone()));
    drop(db);
    d.publish_inventory_event(&pickup, receipt.result.as_ref().unwrap())
        .unwrap();
    d.publish_inventory_event(&pickup, receipt.result.as_ref().unwrap())
        .unwrap();
    let w = d.worlds.get_mut(&origin).unwrap();
    let bags = w.bag_expiry.clone();
    w.publish_delivery(stale, stale_rows).unwrap();
    assert_eq!(
        w.bag_expiry, bags,
        "stale delivery re-created a claimed reward"
    );
    assert_eq!(w.players[&p].character.items[4], 0xa22);
    assert_eq!(w.entities[&dropped].items[0], 0xb16);
    assert!(!w.xml.bag_entitlements.contains_key(&(dropped, 0)));
    assert_eq!(
        store
            .world_event(pickup.id.clone())
            .await
            .unwrap()
            .unwrap()
            .event,
        pickup
    );
    store.publish_world_event(pickup).await.unwrap();
    store.validate_world_events().await.unwrap();
    let w = d.worlds.get_mut(&origin).unwrap();
    let (account, character) = (w.players[&p].character.account, w.players[&p].character.id);
    w.now = 60_000;
    w.remove_entity(dropped);
    assert!(w.entitlement_expiries(&event).is_empty());
    drop(store);
    let reopened = crate::store::Store::open(
        &Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("review-a3.db"),
        &d.content,
    )
    .unwrap();
    assert_eq!(
        reopened.load(account, character).await.unwrap().items[4],
        0xa22
    );
    assert!(
        reopened
            .unpicked_entitlements(event)
            .await
            .unwrap()
            .is_empty()
    );
    reopened.validate_world_events().await.unwrap();
}
#[tokio::test]
async fn xml_rewards_review_a5_replacement_backpressure_precedes_reserved_bag_and_ids() {
    let (store, mut d, origin, p) = setup("a5").await;
    let (_, bag) = reward(&store, &mut d, origin, p).await;
    let w = d.worlds.get_mut(&origin).unwrap();
    let held = (0..128)
        .map(|_| {
            w.plan(p, w.players[&p].character.items.clone(), None)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let before = (
        w.entities.len(),
        w.ids.load(Ordering::Relaxed),
        w.bag_expiry.clone(),
        w.rng.clone().next_u64(),
        w.players[&p].character.clone(),
    );
    assert!(
        w.swap(
            p,
            Slot {
                object: bag,
                slot: 0,
                kind: 0xa22
            },
            Slot {
                object: p,
                slot: 4,
                kind: 0xb16
            }
        )
        .is_err()
    );
    assert_eq!(
        w.entities.len(),
        before.0,
        "failed admission leaked an unowned reserved bag"
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), before.1);
    assert_eq!(w.bag_expiry, before.2);
    assert_eq!(w.rng.clone().next_u64(), before.3);
    assert_eq!(w.players[&p].character, before.4);
    assert!(!w.entities[&bag].reserved);
    drop(held);
    let plan = replacement(&mut d, origin, p, bag);
    assert!(plan.dropped.is_some());
}
#[tokio::test]
async fn xml_rewards_review_a2_busy_write_retains_first_defeat_facts_seed_and_global_rng() {
    for damage in [true, false] {
        let (store, mut d, origin, p) =
            setup(if damage { "a2-damage" } else { "a2-lifecycle" }).await;
        let plan = {
            let w = d.worlds.get_mut(&origin).unwrap();
            let plan = w
                .plan(p, w.players[&p].character.items.clone(), None)
                .unwrap();
            w.players.get_mut(&p).unwrap().pending = true;
            plan
        };
        let write = crate::store::InventoryWrite {
            id: plan.character,
            account: plan.account,
            revision: plan.revision,
            items: plan.items.clone(),
        };
        let inventory = d.retain_inventory_event(plan).unwrap();
        store.prepare_world_event(inventory.clone()).await.unwrap();
        let (seed, original, character, mut expected, actor) = {
            let w = d.worlds.get_mut(&origin).unwrap();
            let actor = spawn(w, 0x600, 65.5, 64.5);
            let hp = w.entities[&actor].hp;
            let original = w.players[&p].pos;
            let character = w.players[&p].character.id;
            let mut expected = w.rng.clone();
            let seed = expected.next_u64();
            if damage {
                assert_eq!(
                    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap(),
                    hp
                );
            } else {
                w.xml_defeated(actor, TerminalContext::NonPlayer).unwrap();
            }
            assert!(
                w.xml.facts.contains_key(&actor),
                "applied terminal has no immutable native defeat capture"
            );
            assert_eq!(w.xml.facts[&actor].seed, seed);
            assert_eq!(w.rng.clone().next_u64(), expected.clone().next_u64());
            let player = w.players.get_mut(&p).unwrap();
            player.pos = Pos { x: 100., y: 100. };
            player.departing = true;
            player.death_pending = true;
            for _ in 0..7 {
                w.rng.next_u64();
                expected.next_u64();
            }
            (seed, original, character, expected, actor)
        };
        let receipt = store
            .commit_inventory_event(inventory.clone(), write)
            .await
            .unwrap();
        d.publish_inventory_event(&inventory, receipt.result.as_ref().unwrap())
            .unwrap();
        let w = d.worlds.get_mut(&origin).unwrap();
        w.now += 50;
        w.xml_step().unwrap();
        let event = w.pending_terminal_events().into_iter().next().unwrap();
        let payload = event.terminal_payload().unwrap();
        assert_eq!(payload.seed, seed);
        assert_eq!(w.rng.clone().next_u64(), expected.next_u64());
        let recipients = &payload.actions["publication"]["defeat_recipients"];
        assert_eq!(recipients[0]["character"], character);
        assert_eq!(
            recipients[0]["position"],
            serde_json::to_value(original).unwrap()
        );
        assert_eq!(payload.actions["progress"].as_array().unwrap().len(), 1);
        if damage {
            assert!(payload.ledger[0].alive);
            assert!(!payload.ledger[0].departing);
            assert_eq!(
                payload.ledger[0].effective_damage,
                w.xml.facts[&actor].ledger[0].effective_damage
            );
        }
        let receipt = store.coordinate_terminal(event.clone()).await.unwrap();
        w.finish_terminal_event(receipt).unwrap();
        assert_eq!(w.players[&p].terminal_captures, 0);
        assert!(!w.entities.contains_key(&actor));
        assert!(
            store
                .load(w.players[&p].character.account, character)
                .await
                .unwrap()
                .exp
                > 0
        );
    }
}
#[tokio::test]
async fn xml_rewards_review_a1_stale_census_does_not_abort_new_live_prepared_or_committed_inventory()
 {
    for committed in [false, true] {
        let (store, mut d, origin, old_player) =
            setup(if committed { "a1-commit" } else { "a1-prepare" }).await;
        let old = d.worlds.remove(&origin).unwrap();
        let ch = old.players[&old_player].character.clone();
        let map = old.map.clone();
        drop(old);
        d.clock.set_paused(true);
        let handle = Handle::spawn(d);
        let census = handle
            .directory(|d| {
                Ok(d.worlds
                    .summaries()
                    .map(|(_, w)| w.instance.clone())
                    .collect::<Vec<_>>())
            })
            .await
            .unwrap();
        assert!(census.is_empty());
        let (gate, blocked) = tokio::sync::oneshot::channel();
        let pending_store = store.clone();
        let pending_handle = handle.clone();
        let reconciliation = tokio::spawn(async move {
            blocked.await.unwrap();
            crate::server::reconcile_world_origins(&pending_store, &pending_handle, census).await
        });
        let (event, write, p) = handle
            .directory(move |d| {
                let mut w = World::with_ids_context(
                    d.content.clone(),
                    map,
                    d.ids.clone(),
                    Some(WorldDomain::NonRealm),
                )?;
                let p = w.enter(ch, None)?;
                let mut items = w.players[&p].character.items.clone();
                items[5] = 0xa22;
                let plan = w.plan(p, items, None)?;
                w.players.get_mut(&p).unwrap().pending = true;
                let write = crate::store::InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                };
                d.worlds.insert(origin, w);
                let event = d.retain_inventory_event(plan)?;
                Ok((event, write, p))
            })
            .await
            .unwrap();
        store.prepare_world_event(event.clone()).await.unwrap();
        if committed {
            store
                .commit_inventory_event(event.clone(), write.clone())
                .await
                .unwrap();
        }
        gate.send(()).unwrap();
        let reconciled = reconciliation.await.unwrap().unwrap();
        assert!(
            reconciled.is_empty(),
            "old census falsely reconciled a currently live origin"
        );
        assert_eq!(
            store
                .world_event(event.id.clone())
                .await
                .unwrap()
                .unwrap()
                .status,
            if committed { 1 } else { 0 }
        );
        handle
            .directory(move |d| {
                d.retire_lost_inventory_events(reconciled.into_iter().map(|(id, _)| id).collect())
            })
            .await
            .unwrap();
        let receipt = store
            .commit_inventory_event(event.clone(), write)
            .await
            .unwrap();
        let result = receipt.result.unwrap();
        handle
            .directory(move |d| {
                d.publish_inventory_event(&event, &result)?;
                assert_eq!(d.worlds[&origin].players[&p].character.items[5], 0xa22);
                Ok(())
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn xml_rewards_review_a3_genuine_ground_move_transfers_same_unpicked_item_then_expires_once()
{
    let (store, mut d, origin, p) = setup("a3-ground").await;
    let (event, bag) = reward(&store, &mut d, origin, p).await;
    let plan = d
        .worlds
        .get_mut(&origin)
        .unwrap()
        .drop_item(
            p,
            Slot {
                object: bag,
                slot: 0,
                kind: 0xa22,
            },
        )
        .unwrap();
    let dropped = plan.dropped.unwrap();
    let write = crate::store::InventoryWrite {
        id: plan.character,
        account: plan.account,
        revision: plan.revision,
        items: plan.items.clone(),
    };
    let movement = d.retain_inventory_event(plan).unwrap();
    store.prepare_world_event(movement.clone()).await.unwrap();
    let receipt = store
        .commit_inventory_event(movement.clone(), write)
        .await
        .unwrap();
    d.publish_inventory_event(&movement, receipt.result.as_ref().unwrap())
        .unwrap();
    d.publish_inventory_event(&movement, receipt.result.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        store
            .unpicked_entitlements(event.clone())
            .await
            .unwrap()
            .len(),
        1
    );
    let w = d.worlds.get_mut(&origin).unwrap();
    assert_eq!(w.entities[&dropped].items[0], 0xa22);
    assert_eq!(w.xml.bag_entitlements[&(dropped, 0)].0, event.id);
    assert!(!w.xml.bag_entitlements.contains_key(&(bag, 0)));
    w.now = 60_000;
    w.entities.get_mut(&dropped).unwrap().reserved = false;
    w.remove_entity(dropped);
    let proof = w.entitlement_expiries(&event);
    assert_eq!(proof.len(), 1);
    store
        .expire_world_entitlements(event.clone(), proof)
        .await
        .unwrap();
    assert!(store.unpicked_entitlements(event).await.unwrap().is_empty());
    store.validate_world_events().await.unwrap();
    drop(store);
    let reopened = crate::store::Store::open(
        &Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("review-a3-ground.db"),
        &d.content,
    )
    .unwrap();
    reopened.validate_world_events().await.unwrap();
}

#[tokio::test]
async fn xml_rewards_review_a2_captured_terminal_allows_older_queued_progress_to_drain() {
    let (store, mut d, origin, p) = setup("a2-queued").await;
    let w = d.worlds.get_mut(&origin).unwrap();
    w.players
        .get_mut(&p)
        .unwrap()
        .queue_experience(5, None, false)
        .unwrap();
    let actor = spawn(w, 0x600, 65.5, 64.5);
    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
    assert!(w.xml.facts.contains_key(&actor));
    let progress = w.pending_progress_where(|_, _| true);
    assert_eq!(progress.len(), 1);
    let snapshot = progress[0].1.clone();
    store.progress(snapshot.clone()).await.unwrap();
    w.finish_progress(p, snapshot).unwrap();
    assert_eq!(w.players[&p].terminal_captures, 1);
    w.now += 50;
    w.xml_step().unwrap();
    let event = w.pending_terminal_events()[0].clone();
    let receipt = store.coordinate_terminal(event).await.unwrap();
    w.finish_terminal_event(receipt).unwrap();
    assert_eq!(w.players[&p].terminal_captures, 0);
    assert!(w.players[&p].character.exp > 5);
}
