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
    let mut map = Map {
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
    map.cells[40 * 80 + 40].object = Some(BOSS);
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.encounter = true;
    w.name = "Manor of the Immortals".into();
    let boss = w.actors().find(|e| e.kind == BOSS).unwrap().id;
    w.entities.get_mut(&boss).unwrap().activated = Some(0);
    (w, boss)
}
#[test]
fn manor_kage_drop_requests_bind_portals_and_instances_do_not_share_bosses() {
    let mut d = tests::directory();
    let realm = d.worlds.get_mut(&-3).unwrap();
    let p = realm.map.spawns[0];
    let pos = Pos { x: p.0, y: p.1 };
    let seed = (0..256)
        .find(|seed| rand::rngs::StdRng::seed_from_u64(*seed).gen_bool(0.5))
        .unwrap();
    realm.rng = rand::rngs::StdRng::seed_from_u64(seed);
    let kage = realm.spawn_entity(0x6db, pos, vec![]).unwrap();
    realm.defeat_entity(kage).unwrap();
    d.step();
    let portal = d
        .portals
        .iter()
        .find(|(id, p)| p.origin == -3 && d.worlds[&-3].entities[id].kind == 0x739)
        .map(|(_, p)| p.destination.unwrap())
        .expect("successful Kage portal drop must be bound");
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
    assert_eq!(crate::dungeon::portal_drop(0x6db), Some((0x739, 0.5)));
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
                .map(|c| (c.ground, c.object, &c.config))
                .collect::<Vec<_>>(),
            again
                .cells
                .iter()
                .map(|c| (c.ground, c.object, &c.config))
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
        let mut coffins = BTreeMap::new();
        for c in map.cells.iter().filter(|c| c.object == Some(COFFIN)) {
            *coffins.entry(c.region).or_insert(0) += 1;
        }
        assert!(coffins.values().all(|n| (1..=2).contains(n)));
        if map.cells.iter().any(|c| c.region == 250) {
            assert!(coffins.values().any(|n| *n == 2));
        }
        let sx = start % 224;
        let sy = start / 224;
        assert!(!map.cells.iter().enumerate().any(|(i, c)| {
            c.object.is_some_and(|k| ROSTER.contains(&k))
                && sx.abs_diff(i % 224) <= 5
                && sy.abs_diff(i / 224) <= 5
        }));
        let generated = World::new(w.content.clone(), Arc::new(map)).unwrap();
        for shelf in generated.actors().filter(|e| e.kind == BOOKCASE) {
            let cell = &generated.map.cells[shelf.pos.y as usize * 224 + shelf.pos.x as usize];
            assert_eq!(
                shelf.condition & condition::bit(condition::INVINCIBLE) == 0,
                cell.region == 250
            );
        }
    }
    assert!(shapes.len() > 10);
    assert!(secrets > 20);
}
#[test]
fn manor_returns_on_timer_with_four_living_coffins_and_owned_bat_groups() {
    let (mut w, boss) = arena();
    for t in [0, 9100, 11750, 13250, 16750] {
        w.now = t;
        w.mechanic_expiry();
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
fn manor_live_party_join_and_reconnect_do_not_restart_or_duplicate_the_fight() {
    let (mut w, boss) = arena();
    let first = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&first).unwrap().pos = Pos { x: 40.5, y: 45.5 };
    w.npc_step();
    w.entities.get_mut(&boss).unwrap().hp = 14900;
    w.now = 16750;
    w.npc_step();
    let formation: BTreeSet<_> = w
        .actors()
        .filter(|e| e.parent.is_some())
        .map(|e| e.id)
        .collect();
    assert_eq!(formation.len(), 49);
    let mut second = tests::ch(&w);
    second.id = 2;
    second.account = 2;
    let second = w.enter(second, None).unwrap();
    w.players.get_mut(&second).unwrap().pos = Pos { x: 40.5, y: 45.5 };
    let saved = w.leave(first).unwrap();
    let rejoined = w.enter(saved, None).unwrap();
    w.players.get_mut(&rejoined).unwrap().pos = Pos { x: 40.5, y: 45.5 };
    w.now = 17000;
    w.npc_step();
    assert_eq!(w.entities[&boss].activated, Some(0));
    assert_eq!(w.entities[&boss].stage, Some(2));
    assert_eq!(w.entities[&boss].hp, 14900);
    assert_eq!(
        w.actors()
            .filter(|e| e.parent.is_some())
            .map(|e| e.id)
            .collect::<BTreeSet<_>>(),
        formation
    );
    w.now = 21750;
    w.npc_step();
    assert_eq!(w.entities[&boss].stage, Some(3));
    assert!(!w.actors().any(|e| e.parent.is_some()));
}

#[test]
fn manor_corridor_ritual_uses_the_lured_position_and_failed_tosses_do_not_softlock_bats() {
    let (mut w, boss) = arena();
    let map = Arc::make_mut(&mut w.map);
    for c in &mut map.cells {
        c.ground = 0xff;
    }
    for y in 1..79 {
        for x in 39..42 {
            map.cells[y * 80 + x].ground = 0x61;
        }
    }
    w.move_entity(boss, Pos { x: 40.5, y: 60.5 }, 20.).unwrap();
    w.manor_mechanics(boss, None, 0).unwrap();
    assert_eq!(w.entities[&boss].home.y, 40.5);
    w.now = 9100;
    w.manor_mechanics(boss, None, 9100).unwrap();
    assert!(!condition::protected(w.entities[&boss].condition));
    w.now = 9850;
    w.manor_mechanics(boss, None, 9850).unwrap();
    assert_ne!(
        w.entities[&boss].condition & condition::bit(condition::INVULNERABLE),
        0
    );
    assert!(matches!(
        w.manor_movement(boss, None, 9850).0,
        crate::encounter::Motion::Stationary
    ));
    w.now = 11750;
    w.manor_mechanics(boss, None, 11750).unwrap();
    assert_eq!(w.actors().filter(|e| e.kind == CREATURE).count(), 0);
    assert_eq!(w.manor_births.len(), 2);
    w.now = 13249;
    w.mechanic_expiry();
    assert_eq!(w.actors().filter(|e| e.kind == CREATURE).count(), 0);
    w.now = 13250;
    w.mechanic_expiry();
    let mut ys: Vec<_> = w
        .actors()
        .filter(|e| e.kind == CREATURE)
        .map(|e| e.pos.y)
        .collect();
    ys.sort_by(f32::total_cmp);
    assert_eq!(ys, vec![48.5, 72.5]);
    w.now = 16750;
    w.manor_mechanics(boss, None, 16750).unwrap();
    assert_ne!(
        w.entities[&boss].condition & condition::bit(condition::INVINCIBLE),
        0
    );
    assert_eq!(
        w.entities[&boss].condition & condition::bit(condition::INVULNERABLE),
        0
    );
    assert_eq!(
        w.actors()
            .filter(|e| matches!(e.kind, ESCORT | CHASER))
            .count(),
        45
    );
    assert!(w.actors().all(|e| w.walkable(e.pos)));
    assert!(
        w.actors()
            .filter(|e| matches!(e.kind, SWARM | DECOY))
            .all(|e| e.condition & condition::bit(condition::INVINCIBLE) != 0
                && e.condition & condition::bit(condition::INVULNERABLE) == 0)
    );
    w.now = 21750;
    w.manor_mechanics(boss, None, 21750).unwrap();
    assert_eq!(w.entities[&boss].home.y, 40.5);
    assert!(!condition::protected(w.entities[&boss].condition));
}

#[test]
fn manor_skipped_room_broods_leave_capacity_for_ruthven() {
    let (mut w, boss) = arena();
    for y in 0..7 {
        for x in 0..16 {
            let id = w
                .spawn_entity(
                    0x1727,
                    Pos {
                        x: 5.5 + x as f32 * 4.,
                        y: 5.5 + y as f32 * 4.,
                    },
                    vec![],
                )
                .unwrap();
            w.manor_mechanics(id, None, 0).unwrap();
        }
    }
    assert!(w.actors().filter(|e| e.parent.is_some()).count() > 256);
    w.manor_mechanics(boss, None, 0).unwrap();
    w.now = 11750;
    w.manor_mechanics(boss, None, 11750).unwrap();
    w.now = 16750;
    w.mechanic_expiry();
    w.manor_mechanics(boss, None, 16750).unwrap();
    assert_eq!(
        w.actors()
            .filter(|e| e.kind == ESCORT || e.kind == CHASER)
            .count(),
        45
    );
    assert!(w.actors().filter(|e| e.parent.is_some()).count() <= SUMMON_LIMIT);
    assert_eq!(tests::world().summon_limit(), 256);
}

#[test]
fn manor_toss_reserves_capacity_and_a_dead_guards_rock_lands_and_explodes() {
    let (mut w, boss) = arena();
    w.name = "generic budget".into();
    for _ in 0..255 {
        let id = w
            .spawn_entity(BAT, Pos { x: 20.5, y: 20.5 }, vec![])
            .unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(Some(boss), id, &mut w.timed_ids);
    }
    let guard = w
        .spawn_entity(0x1729, Pos { x: 30.5, y: 30.5 }, vec![])
        .unwrap();
    w.manor_mechanics(guard, Some(Pos { x: 32.5, y: 30.5 }), 0)
        .unwrap();
    assert_eq!(w.summon_occupancy(), 256);
    assert!(w.spawn_children(guard, BAT, 1, 0.4).is_err());
    w.remove_entity(guard);
    w.now = 1499;
    w.mechanic_expiry();
    assert!(!w.actors().any(|e| e.kind == ROCK));
    w.now = 1500;
    w.mechanic_expiry();
    let rock = w.actors().find(|e| e.kind == ROCK).unwrap().id;
    assert_eq!(w.entities[&rock].parent, Some(guard));
    assert_eq!(w.entities[&rock].activated, Some(1500));
    assert_eq!(w.entities[&rock].expires, Some(4550));
    assert!(w.manor_births.is_empty());
    w.now = 4500;
    w.npc_step();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == rock).count(), 8);
    w.now = 4550;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&rock));
}

#[test]
fn manor_kage_grave_cycle_and_mine_deadlines_preserve_selected_definitions() {
    let (mut w, _) = arena();
    let kage = w
        .spawn_entity(KAGE, Pos { x: 20.5, y: 20.5 }, vec![])
        .unwrap();
    assert_eq!(w.content.objects[&KAGE].hp, 7000);
    assert_eq!(w.content.objects[&KAGE].defense, 120);
    assert_eq!(w.content.objects[&MINE].hp, 500);
    w.entities.get_mut(&kage).unwrap().hp = 6300;
    w.manor_mechanics(kage, None, 0).unwrap();
    assert_eq!(w.entities[&kage].stage, Some(0));
    w.entities.get_mut(&kage).unwrap().hp = 6299;
    for (now, elapsed, stage, protected) in [
        (0, 0, 1, true),
        (2000, 2000, 2, false),
        (7000, 5000, 3, true),
        (11000, 4000, 2, false),
    ] {
        w.now = now;
        w.manor_mechanics(kage, None, elapsed).unwrap();
        assert_eq!(w.entities[&kage].stage, Some(stage));
        assert_eq!(condition::protected(w.entities[&kage].condition), protected);
    }
    let spin = attacks::manor_entry_schedule(KAGE, 2, 300).snapshot(0.);
    assert_eq!(spin.len(), 2);
    assert_eq!(spin[0].0, 1);
    assert!((spin[0].1 - 30f32.to_radians()).abs() < 0.001);
    assert_eq!(
        attacks::manor_entry_schedule(KAGE, 3, 0).snapshot(0.).len(),
        2
    );
    w.manor_mechanics(kage, Some(Pos { x: 24.5, y: 20.5 }), 0)
        .unwrap();
    w.remove_entity(kage);
    w.now = 12499;
    w.mechanic_expiry();
    assert!(!w.actors().any(|e| e.kind == MINE));
    w.now = 12500;
    w.mechanic_expiry();
    let mine = w.actors().find(|e| e.kind == MINE).unwrap().id;
    assert_eq!(w.entities[&mine].expires, Some(16550));
    w.now = 16499;
    w.manor_mechanics(mine, None, 3999).unwrap();
    assert_eq!(w.entities[&mine].stage, Some(0));
    w.now = 16500;
    w.manor_mechanics(mine, None, 4000).unwrap();
    assert_eq!(w.entities[&mine].stage, Some(1));
    assert_eq!(
        attacks::manor_entry_schedule(MINE, 1, 0).snapshot(0.).len(),
        4
    );
    w.now = 16550;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&mine));
}

#[test]
fn manor_spectral_mine_triggers_its_four_shots_on_a_cloaked_player() {
    let (mut w, _) = arena();
    let (packets, _rx) = mpsc::channel(4096);
    let (lost, _loss) = watch::channel(false);
    let player = w
        .enter(tests::ch(&w), Some(Output { packets, lost }))
        .unwrap();
    w.apply_condition(player, condition::INVISIBLE, 5000)
        .unwrap();
    let mine = w
        .spawn_entity(MINE, w.players[&player].pos, vec![])
        .unwrap();
    w.npc_step();
    assert_eq!(w.entities[&mine].stage, Some(1));
    assert_eq!(w.bullets.iter().filter(|b| b.owner == mine).count(), 4);
    w.npc_step();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == mine).count(), 4);
    w.now = 50;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&mine));
    w.finish_departure(player).unwrap();
    let mine = w
        .spawn_entity(MINE, Pos { x: 20.5, y: 20.5 }, vec![])
        .unwrap();
    w.entities.get_mut(&mine).unwrap().activated = Some(w.now);
    w.manor_mechanics(mine, None, 4000).unwrap();
    w.npc_step();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == mine).count(), 0);
    w.now += 50;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&mine));
}

#[test]
fn manor_realms_have_two_walkable_highland_graveyards_without_closure_membership() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    for source in crate::terrain::realms::sources(std::path::Path::new("data")).unwrap() {
        let map = source.open(&content).unwrap();
        let mut w = World::new(content.clone(), map).unwrap();
        w.rng = rand::rngs::StdRng::seed_from_u64(12351);
        w.populate_coil_snakes().unwrap();
        w.populate_realm_heroes().unwrap();
        w.populate_named_quests().unwrap();
        w.populate_realm_events().unwrap();
        w.populate_hermit().unwrap();
        let original = w.map.clone();
        let old = w.setpieces.len();
        w.populate_manor_entry().unwrap();
        assert_eq!(w.actors().filter(|e| e.kind == KAGE).count(), 2);
        for area in &w.setpieces[old..] {
            let center = (area.y + 32) * w.map.width + area.x + 32;
            assert!(matches!(original.cells[center as usize].terrain, 3 | 4));
            assert!(w.actors().any(|e| e.kind == KAGE
                && area.distance(e.pos) == 0.
                && w.walkable(e.pos)
                && e.quest.is_none()));
            assert_eq!(
                w.actors()
                    .filter(|e| e.kind == GRAVE && area.distance(e.pos) == 0.)
                    .count(),
                24
            );
        }
    }
}

#[test]
fn manor_corner_coffin_death_releases_only_walkable_feratus() {
    let (mut w, _) = arena();
    let map = Arc::make_mut(&mut w.map);
    for (x, y) in [(9, 9), (9, 10), (10, 9)] {
        map.cells[y * 80 + x].ground = 0xff;
    }
    let coffin = w
        .spawn_entity(CREATURE, Pos { x: 10.01, y: 10.01 }, vec![])
        .unwrap();
    w.defeat_entity(coffin).unwrap();
    let children: Vec<_> = w.actors().filter(|e| e.kind == FERATU).collect();
    assert!((1..=4).contains(&children.len()));
    assert!(children.iter().all(|e| w.walkable(e.pos)));
    assert_eq!(
        attacks::schedule(CREATURE, 1000, 1500).unwrap().attacks[0].range,
        4.
    );
    assert_eq!(
        attacks::schedule(ESCORT, 200, 0).unwrap().attacks[0].interval,
        Some(900)
    );
    assert_eq!(
        attacks::schedule(0x1727, 125, 0).unwrap().attacks[0].interval,
        Some(100)
    );
}

#[test]
fn manor_room_swarm_is_once_only_and_clear_exit_stays_in_the_arena_after_luring() {
    let (mut w, boss) = arena();
    let swarmer = w
        .spawn_entity(0x1727, Pos { x: 10.5, y: 10.5 }, vec![])
        .unwrap();
    w.manor_mechanics(swarmer, None, 0).unwrap();
    let n = w
        .actors()
        .filter(|e| e.parent == Some(swarmer) && e.kind == BAT)
        .count();
    assert!((15..=20).contains(&n));
    w.manor_mechanics(swarmer, None, 50).unwrap();
    assert_eq!(w.actors().filter(|e| e.parent == Some(swarmer)).count(), n);
    w.move_entity(boss, Pos { x: 40.5, y: 60.5 }, 20.).unwrap();
    w.defeat_entity(boss).unwrap();
    let exit = &w.entities[&w.clear_exit.unwrap()];
    assert_eq!((exit.pos.x, exit.pos.y), (42.5, 40.5));
    assert!(!exit.published);
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
            let culled = if seed % 2 == 0 {
                let bats: Vec<_> = w
                    .actors()
                    .filter(|e| e.parent == Some(id) && e.kind == BAT)
                    .map(|e| e.id)
                    .collect();
                let n = bats.len();
                for bat in bats {
                    w.remove_entity(bat);
                }
                n
            } else {
                0
            };
            w.entities.get_mut(&id).unwrap().hp = 7999;
            w.manor_mechanics(id, None, 50).unwrap();
            assert_eq!(
                w.actors()
                    .filter(|e| e.parent == Some(id) && e.kind == BAT)
                    .count(),
                23 - culled
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
    assert_eq!(crate::dungeon::manor::blades(14999), 3);
    assert_eq!(crate::dungeon::manor::blades(1), 17);
    assert_eq!(crate::dungeon::manor::profile(BAT).speed, 8. / 1.5);
    let mut clocks = attacks::Clocks::default();
    assert_eq!(
        clocks.due(&attacks::schedule(BOSS, 15000, 0).unwrap(), 0),
        vec![0]
    );
    assert!(
        clocks
            .due(&attacks::schedule(BOSS, 13999, 50).unwrap(), 50)
            .is_empty()
    );
    assert_eq!(
        clocks.due(&attacks::schedule(BOSS, 13999, 700).unwrap(), 700),
        vec![0]
    );
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
    let mut late = tests::ch(&w);
    late.id = 2;
    late.account = 2;
    let late = w.enter(late, None).unwrap();
    assert_eq!(w.players[&late].character.manors, 0);
    assert!(w.pending_progress_where(|_, _| true).is_empty());
}
