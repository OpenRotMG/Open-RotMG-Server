use super::*;
use crate::content::{Cell, Map};
use std::{path::Path, sync::Arc};

// Mixed-kind reservations exercise queries, not natural family generation or habitat.
const QUERY_ROLES: [population::Role; 12] = {
    let kinds = [
        0x62d, 0x62e, 0x62f, 0x631, 0x632, 0x633, 0x6d2, 0x6d3, 0x6d4, 0x6d5, 0x6d6, 0x6d7,
    ];
    let mut roles = [const {
        population::Role {
            kind: 0,
            maximum: 2,
            initial: 0,
            interval: None,
        }
    }; 12];
    let mut i = 0;
    while i < kinds.len() {
        roles[i].kind = kinds[i];
        i += 1;
    }
    roles
};
const QUERY_FAMILY: population::Family = population::Family {
    leader: 0x62d,
    terrain: 7,
    roles: &QUERY_ROLES,
    random_initial: false,
};

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
    let mut w = World::new(
        content,
        Arc::new(Map {
            width: 256,
            height: 129,
            cells,
            spawns: vec![(1.5, 1.5)],
        }),
    )
    .unwrap();
    w.population_regions[7].limit = 8;
    for camp in 0..2 {
        w.place_camp(
            &QUERY_FAMILY,
            vec![Pos { x: 1.5, y: 1.5 }; QUERY_FAMILY.size()],
        )
        .unwrap();
        let initial = w.population[w.camps[camp].start].entity.unwrap();
        w.remove_entity(initial);
    }
    w
}
fn member(w: &mut World, kind: u16, x: f32, camp: usize) -> i32 {
    let c = w.camps.get(camp).expect("query fixture camp missing");
    let slot = (c.start..c.start + c.family.size())
        .find(|i| {
            let s = &w.population[*i];
            s.entity.is_none()
                && (s.kind == kind
                    || matches!(kind, 0x6d2..=0x6d4) && matches!(s.kind, 0x6d2..=0x6d4))
        })
        .expect("query fixture camp has no free matching reservation");
    w.place_member(camp, slot, Pos { x, y: 1.5 }).unwrap();
    let id = w.population[slot].entity.unwrap();
    let e = &w.entities[&id];
    if matches!(kind, 0x6d2..=0x6d4) {
        assert!(matches!(e.kind, 0x6d2..=0x6d4));
    } else {
        assert_eq!(e.kind, kind);
    }
    assert_eq!(w.population[slot].kind, e.kind);
    assert_eq!(w.population[slot].camp, Some(camp));
    assert_eq!(e.camp, Some(camp));
    assert_eq!(e.population_slot, Some(slot));
    assert_eq!((e.pos.x, e.pos.y), (x, 1.5));
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
        .min_by(|a, b| {
            e.pos
                .distance(a.pos)
                .total_cmp(&e.pos.distance(b.pos))
                .then_with(|| a.id.cmp(&b.id))
        });
    if matches!(e.kind, 0x6d5..=0x6d7)
        && let Some(hub) = w
            .entities
            .values()
            .filter(|x| x.camp == e.camp && matches!(x.kind, 0x6d2..=0x6d4))
            .min_by(|a, b| {
                e.pos
                    .distance(a.pos)
                    .total_cmp(&e.pos.distance(b.pos))
                    .then_with(|| a.id.cmp(&b.id))
            })
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
    let left = member(&mut w, 0x62d, 8.5, 0);
    let right = member(&mut w, 0x62e, 12.5, 0);
    member(&mut w, 0x62f, 10.5, 1);
    let hub1 = member(&mut w, 0x6d2, 9.5, 0);
    let hub2 = member(&mut w, 0x6d3, 11.5, 0);
    member(&mut w, 0x6d4, 10.5, 1);
    for kind in ROOTS.iter().copied().chain(0x6d2..=0x6d7) {
        let id = member(&mut w, kind, 10.5, 0);
        QUERY_VISITS.with(|n| n.set(0));
        assert_eq!(w.golem_movement(id, 1000).unwrap(), full_scan(&w, id, 1000));
        if ROOTS.contains(&kind) {
            assert_eq!(QUERY_VISITS.with(|n| n.get()), 0);
            let home = w.entities[&id].home;
            w.entities.get_mut(&id).unwrap().home.x += 40.;
            assert_eq!(
                w.golem_movement(id, 1000).unwrap(),
                (
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 1.
                    },
                    10.
                )
            );
            assert_eq!(w.golem_movement(id, 1000).unwrap(), full_scan(&w, id, 1000));
            assert_eq!(QUERY_VISITS.with(|n| n.get()), 0);
            w.entities.get_mut(&id).unwrap().home = home;
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
        let id = member(&mut w, kind, 10.5, 0);
        assert_eq!(w.golem_movement(id, 1000).unwrap(), full_scan(&w, id, 1000));
        w.remove_entity(id);
    }
}
#[test]
fn golem_query_actual_path_visits_actors_not_inert_scenery() {
    for inert in [0, 20000] {
        let mut w = scene(inert);
        member(&mut w, 0x62d, 8.5, 0);
        member(&mut w, 0x62e, 12.5, 1);
        let hub = member(&mut w, 0x6d2, 9.5, 0);
        let id = member(&mut w, 0x6d5, 10.5, 0);
        let expected = full_scan(&w, id, 1000);
        assert_eq!(
            expected,
            (
                Motion::StrictOrbit {
                    anchor: Anchor::CampEntity(hub),
                    radius: 1.
                },
                10.
            )
        );
        let own = w.entities.values().filter(|e| e.camp == Some(0)).count();
        assert_eq!(own, 3);
        assert_eq!(w.actor_ids.len(), own + 1);
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
            own * 2,
            "actual golem selectors must traverse only own live camp, not foreign actors or scenery"
        );
    }
}
