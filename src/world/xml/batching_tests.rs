use super::*;
use rand::RngCore;

#[tokio::test]
async fn xml_rewards_world_bags_keep_full_suffix_at_128_cap_and_expire_only_published_ordinals() {
    let mut content = Content::load(Path::new("data")).unwrap();
    let mut rules = authored(
        "Pirate",
        0x600,
        "<XpMult>0</XpMult><Behavior>RemainVisible</Behavior><SoulboundLoot prob=\"0\" min=\"4096\">Potion of Speed</SoulboundLoot>",
    );
    for (kind, service) in [
        (6, "projectile"),
        (0x30e, "player"),
        (0xa21, "inventory"),
        (0x500, "container"),
        (0x503, "container"),
        (0x507, "container"),
        (0x508, "container"),
        (0x509, "container"),
    ] {
        rules += &format!(
            r#"<Object type="{kind}" id="{}" disposition="native" nativeService="{service}" provenance="authored" origin="bounded native bag fixture" reason="actual selected dependency"/>"#,
            content.objects[&kind].name
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
    content.game = Some(Arc::new(game::compile_fixture(&content, inputs).unwrap()));
    let content = Arc::new(content);
    let path =
        std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap()).join("bounded-bags.db");
    let store = crate::store::Store::open(&path, &content).unwrap();
    let account = store.list("bounded-bags-fixture".into()).await.unwrap();
    let character = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let mut w = World::in_domain(content, Arc::new(flat_map(128)), WorldDomain::NonRealm).unwrap();
    let p = w.enter(character, None).unwrap();
    let actor = spawn(&mut w, 0x600, 65.5, 64.5);
    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
    let event = w.pending_terminal_events()[0].clone();
    assert_eq!(event.terminal_payload().unwrap().allocations().len(), 4096);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER injected_terminal_failure BEFORE INSERT ON world_entitlements BEGIN SELECT RAISE(ABORT,'owned terminal failure'); END;").unwrap();
    assert!(store.coordinate_terminal(event.clone()).await.is_err());
    assert_eq!(
        store
            .world_event(event.id.clone())
            .await
            .unwrap()
            .unwrap()
            .status,
        0
    );
    assert_eq!(w.pending_terminal_events(), vec![event.clone()]);
    assert!(w.entities.contains_key(&actor));
    assert!(w.bag_expiry.is_empty());
    assert!(!w.xml.claims.contains(&actor));
    db.execute_batch("DROP TRIGGER injected_terminal_failure")
        .unwrap();
    drop(db);
    let receipt = store.coordinate_terminal(event.clone()).await.unwrap();
    w.finish_terminal_event(receipt).unwrap();
    store.publish_world_event(event.clone()).await.unwrap();
    w.acknowledge_terminal_publication(event.id.clone());
    for _ in 0..40 {
        w.publish_entitlements(
            event.clone(),
            store.unpicked_entitlements(event.clone()).await.unwrap(),
        )
        .unwrap();
    }
    assert_eq!(w.bag_expiry.len(), 128);
    assert_eq!(w.xml.bag_entitlements.len(), 1024);
    assert_eq!(
        store
            .unpicked_entitlements(event.clone())
            .await
            .unwrap()
            .len(),
        4096
    );
    let ordinals = w
        .xml
        .bag_entitlements
        .values()
        .map(|(_, ordinal, _, _)| *ordinal)
        .collect::<BTreeSet<_>>();
    assert_eq!(ordinals, (0..1024).collect());
    let bags = w.bag_expiry.keys().copied().collect::<Vec<_>>();
    let stale = w.delivery_batch().pop().unwrap();
    assert!(stale.expiry.is_empty());
    let stale_rows = store.unpicked_entitlements(event.clone()).await.unwrap();
    w.now = 60_000;
    for bag in bags {
        w.remove_entity(bag);
    }
    w.publish_delivery(stale, stale_rows).unwrap();
    assert_eq!(w.bag_expiry.len(), 4);
    let batch = w.delivery_batch();
    let [attempt] = batch.as_slice() else {
        panic!("expected the original delivery to remain queued");
    };
    assert_eq!(attempt.event, event);
    let proofs = attempt.expiry.clone();
    assert_eq!(proofs.len(), 1024);
    store
        .expire_world_entitlements(event.clone(), proofs)
        .await
        .unwrap();
    let rows = store.unpicked_entitlements(event.clone()).await.unwrap();
    assert_eq!(rows.len(), 3072);
    assert_eq!(rows[0].ordinal, 1024);
    w.publish_entitlements(event.clone(), rows).unwrap();
    assert_eq!(w.bag_expiry.len(), 8);
    assert!(
        w.xml
            .bag_entitlements
            .values()
            .all(|(_, ordinal, _, _)| *ordinal >= 1024)
    );
    store.validate_world_events().await.unwrap();
    drop(store);
    let store = crate::store::Store::open(&path, &w.content).unwrap();
    assert_eq!(
        store
            .unpicked_entitlements(event.clone())
            .await
            .unwrap()
            .len(),
        3072
    );
    store.reconcile_lost_world_events(vec![]).await.unwrap();
    assert!(store.unpicked_entitlements(event).await.is_err());
}

#[test]
fn xml_rewards_world_inventory_admission_is_bounded_before_source_ids_or_mutation() {
    let (w, p, _) = fixture("<Behavior>RemainVisible</Behavior>");
    let ids = w.ids.load(Ordering::Relaxed);
    let before = w.players[&p].character.clone();
    let plans = (0..128)
        .map(|_| w.plan(p, before.items.clone(), None).unwrap())
        .collect::<Vec<_>>();
    assert!(
        w.plan(p, before.items.clone(), None)
            .err()
            .unwrap()
            .to_string()
            .contains("backpressure")
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.players[&p].character, before);
    drop(plans);
    assert!(w.plan(p, before.items, None).is_ok());
}

#[test]
fn xml_rewards_world_terminal_admission_is_per_world_and_refuses_lethal_damage_before_hp_ledger_ids_or_rng()
 {
    let (mut first, p, actor) = fixture("<Behavior>RemainVisible</Behavior>");
    let (mut second, q, other) = fixture("<Behavior>RemainVisible</Behavior>");
    let shared = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    first.xml.terminal_allocations = Arc::clone(&shared);
    second.xml.terminal_allocations = shared;
    // Seven stuck terminals of this World plus one real one fill only this World's capacity.
    first
        .xml
        .terminal_capacity
        .store(terminal::PENDING_LIMIT - 1, Ordering::Release);
    first
        .damage_enemy(p, actor, 10000, true, &[], None)
        .unwrap();
    assert_eq!(
        first.xml.terminal_capacity.load(Ordering::Acquire),
        terminal::PENDING_LIMIT
    );
    let target = spawn(&mut first, 0x600, 69.5, 64.5);
    let hp = first.entities[&target].hp;
    let ids = first.ids.load(Ordering::Relaxed);
    let rng = first.rng.clone().next_u64();
    let captured = first.pending_terminal_events();
    let error = first
        .damage_enemy(p, target, 10000, true, &[], None)
        .unwrap_err();
    assert!(error.to_string().contains("backpressure"));
    assert_eq!(first.entities[&target].hp, hp);
    assert!(first.entities[&target].damage.is_empty());
    assert_eq!(first.ids.load(Ordering::Relaxed), ids);
    assert_eq!(first.rng.clone().next_u64(), rng);
    assert_eq!(first.pending_terminal_events(), captured);
    // Another World's kills are unaffected by the first World's stuck terminals.
    second
        .damage_enemy(q, other, 10000, true, &[], None)
        .unwrap();
    assert_eq!(second.entities[&other].hp, 0);
    assert_eq!(second.pending_terminal_events().len(), 1);
    assert_eq!(second.xml.terminal_capacity.load(Ordering::Acquire), 1);
}

#[test]
fn xml_rewards_world_server_wide_pending_allocations_are_bounded_with_a_visible_error() {
    let (mut w, p, actor) = fixture(
        "<Behavior>RemainVisible</Behavior><SoulboundLoot prob=\"1\">Small Firecracker</SoulboundLoot>",
    );
    let total = Arc::new(std::sync::atomic::AtomicUsize::new(
        terminal::PENDING_ALLOCATION_LIMIT,
    ));
    w.xml.terminal_allocations = Arc::clone(&total);
    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
    frame(&mut w, 50);
    assert!(
        w.pending_terminal_events().is_empty(),
        "no plan staged past the server-wide bound"
    );
    let error = w
        .xml_terminal(&w.xml.terminal_attempts.last().unwrap().clone())
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("terminal allocation backpressure"),
        "{error:#}"
    );
    // P2-B: an earlier capacity wait must not make a later, different preflight failure look like a Store wait.
    w.xml_bound_terminal_retry(actor, &anyhow::anyhow!("births geometry-blocked"));
    assert!(
        w.xml.retry_since.contains_key(&actor),
        "non-Store failure starts the bounded retry window"
    );
    w.xml.retry_since.remove(&actor);
    // Capacity waits are not impossible preflights: the retry window never abandons the defeat.
    frame(&mut w, 50 + 2 * terminal::PREFLIGHT_RETRY_MS);
    assert!(w.xml.abandon_physical.is_empty() && w.entities.contains_key(&actor));
    total.store(terminal::PENDING_ALLOCATION_LIMIT - 1, Ordering::Release);
    w.now += 50;
    w.xml_step().unwrap();
    assert_eq!(w.pending_terminal_events().len(), 1);
    assert_eq!(
        total.load(Ordering::Acquire),
        terminal::PENDING_ALLOCATION_LIMIT
    );
    // The reservation travels with the event's permit and is returned when the permit is released.
    w.xml.terminal_leases.clear();
    assert_eq!(
        total.load(Ordering::Acquire),
        terminal::PENDING_ALLOCATION_LIMIT - 1
    );
}

#[test]
fn xml_rewards_world_receipted_terminal_with_blocked_births_publishes_after_the_retry_window() {
    let (mut w, p, actor) = fixture(
        "<Behavior>RemainVisible</Behavior><Behavior childId=\"Piratess\" prob=\"1\">MetamorphoseAtDeath</Behavior>",
    );
    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
    assert_eq!(w.pending_terminal_events().len(), 1);
    // Geometry refuses the journaled replacement after staging, before the receipt.
    let cell =
        (w.entities[&actor].pos.y as i32 * w.map.width + w.entities[&actor].pos.x as i32) as usize;
    Arc::make_mut(&mut w.map).cells[cell].ground = 0xff;
    let _ = drain_events(&mut w);
    let start = w.now;
    for at in [start + 50, start + terminal::PREFLIGHT_RETRY_MS / 2] {
        frame(&mut w, at);
        assert!(
            w.entities.contains_key(&actor),
            "still within the window at {at}"
        );
    }
    frame(&mut w, start + terminal::PREFLIGHT_RETRY_MS + 100);
    frame(&mut w, start + terminal::PREFLIGHT_RETRY_MS + 150);
    assert!(
        !w.entities.contains_key(&actor),
        "receipted defeat published without its blocked birth"
    );
    assert!(!w.entities.values().any(|e| e.kind == 0x601));
    assert!(w.pending_terminal_events().is_empty() && w.xml.abandon_physical.is_empty());
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
}

#[test]
fn xml_rewards_world_receipted_terminal_never_takes_the_unstaged_failure_branch() {
    let (mut w, p, actor) = fixture(
        "<Behavior>RemainVisible</Behavior><Behavior childId=\"Piratess\" prob=\"1\">MetamorphoseAtDeath</Behavior>",
    );
    w.damage_enemy(p, actor, 10000, true, &[], None).unwrap();
    let cell =
        (w.entities[&actor].pos.y as i32 * w.map.width + w.entities[&actor].pos.x as i32) as usize;
    Arc::make_mut(&mut w.map).cells[cell].ground = 0xff;
    let _ = drain_events(&mut w);
    assert!(w.xml.terminals[&actor].receipt);
    // A publication invariant that keeps failing past both retry windows (P2-A).
    let error = anyhow::anyhow!("persistent publication invariant");
    for step in 0..3 {
        w.now += terminal::PREFLIGHT_RETRY_MS + 1;
        w.xml_bound_terminal_retry(actor, &error);
        assert!(
            w.entities.contains_key(&actor) && w.xml.facts.contains_key(&actor),
            "step {step}"
        );
    }
    assert!(w.xml.terminals[&actor].receipt && w.xml.terminal_leases.contains_key(&actor));
}

#[test]
fn xml_rewards_world_paused_inflight_hit_does_not_reserve_terminal_custody() {
    let (mut w, p, target) = fixture("<Behavior>RemainVisible</Behavior>");
    let hp = w.entities[&target].hp;
    w.damage_enemy(p, target, hp - 1, true, &[], None).unwrap();
    assert_eq!(w.entities[&target].hp, 1);
    let spec = w.content.objects[&0x600].projectiles[&0].clone();
    let pos = w.players[&p].pos;
    let mut bullet = Bullet {
        damage_origin: w.damage_origin(p).unwrap(),
        owner: p,
        source_kind: 0x600,
        id: 1,
        friendly: true,
        start: pos,
        pos,
        angle: 0.,
        born: w.now,
        damage: spec.min,
        spec,
        hit: BTreeSet::new(),
        hit_enemy: false,
    };
    let ledger = w.entities[&target].damage.clone();
    w.players.get_mut(&p).unwrap().paused = true;
    assert!(w.bullet_hit(&mut bullet, target));
    assert!(bullet.hit.contains(&target));
    assert_eq!(w.entities[&target].hp, 1);
    assert_eq!(w.entities[&target].damage, ledger);
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
    assert!(w.pending_terminal_events().is_empty());
}

#[test]
fn xml_rewards_world_terminal_claim_history_is_bounded_without_evicting_live_custody() {
    let (mut w, _, target) = fixture("<Behavior>RemainVisible</Behavior>");
    w.reserve_terminal_damage(target, w.entities[&target].hp, true)
        .unwrap();
    w.xml.claims.extend(0..4096);
    w.acknowledge_terminal_publication("unrelated-completed-event".into());
    assert_eq!(w.xml.claims.len(), CHILD_LIMIT + 1);
    assert!(w.xml.claims.contains(&target));
    assert!(w.xml.terminal_leases.contains_key(&target));
}

#[test]
fn xml_rewards_world_directory_instances_keep_independent_terminal_capacity() {
    let mut d = admission_tests::isolated_directory(Arc::new(crate::world::test_content()));
    let first = d
        .instance("pirate-cave", None, Some(DungeonReturn::Random))
        .unwrap();
    let second = d
        .instance("pirate-cave", None, Some(DungeonReturn::Random))
        .unwrap();
    // The first World's terminals are all stuck awaiting commits.
    d.worlds[&first]
        .xml
        .terminal_capacity
        .store(terminal::PENDING_LIMIT, Ordering::Release);
    let shared = d.terminal_allocations.clone();
    let w = d.worlds.get_mut(&second).unwrap();
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Ada".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    let target = w
        .entities
        .values()
        .find(|e| w.content.objects[&e.kind].enemy && e.hp > 0)
        .unwrap()
        .id;
    let e = w.entities.get_mut(&target).unwrap();
    (e.hp, e.condition) = (1, 0);
    w.damage_enemy(p, target, 10, true, &[], None).unwrap();
    assert_eq!(
        w.entities[&target].hp, 0,
        "another World's stuck terminals do not refuse this kill"
    );
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 1);
    assert!(
        Arc::ptr_eq(&w.xml.terminal_allocations, &shared),
        "allocations stay server-wide"
    );
}
