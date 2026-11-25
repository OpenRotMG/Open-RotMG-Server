//! Phase-5 cutover gate: every admitted root/spawn/death/transform reference has a complete XML definition,
//! durable progress keys stay stable, worlds stay isolated and no Rust path moves an actor without XML.
use super::*;
use crate::content::game::geometry::Geometry;
use crate::content::game::{AdmissionRole, Emission, GameData, Predicate, ProgressKey};

/// Independent reference walk; the compiler's own dependency sets are not reused.
fn referenced(game: &GameData) -> BTreeSet<u16> {
    let mut maps: Vec<u32> = game.worlds.iter().map(|w| w.map).collect();
    let mut queue: Vec<u16> = game.roots.iter().copied().collect();
    queue.extend(
        game.worlds
            .iter()
            .flat_map(|w| w.populations.iter().map(|p| p.object)),
    );
    for chest in game.landmark_chests.values() {
        queue.extend([chest.landmark, chest.container]);
        queue.extend(chest.rows.iter().flat_map(|r| r.pool.iter().copied()));
    }
    let (mut seen, mut visited_maps) = (BTreeSet::new(), BTreeSet::new());
    loop {
        while let Some(index) = maps.pop() {
            if visited_maps.insert(index) {
                let map = &game.maps[index as usize];
                queue.extend(map.objects.iter().copied());
                if let Geometry::Pool(pool) = &map.geometry {
                    maps.extend(pool.entries.iter().copied());
                }
            }
        }
        let Some(kind) = queue.pop() else { break };
        if !seen.insert(kind) {
            continue;
        }
        queue.extend(
            game.property_dependencies
                .get(&kind)
                .into_iter()
                .flatten()
                .copied(),
        );
        queue.extend(
            game.deployment_activations
                .get(&kind)
                .into_iter()
                .flatten()
                .map(|a| a.target),
        );
        let Some(d) = game.definitions.get(&kind) else {
            continue;
        };
        queue.extend(d.loot.iter().flat_map(|r| r.pool.iter().copied()));
        for s in &d.states {
            queue.extend(s.orders.iter().flat_map(|o| o.targets.iter().flatten()));
            for p in s.transitions.iter().flat_map(|t| &t.predicates) {
                if let Predicate::Exists(kinds) | Predicate::NoneExist(kinds) = p {
                    queue.extend(kinds);
                }
            }
        }
        for b in d.states.iter().flat_map(|s| &s.behaviors) {
            if let Some(Emission::Child(child)) = b.emission {
                queue.push(child);
            }
            for value in b.fields.values() {
                match value {
                    Value::Objects(kinds) => queue.extend(kinds.iter().copied()),
                    Value::Map(index) => maps.push(*index),
                    _ => {}
                }
            }
        }
    }
    seen
}

#[test]
fn xml_cutover_roots_spawn_death_transform_closure_is_complete_and_keys_are_stable() {
    let content = crate::world::test_content();
    let game = content.game.as_ref().unwrap();
    let ordinary: Vec<_> = content
        .objects
        .values()
        .filter(|o| {
            o.enemy
                && o.terrain > 0
                && o.spawn_weight.is_some_and(|w| w > 0.)
                && o.per_realm_max.is_none()
        })
        .map(|o| o.kind)
        .collect();
    let frozen: Vec<_> = include_str!("../../../tests/fixtures/xml_contract/ordinary-roots.txt")
        .split_whitespace()
        .map(|s| u16::from_str_radix(s, 16).unwrap())
        .collect();
    assert_eq!(ordinary, frozen);
    assert_eq!(ordinary.len(), 63);
    assert!(!content.objects[&0x173].enemy);
    for id in ordinary.into_iter().chain([0x173]) {
        assert_eq!(
            game.definitions[&id].admission_role().unwrap(),
            AdmissionRole::Scripted,
            "{id:#x}"
        );
    }
    assert_eq!(
        content
            .objects
            .values()
            .filter(|o| o.passive_destructible())
            .map(|o| o.kind)
            .collect::<Vec<_>>(),
        [0x122, 0x162, 0x181f]
    );
    let walked = referenced(game);
    assert_eq!(
        (
            walked.difference(&game.reachable).collect::<Vec<_>>(),
            game.reachable.difference(&walked).collect::<Vec<_>>()
        ),
        (vec![], vec![]),
        "independent root/spawn/death/transform walk differs from admitted closure"
    );
    for kind in &walked {
        let d = game
            .definitions
            .get(kind)
            .unwrap_or_else(|| panic!("reachable {kind:#x} lacks an XML definition"));
        game.admission(*kind, d.admission_role().unwrap(), "xml_cutover")
            .unwrap();
        for s in &d.states {
            assert!(s.parent.is_none_or(|p| (p as usize) < d.states.len()));
            assert!(
                s.transitions
                    .iter()
                    .flat_map(|t| &t.targets)
                    .all(|t| (*t as usize) < d.states.len())
            );
        }
    }
    let fixture: Vec<[String; 3]> = serde_json::from_slice(
        &std::fs::read("tests/fixtures/xml_contract/progress.json").unwrap(),
    )
    .unwrap();
    assert_eq!(
        fixture.iter().map(|r| r[2].as_str()).collect::<Vec<_>>(),
        [
            "pirate_caves",
            "dungeons[0]",
            "dungeons[1]",
            "dungeons[2]",
            "abysses",
            "trenches",
            "spider_dens",
            "mad_labs",
            "jungles",
            "manors",
            "beaches",
            "davy_lockers",
            "tombs",
        ]
    );
    assert_eq!(fixture.len(), crate::dungeon::DUNGEONS.len());
    for (index, ([world, boss_id, field], (_, template, _, boss))) in
        fixture.iter().zip(crate::dungeon::DUNGEONS).enumerate()
    {
        assert_eq!(template, world, "clear index {index}");
        assert_eq!(format!("{boss:04x}"), *boss_id, "clear boss {world}");
        let key = serde_json::from_value::<ProgressKey>(serde_json::json!(field)).unwrap();
        assert_eq!(key.index(), index, "durable field {field}");
        assert_eq!(key.world(), world);
    }
    assert_eq!(
        serde_json::to_value(crate::dungeon::HistoricalDungeon::ALL).unwrap(),
        serde_json::json!([
            "pirate_cave",
            "spider_den",
            "forbidden_jungle",
            "snake_pit",
            "sprite_world",
            "undead_lair",
            "abyss_of_demons",
            "tomb_of_the_ancients",
            "ocean_trench",
            "manor_of_the_immortals"
        ])
    );
    let keyed: Vec<_> = game
        .worlds
        .iter()
        .filter_map(|w| Some((w, w.clear_key?)))
        .collect();
    assert_eq!(keyed.len(), fixture.len());
    for (world, key) in keyed {
        let row = fixture.iter().find(|r| r[0] == world.id).unwrap();
        assert_eq!(key.world(), world.id);
        assert_eq!(
            serde_json::to_value(key).unwrap(),
            serde_json::json!(row[2])
        );
        assert_eq!(
            serde_json::from_value::<ProgressKey>(serde_json::json!(row[2])).unwrap(),
            key
        );
    }
}

fn realm(content: Arc<Content>) -> (World, i32) {
    let mut map = flat_map(128);
    map.elevation = Some(vec![120; 128 * 128].into());
    let mut w = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    (ch.account, ch.id, ch.name) = (1, 1, "Ada".into());
    let player = w.enter(ch, None).unwrap();
    let p = w.players.get_mut(&player).unwrap();
    p.pos = Pos { x: 66.5, y: 64.5 };
    p.lab_conditions = crate::condition::bit(crate::condition::INVINCIBLE);
    (w, player)
}

/// Observable actions over two seconds of world time: shots, births/removals and movement.
fn activity(w: &mut World, id: i32) -> Result<bool> {
    let (pos, actors) = (w.entities[&id].pos, w.entities.len());
    for tick in 1..=40 {
        w.now = tick * 50;
        w.xml_step()?;
    }
    Ok(!w.bullets.is_empty()
        || w.entities.len() != actors
        || w.entities
            .get(&id)
            .is_none_or(|e| (e.pos.x, e.pos.y) != (pos.x, pos.y)))
}

#[test]
fn xml_cutover_every_admitted_actor_acts_and_dies_only_through_its_xml_definition() {
    let content = Arc::new(crate::world::test_content());
    let game = content.game.as_ref().unwrap();
    let mut inert = (*content).clone();
    for d in Arc::make_mut(inert.game.as_mut().unwrap())
        .definitions
        .values_mut()
    {
        for s in &mut d.states {
            (s.behaviors, s.transitions) = (vec![], vec![]);
        }
    }
    let inert = Arc::new(inert);
    let (mut active, mut acted, mut refused, mut undefeated) = (0, 0, Vec::new(), Vec::new());
    for (&kind, d) in &game.definitions {
        if !game.reachable.contains(&kind)
            || d.disposition != Disposition::Active
            || d.admission_role().unwrap() != AdmissionRole::Scripted
            || !content.objects[&kind].enemy
        {
            continue;
        }
        active += 1;
        let (mut w, p) = realm(content.clone());
        let Ok(id) = w.spawn_entity(kind, Pos { x: 64.5, y: 64.5 }, vec![]) else {
            refused.push(kind);
            continue;
        };
        acted += usize::from(activity(&mut w, id).unwrap_or_else(|e| panic!("{kind:#x}: {e:#}")));
        // Selected-client hp=0 objects are indestructible scenery-like hazards.
        if let Some(e) = w
            .entities
            .get_mut(&id)
            .filter(|_| content.objects[&kind].hp > 0)
        {
            (e.hp, e.condition) = (1, 0);
            let result = w.damage_enemy(p, id, 1_000_000, true, &[], None);
            if w.entities.contains_key(&id) && !w.xml_terminal_pending(id) {
                undefeated.push(format!("{kind:#x} {result:?}"));
            }
        }
        let (mut w, _) = realm(inert.clone());
        let id = w
            .spawn_entity(kind, Pos { x: 64.5, y: 64.5 }, vec![])
            .unwrap();
        assert!(
            !activity(&mut w, id).unwrap(),
            "{kind:#x} acted with every XML behavior removed: legacy Rust dispatch"
        );
    }
    eprintln!("xml_cutover actors={active} acted={acted} geometry_refused={refused:x?}");
    assert!(active > 400 && acted * 4 > active * 3, "{active} {acted}");
    assert!(refused.is_empty(), "{refused:x?}");
    assert!(undefeated.is_empty(), "{undefeated:#?}");
}

#[test]
fn xml_cutover_world_instances_keep_actors_and_terminal_identities_isolated() {
    let content = Arc::new(crate::world::test_content());
    let (mut a, pa) = realm(content.clone());
    let (mut b, pb) = realm(content);
    assert_ne!(a.xml.instance, b.xml.instance);
    let kind = 0x600;
    let ia = a
        .spawn_entity(kind, Pos { x: 64.5, y: 64.5 }, vec![])
        .unwrap();
    let ib = b
        .spawn_entity(kind, Pos { x: 64.5, y: 64.5 }, vec![])
        .unwrap();
    assert_eq!(ia, ib, "same per-world actor id is the isolation edge case");
    let _ = a.damage_enemy(pa, ia, 1_000_000, true, &[], None);
    assert!(b.entities.contains_key(&ib) && b.pending_terminal_events().is_empty());
    let _ = b.damage_enemy(pb, ib, 1_000_000, true, &[], None);
    let ids = |w: &World| {
        w.pending_terminal_events()
            .iter()
            .map(|e| e.id.clone())
            .collect::<BTreeSet<_>>()
    };
    let (ea, eb) = (ids(&a), ids(&b));
    assert!(!ea.is_empty() && !eb.is_empty() && ea.is_disjoint(&eb));
}
