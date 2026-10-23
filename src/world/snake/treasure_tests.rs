use super::*;
use crate::dungeon::snake::{self, BUTTON, DART, GUARD, SPAWNER};

fn fixture() -> (World, i32, Vec<i32>) {
    let content = crate::world::tests::world().content.clone();
    let map = Arc::new(snake::generate(&content, 0).unwrap());
    let mut w = World::new(content, map).unwrap();
    w.name = "Snake Pit".into();
    w.encounter = true;
    let ch = crate::world::tests::ch(&w);
    let player = w
        .enter_at(
            ch,
            None,
            None,
            AccountPresentation {
                guild: None,
                fame: 0,
                best_fame: BTreeMap::new(),
                credits: 0,
            },
        )
        .unwrap();
    let buttons: Vec<_> = w
        .actors()
        .filter(|e| e.kind == BUTTON)
        .map(|e| e.id)
        .collect();
    assert_eq!(buttons.len(), 2);
    (w, player, buttons)
}
fn components(w: &World, button: i32) -> (i32, Vec<i32>) {
    let pos = w.entities[&button].pos;
    let spawner = w
        .actors()
        .find(|e| e.kind == SPAWNER && e.pos.distance(pos) < 15.)
        .unwrap();
    let traps = w
        .actors()
        .filter(|e| e.kind == DART && e.pos.distance(spawner.pos) < 15.)
        .map(|e| e.id)
        .collect();
    (spawner.id, traps)
}

#[test]
fn treasure_switch_is_local_immune_and_consumed_once() {
    let (mut w, player, buttons) = fixture();
    let button = buttons[0];
    let (spawner, traps) = components(&w, button);
    let (other, other_traps) = components(&w, buttons[1]);
    assert_eq!(
        w.damage_enemy(player, button, 10000, true, &[], None)
            .unwrap(),
        0
    );
    w.snake_mechanics(button, 0).unwrap();
    assert!(!w.actors().any(|e| e.kind == GUARD));
    assert!(!w.npc_attack_schedule(traps[0], 0).unwrap().unwrap().attacks[0].enabled);
    w.players.get_mut(&player).unwrap().pos = w.entities[&button].pos;
    w.now = 50;
    w.npc_step();
    let guards: Vec<_> = w
        .actors()
        .filter(|e| e.kind == GUARD)
        .map(|e| e.id)
        .collect();
    assert_eq!(guards.len(), 1);
    let guard = guards[0];
    assert_eq!(w.entities[&guard].parent, Some(spawner));
    assert_eq!(w.entities[&guard].hp, 5500);
    assert_eq!(w.content.get(GUARD).unwrap().defense, 50);
    assert_eq!(w.entities[&guard].size, 100);
    assert_eq!(w.entities[&button].stage, Some(1));
    assert!(w.entities[&button].extra_stats.contains(&(61, 1)));
    for trap in &traps {
        assert_eq!(w.entities[trap].parent, Some(spawner));
        let schedule = w.npc_attack_schedule(*trap, 0).unwrap().unwrap();
        assert!(schedule.attacks[0].enabled);
        let angle = if w.entities[trap].pos.x < w.entities[&spawner].pos.x {
            0.
        } else {
            std::f32::consts::PI
        };
        assert!(
            matches!(schedule.attacks[0].aim, crate::encounter::attacks::Aim::Fixed(a) if a == angle)
        );
    }
    for trap in other_traps {
        assert_eq!(w.entities[&trap].parent, None);
        assert!(!w.npc_attack_schedule(trap, 0).unwrap().unwrap().attacks[0].enabled);
    }
    assert!(
        !w.actors()
            .any(|e| e.kind == GUARD && e.parent == Some(other))
    );
    for _ in 0..5 {
        w.snake_mechanics(button, 0).unwrap();
    }
    assert_eq!(w.actors().filter(|e| e.kind == GUARD).count(), 1);
    w.now += 2100;
    w.npc_step();
    assert_eq!(
        w.bullets
            .iter()
            .filter(|b| traps.contains(&b.owner))
            .map(|b| b.owner)
            .collect::<BTreeSet<_>>(),
        traps.iter().copied().collect::<BTreeSet<_>>()
    );
    w.bullets.clear();
    w.damage_enemy(player, guard, 10000, true, &[], None)
        .unwrap();
    w.now += 2100;
    w.npc_step();
    assert!(!w.actors().any(|e| e.kind == GUARD));
    assert!(!w.bullets.iter().any(|b| traps.contains(&b.owner)));
    assert!(
        !w.npc_attack_schedule(traps[0], 2000)
            .unwrap()
            .unwrap()
            .attacks[0]
            .enabled
    );
    assert!(w.players[&player].clears_queued.iter().all(|n| *n == 0));
    assert!(crate::loot::private(GUARD));
}

#[test]
fn invalid_or_capacity_blocked_rooms_do_not_consume_the_switch() {
    let (mut w, player, buttons) = fixture();
    let button = buttons[0];
    let (spawner, traps) = components(&w, button);
    w.players.get_mut(&player).unwrap().pos = w.entities[&button].pos;
    let position = w.entities[&traps[0]].pos;
    w.remove_entity(traps[0]);
    assert!(
        w.snake_mechanics(button, 0)
            .unwrap_err()
            .to_string()
            .contains("ten local dart")
    );
    assert_eq!(w.entities[&button].stage, None);
    assert!(!w.actors().any(|e| e.kind == GUARD));
    w.spawn_entity(DART, position, vec![]).unwrap();
    let point = Pos {
        x: w.map.spawns[0].0,
        y: w.map.spawns[0].1,
    };
    for _ in 0..256 {
        let id = w.spawn_entity(0x223, point, vec![]).unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .parent
            .set(Some(spawner), id, &mut w.timed_ids);
    }
    assert!(
        w.snake_mechanics(button, 0)
            .unwrap_err()
            .to_string()
            .contains("summon budget exhausted")
    );
    assert_eq!(w.entities[&button].stage, None);
    assert!(!w.entities[&button].extra_stats.contains(&(61, 1)));
    assert!(!w.actors().any(|e| e.kind == GUARD));
}

#[test]
fn guard_idle_motor_keeps_its_reference_rate_above_and_below_rage() {
    let (mut w, player, buttons) = fixture();
    w.players.get_mut(&player).unwrap().pos = w.entities[&buttons[0]].pos;
    w.snake_mechanics(buttons[0], 0).unwrap();
    let guard = w.actors().find(|e| e.kind == GUARD).unwrap().id;
    let home = w.entities[&guard].home;
    w.players.get_mut(&player).unwrap().pos = Pos {
        x: home.x,
        y: home.y - 11.,
    };
    for hp in [5500, 3299] {
        let e = w.entities.get_mut(&guard).unwrap();
        e.hp = hp;
        e.pos = home;
        e.goal = Some(Pos {
            x: home.x + 1.5,
            y: home.y,
        });
        e.motion = Some(crate::encounter::movement(GUARD, hp, 0).0);
        e.following = Following::Unknown;
        e.follow_remaining = 0;
        w.now += 50;
        w.npc_step();
        let distance = w.entities[&guard].pos.distance(home);
        assert!(
            (distance - crate::encounter::nr_speed(0.2) / 1.5 * 0.05).abs() < 0.00001,
            "hp {hp}: idle motor moved {distance}"
        );
    }
}

#[test]
fn guard_volley_boundaries_follow_reference_geometry_and_timing() {
    use crate::encounter::{Motion, attacks, movement};
    for (hp, age, counts) in [
        (3300, 0, [3, 6, 0]),
        (3299, 0, [3, 6, 3]),
        (3299, 1000, [3, 6, 0]),
        (3299, 2000, [3, 6, 3]),
    ] {
        let shots = attacks::schedule(GUARD, hp, age).unwrap().snapshot(0.);
        for (projectile, count) in counts.into_iter().enumerate() {
            assert_eq!(
                shots
                    .iter()
                    .filter(|(index, _)| usize::from(*index) == projectile)
                    .count(),
                count
            );
        }
    }
    let volley = attacks::schedule(GUARD, 5500, 0).unwrap().snapshot(0.);
    let first_ring_angle = volley
        .iter()
        .find(|(projectile, _)| *projectile == 1)
        .unwrap()
        .1;
    assert!((first_ring_angle - (-150f32).to_radians()).abs() < 0.00001);
    assert!(matches!(
        movement(GUARD, 3300, 0).0,
        Motion::WanderHome { .. }
    ));
    assert!(matches!(
        movement(GUARD, 3299, 0).0,
        Motion::Follow {
            duration: 0,
            cooldown: 0,
            ..
        }
    ));
}
