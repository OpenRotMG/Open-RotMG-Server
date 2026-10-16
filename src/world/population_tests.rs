use super::*;
use crate::encounter::{Anchor, Motion};
use std::path::Path;
fn directory(seed: u64) -> Directory {
    Directory::prepared(Path::new("data"), tests::world().content, 16, seed)
        .unwrap_or_else(|error| panic!("seed {seed}: {error}"))
}
fn camp(w: &World, leader: u16) -> usize {
    w.camps
        .iter()
        .position(|c| c.family.leader == leader)
        .unwrap()
}
fn size(w: &World, index: usize) -> usize {
    1 + w.camps[index]
        .family
        .roles
        .iter()
        .map(|r| r.maximum)
        .sum::<usize>()
}
fn members(w: &World, index: usize) -> Vec<i32> {
    let s = w.camps[index].start;
    w.population[s..s + size(w, index)]
        .iter()
        .filter_map(|s| s.entity.filter(|id| w.entities.contains_key(id)))
        .collect()
}
fn player(w: &mut World, pos: Pos) -> i32 {
    let ch = tests::ch(w);
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = pos;
    p
}
fn assert_budgets(w: &World) {
    for terrain in 1..=12 {
        assert!(w.population_count(terrain) <= if terrain >= 11 { 24 } else { 32 });
    }
    assert_eq!(w.hero_identities().len(), 10);
}

#[test]
fn complete_pack_is_seeded_sourced_distributed_and_regionally_bounded() {
    for seed in [0, 1, 42, 12351, u64::MAX] {
        let a = directory(seed);
        let b = directory(seed);
        let w = &a.worlds[&-3];
        let sites = |w: &World| {
            w.population
                .iter()
                .map(|s| (s.kind, s.pos.x.to_bits(), s.pos.y.to_bits()))
                .collect::<Vec<_>>()
        };
        assert_eq!(sites(w), sites(&b.worlds[&-3]));
        assert_budgets(w);
        assert_eq!(w.population_count(11), 24);
        for family in population::FAMILIES {
            if seed == 12351 || !(2..=7).contains(&family.terrain) {
                assert!(w.camps.iter().any(|c| c.family.leader == family.leader));
            }
        }
        for (index, c) in w.camps.iter().enumerate() {
            assert_eq!(
                members(w, index).len(),
                1 + c.family.roles.iter().map(|r| r.initial).sum::<usize>()
            );
            for id in members(w, index) {
                let e = &w.entities[&id];
                assert_eq!(e.camp, Some(index));
                assert!(e.parent.is_none() && e.expires.is_none());
                assert_eq!(e.hp, w.content.objects[&e.kind].hp);
                assert_eq!(e.size, w.content.objects[&e.kind].size);
            }
        }
        for s in &w.population {
            assert_eq!(
                w.map.cell(s.pos.x, s.pos.y).unwrap().terrain,
                w.content.objects[&s.kind].terrain
            );
            assert!(w.walkable(s.pos));
            if let Some(id) = s.entity
                && w.content.objects[&s.kind].terrain >= 8
            {
                assert_eq!(w.entities[&id].size, w.content.objects[&s.kind].size);
            }
        }
        let arrival = Pos {
            x: w.map.spawns[0].0,
            y: w.map.spawns[0].1,
        };
        assert!(
            w.population
                .iter()
                .any(|s| s.kind == 0x600 && s.pos.distance(arrival) < 60.)
        );
        assert!(
            w.population
                .iter()
                .any(|s| matches!(s.kind, 0x953 | 0x604) && s.pos.distance(arrival) > 64.)
        );
    }
    let ordinary =
        Directory::with_seed(Path::new("data"), tests::world().content, 16, 12351).unwrap();
    assert_budgets(&ordinary.worlds[&-3]);
}

#[test]
fn every_family_reinforces_on_its_role_clock_without_duplicate_ids() {
    for family in population::FAMILIES
        .iter()
        .filter(|family| family.roles[0].interval.is_some())
    {
        let mut d = directory(12351);
        let w = d.worlds.get_mut(&-3).unwrap();
        let c = camp(w, family.leader);
        let start = w.camps[c].start;
        let home = w.population[start].pos;
        player(w, home);
        let other: Vec<_> = w
            .population
            .iter()
            .filter(|s| s.camp != Some(c))
            .filter_map(|s| s.entity)
            .collect();
        for id in other {
            w.remove_entity(id);
        }
        w.repopulate().unwrap();
        let slot = start + 1;
        let old = w.population[slot].entity.unwrap();
        w.remove_entity(old);
        w.repopulate().unwrap();
        let deadline = family.roles[0].interval.unwrap();
        w.now = deadline - 1;
        w.repopulate().unwrap();
        assert!(w.population[slot].entity.is_none());
        w.now = deadline;
        w.repopulate().unwrap();
        let new = w.population[slot].entity.unwrap();
        assert_ne!(old, new);
        for _ in 0..50 {
            w.repopulate().unwrap();
        }
        assert_eq!(w.population[slot].entity, Some(new));
        assert_budgets(w);
    }
}

#[test]
fn orphans_survive_and_cleared_groups_wait_for_player_clearance() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let c = camp(w, 0x609);
    let s = w.camps[c].start;
    let home = w.population[s].pos;
    let p = player(w, home);
    let old = members(w, c);
    w.remove_entity(old[0]);
    w.repopulate().unwrap();
    w.mechanic_expiry();
    assert_eq!(members(w, c).len(), 7);
    assert!(matches!(
        w.family_movement(old[1], home, 0).unwrap().0,
        Motion::Wander
    ));
    w.now = 60000;
    w.repopulate().unwrap();
    assert!(w.population[s].entity.is_none());
    for id in &old[1..] {
        w.remove_entity(*id);
    }
    w.repopulate().unwrap();
    w.now = 120000;
    w.repopulate().unwrap();
    assert!(members(w, c).is_empty());
    w.players.get_mut(&p).unwrap().pos = Pos { x: 0.5, y: 0.5 };
    w.now = 121000;
    w.repopulate().unwrap();
    assert_eq!(members(w, c).len(), 8);
    assert!(old.iter().all(|id| !w.entities.contains_key(id)));
    assert!(w.critical_deaths.is_empty());
    assert_budgets(w);
}

#[test]
fn distinct_leader_phases_and_independent_projectile_clocks_are_real() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let c = camp(w, 0x614);
    let s = w.camps[c].start;
    let mage = w.population[s].entity.unwrap();
    let rogue = w.population[s + 1].entity.unwrap();
    let pos = w.entities[&mage].pos;
    let target = Pos {
        x: pos.x + 4.,
        y: pos.y,
    };
    w.entities.get_mut(&mage).unwrap().hp = 182;
    assert!(matches!(
        w.family_movement(mage, target, 0).unwrap().0,
        Motion::Retreat { .. }
    ));
    assert!(matches!(
        w.family_movement(rogue, target, 2700).unwrap().0,
        Motion::Chase { stop: 1.5, .. }
    ));
    let b = camp(w, 0x953);
    let id = w.population[w.camps[b].start].entity.unwrap();
    w.entities.get_mut(&id).unwrap().hp = 80;
    assert!(matches!(
        w.family_movement(id, target, 0).unwrap().0,
        Motion::Retreat { .. }
    ));
    assert!(crate::encounter::phased_attacks(0x604, 100, 0., 0).is_empty());
    for (t, index) in [(0, 0), (400, 1), (800, 2)] {
        let shots = crate::encounter::phased_attacks(0x617, 200, 0., t);
        assert_eq!(shots.len(), 15);
        assert!(shots.iter().all(|s| s.0 == index));
    }
    assert_eq!(
        crate::encounter::phased_attacks(0x61a, 270, 0., 1000),
        vec![(3, 0.)]
    );
    assert!(
        crate::encounter::phased_attacks(0x60c, 88, 0., 5000)
            .iter()
            .any(|s| s.0 == 0)
    );
    assert_eq!(
        crate::encounter::phased_attacks(0x60c, 88, 0., 400),
        vec![(1, 0.)]
    );
    let elf = camp(w, 0x609);
    let s = w.camps[elf].start;
    let archer = w.population[s + 1].entity.unwrap();
    let leader = w.population[s].entity.unwrap();
    let home = w.entities[&leader].pos;
    assert_eq!(
        w.orbit_anchor(archer, Anchor::CampLeader, target)
            .unwrap()
            .distance(home),
        0.
    );
    assert!(matches!(
        w.family_movement(leader, home, 0).unwrap().0,
        Motion::Wiggle
    ));
    assert!(crate::encounter::phased_attacks(0x609, 200, 0., 4000).is_empty());
}

#[test]
fn family_conditions_share_the_existing_movement_and_shot_pipeline() {
    let mut distances = vec![];
    for effect in [
        None,
        Some(crate::condition::SLOWED),
        Some(crate::condition::PARALYZED),
        Some(crate::condition::STUNNED),
    ] {
        let mut d = directory(12351);
        let w = d.worlds.get_mut(&-3).unwrap();
        let c = camp(w, 0x609);
        let s = w.camps[c].start;
        let id = w.population[s + 1].entity.unwrap();
        let pos = w.entities[&id].pos;
        player(w, w.population[s].pos);
        if let Some(effect) = effect {
            w.apply_condition(id, effect, 1000).unwrap();
        }
        w.step();
        distances.push(pos.distance(w.entities[&id].pos));
        if effect == Some(crate::condition::STUNNED) {
            assert!(w.bullets.iter().all(|b| b.owner != id));
        }
    }
    assert!(distances[0] > 0.);
    assert!((distances[1] * 2. - distances[0]).abs() < 0.0001);
    assert_eq!(distances[2], 0.);
}

#[test]
fn repeated_full_region_clears_preserve_caps_and_never_reuse_dead_ids() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let mut dead = BTreeSet::new();
    for _ in 0..20 {
        let ids: Vec<_> = w
            .population
            .iter()
            .filter_map(|s| s.entity.filter(|id| w.entities.contains_key(id)))
            .collect();
        for id in ids {
            assert!(dead.insert(id));
            w.remove_entity(id);
        }
        w.repopulate().unwrap();
        w.now += 60000;
        w.repopulate().unwrap();
        assert_budgets(w);
        assert_eq!(w.population_count(11), 24);
        assert!(w.critical_deaths.is_empty());
    }
}
