use super::*;

pub(super) fn field(size: i32, terrain: u8) -> World {
    let content = tests::world().content;
    let cell = crate::content::Cell {
        ground: 0x61,
        object: None,
        config: String::new(),
        terrain,
        region: 0,
    };
    let map = Arc::new(Map {
        width: size,
        height: size,
        cells: vec![cell; (size * size) as usize],
        spawns: vec![(1.5, 1.5)],
    });
    let mut w = World::new(content, map).unwrap();
    w.rng = rand::rngs::StdRng::seed_from_u64(163);
    w.configure_population().unwrap();
    w.population_next = u64::MAX;
    w
}
fn check_counts(w: &World) {
    let mut counts = [0; 13];
    for (slot, member) in w.population.iter().enumerate() {
        if let Some(id) = member.entity {
            let e = &w.entities[&id];
            assert_eq!(e.population_slot, Some(slot));
            assert_eq!(e.camp, member.camp);
            counts[w.population_terrain(member) as usize] += 1;
        }
    }
    assert_eq!(counts, w.population_counts);
    let mut quests: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for e in w.actors() {
        let o = &w.content.objects[&e.kind];
        if let Some(level) = o.quest_level.filter(|_| o.quest) {
            quests.entry(level).or_default().insert(e.id);
        }
    }
    assert_eq!(quests, w.quests_by_level);
    let scripted: BTreeSet<_> = w
        .actors()
        .filter(|e| e.population_slot.is_none())
        .map(|e| e.id)
        .collect();
    assert_eq!(scripted, w.non_population_ids);
    for (terrain, count) in counts.iter().enumerate().skip(1) {
        assert!(
            count + w.pending_fauna_count(terrain as u8) <= w.population_regions[terrain].limit
        );
    }
}

#[test]
fn selected_xml_groups_are_bounded_and_species_without_groups_are_single_roots() {
    let mut w = field(60, 1);
    for (kind, min, max) in [
        (0x651, 2, 4),
        (0x654, 2, 5),
        (0x656, 2, 7),
        (0x602, 2, 7),
        (0x204, 2, 20),
    ] {
        let group = w.content.objects[&kind].spawn_group.unwrap();
        assert_eq!((group.min, group.max), (min, max));
        let values: BTreeSet<_> = (0..1000).map(|_| w.sample_population_group(kind)).collect();
        assert!(values.len() > 1 && values.iter().all(|n| (min..=max).contains(n)));
    }
    assert_eq!(w.sample_population_group(0x652), 1);
}

#[test]
fn ordinary_body_targets_scale_with_area_and_do_not_count_scripted_entities() {
    let mut small = field(60, 1);
    let mut large = field(120, 1);
    assert_eq!(small.population_regions[1].target, 36);
    assert_eq!(large.population_regions[1].target, 144);
    let scripted = large
        .spawn_entity(0x652, Pos { x: 10.5, y: 10.5 }, vec![])
        .unwrap();
    large.entities.get_mut(&scripted).unwrap().quest = Some(scripted);
    assert_eq!(large.population_count(1), 0);
    small.populate_realm_population().unwrap();
    large.populate_realm_population().unwrap();
    assert!(small.population_count(1) >= 36);
    assert!(large.population_count(1) >= 144);
    assert!(large.populate_realm_population().is_err());
    assert!(large.entities[&scripted].population_slot.is_none());
    assert_eq!(large.quest_members(scripted, &[0x652]), [scripted]);
    assert!(large.critical_deaths.is_empty());
    assert_eq!(large.population_regions[2].target, 0);
    assert_eq!(large.population_count(2), 0);
    check_counts(&small);
    check_counts(&large);
}

#[test]
fn finite_xml_encounters_are_not_selected_for_ordinary_replenishment() {
    let mut w = field(80, 3);
    let crystal = w
        .content
        .objects
        .values()
        .find(|o| o.terrain > 0 && o.per_realm_max.is_some())
        .unwrap()
        .kind;
    let terrain = w.content.objects[&crystal].terrain;
    w = field(80, terrain);
    w.populate_realm_population().unwrap();
    assert!(w.population.iter().all(|s| s.kind != crystal));
    let mut low = field(80, 9);
    low.populate_realm_population().unwrap();
    assert!(low.population.iter().all(|s| s.kind != 0x7f00));
    check_counts(&w);
}

#[test]
fn transforms_and_orphaned_followers_keep_their_population_accounting() {
    let mut w = field(80, 1);
    w.populate_families(1, &[Pos { x: 30.5, y: 30.5 }], &[0x652])
        .unwrap();
    assert_eq!(w.population_count(1), 4);
    let original = w.population[0].entity.unwrap();
    let replacement = w.transform_fauna(original, 0x652).unwrap();
    assert_ne!(original, replacement);
    assert_eq!(w.population_count(1), 4);
    w.remove_entity(replacement);
    w.repopulate().unwrap();
    assert_eq!(w.population_count(1), 3);
    let followers: Vec<_> = w.population.iter().filter_map(|s| s.entity).collect();
    w.population_next = 0;
    w.repopulate().unwrap();
    assert!(followers.iter().all(|id| w.entities.contains_key(id)));
    assert!(w.population_count(1) >= w.population_regions[1].target);
    check_counts(&w);
}

#[test]
fn compaction_preserves_admitted_births_after_owner_death() {
    let mut w = field(80, 6);
    w.populate_families(
        6,
        &[Pos { x: 20.5, y: 20.5 }, Pos { x: 50.5, y: 50.5 }],
        &[0x62c, 0x950],
    )
    .unwrap();
    let owner = w.population[w.camps[1].start].entity.unwrap();
    let positions = [Pos { x: 55.5, y: 50.5 }, Pos { x: 50.5, y: 55.5 }];
    assert_eq!(
        w.fauna_toss(owner, 0x951, &positions, false, None).unwrap(),
        2
    );
    let ids: Vec<_> = w.population.iter().filter_map(|s| s.entity).collect();
    for id in ids {
        w.remove_entity(id);
    }
    w.population_regions[6].target = 0;
    w.population_next = 0;
    w.now = 1500;
    w.repopulate().unwrap();
    assert_eq!(w.camps.len(), 1);
    assert!(w.fauna_births.iter().all(|b| b.camp == 0));
    w.fauna_birth_step();
    assert!(w.fauna_births.is_empty());
    assert_eq!(w.population_count(6), 2);
    assert!(!w.entities.contains_key(&owner));
    check_counts(&w);
}

#[test]
fn repeated_region_clears_refill_at_fresh_sites_without_growing_the_slot_store() {
    let mut w = field(80, 1);
    w.populate_realm_population().unwrap();
    let mut dead = BTreeSet::new();
    let mut old_sites = BTreeSet::new();
    for _ in 0..10 {
        let ids: Vec<_> = w.population.iter().filter_map(|s| s.entity).collect();
        let sites: BTreeSet<_> = ids
            .iter()
            .map(|id| {
                let p = w.entities[id].pos;
                (p.x.to_bits(), p.y.to_bits())
            })
            .collect();
        if !old_sites.is_empty() {
            assert_ne!(sites, old_sites);
        }
        old_sites = sites;
        for id in ids {
            assert!(dead.insert(id));
            w.remove_entity(id);
        }
        w.population_next = w.now;
        w.repopulate().unwrap();
        assert!(w.population_count(1) >= w.population_regions[1].target);
        assert!(
            w.population
                .iter()
                .filter_map(|s| s.entity)
                .all(|id| !dead.contains(&id))
        );
        assert!(w.population.len() <= w.population_regions[1].limit * 32);
        assert!(w.critical_deaths.is_empty());
        check_counts(&w);
        w.now += 60000;
    }
}

#[test]
fn impossible_safe_placement_is_explicitly_refused_without_partial_groups() {
    let mut w = field(10, 1);
    let error = w.populate_realm_population().unwrap_err();
    assert!(error.to_string().contains("cannot fill population"));
    assert_eq!(w.population_count(1), 0);
    assert!(w.population.is_empty());
}

#[test]
fn regional_refill_waits_for_the_low_water_mark_and_keeps_survivors() {
    let mut w = field(80, 1);
    w.populate_realm_population().unwrap();
    let threshold = w.population_regions[1].target * 3 / 4;
    let ids: Vec<_> = w.population.iter().filter_map(|s| s.entity).collect();
    for id in ids.into_iter().take(w.population_count(1) - threshold) {
        w.remove_entity(id);
    }
    w.population_next = 0;
    w.repopulate().unwrap();
    assert_eq!(w.population_count(1), threshold);
    let id = w.population.iter().find_map(|s| s.entity).unwrap();
    w.remove_entity(id);
    let survivors: Vec<_> = w.population.iter().filter_map(|s| s.entity).collect();
    w.now += 60000;
    w.repopulate().unwrap();
    assert!(w.population_count(1) >= w.population_regions[1].target);
    assert!(survivors.iter().all(|id| w.entities.contains_key(id)));
    check_counts(&w);
}

#[test]
fn dormant_population_first_wake_matches_untracked_reference_ai() {
    let home = Pos { x: 30.5, y: 30.5 };
    let mut cold = field(60, 1);
    let mut reference = field(60, 1);
    cold.populate_solos(1, &[home], &[0x651]).unwrap();
    reference.spawn_entity(0x651, home, vec![]).unwrap();
    for w in [&mut cold, &mut reference] {
        w.encounter = true;
        for time in (0..5000).step_by(50) {
            w.now = time;
            assert!(w.npc_step().is_empty());
        }
        let player = w.enter(tests::ch(w), None).unwrap();
        w.players.get_mut(&player).unwrap().pos = Pos {
            x: home.x + 6.,
            y: home.y,
        };
    }
    let mut shots = 0;
    for time in (5000..6200).step_by(50) {
        cold.now = time;
        reference.now = time;
        let actual = cold.npc_step();
        shots += actual.len();
        assert_eq!(actual, reference.npc_step());
    }
    assert!(shots > 0);
}
