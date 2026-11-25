use super::*;

#[tokio::test]
async fn xml_rewards_world_store_stheno_damage_loot_portal_pickup_lost_reply_reconnect_restart() {
    let content = Arc::new(crate::world::test_content());
    let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join("stheno-world-store.db");
    let store = crate::store::Store::open(&path, &content).unwrap();
    let account = store
        .list("stheno-world-store-fixture".into())
        .await
        .unwrap();
    let character = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let destination_slot = (4..12).find(|slot| character.items[*slot] == -1).unwrap();
    let mut directory = admission_tests::isolated_directory(content.clone());
    let instance = directory
        .instance("snake-pit", None, Some(DungeonReturn::Random))
        .unwrap();
    let (p, boss, mut rx, event, original_hp) = {
        let w = directory.worlds.get_mut(&instance).unwrap();
        let p = w.enter(character.clone(), None).unwrap();
        let rx = output(w, p);
        let boss = w.entities.values().find(|e| e.kind == 0x917).unwrap().id;
        let pos = w.entities[&boss].pos;
        w.players.get_mut(&p).unwrap().pos = (0..32)
            .map(|n| point(pos, n as f64 * std::f64::consts::TAU / 32., 3.))
            .find(|pos| w.walkable(*pos))
            .unwrap();
        for now in (0..=3500).step_by(50) {
            w.now = now;
            w.xml_step().unwrap();
        }
        assert_eq!(
            w.entities[&boss].condition & crate::condition::bit(crate::condition::INVULNERABLE),
            0
        );
        let original_hp = w.entities[&boss].hp;
        assert_eq!(original_hp, content.objects[&0x917].hp);
        // Fixed component-fixture RNG is selected before damage, never after a captured defeat.
        w.rng = rand::rngs::StdRng::seed_from_u64(52);
        assert_eq!(
            w.damage_enemy(p, boss, 20000, true, &[], None).unwrap(),
            original_hp
        );
        let event = w.pending_terminal_events().into_iter().next().unwrap();
        assert!(w.entities.contains_key(&boss));
        assert!(w.clear_exit.is_none());
        assert!(w.bag_expiry.is_empty());
        assert!(!w.xml.claims.contains(&boss));
        assert!(w.xml_terminal_pending(boss));
        assert_eq!(w.entities[&boss].hp, 0);
        assert!(w.entities[&boss].reserved);
        let applied_ledger = w.entities[&boss].damage.clone();
        assert_eq!(applied_ledger[&character.id], original_hp);
        let active_ms = w.xml.actors[&boss].active_ms();
        let shots = w
            .bullets
            .iter()
            .filter(|b| b.owner == boss)
            .map(|b| (b.id, b.born))
            .collect::<BTreeSet<_>>();
        assert_eq!(w.damage_enemy(p, boss, 20000, true, &[], None).unwrap(), 0);
        w.xml_defeated(boss, TerminalContext::NonPlayer).unwrap();
        w.now += 50;
        w.xml_step().unwrap();
        assert!(
            w.bullets
                .iter()
                .filter(|b| b.owner == boss)
                .all(|b| shots.contains(&(b.id, b.born)))
        );
        assert_eq!(w.xml.actors[&boss].active_ms(), active_ms);
        assert_eq!(w.entities[&boss].damage, applied_ledger);
        assert!(w.entities[&boss].reserved);
        assert!(!w.xml.claims.contains(&boss));
        assert!(w.bag_expiry.is_empty());
        assert!(w.clear_exit.is_none());
        assert_eq!(w.pending_terminal_events(), vec![event.clone()]);
        (p, boss, rx, event, original_hp)
    };
    let payload = event.terminal_payload().unwrap();
    assert_eq!(payload.ledger[0].character, character.id);
    assert_eq!(payload.ledger[0].effective_damage, original_hp as u64);
    assert!(
        !payload.allocations().is_empty(),
        "selected deterministic Stheno fixture must have actual loot"
    );
    let total = payload.allocations().len();
    let receipt = store.coordinate_terminal(event.clone()).await.unwrap();
    let committed = store.load(account.id, character.id).await.unwrap();
    assert!(committed.exp > character.exp);
    assert_eq!(committed.kills, character.kills + 1);
    assert_eq!(committed.lifetime.kills, character.lifetime.kills + 1);
    assert_eq!(committed.dungeons[0], character.dungeons[0] + 1);
    {
        let w = directory.worlds.get_mut(&instance).unwrap();
        w.finish_terminal_event(receipt.clone()).unwrap();
        w.finish_terminal_event(receipt).unwrap();
        assert!(!w.entities.contains_key(&boss));
        assert!(w.cleared);
        let portal = w
            .entities
            .values()
            .find(|entity| entity.kind == 0x704)
            .unwrap()
            .id;
        assert_eq!(w.entities[&portal].kind, 0x704);
        assert!(w.entities[&portal].published);
        w.publish_entitlements(
            event.clone(),
            store.unpicked_entitlements(event.clone()).await.unwrap(),
        )
        .unwrap();
        w.publish_entitlements(
            event.clone(),
            store.unpicked_entitlements(event.clone()).await.unwrap(),
        )
        .unwrap();
        let count = w
            .entities
            .values()
            .filter(|e| w.bag_expiry.contains_key(&e.id))
            .map(|e| e.items.iter().filter(|k| **k >= 0).count())
            .sum::<usize>();
        assert_eq!(count, total);
    }
    store.publish_world_event(event.clone()).await.unwrap();
    let (bag, slot, item) = {
        let w = &directory.worlds[&instance];
        let bag = *w.bag_expiry.keys().next().unwrap();
        let slot = w.entities[&bag].items.iter().position(|k| *k >= 0).unwrap();
        (bag, slot, w.entities[&bag].items[slot])
    };
    let plan = {
        let w = directory.worlds.get_mut(&instance).unwrap();
        w.players.get_mut(&p).unwrap().pos = w.entities[&bag].pos;
        w.updates(p, true).unwrap();
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|packet| packet.id == wire::UPDATE && update_ids(&packet).0.contains(&bag))
        );
        let mut body = Writer::new();
        body.i32(w.now as i32)
            .f32(w.players[&p].pos.x)
            .f32(w.players[&p].pos.y)
            .i32(bag)
            .u8(slot as u8)
            .i16(item as i16)
            .i32(p)
            .u8(destination_slot as u8)
            .i16(-1);
        let packet = Packet {
            id: 65,
            body: body.0,
        };
        let mut r = wire::Reader::new(&packet.body);
        r.i32().unwrap();
        r.f32().unwrap();
        r.f32().unwrap();
        let from = Slot {
            object: r.i32().unwrap(),
            slot: r.u8().unwrap() as usize,
            kind: i32::from(r.i16().unwrap()),
        };
        let to = Slot {
            object: r.i32().unwrap(),
            slot: r.u8().unwrap() as usize,
            kind: i32::from(r.i16().unwrap()),
        };
        r.done().unwrap();
        w.swap(p, from, to).unwrap()
    };
    let pickup = directory.retain_inventory_event(plan.clone()).unwrap();
    store.prepare_world_event(pickup.clone()).await.unwrap();
    let write = crate::store::InventoryWrite {
        id: plan.character,
        account: plan.account,
        revision: plan.revision,
        items: plan.items.clone(),
    };
    let scoped = store.for_world_event(pickup.clone()).unwrap();
    store.lifecycle_test_lose_reply();
    assert!(
        scoped
            .exchange_collected(
                vec![write.clone()],
                plan.vaults.clone(),
                plan.collected.clone()
            )
            .await
            .is_err()
    );
    let record = store.world_event(pickup.id.clone()).await.unwrap().unwrap();
    assert_eq!(record.status, 1);
    let result = record.result.unwrap();
    directory.publish_inventory_event(&pickup, &result).unwrap();
    directory.publish_inventory_event(&pickup, &result).unwrap();
    assert!(
        scoped
            .exchange_collected(vec![write], plan.vaults.clone(), plan.collected.clone())
            .await
            .is_err()
    );
    assert_eq!(
        store
            .world_event(pickup.id.clone())
            .await
            .unwrap()
            .unwrap()
            .result,
        Some(result)
    );
    store.publish_world_event(pickup.clone()).await.unwrap();
    directory.forget_inventory_event(&pickup.id).unwrap();
    drop(scoped);
    drop(plan);
    let saved = store.load(account.id, character.id).await.unwrap();
    assert_eq!(saved.items[destination_slot], item);
    assert_eq!(saved.revision, character.revision + 1);
    assert_eq!(saved.dungeons[0], committed.dungeons[0]);
    let departed = directory
        .worlds
        .get_mut(&instance)
        .unwrap()
        .leave(p)
        .unwrap();
    store.progress(departed).await.unwrap();
    let reconnect = store.load(account.id, character.id).await.unwrap();
    let (tx, mut reconnected) = tokio::sync::mpsc::channel(256);
    let (lost, _) = tokio::sync::watch::channel(None);
    let p2 = directory
        .worlds
        .get_mut(&instance)
        .unwrap()
        .enter(
            reconnect.clone(),
            Some(Output {
                trace: None,
                packets: tx,
                lost,
            }),
        )
        .unwrap();
    assert_eq!(
        directory.worlds[&instance].players[&p2].character.items[destination_slot],
        item
    );
    let success = reconnected.try_recv().unwrap();
    assert_eq!(success.id, wire::CREATE_SUCCESS);
    let mut body = wire::Reader::new(&success.body);
    assert_eq!(body.i32().unwrap(), p2);
    assert_eq!(body.i32().unwrap(), character.id);
    body.done().unwrap();
    let original = store
        .world_event(event.id.clone())
        .await
        .unwrap()
        .unwrap()
        .event;
    drop(directory);
    drop(store);
    let store = crate::store::Store::open(&path, &content).unwrap();
    store.reconcile_lost_world_events(vec![]).await.unwrap();
    let restarted = store.load(account.id, character.id).await.unwrap();
    assert_eq!(restarted.items, reconnect.items);
    assert_eq!(restarted.exp, reconnect.exp);
    assert_eq!(restarted.dungeons, reconnect.dungeons);
    assert_eq!(restarted.revision, reconnect.revision);
    assert_eq!(
        store
            .world_event(event.id.clone())
            .await
            .unwrap()
            .unwrap()
            .event,
        original
    );
    store.validate_world_events().await.unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let (picked, expired): (usize, usize) = db
        .query_row(
            "SELECT sum(status=1),sum(status=2) FROM world_entitlements WHERE event=?1",
            [&event.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(picked, 1);
    assert_eq!(expired, total - 1);
    std::fs::write(path.parent().unwrap().join("stheno-world-store-receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"qualification":"World+Store integration through actual ordinary Snake Pit factory and authoritative damage/receipt/inventory paths; encoded UPDATE and pickup packet. NOT App/native-socket or rendered acceptance; full App blocked by genuine Realm elevation.","event":event.id,"pickup":pickup.id,"seed":original.terminal_payload().unwrap().seed,"effective_damage":original_hp,"allocations":total,"picked":picked,"origin_lost_expired":expired,"character":character.id,"xp":restarted.exp,"snake_pits":restarted.dungeons[0],"revision":restarted.revision})).unwrap()).unwrap();
}

#[tokio::test]
async fn xml_rewards_world_store_known_commit_uses_request_time_after_scripted_deadline_or_records_origin_loss()
 {
    let content = Arc::new(crate::world::test_content());
    for mode in [0, 1, 2] {
        let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
            .join(format!("late-incantation-{mode}.db"));
        let store = crate::store::Store::open(&path, &content).unwrap();
        let account = store.list(format!("incantation-{mode}")).await.unwrap();
        let mut ch = store
            .create(account.id, content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.items[4] = 0x722;
        ch.revision = store
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
            .unwrap()[0];
        let mut w = flat_nonrealm(content.clone());
        let p = w.enter(ch.clone(), None).unwrap();
        w.name = "Oryx's Chamber".into();
        w.xml.template = Some("oryx-chamber".into());
        w.cleared = true;
        let pos = w.players[&p].pos;
        let locked = w.spawn_clear_portal(0x721, pos, true).unwrap();
        w.xml_step().unwrap();
        assert_eq!(w.xml_portal_expiry(locked), Some(120_000));
        w.now = 1000;
        let plan = w
            .use_item(
                p,
                Slot {
                    object: p,
                    slot: 4,
                    kind: 0x722,
                },
                pos,
            )
            .unwrap()
            .unwrap();
        let opened = plan.unlock.unwrap().opened;
        let mut d = admission_tests::isolated_directory(content.clone());
        d.ids = w.ids.clone();
        d.clock.set_paused(true);
        d.templates.insert(
            crate::endgame::CELLAR.into(),
            Arc::new(Map::load(Path::new("data/wine-cellar.wmap"), &content).unwrap()),
        );
        d.worlds.insert(-2, w);
        let plan = d.stage_unlock(plan).unwrap();
        let event = d.retain_inventory_event(plan.clone()).unwrap();
        store.prepare_world_event(event.clone()).await.unwrap();
        let scoped = store.for_world_event(event.clone()).unwrap();
        store.lifecycle_test_lose_reply();
        assert!(
            scoped
                .exchange(
                    vec![crate::store::InventoryWrite {
                        id: plan.character,
                        account: plan.account,
                        revision: plan.revision,
                        items: plan.items.clone()
                    }],
                    vec![]
                )
                .await
                .is_err()
        );
        let record = store.world_event(event.id.clone()).await.unwrap().unwrap();
        assert_eq!(record.status, 1);
        let result = record.result.unwrap();
        drop(scoped);
        if mode == 2 {
            drop(d);
            store.reconcile_lost_world_events(vec![]).await.unwrap();
            let lost = store.world_event(event.id.clone()).await.unwrap().unwrap();
            assert_eq!(lost.status, 3);
            assert_eq!(
                lost.result.unwrap()["reconciliation"]["disposition"],
                "committed_effect_origin_lost"
            );
            assert_eq!(store.load(ch.account, ch.id).await.unwrap().items[4], -1);
            assert!(store.prepare_world_event(event.clone()).await.is_err());
            store.validate_world_events().await.unwrap();
            drop(store);
            let store = crate::store::Store::open(&path, &content).unwrap();
            assert_eq!(
                store.load(ch.account, ch.id).await.unwrap().revision,
                ch.revision + 1
            );
            continue;
        }
        let now = if mode == 0 { 119_999 } else { 120_050 };
        let w = d.worlds.get_mut(&-2).unwrap();
        w.now = now;
        w.xml_step().unwrap();
        assert_eq!(w.xml_portal_expiry(locked), Some(120_000));
        assert_eq!(w.players[&p].character.items[4], 0x722);
        assert!(!w.entities[&opened].published);
        let late = d.publish_inventory_event(&event, &result).unwrap();
        assert_eq!(
            late,
            if mode == 0 {
                None
            } else {
                Some((120_000, 120_050))
            }
        );
        if let Some((deadline, at)) = late {
            store
                .record_late_inventory_publication(event.clone(), deadline, at)
                .await
                .unwrap();
        }
        let expires = d.portals[&opened].expires;
        assert_eq!(expires, Some(d.tick + 2400));
        d.worlds.get_mut(&-2).unwrap().now += 1000;
        assert_eq!(d.publish_inventory_event(&event, &result).unwrap(), late);
        assert_eq!(d.portals[&opened].expires, expires);
        let w = d.worlds.get_mut(&-2).unwrap();
        assert!(!w.entities.contains_key(&locked));
        assert!(w.entities[&opened].published && !w.entities[&opened].reserved);
        assert_eq!(w.players[&p].character.items[4], -1);
        assert_eq!(w.players[&p].character.revision, ch.revision + 1);
        store.publish_world_event(event.clone()).await.unwrap();
        d.forget_inventory_event(&event.id).unwrap();
        store.validate_world_events().await.unwrap();
        assert_eq!(store.load(ch.account, ch.id).await.unwrap().items[4], -1);
    }
}

/// Live bot crash (fix-l): a dying giver's causal credit reached a recipient whose leveled progress a staged terminal
/// held, so `pending_progress_where` had nothing to settle and the persistence worker stopped the server.
#[tokio::test]
async fn xml_rewards_causal_credit_recipient_held_by_staged_terminal_settles_through_its_receipt() {
    let content = Arc::new(crate::world::test_content());
    let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join("causal-credit-terminal.db");
    let store = crate::store::Store::open(&path, &content).unwrap();
    let mut characters = Vec::new();
    for name in ["credit-giver", "credit-recipient"] {
        let account = store.list(name.into()).await.unwrap();
        characters.push(
            store
                .create(account.id, content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap(),
        );
    }
    let (giver, mut recipient) = (characters[0].clone(), characters[1].clone());
    // Start just below level two so one uncapped Stheno award crosses several assist thresholds.
    recipient.exp = crate::store::experience_goal(1).unwrap() - 1;
    let mut directory = admission_tests::isolated_directory(content.clone());
    let instance = directory
        .instance("snake-pit", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = directory.worlds.get_mut(&instance).unwrap();
    let boss = w.entities.values().find(|e| e.kind == 0x917).unwrap().id;
    let pos = w.entities[&boss].pos;
    let mut players = Vec::new();
    for ch in [giver.clone(), recipient.clone()] {
        let p = w.enter(ch, None).unwrap();
        w.players.get_mut(&p).unwrap().pos = (0..32)
            .map(|n| point(pos, n as f64 * std::f64::consts::TAU / 32., 3.))
            .find(|pos| w.walkable(*pos))
            .unwrap();
        players.push(p);
    }
    for now in (0..=3500).step_by(50) {
        w.now = now;
        w.xml_step().unwrap();
    }
    w.damage_enemy(players[1], boss, 10, true, &[], None)
        .unwrap();
    w.damage_enemy(players[0], boss, 20000, true, &[], None)
        .unwrap();
    let event = w.pending_terminal_events().into_iter().next().unwrap();
    assert_eq!(
        w.credit_recipients(giver.id),
        vec![(players[1], recipient.account)]
    );
    assert!(w.pending_progress_where(|p, _| p == players[1]).is_empty());
    assert!(w.progress_held_by_terminal(players[1]).unwrap());
    let receipt = store.coordinate_terminal(event).await.unwrap();
    w.finish_terminal_event(receipt).unwrap();
    assert!(w.credit_recipients(giver.id).is_empty());
    assert!(!w.progress_held_by_terminal(players[1]).unwrap());
    let recipient = &w.players[&players[1]].character;
    assert_eq!((recipient.level, recipient.exp), (4, 574));
}

#[tokio::test]
async fn xml_rewards_world_unplaceable_clear_exit_settles_once_across_restart() {
    hard_preflight_restart(false).await;
}

#[tokio::test]
async fn xml_rewards_world_hard_scripted_birth_preflight_settles_once_across_restart() {
    hard_preflight_restart(true).await;
}

async fn hard_preflight_restart(scripted: bool) {
    use crate::store::Store;
    let content = Arc::new(crate::world::test_content());
    let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join(format!("hard-preflight-scripted-{scripted}.db"));
    let store = Store::open(&path, &content).unwrap();
    let account = store.list("hard-preflight".into()).await.unwrap();
    let ch = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let mut directory = admission_tests::isolated_directory(content.clone());
    let instance = directory
        .instance("snake-pit", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = directory.worlds.get_mut(&instance).unwrap();
    let p = w.enter(ch.clone(), None).unwrap();
    let boss = w.entities.values().find(|e| e.kind == 0x917).unwrap().id;
    let pos = w.entities[&boss].pos;
    w.players.get_mut(&p).unwrap().pos = pos;
    for now in (0..=3500).step_by(50) {
        w.now = now;
        w.xml_step().unwrap();
    }
    let pos = w.entities[&boss].pos;
    if scripted {
        w.xml.fault = Some(Fault::TerminalBirthPlan);
    } else {
        let blocked = content
            .grounds
            .iter()
            .filter_map(|(&kind, &speed)| (speed <= 0.).then_some(kind))
            .min()
            .expect("selected non-walkable ground");
        let (x, y) = (pos.x as i32, pos.y as i32);
        let map = Arc::make_mut(&mut w.map);
        for dy in -2..=2 {
            for dx in -2..=2 {
                if (dx, dy) != (0, 0) {
                    map.cells[((y + dy) * map.width + x + dx) as usize].ground = blocked;
                }
            }
        }
        assert!(w.clear_portal_position(pos, false).is_err());
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(52);
    let seed = rng.r#gen::<u64>();
    w.rng = rand::rngs::StdRng::seed_from_u64(52);
    let hp = w.entities[&boss].hp;
    assert_eq!(w.damage_enemy(p, boss, hp, true, &[], None).unwrap(), hp);
    let ledger = w.xml.facts[&boss].ledger.clone();
    assert_eq!(w.xml.facts[&boss].seed, seed);
    assert_eq!(ledger[0].effective_damage, hp as u64);
    let defeated_at = w.now;
    for delta in [
        50,
        50 + terminal::PREFLIGHT_RETRY_MS,
        100 + terminal::PREFLIGHT_RETRY_MS,
    ] {
        assert!(w.pending_terminal_events().is_empty());
        assert!(w.entities[&boss].reserved);
        assert_eq!(w.players[&p].terminal_captures, 1);
        // Neither unrelated draws nor a later position can change the original captured defeat.
        let _ = w.rng.r#gen::<u64>();
        w.players.get_mut(&p).unwrap().pos = Pos { x: 1., y: 1. };
        if scripted {
            w.xml.fault = Some(Fault::TerminalBirthPlan);
        }
        w.now = defeated_at + delta;
        w.xml_step().unwrap();
    }
    let event = w.pending_terminal_events().pop().unwrap();
    let payload = event.terminal_payload().unwrap();
    let publication = &payload.actions["publication"];
    let abandoned = &publication["abandoned_births"][0];
    assert_eq!(abandoned["disposition"], "physical_effect_abandoned");
    if scripted {
        assert_eq!(abandoned["effect"], "scripted_birth_plan");
        assert!(
            abandoned["preflight"]
                .as_str()
                .unwrap()
                .contains("injected")
        );
        assert!(!publication["clear_portal"].is_null());
    } else {
        assert_eq!(abandoned["effect"]["kind"], 0x704);
        assert!(
            abandoned["preflight"]
                .as_str()
                .unwrap()
                .contains("no walkable position")
        );
        assert!(publication["clear_portal"].is_null());
    }
    assert_eq!(publication["now"], defeated_at);
    assert_eq!(publication["defeat_recipients"][0]["character"], ch.id);
    assert_eq!(payload.seed, seed);
    assert_eq!(
        serde_json::to_value(&payload.ledger).unwrap(),
        serde_json::to_value(&ledger).unwrap()
    );
    let expected = crate::loot::evaluator::evaluate(
        w.xml_game(),
        payload.defeat,
        &ledger,
        seed,
        crate::store::world_events::LIMITS,
    )
    .unwrap();
    assert!(!expected.allocations().is_empty());
    assert_eq!(
        serde_json::to_value(&payload.allocation).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    // Reject a real journal write and let both windows elapse with no receipt.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_terminal BEFORE INSERT ON world_events BEGIN SELECT RAISE(ABORT, 'preflight test Store unavailable'); END;").unwrap();
    assert!(store.coordinate_terminal(event.clone()).await.is_err());
    assert!(store.world_event(event.id.clone()).await.unwrap().is_none());
    w.now += 2 * terminal::PREFLIGHT_RETRY_MS;
    w.xml_step().unwrap();
    assert_eq!(w.pending_terminal_events(), std::slice::from_ref(&event));
    assert!(w.entities[&boss].reserved && !w.cleared);
    assert_eq!(w.players[&p].terminal_captures, 1);
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), ch);
    db.execute_batch("DROP TRIGGER reject_terminal").unwrap();
    drop(db);
    let committed = store.coordinate_terminal(event.clone()).await.unwrap();
    assert_eq!(committed.status, 1);
    drop(store);

    let store = Store::open(&path, &content).unwrap();
    assert_eq!(
        store.coordinate_terminal(event.clone()).await.unwrap(),
        committed
    );
    let rows = store.unpicked_entitlements(event.clone()).await.unwrap();
    assert_eq!(rows.len(), payload.allocations().len());
    for (ordinal, (row, allocation)) in rows.iter().zip(payload.allocations()).enumerate() {
        assert_eq!(
            (row.ordinal, row.item, row.owner),
            (ordinal, allocation.item(), allocation.owner())
        );
    }
    w.finish_terminal_event(committed.clone()).unwrap();
    w.finish_terminal_event(committed).unwrap();
    assert!(!w.entities.contains_key(&boss) && w.cleared);
    assert_eq!(w.players[&p].terminal_captures, 0);
    assert_eq!(
        w.entities.values().filter(|e| e.kind == 0x704).count(),
        usize::from(scripted)
    );
    let saved = store.load(ch.account, ch.id).await.unwrap();
    assert_eq!(saved.kills, ch.kills + 1);
    assert_eq!(saved.dungeons[0], ch.dungeons[0] + 1);
    assert!(saved.exp > ch.exp);
    assert_eq!(w.players[&p].character, saved);
    assert_eq!(
        store
            .publish_world_event(event.clone())
            .await
            .unwrap()
            .status,
        2
    );
    assert_eq!(store.coordinate_terminal(event).await.unwrap().status, 2);
    assert_eq!(store.load(ch.account, ch.id).await.unwrap(), saved);
}
