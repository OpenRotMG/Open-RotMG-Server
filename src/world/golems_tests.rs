use super::*;
use crate::encounter::{Anchor, Motion};

fn scan_motion(w: &World, id: i32, elapsed: u64) -> (Motion, f32) {
    let e = &w.entities[&id];
    let camp = e.camp.unwrap();
    if golems::ROOTS.contains(&e.kind) {
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
        .filter(|x| x.camp == Some(camp) && golems::ROOTS.contains(&x.kind))
        .min_by(|a, b| e.pos.distance(a.pos).total_cmp(&e.pos.distance(b.pos)));
    if matches!(e.kind, 0x6d5..=0x6d7)
        && let Some(hub) = w
            .entities
            .values()
            .filter(|x| x.camp == Some(camp) && matches!(x.kind, 0x6d2..=0x6d4))
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

fn check(w: &World) {
    for camp in 0..w.camps.len() {
        let expected: BTreeSet<_> = w
            .entities
            .values()
            .filter(|e| e.camp == Some(camp))
            .map(|e| e.id)
            .collect();
        assert_eq!(
            w.camp_members(camp).map(|e| e.id).collect::<BTreeSet<_>>(),
            expected
        );
        for id in expected {
            if golems::ROOTS.contains(&w.entities[&id].kind)
                || matches!(w.entities[&id].kind, 0x6d2..=0x6d7)
            {
                for elapsed in [0, 2500, 30000] {
                    assert_eq!(
                        w.golem_movement(id, elapsed).unwrap(),
                        scan_motion(w, id, elapsed)
                    );
                }
            }
        }
    }
}

#[test]
fn camp_queries_match_scans_through_transform_removal_and_compaction() {
    for kind in golems::ROOTS {
        let terrain = tests::world().content.objects[kind].terrain;
        let mut w = population_density_tests::field(80, terrain);
        let family = population::FAMILIES
            .iter()
            .find(|f| f.leader == *kind)
            .unwrap();
        for x in [20.5, 50.5] {
            let positions = (0..family.size())
                .map(|i| Pos {
                    x: x + i as f32,
                    y: 20.5,
                })
                .collect();
            w.place_camp(family, positions).unwrap();
        }
        check(&w);
        let near = w.camp_members(1).find(|e| e.kind == 0x6d5).unwrap().pos;
        let foreign: Vec<_> = w
            .camp_members(0)
            .filter(|e| golems::ROOTS.contains(&e.kind) || matches!(e.kind, 0x6d2..=0x6d4))
            .map(|e| e.id)
            .collect();
        for id in foreign {
            w.move_entity(id, near, 10000.).unwrap();
        }
        check(&w);
        let root = w.population[w.camps[1].start].entity.unwrap();
        let root = w.transform_fauna(root, *kind).unwrap();
        check(&w);
        let ids: Vec<_> = w.camp_members(0).map(|e| e.id).collect();
        for id in ids {
            w.remove_entity(id);
            check(&w);
        }
        w.population_regions[terrain as usize].target = 0;
        w.population_next = 0;
        w.repopulate().unwrap();
        assert_eq!(w.entities[&root].camp, Some(0));
        check(&w);
        let satellites: Vec<_> = w
            .camp_members(0)
            .filter(|e| e.id != root)
            .map(|e| e.id)
            .collect();
        w.now = 12345;
        w.remove_entity(root);
        check(&w);
        for id in satellites {
            assert_eq!(w.entities[&id].expires, Some(42345));
        }
    }
}

#[test]
fn respawned_lower_slot_does_not_override_the_lower_entity_id_on_distance_ties() {
    let mut w = population_density_tests::field(80, 7);
    let family = population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x62d)
        .unwrap();
    let pos = Pos { x: 30.5, y: 30.5 };
    w.place_camp(family, vec![pos; family.size()]).unwrap();
    let old_root = w.population[0].entity.unwrap();
    w.place_member(0, 1, pos).unwrap();
    let older = w.population[1].entity.unwrap();
    w.remove_entity(old_root);
    w.place_member(0, 0, pos).unwrap();
    let newer = w.population[0].entity.unwrap();
    assert!(older < newer);
    let hub = w
        .camp_members(0)
        .find(|e| matches!(e.kind, 0x6d2..=0x6d4))
        .unwrap()
        .id;
    assert_eq!(
        w.golem_movement(hub, 0).unwrap().0,
        Motion::StrictOrbit {
            anchor: Anchor::CampEntity(older),
            radius: 3.
        }
    );
    assert!(w.camp_members(0).all(|e| e.expires.is_none()));
    check(&w);
    // Entity kind, not the original slot kind, determines whether it is a root.
    let replacement = w.transform_fauna(older, 0x6d2).unwrap();
    assert_eq!(w.population[1].entity, Some(replacement));
    let gray = w.camp_members(0).find(|e| e.kind == 0x6d5).unwrap().id;
    assert_eq!(
        w.golem_movement(gray, 0).unwrap().0,
        Motion::StrictOrbit {
            anchor: Anchor::CampEntity(hub),
            radius: 1.
        }
    );
    check(&w);
}

#[test]
fn unrelated_entities_do_not_expand_the_camp_query_candidate_bound() {
    let mut w = population_density_tests::field(80, 7);
    let family = population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x62d)
        .unwrap();
    let pos = Pos { x: 30.5, y: 30.5 };
    w.place_camp(family, vec![pos; family.size()]).unwrap();
    let expected: Vec<_> = w.camp_members(0).map(|e| e.id).collect();
    let mut spawned = 0;
    for unrelated in [0, 10000, 100000] {
        while spawned < unrelated {
            let pos = Pos {
                x: (spawned % 80) as f32 + 0.5,
                y: (spawned / 80 % 80) as f32 + 0.5,
            };
            w.spawn_entity(0x602, pos, vec![]).unwrap();
            spawned += 1;
        }
        assert_eq!(w.camp_members(0).size_hint().1, Some(family.size()));
        assert_eq!(
            w.camp_members(0).map(|e| e.id).collect::<Vec<_>>(),
            expected
        );
        check(&w);
    }
}

#[test]
#[should_panic(expected = "live camp member missing")]
fn missing_camp_entity_is_an_invariant_error() {
    let mut w = population_density_tests::field(80, 7);
    let family = population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x62d)
        .unwrap();
    w.place_camp(family, vec![Pos { x: 30.5, y: 30.5 }; family.size()])
        .unwrap();
    w.entities.remove(&w.population[0].entity.unwrap());
    w.camp_members(0).count();
}
