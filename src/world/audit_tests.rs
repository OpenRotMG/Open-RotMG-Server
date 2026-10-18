use super::*;
use crate::{condition, encounter, endgame};

use super::tests::arena;
fn center(w: &World) -> Pos {
    Pos {
        x: w.map.width as f32 / 2. + 0.5,
        y: w.map.height as f32 / 2. + 0.5,
    }
}
fn player(w: &mut World, pos: Pos) -> i32 {
    let id = w.enter(tests::ch(w), None).unwrap();
    w.players.get_mut(&id).unwrap().pos = pos;
    id
}
fn shots(w: &mut World, id: i32) -> Vec<u8> {
    w.npc_step()
        .into_iter()
        .filter(|(owner, _)| *owner == id)
        .flat_map(|(_, data)| {
            let mut r = wire::Reader::new(&data);
            r.take(5).unwrap();
            let index = r.u8().unwrap();
            r.take(14).unwrap();
            let count = if data.len() == 20 { 1 } else { r.u8().unwrap() };
            vec![index; count as usize]
        })
        .collect()
}
fn members(w: &World, camp: usize) -> Vec<i32> {
    w.entities
        .values()
        .filter(|e| e.camp == Some(camp))
        .map(|e| e.id)
        .collect()
}

#[test]
fn target_relative_rings_and_leviathan_fans_rotate_with_the_target() {
    for (kind, hp, ms) in [
        (0x610, 70, 0),
        (0x657, 1000, 0),
        (0x917, 2500, 0),
        (0x6d8, 2000, 0),
        (0x6d8, 2000, 1500),
    ] {
        let a = encounter::phased_attacks(kind, hp, 0., ms);
        let b = encounter::phased_attacks(kind, hp, 0.7, ms);
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(&b) {
            assert_eq!(a.0, b.0);
            assert!((b.1 - a.1 - 0.7).abs() < 0.00001, "{kind:#x}");
        }
    }
}
#[test]
fn dispatch_keeps_sorcerer_weapon_ranges_and_clocks_independent() {
    let mut w = arena(8);
    let pos = center(&w);
    let p = player(&mut w, pos);
    let id = w
        .spawn_entity(
            0x60c,
            Pos {
                x: pos.x + 6.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.apply_condition(id, condition::PARALYZED, 10000).unwrap();
    let mut all = vec![];
    for ms in (0..=5000).step_by(50) {
        w.now = ms;
        all.extend(shots(&mut w, id));
    }
    assert_eq!(all, vec![0, 0]);
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: pos.x + 2.,
        ..pos
    };
    w.now = 5200;
    assert_eq!(shots(&mut w, id), vec![1]);
}
#[test]
fn bandit_choices_are_random_instead_of_a_forced_alternating_cycle() {
    let mut w = arena(11);
    let pos = center(&w);
    player(&mut w, pos);
    let id = w
        .spawn_entity(
            0x954,
            Pos {
                x: pos.x + 3.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.apply_condition(id, condition::PARALYZED, 60000).unwrap();
    let mut sequence = vec![];
    for ms in (0..=40000).step_by(50) {
        w.now = ms;
        sequence.extend(shots(&mut w, id));
    }
    assert_eq!(sequence.len(), 21);
    assert!(sequence.contains(&0) && sequence.contains(&1));
    assert!(sequence.windows(2).any(|p| p[0] == p[1]));
}
#[test]
fn elf_wizard_prediction_is_chosen_per_volley_not_scaled_by_its_probability() {
    let mut w = arena(10);
    let pos = center(&w);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    let id = w.spawn_entity(0x609, pos, vec![]).unwrap();
    w.apply_condition(id, condition::PARALYZED, 60000).unwrap();
    let spec = &w.content.objects[&0x609].projectiles[&0];
    let horizon = (6. / (spec.speed / 10.)).min(spec.lifetime / 1000.);
    let lead = (2. * horizon).atan2(6.);
    let mut predicted = 0;
    let mut direct = 0;
    for ms in (0..60000).step_by(50) {
        w.now = ms;
        let actor = w.players.get_mut(&p).unwrap();
        actor.motion.at = ms;
        actor.motion.velocity = Pos { x: 0., y: 2. };
        w.npc_step();
        let volley: Vec<_> = w
            .bullets
            .iter()
            .filter(|b| b.owner == id && b.born == ms)
            .collect();
        if volley.is_empty() {
            continue;
        }
        assert_eq!(volley.len(), 3);
        let angle = volley.iter().map(|b| b.angle).sum::<f32>() / 3.;
        if angle.abs() < 0.00001 {
            direct += 1;
        } else {
            assert!((angle - lead).abs() < 0.00001);
            predicted += 1;
        }
    }
    assert!(predicted > 0 && direct > 0);
}
#[test]
fn cloaking_does_not_stop_owned_construct_healing_and_dormancy_pauses_its_clock() {
    let mut w = arena(1);
    let pos = center(&w);
    w.populate_families(1, &[pos], &[0x905]).unwrap();
    let ids = members(&w, 0);
    assert_eq!(ids.len(), 3);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    w.apply_condition(p, condition::INVISIBLE, 20000).unwrap();
    for id in &ids {
        let e = w.entities.get_mut(id).unwrap();
        e.hp = w.content.objects[&e.kind].hp - 150;
        if e.kind != 0x905 {
            w.apply_condition(*id, condition::PARALYZED, 20000).unwrap();
        }
    }
    let rock = *ids.iter().find(|id| w.entities[id].kind == 0x905).unwrap();
    let paper = *ids.iter().find(|id| w.entities[id].kind == 0x906).unwrap();
    let before = w.entities[&rock].pos;
    w.npc_step();
    assert!(
        ids.iter()
            .all(|id| w.entities[id].hp == w.content.objects[&w.entities[id].kind].hp)
    );
    assert!(w.entities[&rock].pos.distance(before) > 0.);
    assert!(w.bullets.is_empty());
    w.entities.get_mut(&paper).unwrap().hp -= 80;
    w.players.get_mut(&p).unwrap().pos = Pos { x: 1.5, y: 1.5 };
    for ms in (50..=5000).step_by(50) {
        w.now = ms;
        w.npc_step();
    }
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: pos.x + 6.,
        ..pos
    };
    w.now = 5050;
    w.npc_step();
    let maximum = w.content.objects[&0x906].hp;
    assert_eq!(w.entities[&paper].hp, maximum - 80);
    for ms in (5100..=6000).step_by(50) {
        w.now = ms;
        w.npc_step();
    }
    assert_eq!(w.entities[&paper].hp, maximum);
}
#[test]
fn golem_satellites_use_real_budgets_owned_orbits_and_thirty_second_decay() {
    for kind in [0x62d, 0x62e, 0x62f, 0x631, 0x632, 0x633] {
        let mut w = arena(5);
        let terrain = w.content.objects[&kind].terrain;
        for c in &mut Arc::make_mut(&mut w.map).cells {
            c.terrain = terrain;
        }
        w.configure_population().unwrap();
        let pos = center(&w);
        w.populate_families(terrain, &[pos], &[kind]).unwrap();
        let ids = members(&w, 0);
        assert_eq!(ids.len(), 5);
        assert_eq!(w.population_count(terrain), 5);
        let hub = *ids
            .iter()
            .find(|id| matches!(w.entities[id].kind, 0x6d2..=0x6d4))
            .unwrap();
        let root = *ids.iter().find(|id| w.entities[id].kind == kind).unwrap();
        player(
            &mut w,
            Pos {
                x: pos.x + 6.,
                ..pos
            },
        );
        w.npc_step();
        assert!((w.entities[&hub].pos.distance(w.entities[&root].pos) - 3.).abs() < 0.00001);
        for gray in ids
            .iter()
            .filter(|id| matches!(w.entities[id].kind, 0x6d5..=0x6d7))
        {
            assert!((w.entities[gray].pos.distance(w.entities[&hub].pos) - 1.).abs() < 0.00001);
        }
        w.remove_entity(root);
        assert!(
            members(&w, 0)
                .iter()
                .all(|id| w.entities[id].expires == Some(30000))
        );
        w.now = 29999;
        w.mechanic_expiry();
        assert_eq!(members(&w, 0).len(), 4);
        w.now = 30000;
        w.mechanic_expiry();
        assert!(members(&w, 0).is_empty());
    }
}
#[test]
fn satellite_anchors_identify_the_nearby_member_not_the_first_matching_kind() {
    let mut w = arena(7);
    let pos = center(&w);
    w.populate_families(7, &[pos], &[0x62d]).unwrap();
    let far = Pos {
        x: pos.x + 8.,
        ..pos
    };
    let root = w.spawn_entity(0x62d, far, vec![]).unwrap();
    let hub = w.spawn_entity(0x6d2, far, vec![]).unwrap();
    let gray = w.spawn_entity(0x6d5, far, vec![]).unwrap();
    for id in [root, hub, gray] {
        w.entities.get_mut(&id).unwrap().camp = Some(0);
    }
    for (id, owner, radius) in [(hub, root, 3.), (gray, hub, 1.)] {
        let (motion, _) = w.golem_movement(id, 0).unwrap();
        assert_eq!(
            motion,
            crate::encounter::Motion::StrictOrbit {
                anchor: crate::encounter::Anchor::CampEntity(owner),
                radius,
            }
        );
        assert_eq!(
            w.orbit_anchor(id, crate::encounter::Anchor::CampEntity(owner), None)
                .unwrap()
                .x,
            far.x
        );
    }
    w.entities.get_mut(&root).unwrap().camp = Some(1);
    assert!(
        w.orbit_anchor(hub, crate::encounter::Anchor::CampEntity(root), None)
            .is_err()
    );
}
#[test]
fn secondary_leaders_replenish_only_their_own_bounded_followers_after_top_slot_death() {
    for (root_kind, leader_kind, child_kind) in [(0x640, 0x63e, 0x63a), (0x650, 0x64f, 0x64a)] {
        let mut w = arena(4);
        let terrain = w.content.objects[&root_kind].terrain;
        for c in &mut Arc::make_mut(&mut w.map).cells {
            c.terrain = terrain;
        }
        w.configure_population().unwrap();
        let pos = center(&w);
        w.populate_families(terrain, &[pos], &[root_kind]).unwrap();
        let root = w
            .entities
            .values()
            .find(|e| e.kind == root_kind)
            .unwrap()
            .id;
        let leader = w.camp_member(root, leader_kind).unwrap().unwrap();
        let child = w.camp_member(root, child_kind).unwrap().unwrap();
        let before = w.entities.values().filter(|e| e.kind == child_kind).count();
        let leader_pos = w.entities[&leader].pos;
        player(
            &mut w,
            Pos {
                x: leader_pos.x + 5.,
                ..leader_pos
            },
        );
        w.remove_entity(root);
        w.remove_entity(child);
        w.now = 10000;
        w.repopulate().unwrap();
        assert_eq!(
            w.entities.values().filter(|e| e.kind == child_kind).count(),
            before
        );
        assert!(!w.entities.values().any(|e| e.kind == root_kind));
        assert!(w.population_count(terrain) <= 32);
        w.remove_entity(leader);
        let child = w
            .entities
            .values()
            .find(|e| e.kind == child_kind)
            .unwrap()
            .id;
        w.remove_entity(child);
        w.now = 20000;
        w.repopulate().unwrap();
        assert_eq!(
            w.entities.values().filter(|e| e.kind == child_kind).count(),
            before - 1
        );
    }
}
#[test]
fn lair_escorts_protect_their_own_boss_then_follow_with_resting_hysteresis() {
    let mut w = arena(0);
    let pos = center(&w);
    let own = w
        .spawn_entity(
            0xd90,
            Pos {
                x: pos.x - 8.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    let other = w
        .spawn_entity(
            0xd90,
            Pos {
                x: pos.x + 8.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    for id in [own, other] {
        let e = w.entities.get_mut(&id).unwrap();
        e.stage = Some(0);
        e.activated = Some(0);
        w.apply_condition(id, condition::PARALYZED, 10000).unwrap();
    }
    let id = w
        .spawn_entity(
            0xdaa,
            Pos {
                x: pos.x + 1.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&id).unwrap().parent = Some(own);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 4.,
            ..pos
        },
    );
    let before = w.entities[&id].pos;
    w.npc_step();
    assert!(w.entities[&id].pos.x < before.x);
    let near = Pos {
        x: w.entities[&own].pos.x + 0.9,
        y: pos.y,
    };
    w.move_entity(id, near, 100.).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: near.x + 6.5,
        ..near
    };
    w.now = 50;
    w.npc_step();
    let before = w.entities[&id].pos;
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: before.x + 7.5,
        ..before
    };
    w.now = 100;
    w.npc_step();
    assert!(w.entities[&id].pos.distance(before) < 0.15);
    let before = w.entities[&id].pos;
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: before.x + 8.5,
        ..before
    };
    w.now = 150;
    w.npc_step();
    assert!(w.entities[&id].pos.distance(before) > 0.2);
}
#[test]
fn lair_paladin_full_heal_respects_sick_stun_and_encounter_ownership() {
    let mut w = arena(0);
    let pos = center(&w);
    let a = w.spawn_entity(0xd90, pos, vec![]).unwrap();
    let b = w.spawn_entity(0xd90, pos, vec![]).unwrap();
    let healer = w.spawn_entity(0xdae, pos, vec![]).unwrap();
    let friend = w.spawn_entity(0xdab, pos, vec![]).unwrap();
    let foreign = w.spawn_entity(0xdab, pos, vec![]).unwrap();
    for (id, parent) in [(healer, a), (friend, a), (foreign, b)] {
        let e = w.entities.get_mut(&id).unwrap();
        e.parent = Some(parent);
        e.hp = w.content.objects[&e.kind].hp - 150;
    }
    let hurt = w.entities[&healer].hp;
    let friend_hurt = w.entities[&friend].hp;
    w.apply_condition(healer, condition::STUNNED, 1000).unwrap();
    w.apply_condition(friend, condition::SICK, 5000).unwrap();
    w.encounter_mechanics(healer, pos, 0).unwrap();
    assert_eq!(w.entities[&healer].hp, hurt);
    w.now = 1000;
    w.encounter_mechanics(healer, pos, 1000).unwrap();
    assert_eq!(w.entities[&healer].hp, w.content.objects[&0xdae].hp);
    assert_eq!(w.entities[&friend].hp, friend_hurt);
    assert_eq!(w.entities[&foreign].hp, friend_hurt);
    w.now = 6000;
    w.encounter_mechanics(healer, pos, 6000).unwrap();
    assert_eq!(w.entities[&friend].hp, w.content.objects[&0xdab].hp);
    assert_eq!(w.entities[&foreign].hp, friend_hurt);
}
#[test]
fn armor_waits_for_real_damage_then_heals_without_exceeding_selected_hp() {
    let mut w = arena(0);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 5.,
            ..pos
        },
    );
    let id = w.spawn_entity(0xd7e, pos, vec![]).unwrap();
    let max = w.content.objects[&0xd7e].hp;
    assert!(shots(&mut w, id).is_empty());
    assert_eq!(w.entities[&id].stage, None);
    w.entities.get_mut(&id).unwrap().hp = max - 1;
    w.now = 50;
    assert_eq!(shots(&mut w, id).len(), 2);
    assert!(w.entities[&id].extra_stats.contains(&(61, 1)));
    w.entities.get_mut(&id).unwrap().hp = max / 5 - 1;
    w.now = 100;
    assert_eq!(shots(&mut w, id).len(), 6);
    assert_eq!(w.entities[&id].stage, Some(2));
    assert!(w.entities[&id].extra_stats.contains(&(61, 0)));
    assert_ne!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    for ms in (150..=1600).step_by(50) {
        w.now = ms;
        w.npc_step();
        assert!(w.entities[&id].hp <= max);
    }
    assert_eq!(w.entities[&id].hp, max);
    assert_eq!(w.entities[&id].stage, Some(1));
    assert_eq!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
}
#[test]
fn limon_immobility_bursts_and_box_weapons_have_their_own_deadlines() {
    let mut w = arena(0);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    let id = w.spawn_entity(0xd06, pos, vec![]).unwrap();
    {
        let e = w.entities.get_mut(&id).unwrap();
        e.activated = Some(0);
        e.stage = Some(2);
    }
    w.apply_condition(id, condition::PARALYZED, 60000).unwrap();
    let mut burst = 0;
    for ms in (2300..=2800).step_by(50) {
        w.now = ms;
        let n = shots(&mut w, id).len();
        if n == 18 {
            burst += 1;
        }
    }
    assert_eq!(burst, 1);
    assert_ne!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now = 3800;
    w.npc_step();
    assert_eq!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now = 12300;
    assert_eq!(shots(&mut w, id).len(), 4);
    w.now = 13800;
    assert_eq!(shots(&mut w, id).len(), 3);
    w.now = 14550;
    assert!(shots(&mut w, id).is_empty());
    w.now = 14600;
    assert_eq!(shots(&mut w, id).len(), 3);
    w.entities.get_mut(&id).unwrap().hp = w.content.objects[&0xd06].hp / 5 - 1;
    w.now = 32300;
    w.npc_step();
    assert_eq!(w.entities[&id].stage, Some(0));
    assert_eq!(w.entities[&id].activated, Some(32300));
}
#[test]
fn oryx_dance_uses_phase_relative_two_hundred_ms_offset_and_seven_hundred_ms_deadlines() {
    let mut w = arena(0);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    let id = w.spawn_entity(endgame::ORYX, pos, vec![]).unwrap();
    w.npc_step();
    let mut emissions = vec![];
    for ms in (20000..=23000).step_by(50) {
        w.now = ms;
        let volley = shots(&mut w, id);
        if !volley.is_empty() {
            assert_eq!(volley, vec![6; 8]);
            emissions.push(ms);
        }
    }
    assert_eq!(emissions, vec![20200, 20900, 21600, 22300, 23000]);
    w.now = 105000;
    w.mechanic_expiry();
    w.bullets.clear();
    assert!(shots(&mut w, id).is_empty());
    w.now = 105200;
    assert_eq!(shots(&mut w, id), vec![6; 8]);
    w.entities.get_mut(&id).unwrap().hp = w.content.objects[&endgame::ORYX].hp / 5;
    let before = w.entities[&id].pos;
    w.now = 105250;
    w.npc_step();
    assert!(w.entities[&id].pos.distance(before) > 0.);
}
#[test]
fn red_demon_wanders_then_pursues_and_returns_home_instead_of_permanent_chase() {
    let mut w = arena(1);
    let home = center(&w);
    let p = player(
        &mut w,
        Pos {
            x: home.x + 9.,
            ..home
        },
    );
    let id = w.spawn_entity(0x668, home, vec![]).unwrap();
    for ms in (0..5000).step_by(50) {
        w.now = ms;
        w.npc_step();
        assert!(w.entities[&id].pos.distance(home) <= 1.01);
    }
    let distance = w.entities[&id].pos.distance(w.players[&p].pos);
    for ms in (5000..=6000).step_by(50) {
        w.now = ms;
        w.npc_step();
    }
    assert!(w.entities[&id].pos.distance(w.players[&p].pos) < distance - 1.);
    let mut returned = false;
    for ms in (6050..=16000).step_by(50) {
        w.now = ms;
        w.npc_step();
        if w.entities[&id].stage == Some(0) {
            returned = true;
            break;
        }
    }
    assert!(returned);
    assert!((w.entities[&id].pos.x - home.x).abs() <= 1.1);
}
#[test]
fn hero_reinforcement_opportunities_choose_one_role_instead_of_filling_every_role() {
    for kind in [0x668, 0x67d] {
        let mut w = arena(1);
        let pos = center(&w);
        player(
            &mut w,
            Pos {
                x: pos.x + 6.,
                ..pos
            },
        );
        let id = w.spawn_entity(kind, pos, vec![]).unwrap();
        w.apply_condition(id, condition::PARALYZED, 60000).unwrap();
        w.npc_step();
        for child in w
            .entities
            .values()
            .filter(|e| e.parent == Some(id))
            .map(|e| e.id)
            .collect::<Vec<_>>()
        {
            w.remove_entity(child);
        }
        let mut previous = BTreeMap::<u16, usize>::new();
        for ms in (2000..=40000).step_by(2000) {
            w.now = ms;
            w.npc_step();
            let mut counts = BTreeMap::new();
            for e in w.entities.values().filter(|e| e.parent == Some(id)) {
                *counts.entry(e.kind).or_insert(0) += 1;
            }
            assert!(counts.values().all(|n| *n <= 3));
            assert!(
                counts
                    .iter()
                    .filter(|(kind, n)| **n > previous.get(kind).copied().unwrap_or(0))
                    .count()
                    <= 1
            );
            previous = counts;
        }
        assert!(!previous.is_empty());
    }
}
#[test]
fn phantom_forms_reuse_one_population_slot_and_actual_damage_without_early_rewards() {
    let mut w = arena(5);
    let pos = center(&w);
    w.populate_solos(5, &[pos], &[0x6c5]).unwrap();
    let original = w.population[0].entity.unwrap();
    w.entities.get_mut(&original).unwrap().damage.insert(77, 17);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 5.,
            ..pos
        },
    );
    let xp = w.players[&p].character.exp;
    w.npc_step();
    w.now = 5000;
    w.npc_step();
    let wisp = w.population[0].entity.unwrap();
    assert_ne!(original, wisp);
    assert!(!w.entities.contains_key(&original));
    assert_eq!(w.entities[&wisp].kind, 0x7f22);
    assert_eq!(w.entities[&wisp].damage[&77], 17);
    assert_eq!(w.population_count(5), 1);
    w.now = 14999;
    w.npc_step();
    assert_eq!(w.population[0].entity, Some(wisp));
    w.now = 15000;
    w.npc_step();
    let restored = w.population[0].entity.unwrap();
    assert_eq!(w.entities[&restored].kind, 0x6c5);
    assert_eq!(w.entities[&restored].damage[&77], 17);
    assert_eq!(w.population[0].kind, 0x6c5);
    assert_eq!(w.population_count(5), 1);
    assert_eq!(w.players[&p].character.exp, xp);
    assert_eq!(w.players[&p].character.kills, 0);
    assert!(w.critical_deaths.is_empty());
}
#[test]
fn fauna_packs_draw_initial_counts_and_follow_only_their_owned_leader() {
    for (kind, child, terrain) in [(0x623, 0x620, 5), (0x947, 0x948, 6), (0x945, 0x946, 7)] {
        let mut observed = BTreeSet::new();
        for seed in 0..8 {
            let mut w = arena(terrain);
            w.rng = rand::rngs::StdRng::seed_from_u64(seed);
            let pos = center(&w);
            w.populate_families(terrain, &[pos], &[kind]).unwrap();
            let ids = members(&w, 0);
            observed.insert(ids.len());
            let leader = *ids.iter().find(|id| w.entities[id].kind == kind).unwrap();
            let follower = *ids.iter().find(|id| w.entities[id].kind == child).unwrap();
            w.move_entity(
                follower,
                Pos {
                    x: pos.x + 4.,
                    ..pos
                },
                100.,
            )
            .unwrap();
            let motion = w.fauna_movement(follower, None, 0).unwrap().0;
            assert!(
                matches!(motion,encounter::Motion::Orbit{anchor:encounter::Anchor::CampEntity(id),..}
                |encounter::Motion::Protect{anchor:encounter::Anchor::CampEntity(id),..} if id==leader)
            );
            let foreign = w
                .spawn_entity(kind, w.entities[&leader].pos, vec![])
                .unwrap();
            w.entities.get_mut(&foreign).unwrap().camp = Some(1);
            w.remove_entity(leader);
            assert_eq!(
                w.fauna_movement(follower, None, 0).unwrap(),
                (encounter::Motion::Wander, 4.)
            );
            assert!(w.population_count(terrain) <= 32);
        }
        if kind != 0x945 {
            assert!(observed.len() > 1, "pack initial counts are fixed");
        }
    }
}
#[test]
fn dazed_reduces_each_independent_volley_without_erasing_single_shot_weapons() {
    let mut w = arena(8);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 4.,
            ..pos
        },
    );
    let sorcerer = w.spawn_entity(0x60c, pos, vec![]).unwrap();
    w.apply_condition(sorcerer, condition::DAZED, 1000).unwrap();
    assert_eq!(shots(&mut w, sorcerer), vec![0, 1]);
}
#[test]
fn new_fauna_keeps_selected_projectiles_and_werewolf_enrage_does_not_recreate_followers() {
    let mut w = arena(5);
    for kind in encounter::fauna::ROSTER {
        encounter::validate(&w.content, *kind).unwrap();
        crate::loot::validate(&w.content, *kind).unwrap();
    }
    let pos = center(&w);
    w.populate_families(5, &[pos], &[0x623]).unwrap();
    let root = members(&w, 0)
        .into_iter()
        .find(|id| w.entities[id].kind == 0x623)
        .unwrap();
    player(
        &mut w,
        Pos {
            x: pos.x + 4.,
            ..pos
        },
    );
    w.npc_step();
    let count = w.population_count(5);
    w.entities.get_mut(&root).unwrap().hp = 199;
    w.now = 50;
    assert_eq!(shots(&mut w, root), vec![1]);
    assert_eq!(w.entities[&root].size, 150);
    assert_eq!(w.population_count(5), count);
    assert_eq!(encounter::phased_attacks(0x205, 100, 1., 0).len(), 3);
}
#[test]
fn charge_captures_a_position_instead_of_homing_and_paralysis_freezes_it() {
    let mut w = arena(0);
    let pos = center(&w);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    let bat = w.spawn_entity(0xda0, pos, vec![]).unwrap();
    w.npc_step();
    assert!(w.entities[&bat].pos.x > pos.x);
    w.players.get_mut(&p).unwrap().pos.y += 5.;
    w.now = 50;
    w.npc_step();
    assert_eq!(w.entities[&bat].pos.y, pos.y);
    let held = w.entities[&bat].pos;
    let goal = w.entities[&bat].goal.unwrap();
    w.apply_condition(bat, condition::PARALYZED, 1000).unwrap();
    w.now = 1000;
    w.npc_step();
    assert_eq!(w.entities[&bat].pos.distance(held), 0.);
    assert_eq!(w.entities[&bat].goal.unwrap().distance(goal), 0.);
    w.now = 1050;
    w.npc_step();
    assert!(w.entities[&bat].pos.x > held.x);
    assert_eq!(w.entities[&bat].pos.y, pos.y);
}
#[test]
fn gray_blob_emits_one_proximity_ring_then_withdraws_without_defeat_credit() {
    let mut w = arena(6);
    let pos = center(&w);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 1.5,
            ..pos
        },
    );
    let blob = w.spawn_entity(0x20d, pos, vec![]).unwrap();
    assert_eq!(shots(&mut w, blob).len(), 12);
    assert!(shots(&mut w, blob).is_empty());
    w.now = 50;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&blob));
    assert_eq!(w.players[&p].character.kills, 0);
    assert_eq!(w.players[&p].experience_queued, 0);
    assert!(w.critical_deaths.is_empty());
}
#[tokio::test]
async fn giant_crab_defeat_pickup_is_committed_once_and_survives_reopen() {
    use crate::store::{InventoryWrite, Store};
    let mut w = arena(8);
    w.name = "Realm".into();
    w.rng = rand::rngs::StdRng::seed_from_u64(9009);
    let path = std::env::temp_dir().join(format!("rotmg-giant-crab-{}.db", rand::random::<u64>()));
    let store = Store::open(&path, &w.content).unwrap();
    let account = store.list("crab-loot".into()).await.unwrap();
    let mut ch = store
        .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(
        &w.content,
        18050,
        &mut rand::rngs::StdRng::seed_from_u64(9009),
    )
    .unwrap();
    ch.items[4..].fill(-1);
    store.progress(ch.clone()).await.unwrap();
    let id = w.enter(ch.clone(), None).unwrap();
    let pos = center(&w);
    w.players.get_mut(&id).unwrap().pos = pos;
    w.players.get_mut(&id).unwrap().controller = Controller::Human;
    let mut kills = 0;
    for _ in 0..8 {
        let crab = w
            .spawn_entity(
                0x6c4,
                Pos {
                    x: pos.x + 0.5,
                    ..pos
                },
                vec![],
            )
            .unwrap();
        for _ in 0..4 {
            w.damage_enemy(id, crab, 100, false, &[], Some(0)).unwrap();
        }
        assert!(!w.entities.contains_key(&crab));
        kills += 1;
        let state = &w.players[&id];
        assert_eq!(
            (state.character.kills, state.character.quests),
            (kills, kills)
        );
        assert_eq!(state.experience_queued, 44 * kills as i32);
        let queued = state.experience_queued;
        assert_eq!(
            w.damage_enemy(id, crab, 100, false, &[], Some(0)).unwrap(),
            0
        );
        assert_eq!(w.players[&id].experience_queued, queued);
        if !w.bag_expiry.is_empty() {
            break;
        }
    }
    assert!(
        !w.bag_expiry.is_empty(),
        "defeated ordinary Giant Crabs never award item loot"
    );
    assert!(w.portal_requests.is_empty());
    assert!(w.critical_deaths.is_empty());
    for (player, snapshot) in w.pending_progress() {
        store.progress(snapshot.clone()).await.unwrap();
        w.finish_progress(player, snapshot).unwrap();
    }
    assert_eq!(w.players[&id].character.exp, ch.exp + 44 * kills as i32);
    assert_eq!(w.players[&id].character.fame, ch.fame);
    let bag = *w.bag_expiry.keys().next().unwrap();
    let source = &w.entities[&bag];
    assert_eq!(source.kind, 0x500);
    assert_eq!(source.items.len(), 8);
    assert!(!source.extra_stats.iter().any(|(stat, _)| *stat == 54));
    assert_eq!(w.bag_expiry[&bag], w.now + 60_000);
    let item = source.items[0];
    let before = w.players[&id].character.clone();
    let from = Slot {
        object: bag,
        slot: 0,
        kind: item,
    };
    let to = Slot {
        object: id,
        slot: 4,
        kind: -1,
    };
    let plan = w.swap(id, from, to).unwrap();
    assert_eq!(plan.collected, Some(item as u16));
    assert!(w.entities[&bag].reserved && w.players[&id].pending);
    assert_eq!(w.players[&id].character.items[4], -1);
    assert!(w.swap(id, from, to).is_err());
    let write = InventoryWrite {
        id: plan.character,
        account: plan.account,
        revision: plan.revision,
        items: plan.items.clone(),
    };
    let revisions = store
        .exchange_collected(vec![write.clone()], None, plan.collected)
        .await
        .unwrap();
    assert!(
        store
            .exchange_collected(vec![write], None, plan.collected)
            .await
            .is_err()
    );
    w.finish_inventory(plan, Ok(revisions[0])).unwrap();
    assert_eq!(w.entities[&bag].items[0], -1);
    assert!(!w.entities[&bag].reserved);
    assert!(!w.entities[&bag].drop_slots.contains(&0));
    assert_eq!(w.players[&id].character.items[4], item);
    assert_eq!(w.players[&id].character.revision, before.revision + 1);
    assert!(w.swap(id, from, to).is_err());
    drop(store);
    let reopened = Store::open(&path, &w.content).unwrap();
    let saved = reopened.load(account.id, ch.id).await.unwrap();
    assert_eq!(saved.items, w.players[&id].character.items);
    assert_eq!(
        (saved.exp, saved.fame, saved.kills, saved.quests),
        (ch.exp + 44 * kills as i32, ch.fame, kills, kills)
    );
    assert_eq!(saved.revision, before.revision + 1);
    assert_eq!(
        (saved.xp_boost.active, saved.xp_boost.bonus_exp),
        (ch.xp_boost.active, ch.xp_boost.bonus_exp)
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn giant_crab_public_attempts_use_only_live_positive_contributors_and_the_current_cap() {
    for contributors in [0, 1, 2, 5] {
        let mut w = arena(8);
        let pos = center(&w);
        let crab = w.spawn_entity(0x6c4, pos, vec![]).unwrap();
        for index in 0..contributors + 4 {
            let mut ch = tests::ch(&w);
            ch.id = index + 1;
            ch.account = index + 1;
            let id = w.enter(ch, None).unwrap();
            let p = w.players.get_mut(&id).unwrap();
            if index == contributors {
                p.character.alive = false;
            }
            if index == contributors + 1 {
                p.death_pending = true;
            }
            if index == contributors + 2 {
                p.departing = true;
            }
            w.entities
                .get_mut(&crab)
                .unwrap()
                .damage
                .insert(index + 1, if index == contributors + 3 { 0 } else { 1 });
        }
        let mut expected = Vec::new();
        let mut rng = rand::rngs::StdRng::seed_from_u64(9009);
        for _ in 0..(contributors * 2).min(8) {
            for item in crate::loot::rewards(&w.content, 0x6c4, &mut rng) {
                if !expected.contains(&(item as i32)) || matches!(item, 0xa22 | 0xa23) {
                    expected.push(item as i32);
                }
            }
        }
        w.rng = rand::rngs::StdRng::seed_from_u64(9009);
        let defeated = w.remove_entity(crab);
        w.drop_loot(&defeated).unwrap();
        let actual: Vec<_> = w
            .bag_expiry
            .keys()
            .flat_map(|id| {
                let bag = &w.entities[id];
                assert_eq!(bag.kind, 0x500);
                assert!(!bag.extra_stats.iter().any(|(stat, _)| *stat == 54));
                bag.items.iter().copied().filter(|item| *item >= 0)
            })
            .collect();
        assert_eq!(actual, expected, "contributors={contributors}");
    }
}

#[test]
fn crab_uses_all_four_weapons_then_three_secondaries_without_phase_boundary_loss() {
    let mut w = arena(8);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 4.,
            ..pos
        },
    );
    let crab = w.spawn_entity(0x6c4, pos, vec![]).unwrap();
    w.apply_condition(crab, condition::PARALYZED, 10000)
        .unwrap();
    assert!(shots(&mut w, crab).is_empty());
    for ms in [1000, 2000, 3000, 4000, 5000] {
        w.now = ms;
        assert_eq!(shots(&mut w, crab), vec![0, 1, 2, 3]);
    }
    for ms in [5500, 6000, 6500] {
        w.now = ms;
        assert_eq!(shots(&mut w, crab), vec![4]);
    }
    w.now = 7500;
    assert_eq!(shots(&mut w, crab), vec![0, 1, 2, 3]);
}
#[test]
fn nymph_random_choice_selects_a_whole_fan_not_just_another_projectile_id() {
    let mut w = arena(10);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 4.,
            ..pos
        },
    );
    w.rng = rand::rngs::StdRng::seed_from_u64(3);
    let nymph = w.spawn_entity(0x6d1, pos, vec![]).unwrap();
    w.apply_condition(nymph, condition::PARALYZED, 60000)
        .unwrap();
    let mut kinds = BTreeSet::new();
    for ms in (0..45000).step_by(1500) {
        w.now = ms;
        let volley = shots(&mut w, nymph);
        let kind = volley[0];
        kinds.insert(kind);
        assert!(volley.iter().all(|p| *p == kind));
        assert_eq!(volley.len(), if kind == 0 { 1 } else { 6 });
    }
    assert_eq!(kinds, BTreeSet::from([0, 1]));
}
#[test]
fn death_successors_and_dragon_growth_keep_one_slot_without_carrying_dead_form_damage() {
    for (kind, terrain, successor) in [(0x6c8, 10, 0x6c9), (0x206, 4, 0x207)] {
        let mut w = arena(terrain);
        let pos = center(&w);
        if kind == 0x206 {
            w.populate_families(terrain, &[pos], &[kind]).unwrap();
        } else {
            w.populate_solos(terrain, &[pos], &[kind]).unwrap();
        }
        let old = w.population[0].entity.unwrap();
        w.entities.get_mut(&old).unwrap().damage.insert(88, 10);
        w.entities.get_mut(&old).unwrap().hp = 0;
        w.defeat_entity(old).unwrap();
        let new = w.population[0].entity.unwrap();
        assert_ne!(new, old);
        assert_eq!(w.entities[&new].kind, successor);
        assert!(w.entities[&new].damage.is_empty());
        assert_eq!(w.population_count(terrain), 1);
        if kind == 0x206 {
            w.now = 60000;
            w.encounter_mechanics(new, None, 60000).unwrap();
            let juvenile = w.population[0].entity.unwrap();
            assert_eq!(w.entities[&juvenile].kind, 0x208);
            w.now = 120000;
            w.encounter_mechanics(juvenile, None, 60000).unwrap();
            let adult = w.population[0].entity.unwrap();
            assert_eq!(w.entities[&adult].kind, 0x209);
            player(
                &mut w,
                Pos {
                    x: pos.x + 5.,
                    ..pos
                },
            );
            w.repopulate().unwrap();
            assert_eq!(w.population_count(terrain), 1);
            w.now = 179999;
            w.repopulate().unwrap();
            assert_eq!(w.population_count(terrain), 1);
            w.now = 180000;
            w.repopulate().unwrap();
            assert_eq!(w.population_count(terrain), 2);
        }
        assert!(w.critical_deaths.is_empty());
    }
}
#[test]
fn green_slime_death_splits_into_owned_finite_children_and_not_a_fresh_parent() {
    let mut w = arena(6);
    let pos = center(&w);
    w.populate_families(6, &[pos], &[0x20e]).unwrap();
    let root = w.population[0].entity.unwrap();
    w.entities.get_mut(&root).unwrap().hp = 0;
    w.defeat_entity(root).unwrap();
    let children = members(&w, 0);
    assert!((3..=5).contains(&children.len()));
    assert!(
        children
            .iter()
            .all(|id| w.entities[id].kind == 0x20f && w.entities[id].hp == 50)
    );
    w.repopulate().unwrap();
    w.now = 60000;
    w.repopulate().unwrap();
    assert!(w.population[0].entity.is_none());
    assert_eq!(members(&w, 0), children);
}
#[test]
fn spectres_have_independently_ranged_shots_and_owner_death_does_not_cancel_their_grenade() {
    for (kind, amount) in [(0xda7, 50), (0xda8, 70), (0xda9, 90)] {
        let mut w = arena(0);
        let pos = center(&w);
        let p = player(
            &mut w,
            Pos {
                x: pos.x + 7.,
                ..pos
            },
        );
        let spectre = w.spawn_entity(kind, pos, vec![]).unwrap();
        assert_eq!(shots(&mut w, spectre), vec![0]);
        assert_eq!(w.grenades.len(), 1);
        w.remove_entity(spectre);
        let hp = w.players[&p].character.hp;
        let damage = condition::damage(
            amount,
            w.defense(p).unwrap(),
            w.players[&p].conditions(w.now),
            false,
        );
        w.now = 1500;
        w.grenade_step();
        assert_eq!(w.players[&p].character.hp, (hp - damage).max(0));
        assert!(w.grenades.is_empty());
    }
}
#[test]
fn admitted_sludgets_land_after_owner_death_without_exceeding_their_owned_slots() {
    let mut w = arena(6);
    let pos = center(&w);
    w.populate_families(6, &[pos], &[0x950]).unwrap();
    let root = w.population[0].entity.unwrap();
    w.fauna_wave_mechanics(root, None).unwrap();
    assert_eq!(w.population_count(6), 1);
    assert_eq!(w.pending_fauna_count(6), 5);
    w.remove_entity(root);
    w.now = 1499;
    w.mechanic_expiry();
    assert!(members(&w, 0).is_empty());
    w.now = 1500;
    w.mechanic_expiry();
    let children = members(&w, 0);
    assert_eq!(children.len(), 5);
    assert!(
        children
            .iter()
            .all(|id| w.entities[id].kind == 0x951 && w.entities[id].hp == 250)
    );
    assert_eq!(w.pending_fauna_count(6), 0);
    assert_eq!(w.population_count(6), 5);
}
#[test]
fn pending_fauna_reserves_regional_capacity_before_the_throw_lands() {
    let mut w = arena(6);
    w.population_regions[6].limit = 32;
    let pos = center(&w);
    w.populate_families(6, &[pos], &[0x950]).unwrap();
    let root = w.population[0].entity.unwrap();
    for _ in 0..27 {
        let id = w.spawn_entity(0x624, pos, vec![]).unwrap();
        let slot = w.population.len();
        w.population.push(Spawn {
            kind: 0x624,
            pos,
            entity: Some(id),
            camp: None,
        });
        w.mark_population(id, slot).unwrap();
    }
    w.fauna_wave_mechanics(root, None).unwrap();
    assert_eq!(w.population_count(6), 28);
    assert_eq!(w.pending_fauna_count(6), 4);
    assert!(!w.population_space(6, 1).unwrap());
    w.now = 1500;
    w.mechanic_expiry();
    assert_eq!(w.population_count(6), 32);
}
#[test]
fn urgle_traps_keep_their_distinct_directions_sparse_projectiles_and_finite_lives() {
    let mut w = arena(3);
    let pos = center(&w);
    w.populate_families(3, &[pos], &[0x957]).unwrap();
    let root = w.population[0].entity.unwrap();
    let target = Some(Pos {
        x: pos.x + 6.,
        ..pos
    });
    w.fauna_wave_mechanics(root, target).unwrap();
    assert_eq!(w.pending_fauna_count(3), 3);
    w.now = 1000;
    w.fauna_wave_mechanics(root, target).unwrap();
    w.now = 1500;
    w.mechanic_expiry();
    w.now = 2000;
    w.fauna_wave_mechanics(root, target).unwrap();
    w.now = 2500;
    w.mechanic_expiry();
    w.now = 3500;
    w.mechanic_expiry();
    assert_eq!(w.population_count(3), 16);
    for (kind, count) in [(0x958, 3), (0x959, 4), (0x960, 4), (0x961, 4)] {
        let ids = w
            .entities
            .values()
            .filter(|e| e.kind == kind)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), count);
        assert!(
            ids.iter()
                .all(|e| e.conditions(w.now) & condition::bit(condition::INVULNERABLE) != 0)
        );
        for (t, index) in [(100, 1), (1100, 2), (2100, 3)] {
            let volley = encounter::phased_attacks(kind, 100000, 1., t);
            assert_eq!(volley.len(), 1);
            assert_eq!(volley[0].0, index);
        }
    }
    w.now = 5400;
    w.fauna_wave_mechanics(root, target).unwrap();
    assert_eq!(w.pending_fauna_count(3), 0);
    assert_eq!(w.population_count(3), 16);
    w.now = 13500;
    w.mechanic_expiry();
    assert_eq!(w.population_count(3), 1);
}
#[test]
fn flamer_self_withdrawal_bursts_once_without_fabricating_death_rewards() {
    let mut w = arena(2);
    let pos = center(&w);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 1.5,
            ..pos
        },
    );
    let id = w.spawn_entity(0x904, pos, vec![]).unwrap();
    let character = w.players[&p].character.id;
    w.entities.get_mut(&id).unwrap().hp = 199;
    w.entities
        .get_mut(&id)
        .unwrap()
        .damage
        .insert(character, 801);
    assert_eq!(shots(&mut w, id).len(), 10);
    assert!(shots(&mut w, id).is_empty());
    w.now = 50;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&id));
    assert_eq!(w.players[&p].experience_queued, 0);
    assert_eq!(w.players[&p].character.kills, 0);
    w.mechanic_expiry();
    assert_eq!(w.players[&p].experience_queued, 0);
    assert_eq!(w.players[&p].character.kills, 0);
}
#[test]
fn airborne_fauna_reservations_prevent_camp_replacement_until_landing() {
    let mut w = arena(6);
    let pos = center(&w);
    w.populate_families(6, &[pos], &[0x950]).unwrap();
    let root = w.population[0].entity.unwrap();
    w.fauna_wave_mechanics(root, None).unwrap();
    w.remove_entity(root);
    w.population[0].entity = None;
    w.now = 1000;
    w.repopulate_families().unwrap();
    assert!(w.entities.is_empty());
    assert_eq!(w.pending_fauna_count(6), 5);
    w.now = 1500;
    w.mechanic_expiry();
    assert_eq!(members(&w, 0).len(), 5);
    assert!(w.entities.values().all(|e| e.kind == 0x951));
}
#[test]
fn fauna_tosses_refuse_blocked_endpoints_and_recheck_airborne_landings() {
    let mut w = arena(6);
    let pos = center(&w);
    w.populate_families(6, &[pos], &[0x950]).unwrap();
    let root = w.population[0].entity.unwrap();
    let landing = Pos {
        x: pos.x + 4.,
        ..pos
    };
    let wall = w.content.objects.values().find(|o| o.blocks).unwrap().kind;
    let obstacle = w.spawn_entity(wall, landing, vec![]).unwrap();
    assert_eq!(
        w.fauna_toss(root, 0x951, &[landing], true, None).unwrap(),
        0
    );
    assert_eq!(w.pending_fauna_count(6), 0);
    w.remove_entity(obstacle);
    assert_eq!(
        w.fauna_toss(root, 0x951, &[landing], true, None).unwrap(),
        1
    );
    w.spawn_entity(wall, landing, vec![]).unwrap();
    w.now = 1500;
    w.mechanic_expiry();
    assert_eq!(w.pending_fauna_count(6), 0);
    assert_eq!(w.population_count(6), 1);
}
#[test]
fn sludge_replacement_withdraws_old_children_without_rewards_or_population_growth() {
    let mut w = arena(6);
    let pos = center(&w);
    let p = player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    w.populate_families(6, &[pos], &[0x950]).unwrap();
    let root = w.population[0].entity.unwrap();
    w.fauna_wave_mechanics(root, None).unwrap();
    w.now = 1500;
    w.mechanic_expiry();
    let old = members(&w, 0)
        .into_iter()
        .filter(|id| *id != root)
        .collect::<Vec<_>>();
    w.now = 10000;
    w.fauna_wave_mechanics(root, None).unwrap();
    assert_eq!(w.pending_fauna_count(6), 0);
    assert_eq!(w.population_count(6), 6);
    w.now = 11500;
    w.mechanic_expiry();
    assert!(old.iter().all(|id| !w.entities.contains_key(id)));
    assert_eq!(w.population_count(6), 6);
    assert_eq!(w.players[&p].experience_queued, 0);
    assert_eq!(w.players[&p].character.kills, 0);
}
#[test]
fn swarm_dart_rings_use_separate_once_only_eight_hundred_and_fourteen_hundred_ms_offsets() {
    let mut w = arena(6);
    let pos = center(&w);
    player(
        &mut w,
        Pos {
            x: pos.x + 6.,
            ..pos
        },
    );
    let swarm = w.spawn_entity(0x955, pos, vec![]).unwrap();
    w.apply_condition(swarm, condition::PARALYZED, 10000)
        .unwrap();
    w.npc_step();
    w.now = 3000;
    assert!(shots(&mut w, swarm).is_empty());
    w.now = 3750;
    assert!(shots(&mut w, swarm).is_empty());
    w.now = 3800;
    assert_eq!(shots(&mut w, swarm).len(), 5);
    w.now = 3850;
    assert!(shots(&mut w, swarm).is_empty());
    w.now = 4400;
    assert_eq!(shots(&mut w, swarm).len(), 5);
    w.now = 4450;
    assert!(shots(&mut w, swarm).is_empty());
}
