use super::*;
use crate::{condition, dungeon::jungle::*, encounter::attacks::Clocks};

fn fight() -> (World, i32, Vec<i32>) {
    let mut w = tests::arena(1);
    let boss = w
        .spawn_entity(BOSS, Pos { x: 12.5, y: 12.5 }, vec![])
        .unwrap();
    let totems = [(-5., -5.), (5., -5.), (-5., 5.), (5., 5.)]
        .into_iter()
        .map(|(x, y)| {
            w.spawn_entity(
                BOSS_TOTEM,
                Pos {
                    x: 12.5 + x,
                    y: 12.5 + y,
                },
                vec![],
            )
            .unwrap()
        })
        .collect();
    w.jungle_initialize().unwrap();
    (w, boss, totems)
}

#[test]
fn jungle_damage_ritual_half_health_and_owned_totem_lifecycle() {
    let (mut w, boss, totems) = fight();
    let other = w
        .spawn_entity(BOSS, Pos { x: 3.5, y: 3.5 }, vec![])
        .unwrap();
    let foreign = w
        .spawn_entity(BOSS_TOTEM, Pos { x: 4.5, y: 3.5 }, vec![])
        .unwrap();
    w.entities.get_mut(&foreign).unwrap().quest = Some(other);
    w.entities.get_mut(&other).unwrap().stage = Some(5);
    w.entities.get_mut(&other).unwrap().activated = Some(0);
    w.jungle_mechanics(boss, 0).unwrap();
    w.now = 10000;
    w.jungle_mechanics(boss, 10000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(0));
    assert!(w.jungle_schedule(boss).unwrap().attacks.is_empty());
    w.damage_enemy(0, boss, 1, true, &[], None).unwrap();
    w.jungle_mechanics(boss, 10000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(1));
    assert_eq!(w.actors().filter(|e| e.kind == SPIRIT).count(), 8);
    assert_eq!(w.damage_enemy(0, boss, 1000, true, &[], None).unwrap(), 0);
    for t in &totems {
        assert_eq!(w.damage_enemy(0, *t, 10000, true, &[], None).unwrap(), 0);
        assert!(w.entities.contains_key(t));
    }
    w.now += 5000;
    w.jungle_mechanics(boss, 5000).unwrap();
    assert!(matches!(w.entities[&boss].stage, Some(2 | 3)));
    w.now += 5000;
    w.jungle_mechanics(boss, 5000).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(1));
    assert_eq!(w.actors().filter(|e| e.kind == SPIRIT).count(), 8);
    w.now += 5000;
    w.jungle_mechanics(boss, 5000).unwrap();
    let hp = w.entities[&boss].hp;
    w.damage_enemy(0, boss, hp - 2100, true, &[], None).unwrap();
    w.jungle_mechanics(boss, 0).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(4));
    w.jungle_mechanics(boss, 0).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(5));
    assert_eq!(w.jungle_schedule(boss).unwrap().snapshot(0.).len(), 8);
    for t in &totems {
        assert_eq!(w.jungle_schedule(*t).unwrap().snapshot(0.).len(), 6);
    }
    assert_ne!(
        w.entities[&boss].condition & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now += 7500;
    w.jungle_mechanics(boss, 7500).unwrap();
    assert_eq!(w.entities[&boss].stage, Some(6));
    for t in &totems {
        assert_eq!(w.jungle_schedule(*t).unwrap().snapshot(0.).len(), 4);
    }
    w.damage_enemy(0, boss, 3000, true, &[], None).unwrap();
    w.mechanic_expiry();
    assert_eq!(w.actors().filter(|e| e.kind == SPIRIT).count(), 8);
    for t in totems {
        assert!(w.jungle_schedule(t).unwrap().attacks.is_empty());
    }
    assert_eq!(w.entities[&other].stage, Some(5));
    assert_eq!(w.jungle_schedule(foreign).unwrap().attacks[0].count, 6);
}

#[test]
fn jungle_camp_healing_statuses_and_foliage_collision() {
    let mut w = tests::arena(1);
    let p = Pos { x: 12.5, y: 12.5 };
    let fire = w.spawn_entity(FIRE, p, vec![]).unwrap();
    let shaman = w
        .spawn_entity(SHAMAN, Pos { x: 15.5, ..p }, vec![])
        .unwrap();
    let warrior = w
        .spawn_entity(WARRIOR, Pos { x: 12.5, y: 15.5 }, vec![])
        .unwrap();
    w.jungle_initialize().unwrap();
    w.entities.get_mut(&warrior).unwrap().hp = 100;
    w.jungle_mechanics(fire, 0).unwrap();
    w.jungle_mechanics(shaman, 0).unwrap();
    assert_eq!(w.entities[&warrior].hp, 160);
    let effects = &w.content.objects[&SHAMAN].projectiles[&0].effects;
    assert!(
        effects.contains(&(condition::projectile_effect("Hallucinating").unwrap(), 4000))
            && effects.contains(&(condition::QUIET, 1000))
    );
    let bush = Pos { x: 10.5, y: 10.5 };
    w.spawn_entity(0xdda, bush, vec![]).unwrap();
    assert!(!w.walkable(bush));
    assert!(w.jungle_walkable(BASILISK, bush));
    assert!(!w.jungle_walkable(WARRIOR, bush));
    w.damage_enemy(0, fire, 1000, true, &[], None).unwrap();
    w.mechanic_expiry();
    assert_eq!(w.entities[&warrior].parent, None);
    assert!(w.entities.contains_key(&shaman));
    assert!(matches!(
        crate::encounter::jungle::movement(HUNTER, 0, false).0,
        crate::encounter::Motion::Follow { .. }
    ));
    assert!(matches!(
        crate::encounter::jungle::movement(BABY, 0, false).0,
        crate::encounter::Motion::Maintain { .. }
    ));
}

#[test]
fn jungle_attack_deadlines_and_selected_projectiles() {
    let (w, _, _) = fight();
    for kind in ROSTER {
        crate::encounter::validate(&w.content, kind).unwrap();
        crate::loot::validate(&w.content, kind).unwrap();
    }
    let p = &w.content.objects[&BOSS].projectiles;
    assert_eq!((p[&0].min, p[&0].speed, p[&0].lifetime), (60, 30., 800.));
    assert_eq!((p[&2].min, p[&2].max), (45, 65));
    let mut clock = Clocks::default();
    let counts: Vec<_> = (0..=1500)
        .step_by(50)
        .filter_map(|age| {
            let s = crate::encounter::jungle::schedule(BOSS, 2, age);
            let n = clock.due(&s, age).len();
            (n > 0).then_some((age, n))
        })
        .collect();
    assert_eq!(counts, [(0, 1), (250, 1), (1250, 1), (1500, 1)]);
    let s = crate::encounter::jungle::schedule(BOSS, 3, 200).snapshot(0.);
    assert_eq!(s.len(), 3);
    assert!((s[1].1 - (-45f32).to_radians()).abs() < 0.001);
}

#[test]
fn jungle_generated_routes_rewards_and_transactional_clear() {
    let w = tests::world();
    for seed in 0..32 {
        let map = crate::dungeon::jungle::generate(&w.content, seed).unwrap();
        let mut seen = BTreeSet::new();
        let mut queue = std::collections::VecDeque::from([(60, 100)]);
        while let Some((x, y)) = queue.pop_front() {
            if x < 0 || y < 0 || x >= map.width || y >= map.height || !seen.insert((x, y)) {
                continue;
            }
            let c = &map.cells[(y * map.width + x) as usize];
            if w.content.grounds[&c.ground] <= 0.
                || c.object.is_some_and(|k| w.content.objects[&k].blocks)
            {
                continue;
            }
            queue.extend([(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]);
        }
        let boss = map
            .cells
            .iter()
            .position(|c| c.object == Some(BOSS))
            .unwrap();
        assert!(
            seen.contains(&(boss as i32 % map.width, boss as i32 / map.width)),
            "seed {seed}"
        );
        assert_eq!(
            map.cells
                .iter()
                .filter(|c| c.object == Some(BOSS_TOTEM))
                .count(),
            4
        );
    }
    let mut directory = tests::directory();
    directory.prepared_seed = Some(12351);
    let world = directory.instance("forbidden-jungle", None).unwrap();
    assert_eq!(directory.worlds[&world].name, "Forbidden Jungle");
    let (mut w, boss, _) = fight();
    w.name = "Forbidden Jungle".into();
    let mut ch = tests::ch(&w);
    ch.base[0] = 670;
    ch.hp = 670;
    let player = w.enter(ch, None).unwrap();
    w.players.get_mut(&player).unwrap().pos = Pos { x: 12.5, y: 16.5 };
    w.damage_enemy(player, boss, 5000, true, &[], None).unwrap();
    assert!(w.cleared);
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    let progress = w.pending_progress_where(|_, _| true);
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].1.jungles, 1);
    w.finish_progress(player, progress[0].1.clone()).unwrap();
    assert!(w.entities[&exit].published);
    assert_eq!(w.players[&player].character.jungles, 1);
    assert!(w.pending_progress_where(|_, _| true).is_empty());
    assert!(w.entities.values().any(|e| e.items.contains(&0xc1a)));
    assert_eq!(crate::dungeon::portal_drop(0xdc2), Some((0x733, 0.2)));
}

#[test]
fn jungle_entrance_is_earned_from_a_real_realm_coil_and_eggs_hatch() {
    let mut w = tests::arena(1);
    let coil = w
        .spawn_entity(COIL, Pos { x: 12.5, y: 12.5 }, vec![])
        .unwrap();
    w.coil_mechanics(coil, 0).unwrap();
    w.now = 3000;
    w.coil_mechanics(coil, 3000).unwrap();
    assert_eq!(w.pending_named(coil, Some(EGG)), 1);
    w.now = 4500;
    w.named_birth_step();
    let egg = w.actors().find(|e| e.kind == EGG).unwrap().id;
    assert_eq!(w.entities[&egg].hp, 50);
    w.now = 9500;
    w.coil_mechanics(egg, 5000).unwrap();
    assert!(!w.entities.contains_key(&egg));
    assert!((1..=2).contains(&w.actors().filter(|e| e.kind == SNAKE).count()));
    let mut d = tests::directory();
    let realm = d.endgame.active;
    let w = d.worlds.get_mut(&realm).unwrap();
    let coils: Vec<_> = w
        .actors()
        .filter(|e| e.kind == COIL)
        .map(|e| e.id)
        .collect();
    assert_eq!(coils.len(), 2);
    let ch = tests::ch(w);
    let player = w.enter(ch, None).unwrap();
    let pos = w.entities[&coils[0]].pos;
    w.players.get_mut(&player).unwrap().pos = pos;
    let seed = (0..100)
        .find(|s| rand::rngs::StdRng::seed_from_u64(*s).gen_bool(0.2))
        .unwrap();
    w.rng = rand::rngs::StdRng::seed_from_u64(seed);
    w.damage_enemy(player, coils[0], 1000, true, &[], None)
        .unwrap();
    assert_eq!(w.portal_requests.len(), 1);
    d.step();
    let portal = *d
        .portals
        .iter()
        .find(|(id, p)| p.origin == realm && d.worlds[&realm].entities[id].kind == 0x733)
        .unwrap()
        .0;
    let (_, _, destination) = d.destination(player, Some(portal)).unwrap();
    assert_eq!(d.worlds[&destination].name, "Forbidden Jungle");
}
