use super::*;
use rand::RngCore;

fn domain_world(
    domain: Option<WorldDomain>,
    height: Option<u8>,
    initial: Option<u16>,
) -> Result<World> {
    let mut map = flat_map(128);
    map.elevation = height.map(|h| vec![h; 128 * 128].into());
    map.cells[64 * 128 + 64].object = initial;
    World::with_ids_context(
        Arc::new(crate::world::test_content()),
        Arc::new(map),
        Arc::new(AtomicI32::new(5000)),
        domain,
    )
}

#[test]
fn xml_runtime_world_stay_above_actual_source_requires_realm_context_before_ids_not_display_or_height_guess()
 {
    let content = Arc::new(crate::world::test_content());
    for kind in [0xd02, 0x651] {
        for domain in [None, Some(WorldDomain::Realm)] {
            for height in [None, Some(200)] {
                if domain == Some(WorldDomain::Realm) && height.is_some() {
                    continue;
                }
                let mut map = flat_map(128);
                map.elevation = height.map(|h| vec![h; 128 * 128].into());
                map.cells[0].object = Some(kind);
                let ids = Arc::new(AtomicI32::new(6000));
                let error = format!(
                    "{:#}",
                    World::with_ids_context(content.clone(), Arc::new(map), ids.clone(), domain)
                        .err()
                        .unwrap()
                );
                assert!(
                    error.contains(if domain.is_none() {
                        "explicit native world domain"
                    } else {
                        "missing source elevation capability"
                    }),
                    "{error}"
                );
                assert_eq!(ids.load(Ordering::Relaxed), 6000);
            }
        }
        let mut w = domain_world(None, Some(200), None).unwrap();
        w.name = "Realm".into();
        let ids = w.ids.load(Ordering::Relaxed);
        let rng = w.rng.clone().next_u64();
        assert!(
            format!(
                "{:#}",
                w.spawn_entity(kind, Pos { x: 64.5, y: 64.5 }, vec![])
                    .unwrap_err()
            )
            .contains("explicit native world domain")
        );
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.rng.clone().next_u64(), rng);
        assert!(w.entities.is_empty());
    }
}

#[test]
fn xml_runtime_world_stay_above_nonrealm_is_ineligible_with_or_without_height_and_real_source_alternatives_execute()
 {
    for kind in [0xd02, 0x651] {
        for height in [None, Some(200)] {
            let mut w = domain_world(Some(WorldDomain::NonRealm), height, Some(kind)).unwrap();
            w.name = "Realm".into();
            assert_eq!(w.map.elevation.is_some(), height.is_some());
            let id = *w.entities.keys().next().unwrap();
            let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
            ch.id = 1;
            ch.account = 1;
            ch.name = "Ada".into();
            let p = w.enter(ch, None).unwrap();
            w.players.get_mut(&p).unwrap().pos = Pos { x: 72.5, y: 64.5 };
            if height.is_some() {
                Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap())
                    [64 * 128 + 64] = 0;
            }
            let b = w.xml_game().definitions[&kind]
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
                .find(|b| b.kind == K::StayAbove)
                .unwrap()
                .clone();
            assert_eq!(
                n(&b, F::MinAltitude),
                if kind == 0xd02 { 115. } else { 200. }
            );
            let st = step(&w, id, b.local);
            let mut rng = rand::rngs::StdRng::seed_from_u64(43);
            let before = rng.clone().next_u64();
            assert!(!w.ready(Service::Movement, &b, st).unwrap());
            assert_eq!(
                w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
                ActionOutcome::Blocked
            );
            assert_eq!(rng.next_u64(), before);
            assert!(w.xml.motions.is_empty());
            let mut wrong = b.clone();
            wrong.fields.insert(F::Speed, Value::Bool(false));
            assert!(w.ready(Service::Movement, &wrong, st).is_err());
            let mut absent = b.clone();
            absent.fields.remove(&F::MinAltitude);
            assert!(w.ready(Service::Movement, &absent, st).is_err());
            frame(&mut w, 0);
            assert_eq!(w.bullets.len(), if kind == 0xd02 { 4 } else { 3 });
            frame(&mut w, 50);
            assert!(w.entities[&id].pos.distance(Pos { x: 64.5, y: 64.5 }) > 0.);
            let motion = w
                .xml
                .motions
                .iter()
                .find(|(t, _)| t.scope.identity.actor == id)
                .unwrap();
            assert_ne!(
                motion.0.local, b.local,
                "ineligible StayAbove must not win movement bucket"
            );
            assert_eq!(
                w.xml_game().definitions[&kind].states[0].behaviors[motion.0.local as usize].kind,
                if kind == 0xd02 { K::Wander } else { K::Follow }
            );
        }
    }
    let (mut w, _, id) = fixture(
        r#"<Behavior minAltitude="0">StayAbove</Behavior><Behavior terrain="Grass" minAltitude="0">StayInTerrain</Behavior>"#,
    );
    w.xml.domain = Some(WorldDomain::NonRealm);
    Arc::make_mut(&mut w.map).elevation = None;
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[1].clone();
    assert!(
        w.ready(Service::Movement, &b, step(&w, id, b.local))
            .is_err(),
        "StayInTerrain altitude is not Realm-only"
    );
}

#[test]
fn xml_runtime_world_passive_wall_void_actual_manor_keeps_selected_role_collision_damage_and_map_only_entitlement()
 {
    let content = Arc::new(crate::world::test_content());
    let mut d = super::admission_tests::isolated_directory(content.clone());
    let world = d
        .instance("manor-of-the-immortals", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&world).unwrap();
    let id = w
        .entities
        .values()
        .find(|e| {
            e.kind == 0x181a
                && w.content
                    .void_grounds
                    .contains(&w.map.cell(e.pos.x, e.pos.y).unwrap().ground)
        })
        .unwrap()
        .id;
    let e = &w.entities[&id];
    let pos = e.pos;
    let void_cell = (pos.y as usize) * w.map.width as usize + pos.x as usize;
    let void_config = w.map.cells[void_cell].config.clone();
    assert_eq!(e.hp, 3000);
    let property = &w.content.objects[&0x181a];
    assert!(
        property.enemy && property.static_object && property.full_occupy && property.blocks_sight
    );
    assert_eq!(
        w.xml_game().definitions[&0x181a].admission_role().unwrap(),
        AdmissionRole::Scripted
    );
    assert_eq!(
        w.xml_game().definitions[&0x181a].disposition,
        Disposition::Passive
    );
    assert!(w.xml_game().definitions[&0x181a].loot.is_empty());
    assert_eq!(w.xml_game().definitions[&0x181a].xp_multiplier, 0.);
    assert!(!w.walkable(pos));
    assert!(!w.xml_placeable(0x181a, pos, &[id], None).unwrap());
    let before = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    assert!(w.spawn_entity(0x181a, pos, vec![]).is_err());
    assert_eq!(w.ids.load(Ordering::Relaxed), before);
    assert_eq!(w.rng.clone().next_u64(), rng);
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    ch.name = "Ada".into();
    let p = w.enter(ch, None).unwrap();
    assert!(
        w.entities[&id].conditions(w.now) & crate::condition::bit(crate::condition::INVINCIBLE)
            != 0,
        "actual Void shelf keeps generator eff config"
    );
    assert_eq!(w.damage_enemy(p, id, 1000, true, &[], None).unwrap(), 0);
    if let Err(error) = drain_events(w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    let id = w
        .entities
        .values()
        .find(|e| {
            e.kind == 0x181a
                && e.conditions(w.now) & crate::condition::bit(crate::condition::INVINCIBLE) == 0
        })
        .expect("actual secret-door destructible shelf")
        .id;
    assert!(
        !w.content.void_grounds.contains(
            &w.map
                .cell(w.entities[&id].pos.x, w.entities[&id].pos.y)
                .unwrap()
                .ground
        )
    );
    assert_eq!(w.damage_enemy(p, id, 1000, true, &[], None).unwrap(), 1000);
    if let Err(error) = drain_events(w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert_eq!(w.entities[&id].hp, 2000);
    assert_eq!(w.damage_enemy(p, id, 2000, true, &[], None).unwrap(), 2000);
    if let Err(error) = drain_events(w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert!(!w.entities.contains_key(&id));
    assert!(w.xml.claims.contains(&id));
    assert!(w.xml_game().definitions[&0x181a].loot.is_empty());
    let secret_pos = w.xml.rewards[&id].entity.pos;
    let secret_cell = secret_pos.y as usize * w.map.width as usize + secret_pos.x as usize;
    std::fs::write(Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("world-passive-wall-void-and-damage.json"),serde_json::to_vec_pretty(&serde_json::json!({"kind":0x181a,"hp":3000,"role":"ScriptedPassive","source":w.xml_game().definitions[&0x181a].source,"loot":[],"void":{"cell":void_cell,"config":void_config,"ground":w.map.cells[void_cell].ground,"invincible":true,"authoritative_damage":0,"collision":true},"secret_door":{"cell":secret_cell,"config":w.map.cells[secret_cell].config,"ground":w.map.cells[secret_cell].ground,"damage":[1000,2000],"terminal_claim":true},"qualification":"same unchanged actual Manor map, distinct real cells/conditions; unit services not rendered/earned/durable acceptance"})).unwrap()).unwrap();
}

#[test]
fn xml_runtime_world_passive_void_refuses_wrong_category_program_and_catalog_before_ids_or_dynamic_stamp()
 {
    let base = crate::world::test_content();
    for tamper in [
        "active",
        "nonstatic",
        "class",
        "program",
        "order",
        "transition",
    ] {
        let mut content = base.clone();
        let game = Arc::make_mut(content.game.as_mut().unwrap());
        match tamper {
            "active" => {
                game.definitions.get_mut(&0x181a).unwrap().disposition = Disposition::Active
            }
            "nonstatic" => content.objects.get_mut(&0x181a).unwrap().static_object = false,
            "class" => content.objects.get_mut(&0x181a).unwrap().class = "GameObject".into(),
            "program" | "order" | "transition" => {
                let donor = game
                    .definitions
                    .values()
                    .flat_map(|d| &d.states)
                    .find(|s| match tamper {
                        "order" => !s.orders.is_empty(),
                        "transition" => !s.transitions.is_empty(),
                        _ => !s.behaviors.is_empty(),
                    })
                    .unwrap()
                    .clone();
                let target = &mut game.definitions.get_mut(&0x181a).unwrap().states[0];
                match tamper {
                    "order" => target.orders = donor.orders,
                    "transition" => target.transitions = donor.transitions,
                    _ => target.behaviors = donor.behaviors,
                }
            }
            _ => unreachable!(),
        }
        let mut map = flat_map(128);
        map.cells[0].ground = 255;
        map.cells[0].object = Some(0x181a);
        let ids = Arc::new(AtomicI32::new(7000));
        assert!(
            World::with_ids_context(
                Arc::new(content),
                Arc::new(map),
                ids.clone(),
                Some(WorldDomain::NonRealm)
            )
            .is_err(),
            "{tamper}"
        );
        assert_eq!(ids.load(Ordering::Relaxed), 7000);
    }
    let mut w = domain_world(Some(WorldDomain::NonRealm), None, None).unwrap();
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    let game = w.content.game.as_ref().unwrap().clone();
    Arc::make_mut(&mut w.content).game = Some(Arc::new(game.as_ref().clone()));
    assert!(
        format!(
            "{:#}",
            w.spawn_entity(0x181a, Pos { x: 64.5, y: 64.5 }, vec![])
                .unwrap_err()
        )
        .contains("owning catalog changed")
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
}

fn key_owner(key: u16) -> (World, i32) {
    let mut w = domain_world(Some(WorldDomain::NonRealm), None, None).unwrap();
    w.name = "Nexus".into();
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    ch.name = "Ada".into();
    ch.items[4] = i32::from(key);
    let p = w.enter(ch, None).unwrap();
    (w, p)
}
fn item_plan(w: &mut World, p: i32) -> Result<InventoryPlan> {
    w.use_item(
        p,
        Slot {
            object: p,
            slot: 4,
            kind: w.players[&p].character.items[4],
        },
        w.players[&p].pos,
    )?
    .context("consumable inventory plan absent")
}
fn chamber_incantation() -> (Directory, i32, i32) {
    let (mut w, p) = key_owner(0x722);
    w.name = "Renamed Chamber".into();
    w.xml.template = Some(crate::endgame::CHAMBER.into());
    w.cleared = true;
    let locked = w
        .spawn_clear_portal(0x721, w.players[&p].pos, true)
        .unwrap();
    frame(&mut w, 0);
    assert_eq!(w.xml_portal_expiry(locked), Some(120_000));
    let mut d = super::admission_tests::isolated_directory(w.content.clone());
    d.ids = w.ids.clone();
    d.clock.set_paused(true);
    d.templates.insert(
        crate::endgame::CELLAR.into(),
        Arc::new(Map::load(Path::new("data/wine-cellar.wmap"), &w.content).unwrap()),
    );
    d.worlds.insert(-2, w);
    (d, p, locked)
}

#[test]
fn xml_runtime_world_ocean_key_opens_the_recovered_scripted_portal_with_its_30s_decay() {
    // Admitting Hermit God Drop selects recovered Ocean Trench Portal (objects.xml:639): Decay 30, not a native TTL.
    let (mut w, p) = key_owner(0xc11);
    let plan = item_plan(&mut w, p).unwrap();
    let opened = plan.unlock.unwrap().opened;
    assert_eq!(w.entities[&opened].kind, 0x730);
    let mut d = super::admission_tests::isolated_directory(w.content.clone());
    d.ids = w.ids.clone();
    d.worlds.insert(-2, w);
    let plan = d.stage_unlock(plan).unwrap();
    let destination = d.portals[&opened].destination.unwrap();
    assert_eq!(d.worlds[&destination].name, "Ocean Trench");
    assert!(d.worlds[&destination].map.elevation.is_none());
    assert_eq!(
        d.worlds[&destination].xml.domain,
        Some(WorldDomain::NonRealm)
    );
    let revision = plan.revision + 1;
    let replay = plan.clone();
    d.finish_inventory(plan, Ok(revision)).unwrap();
    assert_eq!(d.worlds[&-2].players[&p].character.items[4], -1);
    assert_eq!(d.portals[&opened].expires, None);
    assert!(d.finish_inventory(replay, Ok(revision)).is_err());
    let w = d.worlds.get_mut(&-2).unwrap();
    assert!(w.xml.actors.contains_key(&opened));
    let at = w.now;
    assert_eq!(w.xml_portal_expiry(opened), Some(at + 30_000));
    frame(w, at + 29_950);
    assert!(w.entities.contains_key(&opened));
    frame(w, at + 30_000);
    assert!(!w.entities.contains_key(&opened));
}

#[test]
fn xml_runtime_world_current_incantation_keeps_scripted_locked_clock_and_native_opened_commit_abort_replay()
 {
    for commit in [false, true] {
        let (mut d, p, locked) = chamber_incantation();
        let w = d.worlds.get_mut(&-2).unwrap();
        frame(w, 35_000);
        let plan = item_plan(w, p).unwrap();
        let opened = plan.unlock.unwrap().opened;
        assert!(w.entities[&locked].reserved && w.entities[&locked].published);
        assert_eq!(w.xml_portal_expiry(locked), Some(120_000));
        assert_eq!(w.entities[&opened].kind, 0x700);
        assert!(!w.xml.actors.contains_key(&opened));
        if !commit {
            frame(d.worlds.get_mut(&-2).unwrap(), 120_000);
            assert!(
                d.stage_unlock(plan).is_err(),
                "expired pre-save staging is known not committed"
            );
            let w = d.worlds.get_mut(&-2).unwrap();
            assert_eq!(w.players[&p].character.items[4], 0x722);
            assert!(!w.entities.contains_key(&opened));
            assert!(!w.entities[&locked].reserved);
            assert_eq!(w.xml_portal_expiry(locked), Some(120_000));
            frame(w, 120_050);
            assert!(!w.entities.contains_key(&locked));
            continue;
        }
        let plan = d.stage_unlock(plan).unwrap();
        let replay = plan.clone();
        let revision = plan.revision + 1;
        frame(d.worlds.get_mut(&-2).unwrap(), 119_999);
        assert_eq!(d.worlds[&-2].xml_portal_expiry(locked), Some(120_000));
        if commit {
            d.finish_inventory(plan, Ok(revision)).unwrap();
            let w = &d.worlds[&-2];
            assert!(!w.entities.contains_key(&locked));
            assert!(w.entities[&opened].published && !w.entities[&opened].reserved);
            assert_eq!(w.players[&p].character.items[4], -1);
            assert!(!w.xml.actors.contains_key(&opened));
            assert_eq!(d.portals[&opened].expires, Some(2400));
            assert!(d.finish_inventory(replay, Ok(revision)).is_err());
        }
    }
}

#[test]
fn xml_runtime_world_published_locked_portal_expiry_during_incantation_pending_retains_source_and_reports_phase4_commit_gap()
 {
    let (mut d, p, locked) = chamber_incantation();
    let plan = item_plan(d.worlds.get_mut(&-2).unwrap(), p).unwrap();
    let opened = plan.unlock.unwrap().opened;
    let plan = d.stage_unlock(plan).unwrap();
    let revision = plan.revision + 1;
    let w = d.worlds.get_mut(&-2).unwrap();
    frame(w, 120_000);
    assert!(w.entities.contains_key(&locked));
    assert!(w.xml_terminal_pending(locked));
    assert!(!w.xml.claims.contains(&locked));
    let terminal = w.xml.terminal_attempts.last().unwrap().clone();
    let error = format!("{:#}", w.terminal(terminal.origin, &terminal).unwrap_err());
    assert!(
        error.contains("phase4") && error.contains("reservation"),
        "{error}"
    );
    assert!(
        d.finish_inventory(plan.clone(), Ok(revision)).is_err(),
        "expired source cannot authorize publication; external COMMIT reconciliation requires Phase4"
    );
    assert_eq!(d.worlds[&-2].players[&p].character.items[4], 0x722);
    assert!(!d.worlds[&-2].entities[&opened].published);
    assert!(
        d.finish_inventory(plan, Err(anyhow::anyhow!("timeout: COMMIT status unknown")))
            .is_err()
    );
    let w = d.worlds.get_mut(&-2).unwrap();
    assert_eq!(
        w.players[&p].character.items[4], 0x722,
        "in-memory state only, not a refund or durable assertion"
    );
    assert!(w.entities[&opened].reserved && !w.entities[&opened].published);
    assert!(w.entities[&locked].reserved);
    assert!(w.players[&p].pending && w.players[&p].persistence_failure.is_some());
    assert_eq!(w.xml_portal_expiry(locked), Some(120_000));
    frame(w, 120_050);
    assert!(w.entities.contains_key(&locked));
    assert!(!w.xml.claims.contains(&locked));
    std::fs::write(Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("world-locked-portal-pending-expiry.json"),serde_json::to_vec_pretty(&serde_json::json!({"locked":0x721,"source_decay_ms":120000,"opened":0x700,"native_actor":false,"expiry_attempt":120000,"error":error,"unknown_commit_retains_reservations_original_deadline_and_quarantines":true,"unknown_commit":"Native unit refuses publication after source expiry. External durable COMMIT may already have consumed Incantation; no durable no-item-loss claim. Phase4 must reconcile lifetime/reservation/publication atomically."})).unwrap()).unwrap();
}

#[test]
fn xml_runtime_world_actual_sprite_and_abyss_factory_source_shoot_and_movement_continue_without_realm_altitude()
 {
    let mut receipts = vec![];
    for (template, kind, movement, shots) in [
        ("sprite-world", 0xd02, K::Wander, 4),
        ("abyss-of-demons", 0x651, K::Follow, 3),
    ] {
        let content = Arc::new(crate::world::test_content());
        let mut d = super::admission_tests::isolated_directory(content);
        let world = d
            .instance(template, None, Some(DungeonReturn::Random))
            .unwrap();
        let w = d.worlds.get_mut(&world).unwrap();
        assert_eq!(w.xml.domain, Some(WorldDomain::NonRealm));
        assert!(w.map.elevation.is_none());
        let (id, pos, target) = w
            .entities
            .values()
            .filter(|e| e.kind == kind)
            .find_map(|e| {
                (0..32)
                    .map(|n| point(e.pos, n as f64 * std::f64::consts::TAU / 32., 8.))
                    .find(|p| w.walkable(*p))
                    .map(|p| (e.id, e.pos, p))
            })
            .expect("actual source actor with walkable target in its own generated world");
        let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
        ch.id = 1;
        ch.account = 1;
        ch.name = "Ada".into();
        let p = w.enter(ch, None).unwrap();
        w.players.get_mut(&p).unwrap().pos = target;
        frame(w, 0);
        frame(w, 50);
        assert!(
            w.entities[&id].pos.distance(pos) > 0.,
            "{template} source movement did not continue"
        );
        let motion = w
            .xml
            .motions
            .iter()
            .find(|(t, _)| t.scope.identity.actor == id)
            .unwrap();
        let definition = &w.xml_game().definitions[&kind];
        let b = definition
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .find(|b| b.local == motion.0.local)
            .unwrap();
        assert_eq!(b.kind, movement);
        assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), shots);
        receipts.push(serde_json::json!({"template":template,"world_domain":"NonRealm","height":false,"kind":kind,"source":definition.source,"movement":b.kind,"position_before":[pos.x,pos.y],"position_after":[w.entities[&id].pos.x,w.entities[&id].pos.y],"shots":shots,"qualification":"ordinary factory and authoritative unit services, prepared fixture player position; not rendered or earned gameplay"}));
    }
    std::fs::write(
        Path::new(&std::env::var("QA_RESULTS_DIR").unwrap())
            .join("world-stayabove-factory-source-context.json"),
        serde_json::to_vec_pretty(&receipts).unwrap(),
    )
    .unwrap();
}

#[test]
fn xml_runtime_world_realm_stayabove_actual_threshold_speed_and_zero_still_require_real_height_with_shadow_grants()
 {
    let mut w = domain_world(Some(WorldDomain::Realm), Some(114), Some(0xd02)).unwrap();
    let id = *w.entities.keys().next().unwrap();
    let b = w.xml_game().definitions[&0xd02].states[0]
        .behaviors
        .iter()
        .find(|b| b.kind == K::StayAbove)
        .unwrap()
        .clone();
    Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap())[64 * 128 + 65] = 115;
    let mut st = step(&w, id, b.local);
    st.delta_ms = 50;
    let mut rng = rand::rngs::StdRng::seed_from_u64(51);
    assert!(w.ready(Service::Movement, &b, st).unwrap());
    assert_eq!(
        w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
        ActionOutcome::Applied
    );
    assert!((w.entities[&id].pos.x - 64.525).abs() < 0.0001);
    Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap()).fill(115);
    assert!(!w.ready(Service::Movement, &b, st).unwrap());
    let (mut w, _, id) = fixture(
        r#"<Behavior minAltitude="0">StayAbove</Behavior><Behavior terrain="Dark Water">StayInTerrain</Behavior><Behavior>Shadow</Behavior>"#,
    );
    Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = 0xbc;
    assert!(
        w.xml_npc_walkable(scope(&w, id), Pos { x: 65.5, y: 64.5 })
            .unwrap()
    );
    Arc::make_mut(&mut w.map).elevation = None;
    assert!(
        w.xml_teleport_endpoint(scope(&w, id), Pos { x: 65.5, y: 64.5 })
            .is_err()
    );
    let ids = w.ids.load(Ordering::Relaxed);
    assert!(
        w.spawn_entity(0x600, Pos { x: 64.5, y: 64.5 }, vec![])
            .is_err()
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
}
