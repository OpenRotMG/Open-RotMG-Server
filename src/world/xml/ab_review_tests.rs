use super::*;
use rand::RngCore;

/// Runs the unchanged GhostAnchor program until its Island PlaceMap stamp awaits durable receipt.
async fn island_stamp(name: &str) -> (crate::store::Store, World, i32, i32, Character) {
    let content = Arc::new(crate::world::test_content());
    let path = Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join(format!("{name}.db"));
    let store = crate::store::Store::open(&path, &content).unwrap();
    let account = store.list(name.into()).await.unwrap();
    let character = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    // Both authored random branches remain real: choose an Island run, not a rewritten program.
    for seed in 0..32 {
        let mut w = flat_nonrealm(content.clone());
        w.prepared_seed = Some(seed);
        let p = w.enter(character.clone(), None).unwrap();
        let anchor = spawn(&mut w, 0xe38, 64.5, 64.5);
        // The encounter fixture supplies its ordinary ship; the source program and orders are unchanged.
        spawn(&mut w, 0xe37, 66.5, 64.5);
        let definition = content.game.as_ref().unwrap().definitions[&0xe38].clone();
        for tick in 0..500 {
            if !w.entities.contains_key(&anchor) {
                break;
            }
            w.now = tick * 50;
            // Stay near the actual moving source; no desktop/client or executor state injection.
            w.players.get_mut(&p).unwrap().pos = w.entities[&anchor].pos;
            let path = w
                .xml
                .actors
                .get(&anchor)
                .map(|a| {
                    a.path()
                        .iter()
                        .map(|s| definition.states[*s as usize].name.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let battle = path.contains(&"Battle");
            for ship in w
                .entities
                .values()
                .filter(|e| e.kind == 0xe37)
                .map(|e| e.id)
                .collect::<Vec<_>>()
            {
                if battle {
                    w.xml_defeated(ship, TerminalContext::NonPlayer).unwrap();
                } else if path.contains(&"IniMid")
                    && w.entities[&ship].hp == w.content.objects[&0xe37].hp
                {
                    w.damage_enemy_nonplayer(ship, 5000, true, &[]).unwrap();
                }
            }
            w.xml_step().unwrap();
            if w.xml.stamps.contains_key(&anchor) {
                return (store, w, anchor, p, character);
            }
            drain_events(&mut w).unwrap();
        }
    }
    panic!("unchanged GhostAnchor program never reached Island");
}

#[tokio::test]
async fn xml_rewards_ab_review_stamp_resumes_real_ghost_anchor_actor_and_releases_custody() {
    let (store, mut w, anchor, p, _) = island_stamp("ab-stamp").await;
    let event = w
        .pending_terminal_events()
        .into_iter()
        .find(|e| e.kind == "stamp")
        .unwrap();
    let source = event.native_payload().unwrap()["origin"]["source"].clone();
    assert_eq!(source["emissions"], 0);
    let active = w.xml.actors[&anchor].active_ms();
    let before = serde_json::to_vec(&w.map.cells).unwrap();
    let ids = w.ids.load(Ordering::Relaxed);
    for _ in 0..5 {
        w.now += 50;
        w.xml_step().unwrap();
        for other in w
            .pending_terminal_events()
            .into_iter()
            .filter(|e| e.kind != "stamp")
        {
            let receipt = store.coordinate_terminal(other.clone()).await.unwrap();
            w.finish_terminal_event(receipt).unwrap();
            store.publish_world_event(other.clone()).await.unwrap();
            w.acknowledge_terminal_publication(other.id.clone());
            let rows = store.unpicked_entitlements(other.clone()).await.unwrap();
            w.publish_entitlements(other, rows).unwrap();
        }
    }
    assert_eq!(w.xml.actors[&anchor].active_ms(), active);
    assert_eq!(serde_json::to_vec(&w.map.cells).unwrap(), before);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    let receipt = store.coordinate_terminal(event.clone()).await.unwrap();
    w.finish_terminal_event(receipt.clone()).unwrap();
    w.finish_terminal_event(receipt).unwrap();
    store.publish_world_event(event.clone()).await.unwrap();
    w.acknowledge_terminal_publication(event.id.clone());
    w.players.get_mut(&p).unwrap().departing = true;
    w.now += 50;
    w.xml_step()
        .expect("normal Actor.tick must resume the captured Map, not poison");
    assert!(!w.xml.stamps.contains_key(&anchor));
    assert!(
        w.xml
            .terminal_leases
            .get(&anchor)
            .is_none_or(|(_, id)| id.as_deref() != Some(event.id.as_str()))
    );
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x741).count(), 4);
    let after_ids = w.ids.load(Ordering::Relaxed);
    for _ in 0..10 {
        w.now += 50;
        w.xml_step().unwrap();
        drain_events(&mut w).unwrap();
    }
    assert!(
        !w.entities.contains_key(&anchor),
        "original Island Decay must retire the source"
    );
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x741).count(), 4);
    assert_eq!(w.ids.load(Ordering::Relaxed), after_ids);
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
    assert!(w.xml.stamps.is_empty());
    assert!(!w.xml.terminal_leases.contains_key(&anchor));
}

#[tokio::test]
async fn xml_rewards_ab_review_spider_egg_first_facts_precede_birth_capacity() {
    let content = Arc::new(crate::world::test_content());
    let path = Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("ab-birth-capacity.db");
    let store = crate::store::Store::open(&path, &content).unwrap();
    let account = store.list("ab-birth".into()).await.unwrap();
    let ch = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let mut w = flat_nonrealm(content.clone());
    let p = w.enter(ch, None).unwrap();
    let pos = Pos { x: 64.5, y: 64.5 };
    w.players.get_mut(&p).unwrap().pos = pos;
    let egg = w.spawn_entity(0x21b, pos, vec![]).unwrap();
    let fillers = (0..CHILD_LIMIT)
        .map(|_| {
            let id = w.spawn_entity(0x21c, pos, vec![]).unwrap();
            w.xml_own(id, Some(egg), w.xml_identity(egg).unwrap().encounter)
                .unwrap();
            id
        })
        .collect::<Vec<_>>();
    assert_eq!(w.xml_owned_count(egg), CHILD_LIMIT);
    let mut expected = w.rng.clone();
    let seed = expected.next_u64();
    let hp = w.entities[&egg].hp;
    assert_eq!(w.damage_enemy(p, egg, 10000, true, &[], None).unwrap(), hp);
    assert_eq!(w.entities[&egg].hp, 0);
    assert!(
        w.xml.facts.contains_key(&egg),
        "birth capacity wait lost first defeat facts"
    );
    assert_eq!(w.xml.facts[&egg].seed, seed);
    assert_eq!(w.players[&p].terminal_captures, 1);
    assert!(w.pending_terminal_events().is_empty());
    let player = w.players.get_mut(&p).unwrap();
    player.pos = Pos { x: 110., y: 110. };
    player.departing = true;
    player.death_pending = true;
    for _ in 0..7 {
        w.rng.next_u64();
        expected.next_u64();
    }
    for id in fillers {
        w.remove_entity(id);
    }
    w.now = 50;
    w.xml_step().unwrap();
    let event = w.pending_terminal_events()[0].clone();
    let payload = event.terminal_payload().unwrap();
    assert_eq!(payload.seed, seed);
    assert!(payload.ledger[0].alive && !payload.ledger[0].departing);
    assert_eq!(payload.ledger[0].effective_damage, hp as u64);
    assert_eq!(
        payload.actions["publication"]["defeat_recipients"][0]["position"],
        serde_json::to_value(pos).unwrap()
    );
    assert_eq!(w.rng.clone().next_u64(), expected.next_u64());
    let receipt = store.coordinate_terminal(event).await.unwrap();
    w.finish_terminal_event(receipt).unwrap();
    assert!(!w.entities.contains_key(&egg));
    assert!((2..=7).contains(&w.entities.values().filter(|e| e.kind == 0x21c).count()));
    assert_eq!(w.players[&p].terminal_captures, 0);
}

#[tokio::test]
async fn xml_rewards_ab_review_settled_fingerprint_history_remains_portable() {
    for state in ["zero-loot", "picked", "prepared", "committed", "unpicked"] {
        let name = format!("ab-fingerprint-{state}");
        let (store, mut d, origin, p) = review_tests::setup(&name).await;
        let w = d.worlds.get_mut(&origin).unwrap();
        let actor = spawn(w, 0x600, 65.5, 64.5);
        w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
        let mut event = w.pending_terminal_events()[0].clone();
        if state == "zero-loot" {
            let mut payload = event.terminal_payload().unwrap().clone();
            payload.allocation = None;
            payload.actions["loot"] = serde_json::json!(false);
            event = crate::store::world_events::Event::terminal(&w.xml_game().fingerprint, payload)
                .unwrap();
        }
        let account = w.players[&p].character.account;
        store.prepare_world_event(event.clone()).await.unwrap();
        if state != "prepared" {
            store.commit_terminal_event(event.clone()).await.unwrap();
        }
        if !matches!(state, "prepared" | "committed") {
            store.publish_world_event(event.clone()).await.unwrap();
        }
        if state == "picked" {
            let ch = w.players[&p].character.clone();
            let mut items = ch.items.clone();
            items[5] = 0xa22;
            store
                .pick_world_entitlement(
                    event.clone(),
                    0,
                    "b".repeat(32),
                    crate::store::InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items,
                    },
                    5,
                )
                .await
                .unwrap();
            let pickup = store
                .world_event("b".repeat(32))
                .await
                .unwrap()
                .unwrap()
                .event;
            store.publish_world_event(pickup).await.unwrap();
        }
        drop(store);
        let mut content = (*d.content).clone();
        Arc::make_mut(content.game.as_mut().unwrap()).fingerprint = "a".repeat(64);
        let path =
            Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join(format!("review-{name}.db"));
        let changed = crate::store::Store::open(&path, &content).unwrap();
        assert!(
            format!(
                "{:#}",
                changed.world_event(event.id.clone()).await.unwrap_err()
            )
            .contains("content fingerprint changed")
        );
        let ready = changed.export_account_save(account).await;
        if matches!(state, "zero-loot" | "picked") {
            assert!(
                ready.is_ok(),
                "settled {state} history must remain portable: {ready:?}"
            );
        } else {
            assert!(ready.is_err(), "unsettled {state} must still refuse");
            // A changed catalogue means a restart, so the prior world is gone and gets an explicit disposition.
            assert_eq!(
                changed.reconcile_lost_world_events(vec![]).await.unwrap(),
                [(
                    event.id.clone(),
                    if state == "prepared" {
                        "aborted_origin_lost"
                    } else {
                        "origin_lost"
                    }
                    .into()
                )]
            );
            assert!(
                changed.export_account_save(account).await.is_ok(),
                "{state}"
            );
        }
        assert_eq!(changed.validate_world_events().await.unwrap(), {
            if state == "picked" { 2 } else { 1 }
        });
    }
}

#[tokio::test]
async fn xml_rewards_ab_review_defeat_during_unpublished_stamp_keeps_hit_facts_and_settles_stamp() {
    let (store, mut w, anchor, _, character) = island_stamp("ab-stamp-defeat").await;
    let stamp = w
        .pending_terminal_events()
        .into_iter()
        .find(|e| e.kind == "stamp")
        .unwrap();
    w.entities
        .get_mut(&anchor)
        .unwrap()
        .damage
        .insert(character.id, 1);
    w.xml_defeated(anchor, TerminalContext::NonPlayer).unwrap();
    let seed = w.xml.facts[&anchor].seed;
    assert!(
        w.pending_terminal_events()
            .iter()
            .all(|e| e.kind == "stamp")
    );
    for _ in 0..3 {
        w.rng.next_u64();
    }
    let receipt = store.coordinate_terminal(stamp.clone()).await.unwrap();
    w.finish_terminal_event(receipt).unwrap();
    store.publish_world_event(stamp.clone()).await.unwrap();
    w.acknowledge_terminal_publication(stamp.id.clone());
    let mut journaled = None;
    for _ in 0..10 {
        w.now += 50;
        w.xml_step().unwrap();
        if let Some(event) = w
            .pending_terminal_events()
            .into_iter()
            .find(|e| e.kind == "terminal")
        {
            journaled.get_or_insert(event.terminal_payload().unwrap().seed);
        }
        drain_events(&mut w).unwrap();
    }
    assert_eq!(
        journaled,
        Some(seed),
        "terminal seed comes from the defeat, not the later retry"
    );
    assert!(!w.entities.contains_key(&anchor));
    assert!(
        w.xml.stamps.is_empty(),
        "published stamp of a defeated source is settled"
    );
    assert!(!w.xml.terminal_leases.contains_key(&anchor));
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
}

#[test]
fn xml_rewards_ab_review_failing_stamp_portal_intent_holds_no_custody_permit() {
    let content = Arc::new(crate::world::test_content());
    let mut w = flat_nonrealm(content.clone());
    let anchor = spawn(&mut w, 0xe38, 64.5, 64.5);
    let pos = w.entities[&anchor].pos;
    let (b, stamp) = content.game.as_ref().unwrap().definitions[&0xe38]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .filter_map(|b| match b.fields.get(&F::Map) {
            Some(Value::Map(map)) => w.xml_prepare_stamp(*map, pos).ok().map(|s| (b.clone(), s)),
            _ => None,
        })
        .find(|(_, stamp)| stamp.has_portals(&w))
        .expect("Island PlaceMap stamps portals");
    // Swap the catalog only after the real preflight, so the staged portal intent is the failing step.
    let mut changed = (*content).clone();
    let mut game = (**changed.game.as_ref().unwrap()).clone();
    game.definitions.remove(&0x741);
    changed.game = Some(Arc::new(game));
    w.content = Arc::new(changed);
    assert!(
        w.xml_stage_stamp(stamp, step(&w, anchor, b.local), &b)
            .is_err()
    );
    assert!(!w.xml.terminal_leases.contains_key(&anchor));
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
}
