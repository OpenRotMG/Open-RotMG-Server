use super::*;
use crate::{
    condition,
    encounter::{Anchor, Motion, attacks::Action, landmarks},
};
fn arena() -> (World, Pos, i32) {
    let mut w = tests::arena(0);
    let pos = Pos {
        x: w.map.width as f32 / 2. + 0.5,
        y: w.map.height as f32 / 2. + 0.5,
    };
    let p = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: pos.x + 6.,
        ..pos
    };
    (w, pos, p)
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
fn members(w: &World, root: i32, kind: u16) -> Vec<i32> {
    let tag = w.entities[&root].quest.unwrap();
    w.entities
        .values()
        .filter(|e| e.quest == Some(tag) && e.kind == kind)
        .map(|e| e.id)
        .collect()
}
#[test]
fn crystal_healing_respects_stun_sick_and_the_existing_damage_ledger() {
    let (mut w, pos, p) = arena();
    let root = w.spawn_entity(0x935, pos, vec![]).unwrap();
    let max = w.content.objects[&0x935].hp;
    let character = w.players[&p].character.id;
    let e = w.entities.get_mut(&root).unwrap();
    e.hp = max - 1000;
    e.stage = Some(3);
    e.activated = Some(0);
    e.damage.insert(character, 1000);
    w.apply_condition(root, condition::STUNNED, 500).unwrap();
    w.npc_step();
    assert_eq!(w.entities[&root].hp, max - 1000);
    assert_eq!(w.entities[&root].next_aux, 0);
    w.now = 500;
    w.apply_condition(root, condition::SICK, 1500).unwrap();
    w.npc_step();
    assert_eq!(w.entities[&root].hp, max - 1000);
    tick(&mut w, 2000);
    assert_eq!(w.entities[&root].hp, max - 1000);
    tick(&mut w, 5500);
    assert_eq!(w.entities[&root].hp, max);
    assert_eq!(w.entities[&root].damage[&character], 1000);
    assert_eq!(w.players[&p].experience_queued, 0);
}
#[test]
fn named_tosses_reject_blocked_admission_and_recheck_the_landing() {
    let (mut w, pos, _) = arena();
    let root = w.spawn_entity(0xd50, pos, vec![]).unwrap();
    w.npc_step();
    let at = Pos {
        x: pos.x + 4.,
        ..pos
    };
    let wall = w.content.objects.values().find(|o| o.blocks).unwrap().kind;
    let obstacle = w.spawn_entity(wall, at, vec![]).unwrap();
    assert_eq!(w.named_toss(root, 0xd53, 5, &[at]).unwrap(), 0);
    w.remove_entity(obstacle);
    assert_eq!(w.named_toss(root, 0xd53, 5, &[at]).unwrap(), 1);
    w.spawn_entity(wall, at, vec![]).unwrap();
    w.now = 1500;
    w.mechanic_expiry();
    assert_eq!(w.pending_named(root, None), 0);
    assert!(members(&w, root, 0xd53).is_empty());
}
fn tick(w: &mut World, now: u64) {
    w.now = now;
    w.mechanic_expiry();
    w.npc_step();
}
#[test]
fn lord_protection_uses_landed_owned_crystals_not_a_foreign_encounter() {
    let (mut w, pos, p) = arena();
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: pos.x + 4.,
        ..pos
    };
    let root = w.spawn_entity(0xd50, pos, vec![]).unwrap();
    w.entities.get_mut(&root).unwrap().hp = 69299;
    for t in (0..=4600).step_by(50) {
        tick(&mut w, t);
    }
    assert_eq!(members(&w, root, 0xd53).len(), 5);
    assert_eq!(members(&w, root, 0xd52).len(), 10);
    assert!(
        members(&w, root, 0xd52).iter().all(|id| w.entities[id]
            .script
            .as_ref()
            .unwrap()
            .no_experience)
    );
    let knight = members(&w, root, 0xd52)[0];
    let xp = w.players[&p].experience_queued;
    w.defeat_entity(knight).unwrap();
    assert_eq!(w.players[&p].experience_queued, xp);
    tick(&mut w, 13200);
    assert_eq!(w.entities[&root].stage, Some(5));
    tick(&mut w, 15200);
    assert_eq!(w.entities[&root].stage, Some(6));
    tick(&mut w, 20200);
    assert_eq!(w.entities[&root].stage, Some(7));
    assert_eq!(w.pending_named(root, Some(0xd51)), 8);
    assert!(members(&w, root, 0xd51).is_empty());
    tick(&mut w, 21700);
    assert_eq!(w.entities[&root].stage, Some(8));
    assert_eq!(members(&w, root, 0xd51).len(), 8);
    assert_ne!(
        w.entities[&root].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    let other = w
        .spawn_entity(
            0xd50,
            Pos {
                x: pos.x + 7.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&other).unwrap().quest = Some(other);
    let foreign = w.spawn_entity(0xd51, pos, vec![]).unwrap();
    w.entities.get_mut(&foreign).unwrap().quest = Some(other);
    for id in members(&w, root, 0xd51) {
        w.remove_entity(id);
    }
    tick(&mut w, 21750);
    assert_eq!(w.entities[&root].stage, Some(1));
    assert!(w.entities.contains_key(&foreign));
    assert_eq!(
        w.entities[&root].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    assert!(w.critical_deaths.is_empty());
}
#[test]
fn lord_terminal_volley_and_recorded_contribution_reward_happen_only_once() {
    let (mut w, pos, p) = arena();
    let root = w.spawn_entity(0xd50, pos, vec![]).unwrap();
    let character = w.players[&p].character.id;
    w.entities.get_mut(&root).unwrap().hp = 6999;
    w.entities
        .get_mut(&root)
        .unwrap()
        .damage
        .insert(character, 63001);
    w.apply_condition(root, condition::STUNNED, 10000).unwrap();
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(9));
    w.now = 2000;
    assert_eq!(shots(&mut w, root).len(), 8);
    assert!(shots(&mut w, root).is_empty());
    w.now = 2050;
    w.mechanic_expiry();
    let xp = w.players[&p].experience_queued;
    assert!(xp > 0);
    assert_eq!(w.players[&p].character.kills, 1);
    w.mechanic_expiry();
    assert_eq!(w.players[&p].experience_queued, xp);
    assert_eq!(w.players[&p].character.kills, 1);
    assert!(w.critical_deaths.is_empty());
}
#[test]
fn named_tosses_land_after_owner_death_and_reserve_both_shared_admission_paths() {
    let (mut w, pos, _) = arena();
    let root = w.spawn_entity(0xd50, pos, vec![]).unwrap();
    w.npc_step();
    let mut fillers = vec![];
    for _ in 0..254 {
        let id = w.spawn_entity(0x65a, pos, vec![]).unwrap();
        w.entities.get_mut(&id).unwrap().quest = Some(id);
        fillers.push(id);
    }
    assert_eq!(
        w.named_toss(
            root,
            0xd53,
            5,
            &[Pos {
                x: pos.x + 4.,
                ..pos
            }]
        )
        .unwrap(),
        1
    );
    assert!(w.spawn_children(root, 0x904, 1, 1.).is_err());
    assert!(w.quest_birth((root, pos, pos), 0xd52, 10, 1, 1.).is_err());
    for id in fillers {
        w.remove_entity(id);
    }
    w.remove_entity(root);
    w.now = 1499;
    w.mechanic_expiry();
    assert!(!w.entities.values().any(|e| e.kind == 0xd53));
    w.now = 1500;
    w.mechanic_expiry();
    let guardian = w.entities.values().find(|e| e.kind == 0xd53).unwrap();
    assert_eq!(guardian.quest, Some(root));
    assert_eq!(w.pending_named(root, None), 0);
}
#[test]
fn ship_requires_an_anchor_and_advances_through_damage_gated_broadsides_and_clouds() {
    let (mut w, pos, _) = arena();
    let root = w.spawn_entity(0xe37, pos, vec![]).unwrap();
    assert!(
        w.landmark_mechanics(
            root,
            Some(Pos {
                x: pos.x + 6.,
                ..pos
            }),
            0
        )
        .unwrap_err()
        .to_string()
        .contains("anchor")
    );
    w.spawn_entity(0xe38, pos, vec![]).unwrap();
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(0));
    w.entities.get_mut(&root).unwrap().hp = 48999;
    w.npc_step();
    tick(&mut w, 3000);
    w.now = 3300;
    assert_eq!(shots(&mut w, root).len(), 4);
    assert_eq!(w.pending_named(root, Some(0xe4f)), 1);
    tick(&mut w, 7300);
    let spirit = members(&w, root, 0xe39);
    assert_eq!(spirit.len(), 1);
    assert!(
        w.entities[&spirit[0]]
            .script
            .as_ref()
            .unwrap()
            .no_experience
    );
    w.entities.get_mut(&root).unwrap().hp = 44999;
    tick(&mut w, 7350);
    w.now = 9350;
    assert_eq!(shots(&mut w, root).len(), 6);
    tick(&mut w, 10350);
    assert_eq!(
        w.entities[&root].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.entities.get_mut(&root).unwrap().hp = 34999;
    tick(&mut w, 10400);
    assert_eq!(w.entities[&root].stage, Some(6));
    tick(&mut w, 12400);
    w.entities.get_mut(&root).unwrap().hp = 24999;
    tick(&mut w, 12450);
    assert_eq!(w.entities[&root].stage, Some(7));
    tick(&mut w, 14450);
    w.entities.get_mut(&root).unwrap().hp = 19999;
    tick(&mut w, 14500);
    assert_eq!(w.entities[&root].stage, Some(8));
    assert_eq!(w.pending_named(root, Some(0xe3b)), 10);
    tick(&mut w, 16000);
    assert_eq!(w.entities[&root].stage, Some(9));
    assert_eq!(members(&w, root, 0xe3b).len(), 10);
    tick(&mut w, 18000);
    w.entities.get_mut(&root).unwrap().hp = 9999;
    tick(&mut w, 18050);
    assert_eq!(w.entities[&root].stage, Some(10));
    w.now = 18150;
    assert_eq!(shots(&mut w, root).len(), 7);
    assert_eq!(w.entities[&root].stage, Some(11));
    assert!(
        w.entities
            .values()
            .filter(|e| e.quest == Some(root))
            .count()
            + w.pending_named(root, None)
            <= 32
    );
    assert!(w.critical_deaths.is_empty());
}
#[test]
fn admitted_mine_bursts_after_ship_death_without_fabricating_rewards_or_an_unselected_portal() {
    let (mut w, pos, p) = arena();
    let root = w.spawn_entity(0xe37, pos, vec![]).unwrap();
    w.spawn_entity(0xe38, pos, vec![]).unwrap();
    w.npc_step();
    w.named_toss(
        root,
        0xe4f,
        5,
        &[Pos {
            x: pos.x + 4.,
            ..pos
        }],
    )
    .unwrap();
    w.remove_entity(root);
    w.now = 1500;
    w.mechanic_expiry();
    let mine = w.entities.values().find(|e| e.kind == 0xe4f).unwrap().id;
    assert_eq!(shots(&mut w, mine).len(), 10);
    assert!(shots(&mut w, mine).is_empty());
    w.now = 1550;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&mine));
    assert_eq!(w.players[&p].experience_queued, 0);
    assert!(w.portal_requests.is_empty());
}
#[test]
fn landmark_models_only_reference_selected_projectiles_across_every_phase_and_boundary() {
    let (w, _, _) = arena();
    for &kind in landmarks::ROSTER {
        crate::encounter::validate(&w.content, kind).unwrap();
        crate::loot::validate(&w.content, kind).unwrap();
        let phases = match kind {
            0x935 => 6,
            0x941 => 7,
            0xd50 => 10,
            0xe37 => 11,
            _ => 0,
        };
        for stage in 0..=phases {
            for age in [
                0, 100, 200, 800, 1000, 2600, 3100, 8100, 10600, 13100, 22600, 32600, 33600,
            ] {
                let s = landmarks::schedule(kind, w.content.objects[&kind].hp, stage, 0, age);
                for a in &s.attacks {
                    if let Action::Shoot { projectile, .. } = a.action {
                        assert!(
                            w.content.objects[&kind]
                                .projectiles
                                .contains_key(&projectile)
                        );
                    }
                    assert!((1..=255).contains(&a.count));
                    assert!(a.gap.is_finite() && a.offset.is_finite());
                }
                assert!(s.snapshot(0.).iter().all(|(_, a)| a.is_finite()));
            }
        }
    }
}
#[test]
fn oasis_children_protect_only_their_own_giant_and_reproduction_stays_bounded() {
    let (mut w, pos, _) = arena();
    let a = w.spawn_entity(0x678, pos, vec![]).unwrap();
    let b = w
        .spawn_entity(
            0x678,
            Pos {
                x: pos.x + 7.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.npc_step();
    for root in [a, b] {
        for kind in 0x679..=0x67c {
            assert!((1..=3).contains(&members(&w, root, kind).len()));
        }
    }
    for t in (2000..60000).step_by(2000) {
        w.now = t;
        w.npc_step();
    }
    for root in [a, b] {
        assert!(
            w.entities
                .values()
                .filter(|e| e.quest == Some(root))
                .count()
                <= 13
        );
    }
    let child = members(&w, a, 0x679)[0];
    let childpos = w.entities[&child].pos;
    w.move_entity(
        a,
        Pos {
            y: childpos.y + 14.,
            ..childpos
        },
        30.,
    )
    .unwrap();
    w.move_entity(b, childpos, 30.).unwrap();
    assert!(matches!(
        w.landmark_movement(child, None, 0).unwrap().0,
        Motion::Protect {
            anchor: Anchor::QuestMember(0x678),
            ..
        }
    ));
    assert_eq!(
        w.orbit_anchor(child, Anchor::QuestMember(0x678), None)
            .unwrap()
            .y,
        w.entities[&a].pos.y
    );
    assert!(w.critical_deaths.is_empty());
}
#[test]
fn deathmage_summons_keep_each_skeleton_role_without_unbounded_repeat_waves() {
    let (mut w, pos, _) = arena();
    let root = w.spawn_entity(0x925, pos, vec![]).unwrap();
    w.npc_step();
    for (kind, min, max) in [
        (0x65a, 3, 5),
        (0x65b, 2, 4),
        (0x65c, 1, 3),
        (0x65d, 1, 2),
        (0x65e, 1, 1),
    ] {
        assert!((min..=max).contains(&members(&w, root, kind).len()));
    }
    for t in (10000..=100000).step_by(10000) {
        w.now = t;
        w.npc_step();
        assert!(
            w.entities
                .values()
                .filter(|e| e.quest == Some(root))
                .count()
                <= 16
        );
    }
}
#[test]
fn crystal_evaluation_heals_then_breaks_without_awarding_a_fake_death_or_losing_damage() {
    let (mut w, pos, p) = arena();
    let root = w.spawn_entity(0x935, pos, vec![]).unwrap();
    let max = w.content.objects[&0x935].hp;
    let character = w.players[&p].character.id;
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(1));
    w.entities.get_mut(&root).unwrap().hp = max - 101;
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(2));
    w.entities.get_mut(&root).unwrap().hp = max - 2101;
    w.entities
        .get_mut(&root)
        .unwrap()
        .damage
        .insert(character, 2101);
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(3));
    assert_eq!(w.entities[&root].hp, max);
    w.entities.get_mut(&root).unwrap().hp = max - 50001;
    w.entities
        .get_mut(&root)
        .unwrap()
        .damage
        .insert(character, 52102);
    w.now = 50;
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(4));
    assert_ne!(
        w.entities[&root].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now = 4050;
    assert_eq!(shots(&mut w, root).len(), 16);
    assert!(shots(&mut w, root).is_empty());
    w.now = 4100;
    w.npc_step();
    assert!(!w.entities.contains_key(&root));
    let boss = w.entities.values().find(|e| e.kind == 0x941).unwrap();
    assert_eq!(boss.damage[&character], 52102);
    assert_eq!(boss.quest, Some(root));
    assert_eq!(boss.hp, w.content.objects[&0x941].hp);
    assert_eq!(w.players[&p].experience_queued, 0);
    assert_eq!(w.players[&p].character.kills, 0);
    assert!(w.critical_deaths.is_empty());
}
#[test]
fn prisoner_uses_sparse_projectile_ids_and_does_not_reproduce_killed_initial_steeds() {
    let (mut w, pos, _) = arena();
    let root = w.spawn_entity(0x941, pos, vec![]).unwrap();
    w.npc_step();
    for t in [200, 400, 600] {
        w.now = t;
        w.npc_step();
    }
    let steeds = members(&w, root, 0x943);
    assert_eq!(steeds.len(), 3);
    for id in steeds {
        w.remove_entity(id);
    }
    w.now = 3500;
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(1));
    w.now = 4100;
    let volley = shots(&mut w, root);
    assert_eq!(volley.iter().filter(|i| **i == 0).count(), 4);
    assert_eq!(volley.iter().filter(|i| **i == 5).count(), 1);
    assert!(members(&w, root, 0x943).is_empty());
    w.now = 21500;
    w.npc_step();
    assert_eq!(w.entities[&root].stage, Some(2));
    w.now = 24500;
    w.npc_step();
    assert!(members(&w, root, 0x942).is_empty());
    assert_eq!(w.pending_named(root, Some(0x942)), 4);
    for t in [24700, 24900, 25100, 25300] {
        w.now = t;
        w.npc_step();
    }
    w.now = 26000;
    w.mechanic_expiry();
    let clones = members(&w, root, 0x942);
    assert_eq!(clones.len(), 8);
    assert_eq!(
        clones
            .iter()
            .filter(|id| w.entities[id]
                .script
                .as_ref()
                .is_some_and(|s| s.no_experience))
            .count(),
        4
    );
    assert!(clones.iter().all(|id| {
        w.entities[id]
            .expires
            .is_some_and(|t| (41700..=43000).contains(&t))
    }));
}
#[test]
fn sphinx_keeps_boundary_volleys_shield_windows_and_scoped_reaper_cleanup() {
    let (mut w, pos, _) = arena();
    let a = w.spawn_entity(0xd54, pos, vec![]).unwrap();
    let b = w
        .spawn_entity(
            0xd54,
            Pos {
                y: pos.y + 8.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.npc_step();
    assert_eq!(members(&w, a, 0xd55).len(), 4);
    assert_eq!(members(&w, b, 0xd55).len(), 4);
    w.now = 2500;
    w.npc_step();
    assert_eq!(
        w.entities[&a].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now = 10600;
    w.npc_step();
    assert_eq!(
        landmarks::schedule(0xd54, w.entities[&a].hp, 0, 0, 10600)
            .snapshot(0.)
            .iter()
            .filter(|(i, _)| *i == 2)
            .count(),
        9
    );
    assert_ne!(
        w.entities[&a].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now = 22600;
    assert_eq!(shots(&mut w, a).iter().filter(|i| **i == 1).count(), 22);
    w.entities.get_mut(&a).unwrap().hp = w.content.objects[&0xd54].hp * 14 / 100;
    w.npc_step();
    let ours = members(&w, a, 0xd55);
    assert!(
        ours.iter()
            .all(|id| w.entities[id].expires == Some(w.now + 500))
    );
    assert!(
        members(&w, b, 0xd55)
            .iter()
            .all(|id| w.entities[id].expires.is_none())
    );
    w.now += 500;
    w.mechanic_expiry();
    assert!(ours.iter().all(|id| !w.entities.contains_key(id)));
    assert_eq!(members(&w, b, 0xd55).len(), 4);
}
