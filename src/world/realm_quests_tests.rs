use super::*;
use crate::{condition, quest};
#[test]
fn all_damage_evaluations_spawn_bounded_owned_waves_without_closure_credit() {
    let mut w = realm();
    for kind in [0x91f, 0x91b, 0x928] {
        let pos = w.entities[&root(&w, kind)].pos;
        for (tier, loss) in [(0, 10), (1, 4501), (2, 14401), (3, 37501)] {
            let tag = w.spawn_entity(kind, pos, vec![]).unwrap();
            let e = w.entities.get_mut(&tag).unwrap();
            e.quest = Some(tag);
            e.stage = Some(0);
            e.hp -= loss - 10;
            activate(&mut w, tag);
            assert_eq!(w.entities[&tag].stage, Some(3 + tier));
            for t in (50..=62000).step_by(50) {
                if !w.entities.contains_key(&tag) {
                    break;
                }
                clock(&mut w, tag, t);
                assert!(w.entities.values().filter(|e| e.quest == Some(tag)).count() <= 32);
            }
            assert!(w.critical_deaths.is_empty());
            let ids: Vec<_> = w
                .entities
                .values()
                .filter(|e| e.quest == Some(tag))
                .map(|e| e.id)
                .collect();
            for id in ids {
                w.remove_entity(id);
            }
        }
    }
}
#[test]
fn every_named_phase_uses_selected_projectiles_and_fresh_players_or_live_decoys() {
    let mut w = realm();
    let pos = w.entities[&root(&w, 0x675)].pos;
    let p = player(&mut w, pos);
    for kind in quest::ROSTER {
        let id = w.spawn_entity(*kind, pos, vec![]).unwrap();
        w.entities.get_mut(&id).unwrap().quest = Some(id);
        for stage in [0, 1, 2, 3, 7, 9] {
            w.entities.get_mut(&id).unwrap().stage = Some(stage);
            for t in (0..30000).step_by(50) {
                for (index, angle) in w.quest_shots(id, w.players[&p].pos, t).unwrap() {
                    assert!(angle.is_finite());
                    assert!(w.content.objects[kind].projectiles.contains_key(&index));
                }
            }
        }
        w.remove_entity(id);
    }
}
#[test]
fn ghost_introduction_is_once_not_an_extra_second_of_shielding_each_cycle() {
    let mut w = realm();
    let tag = root(&w, 0x928);
    let pos = w.entities[&tag].pos;
    for (kind, times) in [
        (0x92b, [0, 1000, 2000, 8000, 9000, 15000, 16000]),
        (0x92c, [0, 1000, 2000, 5000, 6000, 9000, 10000]),
    ] {
        let id = w.spawn_entity(kind, pos, vec![]).unwrap();
        let e = w.entities.get_mut(&id).unwrap();
        e.quest = Some(tag);
        e.stage = Some(0);
        e.activated = Some(w.now);
        for (elapsed, shield) in times
            .into_iter()
            .zip([true, true, false, true, false, true, false])
        {
            clock(&mut w, id, elapsed);
            assert_eq!(
                w.entities[&id].condition & condition::bit(condition::INVULNERABLE) != 0,
                shield
            );
        }
    }
    let id = w.spawn_entity(0x929, pos, vec![]).unwrap();
    let e = w.entities.get_mut(&id).unwrap();
    e.quest = Some(tag);
    e.stage = Some(0);
    e.activated = Some(w.now);
    e.hp = 98500;
    w.quest_mechanics(id).unwrap();
    assert_eq!(w.entities[&id].stage, Some(1));
    for (t, shield) in [(1000, true), (2000, false), (8000, true), (9000, false)] {
        clock(&mut w, id, t);
        assert_eq!(
            w.entities[&id].condition & condition::bit(condition::INVULNERABLE) != 0,
            shield
        );
    }
}
fn realm() -> World {
    Directory::prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        12351,
    )
    .unwrap()
    .worlds
    .remove(&-3)
    .unwrap()
}
fn root(w: &World, k: u16) -> i32 {
    w.entities.values().find(|e| e.kind == k).unwrap().id
}
fn clock(w: &mut World, id: i32, elapsed: u64) {
    w.now = w.entities[&id].activated.unwrap() + elapsed;
    w.quest_mechanics(id).unwrap();
}
fn activate(w: &mut World, id: i32) {
    w.entities.get_mut(&id).unwrap().activated = Some(w.now);
    w.entities.get_mut(&id).unwrap().hp -= 10;
    w.quest_mechanics(id).unwrap();
    clock(w, id, 2500);
    clock(w, id, 6000);
}
fn player(w: &mut World, pos: Pos) -> i32 {
    let p = w.enter(tests::ch(w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = pos;
    p
}
fn transformed(w: &World, tag: i32, kind: u16) -> i32 {
    w.entities
        .values()
        .find(|e| e.quest == Some(tag) && e.kind == kind)
        .unwrap()
        .id
}
#[test]
fn named_quests_have_selected_profiles_and_finite_separate_habitat_budgets() {
    for seed in [1, 2, 42, 12351] {
        let d = Directory::prepared(
            std::path::Path::new("data"),
            tests::world().content,
            16,
            seed,
        )
        .unwrap();
        let w = &d.worlds[&-3];
        assert_eq!(w.hero_identities().len(), 13);
        for kind in quest::ROOTS {
            let id = root(w, kind);
            assert_eq!(w.entities[&id].quest, Some(id));
            assert!(w.population.iter().all(|s| s.entity != Some(id)));
        }
        for kind in quest::ROSTER {
            crate::encounter::validate(&w.content, *kind).unwrap();
            crate::loot::validate(&w.content, *kind).unwrap();
        }
        for terrain in 1..=12 {
            assert!(w.population_count(terrain) <= if terrain >= 11 { 24 } else { 32 });
        }
    }
}
#[test]
fn ent_evaluates_damage_then_grows_and_exposes_real_vulnerability_windows() {
    let mut w = realm();
    let tag = root(&w, 0x91f);
    activate(&mut w, tag);
    assert!(w.critical_deaths.is_empty());
    clock(&mut w, tag, 3000);
    let actual = transformed(&w, tag, 0x920);
    assert_eq!(w.entities[&actual].hp, 1200);
    w.quest_mechanics(actual).unwrap();
    for phase in 0..10 {
        clock(&mut w, actual, phase * 1600);
        assert_eq!(w.entities[&actual].size, 160 + phase as i32 * 20);
        assert_ne!(
            w.entities[&actual].condition & condition::bit(condition::INVULNERABLE),
            0
        );
    }
    clock(&mut w, actual, 20400);
    assert_eq!(
        w.entities[&actual].condition & condition::bit(condition::INVULNERABLE),
        0
    );
    let pos = w.entities[&actual].pos;
    let p = player(&mut w, pos);
    w.damage_enemy(p, actual, 10000, true, &[], None).unwrap();
    assert_eq!(w.critical_deaths, vec![tag]);
    assert_eq!(w.players[&p].character.quests, 1);
}
#[test]
fn phoenix_all_forms_carry_damage_and_only_final_defeat_rewards_original_identity() {
    let mut w = realm();
    let original = root(&w, 0x675);
    let pos = w.entities[&original].pos;
    let p = player(&mut w, pos);
    w.damage_enemy(p, original, 9999, true, &[], None).unwrap();
    let egg = transformed(&w, original, 0x6c2);
    assert_eq!(w.entities[&egg].hp, 5000);
    assert!(w.critical_deaths.is_empty());
    assert_eq!(w.players[&p].character.kills, 0);
    w.entities.get_mut(&egg).unwrap().activated = Some(w.now);
    w.quest_mechanics(egg).unwrap();
    assert_eq!(w.damage_enemy(p, egg, 9999, true, &[], None).unwrap(), 0);
    clock(&mut w, egg, 2000);
    w.damage_enemy(p, egg, 9999, true, &[], None).unwrap();
    let reborn = transformed(&w, original, 0x6c1);
    assert_eq!(w.entities[&reborn].hp, 4200);
    assert_eq!(
        w.entities[&reborn].damage[&w.players[&p].character.id],
        8000
    );
    w.quest_mechanics(reborn).unwrap();
    w.damage_enemy(p, reborn, 9999, true, &[], None).unwrap();
    assert_eq!(w.critical_deaths, vec![original]);
    assert_eq!(w.players[&p].character.quests, 1);
    assert_eq!(w.players[&p].character.kills, 1);
    assert_eq!(w.damage_enemy(p, reborn, 1, true, &[], None).unwrap(), 0);
}
#[test]
fn ghost_master_release_cannot_adopt_another_kings_encounter() {
    let mut w = realm();
    let original = root(&w, 0x928);
    activate(&mut w, original);
    let master = transformed(&w, original, 0x929);
    let other = w
        .spawn_entity(0x928, w.entities[&original].pos, vec![])
        .unwrap();
    w.entities.get_mut(&other).unwrap().quest = Some(other);
    w.entities.get_mut(&master).unwrap().hp = 93999;
    w.quest_mechanics(master).unwrap();
    assert!(!w.entities.contains_key(&original));
    assert!(w.entities.contains_key(&other));
    assert_eq!(w.entities[&transformed(&w, original, 0x92d)].hp, 5000);
    assert!(w.critical_deaths.is_empty());
    w.quest_mechanics(master).unwrap();
    clock(&mut w, master, 3000);
    assert!(!w.entities.contains_key(&master));
}
#[test]
fn lich_full_healing_is_own_encounter_only_and_obeys_sick_and_stun() {
    let mut w = realm();
    let original = root(&w, 0x91b);
    let pos = w.entities[&original].pos;
    let leader = w.quest_transform(original, 0x91c).unwrap();
    w.quest_mechanics(leader).unwrap();
    let other = w.spawn_entity(0x91c, pos, vec![]).unwrap();
    let e = w.entities.get_mut(&other).unwrap();
    e.quest = Some(other);
    e.hp = 200;
    w.quest_spawn_survivors(&w.entities[&leader].clone(), 0x91d, 1)
        .unwrap();
    let bearer = transformed(&w, original, 0x91d);
    w.entities.get_mut(&leader).unwrap().hp = 200;
    w.entities.get_mut(&bearer).unwrap().activated = Some(w.now);
    w.quest_mechanics(bearer).unwrap();
    assert_eq!(w.entities[&leader].hp, 1100);
    assert_eq!(w.entities[&other].hp, 200);
    w.entities.get_mut(&leader).unwrap().hp = 200;
    w.apply_condition(leader, condition::SICK, 5000).unwrap();
    clock(&mut w, bearer, 200);
    assert_eq!(w.entities[&leader].hp, 200);
    w.entities.get_mut(&leader).unwrap().statuses.clear();
    w.apply_condition(bearer, condition::STUNNED, 5000).unwrap();
    clock(&mut w, bearer, 400);
    assert_eq!(w.entities[&leader].hp, 200);
    assert_eq!(w.entities[&bearer].home.x, pos.x);
}
#[test]
fn named_transformations_preserve_status_boundaries_and_never_close_on_despawn() {
    let mut w = realm();
    let original = root(&w, 0x675);
    w.entities.get_mut(&original).unwrap().activated = Some(w.now);
    w.quest_mechanics(original).unwrap();
    let followers: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.quest == Some(original) && e.id != original)
        .map(|e| e.id)
        .collect();
    assert_eq!(followers.len(), 8);
    w.quest_transform(original, 0x6c2).unwrap();
    w.mechanic_expiry();
    assert!(followers.iter().all(|id| w.entities.contains_key(id)));
    assert!(w.critical_deaths.is_empty());
    let egg = transformed(&w, original, 0x6c2);
    w.remove_entity(egg);
    w.repopulate().unwrap();
    assert!(
        w.entities
            .values()
            .all(|e| e.quest != Some(original) || !quest::ROOTS.contains(&e.kind))
    );
    assert!(w.critical_deaths.is_empty());
}
