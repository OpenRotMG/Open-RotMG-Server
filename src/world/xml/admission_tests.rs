use super::*;
use crate::assert_matches;
use crate::content::game::{AdmissionRole, DeploymentKind, NativeService};
use rand::RngCore;

fn native_world(class: u16, item: u16) -> (World, i32) {
    let content = Arc::new(crate::world::test_content());
    let mut map = flat_map(128);
    map.elevation = Some(vec![200; 128 * 128].into());
    let mut w = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    w.clock.set_paused(true);
    let mut ch = Character::fresh(w.content.get(class).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    ch.name = "Ada".into();
    ch.base[1] = w.content.classes[&class].caps[1];
    ch.mp = ch.base[1];
    ch.items[1] = i32::from(item);
    let p = w.enter(ch, None).unwrap();
    (w, p)
}
fn cast(w: &mut World, p: i32, pos: Pos) -> Result<Option<InventoryPlan>> {
    w.use_item(
        p,
        Slot {
            object: p,
            slot: 1,
            kind: w.players[&p].character.items[1],
        },
        pos,
    )
}

#[test]
fn xml_runtime_world_raw_and_root_omissions_refuse_before_ids_rng_or_publication() {
    for kind in [0x1c7, 0x173, 0xd7d] {
        let (mut w, p, _) = fixture("<Behavior>RemainVisible</Behavior>");
        let mut rx = output(&mut w, p);
        let ids = w.ids.load(Ordering::Relaxed);
        let rng = w.rng.clone().next_u64();
        let count = w.entities.len();
        let error = format!(
            "{:#}",
            w.spawn_entity(kind, Pos { x: 66.5, y: 64.5 }, vec![])
                .unwrap_err()
        );
        assert!(
            error.contains("missing explicit admission definition")
                && error.contains(&format!("{kind:#x}"))
                && error.contains("world="),
            "{error}"
        );
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.rng.clone().next_u64(), rng);
        assert_eq!(w.entities.len(), count);
        assert!(drain(&mut rx).is_empty());
        let mut map = flat_map(128);
        map.cells[1].object = Some(0x120);
        map.cells[2].object = Some(kind);
        let shared = Arc::new(AtomicI32::new(5000));
        assert!(
            World::with_ids_context(
                w.content.clone(),
                Arc::new(map),
                shared.clone(),
                Some(WorldDomain::Realm)
            )
            .is_err()
        );
        assert_eq!(shared.load(Ordering::Relaxed), 5000);
    }
}

#[test]
fn xml_runtime_world_native_entity_roles_and_actual_companion_owner_reservation() {
    let (mut w, p) = native_world(0x324, 0xae3);
    let pos = w.players[&p].pos;
    let before = w.ids.load(Ordering::Relaxed);
    for kind in [
        0xc2b, 0x307, 6, 0x711, 0x715, 0x1409, 0x1407, 0x504, 0x712, 0x1cb,
    ] {
        assert!(
            w.spawn_entity(kind, pos, vec![]).is_err(),
            "raw native bypass {kind:#x}"
        );
        assert_eq!(w.ids.load(Ordering::Relaxed), before);
    }
    let wall = spawn(&mut w, 0x120, 70.5, 64.5);
    assert_eq!(w.entities[&wall].kind, 0x120);
    w.players.get_mut(&p).unwrap().character.items[4] = 0xc2b;
    let plan = w
        .use_item(
            p,
            Slot {
                object: p,
                slot: 4,
                kind: 0xc2b,
            },
            pos,
        )
        .unwrap()
        .unwrap();
    let pet = w.entities.values().find(|e| e.kind == 0x1407).unwrap().id;
    assert!(
        w.companions.contains(&pet) && !w.entities[&pet].published && w.entities[&pet].reserved
    );
    assert_eq!(w.players[&p].character.items[4], 0xc2b);
    let revision = plan.revision + 1;
    w.finish_inventory(plan, Ok(revision)).unwrap();
    assert!(w.entities[&pet].published && !w.entities[&pet].reserved);
    assert_eq!(w.players[&p].character.items[4], -1);
    w.clear_companions(p);
    assert!(!w.entities.contains_key(&pet));
}

#[test]
fn xml_runtime_world_composite_prism_preflight_preserves_spending_rng_and_source_order() {
    let (mut w, p) = native_world(0x324, 0xc2a);
    let origin = w.players[&p].pos;
    let target = Pos {
        x: origin.x + 4.,
        y: origin.y,
    };
    let cell = target.y as usize * 128 + target.x as usize;
    Arc::make_mut(&mut w.map).cells[cell].ground = 255;
    let rng = w.rng.clone().next_u64();
    let ids = w.ids.load(Ordering::Relaxed);
    let mana = w.players[&p].character.mp;
    assert!(cast(&mut w, p, target).is_err());
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.players[&p].character.mp, mana);
    assert_eq!(w.players[&p].ability_ready_ms, 0);
    assert!(w.entities.is_empty());
    Arc::make_mut(&mut w.map).cells[cell].ground = 0x18;
    cast(&mut w, p, target).unwrap();
    let bomb = w.entities.values().find(|e| e.kind == 0x1409).unwrap();
    assert_eq!(bomb.pos.distance(origin), 0.);
    assert_eq!(w.players[&p].pos.distance(target), 0.);
    assert_eq!(
        w.players[&p].character.mp,
        mana - w.content.objects[&0xc2a].mana_cost
    );
    assert_eq!(w.ability_state.objects.len(), 1);
    assert!(cast(&mut w, p, target).is_err());
    assert_eq!(w.entities.len(), 1);
    let enemy = w
        .spawn_entity(
            0x678,
            Pos {
                x: origin.x + 3.,
                y: origin.y,
            },
            vec![],
        )
        .unwrap();
    let hp = w.entities[&enemy].hp;
    let ids = w.ids.load(Ordering::Relaxed);
    w.now = 2999;
    w.ability_step().unwrap();
    assert_eq!(w.entities[&enemy].hp, hp);
    assert_eq!(w.ability_state.objects.len(), 1);
    w.now = 3000;
    w.ability_step().unwrap();
    let after = w.entities[&enemy].hp;
    assert_eq!(
        hp - after,
        (300 - w.content.objects[&0x678].defense).max(45)
    );
    assert!(w.ability_state.objects.is_empty());
    assert_eq!(w.entities.len(), 1);
    w.ability_step().unwrap();
    assert_eq!(w.entities[&enemy].hp, after);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(
        w.players[&p].character.mp,
        mana - w.content.objects[&0xc2a].mana_cost
    );
}

#[test]
fn xml_runtime_world_delayed_trap_retains_actual_cast_and_rechecks_catalog_owner_and_geometry() {
    let (mut w, p) = native_world(0x322, 0xab1);
    let target = Pos { x: 69.5, y: 64.5 };
    let mana = w.players[&p].character.mp;
    cast(&mut w, p, target).unwrap();
    let after = w.ids.load(Ordering::Relaxed);
    assert_eq!(w.pending_deployments(), 1);
    assert!(w.entities.is_empty());
    w.players.get_mut(&p).unwrap().character.items[1] = -1;
    let i = 64 * 128 + 69;
    Arc::make_mut(&mut w.map).cells[i].ground = 255;
    w.now = 1500;
    w.ability_step().unwrap();
    assert_eq!(w.pending_deployments(), 1);
    assert!(w.entities.is_empty());
    Arc::make_mut(&mut w.map).cells[i].ground = 0x18;
    let game = w.content.game.as_ref().unwrap().clone();
    Arc::make_mut(&mut w.content).game = Some(Arc::new(game.as_ref().clone()));
    assert!(format!("{:#}", w.ability_step().unwrap_err()).contains("catalog changed"));
    assert_eq!(w.pending_deployments(), 1);
    Arc::make_mut(&mut w.content).game = Some(game);
    w.ability_step().unwrap();
    assert_eq!(w.pending_deployments(), 0);
    assert_eq!(w.entities.values().next().unwrap().kind, 0x711);
    assert_eq!(w.ids.load(Ordering::Relaxed), after);
    assert_eq!(
        w.players[&p].character.mp,
        mana - w.content.objects[&0xab1].mana_cost
    );
    w.ability_step().unwrap();
    assert_eq!(w.entities.len(), 1);
    w.clear_abilities(p);
    assert!(w.entities.is_empty());
}

fn bound_trap_world(target: u16) -> (World, i32) {
    let mut c = Content::load(Path::new("data")).unwrap();
    let mut declarations = String::new();
    for (kind, service) in [(target, "trap"), (0xab1, "inventory"), (0x322, "player")] {
        let name = &c.objects[&kind].name;
        declarations += &format!(
            r#"<Object type="{kind}" id="{name}" disposition="native" nativeService="{service}" provenance="authored" origin="XML-bound native World mutation fixture" reason="actual selected equipped Trap activation"/>"#
        );
    }
    let inputs = BTreeMap::from([
        (
            "game/123.5.1/game.xml".into(),
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Hunting Trap"/></Game>"#
                .to_vec(),
        ),
        (
            "game/123.5.1/rules.xml".into(),
            format!("<Objects>{declarations}</Objects>").into_bytes(),
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
    let mut w = World::in_domain(Arc::new(c), Arc::new(flat_map(128)), WorldDomain::Realm).unwrap();
    let mut ch = Character::fresh(w.content.get(0x322).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    ch.mp = 100;
    ch.items[1] = 0xab1;
    let p = w.enter(ch, None).unwrap();
    (w, p)
}

#[test]
fn xml_runtime_world_xml_binding_changes_real_trap_birth_not_item_or_binary() {
    let mut observations = vec![];
    for target in [0x711, 0x1407] {
        let (mut w, p) = bound_trap_world(target);
        let pos = w.players[&p].pos;
        let canonical = &w.xml_game().properties[&0xab1];
        assert!(
            w.xml_game()
                .deployment_admission(
                    target,
                    &canonical.clone(),
                    0,
                    DeploymentKind::Trap,
                    "cloned metadata is not proof"
                )
                .is_err()
        );
        assert!(
            w.xml_game()
                .entity_admission(
                    target,
                    AdmissionRole::Native(NativeService::Deployment(DeploymentKind::Trap)),
                    "ordinary native bypass"
                )
                .is_err()
        );
        cast(&mut w, p, pos).unwrap();
        w.now = 1500;
        w.ability_step().unwrap();
        assert_eq!(w.entities.values().next().unwrap().kind, target);
        assert_eq!(w.players[&p].character.mp, 60);
        observations.push(serde_json::json!({"XML_binding":target,"actual_birth":w.entities.values().next().unwrap().kind,"remaining_mana":60,"qualification":"same-unit-executable native service outcome, not rendered gameplay"}));
    }
    std::fs::write(
        Path::new(&std::env::var("QA_RESULTS_DIR").unwrap())
            .join("xml-runtime-world-deployment-mutation.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
}

#[test]
fn xml_runtime_world_initial_map_actor_and_wire_bounds_refuse_before_ids() {
    let (w, _, _) = fixture("<Behavior>RemainVisible</Behavior>");
    let mut map = flat_map((ACTOR_LIMIT + 1).isqrt() + 1);
    for cell in map.cells.iter_mut().take(ACTOR_LIMIT + 1) {
        cell.object = Some(0x600);
    }
    let ids = Arc::new(AtomicI32::new(5000));
    let error = format!(
        "{:#}",
        World::with_ids_context(
            w.content.clone(),
            Arc::new(map),
            ids.clone(),
            Some(WorldDomain::Realm)
        )
        .err()
        .unwrap()
    );
    assert!(error.contains("initial map actor capacity exceeded"));
    assert_eq!(ids.load(Ordering::Relaxed), 5000);
    let mut map = flat_map(1);
    map.width = 32768;
    map.cells.resize(32768, map.cells[0].clone());
    assert!(
        World::with_ids_context(
            w.content.clone(),
            Arc::new(map),
            ids.clone(),
            Some(WorldDomain::Realm)
        )
        .is_err()
    );
    assert_eq!(ids.load(Ordering::Relaxed), 5000);
}

#[test]
fn xml_runtime_world_template_native_service_without_factory_scope_refuses_startup() {
    let mut c = Content::load(Path::new("data")).unwrap();
    let merchant = c.objects.values().find(|o| o.class == "Merchant").unwrap();
    let (kind, name) = (merchant.kind, merchant.name.clone());
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Behavior map="Stamp">PlaceMap</Behavior>"#,
    ) + &format!(
        r#"<Object type="{kind}" id="{name}" disposition="native" nativeService="merchant" provenance="authored" origin="actual selected native Merchant fixture" reason="unsupported stamp sale factory negative"/><Object type="6" id="Blade" disposition="native" nativeService="projectile" provenance="authored" origin="actual selected Pirate dependency" reason="explicit property-only fixture edge"/>"#
    );
    let inputs = BTreeMap::from([
        ("game/123.5.1/game.xml".into(), br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/><Geometry file="geometry.xml"/></Game>"#.to_vec()),
        ("game/123.5.1/rules.xml".into(), format!("<Objects>{rules}</Objects>").into_bytes()),
        ("game/123.5.1/geometry.xml".into(), format!(r#"<Geometry schema="1"><Template id="Stamp" provenance="authored" origin="native negative fixture" reason="no sale/stock factory in a stamp" width="1" height="1" rotation="fixed" rooms="1"><Anchor x="0" y="0"/><CharMap char="x" object="{name}"/><Pattern><Row value="x"/></Pattern></Template></Geometry>"#).into_bytes()),
        ("objects.xml".into(), std::fs::read("data/objects.xml").unwrap()),
        ("grounds.xml".into(), std::fs::read("data/grounds.xml").unwrap()),
    ]);
    c.game = Some(Arc::new(game::compile_fixture(&c, inputs).unwrap()));
    let ids = Arc::new(AtomicI32::new(5000));
    let error = format!(
        "{:#}",
        World::with_ids_context(
            Arc::new(c),
            Arc::new(flat_map(128)),
            ids.clone(),
            Some(WorldDomain::Realm)
        )
        .err()
        .unwrap()
    );
    assert!(
        error.contains("native service factory/owner scope") && error.contains("PlaceMap=Stamp")
    );
    assert_eq!(ids.load(Ordering::Relaxed), 5000);
}

#[test]
fn xml_runtime_world_delayed_trap_expiry_and_changed_character_refuse_without_rebirth_or_spend() {
    for changed_character in [false, true] {
        let (mut w, p) = bound_trap_world(0x711);
        cast(&mut w, p, Pos { x: 69.5, y: 64.5 }).unwrap();
        let ids = w.ids.load(Ordering::Relaxed);
        let mana = w.players[&p].character.mp;
        if changed_character {
            w.players.get_mut(&p).unwrap().character.id += 1;
            w.now = 1500;
        } else {
            w.now = 11_500;
        }
        for attempt in 0..2 {
            if changed_character || attempt == 0 {
                let error = format!("{:#}", w.ability_step().unwrap_err());
                assert!(
                    error.contains(if changed_character {
                        "owner/cast reservation changed"
                    } else {
                        "reservation expired and released"
                    }),
                    "{error}"
                );
            } else {
                w.ability_step().unwrap();
            }
            assert!(w.entities.is_empty());
            assert_eq!(w.pending_deployments(), usize::from(changed_character));
            assert_eq!(w.ids.load(Ordering::Relaxed), ids);
            assert_eq!(w.players[&p].character.mp, mana);
        }
        w.clear_abilities(p);
        assert_eq!(w.pending_deployments(), 0);
    }
}

#[test]
fn xml_runtime_world_terminal_portal_source_plans_require_coordinator_without_consuming_any_birth()
{
    use crate::behavior::{DeathHook, Rewards};
    let content = Arc::new(crate::world::test_content());
    let game = content.game.as_ref().unwrap();
    let sources: Vec<_> = game
        .definitions
        .values()
        .flat_map(|d| {
            d.states
                .iter()
                .flat_map(|s| &s.behaviors)
                .filter(|b| matches!(b.kind, K::Metamorphose | K::MetamorphoseAtDeath))
                .filter(|b| {
                    objects(b, F::ChildId).iter().any(|kind| {
                        game.definitions[kind].native_service == Some(NativeService::Portal)
                    })
                })
                .map(|b| (d.kind, b.clone()))
        })
        .collect();
    assert_eq!(sources.len(), 6);
    assert_eq!(
        sources
            .iter()
            .map(|(_, b)| objects(b, F::ChildId)[0])
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0x704, 0x70e, 0xd7b])
    );
    let mut evidence = vec![];
    for (kind, b) in sources {
        let mut map = flat_map(128);
        map.elevation = Some(vec![200; 128 * 128].into());
        let mut w = World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
        let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
        ch.account = 1;
        ch.id = 1;
        let p = w.enter(ch, None).unwrap();
        let mut rx = output(&mut w, p);
        let id = spawn(&mut w, kind, 64.5, 64.5);
        w.entities.get_mut(&id).unwrap().damage.insert(1, 17);
        let origin = scope(&w, id);
        let plan = TerminalPlan {
            origin,
            context: Some(TerminalContext::NonPlayer),
            announcements: vec![],
            reason: if b.kind == K::Metamorphose {
                TerminalReason::Metamorphose
            } else {
                TerminalReason::Defeated
            },
            rewards: Rewards {
                loot: true,
                xp: true,
            },
            behavior: (b.kind == K::Metamorphose).then(|| b.clone()),
            hooks: if b.kind == K::MetamorphoseAtDeath {
                vec![DeathHook {
                    owner: step(&w, id, b.local).token,
                    behavior: b.clone(),
                    seed: 77,
                }]
            } else {
                vec![]
            },
        };
        let ids = w.ids.load(Ordering::Relaxed);
        let mut after_seed = w.rng.clone();
        after_seed.next_u64();
        let rng = after_seed.next_u64();
        let before = format!("{:?}", plan);
        for _ in 0..2 {
            let error = w
                .xml_terminal(&plan)
                .unwrap_err()
                .context("lifecycle source");
            assert_eq!(terminal::pending_level(&error), tracing::Level::DEBUG);
            assert_eq!(
                terminal::pending_level(&anyhow::anyhow!("other").context("wrapped")),
                tracing::Level::ERROR
            );
            let error = format!("{error:#}");
            assert!(error.contains("awaiting durable receipt"), "{error}");
            assert_eq!(w.entities.len(), 1);
            assert_eq!(w.entities[&id].damage[&1], 17);
            assert_eq!(w.ids.load(Ordering::Relaxed), ids);
            assert_eq!(w.rng.clone().next_u64(), rng);
            assert!(w.xml.claims.is_empty() && w.xml.rewards.is_empty());
            assert!(w.xml.retry_terminals.contains(&id));
            assert!(drain(&mut rx).is_empty());
        }
        assert_eq!(before, format!("{:?}", w.xml.terminal_attempts[0]));
        assert_eq!(before, format!("{:?}", w.xml.terminal_attempts[1]));
        drain_events_plan(&mut w, Some(&plan)).unwrap();
        assert!(!w.entities.contains_key(&id));
        let target = objects(&b, F::ChildId)[0];
        assert_eq!(
            w.entities
                .values()
                .filter(|entity| entity.kind == target && entity.published)
                .count(),
            1
        );
        let portal = w
            .entities
            .values()
            .find(|entity| entity.kind == target)
            .unwrap()
            .id;
        assert!(w.xml.portals.contains_key(&portal));
        evidence.push(serde_json::json!({"kind":kind,"source":b.source,"portal":objects(&b,F::ChildId)[0],"refusals":2,"original_retained":true,"qualification":"source-qualified service plan; not rendered/usable portal or Phase4 acceptance"}));
    }
    std::fs::write(
        Path::new(&std::env::var("QA_RESULTS_DIR").unwrap())
            .join("world-terminal-portal-coordinator-boundary.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}

#[test]
fn xml_runtime_world_ordinary_native_services_cannot_bypass_their_factories_through_child_ingress()
{
    let (w, _) = native_world(0x322, 0xab1);
    for kind in [0x712, 0x504, 0x1cb, 0x1ca, 0x1407, 0x711, 6, 0xc2b, 0x307] {
        let ids = w.ids.load(Ordering::Relaxed);
        let error = format!("{:#}", w.xml_preflight_kind(kind).unwrap_err());
        assert!(
            error.contains("actual native service factory/owner scope"),
            "{kind:#x}: {error}"
        );
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    }
    w.xml_preflight_kind(0x120).unwrap();
    w.xml_preflight_kind(0x1c7).unwrap();
}

#[test]
fn xml_runtime_world_terminal_portal_actor_retries_cached_plan_without_retirement_or_publication() {
    let (mut w, p) = native_world(0x322, 0xab1);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 65.5, y: 64.5 };
    let id = spawn(&mut w, 0x927, 64.5, 64.5);
    frame(&mut w, 0);
    let mut rx = output(&mut w, p);
    let ids = w.ids.load(Ordering::Relaxed);
    let hp = w.entities[&id].hp;
    for _ in 0..2 {
        w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
        assert!(!w.pending_terminal_events().is_empty());
        assert_eq!(w.entities.len(), 1);
        assert_eq!(w.entities[&id].hp, hp);
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert!(w.xml.retry_terminals.contains(&id));
        assert!(w.xml.actors.contains_key(&id));
        assert!(w.xml.claims.is_empty() && w.xml.rewards.is_empty());
        assert!(drain(&mut rx).is_empty());
    }
    assert_eq!(w.xml.terminal_attempts.len(), 2);
    assert_eq!(
        format!("{:?}", w.xml.terminal_attempts[0]),
        format!("{:?}", w.xml.terminal_attempts[1])
    );
    drain_events(&mut w).unwrap();
    assert!(!w.entities.contains_key(&id));
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.kind == 0x704 && e.published)
            .count(),
        1
    );
}

#[test]
fn xml_runtime_world_actual_native_scene_factories_keep_catalog_roles_and_private_container_ownership()
 {
    let content = Arc::new(crate::world::test_content());
    let mut services = BTreeSet::new();
    let mut interactions = BTreeSet::new();
    for name in [
        "nexus",
        "nexus-limbo",
        "guild-hall-1",
        "guild-hall-2",
        "guild-hall-3",
        "guild-hall-4",
        "vault",
    ] {
        let map = Map::load(&Path::new("data").join(format!("{name}.wmap")), &content).unwrap();
        let w = World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
        for e in w.entities.values() {
            let d = &w.xml_game().definitions[&e.kind];
            if let Some(service) = d.native_service {
                services.insert(format!("{service:?}"));
                assert!(!w.xml.actors.contains_key(&e.id) && !w.xml.unstarted.contains(&e.id));
                let property = &w.content.objects[&e.kind];
                if service == NativeService::Interaction {
                    interactions.insert(property.class.clone());
                }
                if property.class == "Merchant" {
                    assert!(w.merchants.contains_key(&e.id));
                }
            }
        }
    }
    assert_eq!(
        interactions,
        BTreeSet::from(
            [
                "MoneyChanger",
                "CharacterChanger",
                "NameChanger",
                "GuildRegister",
                "GuildChronicle",
                "GuildBoard"
            ]
            .map(str::to_owned)
        )
    );
    for service in ["Scenery", "Portal", "Merchant", "Interaction"] {
        assert!(services.contains(service));
    }
    let (mut w, p) = native_world(0x322, 0xab1);
    let pos = w.players[&p].pos;
    let bag = w.loot_bag(pos, Some(1), vec![0xa00]).unwrap();
    assert!(w.entities[&bag].accessible(1));
    assert!(!w.entities[&bag].accessible(2));
    assert!(!w.xml.actors.contains_key(&bag));
    assert!(w.bag_expiry.contains_key(&bag));
    let mut missing = content.as_ref().clone();
    Arc::make_mut(missing.game.as_mut().unwrap())
        .definitions
        .remove(&0x1ca);
    let map = Map::load(Path::new("data/nexus.wmap"), &content).unwrap();
    let ids = Arc::new(AtomicI32::new(5000));
    assert!(
        World::with_ids_context(
            Arc::new(missing),
            Arc::new(map),
            ids.clone(),
            Some(WorldDomain::Realm)
        )
        .is_err()
    );
    assert_eq!(ids.load(Ordering::Relaxed), 5000);
}

#[test]
fn xml_runtime_world_unsupported_native_field_variants_and_integer_wire_scalars_refuse_before_effects()
 {
    for (kind, field, value) in [
        (K::ObjectToss, F::Damage, Value::Number(1.)),
        (K::ObjectToss, F::Radius, Value::Number(1.)),
        (
            K::ObjectToss,
            F::TextureSourceId,
            Value::Objects(vec![0x600]),
        ),
        (K::PlaySound, F::SoundId, Value::Integer(256)),
        (K::Protect, F::Radius, Value::Number(1.)),
        (K::Shoot, F::AcquireRange, Value::Number(1.)),
        (K::Zap, F::Damage, Value::Number(0.5)),
        (K::HealZap, F::Healing, Value::Number(0.5)),
        (K::Say, F::BubbleTime, Value::Millis(256_000)),
        (K::Say, F::BubbleTime, Value::Millis(255_001)),
        (K::ShowEffect, F::FlashRepeats, Value::Integer(16_777_217)),
    ] {
        let (mut w, p, id) = fixture("<Behavior>RemainVisible</Behavior>");
        let mut rx = output(&mut w, p);
        let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        b.kind = kind;
        b.fields.insert(field, value);
        let ids = w.ids.load(Ordering::Relaxed);
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let before = rng.clone().next_u64();
        let error = format!(
            "{:#}",
            w.apply(Service::Visual, &b, step(&w, id, b.local), &mut rng)
                .unwrap_err()
        );
        assert!(
            error.contains(&b.source.file) && error.contains(&b.source.object),
            "{error}"
        );
        if field == F::SoundId {
            assert!(error.contains("native byte"), "{error}");
        }
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(rng.next_u64(), before);
        assert!(w.xml.births.is_empty() && w.bullets.is_empty());
        assert!(drain(&mut rx).is_empty());
    }
    let (mut w, _, id) = fixture(r#"<Behavior protecteeId="Piratess">Orbit</Behavior>"#);
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    b.fields.insert(F::SpeedVariability, Value::Number(1.5));
    let mut rng = rand::rngs::StdRng::seed_from_u64(4);
    let before = rng.clone().next_u64();
    assert!(
        format!(
            "{:#}",
            w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
                .unwrap_err()
        )
        .contains("speed envelope before sampling")
    );
    assert_eq!(rng.next_u64(), before);
    assert!(w.xml.motions.is_empty());
}

#[test]
fn xml_runtime_world_native_text_byte_projection_preserves_source_millis_and_refuses_overflow() {
    let (mut w, p, id) = fixture(r#"<Behavior text="Hello">Say</Behavior>"#);
    let mut rx = output(&mut w, p);
    let source = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    for (time, seconds) in [
        (0, 0),
        (1, 1),
        (999, 1),
        (1000, 1),
        (1001, 2),
        (2500, 3),
        (255000, 255),
    ] {
        let mut b = source.clone();
        b.fields.insert(F::BubbleTime, Value::Millis(time));
        native_validate(&b).unwrap();
        let mut st = step(&w, id, b.local);
        st.text_choice = Some(0);
        assert_eq!(
            w.apply(
                Service::Speech,
                &b,
                st,
                &mut rand::rngs::StdRng::seed_from_u64(3)
            )
            .unwrap(),
            ActionOutcome::Applied
        );
        assert_eq!(ms(&b, F::BubbleTime), time);
        let packets = drain(&mut rx);
        let text = packets.iter().find(|p| p.id == wire::TEXT).unwrap();
        let mut r = wire::Reader::new(&text.body);
        r.utf().unwrap();
        assert_eq!(r.i32().unwrap(), id);
        r.i32().unwrap();
        assert_eq!(r.u8().unwrap(), seconds);
        r.utf().unwrap();
        assert_eq!(r.utf().unwrap(), "Hello");
        r.utf().unwrap();
        r.done().unwrap();
    }
    let mut b = source;
    b.fields.insert(F::BubbleTime, Value::Millis(255001));
    let mut st = step(&w, id, b.local);
    st.text_choice = Some(0);
    let error = format!(
        "{:#}",
        w.apply(
            Service::Speech,
            &b,
            st,
            &mut rand::rngs::StdRng::seed_from_u64(3)
        )
        .unwrap_err()
    );
    assert!(
        error.contains("bubbleTime exceeds native") && error.contains(&b.source.file),
        "{error}"
    );
    assert!(drain(&mut rx).is_empty());
    let full = crate::world::test_content();
    let actual = full.game.as_ref().unwrap().definitions[&0xd7e]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == K::Say && ms(b, F::BubbleTime) == 1500)
        .unwrap();
    assert_eq!(bubble_seconds(actual).unwrap(), 2);
    assert_eq!(ms(actual, F::BubbleTime), 1500);
}

#[test]
fn xml_runtime_world_only_actual_map_static_native_scenery_can_occupy_void_without_traversal_permission()
 {
    let content = Arc::new(crate::world::test_content());
    let nexus = Map::load(Path::new("data/nexus.wmap"), &content).unwrap();
    assert_eq!(nexus.cells[4300].object, Some(0x1c5));
    assert!(content.void_grounds.contains(&nexus.cells[4300].ground));
    let pos = Pos {
        x: (4300 % nexus.width as usize) as f32 + 0.5,
        y: (4300 / nexus.width as usize) as f32 + 0.5,
    };
    let w = World::in_domain(content.clone(), Arc::new(nexus), WorldDomain::Realm).unwrap();
    assert!(
        w.entities
            .values()
            .any(|e| e.kind == 0x1c5 && e.pos.distance(pos) == 0.)
    );
    assert!(!w.walkable(pos));
    for kind in [0x600, 0x504, 0x712, 0x1ca, 0x1cb] {
        let mut map = flat_map(128);
        map.cells[0].ground = 255;
        map.cells[0].object = Some(kind);
        let ids = Arc::new(AtomicI32::new(5000));
        let error = format!(
            "{:#}",
            World::with_ids_context(
                content.clone(),
                Arc::new(map),
                ids.clone(),
                Some(WorldDomain::Realm)
            )
            .err()
            .unwrap()
        );
        assert!(error.contains("object on void ground"), "{error}");
        assert_eq!(ids.load(Ordering::Relaxed), 5000);
    }
    let mut map = flat_map(128);
    map.cells[0].ground = 255;
    map.cells[0].object = Some(0x1c5);
    let mut w =
        World::in_domain(content.clone(), Arc::new(map.clone()), WorldDomain::Realm).unwrap();
    assert!(!w.walkable(Pos { x: 0.5, y: 0.5 }));
    assert!(
        w.spawn_entity(0x1c5, Pos { x: 0.5, y: 0.5 }, vec![])
            .is_err()
    );
    assert!(
        !w.xml_placeable(0x1c5, Pos { x: 0.5, y: 0.5 }, &[], None)
            .unwrap()
    );
    let mut nonstatic = content.as_ref().clone();
    nonstatic.objects.get_mut(&0x1c5).unwrap().static_object = false;
    let ids = Arc::new(AtomicI32::new(5000));
    assert!(
        World::with_ids_context(
            Arc::new(nonstatic),
            Arc::new(map),
            ids.clone(),
            Some(WorldDomain::Realm)
        )
        .is_err()
    );
    assert_eq!(ids.load(Ordering::Relaxed), 5000);
}

pub(super) fn isolated_directory(content: Arc<Content>) -> Directory {
    Directory {
        realm_lockouts: BTreeMap::new(),
        clock: crate::clock::Clock::default(),
        worlds: execution::Worlds::new(),
        templates: BTreeMap::from([(
            crate::endgame::CHAMBER.to_owned(),
            Arc::new(Map::load(Path::new("data/oryx-chamber.wmap"), &content).unwrap()),
        )]),
        realms: vec![],
        realm_rng: rand::rngs::StdRng::seed_from_u64(1),
        portal_rng: rand::rngs::StdRng::seed_from_u64(2),
        portal_positions: vec![],
        content: content.clone(),
        ids: Arc::new(AtomicI32::new(1000)),
        grants: BTreeMap::new(),
        portals: BTreeMap::new(),
        terminal_allocations: Arc::default(),
        inventory_events: BTreeMap::new(),
        inventory_capacity: Arc::default(),
        chat_budgets: BTreeMap::new(),
        guild_invites: BTreeMap::new(),
        endgame: endgame::RealmDirectory::default(),
        realm_population: realm_population::Population::default(),
        teleports: BTreeMap::new(),
        idle: BTreeMap::new(),
        next_world: 2,
        tick: 0,
        max_tick_us: 0,
        diagnostic_ticks: Default::default(),
        performance: Default::default(),
        attribution: Default::default(),
        max_players: 16,
        prepared_seed: Some(12351),
    }
}

#[test]
fn xml_runtime_world_directory_does_not_consume_xml_taskmaster_on_legacy_chamber_deadlines() {
    let content = Arc::new(crate::world::test_content());
    let mut d = isolated_directory(content.clone());
    let map = Map::load(Path::new("data/oryx-castle.wmap"), &content).unwrap();
    let mut w = World::with_ids_context(
        content.clone(),
        Arc::new(map),
        d.ids.clone(),
        Some(WorldDomain::Realm),
    )
    .unwrap();
    w.name = "Oryx's Castle".into();
    w.cleared = true;
    let marker = w.entities.values().find(|e| e.kind == 0xd7a).unwrap().id;
    let b = content.game.as_ref().unwrap().definitions[&0xd7a]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == K::Metamorphose)
        .unwrap()
        .clone();
    let plan = TerminalPlan {
        origin: scope(&w, marker),
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
    d.worlds.insert(1, w);
    for pending in [false, true] {
        if pending {
            assert!(d.worlds.get_mut(&1).unwrap().xml_terminal(&plan).is_err());
        }
        for deadline in [0, 100, 5000] {
            d.worlds
                .get_mut(&1)
                .unwrap()
                .entities
                .get_mut(&marker)
                .unwrap()
                .next_aux = deadline;
            for now in [0, 100, 5000, 10000] {
                d.tick = now / 50;
                let ids = d.ids.load(Ordering::Relaxed);
                d.endgame_step(&[1]);
                assert_eq!(d.worlds.len(), 1);
                assert!(d.portals.is_empty() && d.idle.is_empty());
                assert_eq!(d.next_world, 2);
                assert_eq!(d.ids.load(Ordering::Relaxed), ids);
                let w = &d.worlds[&1];
                assert_eq!(w.entities[&marker].next_aux, deadline);
                assert!(!w.entities.values().any(|e| e.kind == 0xd7b));
                assert!(w.xml.claims.is_empty() && w.xml.rewards.is_empty());
            }
        }
    }
}

#[test]
fn xml_runtime_world_reserved_sphinx_and_hermit_start_only_xml_births_after_earned_admission() {
    for (kind, child, expected, announcement) in [
        (
            0xd54,
            0xd55,
            4,
            "At last, a Grand Sphinx will teach you to respect!",
        ),
        (
            0xd61,
            0xd62,
            8,
            "My Hermit God's thousand tentacles shall drag you to a watery grave!",
        ),
    ] {
        let (mut w, p) = native_world(0x322, 0xab1);
        w.players.get_mut(&p).unwrap().pos = Pos { x: 14.5, y: 14.5 };
        let root = spawn(&mut w, kind, 64.5, 64.5);
        w.entities
            .get_mut(&root)
            .unwrap()
            .quest
            .set(Some(root), root, &mut w.quest_ids);
        let entity = w.remove_entity(root);
        let condition = entity.condition;
        let next_aux = entity.next_aux;
        let area = realm_setpieces::Area {
            x: 63,
            y: 63,
            width: 3,
            height: 3,
        };
        let site = w.capture_event_site(area, [64 * 128 + 64]).unwrap();
        w.event_reserves.push(realm_events::ReservedEvent {
            game: w.content.game.as_ref().unwrap().clone(),
            root: entity,
            children: vec![],
            site,
        });
        w.event_waiting = Some(77);
        w.event_choice = Some(kind);
        let mut rx = output(&mut w, p);
        let ids = w.ids.load(Ordering::Relaxed);
        let rng = w.rng.clone().next_u64();
        let event_rng = w.event_rng.clone().next_u64();
        assert_matches!(w.earn_realm_event(77).unwrap(), realm_events::EventAdmission::Spawned(id) if id==root);
        assert_eq!(w.entities.len(), 1);
        assert_eq!(w.entities[&root].quest, Some(root));
        assert_eq!(w.xml_identity(root).unwrap().encounter, Some(root as u64));
        assert!(w.entities[&root].activated.is_none());
        assert_eq!(w.entities[&root].condition, condition);
        assert_eq!(w.entities[&root].next_aux, next_aux);
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.rng.clone().next_u64(), rng);
        assert_eq!(w.event_rng.clone().next_u64(), event_rng);
        assert!(drain(&mut rx).is_empty());
        w.players.get_mut(&p).unwrap().pos = Pos { x: 65.5, y: 64.5 };
        for id in w.xml_reserve_ids(CHILD_LIMIT).unwrap() {
            w.xml.births.push(super::super::spawn::Birth {
                game: w.content.game.as_ref().unwrap().clone(),
                source: w.xml_game().definitions[&kind].source.clone(),
                at: 1_000_000,
                id,
                identity: Identity {
                    world: w.xml.world,
                    actor: id,
                    parent: Some(root),
                    encounter: Some(root as u64),
                },
                kind: 0x120,
                pos: Pos { x: 90.5, y: 90.5 },
            });
        }
        frame(&mut w, 0);
        assert_eq!(w.entities.values().filter(|e| e.kind == child).count(), 0);
        assert_eq!(texts(&mut rx), vec![announcement]);
        w.xml.births.clear();
        frame(&mut w, 50);
        if kind == 0xd61 {
            assert_eq!(
                w.xml
                    .births
                    .iter()
                    .filter(|b| b.kind == child && b.identity.parent == Some(root))
                    .count(),
                expected
            );
            frame(&mut w, 1550);
        }
        let children: Vec<_> = w.entities.values().filter(|e| e.kind == child).collect();
        assert_eq!(children.len(), expected);
        assert!(children.iter().all(|e| e.parent == Some(root)
            && w.xml_identity(e.id).unwrap().encounter == Some(root as u64)));
        assert!(texts(&mut rx).is_empty());
    }
}

#[test]
fn xml_runtime_world_ocean_trench_breath_drains_at_uses_breath_restores_at_vents_and_suffocates() {
    let content = Arc::new(crate::world::test_content());
    let mut d = isolated_directory(content.clone());
    let enter = |w: &mut World| {
        let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
        (ch.name, ch.account, ch.id) = ("Diver".into(), 1, 1);
        w.enter(ch, None).unwrap()
    };
    let cave = d
        .instance("pirate-cave", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&cave).unwrap();
    let p = enter(w);
    let hp = w.players[&p].character.hp;
    for _ in 0..100 {
        w.step();
    }
    assert_eq!(
        (w.players[&p].oxygen, w.players[&p].character.hp),
        (-1, hp),
        "no UsesBreath, no breath"
    );
    let id = d
        .instance("ocean-trench", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&id).unwrap();
    let p = enter(w);
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    let vents: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == crate::dungeon::trench::VENT)
        .map(|e| e.pos)
        .collect();
    let vent = vents[0];
    let dry = (0..w.map.width)
        .map(|x| Pos {
            x: x as f32 + 0.5,
            y: vent.y,
        })
        .find(|p| vents.iter().all(|v| v.distance(*p) > 3.))
        .unwrap();
    let run = |w: &mut World, at: Pos, steps: usize| {
        for _ in 0..steps {
            w.players.get_mut(&p).unwrap().pos = at;
            w.step();
        }
    };
    assert_eq!(w.players[&p].oxygen, 100);
    let before = w.now;
    run(w, dry, 80);
    let drained = |ms: u64| (7.25 * ms as f64 / 1000.).floor() as i32;
    assert_eq!(
        100 - w.players[&p].oxygen,
        drained(w.now) - drained(before),
        "7.25 breath per second"
    );
    assert_eq!(100 - w.players[&p].oxygen, 29);
    w.players.get_mut(&p).unwrap().oxygen = 50;
    run(w, vent, 20);
    let gained = w.players[&p].oxygen - 50;
    assert!(
        (41..=44).contains(&gained),
        "vent nets RestoresBreath 50/s minus drain 7.25/s, got {gained}"
    );
    let player = w.players.get_mut(&p).unwrap();
    (player.oxygen, player.character.hp) = (0, 35);
    run(w, dry, 12);
    let player = &w.players[&p];
    assert!(
        player.death_pending && player.character.hp == 0,
        "hp {}",
        player.character.hp
    );
    assert_eq!(player.killer, "suffocation");
}

#[test]
fn xml_runtime_world_realm_hermit_defeat_turns_its_drop_into_one_journaled_ocean_trench_portal() {
    use crate::dungeon::trench::HERMIT;
    let content = Arc::new(crate::world::test_content());
    let mut map = flat_map(160);
    for (i, c) in map.cells.iter_mut().enumerate() {
        if (70..150).contains(&(i % 160)) && (70..150).contains(&(i / 160)) {
            c.terrain = 3;
        }
    }
    map.spawns = vec![(10.5, 10.5)];
    map.elevation = Some(vec![200; 160 * 160].into());
    let mut w = World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
    w.name = "Realm".into();
    w.xml.template = Some("realm".into());
    w.encounter = true;
    let hermit = w
        .xml_game()
        .named_site(crate::dungeon::trench::HERMIT)
        .unwrap()
        .clone();
    w.allocate_site(&hermit).unwrap();
    let hermit = w.entities.values().find(|e| e.kind == HERMIT).unwrap().id;
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Diver".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    let at = w.entities[&hermit].pos;
    let count = |w: &World, kind: u16| w.entities.values().filter(|e| e.kind == kind).count();
    let run = |w: &mut World, ms: u64| {
        for _ in 0..ms / 50 {
            w.players.get_mut(&p).unwrap().pos = Pos {
                x: at.x,
                y: at.y + 6.,
            };
            w.step();
            drain_events(w).unwrap();
        }
    };
    run(&mut w, 3000);
    assert_eq!(
        count(&w, 0xd66),
        1,
        "the encounter overlay places one Hermit God Drop"
    );
    assert_eq!(count(&w, 0x730), 0);
    let e = w.entities.get_mut(&hermit).unwrap();
    (e.hp, e.condition) = (1, 0);
    w.damage_enemy(p, hermit, 100_000, true, &[], None)
        .unwrap_or_default();
    run(&mut w, 3000);
    assert!(!w.entities.contains_key(&hermit));
    assert_eq!((count(&w, 0xd66), count(&w, 0x730)), (0, 1));
    let portal = w.entities.values().find(|e| e.kind == 0x730).unwrap();
    assert!(portal.published);
    let journal = rusqlite::Connection::open(
        std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
            .join(format!("world-terminal-{}.db", w.xml.instance)),
    )
    .unwrap();
    let births: Vec<String> = journal
        .prepare("SELECT payload FROM world_events WHERE kind='terminal'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .filter(|payload: &String| payload.contains("\"births\":[[1840,"))
        .collect();
    assert_eq!(births.len(), 1, "one journaled 0x730 birth");
    run(&mut w, 5000);
    assert_eq!(count(&w, 0x730), 1, "no repeat");
}

#[test]
fn xml_runtime_world_ordinary_locker_factory_does_not_install_hidden_legacy_davy_program() {
    let content = Arc::new(crate::world::test_content());
    let mut d = isolated_directory(content.clone());
    let id = d
        .instance(
            crate::dungeon::davy::TEMPLATE,
            None,
            Some(DungeonReturn::Random),
        )
        .unwrap();
    let w = d.worlds.get_mut(&id).unwrap();
    let boss = w.entities.values().find(|e| e.kind == 0xe32).unwrap().id;
    let definition = &w.content.objects[&0xe32];
    assert!(w.entities[&boss].published);
    assert_eq!(w.entities[&boss].size, definition.size);
    assert!(w.entities[&boss].activated.is_none());
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let (packets, mut rx) = mpsc::channel(256);
    let (lost, _) = watch::channel(None);
    let p = w
        .enter(
            ch,
            Some(Output {
                trace: None,
                packets,
                lost,
            }),
        )
        .unwrap();
    assert!(
        !drain(&mut rx)
            .iter()
            .any(|packet| packet.id == wire::NOTIFICATION)
    );
    let home = w.entities[&boss].pos;
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: home.x + 9.,
        y: home.y,
    };
    frame(w, 0);
    assert!(w.xml.actors.contains_key(&boss));
    assert_ne!(
        w.entities[&boss].conditions(w.now) & crate::condition::bit(crate::condition::INVINCIBLE),
        0
    );
    w.players.get_mut(&p).unwrap().pos = home;
    frame(w, 50);
    assert_eq!(
        w.entities[&boss].conditions(w.now) & crate::condition::bit(crate::condition::INVINCIBLE),
        0
    );
    assert_ne!(
        w.entities[&boss].conditions(w.now) & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
}

#[test]
fn xml_runtime_world_object_toss_refusal_preserves_due_and_future_owned_reservations() {
    let (mut w, p, id) = fixture(
        r#"<Behavior childId="Piratess" type="auto" range="3" rangeJitter="0" defaultAngle="0" cooldown="1">ObjectToss</Behavior>"#,
    );
    frame(&mut w, 0);
    frame(&mut w, 1000);
    assert_eq!(w.xml.births.len(), 2);
    let reservations: Vec<_> = w
        .xml
        .births
        .iter()
        .map(|b| (b.id, b.at, b.identity))
        .collect();
    let game = w.content.game.as_ref().unwrap().clone();
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    let mut rx = output(&mut w, p);
    w.now = 1500;
    Arc::make_mut(&mut w.content).game = Some(Arc::new(game.as_ref().clone()));
    for _ in 0..2 {
        let error = format!("{:#}", w.xml_birth_deadlines().unwrap_err());
        assert!(
            error.contains("owning catalog changed")
                && error.contains("rules.xml:1")
                && error.contains("object=Pirate"),
            "{error}"
        );
        assert_eq!(
            w.xml
                .births
                .iter()
                .map(|b| (b.id, b.at, b.identity))
                .collect::<Vec<_>>(),
            reservations
        );
        assert_eq!(w.entities.len(), 1);
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.rng.clone().next_u64(), rng);
        assert!(drain(&mut rx).is_empty());
    }
    Arc::make_mut(&mut w.content).game = Some(game);
    let saved = w.xml.births[0].identity;
    w.xml.births[0].identity.world += 1;
    assert!(
        format!("{:#}", w.xml_birth_deadlines().unwrap_err()).contains("reserved identity changed")
    );
    w.xml.births[0].identity = saved;
    Arc::make_mut(&mut w.map).cells[64 * 128 + 67].ground = 255;
    w.xml_birth_deadlines().unwrap();
    assert_eq!(w.xml.births.len(), 2);
    Arc::make_mut(&mut w.map).cells[64 * 128 + 67].ground = 0x18;
    w.remove_entity(id);
    w.xml_birth_deadlines().unwrap();
    assert_eq!(w.xml.births.len(), 1);
    assert_eq!(w.entities[&reservations[0].0].parent, Some(id));
    assert_eq!(
        w.xml_identity(reservations[0].0).unwrap(),
        reservations[0].2
    );
    w.now = 2500;
    w.xml_birth_deadlines().unwrap();
    w.xml_birth_deadlines().unwrap();
    assert!(w.xml.births.is_empty());
    assert_eq!(w.entities.len(), 2);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
}

#[test]
fn xml_runtime_world_initial_map_settings_and_stock_refuse_before_any_ids() {
    let content = Arc::new(crate::world::test_content());
    let original = Map::load(Path::new("data/nexus.wmap"), &content).unwrap();
    for config in [
        "unsupported:1",
        "size:NaN",
        "mtype:2560;mcount:-1;mtime:-1",
        "mtype:2560;mcount:1;mtime:-2",
        "mtype:2560;mcount:1",
        "mtype:65535;mcount:1;mtime:-1",
    ] {
        let mut map = original.clone();
        let cell = map
            .cells
            .iter_mut()
            .rev()
            .find(|cell| {
                cell.object
                    .is_some_and(|kind| content.objects[&kind].class == "Merchant")
            })
            .unwrap();
        cell.config = config.into();
        let ids = Arc::new(AtomicI32::new(5000));
        let error = World::with_ids_context(
            content.clone(),
            Arc::new(map),
            ids.clone(),
            Some(WorldDomain::Realm),
        )
        .err()
        .unwrap();
        assert!(
            format!("{error:#}").contains("initial map cell="),
            "{error:#}"
        );
        assert_eq!(ids.load(Ordering::Relaxed), 5000, "{config}");
    }
    let mut changed = content.as_ref().clone();
    let stock = original
        .cells
        .iter()
        .find(|cell| {
            cell.object
                .is_some_and(|kind| content.objects[&kind].class == "Merchant")
        })
        .unwrap();
    let kind = stock
        .config
        .split(';')
        .find_map(|s| s.strip_prefix("mtype:"))
        .unwrap();
    let kind = if let Some(hex) = kind.strip_prefix("0x") {
        u16::from_str_radix(hex, 16).unwrap()
    } else {
        kind.parse().unwrap()
    };
    Arc::make_mut(changed.game.as_mut().unwrap())
        .definitions
        .remove(&kind);
    let ids = Arc::new(AtomicI32::new(5000));
    assert!(
        World::with_ids_context(
            Arc::new(changed),
            Arc::new(original),
            ids.clone(),
            Some(WorldDomain::Realm)
        )
        .is_err()
    );
    assert_eq!(ids.load(Ordering::Relaxed), 5000);
}

#[test]
fn xml_runtime_world_native_entry_and_vault_batch_preflight_do_not_partially_allocate() {
    let content = Arc::new(crate::world::test_content());
    let mut w =
        World::in_domain(content.clone(), Arc::new(flat_map(128)), WorldDomain::Realm).unwrap();
    let enter = |w: &mut World, ch| {
        w.enter_at(
            ch,
            None,
            None,
            AccountPresentation {
                guild: None,
                credits: 100,
                fame: 0,
                best_fame: BTreeMap::new(),
            },
            false,
        )
    };
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    Arc::make_mut(&mut w.map).spawns.clear();
    assert!(enter(&mut w, ch.clone()).is_err());
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    Arc::make_mut(&mut w.map).spawns = vec![(64.5, 64.5)];
    Arc::make_mut(&mut w.map).cells[64 * 128 + 64].ground = 255;
    assert!(enter(&mut w, ch.clone()).is_err());
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert!(w.players.is_empty() && w.entities.is_empty());
    Arc::make_mut(&mut w.map).cells[64 * 128 + 64].ground = 0x18;
    ch.pet_kind = Some(
        content
            .named(&content.objects[&0x1513].companion.as_ref().unwrap().object)
            .unwrap()
            .kind,
    );
    let game = w.content.game.as_ref().unwrap().clone();
    Arc::make_mut(Arc::make_mut(&mut w.content).game.as_mut().unwrap())
        .definitions
        .remove(&ch.pet_kind.unwrap());
    assert!(enter(&mut w, ch.clone()).is_err());
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    Arc::make_mut(&mut w.content).game = Some(game);
    enter(&mut w, ch).unwrap();
    assert_eq!(w.players.len(), 1);
    assert_eq!(w.entities.len(), 1);

    let map = Map::load(Path::new("data/vault.wmap"), &content).unwrap();
    let mut w = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    w.name = "Vault".into();
    w.xml.template = Some("vault".into());
    w.owner = Some(1);
    let vault = crate::store::Vault {
        account: 1,
        chest: 0,
        items: vec![-1; 8],
        revision: 0,
    };
    let last = w.map.cells.iter().rposition(|c| c.region == 12).unwrap();
    let ground = w.map.cells[last].ground;
    Arc::make_mut(&mut w.map).cells[last].ground = 255;
    let ids = w.ids.load(Ordering::Relaxed);
    let entities = w.entities.len();
    assert!(w.load_vaults(vec![vault.clone()]).is_err());
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.entities.len(), entities);
    assert!(w.vaults.is_empty() && w.closed_vaults.is_empty());
    Arc::make_mut(&mut w.map).cells[last].ground = ground;
    w.load_vaults(vec![vault]).unwrap();
    assert_eq!(w.vaults.len(), 1);
    assert_eq!(w.closed_vaults.len(), 19);
}

fn generated_dungeon(content: &Content, template: &str, seed: u64) -> Result<Map> {
    crate::dungeon::generate_selected(content, template, seed)
}

#[test]
fn xml_runtime_world_ordinary_dungeon_factories_keep_native_admission_and_source_geometry_limits() {
    let content = Arc::new(crate::world::test_content());
    let mut failures = vec![];
    for (_, template, _, boss) in crate::dungeon::DUNGEONS {
        let mut d = isolated_directory(content.clone());
        let seed = d.prepared_seed.unwrap() ^ d.next_world as u64;
        let map = generated_dungeon(&content, template, seed).unwrap();
        let before = d.ids.load(Ordering::Relaxed);
        let rng = d.realm_rng.clone().next_u64();
        let portal_rng = d.portal_rng.clone().next_u64();
        match d.instance(template, None, Some(DungeonReturn::Random)) {
            Ok(id) => {
                let w = &d.worlds[&id];
                assert_eq!(w.xml.domain, Some(WorldDomain::NonRealm));
                assert!(w.map.elevation.is_none());
                assert_eq!(
                    serde_json::to_vec(&w.map.cells).unwrap(),
                    serde_json::to_vec(&map.cells).unwrap(),
                    "{template}: generated map changed"
                );
                assert!(
                    w.entities.values().any(|e| e.kind == boss),
                    "{template} missing actual root"
                );
                let spec = w.xml_world_template().unwrap();
                let linked = &w.xml_game().maps[spec.map as usize];
                for e in w.entities.values().filter(|e| w.xml_is_portal(e.kind)) {
                    assert!(
                        linked.objects.contains(&e.kind),
                        "{template}: generated portal {:#x} absent from map binding",
                        e.kind
                    );
                    w.xml_portal_route(e.kind).unwrap();
                }
                for e in w.entities.values() {
                    let declared = w
                        .xml_declared_admission(e.kind, "ordinary dungeon factory regression")
                        .unwrap();
                    w.xml_game()
                        .entity_admission(
                            e.kind,
                            declared.admission_role().unwrap(),
                            "actual generated placement",
                        )
                        .unwrap();
                    assert!(
                        e.activated.is_none(),
                        "{template}: legacy AI initialization {:#x}",
                        e.kind
                    );
                }
            }
            Err(error) => {
                let error = format!("{error:#}");
                assert_eq!(
                    d.ids.load(Ordering::Relaxed),
                    before,
                    "{template} allocated before refusal: {error}"
                );
                assert_eq!(d.realm_rng.clone().next_u64(), rng);
                assert_eq!(d.portal_rng.clone().next_u64(), portal_rng);
                assert_eq!(d.next_world, 2);
                assert_eq!(d.worlds.len(), 0);
                assert!(d.portals.is_empty() && d.grants.is_empty() && d.idle.is_empty());
                failures.push(format!("{template}: {error}"));
            }
        };
    }
    assert!(
        failures.is_empty(),
        "ordinary dungeon factories incomplete:\n{}",
        failures.join("\n")
    );
}

#[tokio::test]
async fn xml_runtime_world_execution_pool_preserves_owned_game_and_refused_ingress_without_ids() {
    let (w, _, _) = fixture("<Behavior>RemainVisible</Behavior>");
    let mut d = isolated_directory(w.content.clone());
    d.clock.set_paused(true);
    let fingerprint = w.xml_game().fingerprint.clone();
    d.worlds.insert(7, w);
    let handle = Handle::spawn(d);
    handle
        .in_world(7)
        .call(move |w| {
            assert_eq!(w.xml_game().fingerprint, fingerprint);
            assert_eq!(w.xml.domain, Some(WorldDomain::Realm));
            let before = w.ids.load(Ordering::Relaxed);
            let rng = w.rng.clone().next_u64();
            let count = w.entities.len();
            for kind in [0x173, 0x711, 0xc2b, 0x1407] {
                assert!(
                    w.spawn_entity(kind, Pos { x: 70.5, y: 64.5 }, vec![])
                        .is_err()
                );
            }
            assert_eq!(w.ids.load(Ordering::Relaxed), before);
            assert_eq!(w.rng.clone().next_u64(), rng);
            assert_eq!(w.entities.len(), count);
            let wall = w.spawn_entity(0x120, Pos { x: 70.5, y: 64.5 }, vec![])?;
            assert_eq!(w.entities[&wall].kind, 0x120);
            Ok(())
        })
        .await
        .unwrap();
}

#[test]
fn xml_runtime_world_reserved_event_rechecks_catalog_and_entitlement_before_selection_rng() {
    let (mut w, p) = native_world(0x322, 0xab1);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 14.5, y: 14.5 };
    let root = spawn(&mut w, 0xd54, 64.5, 64.5);
    w.entities
        .get_mut(&root)
        .unwrap()
        .quest
        .set(Some(root), root, &mut w.quest_ids);
    let entity = w.remove_entity(root);
    let site = w
        .capture_event_site(
            realm_setpieces::Area {
                x: 63,
                y: 63,
                width: 3,
                height: 3,
            },
            [64 * 128 + 64],
        )
        .unwrap();
    let game = w.content.game.as_ref().unwrap().clone();
    w.event_reserves.push(realm_events::ReservedEvent {
        game: game.clone(),
        root: entity,
        children: vec![],
        site,
    });
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.event_rng.clone().next_u64();
    Arc::make_mut(&mut w.content).game = Some(Arc::new(game.as_ref().clone()));
    assert!(
        format!("{:#}", w.earn_realm_event(77).err().unwrap()).contains("owning catalog changed")
    );
    assert!(w.event_waiting.is_none() && w.event_choice.is_none());
    assert_eq!(w.event_rng.clone().next_u64(), rng);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.event_reserves.len(), 1);
    assert!(w.entities.is_empty());
    Arc::make_mut(&mut w.content).game = Some(game);
    w.event_reserves[0]
        .root
        .quest
        .set(None, root, &mut BTreeSet::new());
    assert!(
        format!("{:#}", w.earn_realm_event(77).err().unwrap())
            .contains("actual encounter ownership")
    );
    assert_eq!(w.event_rng.clone().next_u64(), rng);
    w.event_reserves[0]
        .root
        .quest
        .set(Some(root), root, &mut BTreeSet::new());
    w.event_waiting = Some(77);
    w.event_choice = Some(0xd54);
    assert_matches!(w.earn_realm_event(77).unwrap(), realm_events::EventAdmission::Spawned(id) if id == root);
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
}

#[test]
fn xml_runtime_world_native_inventory_handlers_recheck_private_roles_and_key_source_before_trade_effects()
 {
    let (mut w, p) = native_world(0x324, 0xc2a);
    let mut character = w.players[&p].character.clone();
    character.id = 2;
    character.account = 2;
    character.name = "Bea".into();
    let q = w.enter(character, None).unwrap();
    w.request_trade(p, "Bea".into()).unwrap();
    w.request_trade(q, "Ada".into()).unwrap();
    assert!(w.has_active_trade(p) && w.has_active_trade(q));
    let pos = w.players[&p].pos;
    let bag = w.loot_bag(pos, Some(1), vec![0xa00]).unwrap();
    let source = Slot {
        object: bag,
        slot: 0,
        kind: 0xa00,
    };
    let destination = Slot {
        object: q,
        slot: 4,
        kind: -1,
    };
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    assert!(w.swap(q, source, destination).is_err());
    assert!(w.has_active_trade(p) && w.has_active_trade(q));
    let game = w.content.game.as_ref().unwrap().clone();
    Arc::make_mut(Arc::make_mut(&mut w.content).game.as_mut().unwrap())
        .definitions
        .remove(&w.entities[&bag].kind);
    for error in [
        w.use_item(p, source, pos).err().unwrap(),
        w.drop_item(p, source).err().unwrap(),
        w.swap(
            p,
            source,
            Slot {
                object: p,
                ..destination
            },
        )
        .err()
        .unwrap(),
    ] {
        assert!(
            format!("{error:#}").contains("owning catalog changed"),
            "{error:#}"
        );
    }
    assert!(w.has_active_trade(p) && w.has_active_trade(q));
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert_eq!(w.entities[&bag].items[0], 0xa00);
    assert!(!w.entities[&bag].reserved && !w.players[&p].pending);
    Arc::make_mut(&mut w.content).game = Some(game.clone());
    let key = game
        .definitions
        .values()
        .filter(|d| d.native_service == Some(NativeService::Inventory))
        .map(|d| &game.properties[&d.kind])
        .find(|item| {
            item.activate == "Create"
                && item
                    .create_object
                    .as_ref()
                    .is_some_and(|name| w.content.named(name).unwrap().dungeon_name.is_some())
        })
        .unwrap()
        .clone();
    w.players.get_mut(&p).unwrap().character.items[4] = 0xa00;
    let slot = Slot {
        object: p,
        slot: 4,
        kind: 0xa00,
    };
    assert!(
        w.begin_create(p, slot, &key).is_err(),
        "known key metadata is not ownership"
    );
    assert!(w.has_active_trade(p));
    let portal = w
        .content
        .named(key.create_object.as_ref().unwrap())
        .unwrap()
        .kind;
    w.players.get_mut(&p).unwrap().character.items[4] = i32::from(key.kind);
    Arc::make_mut(Arc::make_mut(&mut w.content).game.as_mut().unwrap())
        .definitions
        .remove(&portal);
    let slot = Slot {
        kind: i32::from(key.kind),
        ..slot
    };
    assert!(w.use_item(p, slot, pos).is_err());
    assert!(w.has_active_trade(p) && w.has_active_trade(q));
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    Arc::make_mut(&mut w.content).game = Some(game);
    let plan = w.use_item(p, slot, pos).unwrap().unwrap();
    assert!(!w.has_active_trade(p) && !w.has_active_trade(q));
    assert_eq!(w.players[&p].character.items[4], i32::from(key.kind));
    let opened = w.entities.values().find(|e| e.kind == portal).unwrap();
    assert!(opened.reserved && !opened.published);
    assert_eq!(plan.items[4], -1);
    let revision = plan.revision + 1;
    let mut d = isolated_directory(w.content.clone());
    d.ids = w.ids.clone();
    w.name = "Nexus".into();
    d.worlds.insert(-2, w);
    let plan = d.stage_unlock(plan).unwrap();
    d.finish_inventory(plan, Ok(revision)).unwrap();
    let w = &d.worlds[&-2];
    assert_eq!(w.players[&p].character.items[4], -1);
    assert!(
        w.entities
            .values()
            .any(|e| e.kind == portal && e.published && !e.reserved)
    );
}

fn pending_undead_key() -> (Directory, InventoryPlan, i32, i32) {
    let (mut w, p) = native_world(0x324, 0xc2a);
    w.name = "Nexus".into();
    w.players.get_mut(&p).unwrap().character.items[4] = 0x701;
    let pos = w.players[&p].pos;
    let plan = w
        .use_item(
            p,
            Slot {
                object: p,
                slot: 4,
                kind: 0x701,
            },
            pos,
        )
        .unwrap()
        .unwrap();
    let opened = plan.unlock.unwrap().opened;
    let mut d = isolated_directory(w.content.clone());
    d.ids = w.ids.clone();
    d.clock.set_paused(true);
    d.worlds.insert(-2, w);
    (d, plan, p, opened)
}

#[test]
fn xml_runtime_world_scripted_key_portal_stays_nonexecuting_until_commit_then_decays_from_publication()
 {
    let (mut d, plan, p, opened) = pending_undead_key();
    let plan = d.stage_unlock(plan).unwrap();
    let destination = d.portals[&opened].destination.unwrap();
    assert_eq!(d.worlds[&destination].name, "Undead Lair");
    let w = d.worlds.get_mut(&-2).unwrap();
    assert!(w.xml_game().definitions[&0x71a].native_service.is_none());
    assert_eq!(
        w.xml_game().definitions[&0x71a].disposition,
        Disposition::Active
    );
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    w.players.get_mut(&p).unwrap().paused = true;
    for at in [0, 30_000, 35_000] {
        frame(w, at);
        assert!(w.entities[&opened].reserved && !w.entities[&opened].published);
        assert!(w.xml.actors[&opened].path().is_empty());
        assert_eq!(w.xml.actors[&opened].active_ms(), 0);
        assert!(w.xml_portal_expiry(opened).is_none());
        assert!(w.xml.active.is_empty() && w.xml.births.is_empty() && w.xml.rewards.is_empty());
    }
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert_eq!(w.players[&p].character.items[4], 0x701);
    assert!(d.destination(p, Some(opened)).is_err());
    let replay = plan.clone();
    let revision = plan.revision + 1;
    d.finish_inventory(plan, Ok(revision)).unwrap();
    assert_eq!(d.portals[&opened].expires, None);
    let w = d.worlds.get_mut(&-2).unwrap();
    assert_eq!(w.players[&p].character.items[4], -1);
    assert!(w.entities[&opened].published && !w.entities[&opened].reserved);
    assert_eq!(w.xml_portal_expiry(opened), Some(65_000));
    assert_eq!(w.xml.actors[&opened].active_ms(), 0);
    assert!(!w.xml.actors[&opened].path().is_empty());
    assert!(d.finish_inventory(replay, Ok(revision)).is_err());
    let w = d.worlds.get_mut(&-2).unwrap();
    assert_eq!(w.xml_portal_expiry(opened), Some(65_000));
    frame(w, 64_999);
    assert!(w.entities.contains_key(&opened));
    assert_eq!(d.destination(p, Some(opened)).unwrap().2, destination);
    let w = d.worlds.get_mut(&-2).unwrap();
    frame(w, 65_000);
    assert!(!w.entities.contains_key(&opened));
    assert!(w.xml.claims.contains(&opened));
    frame(w, 65_050);
    assert!(
        w.xml.rewards.is_empty(),
        "Decay dropLoot=false/giveXP=false creates no reward intent"
    );
    assert!(d.destination(p, Some(opened)).is_err());
    d.retire_expired_portals();
    assert!(!d.portals.contains_key(&opened));
}

#[test]
fn xml_runtime_world_scripted_portal_abort_and_stale_source_release_without_root_hooks_or_item_loss()
 {
    for failure in ["store", "catalog", "directory_catalog", "owner"] {
        let (mut d, plan, p, opened) = pending_undead_key();
        if failure == "store" {
            let plan = d.stage_unlock(plan).unwrap();
            let w = d.worlds.get_mut(&-2).unwrap();
            frame(w, 35_000);
            assert!(
                d.finish_inventory(
                    plan,
                    Err(anyhow::anyhow!("injected unknown-COMMIT inventory failure"))
                )
                .is_err()
            );
            let w = &d.worlds[&-2];
            assert!(w.entities[&opened].reserved && !w.entities[&opened].published);
            assert!(w.xml.actors[&opened].path().is_empty());
            assert_eq!(w.players[&p].character.items[4], 0x701);
            assert!(w.players[&p].pending && w.players[&p].persistence_failure.is_some());
            assert!(d.portals.contains_key(&opened));
            continue;
        } else {
            if failure == "directory_catalog" {
                let game = d.content.game.as_ref().unwrap().clone();
                Arc::make_mut(&mut d.content).game = Some(Arc::new(game.as_ref().clone()));
            }
            let w = d.worlds.get_mut(&-2).unwrap();
            if failure == "catalog" {
                let game = w.content.game.as_ref().unwrap().clone();
                Arc::make_mut(&mut w.content).game = Some(Arc::new(game.as_ref().clone()));
            } else if failure == "owner" {
                w.players.get_mut(&p).unwrap().character.id += 1;
            }
            assert!(d.stage_unlock(plan).is_err());
        }
        let w = &d.worlds[&-2];
        assert_eq!(w.players[&p].character.items[4], 0x701, "{failure}");
        assert!(!w.entities.contains_key(&opened));
        assert!(!w.xml.actors.contains_key(&opened));
        assert!(
            w.xml.active.is_empty()
                && w.xml.claims.is_empty()
                && w.xml.rewards.is_empty()
                && w.xml.deadlines.is_empty()
        );
        assert!(!d.portals.contains_key(&opened));
        assert_eq!(d.worlds.len(), 1);
    }
}

#[test]
fn xml_runtime_world_scripted_physical_portal_cannot_use_raw_or_terminal_birth_entitlement() {
    let mut content = Content::load(Path::new("data")).unwrap();
    let rules = authored(
        "Pirate",
        0x600,
        r#"<Behavior childId="Undead Lair Portal" posJitter="0">MetamorphoseAtDeath</Behavior>"#,
    ) + &authored(
        "Undead Lair Portal",
        0x71a,
        r#"<Behavior time="30">Decay</Behavior>"#,
    ) + r#"<Object type="6" id="Blade" disposition="native" nativeService="projectile" provenance="authored" origin="selected Pirate projectile property" reason="explicit terminal fixture property dependency"/>"#;
    let inputs = BTreeMap::from([
        (
            "game/123.5.1/game.xml".into(),
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/><Geometry file="geometry.xml"/><World id="undead-lair" map="Fixture"><Instance entrance="Undead Lair Portal" name="Undead Lair" provenance="authored" origin="synthetic portal fixture" reason="Explicit first-use destination"/><Clear boss="Pirate" route="scripted" provenance="authored" origin="synthetic portal fixture" reason="Only the linked terminal birth owns the portal"/></World></Game>"#
                .to_vec(),
        ),
        (
            "game/123.5.1/rules.xml".into(),
            format!("<Objects>{rules}</Objects>").into_bytes(),
        ),
        (
            "game/123.5.1/geometry.xml".into(),
            br#"<Geometry schema="1"><Template id="Fixture" provenance="authored" origin="synthetic portal fixture" reason="Link the scripted entrance to a real instance binding" width="2" height="2" rotation="fixed" rooms="1"><CharMap char="." ground="Grass"/><Pattern><Row value=".."/><Row value=".."/></Pattern><Spawn x="0.5" y="0.5"/></Template></Geometry>"#.to_vec(),
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
    let mut w = World::in_domain(
        Arc::new(content),
        Arc::new(flat_map(128)),
        WorldDomain::Realm,
    )
    .unwrap();
    let pos = Pos { x: 64.5, y: 64.5 };
    let root = w.spawn_entity(0x600, pos, vec![]).unwrap();
    let ids = w.ids.load(Ordering::Relaxed);
    assert!(w.spawn_entity(0x71a, pos, vec![]).is_err());
    assert!(
        w.insert_portal_entity(0x600, pos, "wrong physical class")
            .is_err()
    );
    for _ in 0..2 {
        w.xml_defeated(root, TerminalContext::NonPlayer).unwrap();
        assert!(!w.pending_terminal_events().is_empty());
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.entities.len(), 1);
        assert!(w.entities.contains_key(&root));
        assert!(w.xml.claims.is_empty() && w.xml.rewards.is_empty());
    }
    drain_events(&mut w).unwrap();
    assert!(!w.entities.contains_key(&root));
    let portal = w.entities.values().find(|e| e.kind == 0x71a).unwrap().id;
    assert_eq!(w.xml_portal_expiry(portal), Some(w.now + 30_000));
    assert_eq!(
        &w.xml.portals[&portal].intent.destination,
        &crate::content::game::PortalDestination::Instance("undead-lair".into())
    );
}

#[test]
fn xml_runtime_world_catalog_replacement_refuses_before_clock_native_cost_or_source_execution() {
    let (mut w, p, id) = fixture("<Behavior>Wander</Behavior>");
    let game = w.content.game.as_ref().unwrap().clone();
    let ids = w.ids.load(Ordering::Relaxed);
    let rng = w.rng.clone().next_u64();
    let previous = w.last_npc_step;
    w.now = 50;
    Arc::make_mut(&mut w.content).game = Some(Arc::new(game.as_ref().clone()));
    assert!(format!("{:#}", w.xml_step().unwrap_err()).contains("owning catalog changed"));
    assert_eq!(w.last_npc_step, previous);
    assert_eq!(w.entities[&id].pos.x, 64.5);
    assert!(
        w.use_item(
            p,
            Slot {
                object: p,
                slot: 1,
                kind: 0xc2b
            },
            w.players[&p].pos
        )
        .is_err()
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert!(w.xml.actors.is_empty() && w.xml.motions.is_empty());
    Arc::make_mut(&mut w.content).game = Some(game);
    frame(&mut w, 50);
    assert!(w.xml.actors.contains_key(&id));
}

#[test]
fn xml_runtime_world_locker_keys_open_exactly_their_door_pair_through_recovered_openers() {
    use crate::dungeon::davy::{self, BOSS, KEYS, OPENERS};
    let content = Arc::new(crate::world::test_content());
    let mut d = isolated_directory(content.clone());
    let id = d
        .instance(davy::TEMPLATE, None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&id).unwrap();
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    ch.name = "Diver".into();
    ch.account = 1;
    ch.id = 1;
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    let doors = |w: &World, c: usize| {
        w.entities
            .values()
            .filter(|e| davy::door_color(e.kind) == Some(c))
            .count()
    };
    let openers = |w: &World, c: usize| {
        w.entities
            .values()
            .filter(|e| e.kind == OPENERS[c])
            .map(|e| e.pos)
            .collect::<Vec<_>>()
    };
    // Every coloured room has one door pair and one opener.
    let closed: Vec<_> = (0..4).map(|c| doors(w, c)).collect();
    assert!((0..4).all(|c| closed[c] > 0 && closed[c] == 2 * openers(w, c).len()));
    let mut now = 0;
    let mut stand = |w: &mut World, at: Pos| {
        w.players.get_mut(&p).unwrap().pos = at;
        for _ in 0..10 {
            now += 50;
            frame(w, now);
        }
    };
    for c in 0..4 {
        let key = w.entities.values().find(|e| e.kind == KEYS[c]).unwrap().pos;
        let first = openers(w, c)[0];
        stand(w, first);
        assert_eq!(doors(w, c), closed[c], "colour {c} opened before its key");
        stand(w, key);
        assert!(!w.entities.values().any(|e| e.kind == KEYS[c]));
        for (n, opener) in openers(w, c).into_iter().enumerate() {
            stand(w, opener);
            assert_eq!(
                doors(w, c),
                closed[c] - 2 * (n + 1),
                "colour {c} opener {n}"
            );
        }
        assert!(openers(w, c).is_empty());
        assert!((c + 1..4).all(|other| doors(w, other) == closed[other]));
    }
    let goal = w.entities.values().find(|e| e.kind == BOSS).unwrap().home;
    let (sx, sy) = w.map.spawns[0];
    let mut seen = std::collections::BTreeSet::from([(sx as i32, sy as i32)]);
    let mut queue = std::collections::VecDeque::from([(sx as i32, sy as i32)]);
    while let Some((x, y)) = queue.pop_front() {
        for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            let at = Pos {
                x: nx as f32 + 0.5,
                y: ny as f32 + 0.5,
            };
            if w.walkable(at) && seen.insert((nx, ny)) {
                queue.push_back((nx, ny));
            }
        }
    }
    assert!(
        seen.contains(&(goal.x as i32, goal.y as i32)),
        "boss room unreachable after all keys"
    );
}

#[test]
fn xml_runtime_world_horrific_creation_starts_shielded_and_opens_on_a_vat() {
    use crate::condition::{INVULNERABLE, bit};
    use crate::dungeon::lab::CREATION;
    let content = Arc::new(crate::world::test_content());
    let mut d = isolated_directory(content.clone());
    let id = d
        .instance("mad-lab", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&id).unwrap();
    let e = w.entities.values().find(|e| e.kind == CREATION).unwrap();
    let (creation, home, hp) = (e.id, e.pos, e.hp);
    let shield = bit(INVULNERABLE);
    assert!(e.activated.is_none());
    assert_eq!(e.condition & shield, shield);
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Chemist".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    assert_eq!(
        w.damage_enemy(p, creation, 100, true, &[], None).unwrap(),
        0
    );
    assert_eq!(w.entities[&creation].hp, hp);

    let vat = w
        .map
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.ground == 0xa9)
        .map(|(i, _)| Pos {
            x: (i % w.map.width as usize) as f32 + 0.5,
            y: (i / w.map.width as usize) as f32 + 0.5,
        })
        .min_by(|a, b| home.distance(*a).total_cmp(&home.distance(*b)))
        .unwrap();
    w.players.get_mut(&p).unwrap().pos = vat;
    w.position_entity(creation, vat).unwrap();
    frame(w, 50); // Ini -> FirstNormal
    assert_eq!(w.entities[&creation].condition & shield, shield);
    frame(w, 100); // FirstNormal -> RandomTexture
    assert_eq!(w.entities[&creation].condition & shield, 0);
    frame(w, 200); // RandomTexture -> AltTex*
    assert!(w.damage_enemy(p, creation, 100, true, &[], None).unwrap() > 0);
}

#[test]
fn xml_runtime_world_mad_lab_open_wall_drops_emergency_walls_after_boss_and_every_coil_in_either_order()
 {
    use crate::dungeon::lab::{BOSS, CREATION, EMERGENCY, OPEN_WALL, TESLA};
    for boss_first in [true, false] {
        let content = Arc::new(crate::world::test_content());
        let mut d = isolated_directory(content.clone());
        let id = d
            .instance("mad-lab", None, Some(DungeonReturn::Random))
            .unwrap();
        let w = d.worlds.get_mut(&id).unwrap();
        let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
        (ch.name, ch.account, ch.id) = ("Chemist".into(), 1, 1);
        let p = w.enter(ch, None).unwrap();
        w.players.get_mut(&p).unwrap().lab_conditions =
            crate::condition::bit(crate::condition::INVINCIBLE);
        let ids = |w: &World, kind: u16| {
            w.entities
                .values()
                .filter(|e| e.kind == kind)
                .map(|e| e.id)
                .collect::<Vec<_>>()
        };
        let opener = w.entities[&ids(w, OPEN_WALL)[0]].pos;
        let creation = w
            .entities
            .values()
            .find(|e| e.kind == CREATION)
            .unwrap()
            .pos;
        assert_eq!((ids(w, EMERGENCY).len(), ids(w, OPEN_WALL).len()), (4, 1));
        let settle = |w: &mut World| {
            for _ in 0..60 {
                w.players.get_mut(&p).unwrap().pos = Pos {
                    x: opener.x,
                    y: opener.y + 3.,
                };
                w.step();
            }
        };
        let reachable = |w: &World| {
            let (sx, sy) = w.map.spawns[0];
            let mut seen = std::collections::BTreeSet::from([(sx as i32, sy as i32)]);
            let mut queue = std::collections::VecDeque::from([(sx as i32, sy as i32)]);
            while let Some((x, y)) = queue.pop_front() {
                for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                    if w.walkable(Pos {
                        x: nx as f32 + 0.5,
                        y: ny as f32 + 0.5,
                    }) && seen.insert((nx, ny))
                    {
                        queue.push_back((nx, ny));
                    }
                }
            }
            let (cx, cy) = (creation.x as i32, creation.y as i32);
            [(cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)]
                .iter()
                .any(|c| seen.contains(c))
        };
        settle(w);
        assert_eq!(
            ids(w, EMERGENCY).len(),
            4,
            "walls fell with Dr Terrible and coils alive"
        );
        assert!(!reachable(w));
        let mut coils = ids(w, TESLA);
        assert_eq!(coils.len(), 10);
        let last = coils.pop().unwrap();
        if boss_first {
            w.remove_entity(ids(w, BOSS)[0]);
            for coil in coils {
                w.remove_entity(coil);
            }
            settle(w);
            assert_eq!(ids(w, EMERGENCY).len(), 4, "walls fell with one coil alive");
            w.remove_entity(last);
        } else {
            for coil in coils.into_iter().chain([last]) {
                w.remove_entity(coil);
            }
            settle(w);
            assert_eq!(
                ids(w, EMERGENCY).len(),
                4,
                "walls fell with Dr Terrible alive"
            );
            w.remove_entity(ids(w, BOSS)[0]);
        }
        settle(w);
        assert!(ids(w, EMERGENCY).is_empty(), "boss_first={boss_first}");
        assert!(reachable(w), "Creation unreachable boss_first={boss_first}");
    }
}

#[test]
fn xml_runtime_world_locker_lanterns_open_davys_window_and_the_cycle_restarts() {
    use crate::condition::{INVULNERABLE, bit};
    use crate::dungeon::davy::{self, BOSS, LIT, UNLIT};
    let content = Arc::new(crate::world::test_content());
    let mut d = isolated_directory(content.clone());
    let id = d
        .instance(davy::TEMPLATE, None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&id).unwrap();
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    ch.name = "Diver".into();
    ch.account = 1;
    ch.id = 1;
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().lab_conditions = bit(crate::condition::INVINCIBLE);
    let davy = w.entities.values().find(|e| e.kind == BOSS).unwrap().id;
    w.players.get_mut(&p).unwrap().pos = w.entities[&davy].pos;
    let count = |w: &World, kind: u16| w.entities.values().filter(|e| e.kind == kind).count();
    let shielded = |w: &World| w.entities[&davy].conditions(w.now) & bit(INVULNERABLE) != 0;
    let lanterns = count(w, UNLIT);
    assert!(lanterns > 0 && count(w, LIT) == 0);
    let mut now = 0;
    let mut run = |w: &mut World, ms: u64| {
        for _ in 0..ms / 50 {
            now += 50;
            frame(w, now);
        }
    };
    run(w, 2000);
    assert!(
        shielded(w),
        "Davy must start shielded while lanterns are out"
    );
    // ghostShip.xml: each unlit lantern relights through MetamorphoseAtDeath; all lit opens LightsOn.
    for lamp in w
        .entities
        .values()
        .filter(|e| e.kind == UNLIT)
        .map(|e| e.id)
        .collect::<Vec<_>>()
    {
        w.damage_enemy(p, lamp, 100_000, true, &[], None).unwrap();
        drain_events(w).unwrap();
    }
    run(w, 500);
    assert_eq!((count(w, UNLIT), count(w, LIT)), (0, lanterns));
    run(w, 1000);
    assert!(!shielded(w), "all lanterns lit must open Davy's window");
    let quarter = w.content.objects[&BOSS].hp / 4 + 1;
    w.damage_enemy(p, davy, quarter + 100, true, &[], None)
        .unwrap();
    run(w, 3000);
    assert!(shielded(w), "crossing 75% HP closes the window (LightsOut)");
    assert_eq!(
        (count(w, UNLIT), count(w, LIT)),
        (lanterns, 0),
        "KillLight relights the cycle"
    );
}

#[test]
fn xml_runtime_world_tomb_sarcophagi_wake_the_bosses_and_their_defeat_opens_the_portal() {
    use crate::dungeon::tomb::{self as d, ANCHOR, SARCOPHAGUS, STATUES};
    let content = Arc::new(crate::world::test_content());
    let map =
        Arc::new(crate::dungeon::generate_selected(&content, "tomb-of-the-ancients", 12).unwrap());
    let mut w = World::in_domain(content.clone(), map, WorldDomain::NonRealm).unwrap();
    w.name = "Tomb of the Ancients".into();
    w.xml.template = Some("tomb-of-the-ancients".into());
    w.encounter = true;
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Digger".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    // A watcher at the anchor who never deals damage, and a damager who is far away at the clear.
    let others: Vec<_> = [(2, "Watcher"), (3, "Faraway")]
        .into_iter()
        .map(|(id, name)| {
            let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
            (ch.name, ch.account, ch.id) = (name.into(), id, id);
            w.enter(ch, None).unwrap()
        })
        .collect();
    for id in std::iter::once(p).chain(others.iter().copied()) {
        w.players.get_mut(&id).unwrap().lab_conditions =
            crate::condition::bit(crate::condition::INVINCIBLE);
    }
    let bosses = [d::NUT, d::GEB, d::BES];
    let count = |w: &World, kinds: &[u16]| {
        w.entities
            .values()
            .filter(|e| kinds.contains(&e.kind))
            .count()
    };
    let anchor = w.entities.values().find(|e| e.kind == ANCHOR).unwrap().pos;
    let far = Pos {
        x: anchor.x + 30.,
        y: anchor.y,
    };
    let mut now = 0;
    let mut run = |w: &mut World, at: Pos, ms: u64| {
        w.players.get_mut(&p).unwrap().pos = at;
        w.players.get_mut(&others[0]).unwrap().pos = anchor;
        w.players.get_mut(&others[1]).unwrap().pos = far;
        for _ in 0..ms / 50 {
            now += 50;
            frame(w, now);
        }
    };
    run(&mut w, anchor, 500);
    assert_eq!((count(&w, &STATUES), count(&w, &bosses)), (3, 0));
    // tombOfTheAncients.xml: statues ComeAlive when no Active Sarcophagus exists.
    let kill = |w: &mut World, kinds: &[u16]| {
        for id in w
            .entities
            .values()
            .filter(|e| kinds.contains(&e.kind))
            .map(|e| e.id)
            .collect::<Vec<_>>()
        {
            if let Some(e) = w.entities.get_mut(&id) {
                e.hp = 1;
                e.condition = 0;
                if bosses.contains(&e.kind) {
                    e.damage.insert(3, 50);
                }
            }
            let _ = w.damage_enemy(p, id, 100_000, true, &[], None);
            drain_events(w).unwrap();
        }
    };
    kill(&mut w, &[SARCOPHAGUS]);
    // XML actors sleep without a player within 20 tiles, so visit each statue room.
    let statues: Vec<_> = w
        .entities
        .values()
        .filter(|e| STATUES.contains(&e.kind))
        .map(|e| e.pos)
        .collect();
    for at in statues.iter().chain(&statues) {
        run(&mut w, *at, 1000);
    }
    run(&mut w, anchor, 500);
    assert_eq!(count(&w, &[SARCOPHAGUS]), 0);
    assert_eq!(
        (count(&w, &STATUES), count(&w, &bosses)),
        (0, 3),
        "statues metamorphose into the bosses"
    );
    let portal = content
        .objects
        .values()
        .find(|o| o.name == "Realm Portal")
        .unwrap()
        .kind;
    assert_eq!(count(&w, &[portal]), 0);
    for _ in 0..20 {
        kill(&mut w, &bosses);
        run(&mut w, anchor, 500);
        if count(&w, &bosses) == 0 {
            break;
        }
    }
    run(&mut w, anchor, 2000);
    assert_eq!(count(&w, &bosses), 0);
    assert_eq!(
        count(&w, &[ANCHOR]),
        0,
        "the anchor metamorphoses once the bosses fall"
    );
    assert_eq!(count(&w, &[portal]), 1);
    let tombs = |w: &World| {
        std::iter::once(p)
            .chain(others.iter().copied())
            .map(|id| w.players[&id].character.tombs)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        tombs(&w),
        vec![1, 0, 0],
        "boss damagers within 25 tiles of the anchor only"
    );
    run(&mut w, anchor, 2000);
    assert_eq!(tombs(&w), vec![1, 0, 0], "no repeat credit");
}

#[test]
fn xml_runtime_world_manor_vampire_summoner_turns_ruthven_defeat_into_one_realm_portal() {
    use crate::dungeon::manor::{BOSS, SUMMONER};
    let content = Arc::new(crate::world::test_content());
    let map = Arc::new(
        crate::dungeon::generate_selected(&content, "manor-of-the-immortals", 12).unwrap(),
    );
    let mut w = World::in_domain(content.clone(), map, WorldDomain::NonRealm).unwrap();
    w.name = "Manor of the Immortals".into();
    w.xml.template = Some("manor-of-the-immortals".into());
    w.encounter = true;
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Hunter".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    let summoner = w
        .entities
        .values()
        .find(|e| e.kind == SUMMONER)
        .unwrap()
        .pos;
    let ruthven = w.entities.values().find(|e| e.kind == BOSS).unwrap().id;
    let portals = |w: &World| {
        (w.entities.values())
            .filter(|e| e.kind == 0x704)
            .map(|e| e.pos)
            .collect::<Vec<_>>()
    };
    let mut now = 0;
    let mut run = |w: &mut World, ms: u64| {
        for _ in 0..ms / 50 {
            now += 50;
            w.players.get_mut(&p).unwrap().pos = summoner;
            frame(w, now);
        }
    };
    run(&mut w, 500);
    assert!(portals(&w).is_empty());
    let e = w.entities.get_mut(&ruthven).unwrap();
    (e.hp, e.condition) = (1, 0);
    w.damage_enemy(p, ruthven, 100_000, true, &[], None)
        .unwrap();
    drain_events(&mut w).unwrap();
    run(&mut w, 2000);
    assert!(!w.entities.values().any(|e| e.kind == SUMMONER));
    // manorOfTheImmortals.xml:939-957: noneExist Lord Ruthven -> Decay -> MetamorphoseAtDeath Realm Portal.
    let exits = portals(&w);
    assert!(
        exits.len() == 1 && exits[0].distance(summoner) < 1.,
        "{exits:?}"
    );
    assert_eq!(w.players[&p].character.manors, 1);
    run(&mut w, 2000);
    assert_eq!((portals(&w).len(), w.players[&p].character.manors), (1, 1));
}

#[test]
fn xml_runtime_world_tomb_switches_and_sarcophagi_toss_their_only_traps_and_priests_once() {
    use crate::dungeon::tomb::{DISABLED_TRAP, SARCOPHAGUS, SWITCH, TRAP};
    let content = Arc::new(crate::world::test_content());
    let map =
        Arc::new(crate::dungeon::generate_selected(&content, "tomb-of-the-ancients", 12).unwrap());
    let placed = |kind| map.cells.iter().filter(|c| c.object == Some(kind)).count();
    assert_eq!(
        [TRAP, 0xd2a, 0xd2b].map(placed),
        [0, 0, 0],
        "dungeons.xml:1532/1569 place no room children"
    );
    let mut w = World::in_domain(content.clone(), map, WorldDomain::NonRealm).unwrap();
    w.xml.template = Some("tomb-of-the-ancients".into());
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Digger".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    let near = |w: &World, at: Pos, kinds: &[u16]| {
        let mut found: Vec<_> = w
            .entities
            .values()
            .filter(|e| kinds.contains(&e.kind) && e.pos.distance(at) < 15.)
            .map(|e| e.kind)
            .collect();
        found.sort();
        found
    };
    let mut now = 0;
    let mut run = |w: &mut World, at: Pos, ms: u64| {
        w.players.get_mut(&p).unwrap().pos = at;
        for _ in 0..ms / 50 {
            now += 50;
            frame(w, now);
        }
    };
    let sarc = w
        .entities
        .values()
        .find(|e| e.kind == SARCOPHAGUS)
        .unwrap()
        .pos;
    run(&mut w, sarc, 3000);
    let priests = [[0xd2a; 4], [0xd2b; 4]].concat();
    assert_eq!(
        near(&w, sarc, &[0xd2a, 0xd2b]),
        priests,
        "actors.xml:7112-7119 eight ObjectToss priests"
    );
    run(&mut w, sarc, 5000);
    assert_eq!(
        near(&w, sarc, &[0xd2a, 0xd2b]),
        priests,
        "cooldown 36000 tosses once"
    );
    let switch = w.entities.values().find(|e| e.kind == SWITCH).unwrap().pos;
    run(&mut w, switch, 2000);
    let traps = near(&w, switch, &[TRAP]);
    assert!(
        (1..=6).contains(&traps.len()),
        "six ObjectToss traps, minus blocked landings: {traps:?}"
    );
    run(&mut w, switch, 3000);
    assert!(near(&w, switch, &[TRAP]).is_empty());
    assert_eq!(
        near(&w, switch, &[DISABLED_TRAP]).len(),
        traps.len(),
        "Done orders every local trap to Metamorphose"
    );
}

#[test]
fn xml_runtime_world_damage_wire_amount_is_the_clients_unsigned_short_while_full_damage_applies() {
    let (mut w, p) = native_world(0x322, 0xab1);
    let target = spawn(&mut w, 0x601, 66.5, 64.5);
    w.players.get_mut(&p).unwrap().known.insert(target);
    let mut rx = output(&mut w, p);
    for (raw, shown) in [(40_000, 40_000u16), (70_000, u16::MAX)] {
        let e = w.entities.get_mut(&target).unwrap();
        (e.hp, e.condition) = (1_000_000, 0);
        let applied = w.damage_enemy(p, target, raw, true, &[], None).unwrap();
        assert_eq!((applied, w.entities[&target].hp), (raw, 1_000_000 - raw));
        let packet = drain(&mut rx)
            .into_iter()
            .find(|packet| packet.id == 47)
            .unwrap();
        let mut r = wire::Reader::new(&packet.body);
        assert_eq!(r.i32().unwrap(), target);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.i16().unwrap() as u16, shown);
    }
}

#[test]
fn xml_runtime_world_quest_defeat_credits_qualified_damagers_and_notifies_arrow_holders_once() {
    let (mut w, a) = native_world(0x322, 0xab1);
    w.name = "Realm".into();
    w.xml.template = Some("realm".into());
    w.encounter = true;
    let mut players = vec![a];
    for id in [2, 3] {
        let mut ch = Character::fresh(w.content.get(0x322).unwrap()).unwrap();
        (ch.id, ch.account, ch.name) = (id, id, format!("Hero{id}"));
        players.push(w.enter(ch, None).unwrap());
    }
    let giant = spawn(&mut w, 0x678, 64.5, 64.5);
    w.set_quest(giant, Some(giant));
    // a damages from inside 25 tiles, b only watches from inside, c damaged from 36 tiles away.
    let spots = [
        Pos { x: 66.5, y: 64.5 },
        Pos { x: 70.5, y: 64.5 },
        Pos { x: 64.5, y: 100.5 },
    ];
    let mut rx = Vec::new();
    for p in &players {
        let player = w.players.get_mut(p).unwrap();
        player.character.level = 10;
        player.lab_conditions = crate::condition::bit(crate::condition::INVINCIBLE);
    }
    for p in &players {
        rx.push(output(&mut w, *p));
    }
    let run = |w: &mut World, steps: usize| {
        for _ in 0..steps {
            for (p, at) in players.iter().zip(spots) {
                w.players.get_mut(p).unwrap().pos = at;
            }
            w.step();
            drain_events(w).unwrap();
        }
    };
    run(&mut w, 2);
    assert!(players.iter().all(|p| w.players[p].quest == Some(giant)));
    for r in &mut rx {
        drain(r);
    }
    let e = w.entities.get_mut(&giant).unwrap();
    (e.hp, e.condition) = (1, 0);
    e.damage.insert(3, 5);
    w.players.get_mut(&a).unwrap().pending = true;
    let _ = w.damage_enemy(a, giant, 100_000, true, &[], None);
    assert!(w.xml.facts.contains_key(&giant) && !w.xml.terminals.contains_key(&giant));
    w.updates(players[1], false).unwrap();
    assert_eq!(w.players[&players[1]].quest, Some(giant));
    assert!(drain(&mut rx[1]).iter().all(|packet| packet.id != 34));
    w.players.get_mut(&a).unwrap().pending = false;
    run(&mut w, 10);
    assert!(!w.entities.contains_key(&giant));
    let notices = |rx: &mut tokio::sync::mpsc::Receiver<Packet>| {
        drain(rx)
            .into_iter()
            .filter(|packet| packet.id == wire::NOTIFICATION)
            .map(|packet| {
                let mut r = wire::Reader::new(&packet.body);
                r.i32().unwrap();
                r.utf().unwrap()
            })
            .filter(|text| text == "Quest Complete!")
            .count()
    };
    let credited: Vec<_> = players
        .iter()
        .map(|p| w.players[p].character.lifetime.qualified_quests)
        .collect();
    assert_eq!(
        credited,
        vec![1, 0, 0],
        "alive damager within 25 tiles only"
    );
    // The arrow retargets only after 'Quest Complete!', as base's death-time notification did.
    let order: Vec<_> = drain(&mut rx[1])
        .into_iter()
        .filter_map(|packet| match packet.id {
            wire::NOTIFICATION => {
                let mut r = wire::Reader::new(&packet.body);
                r.i32().unwrap();
                (r.utf().unwrap() == "Quest Complete!").then_some("complete")
            }
            34 => Some("retarget"),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec!["complete", "retarget"]);
    assert_eq!(
        rx.iter_mut().map(notices).collect::<Vec<_>>(),
        vec![1, 0, 0],
        "arrow holders within 25 tiles"
    );
    run(&mut w, 10);
    assert_eq!(
        rx.iter_mut().map(notices).collect::<Vec<_>>(),
        vec![0, 0, 0],
        "once per committed terminal"
    );
    assert_eq!(w.players[&a].character.lifetime.qualified_quests, 1);
}

#[test]
fn xml_runtime_world_single_spawn_states_without_starting_minions_emit_on_entry() {
    // Cooldown-1000 startWithMinions=false states spawn once on entry (behavior.cc:1064-1095).
    let (mut w, p) = native_world(0x322, 0xab1);
    w.name = "Realm".into();
    w.xml.template = Some("realm".into());
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    let crystal = spawn(&mut w, 0x935, 64.5, 64.5);
    let spawner = spawn(&mut w, 0xe28, 58.5, 64.5);
    for _ in 0..200 {
        w.players.get_mut(&p).unwrap().pos = Pos { x: 61.5, y: 64.5 };
        // Hold the Crystal below every threshold so its 5s Evaluation heal cannot stall the break.
        if let Some(e) = w.entities.get_mut(&crystal).filter(|e| !e.reserved) {
            e.hp = 1;
        }
        w.step();
        drain_events(&mut w).unwrap();
    }
    let kinds: Vec<_> = w.entities.values().map(|e| e.kind).collect();
    assert!(
        !w.entities.contains_key(&crystal) && kinds.contains(&0x941),
        "BreakCrystal releases its Prisoner"
    );
    assert!(
        !w.entities.contains_key(&spawner) && kinds.contains(&0xe26),
        "MakeSnake spawns its guard before Decay"
    );
}

#[test]
fn xml_runtime_world_heal_includes_a_host_in_its_own_target_group() {
    // Mysterious Crystal heals itself in Evaluation (actors.xml:4236); behavior.cc:1306-1317 scans the host too.
    let (mut w, p) = native_world(0x322, 0xab1);
    w.name = "Realm".into();
    w.xml.template = Some("realm".into());
    let crystal = spawn(&mut w, 0x935, 64.5, 64.5);
    w.entities.get_mut(&crystal).unwrap().hp = 996_000;
    for _ in 0..140 {
        w.players.get_mut(&p).unwrap().pos = Pos { x: 61.5, y: 64.5 };
        w.step();
        drain_events(&mut w).unwrap();
    }
    assert_eq!(w.entities[&crystal].hp, 1_000_000);
}

#[test]
fn xml_rewards_world_followers_carrying_the_quest_tag_never_credit_the_realm_line() {
    let (mut w, p) = native_world(0x322, 0xab1);
    w.name = "Realm".into();
    w.xml.template = Some("realm".into());
    w.encounter = true;
    let giant = spawn(&mut w, 0x678, 64.5, 64.5);
    w.set_quest(giant, Some(giant));
    // Landmark follower, event child and Hermit spawner tags (landmark_ai.rs, realm_events.rs, ocean_setpiece.rs).
    for (i, kind) in [0x679, 0xd57, 0xd65].into_iter().enumerate() {
        let follower = w
            .spawn_entity(
                kind,
                Pos {
                    x: 60.5 + i as f32,
                    y: 70.5,
                },
                vec![],
            )
            .unwrap();
        w.set_quest(follower, Some(giant));
        let e = w.entities.get_mut(&follower).unwrap();
        (e.hp, e.condition) = (1, 0);
        let _ = w.damage_enemy(p, follower, 100_000, true, &[], None);
        for _ in 0..4 {
            drain_events(&mut w).unwrap();
            let now = w.now + 50;
            frame(&mut w, now);
        }
        assert!(!w.entities.contains_key(&follower), "{kind:#x} defeated");
        assert!(
            w.critical_deaths.is_empty() && w.critical_killers.is_empty(),
            "{kind:#x} credited the root"
        );
    }
    let e = w.entities.get_mut(&giant).unwrap();
    (e.hp, e.condition) = (1, 0);
    let _ = w.damage_enemy(p, giant, 100_000, true, &[], None);
    for _ in 0..4 {
        drain_events(&mut w).unwrap();
        let now = w.now + 50;
        frame(&mut w, now);
    }
    assert_eq!(w.critical_deaths, vec![giant]);
    assert_eq!(
        w.critical_killers.get(&giant),
        Some(&(0x678, "Ada".to_string()))
    );
}

/// Spawns `kind` with a preset ledger, runs World::step until it self-ends, and returns each player's
/// (kills, quests, lifetime kills) plus the committed terminal events.
type SelfEnd = (World, Vec<[u64; 3]>, Vec<crate::store::world_events::Event>);
fn self_end(kind: u16, hp: i32, ms: u64, pending_ms: u64) -> SelfEnd {
    let (mut w, a) = native_world(0x322, 0xab1);
    w.name = "Realm".into();
    w.xml.template = Some("realm".into());
    // Actor seeds otherwise follow the process-wide World counter, so death-hook chances varied with test order.
    w.prepared_seed = Some(1);
    let mut players = vec![a];
    for id in [2, 3] {
        let mut ch = Character::fresh(w.content.get(0x322).unwrap()).unwrap();
        (ch.id, ch.account, ch.name) = (id, id, format!("Hero{id}"));
        players.push(w.enter(ch, None).unwrap());
    }
    let at = Pos { x: 64.5, y: 64.5 };
    let id = w.spawn_entity(kind, at, vec![]).unwrap();
    let e = w.entities.get_mut(&id).unwrap();
    e.hp = hp;
    // Character 99 is absent (departed), so its larger share is ineligible; character 3 stays 36 tiles away.
    e.damage = BTreeMap::from([(1, 100), (2, 300), (3, 200), (99, 900)]);
    let mut events = Vec::new();
    for now in (0..ms).step_by(50) {
        for (i, p) in players.iter().enumerate() {
            let player = w.players.get_mut(p).unwrap();
            player.pos = if i == 2 {
                Pos { x: 64.5, y: 100.5 }
            } else {
                Pos {
                    x: 65.5 + i as f32,
                    y: 64.5,
                }
            };
            player.lab_conditions = crate::condition::bit(crate::condition::INVINCIBLE);
            if i == 1 && w.entities.contains_key(&id) {
                // Hero2 follows this actor's quest arrow; the Realm quest selector would otherwise retarget it.
                player.quest = Some(id);
            }
        }
        // A pending recipient holds the captured terminal in progress backpressure (the retry path).
        if now <= pending_ms && pending_ms > 0 {
            if now == pending_ms {
                assert!(
                    w.xml.facts.contains_key(&id) && w.entities.contains_key(&id),
                    "captured, not yet staged"
                );
            }
            w.players.get_mut(&players[1]).unwrap().pending = now < pending_ms;
        }
        w.step();
        if now >= pending_ms {
            events.extend(w.pending_terminal_events());
            drain_events(&mut w).unwrap();
        }
    }
    let counts = players
        .iter()
        .map(|p| {
            let ch = &w.players[p].character;
            [ch.kills, ch.quests, ch.lifetime.kills]
        })
        .collect();
    (w, counts, events)
}

#[test]
fn xml_rewards_world_rewarded_self_endings_credit_the_largest_eligible_damager_once() {
    // Djinn (actors.xml:3196-3202) and Lord of the Lost Lands (actors.xml:7951-7957) Explode with dropLoot/giveXP true.
    for (kind, hp, quests, pending_ms) in [(0x91a, 400, 0, 2000), (0xd50, 1000, 1, 5000)] {
        let (mut w, counts, events) = self_end(kind, hp, 9000, pending_ms);
        assert!(
            w.entities.values().all(|e| e.kind != kind),
            "{kind:#x} self-ended"
        );
        assert_eq!(
            counts,
            vec![[0; 3], [1, quests, 1], [0; 3]],
            "{kind:#x}: largest eligible damager only"
        );
        // P2-D: LotLL's rewarded Explode ends its quest like a defeat: the arrow holder is told and the damager qualifies.
        if quests == 1 {
            let publication = &events[0].terminal_payload().unwrap().actions["publication"];
            assert_eq!(
                publication["quest_complete"],
                serde_json::json!([w
                    .players
                    .iter()
                    .find(|(_, p)| p.character.id == 2)
                    .unwrap()
                    .0])
            );
            let hero = w.players.values().find(|p| p.character.id == 2).unwrap();
            assert_eq!(hero.character.lifetime.qualified_quests, 1);
        }
        // Restart replay of the committed receipt credits nothing again.
        let event = events.first().expect("journaled self-ending").clone();
        let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
            .join(format!("world-terminal-{}.db", w.xml.instance));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let store = crate::store::Store::open(&path, &w.content).unwrap();
        let record = runtime.block_on(store.coordinate_terminal(event)).unwrap();
        w.finish_terminal_event(record).unwrap();
        let saved = runtime.block_on(store.load(2, 2)).unwrap();
        assert_eq!(
            (saved.kills, saved.quests, saved.lifetime.kills),
            (1, quests, 1)
        );
        assert_eq!(
            w.players
                .values()
                .find(|p| p.character.id == 2)
                .unwrap()
                .character
                .kills,
            1
        );
    }
}

#[test]
fn xml_rewards_world_unrewarded_self_endings_credit_nobody_and_publish_without_a_journal() {
    // Coffin Creature Decays with dropLoot/giveXP false (actors.xml:13350-13371); Stheno Swarm's Decay uses the false defaults.
    // Coffin Creature's MetamorphoseAtDeath hooks (actors.xml:13372-13374) are births that need the journal; the
    // Swarm has no account, loot, birth, clear or quest effect, so it publishes without an event or held permit.
    for (kind, ms, journaled) in [(0x1722, 23_000, true), (0x919, 8000, false)] {
        let (w, counts, events) = self_end(kind, 10, ms, 0);
        assert!(
            w.entities.values().all(|e| e.kind != kind),
            "{kind:#x} self-ended"
        );
        assert_eq!(!events.is_empty(), journaled, "{kind:#x}");
        assert!(w.xml.terminals.is_empty() && w.xml.facts.is_empty());
        assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
        assert_eq!(counts, vec![[0; 3]; 3], "{kind:#x}");
    }
}

#[test]
fn xml_runtime_world_white_fountain_heals_only_the_closest_damaged_player_with_base_feedback() {
    let (mut w, a) = native_world(0x322, 0xab1);
    let mut players = vec![a];
    for id in [2, 3] {
        let mut ch = Character::fresh(w.content.get(0x322).unwrap()).unwrap();
        (ch.id, ch.account, ch.name) = (id, id, format!("Hero{id}"));
        players.push(w.enter(ch, None).unwrap());
    }
    spawn(&mut w, 0x1c7, 64.5, 64.5);
    // a is damaged at 2 tiles, b is damaged at 1 tile, c is closest at 0.5 tiles but at full HP.
    let b = players[1];
    let spots = [
        Pos { x: 66.5, y: 64.5 },
        Pos { x: 65.5, y: 64.5 },
        Pos { x: 65., y: 64.5 },
    ];
    let max = |w: &World, p: i32| w.players[&p].effective(&w.content, w.now).unwrap()[0];
    for p in [a, b] {
        w.players.get_mut(&p).unwrap().character.hp = 1;
    }
    let mut rx = output(&mut w, a);
    let (mut healed, mut packets) = (Vec::new(), Vec::new());
    for _ in 0..90 {
        for (p, at) in players.iter().zip(spots) {
            w.players.get_mut(p).unwrap().pos = at;
        }
        let before: Vec<_> = players.iter().map(|p| w.players[p].character.hp).collect();
        w.step();
        // Natural regeneration adds a few HP per second; a fountain zap adds up to 100.
        let gained: Vec<_> = players
            .iter()
            .zip(before)
            .filter(|(p, hp)| w.players[p].character.hp > hp + 20)
            .map(|(p, _)| *p)
            .collect();
        assert!(gained.len() <= 1, "one player per cooldown: {gained:?}");
        healed.extend(gained);
        packets.extend(drain(&mut rx));
    }
    // b is zapped until full before the farther a is touched; full-HP c never.
    let first = healed.iter().take_while(|p| **p == b).count();
    assert!(
        first > 0 && first < healed.len() && healed[first..].iter().all(|p| *p == a),
        "{healed:?}"
    );
    let zaps = |p: i32| healed.iter().filter(|h| **h == p).count();
    assert!(
        [a, b]
            .iter()
            .all(|p| w.players[p].character.hp == max(&w, *p))
    );
    let text = |id: i32| {
        packets
            .iter()
            .filter(|packet| packet.id == wire::NOTIFICATION)
            .filter_map(|packet| {
                let mut r = wire::Reader::new(&packet.body);
                (r.i32().unwrap() == id).then(|| r.utf().unwrap())
            })
            .collect::<Vec<_>>()
    };
    for p in [a, b] {
        let shown = text(p);
        assert_eq!(shown.len(), zaps(p));
        assert!(
            shown
                .iter()
                .all(|t| t.strip_prefix('+').unwrap().parse::<i32>().unwrap() > 20),
            "{shown:?}"
        );
    }
    let heal_effects = packets
        .iter()
        .filter(|packet| packet.id == wire::SHOW_EFFECT)
        .filter(|packet| {
            let mut r = wire::Reader::new(&packet.body);
            r.u8().unwrap() == 1 && r.i32().unwrap() == b
        })
        .count();
    assert_eq!(heal_effects, zaps(b));
}

#[test]
fn xml_runtime_world_scripted_tutorial_loot_bag_is_a_swap_source() {
    let (mut w, p) = native_world(0x30e, 0xa2e);
    let pos = w.players[&p].pos;
    let bag = w
        .spawn_entity(0x806, pos, vec![0xa4f, -1, -1, -1, -1, -1, -1, -1])
        .unwrap();
    let source = Slot {
        object: bag,
        slot: 0,
        kind: 0xa4f,
    };
    let destination = Slot {
        object: p,
        slot: 11,
        kind: -1,
    };
    assert_eq!(w.swap(p, source, destination).unwrap().items[11], 0xa4f);
}

#[test]
fn xml_runtime_world_loot_bag_rests_under_player_blocking_scenery_but_not_walls() {
    let (mut w, p) = native_world(0x30e, 0xa2e);
    spawn(&mut w, 0x100, 70.5, 64.5);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 69.6, y: 64.5 };
    let bag = w
        .loot_bag(Pos { x: 70.3, y: 64.5 }, Some(1), vec![0xa4f])
        .unwrap();
    let source = Slot {
        object: bag,
        slot: 0,
        kind: 0xa4f,
    };
    let destination = Slot {
        object: p,
        slot: 11,
        kind: -1,
    };
    assert_eq!(w.swap(p, source, destination).unwrap().items[11], 0xa4f);
    spawn(&mut w, 0x120, 72.5, 64.5);
    let error = w
        .loot_bag(Pos { x: 72.3, y: 64.5 }, Some(1), vec![0xa4f])
        .unwrap_err();
    assert!(format!("{error:#}").contains("native placement blocked"));
}

#[test]
fn xml_runtime_world_shield_orcs_cannot_take_explode_order_after_key_retires() {
    let (mut w, player) = native_world(0x30e, 0xa2e);
    let pos = w.players[&player].pos;
    let key = w
        .spawn_entity(
            0x900,
            Pos {
                x: pos.x + 3.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    for now in (0..=1000).step_by(50) {
        frame(&mut w, now);
    }
    let children = [0x901, 0x902].map(|kind| {
        w.entities
            .values()
            .find(|entity| entity.kind == kind && *entity.parent == Some(key))
            .expect("Key spawns each recovered listener")
            .id
    });
    assert!(children.iter().all(|child| *child > key));
    assert!(w.xml.orders.keys().any(|(issuer, _)| *issuer == key));
    w.entities.get_mut(&key).unwrap().hp = 499;
    for now in (1050..=5000).step_by(50) {
        frame(&mut w, now);
    }
    // FollowOrders polls a living issuer (behavior.cc:1460-1489); Explode kills synchronously (639-667).
    assert!(!w.entities.contains_key(&key));
    assert!(!w.xml.orders.keys().any(|(issuer, _)| *issuer == key));
    assert!(children.iter().all(|child| w.entities[child].hp > 0));
}

#[test]
fn xml_runtime_world_party_god_alt_texture_clears_on_xml_state_exit() {
    let (mut w, player) = native_world(0x30e, 0xa2e);
    let pos = w.players[&player].pos;
    let boss = w.spawn_entity(0xe56, pos, vec![]).unwrap();
    let texture = |w: &World| {
        w.entities[&boss]
            .extra_stats
            .iter()
            .find_map(|(stat, value)| (*stat == 61).then_some(*value))
            .unwrap_or(0)
    };
    for at in (0..=750).step_by(50) {
        frame(&mut w, at);
        assert_eq!(texture(&w), 0);
    }
    frame(&mut w, 800);
    assert_eq!(
        texture(&w),
        0,
        "entered Two; its Update runs on the next tick"
    );
    for at in (850..=1550).step_by(50) {
        frame(&mut w, at);
        assert_eq!(texture(&w), 3);
    }
    frame(&mut w, 1600);
    assert_eq!(texture(&w), 0, "Two clears before One's next update");
}

#[test]
fn xml_runtime_world_party_god_heals_every_200ms_while_stunned_and_backs_off_when_full() {
    let (mut w, player) = native_world(0x30e, 0xa2e);
    let boss = w
        .spawn_entity(0xe56, w.players[&player].pos, vec![])
        .unwrap();
    let mut rx = output(&mut w, player);
    w.players.get_mut(&player).unwrap().known.insert(boss);
    frame(&mut w, 0);
    w.apply_condition(boss, crate::condition::STUNNED, 2000)
        .unwrap();
    w.entities.get_mut(&boss).unwrap().hp = 22500;
    for (at, expected) in [
        (50, 22500),
        (950, 22500),
        (1000, 25500),
        (1150, 25500),
        (1200, 28500),
        (1350, 28500),
        (1400, 30000),
    ] {
        frame(&mut w, at);
        assert_eq!(w.entities[&boss].hp, expected, "unexpected Heal at {at} ms");
        assert_ne!(
            w.entities[&boss].conditions(at) & crate::condition::bit(crate::condition::STUNNED),
            0
        );
    }
    assert!(
        drain(&mut rx)
            .iter()
            .all(|packet| packet.id != wire::SHOW_EFFECT),
        "hideVisual must suppress the heal line"
    );
}
