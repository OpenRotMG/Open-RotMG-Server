use super::*;
use rand::RngCore;

thread_local! {
    pub(super) static COUNTS: std::cell::Cell<(usize, usize, usize, usize)> = const { std::cell::Cell::new((0, 0, 0, 0)) };
}

fn scan(w: &World, audience: &[Pos]) -> Vec<i32> {
    w.actors()
        .filter(|e| {
            if e.population_slot.is_some()
                && e.activated.is_none()
                && e.stage.is_none()
                && e.expires.is_none()
                && e.next_shot <= w.now
                && e.next_aux == 0
                && e.reaction_until == 0
                && e.blocked_since.is_none()
                && e.charge.is_none()
                && e.script.is_none()
                && audience.iter().all(|p| e.pos.distance(*p) >= 20.)
            {
                return false;
            }
            let object = &w.content.objects[&e.kind];
            object.class != "Wall"
                && !matches!(e.kind, 0xd5f | 0xd60)
                && (object.enemy || matches!(e.kind, 0x173 | 0xd7d))
        })
        .map(|e| e.id)
        .collect()
}
fn check(w: &mut World, audience: &[Pos]) -> Vec<i32> {
    let expected = scan(w, audience);
    let rng = w.rng.clone().next_u64();
    let now = w.now;
    let actual = w.npc_candidates(audience);
    assert_eq!(actual, expected);
    assert!(w.npc_global_ids.is_subset(&w.actor_ids));
    assert_eq!(w.rng.clone().next_u64(), rng);
    assert_eq!(w.now, now);
    actual
}
fn ordinary(w: &mut World, pos: Pos) -> i32 {
    let id = w.spawn_entity(0x600, pos, vec![]).unwrap();
    let slot = w.population.len();
    w.population.push(Spawn {
        kind: 0x600,
        pos,
        entity: Some(id),
        camp: None,
    });
    w.mark_population(id, slot).unwrap();
    assert!(w.population_count(11) <= w.population_regions[11].limit);
    id
}

#[test]
fn npc_spatial_queries_match_scans_for_clustered_spread_and_edge_audiences() {
    let mut w = population_density_tests::field(160, 11);
    for y in (4..156).step_by(16) {
        for x in (4..156).step_by(16) {
            ordinary(
                &mut w,
                Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                },
            );
        }
    }
    let special = w
        .spawn_entity(0x600, Pos { x: 150.5, y: 150.5 }, vec![])
        .unwrap();
    w.spawn_entity(0x712, Pos { x: 120.5, y: 120.5 }, vec![])
        .unwrap();
    assert_eq!(check(&mut w, &[]), [special]);
    let near = Pos { x: 64.5, y: 64.5 };
    check(&mut w, &[near]);
    let single = COUNTS.get();
    check(&mut w, &[near; 9]);
    let cluster = COUNTS.get();
    assert_eq!((cluster.0, cluster.1), (single.0, single.1));
    assert_eq!(cluster.2, single.2 * 9);
    for audience in [
        vec![Pos { x: 0.5, y: 0.5 }, Pos { x: 159.5, y: 159.5 }],
        vec![Pos { x: 0.5, y: 150.5 }, near, Pos { x: 150.5, y: 0.5 }],
        vec![Pos {
            x: 10000.,
            y: -10000.,
        }],
        vec![],
    ] {
        check(&mut w, &audience);
    }
    let p = Pos { x: 1.5, y: 1.5 };
    let boundary = ordinary(&mut w, Pos { x: 21.5, y: 1.5 });
    let inside = ordinary(&mut w, Pos { x: 21.499, y: 1.5 });
    let ids = check(&mut w, &[p]);
    assert!(!ids.contains(&boundary));
    assert!(ids.contains(&inside));
}

#[test]
fn unrelated_dormant_population_does_not_expand_steady_query_work() {
    let mut w = population_density_tests::field(320, 11);
    let near = ordinary(&mut w, Pos { x: 65.5, y: 64.5 });
    for i in 0..1000 {
        ordinary(
            &mut w,
            Pos {
                x: 200.5 + (i % 50) as f32,
                y: 200.5 + (i / 50) as f32,
            },
        );
    }
    check(&mut w, &[]);
    assert_eq!(COUNTS.get().0, 1001);
    assert_eq!(check(&mut w, &[Pos { x: 64.5, y: 64.5 }]), [near]);
    assert_eq!(COUNTS.get(), (1, 43 * 43, 43 * 43, 1));
}

#[test]
fn complete_global_coverage_avoids_redundant_cell_queries() {
    let mut w = population_density_tests::field(64, 11);
    for i in 0..32 {
        ordinary(
            &mut w,
            Pos {
                x: 24.5 + (i % 8) as f32,
                y: 24.5 + (i / 8) as f32,
            },
        );
    }
    for _ in 0..2 {
        assert_eq!(check(&mut w, &[Pos { x: 28.5, y: 28.5 }]).len(), 32);
        assert_eq!(COUNTS.get(), (32, 0, 0, 32));
    }
}

#[test]
fn every_existing_clock_or_script_keeps_distant_population_eligible() {
    let mutations: [fn(&mut Entity); 8] = [
        |e| e.activated = Some(0),
        |e| e.stage = Some(0),
        // Eligibility reads the field directly; the expiry index is irrelevant here.
        |e| e.expires.set(Some(200), e.id, &mut BTreeSet::new()),
        |e| e.next_shot = 101,
        |e| e.next_aux = 1,
        |e| e.reaction_until = 1,
        |e| e.blocked_since = Some(0),
        |e| e.script = Some(landmark_ai::Script::default()),
    ];
    for mutate in mutations {
        let mut w = population_density_tests::field(64, 11);
        w.now = 100;
        let id = ordinary(&mut w, Pos { x: 50.5, y: 50.5 });
        mutate(w.entities.get_mut(&id).unwrap());
        assert_eq!(check(&mut w, &[]), [id]);
    }
    let mut w = population_density_tests::field(64, 11);
    let id = ordinary(&mut w, Pos { x: 50.5, y: 50.5 });
    w.charge_goal(
        id,
        Some(Pos { x: 51.5, y: 50.5 }),
        crate::encounter::Motion::Charge {
            sight: 20.,
            cooldown: 1000,
            delay: 0,
            stop: 0.,
            idle: 0.,
        },
        2.,
    );
    assert!(w.entities[&id].charge.is_some());
    assert_eq!(check(&mut w, &[]), [id]);
    w.entities.get_mut(&id).unwrap().charge = None;
    w.entities.get_mut(&id).unwrap().next_shot = 100;
    assert_eq!(check(&mut w, &[]), [id]);
    w.now = 100;
    assert!(check(&mut w, &[]).is_empty());
    assert!(!w.npc_global_ids.contains(&id));
}

#[test]
fn movement_transform_and_removal_preserve_spatial_membership() {
    let mut w = population_density_tests::field(80, 11);
    let id = ordinary(&mut w, Pos { x: 60.5, y: 60.5 });
    let audience = [Pos { x: 10.5, y: 10.5 }];
    assert!(check(&mut w, &audience).is_empty());
    w.move_entity(id, Pos { x: 12.5, y: 10.5 }, 10000.).unwrap();
    assert_eq!(check(&mut w, &audience), [id]);
    w.move_entity(id, Pos { x: 60.5, y: 60.5 }, 10000.).unwrap();
    assert!(check(&mut w, &audience).is_empty());
    let next = w.transform_fauna(id, 0x600).unwrap();
    assert!(!w.npc_global_ids.contains(&id));
    assert!(w.npc_global_ids.contains(&next));
    check(&mut w, &audience);
    w.remove_entity(next);
    assert!(check(&mut w, &audience).is_empty());
    assert!(w.npc_global_ids.is_empty());
}

#[test]
fn pruned_golem_satellites_regain_offscreen_deadline_work() {
    let family = population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x62d)
        .unwrap();
    let mut w = population_density_tests::field(80, family.terrain);
    w.place_camp(family, vec![Pos { x: 50.5, y: 50.5 }; family.size()])
        .unwrap();
    assert!(check(&mut w, &[]).is_empty());
    assert!(w.npc_global_ids.is_empty());
    let root = w.population[w.camps[0].start].entity.unwrap();
    w.now = 12345;
    w.remove_entity(root);
    let ids = check(&mut w, &[]);
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|id| w.entities[id].expires == Some(42345)));
    w.now = 42345;
    w.mechanic_expiry();
    assert!(check(&mut w, &[]).is_empty());
    w.population_regions[family.terrain as usize].target = 0;
    w.population_next = 0;
    w.repopulate().unwrap();
    assert!(w.camps.is_empty());
    assert!(check(&mut w, &[]).is_empty());
}

#[test]
fn pruned_shield_orcs_regain_offscreen_command_work() {
    let family = population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x900)
        .unwrap();
    let mut w = population_density_tests::field(80, family.terrain);
    w.place_camp(family, vec![Pos { x: 50.5, y: 50.5 }; family.size()])
        .unwrap();
    assert!(check(&mut w, &[]).is_empty());
    assert!(w.npc_global_ids.is_empty());
    let key = w.population[w.camps[0].start].entity.unwrap();
    w.entities.get_mut(&key).unwrap().hp = 499;
    w.now = 100;
    w.midland_mechanics(key).unwrap();
    assert_eq!(check(&mut w, &[]).len(), 4);
    assert!(w.camp_members(0).all(|e| e.stage == Some(2)));
    assert_eq!(w.entities[&key].expires, Some(1100));
}

#[test]
fn spatially_selected_stasis_clocks_remain_global_after_audience_leaves() {
    let mut w = population_density_tests::field(80, 11);
    w.encounter = true;
    let id = ordinary(&mut w, Pos { x: 12.5, y: 10.5 });
    assert!(check(&mut w, &[]).is_empty());
    let player = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&player).unwrap().pos = Pos { x: 10.5, y: 10.5 };
    w.apply_condition(id, crate::condition::STASIS, 1000)
        .unwrap();
    w.now = 50;
    w.npc_step();
    assert_eq!(w.entities[&id].next_aux, 50);
    assert!(w.entities[&id].activated.is_none());
    assert!(w.npc_global_ids.contains(&id));
    w.players.get_mut(&player).unwrap().paused = true;
    w.now = 100;
    w.npc_step();
    assert_eq!(w.entities[&id].next_aux, 100);
    assert_eq!(check(&mut w, &[]), [id]);
}
