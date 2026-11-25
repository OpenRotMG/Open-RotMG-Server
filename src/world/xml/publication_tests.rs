use super::*;
use rand::RngCore;

#[test]
fn xml_rewards_terminal_victory_portal_publishes_once_with_source_clock_not_legacy_exit() {
    let content = Arc::new(crate::world::test_content());
    for (kind, target, template, name) in [
        (0x1740, 0x721, crate::endgame::CHAMBER, "Oryx's Chamber"),
        (0x932, 0x71d, crate::endgame::CELLAR, "Wine Cellar"),
    ] {
        let mut w = flat_nonrealm(content.clone());
        w.name = name.into();
        w.xml.template = Some(template.into());
        w.return_route = Some(DungeonReturn::Random);
        let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
        ch.account = 1;
        ch.id = 1;
        let p = w.enter(ch, None).unwrap();
        w.players.get_mut(&p).unwrap().pos = Pos { x: 65.5, y: 64.5 };
        let mut rx = output(&mut w, p);
        let id = spawn(&mut w, kind, 64.5, 64.5);
        w.updates(p, true).unwrap();
        while rx.try_recv().is_ok() {}
        let rng = w.rng.clone().next_u64();
        let ids = w.ids.load(Ordering::Relaxed);
        let (linked, pos, replacement) = w.xml_victory_birth(&w.entities[&id]).unwrap().unwrap();
        assert_eq!(
            (linked, pos.x, pos.y, replacement),
            (target, 64.7, 64.7, false)
        );
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.rng.clone().next_u64(), rng);
        w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
        let event = w.pending_terminal_events()[0].clone();
        let ids = w.ids.load(Ordering::Relaxed);
        assert!(!w.entities.values().any(|e| e.kind == target));
        assert!(w.entities.contains_key(&id));
        assert!(rx.try_recv().is_err());
        drain_events(&mut w).unwrap();
        assert!(w.cleared);
        assert!(!w.entities.contains_key(&id));
        let portals = w
            .entities
            .values()
            .filter(|e| e.kind == target && e.published)
            .collect::<Vec<_>>();
        assert_eq!(portals.len(), 1);
        let portal = portals[0].id;
        assert_eq!(portal, ids);
        assert_eq!((portals[0].pos.x, portals[0].pos.y), (64.7, 64.7));
        assert_eq!(w.xml.portals[&portal].event, event.id);
        w.updates(p, true).unwrap();
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|packet| packet.id == wire::UPDATE && update_ids(&packet).0.contains(&portal))
        );
        if target == 0x721 {
            assert_eq!(w.xml_portal_expiry(portal), Some(120_000));
            w.now = 119_999;
            w.xml_step().unwrap();
            assert!(w.entities.contains_key(&portal));
            w.now = 120_000;
            w.xml_step().unwrap();
            drain_events(&mut w).unwrap();
            assert!(!w.entities.contains_key(&portal));
        }
    }
}
#[test]
fn xml_rewards_victory_portal_beside_a_wall_publishes_at_the_corpse_once() {
    let content = Arc::new(crate::world::test_content());
    let mut w = flat_nonrealm(content.clone());
    w.name = "Wine Cellar".into();
    w.xml.template = Some(crate::endgame::CELLAR.into());
    w.return_route = Some(DungeonReturn::Random);
    // Wine Wall 4 (FullOccupy) fills the tile the +0.2 offset lands in, as in native wine-cellar job 303782fb.
    spawn(&mut w, 0x1d4, 65.5, 65.5);
    let at = Pos { x: 64.9, y: 64.9 };
    let id = w.spawn_entity(0x932, at, vec![]).unwrap();
    w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
    drain_events(&mut w).unwrap();
    drain_events(&mut w).unwrap();
    assert!(w.cleared);
    assert!(!w.entities.contains_key(&id));
    let portals: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == 0x71d && e.published)
        .collect();
    assert_eq!(portals.len(), 1);
    assert_eq!((portals[0].pos.x, portals[0].pos.y), (at.x, at.y));
}
#[tokio::test]
async fn xml_rewards_davy_stamp_lost_reply_whole_publication_and_concurrent_first_use_bind_one_world()
 {
    let content = Arc::new(crate::world::test_content());
    let path = Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("davy-publication.db");
    let store = crate::store::Store::open(&path, &content).unwrap();
    let mut w = flat_nonrealm(content.clone());
    let anchor = spawn(&mut w, 0xe38, 64.5, 64.5);
    let b=content.game.as_ref().unwrap().definitions[&0xe38].states.iter().flat_map(|s|&s.behaviors).find(|b|b.kind==K::PlaceMap&&matches!(value(b,F::Map),Some(Value::Map(map)) if content.game.as_ref().unwrap().maps[*map as usize].name=="ghost_ship/encounter/davyisland.jm")).unwrap().clone();
    let st = step(&w, anchor, b.local);
    let before = serde_json::to_vec(&w.map.cells).unwrap();
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    let Some(Value::Map(map)) = value(&b, F::Map) else {
        panic!("linked stamp")
    };
    let stamp = w.xml_prepare_stamp(*map, w.entities[&anchor].pos).unwrap();
    let error = format!(
        "{:#}",
        w.xml_apply_stamp(stamp, st.token.scope.identity)
            .unwrap_err()
    );
    assert!(
        error.contains("requires phase4 coordinator receipt"),
        "{error}"
    );
    assert_eq!(
        w.apply(
            Service::Map,
            &b,
            st,
            &mut rand::rngs::StdRng::seed_from_u64(52)
        )
        .unwrap(),
        ActionOutcome::Pending
    );
    let event = w.pending_terminal_events()[0].clone();
    store.prepare_world_event(event.clone()).await.unwrap();
    let receipt = store.commit_terminal_event(event.clone()).await.unwrap();
    assert_eq!(serde_json::to_vec(&w.map.cells).unwrap(), before);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert_eq!(w.entities.len(), 1);
    assert!(w.xml.actors.is_empty() && w.xml.rewards.is_empty());
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 1);
    let late = store.coordinate_terminal(event.clone()).await.unwrap();
    assert_eq!(late.event, receipt.event);
    w.finish_terminal_event(late.clone()).unwrap();
    let published = serde_json::to_vec(&w.map.cells).unwrap();
    assert_ne!(published, before);
    let after_ids = w.ids.load(Ordering::Relaxed);
    w.finish_terminal_event(late).unwrap();
    assert_eq!(w.ids.load(Ordering::Relaxed), after_ids);
    assert_eq!(serde_json::to_vec(&w.map.cells).unwrap(), published);
    assert_eq!(
        w.apply(
            Service::Map,
            &b,
            st,
            &mut rand::rngs::StdRng::seed_from_u64(52)
        )
        .unwrap(),
        ActionOutcome::Applied
    );
    store.publish_world_event(event.clone()).await.unwrap();
    w.acknowledge_terminal_publication(event.id);
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.kind == 0x741 && e.published)
            .count(),
        4
    );
    assert!(
        w.entities
            .values()
            .filter(|e| e.kind == 0x741)
            .all(|e| w.xml_portal_expiry(e.id) == Some(30_000))
    );
    let portal = w
        .entities
        .values()
        .find(|e| e.kind == 0x741)
        .unwrap()
        .clone();
    assert_eq!(w.xml_portal_expiry(portal.id), Some(30_000));
    assert_eq!(
        w.xml.portals[&portal.id].intent.destination,
        crate::content::game::PortalDestination::Instance(crate::dungeon::davy::TEMPLATE.into())
    );
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let p = w.enter(ch.clone(), None).unwrap();
    ch.account = 2;
    ch.id = 2;
    ch.name = "Second".into();
    let q = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = portal.pos;
    w.players.get_mut(&q).unwrap().pos = portal.pos;
    let mut d = admission_tests::isolated_directory(content);
    d.clock.set_paused(true);
    d.worlds.insert(17, w);
    let handle = Handle::spawn(d);
    let (first, second) = tokio::join!(
        handle.directory(move |d| d.destination(p, Some(portal.id))),
        handle.directory(move |d| d.destination(q, Some(portal.id)))
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.2, second.2);
    handle
        .directory(move |d| {
            assert_eq!(d.worlds.len(), 2);
            assert_eq!(d.portals[&portal.id].destination, Some(first.2));
            assert_eq!(d.worlds[&first.2].name, crate::dungeon::davy::NAME);
            assert_eq!(d.worlds[&17].xml_portal_expiry(portal.id), Some(30_000));
            Ok(())
        })
        .await
        .unwrap();
}

#[test]
fn xml_rewards_terminal_portal_failed_first_use_retains_template_lifetime_and_no_partial_destination()
 {
    let content = Arc::new(crate::world::test_content());
    let mut d = admission_tests::isolated_directory(content.clone());
    let mut w = World::with_ids_context(
        content.clone(),
        Arc::new(flat_map(128)),
        d.ids.clone(),
        Some(WorldDomain::NonRealm),
    )
    .unwrap();
    w.return_route = Some(DungeonReturn::Random);
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let p = w.enter(ch, None).unwrap();
    let actor = spawn(&mut w, 0xd7a, 64.5, 64.5);
    let b = content.game.as_ref().unwrap().definitions[&0xd7a]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == K::Metamorphose)
        .unwrap()
        .clone();
    let plan = TerminalPlan {
        origin: scope(&w, actor),
        context: Some(TerminalContext::NonPlayer),
        announcements: vec![],
        reason: TerminalReason::Metamorphose,
        rewards: crate::behavior::Rewards {
            loot: false,
            xp: false,
        },
        behavior: Some(b),
        hooks: vec![],
    };
    assert!(w.xml_terminal(&plan).is_err());
    drain_events_plan(&mut w, Some(&plan)).unwrap();
    let portal = w.entities.values().find(|e| e.kind == 0xd7b).unwrap().id;
    w.players.get_mut(&p).unwrap().pos = w.entities[&portal].pos;
    let expires = w.xml.portals[&portal].expires;
    d.worlds.insert(17, w);
    let template = d.templates.remove(crate::endgame::CHAMBER).unwrap();
    let ids = d.ids.load(Ordering::Relaxed);
    let rng = d.realm_rng.clone().next_u64();
    let next = d.next_world;
    for _ in 0..2 {
        let error = format!("{:#}", d.destination(p, Some(portal)).unwrap_err());
        assert!(
            error.contains("Dungeon construction failed") && error.contains("event="),
            "{error}"
        );
        assert_eq!(d.worlds.len(), 1);
        assert!(!d.portals.contains_key(&portal));
        assert_eq!(d.ids.load(Ordering::Relaxed), ids);
        assert_eq!(d.realm_rng.clone().next_u64(), rng);
        assert_eq!(d.next_world, next);
        assert!(d.worlds[&17].entities[&portal].published);
        assert_eq!(d.worlds[&17].xml.portals[&portal].expires, expires);
        assert_eq!(
            &d.worlds[&17].xml.portals[&portal].intent.destination,
            &crate::content::game::PortalDestination::Instance(crate::endgame::CHAMBER.into())
        );
    }
    d.templates.insert(crate::endgame::CHAMBER.into(), template);
    let (_, _, destination) = d.destination(p, Some(portal)).unwrap();
    assert_eq!(d.worlds[&destination].name, "Oryx's Chamber");
    assert_eq!(d.destination(p, Some(portal)).unwrap().2, destination);
    assert_eq!(d.worlds.len(), 2);
    assert_eq!(d.worlds[&17].xml.portals[&portal].expires, expires);
}

#[test]
fn xml_rewards_actual_sprite_factory_publishes_only_the_linked_clear_portal() {
    let content = Arc::new(crate::world::test_content());
    let mut d = admission_tests::isolated_directory(content.clone());
    let world = d
        .instance("sprite-world", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&world).unwrap();
    assert!(w.clear_exit.is_none());
    assert!(!w.entities.values().any(|e| e.kind == 0x70e));
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.name = "SpriteFixture".into();
    let p = w.enter(ch, None).unwrap();
    let boss = w.entities.values().find(|e| e.kind == 0xd06).unwrap().id;
    // Construction admits Limon into his invulnerable start; wait out begin_teleport1 like a real arrival.
    for at in [50, 2100] {
        w.players.get_mut(&p).unwrap().pos = w.entities[&boss].pos;
        frame(w, at);
    }
    w.players.get_mut(&p).unwrap().pos = w.entities[&boss].pos;
    assert_eq!(w.damage_enemy(p, boss, 1, true, &[], None).unwrap(), 1);
    w.xml_defeated(boss, TerminalContext::NonPlayer).unwrap();
    assert!(w.entities.contains_key(&boss));
    assert!(!w.entities.values().any(|e| e.kind == 0x70e));
    drain_events(w).unwrap();
    assert!(w.cleared);
    assert!(!w.entities.contains_key(&boss));
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.kind == 0x70e && e.published)
            .count(),
        1
    );
    assert_eq!(w.players[&p].character.dungeons[1], 1);
}
#[test]
fn xml_rewards_portal_publication_refuses_missing_owner_scoped_route_before_ids_or_rng() {
    let content = Arc::new(crate::world::test_content());
    let w = World::in_domain(content, Arc::new(flat_map(128)), WorldDomain::NonRealm).unwrap();
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    let error = format!(
        "{:#}",
        w.xml_preflight_publication(0x720, Pos { x: 64.5, y: 64.5 }, &[], None)
            .unwrap_err()
    );
    assert!(error.contains("requires a publishable route"), "{error}");
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert!(w.entities.is_empty());
}
