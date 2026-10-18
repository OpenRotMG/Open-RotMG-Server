use super::{tests, *};
use std::path::Path;

fn arena() -> World {
    let mut w = tests::world();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut w.map).cells {
        cell.ground = 0x61;
    }
    w
}

#[test]
fn selected_projectiles_deliver_native_condition_ids_through_authoritative_collisions() {
    for (kind, projectile, condition) in
        [(0x625, 0, 4), (0x917, 1, 8), (0xd90, 2, 11), (0xd90, 1, 2)]
    {
        let mut w = arena();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let pos = w.players[&id].pos;
        let start = Pos {
            x: pos.x - 0.2,
            y: pos.y,
        };
        let owner = w.spawn_entity(kind, start, vec![]).unwrap();
        let spec = w.content.objects[&kind].projectiles[&projectile].clone();
        assert!(spec.effects.iter().any(|(index, _)| *index == condition));
        w.bullets.push(Bullet {
            owner,
            source_kind: kind,
            id: 0,
            friendly: false,
            start,
            pos: start,
            angle: 0.,
            born: 0,
            damage: spec.min,
            spec,
            hit: BTreeSet::new(),
        });
        w.step();
        assert!(w.players[&id].character.hp < 100);
        assert_ne!(
            w.players[&id].conditions(w.now) & crate::condition::bit(condition),
            0
        );
        if condition == crate::condition::QUIET {
            assert_eq!(w.players[&id].character.mp, 0);
        }
    }
}

#[test]
fn multihit_hits_distinct_targets_once_and_native_projectile_modes_match_client_math() {
    let mut w = arena();
    let id = w.enter(tests::ch(&w), None).unwrap();
    let pos = w.players[&id].pos;
    let a = w
        .spawn_entity(
            0x600,
            Pos {
                x: pos.x + 1.,
                y: pos.y,
            },
            vec![],
        )
        .unwrap();
    let b = w
        .spawn_entity(
            0x600,
            Pos {
                x: pos.x + 2.,
                y: pos.y,
            },
            vec![],
        )
        .unwrap();
    let before = w.entities[&a].hp;
    let mut spec = w.content.objects[&0xc02].projectiles[&0].clone();
    assert!(spec.multi_hit);
    spec.boomerang = true;
    w.bullets.push(Bullet {
        owner: id,
        source_kind: crate::content::WIZARD,
        id: 0,
        friendly: true,
        start: pos,
        pos,
        angle: 0.,
        born: 0,
        damage: 1,
        spec: spec.clone(),
        hit: BTreeSet::new(),
    });
    for _ in 0..(spec.lifetime as usize / 50 + 1) {
        w.step();
    }
    assert_eq!(w.entities[&a].hp, before - 1);
    assert_eq!(w.entities[&b].hp, before - 1);
    assert!(w.bullets.is_empty());
    let bulwark = &w.content.objects[&0xc04].projectiles[&0];
    assert!(bulwark.parametric);
    let (x, y) = bulwark.offset(0, 0., bulwark.lifetime / 4.);
    assert!((x + bulwark.magnitude).abs() < 0.0001 && y.abs() < 0.0001);
    assert!(spec.offset(0, 0., spec.lifetime).0.abs() < 0.0001);
    assert_eq!(
        crate::condition::damage(
            100,
            20,
            crate::condition::bit(crate::condition::ARMORED),
            false
        ),
        60
    );
    assert_eq!(
        crate::condition::damage(
            100,
            20,
            crate::condition::bit(crate::condition::INVULNERABLE),
            false
        ),
        0
    );
}

#[test]
fn inland_population_respects_xml_biomes_caps_and_validated_attack_contracts() {
    let d = Directory::load(Path::new("data"), tests::world().content, 16).unwrap();
    let w = &d.worlds[&-3];
    let alive = w
        .population
        .iter()
        .filter(|s| s.entity.is_some_and(|id| w.entities.contains_key(&id)))
        .count();
    assert!(alive > 250 && alive <= 344);
    for (terrain, roster) in progression::REALM_ROSTER {
        for kind in *roster {
            crate::encounter::validate(&w.content, *kind).unwrap();
        }
        let spawns: Vec<_> = w
            .population
            .iter()
            .filter(|s| {
                roster.contains(&s.kind) && s.entity.is_some_and(|id| w.entities.contains_key(&id))
            })
            .collect();
        assert!(!spawns.is_empty() && spawns.len() <= 32);
        for spawn in spawns {
            assert_eq!(w.content.objects[&spawn.kind].terrain, *terrain);
            assert_eq!(
                w.map.cell(spawn.pos.x, spawn.pos.y).unwrap().terrain,
                *terrain
            );
            assert!(w.walkable(spawn.pos));
        }
    }
}

#[test]
fn selected_dungeons_credit_character_identity_once_and_publish_exits_after_commit() {
    let mut d = Directory::load(Path::new("data"), tests::world().content, 16).unwrap();
    for (index, (_, template, _, boss)) in crate::dungeon::DUNGEONS.iter().enumerate().skip(1) {
        let world = d.instance(template, None).unwrap();
        let w = d.worlds.get_mut(&world).unwrap();
        let boss = w.entities.values().find(|e| e.kind == *boss).unwrap().id;
        let position = w.entities[&boss].pos;
        let mut first = tests::ch(w);
        first.account = 301;
        first.id = 401;
        let mut second = first.clone();
        second.account = 302;
        second.id = 402;
        second.exp = i32::MAX;
        let (a, b) = if index % 2 == 0 {
            let b = w.enter(second, None).unwrap();
            (w.enter(first, None).unwrap(), b)
        } else {
            (
                w.enter(first, None).unwrap(),
                w.enter(second, None).unwrap(),
            )
        };
        w.players.get_mut(&b).unwrap().experience_queued = 1;
        w.players.get_mut(&a).unwrap().pos = position;
        w.players.get_mut(&b).unwrap().pos = position;
        w.entities.get_mut(&boss).unwrap().damage.insert(401, 2000);
        let defeated = w.remove_entity(boss);
        w.drop_loot(&defeated).unwrap();
        let exit = w.clear_exit.unwrap();
        assert!(!w.entities[&exit].published);
        let plans = w.pending_progress();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].0, a);
        assert_eq!(w.players[&b].experience_queued, 1);
        assert!(w.begin_departure(b).is_err());
        assert_eq!(
            match defeated.kind {
                crate::dungeon::trench::THESSAL => plans[0].1.trenches,
                crate::dungeon::spider::BOSS => plans[0].1.spider_dens,
                crate::dungeon::abyss::MALPHAS => plans[0].1.abysses,
                _ => plans[0].1.dungeons[index - 1],
            },
            1
        );
        assert_eq!(w.players[&a].character.dungeons, [0; 3]);
        assert_eq!(w.players[&b].character.dungeons, [0; 3]);
        assert_eq!(w.players[&a].character.abysses, 0);
        assert_eq!(w.players[&b].character.abysses, 0);
        w.finish_progress(a, plans[0].1.clone()).unwrap();
        assert!(w.entities[&exit].published);
        assert_eq!(
            match defeated.kind {
                crate::dungeon::trench::THESSAL => w.players[&a].character.trenches,
                crate::dungeon::spider::BOSS => w.players[&a].character.spider_dens,
                crate::dungeon::abyss::MALPHAS => w.players[&a].character.abysses,
                _ => w.players[&a].character.dungeons[index - 1],
            },
            1
        );
        assert!(w.pending_progress().is_empty());
        assert!(w.drop_loot(&defeated).is_err());
    }
}

#[test]
fn selected_private_boss_rewards_reach_every_eligible_player_including_the_ninth() {
    let mut directory = Directory::load(Path::new("data"), tests::world().content, 16).unwrap();
    for (template, kind, potion) in [
        ("spider-den", crate::dungeon::spider::BOSS, 0x707),
        ("snake-pit", 0x917, 0xa21),
        ("undead-lair", 0xd90, 0xa35),
        ("abyss-of-demons", crate::dungeon::abyss::MALPHAS, 0xa34),
    ] {
        let scope = directory.instance(template, None).unwrap();
        let w = directory.worlds.get_mut(&scope).unwrap();
        let boss = w.entities.values().find(|e| e.kind == kind).unwrap().id;
        let position = w.entities[&boss].pos;
        let threshold = w.content.objects[&kind].hp / 10;
        let mut participants = Vec::new();
        for index in 0..10 {
            let mut character = tests::ch(w);
            character.account = 800 + index;
            character.id = 900 + index;
            let player = w.enter(character, None).unwrap();
            w.players.get_mut(&player).unwrap().pos = position;
            if index < 9 {
                w.entities
                    .get_mut(&boss)
                    .unwrap()
                    .damage
                    .insert(900 + index, threshold + 1);
                participants.push(player);
            }
        }
        let defeated = w.remove_entity(boss);
        w.drop_loot(&defeated).unwrap();
        for player in &participants {
            let account = w.players[player].character.account;
            let bags = w
                .entities
                .values()
                .filter(|e| e.extra_stats.contains(&(54, account)))
                .collect::<Vec<_>>();
            assert!(
                !bags.is_empty(),
                "eligible account {account} lost its private reward"
            );
            assert_eq!(
                bags.iter()
                    .flat_map(|bag| &bag.items)
                    .filter(|item| **item == potion)
                    .count(),
                1
            );
        }
        assert!(
            !w.entities
                .values()
                .any(|e| e.extra_stats.contains(&(54, 809)))
        );
        let plans = w.pending_progress();
        assert_eq!(plans.len(), 9);
        let exit = w.clear_exit.unwrap();
        assert!(!w.entities[&exit].published);
        for (player, snapshot) in plans {
            w.finish_progress(player, snapshot).unwrap();
        }
        assert!(w.entities[&exit].published);
        assert!(participants.iter().all(|player| {
            w.players[player].character.dungeons.iter().sum::<u64>()
                + w.players[player].character.abysses
                + w.players[player].character.spider_dens
                == 1
        }));
    }
}

#[test]
fn spell_rolls_xml_damage_and_publishes_the_authoritative_values_without_weapon_rng() {
    use rand::RngCore;
    let mut w = arena();
    let mut ch = tests::ch(&w);
    ch.items[1] = 0xb24;
    let (tx, mut rx) = mpsc::channel(64);
    let (lost, _) = watch::channel(false);
    let id = w.enter(ch, Some(Output { packets: tx, lost })).unwrap();
    while rx.try_recv().is_ok() {}
    w.now = 1000;
    let target = w.players[&id].pos;
    let slot = Slot {
        object: id,
        slot: 1,
        kind: 0xb24,
    };
    let native_seed = w.players[&id].seed;
    let spec = w.content.objects[&0xb24].projectiles[&0].clone();
    let mut expected_rng = w.rng.clone();
    let expected: Vec<_> = (0..20)
        .map(|_| expected_rng.gen_range(spec.min..=spec.max))
        .collect();
    w.use_item(id, slot, target).unwrap();
    assert_eq!(
        w.bullets.iter().map(|b| b.damage).collect::<Vec<_>>(),
        expected
    );
    assert!(expected.iter().any(|damage| *damage > spec.min));
    assert_eq!(w.players[&id].seed, native_seed);
    for bullet in &w.bullets {
        let packet = rx.try_recv().unwrap();
        assert_eq!(packet.id, 13);
        let mut r = crate::wire::Reader::new(&packet.body);
        assert_eq!(r.u8().unwrap(), bullet.id);
        assert_eq!(r.i32().unwrap(), id);
        assert_eq!(r.i16().unwrap(), 0xb24);
        assert_eq!(r.f32().unwrap(), target.x);
        assert_eq!(r.f32().unwrap(), target.y);
        assert_eq!(r.f32().unwrap(), bullet.angle);
        assert_eq!(r.i16().unwrap() as i32, bullet.damage);
        r.done().unwrap();
    }
    assert!(rx.try_recv().is_err());
    let mut before_rejected = w.rng.clone();
    assert!(w.use_item(id, slot, target).is_err());
    assert_eq!(w.rng.next_u64(), before_rejected.next_u64());
    assert_eq!(w.players[&id].seed, native_seed);
    assert_eq!(w.bullets.len(), 20);
}

#[test]
fn timed_conditions_expire_and_quiet_wastes_mana_potions_but_blocks_spells() {
    let mut w = tests::world();
    w.safe = true;
    let mut character = tests::ch(&w);
    character.items[4] = 0xa23;
    let id = w.enter(character, None).unwrap();
    w.apply_condition(id, crate::condition::QUIET, 1000)
        .unwrap();
    w.apply_condition(id, 8, 600).unwrap();
    assert_eq!(w.players[&id].character.mp, 0);
    let plan = w
        .use_item(
            id,
            Slot {
                object: id,
                slot: 4,
                kind: 0xa23,
            },
            w.players[&id].pos,
        )
        .unwrap()
        .unwrap();
    assert_eq!(w.players[&id].character.items[4], 0xa23);
    assert_eq!(w.players[&id].character.mp, 0);
    let revision = plan.revision + 1;
    w.finish_inventory(plan, Ok(revision)).unwrap();
    assert_eq!(w.players[&id].character.items[4], -1);
    assert_eq!(w.players[&id].character.mp, 0);
    let ability = w.players[&id].character.items[1];
    assert!(
        w.use_item(
            id,
            Slot {
                object: id,
                slot: 1,
                kind: ability
            },
            w.players[&id].pos
        )
        .is_err()
    );
    for _ in 0..19 {
        w.step();
    }
    assert_eq!(
        w.players[&id].conditions(w.now),
        crate::condition::bit(crate::condition::QUIET)
    );
    assert_eq!(w.players[&id].character.mp, 0);
    w.step();
    assert_eq!(w.players[&id].conditions(w.now), 0);
    for _ in 0..30 {
        w.step();
    }
    assert!(w.players[&id].character.mp > 0);
}

#[test]
fn conveyors_credit_real_server_time_not_client_clock_and_slowed_has_native_speed() {
    let mut w = tests::world();
    let id = w.enter(tests::ch(&w), None).unwrap();
    let pos = w.players[&id].pos;
    let cell = pos.y as usize * w.map.width as usize + pos.x as usize;
    Arc::make_mut(&mut w.map).cells[cell].ground = 0xa1;
    w.players.get_mut(&id).unwrap().last_move = 100;
    w.players.get_mut(&id).unwrap().movement_credit = 0.;
    w.now = 100;
    let moved = Pos {
        x: pos.x - 0.25,
        y: pos.y,
    };
    w.move_to(id, moved, 200).unwrap();
    assert!(
        w.move_to(
            id,
            Pos {
                x: moved.x - 2.5,
                y: moved.y
            },
            1200
        )
        .is_err()
    );
    w.apply_condition(id, crate::condition::SLOWED, 2000)
        .unwrap();
    w.now += 100;
    assert!(
        w.validate_move(
            id,
            Pos {
                x: moved.x + 0.9,
                y: moved.y
            },
            300
        )
        .is_err()
    );
    assert!((w.content.pushes[&0xa1].0 + 0.0025).abs() < 0.000001);
}

#[test]
fn native_quest_publishes_a_distant_eligible_target_before_its_marker_and_retargets() {
    let mut w = tests::world();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut w.map).cells {
        cell.ground = 0x61;
    }
    let mut ch = tests::ch(&w);
    ch.level = 3;
    let (tx, mut rx) = mpsc::channel(128);
    let (lost, _) = watch::channel(false);
    let first = w
        .spawn_entity(
            0x617,
            Pos {
                x: w.map.spawns[0].0,
                y: w.map.spawns[0].1 - 30.,
            },
            vec![],
        )
        .unwrap();
    let second = w
        .spawn_entity(
            0x617,
            Pos {
                x: w.map.spawns[0].0,
                y: w.map.spawns[0].1 - 60.,
            },
            vec![],
        )
        .unwrap();
    let id = w.enter(ch, Some(Output { packets: tx, lost })).unwrap();
    assert_eq!(w.players[&id].quest, Some(first));
    assert!(w.players[&id].known.contains(&first));
    assert!(!w.players[&id].known.contains(&second));
    let mut messages = Vec::new();
    while let Ok(p) = rx.try_recv() {
        messages.push(p);
    }
    let update = messages.iter().position(|p| p.id == wire::UPDATE).unwrap();
    let marker = messages.iter().position(|p| p.id == 34).unwrap();
    assert!(update < marker);
    assert_eq!(messages[marker].body, first.to_be_bytes());
    w.remove_entity(first);
    w.updates(id, false).unwrap();
    assert_eq!(w.players[&id].quest, Some(second));
    w.players.get_mut(&id).unwrap().character.level = 9;
    w.updates(id, false).unwrap();
    assert_eq!(w.players[&id].quest, None);
    w.players.get_mut(&id).unwrap().character.level = 3;
    w.safe = true;
    w.updates(id, false).unwrap();
    assert_eq!(w.players[&id].quest, None);
}

#[test]
fn permanent_potion_publishes_only_after_commit_and_never_revives_pending_death() {
    let mut w = tests::world();
    let mut ch = tests::ch(&w);
    ch.items[4] = 0xa21;
    let id = w.enter(ch, None).unwrap();
    let before = w.players[&id].character.base;
    let slot = Slot {
        object: id,
        slot: 4,
        kind: 0xa21,
    };
    let plan = w.use_item(id, slot, w.players[&id].pos).unwrap().unwrap();
    assert_eq!(w.players[&id].character.base, before);
    assert_eq!(w.players[&id].character.items[4], 0xa21);
    assert!(w.use_item(id, slot, w.players[&id].pos).is_err());
    assert!(w.pending_deaths().is_empty());
    w.players.get_mut(&id).unwrap().character.hp = 0;
    w.players.get_mut(&id).unwrap().death_pending = true;
    w.finish_inventory(plan, Ok(1)).unwrap();
    assert_eq!(w.players[&id].character.base[4], before[4] + 1);
    assert_eq!(w.players[&id].character.items[4], -1);
    assert_eq!(w.players[&id].character.hp, 0);
    assert!(w.players[&id].death_pending);
    assert_eq!(w.pending_deaths().len(), 1);
}

#[test]
fn capped_potion_is_retained_and_failed_commit_quarantines_without_stat_gain() {
    let mut w = tests::world();
    let mut ch = tests::ch(&w);
    ch.items[4] = 0xa4c;
    ch.base[5] = w.content.classes[&ch.class].caps[5];
    let id = w.enter(ch, None).unwrap();
    let slot = Slot {
        object: id,
        slot: 4,
        kind: 0xa4c,
    };
    assert!(w.use_item(id, slot, w.players[&id].pos).is_err());
    assert!(!w.players[&id].pending);
    assert_eq!(w.players[&id].character.items[4], 0xa4c);
    w.players.get_mut(&id).unwrap().character.base[5] -= 1;
    let before = w.players[&id].character.base;
    let plan = w.use_item(id, slot, w.players[&id].pos).unwrap().unwrap();
    assert!(
        w.finish_inventory(plan, Err(anyhow::anyhow!("database unavailable")))
            .is_err()
    );
    assert_eq!(w.players[&id].character.base, before);
    assert_eq!(w.players[&id].character.items[4], 0xa4c);
    assert!(w.players[&id].persistence_failure.is_some());
}
