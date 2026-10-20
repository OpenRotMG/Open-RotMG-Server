use super::*;
use crate::{condition, dungeon::manor::*, encounter::attacks};

#[test]
fn manor_life_steal_requires_an_acknowledged_once_only_hit_and_living_owner() {
    let (mut w, boss) = arena();
    let (packets, _rx) = mpsc::channel(4096);
    let (lost, _loss) = watch::channel(false);
    let player = w
        .enter(tests::ch(&w), Some(Output { packets, lost }))
        .unwrap();
    let pos = w.players[&player].pos;
    let b = Bullet {
        owner: boss,
        source_kind: BOSS,
        id: 3,
        friendly: false,
        start: pos,
        pos,
        angle: 0.,
        born: 0,
        damage: 30,
        spec: w.content.objects[&BOSS].projectiles[&0].clone(),
        hit: BTreeSet::new(),
    };
    w.entities.get_mut(&boss).unwrap().hp = 14980;
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&player).unwrap(),
        vec![b.clone()],
        &w.clock
    ));
    w.native_player_hit(player, boss, 3).unwrap();
    assert_eq!(w.entities[&boss].hp, 14980);
    w.acknowledge_shoot(player, 0).unwrap();
    w.native_player_hit(player, boss, 3).unwrap();
    assert_eq!(w.entities[&boss].hp, 15000);
    assert_eq!(w.players[&player].character.hp, 70);
    w.entities.get_mut(&boss).unwrap().hp = 14980;
    w.native_player_hit(player, boss, 3).unwrap();
    assert_eq!(w.entities[&boss].hp, 14980);
    let mut b = b.clone();
    b.id = 4;
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&player).unwrap(),
        vec![b.clone()],
        &w.clock
    ));
    w.acknowledge_shoot(player, 50).unwrap();
    w.apply_condition(player, condition::INVULNERABLE, 1000)
        .unwrap();
    w.native_player_hit(player, boss, 4).unwrap();
    assert_eq!(w.entities[&boss].hp, 14980);
    w.players.get_mut(&player).unwrap().statuses.clear();
    b.id = 5;
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&player).unwrap(),
        vec![b],
        &w.clock
    ));
    w.acknowledge_shoot(player, 100).unwrap();
    w.remove_entity(boss);
    w.native_player_hit(player, boss, 5).unwrap();
    assert_eq!(w.players[&player].character.hp, 40);
}

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
            80 * 80
        ],
        spawns: vec![(5.5, 5.5)],
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.encounter = true;
    w.name = "Manor of the Immortals".into();
    let boss = w
        .spawn_entity(BOSS, Pos { x: 40.5, y: 40.5 }, vec![])
        .unwrap();
    w.entities.get_mut(&boss).unwrap().activated = Some(0);
    (w, boss)
}
#[test]
fn manor_kage_defeat_binds_an_earned_portal_and_instances_do_not_share_bosses() {
    let mut d = tests::directory();
    let realm = d.worlds.get_mut(&-3).unwrap();
    let p = realm.map.spawns[0];
    let pos = Pos { x: p.0, y: p.1 };
    let kage = realm.spawn_entity(0x6db, pos, vec![]).unwrap();
    realm.defeat_entity(kage).unwrap();
    d.step();
    let portal = d
        .portals
        .iter()
        .find(|(id, p)| p.origin == -3 && d.worlds[&-3].entities[id].kind == 0x739)
        .map(|(_, p)| p.destination.unwrap())
        .expect("Kage's earned Manor portal must be bound");
    assert_eq!(d.worlds[&portal].name, "Manor of the Immortals");
    let second = d.instance("manor-of-the-immortals", None).unwrap();
    assert_ne!(second, portal);
    let boss = d.worlds[&portal]
        .actors()
        .find(|e| e.kind == BOSS)
        .unwrap()
        .id;
    d.worlds
        .get_mut(&portal)
        .unwrap()
        .defeat_entity(boss)
        .unwrap();
    assert!(d.worlds[&portal].cleared);
    assert!(!d.worlds[&second].cleared);
    assert!(
        d.worlds[&second]
            .actors()
            .any(|e| e.kind == BOSS && e.hp == 15000)
    );
}

#[test]
fn manor_selected_content_and_every_attack_channel_are_valid() {
    let w = tests::world();
    assert_eq!(
        (
            w.content.objects[&BOSS].hp,
            w.content.objects[&BOSS].defense
        ),
        (15000, 95)
    );
    assert!(w.content.objects[&BOSS].leach_health);
    assert_eq!(w.content.objects[&CREATURE].defense, 75);
    assert_eq!(w.content.objects[&0x172b].hp, 600);
    for kind in ROSTER {
        crate::encounter::validate(&w.content, kind).unwrap();
        crate::loot::validate(&w.content, kind).unwrap();
    }
    assert_eq!(crate::dungeon::portal_drop(0x6db), Some((0x739, 1.)));
    assert!(crate::loot::private(BOSS) && crate::loot::private(COFFIN));
    assert!(!crate::loot::collectibles(&w.content).is_empty());
}
#[test]
fn manor_layouts_are_seeded_connected_and_have_a_safe_arrival() {
    let w = tests::world();
    let mut shapes = BTreeSet::new();
    let mut secrets = 0;
    for seed in 0..64 {
        let map = crate::dungeon::manor::generate(&w.content, seed).unwrap();
        let again = crate::dungeon::manor::generate(&w.content, seed).unwrap();
        assert_eq!(
            map.cells
                .iter()
                .map(|c| (c.ground, c.object))
                .collect::<Vec<_>>(),
            again
                .cells
                .iter()
                .map(|c| (c.ground, c.object))
                .collect::<Vec<_>>()
        );
        let start = map.spawns[0];
        let start = start.1 as usize * 224 + start.0 as usize;
        let mut seen = BTreeSet::from([start]);
        let mut todo = vec![start];
        while let Some(i) = todo.pop() {
            for j in [i.wrapping_sub(1), i + 1, i.wrapping_sub(224), i + 224] {
                if j >= map.cells.len() {
                    continue;
                }
                let c = &map.cells[j];
                if c.ground != 0xff
                    && c.object
                        .is_none_or(|k| k == BOOKCASE || !w.content.objects[&k].blocks)
                    && seen.insert(j)
                {
                    todo.push(j);
                }
            }
        }
        let bosses: Vec<_> = map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(BOSS))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(bosses.len(), 1);
        assert!(seen.contains(&bosses[0]));
        shapes.insert(bosses[0]);
        for (i, c) in map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(COFFIN))
        {
            assert!(seen.contains(&i));
            secrets += 1;
            assert_ne!(c.region, 1);
        }
        let sx = start % 224;
        let sy = start / 224;
        assert!(!map.cells.iter().enumerate().any(|(i, c)| {
            c.object.is_some_and(|k| ROSTER.contains(&k))
                && sx.abs_diff(i % 224) <= 5
                && sy.abs_diff(i / 224) <= 5
        }));
        World::new(w.content.clone(), Arc::new(map)).unwrap();
    }
    assert!(shapes.len() > 10);
    assert!(secrets > 20);
}
#[test]
fn manor_returns_on_timer_with_four_living_coffins_and_owned_bat_groups() {
    let (mut w, boss) = arena();
    for t in [0, 9100, 11750, 16750] {
        w.now = t;
        w.manor_mechanics(boss, Some(Pos { x: 42., y: 42. }), t)
            .unwrap();
    }
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == CREATURE && e.parent == Some(boss))
            .count(),
        4
    );
    let roots: Vec<_> = w
        .actors()
        .filter(|e| matches!(e.kind, SWARM | DECOY) && e.parent == Some(boss))
        .map(|e| e.id)
        .collect();
    assert_eq!(roots.len(), 4);
    for root in &roots {
        w.manor_mechanics(*root, None, 0).unwrap();
    }
    assert_eq!(
        w.actors()
            .filter(|e| matches!(e.kind, ESCORT | CHASER))
            .count(),
        45
    );
    assert_eq!(w.entities[&boss].extra_stats, vec![(61, 1)]);
    assert!(condition::protected(w.entities[&boss].condition));
    w.now = 21750;
    w.manor_mechanics(boss, None, 21750).unwrap();
    assert!(!condition::protected(w.entities[&boss].condition));
    assert_eq!(w.entities[&boss].extra_stats, vec![(61, 0)]);
    assert_eq!(
        w.actors()
            .filter(|e| matches!(e.kind, SWARM | DECOY | ESCORT | CHASER))
            .count(),
        0
    );
    assert_eq!(w.actors().filter(|e| e.kind == CREATURE).count(), 4);
    assert!(!w.cleared);
}
#[test]
fn manor_treasure_coffin_waves_are_once_only_and_impostors_do_not_clear_the_dungeon() {
    let mut real = false;
    let mut fake = false;
    for seed in 0..16 {
        let (mut w, _) = arena();
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        let id = w
            .spawn_entity(COFFIN, Pos { x: 20.5, y: 20.5 }, vec![])
            .unwrap();
        w.entities.get_mut(&id).unwrap().hp = 14999;
        w.manor_mechanics(id, None, 0).unwrap();
        if w.entities.contains_key(&id) {
            real = true;
            assert_eq!(
                w.actors()
                    .filter(|e| e.parent == Some(id) && e.kind == BAT)
                    .count(),
                15
            );
            w.entities.get_mut(&id).unwrap().hp = 7999;
            w.manor_mechanics(id, None, 50).unwrap();
            assert_eq!(
                w.actors()
                    .filter(|e| e.parent == Some(id) && e.kind == BAT)
                    .count(),
                23
            );
            assert_eq!(
                w.actors()
                    .filter(|e| e.parent == Some(id) && e.kind == 0x1728)
                    .count(),
                2
            );
            let n = w.actors().count();
            w.manor_mechanics(id, None, 100).unwrap();
            assert_eq!(w.actors().count(), n);
        } else {
            fake = true;
            assert!(
                w.actors()
                    .any(|e| e.kind == CREATURE && e.hp == 1000 && e.expires == Some(20000))
            );
        }
        assert!(!w.cleared);
        assert!(
            !w.actors()
                .any(|e| w.content.objects[&e.kind].class == "Container")
        );
    }
    assert!(real && fake);
}

#[test]
fn manor_blade_escalation_and_rock_burst_use_selected_indices_once() {
    assert_eq!(crate::dungeon::manor::blades(15000), 3);
    assert_eq!(crate::dungeon::manor::blades(13000), 5);
    assert_eq!(crate::dungeon::manor::blades(1), 18);
    let before = attacks::schedule(ROCK, 400, 2950).unwrap();
    assert!(before.snapshot(0.).is_empty());
    let burst = attacks::schedule(ROCK, 400, 3000).unwrap();
    assert_eq!(burst.snapshot(0.).len(), 8);
    assert!(
        attacks::schedule(ROCK, 400, 3050)
            .unwrap()
            .snapshot(0.)
            .is_empty()
    );
    assert!(
        attacks::schedule(BOSS, 15000, 16750)
            .unwrap()
            .attacks
            .is_empty()
    );
}
#[test]
fn manor_coffin_creatures_split_on_earned_and_timed_defeat_but_not_removal() {
    let (mut w, boss) = arena();
    let a = w
        .spawn_entity(CREATURE, Pos { x: 30.5, y: 30.5 }, vec![])
        .unwrap();
    let b = w
        .spawn_entity(CREATURE, Pos { x: 31.5, y: 30.5 }, vec![])
        .unwrap();
    for id in [a, b] {
        w.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(Some(boss), id, &mut w.timed_ids);
    }
    w.remove_entity(b);
    assert_eq!(w.actors().filter(|e| e.kind == FERATU).count(), 0);
    w.finish_timed_defeat(a).unwrap();
    let n = w
        .actors()
        .filter(|e| e.kind == FERATU && e.parent == Some(boss))
        .count();
    assert!((1..=4).contains(&n));
    w.defeat_entity(boss).unwrap();
    assert_eq!(w.actors().filter(|e| e.parent == Some(boss)).count(), 0);
}
#[test]
fn manor_clear_is_committed_once_and_does_not_disappear_in_counter_zip() {
    let (mut w, boss) = arena();
    let mut ch = tests::ch(&w);
    ch.level = 20;
    let player = w.enter(ch, None).unwrap();
    let character = w.players[&player].character.id;
    w.players.get_mut(&player).unwrap().pos = Pos { x: 40., y: 40. };
    w.entities
        .get_mut(&boss)
        .unwrap()
        .damage
        .insert(character, 15000);
    w.defeat_entity(boss).unwrap();
    assert!(w.cleared);
    let snapshots = w.pending_progress_where(|_, _| true);
    assert_eq!(snapshots.len(), 1);
    let (_, saved) = snapshots.into_iter().next().unwrap();
    assert_eq!(saved.manors, 1);
    assert_eq!(saved.spider_dens, 0);
    let exit = w
        .clear_exit
        .expect("clear exit must be staged before persistence");
    assert!(!w.entities[&exit].published);
    w.finish_progress(player, saved).unwrap();
    assert_eq!(w.players[&player].character.manors, 1);
    assert!(w.pending_progress_where(|_, _| true).is_empty());
    assert!(w.clear_exit.is_none());
    assert!(w.entities[&exit].published);
}
