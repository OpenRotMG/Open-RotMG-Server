use super::*;
use crate::condition;
use std::path::Path;

fn arena() -> (World, i32) {
    let content = tests::world().content;
    let map = Map {
        width: 80,
        height: 80,
        cells: vec![
            crate::content::Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            6400
        ],
        spawns: vec![(16.5, 16.5)],
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.encounter = true;
    w.name = "Mad Lab".into();
    let p = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 16.5, y: 16.5 };
    (w, p)
}

#[test]
fn community_lab_registration_connects_an_ordinary_flying_brain_drop_to_a_complete_dungeon() {
    assert_eq!(crate::dungeon::portal_drop(0x657), Some((0x890, 0.17)));
    assert_eq!(crate::dungeon::portal_drop(0x658), None);
    assert!(crate::dungeon::DUNGEONS.contains(&(0x890, "mad-lab", "Mad Lab", 0x976)));
    let mut d =
        Directory::test_prepared(Path::new("data"), tests::world().content, 16, 12351).unwrap();
    let world = d.instance("mad-lab", None).unwrap();
    let w = &d.worlds[&world];
    let reached = crate::dungeon::tests::reachable(&w.map, &w.content);
    for kind in [0x976, 0x1e15, 0x1e16] {
        let locations: Vec<_> = w
            .map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(kind))
            .collect();
        assert!(!locations.is_empty(), "missing Lab actor {kind:#x}");
        assert!(
            locations.iter().all(|(i, _)| reached.contains(i)),
            "disconnected Lab actor {kind:#x}"
        );
    }
    assert!(
        w.entities
            .values()
            .filter(|e| w.content.objects[&e.kind].enemy)
            .count()
            > 12
    );
}

#[test]
fn community_lab_entry_species_normal_death_uses_world_rng_and_damage_credit_once() {
    let (mut w, player) = arena();
    w.name = "Realm".into();
    // Isolate the portal roll from unrelated loot RNG while retaining the shared damage/death path.
    w.encounter = false;
    let character = w.players[&player].character.id;
    let mut wins = 0;
    let mut misses = 0;
    for kind in [0x658, 0x657] {
        for seed in 0..32 {
            let position = Pos { x: 18.5, y: 16.5 };
            let enemy = w.spawn_entity(kind, position, vec![]).unwrap();
            w.rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mut expected = w.rng.clone();
            let rolled = kind == 0x657 && expected.gen_bool(0.17);
            let first = w
                .damage_enemy(player, enemy, 100, false, &[], Some(0))
                .unwrap();
            assert!(first > 0 && w.entities[&enemy].hp > 0);
            assert_eq!(w.entities[&enemy].damage[&character], first);
            assert!(w.portal_requests.is_empty());
            let remaining = w.entities[&enemy].hp;
            let raw = remaining + w.content.objects[&kind].defense;
            let kills = w.players[&player].character.kills;
            assert_eq!(
                w.damage_enemy(player, enemy, raw, false, &[], Some(1))
                    .unwrap(),
                remaining
            );
            assert!(!w.entities.contains_key(&enemy));
            assert_eq!(w.players[&player].character.kills, kills + 1);
            assert_eq!(
                w.portal_requests.len(),
                usize::from(rolled),
                "kind={kind:#x}, seed={seed}"
            );
            assert!(
                w.portal_requests
                    .iter()
                    .all(|(portal, point)| *portal == 0x890 && point.distance(position) < 0.01)
            );
            assert_eq!(
                rand::RngCore::next_u64(&mut w.rng.clone()),
                rand::RngCore::next_u64(&mut expected),
                "portal roll must use only the World RNG"
            );
            let next = rand::RngCore::next_u64(&mut w.rng.clone());
            assert_eq!(
                w.damage_enemy(player, enemy, raw, false, &[], Some(1))
                    .unwrap(),
                0
            );
            assert_eq!(w.players[&player].character.kills, kills + 1);
            assert_eq!(w.portal_requests.len(), usize::from(rolled));
            assert_eq!(rand::RngCore::next_u64(&mut w.rng.clone()), next);
            w.portal_requests.clear();
            if kind == 0x657 {
                if rolled {
                    wins += 1;
                } else {
                    misses += 1;
                }
            }
        }
    }
    assert!(wins > 0 && misses > 0);
}

#[test]
fn community_lab_green_status_is_persistent_and_blue_is_an_actor_not_ordinary_water() {
    let (mut w, p) = arena();
    let pos = w.players[&p].pos;
    let green = w.spawn_entity(0x1e15, pos, vec![]).unwrap();
    w.step();
    let expected = condition::bit(28) | condition::bit(15) | condition::bit(condition::STUNNED);
    assert_eq!(w.players[&p].conditions(w.now) & expected, expected);
    w.remove_entity(green);
    w.now += 100_000;
    w.step();
    assert_eq!(w.players[&p].conditions(w.now) & expected, expected);
    w.spawn_entity(0x1e16, pos, vec![]).unwrap();
    w.step();
    assert_eq!(w.players[&p].conditions(w.now) & expected, 0);
}

#[test]
fn community_lab_selected_encounter_has_real_registered_attacks() {
    let content = tests::world().content;
    for kind in [
        0x976, 0x978, 0x979, 0x980, 0x981, 0x982, 0x983, 0x993, 0x994, 0x995, 0x996, 0x12aa,
        0x1e23, 0x1e24, 0x1e25,
    ] {
        crate::encounter::validate(&content, kind).unwrap();
        crate::loot::validate(&content, kind).unwrap();
    }
}

#[test]
fn community_lab_pool_geometry_and_status_match_for_human_qa_and_autonomous_players() {
    for controller in [Controller::Human, Controller::Qa, Controller::Autonomous] {
        let (mut w, p) = arena();
        w.players.get_mut(&p).unwrap().controller = controller;
        w.spawn_entity(0x1e15, Pos { x: 16.5, y: 16.5 }, vec![])
            .unwrap();
        for (pos, affected) in [
            (Pos { x: 18., y: 16.5 }, false),
            (Pos { x: 17.99, y: 17.99 }, true),
        ] {
            w.players.get_mut(&p).unwrap().pos = pos;
            w.lab_step();
            assert_eq!(
                w.players[&p].conditions(w.now) & STUNNED != 0,
                affected,
                "{controller:?}"
            );
        }
        let position = w.players[&p].pos;
        assert!(
            w.shoot(p, 0, 0, 0xa97, position, 0.).is_err(),
            "pool Stunned must suppress actual attacks"
        );
    }
}

#[test]
fn community_lab_blue_cures_pool_sources_and_transformation_without_erasing_combat_timers() {
    let (mut w, p) = arena();
    let pos = w.players[&p].pos;
    let green = w.spawn_entity(0x1e15, pos, vec![]).unwrap();
    w.lab_step();
    w.remove_entity(green);
    w.apply_condition(p, condition::STUNNED, 3000).unwrap();
    w.apply_condition(p, 15, 5000).unwrap();
    w.apply_condition(p, 28, 30000).unwrap();
    let timers = w.players[&p].statuses.clone();
    let blue = w.spawn_entity(0x1e16, pos, vec![]).unwrap();
    w.lab_step();
    assert_eq!(w.players[&p].lab_conditions, 0);
    assert!(!w.players[&p].statuses.contains_key(&28));
    for effect in [condition::STUNNED, 15] {
        assert_eq!(w.players[&p].statuses[&effect], timers[&effect]);
    }
    w.remove_entity(blue);
    w.now = 5000;
    assert_eq!(w.players[&p].conditions(w.now), 0);
}

#[test]
fn community_lab_blue_wins_overlap_independently_of_actor_insertion_order() {
    for kinds in [[0x1e15, 0x1e16], [0x1e16, 0x1e15]] {
        let (mut w, p) = arena();
        let pos = w.players[&p].pos;
        for kind in kinds {
            w.spawn_entity(kind, pos, vec![]).unwrap();
        }
        w.lab_step();
        assert_eq!(w.players[&p].conditions(w.now), 0);
    }
}

#[test]
fn community_lab_ordinary_blue_water_does_not_cure_and_paused_actors_do_not_acquire_pool_status() {
    let (mut w, p) = arena();
    let pos = w.players[&p].pos;
    w.apply_condition(p, 28, 30000).unwrap();
    let width = w.map.width;
    Arc::make_mut(&mut w.map).cells[(pos.y as i32 * width + pos.x as i32) as usize].ground = 0x7a;
    w.lab_step();
    assert_ne!(w.players[&p].conditions(w.now) & condition::bit(28), 0);
    w.players.get_mut(&p).unwrap().paused = true;
    w.spawn_entity(0x1e15, pos, vec![]).unwrap();
    w.lab_step();
    assert_eq!(w.players[&p].lab_conditions, 0);
}

#[test]
fn community_lab_holy_cleansing_keeps_existing_effect_list_and_green_reapplies_on_next_tick() {
    let (mut w, p) = arena();
    let pos = w.players[&p].pos;
    let green = w.spawn_entity(0x1e15, pos, vec![]).unwrap();
    w.lab_step();
    w.finish_consumable(p, (p, 0), vec![crate::content::ConsumableEffect::Cleanse])
        .unwrap();
    assert_eq!(w.players[&p].conditions(w.now), SPEEDY);
    w.lab_step();
    assert_ne!(w.players[&p].conditions(w.now) & STUNNED, 0);
    w.remove_entity(green);
    w.finish_consumable(
        p,
        (p, 1),
        vec![crate::content::ConsumableEffect::Clear {
            effect: 15,
            check: None,
        }],
    )
    .unwrap();
    assert_eq!(w.players[&p].conditions(w.now) & SPEEDY, 0);
}

fn boss_arena() -> (World, i32, i32) {
    let (mut w, p) = arena();
    let boss = w
        .spawn_entity(0x976, Pos { x: 20.5, y: 16.5 }, vec![])
        .unwrap();
    (w, p, boss)
}

#[test]
fn community_lab_first_turret_subsequent_potion_and_rage_transitions_preserve_comparator_anomaly() {
    let (mut w, p, boss) = boss_arena();
    let target = w.players[&p].pos;
    w.lab_mechanics(boss, Some(target), 0).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(0));
    assert_eq!(w.lab_tosses.last().unwrap().kind, 0x978);
    w.now += 12000;
    w.lab_mechanics(boss, Some(target), 12000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(1));
    assert_eq!(w.lab_tosses.last().unwrap().kind, 0x12aa);
    w.now += 10000;
    w.lab_mechanics(boss, Some(target), 10000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(2));
    assert_ne!(
        w.entities[&boss].conditions(w.now) & condition::bit(condition::INVINCIBLE),
        0
    );
    w.now += 15000;
    w.lab_mechanics(boss, Some(target), 15000).unwrap();
    w.now += 12000;
    w.lab_mechanics(boss, Some(target), 12000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(4));
    assert_eq!(w.lab_tosses.last().unwrap().kind, 0x978);
    w.entities.get_mut(&boss).unwrap().hp = 17999;
    w.lab_mechanics(boss, Some(target), 0).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(12));
    w.now += 12000;
    w.lab_mechanics(boss, Some(target), 12000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(13));
    assert_eq!(w.lab_tosses.last().unwrap().kind, 0x12aa);
}

#[test]
fn community_lab_hide_controls_gas_safe_corner_cages_and_owned_cleanup() {
    let (mut w, p, boss) = boss_arena();
    let target = w.players[&p].pos;
    for (kind, x, y) in [
        (0x993, 12.5, 12.5),
        (0x994, 25.5, 12.5),
        (0x995, 12.5, 21.5),
        (0x996, 25.5, 21.5),
        (0x892, 26.5, 16.5),
        (0x1e22, 20.5, 15.5),
    ] {
        w.spawn_entity(kind, Pos { x, y }, vec![]).unwrap();
    }
    w.lab_mechanics(boss, Some(target), 0).unwrap();
    let gas: Vec<_> = w
        .actors()
        .filter(|e| (0x993..=0x996).contains(&e.kind))
        .map(|e| e.id)
        .collect();
    assert_eq!(
        gas.iter()
            .filter(|id| w.lab_shots(**id, target).is_empty())
            .count(),
        1
    );
    w.entities.get_mut(&boss).unwrap().stage = Some(1);
    w.now += 10000;
    w.lab_mechanics(boss, Some(target), 10000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(2));
    assert!(gas.iter().all(|id| w.lab_shots(*id, target).is_empty()));
    assert_eq!(
        w.actors()
            .filter(|e| e.parent == Some(boss) && matches!(e.kind, 0x1e23..=0x1e25))
            .count(),
        3
    );
    assert!(matches!(
        w.lab_movement(boss).0,
        crate::encounter::Motion::Protect { .. }
    ));
    w.defeat_entity(boss).unwrap();
    assert!(!w.actors().any(|e| e.parent == Some(boss)));
    assert!(gas.iter().all(|id| w.lab_shots(*id, target).is_empty()));
    assert_eq!(
        w.actors()
            .filter(|e| matches!(e.kind, 0x1e23..=0x1e25))
            .count(),
        3
    );
    let pending = w.lab_tosses.len();
    let entities = w.entities.len();
    w.now += 2000;
    w.lab_step();
    assert_eq!(w.entities.len(), entities + pending);
    assert!(w.lab_tosses.is_empty());
}

#[test]
fn community_lab_tosses_land_after_telegraph_and_potions_explode_once_with_selected_damage() {
    let (mut w, p, boss) = boss_arena();
    let target = w.players[&p].pos;
    w.lab_mechanics(boss, Some(target), 0).unwrap();
    w.now = 1499;
    w.lab_step();
    assert!(!w.actors().any(|e| e.kind == 0x978));
    w.now = 1500;
    w.lab_step();
    let potion = w.actors().find(|e| e.kind == 0x978).unwrap().id;
    w.lab_mechanics(potion, Some(target), 1999).unwrap();
    assert!(w.lab_shots(potion, target).is_empty());
    w.lab_mechanics(potion, Some(target), 2000).unwrap();
    assert_eq!(w.lab_shots(potion, target).len(), 6);
    assert_eq!(w.content.objects[&0x978].projectiles[&0].min, 200);
    w.now += 50;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&potion));
}

#[test]
fn community_lab_interruption_suspends_phase_clock_and_stun_suppresses_actual_emission() {
    let (mut w, p, boss) = boss_arena();
    w.npc_step();
    let stage = w.entities[&boss].stage;
    let elapsed = w.now - w.entities[&boss].activated.unwrap();
    w.players.get_mut(&p).unwrap().departing = true;
    for _ in 0..400 {
        w.now += 50;
        w.npc_step();
    }
    assert_eq!(w.entities[&boss].stage, stage);
    assert_eq!(w.now - w.entities[&boss].activated.unwrap(), elapsed);
    w.players.get_mut(&p).unwrap().departing = false;
    let bot = w.spawn_entity(0x979, w.players[&p].pos, vec![]).unwrap();
    w.apply_condition(bot, condition::STUNNED, 1000).unwrap();
    assert!(w.npc_step().iter().all(|(owner, _)| *owner != bot));
}

#[test]
fn community_lab_pool_state_cannot_survive_death_departure_or_character_reentry() {
    for departure in [false, true] {
        let (mut w, p) = arena();
        let pos = w.players[&p].pos;
        w.spawn_entity(0x1e15, pos, vec![]).unwrap();
        w.lab_step();
        let snapshot = w.players[&p].character.clone();
        if departure {
            w.players.get_mut(&p).unwrap().departing = true;
        } else {
            w.players.get_mut(&p).unwrap().death_pending = true;
        }
        w.lab_step();
        assert_eq!(w.players[&p].lab_conditions, 0);
        let mut other = tests::arena(0);
        let replacement = other.enter(snapshot, None).unwrap();
        assert_eq!(other.players[&replacement].conditions(other.now), 0);
    }
}

#[test]
fn community_lab_cyborg_blinks_without_shooting_then_explodes_and_experiment_uses_selected_zero() {
    let (mut w, p) = arena();
    let target = w.players[&p].pos;
    let cyborg = w.spawn_entity(0x980, target, vec![]).unwrap();
    w.entities.get_mut(&cyborg).unwrap().hp = 399;
    w.lab_mechanics(cyborg, Some(target), 0).unwrap();
    assert_eq!(w.entities[&cyborg].stage, Some(3));
    assert!(w.lab_shots(cyborg, target).is_empty());
    w.now += 2000;
    w.lab_mechanics(cyborg, Some(target), 2000).unwrap();
    let shots = w.lab_shots(cyborg, target);
    assert_eq!(shots.len(), 8);
    assert!(shots.iter().all(|(index, _)| *index == 2));
    let experiment = w.spawn_entity(0x1e23, target, vec![]).unwrap();
    assert_eq!(w.lab_shots(experiment, target), vec![(0, 0.)]);
}

#[test]
fn community_lab_clear_private_rewards_and_exit_are_once_only_and_commit_gated() {
    let (mut w, p, boss) = boss_arena();
    let mut visitor = tests::ch(&w);
    visitor.id = 2;
    visitor.account = 2;
    let visitor = w.enter(visitor, None).unwrap();
    w.entities
        .get_mut(&boss)
        .unwrap()
        .damage
        .insert(w.players[&p].character.id, 1);
    let dead = w.remove_entity(boss);
    w.drop_loot(&dead).unwrap();
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    let wisdom: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.items.contains(&0xa35))
        .collect();
    assert_eq!(wisdom.len(), 1);
    assert!(wisdom[0].accessible(1));
    assert!(!wisdom[0].accessible(2));
    let plans = w.pending_progress();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].0, p);
    assert_eq!(plans[0].1.mad_labs, 1);
    assert_eq!(w.players[&p].character.mad_labs, 0);
    w.finish_progress(p, plans[0].1.clone()).unwrap();
    assert_eq!(w.players[&p].character.mad_labs, 1);
    assert!(w.entities[&exit].published);
    assert_eq!(w.players[&visitor].character.mad_labs, 0);
    let bags = w.entities.len();
    assert!(w.drop_loot(&dead).is_err());
    assert_eq!(w.entities.len(), bags);
    assert!(w.pending_progress().is_empty());
}

#[test]
fn community_lab_dead_departing_and_zero_damage_players_cannot_receive_rewards_or_credit() {
    for (dead, departing, damage) in [(true, false, 1), (false, true, 1), (false, false, 0)] {
        let (mut w, p, boss) = boss_arena();
        w.entities.get_mut(&boss).unwrap().damage.insert(1, damage);
        w.players.get_mut(&p).unwrap().death_pending = dead;
        w.players.get_mut(&p).unwrap().departing = departing;
        let defeated = w.remove_entity(boss);
        w.drop_loot(&defeated).unwrap();
        assert!(!w.entities.values().any(|e| !e.drop_slots.is_empty()));
        assert!(w.pending_progress().is_empty());
        assert!(!w.entities[&w.clear_exit.unwrap()].published);
    }
}

#[test]
fn community_lab_cyborg_and_experiment_prediction_uses_recent_ordinary_native_movement() {
    for kind in [0x980, 0x981, 0x1e23, 0x1e24] {
        let (mut w, p) = arena();
        w.players.get_mut(&p).unwrap().controller = Controller::Human;
        let original = w.players[&p].pos;
        w.native_move_to(p, original, 1000).unwrap();
        w.native_move_to(
            p,
            Pos {
                x: original.x,
                y: original.y + 0.4,
            },
            1100,
        )
        .unwrap();
        let target = w.players[&p].pos;
        let enemy = w
            .spawn_entity(
                kind,
                Pos {
                    x: original.x + 5.,
                    y: original.y,
                },
                vec![],
            )
            .unwrap();
        let heading = (target.y - original.y).atan2(target.x - original.x - 5.);
        let shots = w.legacy_enemy_shots(enemy, target, 0).unwrap();
        assert!((shots[0].1 - heading).abs() > 0.2);
        w.now = 201;
        let stale = w.legacy_enemy_shots(enemy, target, 0).unwrap();
        assert!((stale[0].1 - heading).abs() < 0.0001);
    }
}

#[test]
fn community_lab_global_rage_is_strict_irreversible_and_does_not_reset_timers_or_erase_summons() {
    for stage in [0, 1, 2] {
        let (mut w, p, boss) = boss_arena();
        let target = w.players[&p].pos;
        w.entities.get_mut(&boss).unwrap().stage = Some(stage);
        for hp in [18001, 18000] {
            w.entities.get_mut(&boss).unwrap().hp = hp;
            w.lab_mechanics(boss, Some(target), 0).unwrap();
            assert_eq!(w.entities[&boss].stage, Some(stage));
        }
        let child = w.spawn_entity(0x979, target, vec![]).unwrap();
        w.entities.get_mut(&child).unwrap().parent = Some(boss);
        w.entities.get_mut(&boss).unwrap().hp = 17999;
        w.lab_mechanics(boss, Some(target), 0).unwrap();
        assert_eq!(w.entities[&boss].stage, Some(12));
        assert_eq!(
            w.entities[&boss].conditions(w.now) & condition::bit(condition::INVINCIBLE),
            0
        );
        let activated = w.entities[&boss].activated;
        w.now += 50;
        w.entities.get_mut(&boss).unwrap().hp = 18001;
        w.lab_mechanics(boss, Some(target), 50).unwrap();
        assert_eq!(w.entities[&boss].stage, Some(12));
        assert_eq!(w.entities[&boss].activated, activated);
        assert!(w.entities.contains_key(&child));
    }
}

#[test]
fn community_lab_old_character_json_defaults_only_the_new_counter() {
    let (w, p) = arena();
    let mut value = serde_json::to_value(&w.players[&p].character).unwrap();
    value.as_object_mut().unwrap().remove("mad_labs");
    let restored: Character = serde_json::from_value(value).unwrap();
    assert_eq!(restored.mad_labs, 0);
}

#[tokio::test]
async fn community_lab_real_store_retains_clear_pickup_and_permanent_death_without_reward_replay() {
    use crate::store::{InventoryWrite, Store};
    let (mut w, old) = arena();
    w.leave(old).unwrap();
    let path =
        std::env::temp_dir().join(format!("community-lab-{:016x}.db", rand::random::<u64>()));
    let store = Store::open(&path, &w.content).unwrap();
    let account = store.list("community-lab-owner".into()).await.unwrap();
    let ch = store
        .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let p = w.enter(ch, None).unwrap();
    let pos = w.players[&p].pos;
    let boss = w.spawn_entity(0x976, pos, vec![]).unwrap();
    w.entities
        .get_mut(&boss)
        .unwrap()
        .damage
        .insert(character, 1);
    w.defeat_entity(boss).unwrap();
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    let (owner, progress) = w.pending_progress().pop().unwrap();
    store.progress(progress.clone()).await.unwrap();
    w.finish_progress(owner, progress).unwrap();
    assert!(w.entities[&exit].published);
    assert_eq!(store.load(account.id, character).await.unwrap().mad_labs, 1);
    let bag = w
        .entities
        .values()
        .find(|e| e.items.contains(&0xa35))
        .unwrap();
    let (bag, slot) = (
        bag.id,
        bag.items.iter().position(|item| *item == 0xa35).unwrap(),
    );
    w.players.get_mut(&p).unwrap().known.insert(bag);
    let pickup = w
        .swap(
            p,
            Slot {
                object: bag,
                slot,
                kind: 0xa35,
            },
            Slot {
                object: p,
                slot: 5,
                kind: -1,
            },
        )
        .unwrap();
    assert!(
        !store
            .load(account.id, character)
            .await
            .unwrap()
            .items
            .contains(&0xa35)
    );
    let revision = store
        .exchange(
            vec![InventoryWrite {
                id: pickup.character,
                account: pickup.account,
                revision: pickup.revision,
                items: pickup.items.clone(),
            }],
            pickup.vaults.clone(),
        )
        .await
        .unwrap()[0];
    w.finish_inventory(pickup, Ok(revision)).unwrap();
    assert_eq!(
        store.load(account.id, character).await.unwrap().items[5],
        0xa35
    );
    let gas = w.content.objects[&0x993].projectiles[&0].clone();
    let bullet = Bullet {
        owner: 9999,
        source_kind: 0x993,
        id: 0,
        friendly: false,
        start: pos,
        pos,
        angle: 0.,
        born: w.now,
        damage: gas.min,
        spec: gas,
        hit: BTreeSet::new(),
    };
    w.damage_player(p, &bullet, false).unwrap();
    assert!(w.players[&p].death_pending);
    let (id, dead, killer) = w.pending_deaths().pop().unwrap();
    store.death(dead.clone(), killer.clone()).await.unwrap();
    assert!(store.death(dead, killer).await.is_err());
    w.finish_death(id).unwrap();
    assert!(store.load(account.id, character).await.is_err());
    drop(store);
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let (alive, items, snapshot): (bool, String, String) = db
        .query_row(
            "SELECT alive,items,stats FROM characters WHERE id=?1",
            [character],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(!alive);
    assert_eq!(
        serde_json::from_str::<Vec<i32>>(&items).unwrap(),
        vec![-1; 12]
    );
    assert_eq!(
        serde_json::from_str::<Character>(&snapshot)
            .unwrap()
            .mad_labs,
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
}

fn generated_lab() -> (World, i32, i32) {
    let content = tests::world().content;
    let map = crate::dungeon::lab::generate(&content, 7).unwrap();
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.name = "Mad Lab".into();
    w.encounter = true;
    let p = w.enter(tests::ch(&w), None).unwrap();
    let boss = w.actors().find(|e| e.kind == 0x976).unwrap().id;
    w.players.get_mut(&p).unwrap().pos = Pos { x: 56.5, y: 18.5 };
    (w, p, boss)
}

fn cage_children(w: &World, boss: i32) -> BTreeSet<i32> {
    w.actors()
        .filter(|e| e.parent == Some(boss) && matches!(e.kind, 0x1e23..=0x1e25))
        .map(|e| e.id)
        .collect()
}

fn next_hide(w: &mut World, p: i32, boss: i32) {
    let target = w.players[&p].pos;
    if w.entities[&boss].stage.is_none() {
        w.lab_mechanics(boss, Some(target), 0).unwrap();
    }
    for duration in [12000, 10000] {
        w.now += duration;
        w.lab_mechanics(boss, Some(target), duration).unwrap();
    }
}

#[test]
fn community_lab_four_cages_keep_live_survivors_bounded_across_many_real_phase_cycles() {
    let (mut w, p, boss) = generated_lab();
    for _ in 0..32 {
        next_hide(&mut w, p, boss);
        let children = cage_children(&w, boss);
        assert_eq!(children.len(), 12);
        let identities: BTreeSet<_> = children
            .iter()
            .map(|id| {
                let e = &w.entities[id];
                (e.kind, e.home.x as i32, e.home.y as i32)
            })
            .collect();
        assert_eq!(identities.len(), 12);
        w.now += 15000;
        w.lab_mechanics(boss, Some(w.players[&p].pos), 15000)
            .unwrap();
    }
}

#[test]
fn community_lab_real_child_death_refills_only_that_cage_kind_on_next_hide_entry() {
    let (mut w, p, boss) = generated_lab();
    next_hide(&mut w, p, boss);
    let original = cage_children(&w, boss);
    let child = *original.first().unwrap();
    let kind = w.entities[&child].kind;
    let home = w.entities[&child].home;
    for _ in 0..40 {
        w.damage_enemy(p, child, 100, false, &[], Some(0)).unwrap();
    }
    assert!(!w.entities.contains_key(&child));
    assert_eq!(w.players[&p].character.kills, 1);
    assert!(w.players[&p].experience_queued > 0);
    w.now += 1000;
    w.lab_mechanics(boss, Some(w.players[&p].pos), 1000)
        .unwrap();
    assert_eq!(cage_children(&w, boss).len(), 11);
    w.now += 15000;
    w.lab_mechanics(boss, Some(w.players[&p].pos), 15000)
        .unwrap();
    assert_eq!(cage_children(&w, boss).len(), 11);
    next_hide(&mut w, p, boss);
    let after = cage_children(&w, boss);
    assert_eq!(after.len(), 12);
    assert!(
        original
            .iter()
            .filter(|id| **id != child)
            .all(|id| after.contains(id))
    );
    let added: Vec<_> = after.difference(&original).collect();
    assert_eq!(added.len(), 1);
    let replacement = &w.entities[added[0]];
    assert_eq!(replacement.kind, kind);
    assert_eq!(replacement.home.distance(home), 0.);
}

#[test]
fn community_lab_shared_and_owned_summon_limits_fail_explicitly_before_admission() {
    for (limit, hide) in [(256, true), (128, false)] {
        let (mut w, p, boss) = generated_lab();
        let target = w.players[&p].pos;
        w.lab_mechanics(boss, Some(target), 0).unwrap();
        while w
            .actors()
            .filter(|e| e.parent.is_some() || e.quest.is_some())
            .count()
            < limit
        {
            let child = w
                .spawn_entity(0x979, Pos { x: 12.5, y: 66.5 }, vec![])
                .unwrap();
            w.entities.get_mut(&child).unwrap().parent = Some(boss);
        }
        let before = w.entities.len();
        let result = if hide {
            w.entities.get_mut(&boss).unwrap().stage = Some(1);
            w.now += 10000;
            w.lab_mechanics(boss, Some(target), 10000)
        } else {
            w.players.get_mut(&p).unwrap().pos = Pos { x: 56.5, y: 16.5 };
            w.entities.get_mut(&boss).unwrap().next_aux = w.now;
            w.lab_mechanics(boss, Some(w.players[&p].pos), 0)
        };
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains(if hide {
                "shared summon budget"
            } else {
                "owned summon budget"
            }),
            "{error}"
        );
        assert_eq!(w.entities.len(), before);
        assert!(w.lab_tosses.is_empty());
        assert!(cage_children(&w, boss).is_empty());
    }
}

#[test]
fn community_lab_dead_boss_preserves_survivors_and_each_admitted_ownerless_toss_lands_once() {
    let (mut w, p, boss) = generated_lab();
    next_hide(&mut w, p, boss);
    let survivors = cage_children(&w, boss);
    assert_eq!(survivors.len(), 12);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 56.5, y: 16.5 };
    w.now += 15000;
    w.lab_mechanics(boss, Some(w.players[&p].pos), 15000)
        .unwrap();
    assert_eq!(w.lab_tosses.len(), 1);
    for _ in 0..100 {
        w.damage_enemy(p, boss, 1000, false, &[], Some(1)).unwrap();
    }
    assert!(!w.entities.contains_key(&boss));
    assert!(w.cleared);
    assert!(survivors.iter().all(|id| w.entities.contains_key(id)));
    assert!(!w.actors().any(|e| e.parent == Some(boss)));
    let before = w.entities.len();
    w.now += 1500;
    w.lab_step();
    assert_eq!(w.entities.len(), before + 1);
    assert!(w.lab_tosses.is_empty());
    let potion = w.actors().find(|e| e.kind == 0x978).unwrap();
    assert_eq!(potion.parent, None);
    let count = w.entities.len();
    for _ in 0..32 {
        w.now += 1000;
        w.lab_step();
    }
    assert_eq!(w.entities.len(), count);
    assert!(
        w.actors()
            .filter(|e| w.content.objects[&e.kind].enemy)
            .count()
            < 256
    );
    let progress = w.pending_progress();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].1.mad_labs, 1);
    assert!(w.pending_progress().is_empty());
}
