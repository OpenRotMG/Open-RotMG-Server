use super::*;
use crate::content::{Cell, Map};
use std::{path::Path, sync::Arc};

fn scene(inert: usize) -> World {
    let content = Arc::new(Content::load(Path::new("data")).unwrap());
    let mut cells = vec![
        Cell {
            ground: 0x61,
            object: None,
            config: String::new(),
            terrain: 0,
            region: 0
        };
        256 * 129
    ];
    for cell in cells.iter_mut().skip(1024).take(inert) {
        cell.object = Some(0x1ce);
    }
    World::new(
        content,
        Arc::new(Map {
            width: 256,
            height: 129,
            cells,
            spawns: vec![(1.5, 1.5)],
        }),
    )
    .unwrap()
}
fn member(w: &mut World, kind: u16, x: f32, camp: usize) -> i32 {
    let id = w.spawn_entity(kind, Pos { x, y: 1.5 }, vec![]).unwrap();
    w.entities.get_mut(&id).unwrap().camp = Some(camp);
    assert!(w.actor_ids.contains(&id));
    id
}
fn full_scan(w: &World, id: i32, elapsed: u64) -> (Motion, f32) {
    let e = &w.entities[&id];
    if ROOTS.contains(&e.kind) {
        return if e.pos.distance(e.home) > 16. {
            (
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 1.,
                },
                10.,
            )
        } else {
            crate::encounter::movement(e.kind, e.hp, elapsed)
        };
    }
    let root = w
        .entities
        .values()
        .filter(|x| x.camp == e.camp && ROOTS.contains(&x.kind))
        .min_by(|a, b| e.pos.distance(a.pos).total_cmp(&e.pos.distance(b.pos)));
    if matches!(e.kind, 0x6d5..=0x6d7)
        && let Some(hub) = w
            .entities
            .values()
            .filter(|x| x.camp == e.camp && matches!(x.kind, 0x6d2..=0x6d4))
            .min_by(|a, b| e.pos.distance(a.pos).total_cmp(&e.pos.distance(b.pos)))
    {
        return (
            Motion::StrictOrbit {
                anchor: Anchor::CampEntity(hub.id),
                radius: 1.,
            },
            10.,
        );
    }
    match root {
        Some(root) => (
            Motion::StrictOrbit {
                anchor: Anchor::CampEntity(root.id),
                radius: 3.,
            },
            5.,
        ),
        None => (Motion::Stationary, 0.),
    }
}
#[test]
fn golem_query_full_scan_motion_and_legitimate_actor_index_are_equivalent() {
    let mut w = scene(20000);
    for kind in ROOTS.iter().copied().chain(0x6d2..=0x6d7) {
        assert!(w.content.objects[&kind].enemy);
        let mut map = (*w.map).clone();
        map.cells[0].object = Some(kind);
        let mapped = World::new(w.content.clone(), Arc::new(map)).unwrap();
        let mapped_ids = mapped
            .entities
            .values()
            .filter(|e| e.kind == kind)
            .map(|e| e.id)
            .collect::<Vec<_>>();
        assert_eq!(mapped_ids.len(), 1);
        assert!(mapped_ids.iter().all(|id| mapped.actor_ids.contains(id)));
    }
    let left = member(&mut w, 0x62d, 8.5, 7);
    let right = member(&mut w, 0x62e, 12.5, 7);
    member(&mut w, 0x62f, 10.5, 9);
    let hub1 = member(&mut w, 0x6d2, 9.5, 7);
    let hub2 = member(&mut w, 0x6d3, 11.5, 7);
    member(&mut w, 0x6d4, 10.5, 9);
    for kind in ROOTS.iter().copied().chain(0x6d2..=0x6d7) {
        let id = member(&mut w, kind, 10.5, 7);
        QUERY_VISITS.with(|n| n.set(0));
        assert_eq!(w.golem_movement(id, 1000).unwrap(), full_scan(&w, id, 1000));
        if ROOTS.contains(&kind) {
            assert_eq!(QUERY_VISITS.with(|n| n.get()), 0);
        }
        w.entities.get_mut(&left).unwrap().hp = 0;
        assert!(w.actor_ids.contains(&left));
        assert_eq!(w.golem_movement(id, 1000).unwrap(), full_scan(&w, id, 1000));
        w.remove_entity(id);
        assert!(!w.actor_ids.contains(&id));
    }
    for id in [hub1, hub2, left, right] {
        w.remove_entity(id);
    }
    for kind in 0x6d2..=0x6d7 {
        let id = member(&mut w, kind, 10.5, 7);
        assert_eq!(w.golem_movement(id, 1000).unwrap(), full_scan(&w, id, 1000));
        w.remove_entity(id);
    }
}
#[test]
fn golem_query_actual_path_visits_actors_not_inert_scenery() {
    for inert in [0, 20000] {
        let mut w = scene(inert);
        member(&mut w, 0x62d, 8.5, 7);
        member(&mut w, 0x62e, 12.5, 9);
        member(&mut w, 0x6d2, 9.5, 7);
        let id = member(&mut w, 0x6d5, 10.5, 7);
        let expected = full_scan(&w, id, 1000);
        QUERY_VISITS.with(|n| n.set(0));
        assert_eq!(w.golem_movement(id, 1000).unwrap(), expected);
        let visited = QUERY_VISITS.with(|n| n.get());
        println!(
            "golem actual query cost inert={inert} entities={} actors={} queries=2 visited={visited} result={expected:?}",
            w.entities.len(),
            w.actor_ids.len()
        );
        assert_eq!(
            visited,
            w.actor_ids.len() * 2,
            "actual golem selectors must not traverse immutable scenery"
        );
    }
}
