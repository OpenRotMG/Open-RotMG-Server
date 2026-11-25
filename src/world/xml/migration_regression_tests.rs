use super::*;
use crate::condition::{self, bit};

fn arena(size: usize) -> World {
    let mut map = flat_map(size);
    let center = size as f32 / 2. + 0.5;
    map.spawns = vec![(center, center)];
    World::in_domain(
        Arc::new(test_content()),
        Arc::new(map),
        WorldDomain::NonRealm,
    )
    .unwrap()
}

fn join(w: &mut World, id: i32, pos: Pos) -> i32 {
    let ch = Character {
        id,
        account: id,
        name: format!("Diver{id}"),
        ..Character::fresh(w.content.get(0x307).unwrap()).unwrap()
    };
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = pos;
    p
}

fn tick(w: &mut World, at: u64) {
    w.now = at;
    w.xml_step().unwrap();
    drain_events(w).unwrap();
}

fn state(w: &World, id: i32) -> &str {
    let index = *w.xml.actors[&id].path().last().unwrap();
    w.xml_game().definitions[&w.entities[&id].kind].states[index as usize]
        .name
        .as_str()
}

#[test]
fn selected_shaman_green_spinner_damages_and_blinds_once_after_ack() {
    let (mut w, p) = crate::world::movement_tests::fixture(0x18, false);
    let pos = w.players[&p].pos;
    let shaman = w
        .spawn_entity(
            0x6c6,
            Pos {
                x: pos.x + 3.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.clock.set_paused(true);
    let mut hit = None;
    for now in (0..=5300).step_by(50) {
        tick(&mut w, now);
        hit = w
            .bullets
            .iter()
            .find(|b| b.owner == shaman && !b.spec.effects.is_empty())
            .cloned();
        if hit.is_some() {
            break;
        }
        while !w.players[&p].shoot_acks.is_empty() {
            w.acknowledge_shoot(p, -1).unwrap();
        }
    }
    let b = hit.expect("the recovered fire2 state emits selected projectile 1");
    assert_eq!(
        (b.damage, b.spec.effects.as_slice()),
        (30, &[(8, 1500)][..])
    );
    w.native_player_hit(p, shaman, b.id).unwrap();
    assert_eq!(w.players[&p].character.hp, 200);
    assert!(w.players[&p].statuses.is_empty());
    w.acknowledge_shoot(p, 1000).unwrap();
    assert!(
        w.players[&p]
            .visible_projectiles
            .contains_key(&(shaman, b.id))
    );
    w.native_player_hit(p, shaman, b.id).unwrap();
    let hit_at = w.now;
    w.now += 50;
    w.native_player_hit(p, shaman, b.id).unwrap();
    let player = &w.players[&p];
    assert_eq!(player.character.hp, 170);
    assert_eq!(player.statuses[&8], (hit_at, hit_at + 1500));
    assert_ne!(player.conditions(hit_at + 1499) & bit(8), 0);
    assert_eq!(player.conditions(hit_at + 1500) & bit(8), 0);
}

#[test]
fn recovered_overlapping_vents_add_breath_cap_and_exclude_the_exact_edge() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let edge = join(
        &mut w,
        2,
        Pos {
            x: pos.x + 1.,
            ..pos
        },
    );
    for player in w.players.values_mut() {
        player.oxygen = 50;
    }
    for _ in 0..2 {
        w.spawn_entity(0x731, pos, vec![]).unwrap();
    }
    tick(&mut w, 0);
    tick(&mut w, 50);
    assert_eq!((w.players[&p].oxygen, w.players[&edge].oxygen), (55, 50));
    for now in (100..=600).step_by(50) {
        tick(&mut w, now);
    }
    assert_eq!((w.players[&p].oxygen, w.players[&edge].oxygen), (100, 50));
}

#[test]
fn wounded_thessal_only_accepts_the_recovered_answer_during_question_and_tosses_once() {
    const ANSWER: &str = " He LIVES   and reigns and conquers the world ";
    let pos = Pos { x: 40.5, y: 40.5 };
    let mut w = arena(80);
    let near = join(&mut w, 1, pos);
    let speaker = join(
        &mut w,
        2,
        Pos {
            x: pos.x + 31.,
            ..pos
        },
    );
    let id = w.spawn_entity(0x1704, pos, vec![]).unwrap();
    w.xml_chat(near, ANSWER).unwrap();
    for now in (0..=1300).step_by(50) {
        tick(&mut w, now);
    }
    assert_ne!(state(&w, id), "Wait");
    w.xml_chat(near, "He lives and reigns").unwrap();
    w.xml_chat(speaker, ANSWER).unwrap();
    tick(&mut w, 1350);
    assert_ne!(state(&w, id), "Wait");
    w.players.get_mut(&speaker).unwrap().pos.x -= 6.;
    w.xml_chat(speaker, ANSWER).unwrap();
    w.players.get_mut(&speaker).unwrap().death_pending = true;
    tick(&mut w, 1400);
    assert_ne!(state(&w, id), "Wait");
    w.players.get_mut(&speaker).unwrap().death_pending = false;
    w.xml_chat(speaker, ANSWER).unwrap();
    tick(&mut w, 1450);
    assert_eq!(state(&w, id), "Wait");
    let mut gifts = BTreeSet::new();
    for now in (1500u64..=6500).step_by(50) {
        if now.is_multiple_of(500) {
            w.xml_chat(speaker, ANSWER).unwrap();
        }
        tick(&mut w, now);
        gifts.extend(
            w.actors()
                .filter(|e| e.kind == 0x1705 && e.parent == Some(id))
                .map(|e| e.id),
        );
    }
    assert_eq!(gifts.len(), 3);
    assert!(!w.entities.contains_key(&id));
}

#[test]
fn bonegrind_commits_two_private_rewards_once_after_permanent_flee() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let _outsider = join(&mut w, 2, pos);
    let boss = w
        .spawn_entity(
            0x926,
            Pos {
                x: pos.x + 4.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    tick(&mut w, 0);
    assert!(w.bullets.iter().any(|b| b.owner == boss));
    let hp = w.entities[&boss].hp;
    w.damage_enemy(p, boss, hp * 3 / 4, true, &[], None)
        .unwrap();
    tick(&mut w, 50);
    assert_eq!(state(&w, boss), "Flee");
    w.entities.get_mut(&boss).unwrap().hp = hp;
    for now in (100..=11_000).step_by(50) {
        tick(&mut w, now);
    }
    assert_eq!(state(&w, boss), "Flee");
    w.damage_enemy(p, boss, hp * 2, true, &[], None).unwrap();
    assert!(w.bag_expiry.is_empty());
    tick(&mut w, 11_050);
    let bags = |w: &World| {
        w.actors()
            .filter(|e| w.bag_expiry.contains_key(&e.id))
            .map(|e| (e.id, e.accessible(1) && !e.accessible(2), e.items.clone()))
            .collect::<Vec<_>>()
    };
    let first = bags(&w);
    assert!(!first.is_empty() && first.iter().all(|(_, private, _)| *private));
    let items: BTreeSet<_> = first
        .iter()
        .flat_map(|(_, _, items)| items.iter())
        .filter(|&&id| id >= 0)
        .map(|&id| w.content.objects[&(id as u16)].name.as_str())
        .collect();
    assert_eq!(items, BTreeSet::from(["Health Potion", "Magic Potion"]));
    assert_eq!(
        first
            .iter()
            .flat_map(|(_, _, items)| items)
            .filter(|&&id| id >= 0)
            .count(),
        2
    );
    assert_eq!(w.players[&p].character.kills, 1);
    for now in (11_100..=12_000).step_by(50) {
        tick(&mut w, now);
    }
    assert_eq!(bags(&w), first);
    assert_eq!(w.players[&p].character.kills, 1);
}

#[test]
fn actual_crystal_clone_decays_at_seventeen_seconds_without_rewards() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let clone = w.spawn_entity(0x942, pos, vec![]).unwrap();
    for now in (0..17_000).step_by(50) {
        tick(&mut w, now);
    }
    assert!(w.entities.contains_key(&clone));
    tick(&mut w, 17_000);
    assert!(!w.entities.contains_key(&clone));
    assert!(w.bag_expiry.is_empty());
    assert_eq!(w.players[&p].character.kills, 0);
}

#[tokio::test]
async fn arachna_clear_receipt_replays_after_database_reopen_without_extra_exit_or_progress() {
    let mut w = arena(64);
    w.xml.template = Some("spider-den".into());
    w.encounter = true;
    let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join("arachna-clear-replay.db");
    let store = crate::store::Store::open(&path, &w.content).unwrap();
    let account = store.list("arachna-clear-replay".into()).await.unwrap();
    let ch = store
        .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let p = w.enter(ch.clone(), None).unwrap();
    let boss = w.spawn_entity(0x936, w.players[&p].pos, vec![]).unwrap();
    w.damage_enemy(p, boss, 100_000, true, &[], None).unwrap();
    let event = w.pending_terminal_events().pop().unwrap();
    assert!(!w.cleared && w.clear_exit.is_none());
    let receipt = store.coordinate_terminal(event.clone()).await.unwrap();
    drop(store);
    let store = crate::store::Store::open(&path, &w.content).unwrap();
    assert_eq!(
        store.load(account.id, ch.id).await.unwrap().spider_dens,
        ch.spider_dens + 1
    );
    w.finish_terminal_event(receipt).unwrap();
    let portal = w
        .actors()
        .find(|e| e.kind == 0x704 && e.published)
        .unwrap()
        .id;
    store.publish_world_event(event.clone()).await.unwrap();
    w.acknowledge_terminal_publication(event.id.clone());
    let replay = store.coordinate_terminal(event).await.unwrap();
    w.finish_terminal_event(replay).unwrap();
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == 0x704)
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        [portal]
    );
    assert!(w.cleared && !w.entities.contains_key(&boss));
    assert_eq!(w.players[&p].character.spider_dens, ch.spider_dens + 1);
    assert_eq!(
        store.load(account.id, ch.id).await.unwrap().spider_dens,
        ch.spider_dens + 1
    );
}

#[test]
fn davy_lights_survive_player_turnover_and_enter_done_on_boss_death() {
    use crate::dungeon::davy::{BOSS, LIT, UNLIT};
    let mut w = arena(80);
    let pos = Pos { x: 40.5, y: 40.5 };
    let owner = join(&mut w, 1, pos);
    let fallen = join(&mut w, 2, pos);
    let boss = w.spawn_entity(BOSS, pos, vec![]).unwrap();
    let lamps: Vec<_> = [(-4., 0.), (4., 0.), (0., -4.), (0., 4.)]
        .into_iter()
        .map(|(x, y)| {
            w.spawn_entity(
                UNLIT,
                Pos {
                    x: pos.x + x,
                    y: pos.y + y,
                },
                vec![],
            )
            .unwrap()
        })
        .collect();
    for now in (0..=1800).step_by(50) {
        tick(&mut w, now);
    }
    w.players.get_mut(&owner).unwrap().pos = Pos {
        x: pos.x + 9.,
        ..pos
    };
    for (i, lamp) in lamps.into_iter().enumerate() {
        let start = 1850 + i as u64 * 500;
        w.damage_enemy(owner, lamp, 100_000, true, &[], None)
            .unwrap();
        for now in (start..start + 500).step_by(50) {
            tick(&mut w, now);
        }
        assert_eq!(w.actors().filter(|e| e.kind == LIT).count(), i + 1);
        if i < 3 {
            assert_ne!(
                w.entities[&boss].conditions(w.now) & bit(condition::INVULNERABLE),
                0
            );
        }
    }
    assert_eq!(
        w.entities[&boss].conditions(w.now) & bit(condition::INVULNERABLE),
        0
    );
    assert!(w.entities[&boss].pos.distance(w.players[&owner].pos) > 8.);
    w.players.get_mut(&fallen).unwrap().death_pending = true;
    w.leave(owner).unwrap();
    tick(&mut w, 3850);
    let replacement = join(&mut w, 3, pos);
    w.damage_enemy(replacement, boss, 100_000, true, &[], None)
        .unwrap();
    for now in (3900..=20_000).step_by(50) {
        tick(&mut w, now);
    }
    assert!(!w.entities.contains_key(&boss));
    assert_eq!(w.actors().filter(|e| e.kind == UNLIT).count(), 0);
    let lit: Vec<_> = w.actors().filter(|e| e.kind == LIT).collect();
    assert_eq!(lit.len(), 4);
    assert!(lit.iter().all(|e| state(&w, e.id) == "Done"));
}

#[test]
fn journaled_enforcer_and_jar_births_replace_only_their_own_actors() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let mut mini_counts = BTreeSet::new();
    let mut scarab_counts = BTreeSet::new();
    for i in 0..24 {
        w.rng = rand::rngs::StdRng::seed_from_u64(i);
        for (kind, child, counts) in [
            (0x982, 0x979, &mut mini_counts),
            (0xd2d, 0xd1b, &mut scarab_counts),
        ] {
            let before: BTreeSet<_> = w
                .actors()
                .filter(|e| e.kind == child)
                .map(|e| e.id)
                .collect();
            let id = w.spawn_entity(kind, pos, vec![]).unwrap();
            w.damage_enemy(p, id, 100_000, true, &[], None).unwrap();
            assert_eq!(w.actors().filter(|e| e.kind == child).count(), before.len());
            drain_events(&mut w).unwrap();
            let new: Vec<_> = w
                .actors()
                .filter(|e| e.kind == child && !before.contains(&e.id))
                .collect();
            assert!(!w.entities.contains_key(&id));
            counts.insert(new.len());
            assert!(
                !w.cleared
                    && w.players[&p].character.tombs == 0
                    && w.players[&p].character.mad_labs == 0
            );
        }
    }
    assert_eq!(mini_counts, BTreeSet::from([1, 2, 3]));
    assert_eq!(scarab_counts, BTreeSet::from([0, 1]));
}

#[test]
fn sand_phantom_xml_transform_round_trip_preserves_the_same_damage_ledger() {
    let (mut w, p) = crate::world::movement_tests::fixture(0x18, false);
    let pos = w.players[&p].pos;
    let phantom = w
        .spawn_entity(
            0x6c5,
            Pos {
                x: pos.x + 2.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.damage_enemy(p, phantom, 19, true, &[], None).unwrap();
    for now in (0..=2500).step_by(50) {
        tick(&mut w, now);
    }
    let wisp = w.actors().find(|e| e.kind == 0x7f22).unwrap().id;
    assert!(!w.entities.contains_key(&phantom));
    assert_eq!(w.entities[&wisp].damage, BTreeMap::from([(1, 19)]));
    for now in (2550..=8900).step_by(50) {
        tick(&mut w, now);
    }
    let returned = w.actors().find(|e| e.kind == 0x6c5).unwrap();
    assert_ne!(returned.id, phantom);
    assert_eq!(returned.damage, BTreeMap::from([(1, 19)]));
    assert!(!w.entities.contains_key(&wisp));
    assert_eq!(w.players[&p].character.kills, 0);
    assert!(w.bag_expiry.is_empty());
}

#[test]
fn recovered_ent_private_tincture_only_goes_to_its_damager() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let outsider = join(&mut w, 2, pos);
    let mut outcomes = BTreeSet::new();
    for seed in 0..64 {
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        let ent = w.spawn_entity(0x921, pos, vec![]).unwrap();
        w.damage_enemy(p, ent, 100_000, true, &[], None).unwrap();
        let event = w.pending_terminal_events().pop().unwrap();
        let payload = event.terminal_payload().unwrap();
        outcomes.insert(payload.allocations().len());
        drain_events(&mut w).unwrap();
        for bag in w.actors().filter(|e| w.bag_expiry.contains_key(&e.id)) {
            assert!(bag.accessible(1) && !bag.accessible(2));
            assert_eq!(
                bag.items
                    .iter()
                    .filter(|&&item| item >= 0)
                    .copied()
                    .collect::<Vec<_>>(),
                [0xb0c]
            );
        }
    }
    assert_eq!(outcomes, BTreeSet::from([0, 1]));
    assert_eq!(w.players[&outsider].character.kills, 0);
}

#[test]
fn masked_party_god_only_credits_one_clear_and_private_cocktails_to_the_contributor() {
    let mut w = arena(64);
    w.name = "Beachzone".into();
    w.xml.template = Some("beachzone".into());
    w.encounter = true;
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let outside = join(&mut w, 2, pos);
    let boss = w.spawn_entity(0xe56, pos, vec![]).unwrap();
    tick(&mut w, 0);
    // Direct lethal damage isolates the terminal contract from the recovered repeat self-heal.
    w.damage_enemy(p, boss, 100_000, true, &[], None).unwrap();
    assert!(!w.cleared && w.bag_expiry.is_empty());
    for now in (50..=300).step_by(50) {
        tick(&mut w, now);
    }
    assert!(w.cleared && !w.entities.contains_key(&boss));
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == 0x704 && e.published)
            .count(),
        1
    );
    assert_eq!(
        (
            w.players[&p].character.beaches,
            w.players[&outside].character.beaches
        ),
        (1, 0)
    );
    let bags: Vec<_> = w
        .actors()
        .filter(|e| w.bag_expiry.contains_key(&e.id))
        .collect();
    assert!(bags.iter().all(|e| e.accessible(1) && !e.accessible(2)));
    let drinks: BTreeSet<_> = bags
        .iter()
        .flat_map(|e| &e.items)
        .filter(|&&k| k >= 0)
        .map(|&k| w.content.objects[&(k as u16)].name.as_str())
        .collect();
    assert_eq!(
        drinks,
        BTreeSet::from([
            "Bahama Sunrise",
            "Blue Paradise",
            "Lime Jungle Bay",
            "Pink Passion Breeze"
        ])
    );
}

#[test]
fn lab_aura_cure_is_player_local_and_does_not_stick_on_exit() {
    let mut w = arena(64);
    let green = Pos { x: 32.5, y: 32.5 };
    let blue = Pos {
        x: green.x + 8.,
        ..green
    };
    let treated = join(&mut w, 1, blue);
    let witness = join(&mut w, 2, green);
    w.spawn_entity(0x1e15, green, vec![]).unwrap();
    w.spawn_entity(0x1e16, blue, vec![]).unwrap();
    for p in [treated, witness] {
        w.apply_condition(p, condition::HEXED, 1000).unwrap();
    }
    tick(&mut w, 0);
    let hexed = |w: &World, p: i32| w.players[&p].conditions(w.now) & bit(condition::HEXED) != 0;
    assert!(!hexed(&w, treated) && hexed(&w, witness));
    {
        let player = w.players.get_mut(&treated).unwrap();
        player.pos = green;
        player.paused = true;
    }
    tick(&mut w, 200);
    assert!(!hexed(&w, treated) && hexed(&w, witness));
    w.players.get_mut(&treated).unwrap().paused = false;
    tick(&mut w, 400);
    assert!(hexed(&w, treated));
    w.leave(treated).unwrap();
    assert!(w.xml.auras.keys().all(|(_, target, _)| *target != treated));
    let reentry = join(&mut w, 1, blue);
    assert!(!hexed(&w, reentry) && hexed(&w, witness));
}

#[test]
fn deathmage_full_escort_caps_do_not_stop_fire_or_mix_owners() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let roots = [-4., 4.].map(|dx| {
        w.spawn_entity(
            0x925,
            Pos {
                x: pos.x + dx,
                ..pos
            },
            vec![],
        )
        .unwrap()
    });
    let owned = |w: &World, root| {
        w.actors()
            .filter(|e| e.parent == Some(root))
            .map(|e| e.id)
            .collect::<BTreeSet<_>>()
    };
    for now in (0..=250).step_by(50) {
        tick(&mut w, now);
    }
    let [a, b] = roots.map(|root| owned(&w, root));
    assert_eq!((a.len(), b.len()), (8, 8));
    assert!(a.is_disjoint(&b));
    for now in (300..=8350).step_by(50) {
        tick(&mut w, now);
    }
    for root in roots {
        assert!(w.bullets.iter().any(|s| s.owner == root && s.born > 8000));
    }
    assert_eq!(roots.map(|root| owned(&w, root)), [a.clone(), b.clone()]);
    w.damage_enemy(p, roots[0], 100_000, true, &[], None)
        .unwrap();
    for now in (8400..=9500).step_by(50) {
        tick(&mut w, now);
    }
    assert!(a.iter().all(|id| w.entities[id].parent != Some(roots[1])));
    assert_eq!(owned(&w, roots[1]), b);
}

#[test]
fn lab_children_can_die_after_terrible_without_recrediting_his_clear() {
    let mut w = arena(64);
    w.name = "Mad Lab".into();
    w.xml.template = Some("mad-lab".into());
    w.encounter = true;
    let pos = Pos { x: 32.5, y: 32.5 };
    let owner = join(&mut w, 1, pos);
    let witness = join(&mut w, 2, pos);
    let boss = w.spawn_entity(0x976, pos, vec![]).unwrap();
    let minion = w.spawn_entity(0x1e25, pos, vec![]).unwrap();
    w.damage_enemy(owner, boss, 100_000, true, &[], None)
        .unwrap();
    drain_events(&mut w).unwrap();
    assert!(w.cleared);
    assert_eq!(
        (
            w.players[&owner].character.mad_labs,
            w.players[&witness].character.mad_labs
        ),
        (1, 0)
    );
    w.damage_enemy(owner, minion, 100_000, true, &[], None)
        .unwrap();
    drain_events(&mut w).unwrap();
    assert!(!w.entities.contains_key(&minion));
    assert_eq!(
        (
            w.players[&owner].character.mad_labs,
            w.players[&witness].character.mad_labs
        ),
        (1, 0)
    );
    assert!(
        w.actors()
            .filter(|e| w.bag_expiry.contains_key(&e.id))
            .all(|e| e.accessible(1) && !e.accessible(2))
    );
}

#[test]
fn tomb_center_and_edge_turrets_follow_selected_stasis_flags() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    join(&mut w, 1, pos);
    let turrets = [0xd31, 0xd32, 0xd33, 0xd34, 0xd35, 0xd36]
        .map(|kind| w.spawn_entity(kind, pos, vec![]).unwrap());
    tick(&mut w, 0);
    for (i, turret) in turrets.into_iter().enumerate() {
        assert_eq!(w.entities[&turret].hp, 0);
        w.apply_condition(turret, condition::STASIS, 2000).unwrap();
        assert_eq!(
            w.entities[&turret].conditions(w.now) & bit(condition::STASIS) != 0,
            i < 3
        );
    }
    assert!(!w.cleared);
}

#[test]
fn vengeful_spirit_awards_no_xp_on_repeated_combat_deaths() {
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    let exp = w.players[&p].character.exp;
    for _ in 0..2 {
        let spirit = w.spawn_entity(0xe39, pos, vec![]).unwrap();
        w.damage_enemy(p, spirit, 100_000, true, &[], None).unwrap();
        drain_events(&mut w).unwrap();
        assert!(!w.entities.contains_key(&spirit));
        assert_eq!(w.players[&p].character.exp, exp);
    }
}

#[test]
fn nut_births_three_owned_artifacts_once_after_half_health() {
    use crate::dungeon::tomb::{ANCHOR, NUT};
    let mut w = arena(64);
    let pos = Pos { x: 32.5, y: 32.5 };
    let p = join(&mut w, 1, pos);
    w.spawn_entity(ANCHOR, pos, vec![]).unwrap();
    let nut = w
        .spawn_entity(
            NUT,
            Pos {
                x: pos.x + 4.,
                ..pos
            },
            vec![],
        )
        .unwrap();
    w.damage_enemy(p, nut, w.entities[&nut].hp * 3 / 5, true, &[], None)
        .unwrap();
    for now in (0..=19_000).step_by(50) {
        tick(&mut w, now);
    }
    let mut artifacts: Vec<_> = w
        .actors()
        .filter(|e| (0xd1c..=0xd1e).contains(&e.kind))
        .map(|e| (e.kind, *e.parent))
        .collect();
    artifacts.sort_unstable();
    assert_eq!(
        artifacts,
        [0xd1c, 0xd1d, 0xd1e].map(|kind| (kind, Some(nut)))
    );
    assert!(!w.cleared);
}

#[test]
fn generated_bookshelves_protect_only_ordinary_walls() {
    use crate::dungeon::manor::BOOKCASE;
    let content = Arc::new(test_content());
    for seed in [13166792894241567768, 1, 53] {
        let map = Arc::new(
            crate::dungeon::generate_selected(&content, "manor-of-the-immortals", seed).unwrap(),
        );
        let w = World::new(content.clone(), map).unwrap();
        let mut positions = BTreeSet::new();
        let mut counts = [0, 0];
        for e in w.entities.values().filter(|e| e.kind == BOOKCASE) {
            let secret = w.map.cell(e.pos.x, e.pos.y).unwrap().region == 250;
            assert!(positions.insert((e.pos.x as i32, e.pos.y as i32)));
            assert_eq!(
                e.conditions(w.now) & bit(condition::INVINCIBLE) == 0,
                secret
            );
            counts[usize::from(secret)] += 1;
        }
        assert!(counts.iter().all(|&n| n > 0), "seed {seed}: {counts:?}");
    }
}

#[test]
fn creation_room_blink_and_fog_follow_the_actual_open_wall_gate() {
    use crate::dungeon::lab::{BOSS, CREATION, EMERGENCY, OPEN_WALL, TESLA};
    use crate::world::sight::tests::{blink, refused};
    let mut d = admission_tests::isolated_directory(Arc::new(test_content()));
    let world = d
        .instance("mad-lab", None, Some(DungeonReturn::Random))
        .unwrap();
    let w = d.worlds.get_mut(&world).unwrap();
    let object = |w: &World, kind| w.actors().find(|e| e.kind == kind).unwrap().id;
    let opener = w.entities[&object(w, OPEN_WALL)].pos;
    let creation = object(w, CREATION);
    let at = w.entities[&creation].pos;
    let outside = Pos {
        y: opener.y + 3.,
        ..opener
    };
    let inside = Pos { y: at.y + 2., ..at };
    let (fixture, rogue) = crate::world::movement_tests::fixture(0x18, false);
    let p = w
        .enter(fixture.players[&rogue].character.clone(), None)
        .unwrap();
    w.players.get_mut(&p).unwrap().pos = outside;
    assert!(w.walkable(outside) && w.walkable(inside));
    let tile = (inside.x as i16, inside.y as i16);
    w.updates(p, true).unwrap();
    assert!(!w.players[&p].tiles.contains(&tile) && !w.players[&p].known.contains(&creation));
    refused(w, p, 0xa5a, inside);
    let remove: Vec<_> = w
        .actors()
        .filter(|e| [BOSS, TESLA].contains(&e.kind))
        .map(|e| e.id)
        .collect();
    for actor in remove {
        w.remove_entity(actor);
    }
    for now in (0..=3000).step_by(50) {
        tick(w, now);
    }
    assert!(!w.actors().any(|e| e.kind == EMERGENCY));
    w.updates(p, false).unwrap();
    assert!(w.players[&p].tiles.contains(&tile) && w.players[&p].known.contains(&creation));
    blink(w, p, 0xa5a, inside).unwrap();
    assert_eq!(w.players[&p].pos.distance(inside), 0.);
}
