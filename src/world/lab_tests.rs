use super::*;
use crate::{
    condition,
    dungeon::lab,
    encounter::{Anchor, Motion},
};

fn arena() -> (World, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = lab::generate(&content, 12351).unwrap();
    let world = World::new(content, Arc::new(map)).unwrap();
    let id = world.actors().find(|e| e.kind == lab::CREATION).unwrap().id;
    (world, id)
}

#[test]
fn creation_door_requires_boss_and_every_coil_in_either_order() {
    for boss_first in [false, true] {
        let (mut w, id) = arena();
        let home = w.entities[&id].home;
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, None);
        assert!(matches!(w.lab_movement(id).0, Motion::Stationary));
        let coils: Vec<_> = w
            .actors()
            .filter(|e| e.kind == lab::TESLA)
            .map(|e| e.id)
            .collect();
        assert!(!coils.is_empty());
        if boss_first {
            w.cleared = true;
        }
        for coil in coils {
            w.lab_mechanics(id, Some(home), 0).unwrap();
            assert_eq!(w.entities[&id].stage, None);
            assert!(w.entities.values().any(|e| e.kind == lab::EMERGENCY));
            w.remove_entity(coil);
        }
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, boss_first.then_some(1));
        w.cleared = true;
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(1));
        assert!(!w.entities.values().any(|e| e.kind == lab::EMERGENCY));
    }
}

#[test]
fn creation_attacks_and_pool_cycle_preserve_identity_and_damage() {
    let (mut w, id) = arena();
    let home = w.entities[&id].home;
    w.entities.get_mut(&id).unwrap().stage = Some(1);
    let before = w.entities.len();
    let hp = w.entities[&id].hp;
    w.entities.get_mut(&id).unwrap().damage.insert(123, 500);
    let shots = |w: &World, projectile, count| {
        let shots = w.lab_shots(id, home);
        assert_eq!(shots.len(), count);
        assert!(
            shots
                .iter()
                .all(|(index, angle)| *index == projectile && angle.is_finite())
        );
    };
    shots(&w, 1, 3);
    // The pools of the Creation's own room, wherever the generated layout puts it.
    let pool = |w: &World, kind| {
        w.actors()
            .filter(|e| e.kind == kind)
            .map(|e| e.pos)
            .min_by(|a, b| home.distance(*a).total_cmp(&home.distance(*b)))
            .unwrap()
    };
    let (green, blue) = (pool(&w, lab::HEXXER), pool(&w, lab::DEHEXXER));
    for (elapsed, stage, projectile, count) in [
        (10000, 3, 0, 0),
        (0, 4, 6, 1),
        (16000, 5, 0, 0),
        (0, 6, 5, 8),
        (1000, 7, 4, 8),
        (1000, 1, 1, 3),
    ] {
        w.lab_mechanics(id, Some(home), elapsed).unwrap();
        assert_eq!(w.entities[&id].stage, Some(stage));
        shots(&w, projectile, count);
        assert_ne!(
            w.entities[&id].condition & condition::bit(condition::INVINCIBLE),
            0
        );
    }
    for _ in 0..3 {
        let e = w.entities.get_mut(&id).unwrap();
        e.pos = green;
        e.goal = Some(e.pos);
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(2));
        assert!(w.entities[&id].goal.is_none());
        assert_eq!(
            w.entities[&id].condition & condition::bit(condition::INVINCIBLE),
            0
        );
        shots(&w, 0, 0);
        w.entities.get_mut(&id).unwrap().pos = blue;
        w.lab_mechanics(id, Some(home), 9999).unwrap();
        assert_eq!(w.entities[&id].stage, Some(2));
        w.now += 10000;
        assert!(matches!(
            w.lab_movement(id).0,
            Motion::Protect {
                anchor: Anchor::Kind(lab::DEHEXXER),
                ..
            }
        ));
        w.entities.get_mut(&id).unwrap().pos = blue;
        w.lab_mechanics(id, Some(home), 10000).unwrap();
        assert_eq!(w.entities[&id].stage, Some(8));
        shots(&w, 1, 3);
        w.entities.get_mut(&id).unwrap().pos = home;
        w.lab_mechanics(id, Some(home), 0).unwrap();
        assert_eq!(w.entities[&id].stage, Some(4));
        shots(&w, 6, 1);
        w.entities.get_mut(&id).unwrap().stage = Some(1);
    }
    assert_eq!(w.entities.len(), before);
    assert_eq!(w.entities[&id].hp, hp);
    assert_eq!(w.entities[&id].damage[&123], 500);
}

#[test]
fn lab_ordinary_variants_use_selected_projectiles_and_bounded_bursts() {
    let (mut w, _) = arena();
    // Open floor south of Dr Terrible in the generated boss room.
    let boss = w.actors().find(|e| e.kind == lab::BOSS).unwrap().pos;
    let target = Pos {
        x: boss.x,
        y: boss.y + 3.,
    };
    for (kind, counts) in [
        (0x979, vec![(0, 2)]),
        (0x981, vec![(0, 1)]),
        (0x982, vec![(0, 3), (1, 4)]),
        (0x990, vec![(0, 1)]),
        (0x991, vec![(0, 1)]),
    ] {
        let id = w.spawn_entity(kind, target, vec![]).unwrap();
        let shots = w.lab_shots(id, target);
        assert_eq!(shots.len(), counts.iter().map(|(_, n)| n).sum::<usize>());
        for (index, count) in counts {
            assert_eq!(shots.iter().filter(|(i, _)| *i == index).count(), count);
            assert!(w.content.objects[&kind].projectiles.contains_key(&index));
        }
        w.remove_entity(id);
    }
    let enforcer = w.spawn_entity(0x980, target, vec![]).unwrap();
    for (elapsed, stage, index, count) in [
        (10000, 1, 1, 1),
        (3000, 2, 1, 1),
        (0, 3, 0, 0),
        (2000, 4, 2, 8),
    ] {
        if stage == 3 {
            w.entities.get_mut(&enforcer).unwrap().hp = 1;
        }
        w.lab_mechanics(enforcer, Some(target), elapsed).unwrap();
        assert_eq!(w.entities[&enforcer].stage, Some(stage));
        let shots = w.lab_shots(enforcer, target);
        assert_eq!(shots.len(), count);
        assert!(shots.iter().all(|(i, _)| *i == index));
    }
    assert_eq!(*w.entities[&enforcer].expires, Some(w.now + 50));
}

#[tokio::test]
async fn lab_cage_children_do_not_duplicate_boss_rewards_or_clear() {
    let (mut w, _) = arena();
    w.name = "Mad Lab".into();
    w.encounter = true;
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
    let boss = w.actors().find(|e| e.kind == lab::BOSS).unwrap().id;
    let home = w.entities[&boss].home;
    let mut players = Vec::new();
    for name in ["contributor", "bystander"] {
        let account = store.list(name.into()).await.unwrap();
        let character = store
            .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let id = w.enter(character, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = home;
        players.push(id);
    }
    w.lab_mechanics(boss, Some(home), 0).unwrap();
    w.lab_mechanics(boss, Some(home), 20000).unwrap();
    let children: Vec<_> = w
        .actors()
        .filter(|e| e.parent == Some(boss) && matches!(e.kind, 0x1e23..=0x1e25))
        .map(|e| e.id)
        .collect();
    assert_eq!(children.len(), 4);
    w.lab_mechanics(boss, Some(home), 0).unwrap();
    assert_eq!(
        w.actors()
            .filter(|e| matches!(e.kind, 0x1e23..=0x1e25))
            .count(),
        4
    );
    let contributor = &w.players[&players[0]].character;
    let owner = contributor.account;
    w.entities
        .get_mut(&boss)
        .unwrap()
        .damage
        .insert(contributor.id, 1);
    // Comparator threshold is integer damage, not 32% HP: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/loot/Loots.cs#L106
    w.defeat_entity(boss).unwrap();
    assert!(w.cleared);
    assert_eq!(w.players[&players[0]].clears_queued.iter().sum::<u64>(), 1);
    assert_eq!(w.players[&players[1]].clears_queued.iter().sum::<u64>(), 0);
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    assert_eq!(w.entities[&exit].kind, 0x704);
    assert!(w.entities[&exit].pos.distance(home) < 2.1);
    let bags: Vec<_> = w.bag_expiry.keys().copied().collect();
    assert!(bags.iter().any(|id| w.entities[id].items.contains(&0xa35)));
    assert!(
        bags.iter()
            .all(|id| w.entities[id].extra_stats.contains(&(54, owner)))
    );
    for id in children {
        assert_eq!(*w.entities[&id].parent, None);
        w.defeat_entity(id).unwrap();
    }
    assert_eq!(w.bag_expiry.len(), bags.len());
    assert_eq!(w.clear_exit, Some(exit));
    assert_eq!(w.players[&players[0]].clears_queued.iter().sum::<u64>(), 1);
}

#[tokio::test]
async fn horrific_creation_guarantees_defense_potion_to_each_contributor() {
    let (mut w, creation) = arena();
    w.name = "Mad Lab".into();
    w.encounter = true;
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
    let home = w.entities[&creation].home;
    let mut ids = Vec::new();
    for name in ["top", "chip", "bystander"] {
        let account = store.list(name.into()).await.unwrap();
        let character = store
            .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let id = w.enter(character, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = home;
        ids.push(id);
    }
    let top = w.players[&ids[0]].character.id;
    let chip = w.players[&ids[1]].character.id;
    let owners = [
        w.players[&ids[0]].character.account,
        w.players[&ids[1]].character.account,
    ];
    w.entities
        .get_mut(&creation)
        .unwrap()
        .damage
        .insert(top, 50);
    w.entities
        .get_mut(&creation)
        .unwrap()
        .damage
        .insert(chip, 1);
    w.defeat_entity(creation).unwrap();
    assert!(!w.cleared);
    let bags: Vec<_> = w.bag_expiry.keys().copied().collect();
    assert_eq!(bags.len(), 2);
    for account in owners {
        let bag = bags
            .iter()
            .find(|id| w.entities[id].extra_stats.contains(&(54, account)))
            .unwrap();
        assert_eq!(
            w.entities[bag]
                .items
                .iter()
                .filter(|item| **item == 0xa20)
                .count(),
            1
        );
    }
}

#[test]
fn lab_pool_remedy_is_actor_driven_source_aware_and_player_local() {
    let (mut w, first) = super::movement_tests::fixture(0xa7, false);
    let pool = condition::bit(condition::HEXED) | condition::bit(condition::SPEEDY) | STUNNED;
    let mut other = w.players[&first].character.clone();
    other.id = 2;
    other.account = 2;
    let second = w.enter(other, None).unwrap();
    w.players.get_mut(&second).unwrap().pos.x = 12.5;
    w.players.get_mut(&first).unwrap().lab_conditions = pool;
    w.lab_step();
    assert_eq!(w.players[&first].lab_conditions, pool);
    w.players.get_mut(&first).unwrap().lab_conditions = 0;
    let green = w.players[&first].pos;
    w.spawn_entity(lab::HEXXER, green, vec![]).unwrap();
    w.lab_step();
    assert_eq!(w.players[&first].conditions(w.now), pool);
    assert_eq!(w.players[&second].conditions(w.now), 0);
    w.players.get_mut(&first).unwrap().pos.x = 12.5;
    w.lab_step();
    assert_eq!(w.players[&first].lab_conditions, pool);
    let blue = w.players[&first].pos;
    w.spawn_entity(lab::DEHEXXER, blue, vec![]).unwrap();
    w.players.get_mut(&second).unwrap().pos = green;
    for effect in [
        condition::HEXED,
        condition::STUNNED,
        condition::SPEEDY,
        condition::QUIET,
    ] {
        w.players
            .get_mut(&first)
            .unwrap()
            .statuses
            .insert(effect, (0, 1000));
    }
    w.lab_step();
    let cured = &w.players[&first];
    assert_eq!(cured.lab_conditions, 0);
    assert!(!cured.statuses.contains_key(&condition::HEXED));
    assert_eq!(
        cured.conditions(w.now),
        STUNNED | condition::bit(condition::SPEEDY) | condition::bit(condition::QUIET)
    );
    assert_eq!(w.players[&second].lab_conditions, pool);
    w.now = 1000;
    w.lab_step();
    assert_eq!(w.players[&first].conditions(w.now), 0);
    assert_eq!(w.players[&second].conditions(w.now), pool);
}

#[test]
fn lab_pools_leave_the_l_room_walkway_dry() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = lab::generate(&content, 12351).unwrap();
    let mut w = World::new(content, Arc::new(map)).unwrap();
    let mut character = super::movement_tests::fixture(0xa9, false)
        .0
        .players
        .values()
        .next()
        .unwrap()
        .character
        .clone();
    character.id = 7;
    character.account = 7;
    let id = w.enter(character, None).unwrap();
    let ground = |w: &World, x: i32, y: i32| w.map.cells[(y * w.map.width + x) as usize].ground;
    // A grate walkway tile beside a green vat, and a Hexxer on that vat tile, so the walkway lies in its 3x3.
    let (dry, wet) = (0..w.map.height)
        .flat_map(|y| (0..w.map.width).map(move |x| (x, y)))
        .filter(|&(x, y)| ground(&w, x, y) == 0xd2)
        .find_map(|(x, y)| {
            [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .into_iter()
                .find(|&(a, b)| ground(&w, a, b) == lab::BAD_VAT)
                .map(|vat| ((x, y), vat))
        })
        .expect("a grate walkway beside a Hexxer vat");
    let hexxer = Pos {
        x: wet.0 as f32 + 0.5,
        y: wet.1 as f32 + 0.5,
    };
    w.spawn_entity(lab::HEXXER, hexxer, vec![]).unwrap();
    for ((x, y), hexed) in [(dry, false), (wet, true)] {
        let player = w.players.get_mut(&id).unwrap();
        player.lab_conditions = 0;
        player.pos = Pos {
            x: x as f32 + 0.5,
            y: y as f32 + 0.5,
        };
        w.lab_step();
        assert_eq!(w.players[&id].lab_conditions != 0, hexed, "({x}, {y})");
    }
}

#[test]
fn lab_pool_blue_precedence_pause_and_terminal_player_cleanup() {
    let (mut w, id) = super::movement_tests::fixture(0xa9, false);
    let pool = condition::bit(condition::HEXED) | condition::bit(condition::SPEEDY) | STUNNED;
    let position = w.players[&id].pos;
    w.lab_step();
    assert_eq!(w.players[&id].conditions(w.now), 0);
    w.spawn_entity(lab::HEXXER, position, vec![]).unwrap();
    let blue = w.spawn_entity(lab::DEHEXXER, position, vec![]).unwrap();
    w.lab_step();
    assert_eq!(w.players[&id].conditions(w.now), 0);
    w.remove_entity(blue);
    w.players.get_mut(&id).unwrap().paused = true;
    w.lab_step();
    assert_eq!(w.players[&id].lab_conditions, 0);
    w.players.get_mut(&id).unwrap().paused = false;
    w.lab_step();
    assert_eq!(w.players[&id].lab_conditions, pool);
    for terminal in 0..3 {
        let player = w.players.get_mut(&id).unwrap();
        player.lab_conditions = pool;
        player.character.alive = terminal != 0;
        player.death_pending = terminal == 1;
        player.departing = terminal == 2;
        player.statuses.insert(condition::QUIET, (0, 1000));
        w.lab_step();
        assert_eq!(w.players[&id].lab_conditions, 0);
        assert_eq!(
            w.players[&id].conditions(w.now),
            condition::bit(condition::QUIET)
        );
    }
}

fn doctor(w: &World) -> (i32, Pos) {
    let id = w.actors().find(|e| e.kind == lab::BOSS).unwrap().id;
    (id, w.entities[&id].home)
}

/// Runs Dr Terrible's mechanics at `now` with his real phase clock, then lands due tosses.
fn tick(w: &mut World, boss: i32, now: u64) {
    w.now = now;
    let elapsed = now - w.entities[&boss].activated.unwrap_or(now);
    w.lab_mechanics(boss, None, elapsed).unwrap();
    w.lab_step();
}

fn listener(w: &mut World, n: i32, pos: Option<Pos>) -> (i32, mpsc::Receiver<Packet>) {
    let mut character = Character::fresh(w.content.get(crate::content::WIZARD).unwrap()).unwrap();
    character.id = n;
    character.account = n;
    character.name = format!("Listener{n}");
    let id = w.enter(character, None).unwrap();
    let (packets, inbox) = mpsc::channel(256);
    let (lost, _) = watch::channel(None);
    let p = w.players.get_mut(&id).unwrap();
    p.tx = Some(Output {
        trace: None,
        packets,
        lost,
    });
    if let Some(pos) = pos {
        p.pos = pos;
    }
    (id, inbox)
}

fn said(inbox: &mut mpsc::Receiver<Packet>) -> Vec<(String, String)> {
    std::iter::from_fn(|| inbox.try_recv().ok())
        .filter(|p| p.id == wire::TEXT)
        .map(|p| {
            let mut r = wire::Reader::new(&p.body);
            let name = r.utf().unwrap();
            r.i32().unwrap();
            r.i32().unwrap();
            r.u8().unwrap();
            r.utf().unwrap();
            (name, r.utf().unwrap())
        })
        .collect()
}

#[test]
fn dr_terrible_cycles_twenty_seconds_out_with_gas_and_twenty_in_his_bubble() {
    let (mut w, _) = arena();
    let (boss, home) = doctor(&w);
    let spawners: Vec<_> = w
        .actors()
        .filter(|e| (0x993..=0x996).contains(&e.kind))
        .map(|e| (e.id, e.kind))
        .collect();
    assert_eq!(spawners.len(), 4);
    for cycle in 0..5u64 {
        let out = cycle % 4 * 2;
        for (now, stage) in [(0, out), (19999, out), (20000, out + 1), (39999, out + 1)] {
            tick(&mut w, boss, cycle * 40000 + now);
            let bubble = stage % 2 == 1;
            assert_eq!(w.entities[&boss].stage, Some(stage));
            assert_eq!(
                w.entities[&boss].condition & condition::bit(condition::INVINCIBLE) != 0,
                bubble
            );
            assert_eq!(
                matches!(
                    w.lab_movement(boss).0,
                    Motion::Protect {
                        anchor: Anchor::Home,
                        ..
                    }
                ),
                bubble
            );
            // Gas only while he is out: 18 shots 20 degrees apart from all but the rotating safe spawner.
            for &(id, kind) in &spawners {
                let shots = w.lab_shots(id, home);
                if bubble || kind == 0x993 + (cycle % 4) as u16 {
                    assert!(shots.is_empty());
                } else {
                    assert_eq!(shots.len(), 18);
                    assert!(
                        shots
                            .windows(2)
                            .all(|s| ((s[1].1 - s[0].1).to_degrees() - 20.).abs() < 1e-3)
                    );
                }
            }
        }
    }
}

#[test]
fn dr_terrible_cages_release_one_minion_each_while_fewer_than_four_live() {
    let (mut w, _) = arena();
    let (boss, home) = doctor(&w);
    let cages: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == lab::CAGE)
        .map(|e| e.pos)
        .collect();
    // Two cages on each side wall; none at the north door or the south entrance.
    assert_eq!(cages.len(), 4);
    assert!(
        cages
            .iter()
            .all(|c| (c.x - home.x).abs() == 9. && (c.y - home.y).abs() == 1.)
    );
    let caged = |w: &World| -> BTreeMap<i32, Pos> {
        w.actors()
            .filter(|e| e.parent == Some(boss) && matches!(e.kind, 0x1e23..=0x1e25))
            .map(|e| (e.id, e.home))
            .collect()
    };
    let bubble = |w: &mut World, k: u64| {
        tick(w, boss, 40000 * k);
        tick(w, boss, 40000 * k + 20000);
        assert_eq!(w.entities[&boss].stage.unwrap() % 2, 1);
    };
    bubble(&mut w, 0);
    let first = caged(&w);
    assert_eq!(first.len(), 4);
    for cage in &cages {
        assert_eq!(
            first.values().filter(|h| h.distance(*cage) < 0.1).count(),
            1
        );
    }
    bubble(&mut w, 1);
    assert!(caged(&w).keys().eq(first.keys()));
    let (&dead, &cage) = first.iter().next().unwrap();
    w.remove_entity(dead);
    bubble(&mut w, 2);
    let second = caged(&w);
    assert_eq!(second.len(), 4);
    assert!(!second.contains_key(&dead));
    assert_eq!(
        second.values().filter(|h| h.distance(cage) < 0.1).count(),
        1
    );
    let extra = Pos {
        x: home.x - 9.,
        y: home.y + 3.,
    };
    w.spawn_entity(lab::CAGE, extra, vec![]).unwrap();
    bubble(&mut w, 3);
    assert!(caged(&w).keys().eq(second.keys()));
    let mut kinds = BTreeSet::new();
    for k in 4..14 {
        for id in caged(&w).into_keys() {
            w.remove_entity(id);
        }
        bubble(&mut w, k);
        let released = caged(&w);
        assert_eq!(released.len(), 4);
        assert_eq!(
            released
                .values()
                .map(|h| (h.x.to_bits(), h.y.to_bits()))
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
        kinds.extend(released.keys().map(|id| w.entities[id].kind));
    }
    assert_eq!(kinds, BTreeSet::from([0x1e23, 0x1e24, 0x1e25]));
}

#[test]
fn dr_terrible_experiments_heal_him_inside_his_bubble_up_to_full() {
    let (mut w, _) = arena();
    let (boss, home) = doctor(&w);
    tick(&mut w, boss, 0);
    tick(&mut w, boss, 20000);
    assert_ne!(
        w.entities[&boss].condition & condition::bit(condition::INVINCIBLE),
        0
    );
    let maximum = w.content.objects[&lab::BOSS].hp;
    w.entities.get_mut(&boss).unwrap().hp = maximum - 700;
    let healer = w
        .spawn_entity(
            0x1e23,
            Pos {
                x: home.x + 4.,
                y: home.y,
            },
            vec![],
        )
        .unwrap();
    w.entities
        .get_mut(&healer)
        .unwrap()
        .parent
        .set(Some(boss), healer, &mut w.timed_ids);
    assert_eq!(
        w.lab_movement(healer),
        (
            Motion::Protect {
                anchor: Anchor::Parent,
                distance: 1.
            },
            lab::profile(0x1e23).speed
        )
    );
    let (player, mut inbox) = listener(&mut w, 1, Some(home));
    w.players
        .get_mut(&player)
        .unwrap()
        .known
        .extend([boss, healer]);
    let hp = |w: &World| w.entities[&boss].hp;
    w.lab_mechanics(healer, None, 0).unwrap();
    assert_eq!(hp(&w), maximum - 700);
    w.entities.get_mut(&healer).unwrap().pos.x = home.x + 2.;
    for (wait, expected) in [(0, 200), (0, 200), (2499, 200), (1, 0), (2500, 0)] {
        w.now += wait;
        w.lab_mechanics(healer, None, 0).unwrap();
        assert_eq!(hp(&w), maximum - expected);
    }
    let shown: Vec<_> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter(|p| p.id == wire::NOTIFICATION)
        .map(|p| {
            let mut r = wire::Reader::new(&p.body);
            assert_eq!(r.i32().unwrap(), boss);
            r.utf().unwrap()
        })
        .collect();
    assert_eq!(shown, ["+500", "+200"]);
}

#[test]
fn dr_terrible_tosses_every_eight_seconds_at_players_within_ten_tiles() {
    let (mut w, _) = arena();
    let (boss, home) = doctor(&w);
    let near = [
        Pos {
            x: home.x,
            y: home.y + 9.,
        },
        Pos {
            x: home.x - 6.,
            y: home.y,
        },
    ];
    let far = Pos {
        x: home.x + 7.5,
        y: home.y + 7.5,
    };
    let mut players = Vec::new();
    for (n, pos) in near.iter().chain([&far]).enumerate() {
        assert!(w.walkable(*pos));
        players.push(listener(&mut w, n as i32 + 1, Some(*pos)).0);
    }
    let mut seen = BTreeSet::new();
    let mut landed = Vec::new();
    let mut run = |w: &mut World, span: std::ops::Range<u64>| {
        for now in span.step_by(250) {
            tick(w, boss, now);
            for e in w.actors().filter(|e| {
                e.parent == Some(boss) && matches!(e.kind, 0x978 | 0x12aa) && seen.insert(e.id)
            }) {
                assert!(near.iter().any(|p| p.distance(e.pos) < 0.01));
                landed.push((now, e.kind));
            }
        }
    };
    run(&mut w, 0..60000);
    for id in &players[..2] {
        w.players.get_mut(id).unwrap().pos = far;
    }
    run(&mut w, 60000..100000);
    // A potion, a turret and a potion per 20 s gas phase; nothing from the bubble or beyond 10 tiles.
    assert_eq!(
        landed,
        [
            (1500, 0x978),
            (9500, 0x12aa),
            (17500, 0x978),
            (41500, 0x978),
            (49500, 0x12aa),
            (57500, 0x978)
        ]
    );
}

#[test]
fn dr_terrible_rage_stays_out_without_gas_or_turrets() {
    let (mut w, _) = arena();
    let (boss, home) = doctor(&w);
    listener(
        &mut w,
        1,
        Some(Pos {
            x: home.x - 6.,
            y: home.y,
        }),
    );
    tick(&mut w, boss, 0);
    w.entities.get_mut(&boss).unwrap().hp = w.content.objects[&lab::BOSS].hp / 5 - 1;
    let mut landed = Vec::new();
    for now in (250..=60000).step_by(250) {
        let before = w.actors().filter(|e| e.parent == Some(boss)).count();
        tick(&mut w, boss, now);
        let e = &w.entities[&boss];
        assert_eq!(e.stage, Some(8));
        assert_eq!(e.condition & condition::bit(condition::INVINCIBLE), 0);
        assert!(
            w.actors()
                .filter(|e| (0x993..=0x996).contains(&e.kind))
                .all(|e| w.lab_shots(e.id, home).is_empty())
        );
        if w.actors().filter(|e| e.parent == Some(boss)).count() > before {
            landed.push(now);
        }
    }
    assert_eq!(
        landed,
        [1500, 1750, 9750, 17750, 25750, 33750, 41750, 49750, 57750]
    );
    assert!(!w.actors().any(|e| e.kind == 0x12aa));
    assert_eq!(
        w.actors()
            .filter(|e| e.parent == Some(boss) && e.kind == 0x978)
            .count(),
        9
    );
}

#[test]
fn lab_bosses_taunt_on_phases_door_and_death() {
    let (mut w, creation) = arena();
    let (boss, home) = doctor(&w);
    let (_, mut near) = listener(&mut w, 1, Some(home));
    let (away_id, mut away) = listener(&mut w, 2, None);
    assert!(w.players[&away_id].pos.distance(home) >= 20.);
    fn heard(inbox: &mut mpsc::Receiver<Packet>, speaker: &str, lines: &[&str]) {
        let lines_heard = said(inbox);
        assert_eq!(lines_heard.len(), lines.len().min(1));
        for (name, text) in lines_heard {
            assert_eq!(name, speaker);
            assert!(lines.contains(&text.as_str()), "unexpected taunt {text}");
        }
    }
    tick(&mut w, boss, 0);
    heard(&mut near, "#Dr Terrible", super::lab::OUT_TAUNTS);
    tick(&mut w, boss, 9999);
    heard(&mut near, "#Dr Terrible", &[]);
    tick(&mut w, boss, 10000);
    heard(&mut near, "#Dr Terrible", super::lab::OUT_TAUNTS);
    tick(&mut w, boss, 20000);
    heard(&mut near, "#Dr Terrible", super::lab::BUBBLE_TAUNTS);
    tick(&mut w, boss, 35000);
    heard(&mut near, "#Dr Terrible", &[]);
    heard(&mut away, "#Dr Terrible", &[]);
    // Horrific Creation is heard everywhere in the Lab, not only near its sealed room.
    let defeated = w.remove_entity(boss);
    w.lab_death(&defeated).unwrap();
    for inbox in [&mut near, &mut away] {
        heard(inbox, "#Horrific Creation", &["WHO KILL MASTER? RAHHHH!"]);
    }
    w.cleared = true;
    let coils: Vec<_> = w
        .actors()
        .filter(|e| e.kind == lab::TESLA)
        .map(|e| e.id)
        .collect();
    for coil in coils {
        w.remove_entity(coil);
    }
    let position = w.entities[&creation].pos;
    w.lab_mechanics(creation, None, 0).unwrap();
    for inbox in [&mut near, &mut away] {
        heard(
            inbox,
            "#Horrific Creation",
            &["Me door is open. Come let me crush you!"],
        );
    }
    // Fight taunts need a present player and come at most every 10 s.
    w.lab_mechanics(creation, Some(position), 0).unwrap();
    w.now += 10000;
    w.lab_mechanics(creation, None, 0).unwrap();
    heard(&mut near, "#Horrific Creation", &[]);
    w.lab_mechanics(creation, Some(position), 0).unwrap();
    heard(&mut away, "#Horrific Creation", super::lab::CREATION_TAUNTS);
}

#[test]
fn dungeon_escaped_experiment_heals_a_crusher_within_three_tiles() {
    let (mut w, _) = arena();
    let (_, home) = doctor(&w);
    let crusher = w.spawn_entity(0x983, home, vec![]).unwrap();
    let maximum = w.content.objects[&0x983].hp;
    w.entities.get_mut(&crusher).unwrap().hp = maximum - 1000;
    let healer = w
        .spawn_entity(
            0x981,
            Pos {
                x: home.x + 3.,
                y: home.y,
            },
            vec![],
        )
        .unwrap();
    for (wait, step, expected) in [
        (0, 0., 700),
        (0, 0., 700),
        (2500, 0., 400),
        (2500, 0.5, 400),
    ] {
        w.now += wait;
        w.entities.get_mut(&healer).unwrap().pos.x += step;
        w.lab_mechanics(healer, None, 0).unwrap();
        assert_eq!(w.entities[&crusher].hp, maximum - expected);
    }
}

#[test]
fn enforcer_death_releases_one_to_three_mini_bots() {
    let (mut w, _) = arena();
    let (_, home) = doctor(&w);
    let mut counts = BTreeSet::new();
    for _ in 0..60 {
        let enforcer = w.spawn_entity(0x982, home, vec![]).unwrap();
        let defeated = w.remove_entity(enforcer);
        w.lab_death(&defeated).unwrap();
        let bots: Vec<_> = w
            .actors()
            .filter(|e| e.kind == 0x979 && e.pos.distance(home) < 0.5)
            .map(|e| e.id)
            .collect();
        counts.insert(bots.len());
        for id in bots {
            w.remove_entity(id);
        }
    }
    assert_eq!(counts, BTreeSet::from([1, 2, 3]));
}

#[tokio::test]
async fn lab_minions_leave_public_potions_and_owner_only_equipment() {
    let (mut w, _) = arena();
    w.name = "Mad Lab".into();
    w.encounter = true;
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
    let (_, home) = doctor(&w);
    let account = store.list("minion-hunter".into()).await.unwrap();
    let character = store
        .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let player = w.enter(character, None).unwrap();
    w.players.get_mut(&player).unwrap().pos = home;
    let (character, owner) = {
        let c = &w.players[&player].character;
        (c.id, c.account)
    };
    let spot = Pos {
        x: home.x,
        y: home.y + 3.,
    };
    let (mut public, mut cage_public, mut owned) =
        (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for seed in 0..300u64 {
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        // Dungeon and cage Experiments and Cyborgs all carry owner-only equipment lists.
        let kind = [0x981, 0x1e23, 0x980, 0x1e24][seed as usize % 4];
        let id = w.spawn_entity(kind, spot, vec![]).unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .damage
            .insert(character, 100);
        w.defeat_entity(id).unwrap();
        for bag in w.bag_expiry.keys().copied().collect::<Vec<_>>() {
            let bag = w.remove_entity(bag);
            let items: Vec<_> = bag
                .items
                .iter()
                .copied()
                .filter(|item| *item >= 0)
                .collect();
            if bag.extra_stats.contains(&(54, owner)) {
                assert_ne!(bag.kind, 0x500, "{kind:#x}: owner-only bags are coloured");
                assert!(items.iter().all(|item| !matches!(item, 0xa22 | 0xa23)));
                owned.extend(items);
            } else {
                assert!(bag.extra_stats.iter().all(|(stat, _)| *stat != 54));
                assert_eq!(bag.kind, 0x500, "{kind:#x}: public bags are brown");
                assert!(items.iter().all(|item| matches!(item, 0xa22 | 0xa23)));
                if (0x1e23..=0x1e25).contains(&kind) {
                    cage_public.extend(items.iter().copied());
                }
                public.extend(items);
            }
        }
    }
    assert_eq!(public, BTreeSet::from([0xa22, 0xa23]));
    // Dr Terrible's cage minions leave Magic potions only.
    assert_eq!(cage_public, BTreeSet::from([0xa23]));
    assert!(!owned.is_empty());
}

#[test]
fn acid_room_turrets_fire_on_their_own_beats_not_in_waves() {
    let (mut w, _) = arena();
    w.encounter = true;
    let turrets: BTreeMap<_, _> = w
        .actors()
        .filter(|e| matches!(e.kind, 0x990 | 0x991))
        .map(|e| (e.id, (e.kind, e.home)))
        .collect();
    assert_eq!(turrets.len(), 54);
    let xs: Vec<_> = turrets.values().map(|(_, p)| p.x).collect();
    let ys: Vec<_> = turrets.values().map(|(_, p)| p.y).collect();
    let mid = |v: &[f32]| {
        (v.iter().copied().fold(f32::MAX, f32::min) + v.iter().copied().fold(f32::MIN, f32::max))
            / 2.
    };
    let (fixture, id) = super::movement_tests::fixture(0x18, false);
    let player = w
        .enter(fixture.players[&id].character.clone(), None)
        .unwrap();
    let p = w.players.get_mut(&player).unwrap();
    p.pos = Pos {
        x: mid(&xs).floor() + 0.5,
        y: mid(&ys).floor() + 0.5,
    };
    p.statuses.insert(condition::INVINCIBLE, (0, u64::MAX));
    // Ten seconds in the middle of the acid room: per turret one Green Star every 3 s on its own beat.
    let mut shots: BTreeMap<i32, Vec<u64>> = BTreeMap::new();
    for _ in 0..200 {
        w.step();
        let now = w.now;
        let fired: Vec<_> = w
            .bullets
            .iter()
            .filter(|b| b.born == now && turrets.contains_key(&b.owner))
            .map(|b| b.owner)
            .collect();
        // Never a wave: at most one turret of each wall in any tick.
        assert!(fired.len() <= 2, "{} turrets fired at {now}", fired.len());
        for id in fired {
            shots.entry(id).or_default().push(now);
        }
    }
    assert!(shots.len() >= 20, "only {} turrets reached", shots.len());
    for times in shots.values() {
        assert!(times.windows(2).all(|t| t[1] - t[0] == 3000), "{times:?}");
    }
    // Wall neighbours stand at least a third of a second apart in the beat.
    for (id, times) in &shots {
        let (kind, home) = turrets[id];
        for (other, theirs) in &shots {
            let (k, h) = turrets[other];
            if k == kind && (h.y - home.y - 1.).abs() < 0.1 {
                let gap = (times[0] % 3000).abs_diff(theirs[0] % 3000);
                assert!(
                    gap.min(3000 - gap) >= 300,
                    "{kind:#x} rows {} and {}",
                    home.y,
                    h.y
                );
            }
        }
    }
}

#[test]
fn creation_room_stays_dark_and_refuses_blinks_until_its_walls_fall() {
    use super::sight::tests::{blink, refused};
    const CLOAK: u16 = 0xa5a;
    let (mut w, creation) = arena();
    w.name = lab::NAME.into();
    let walls: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == lab::EMERGENCY)
        .map(|e| e.pos)
        .collect();
    assert_eq!(walls.len(), 4);
    assert!(w.content.objects[&lab::EMERGENCY].blocks_sight);
    let x = walls.iter().map(|p| p.x).sum::<f32>() / 4.;
    // A Rogue in Dr Terrible's room just south of the sealed neck; a free tile beside the Horrific Creation.
    let outside = Pos {
        x: x.floor() + 0.5,
        y: walls[0].y + 1.,
    };
    let h = w.entities[&creation].pos;
    let inside = Pos {
        x: h.x,
        y: h.y + 2.,
    };
    assert!(w.walkable(outside) && w.walkable(inside) && outside.distance(inside) < 20.);
    let (fixture, id) = super::movement_tests::fixture(0x18, false);
    let player = w
        .enter(fixture.players[&id].character.clone(), None)
        .unwrap();
    w.players.get_mut(&player).unwrap().pos = outside;
    let tile = (inside.x as i16, inside.y as i16);
    w.updates(player, true).unwrap();
    let p = &w.players[&player];
    assert!(!p.tiles.contains(&tile), "the sealed room leaked terrain");
    assert!(
        !p.known.contains(&creation),
        "the sealed room leaked the Creation"
    );
    // Dr Terrible's room itself is in plain sight, to its far corners.
    let dr = w.actors().find(|e| e.kind == lab::BOSS).unwrap().home;
    let corner = (dr.x as i16 - 10, dr.y as i16);
    assert!(p.tiles.contains(&(dr.x as i16, dr.y as i16)) && p.tiles.contains(&corner));
    refused(&mut w, player, CLOAK, inside);
    // Dr Terrible dead and every coil gone: the walls drop and the room appears without the player moving.
    let coils: Vec<_> = w
        .actors()
        .filter(|e| e.kind == lab::TESLA)
        .map(|e| e.id)
        .collect();
    for coil in coils {
        w.remove_entity(coil);
    }
    w.cleared = true;
    w.lab_mechanics(creation, None, 0).unwrap();
    assert!(!w.entities.values().any(|e| e.kind == lab::EMERGENCY));
    assert_eq!(w.players[&player].tile_scan, None);
    w.updates(player, false).unwrap();
    let p = &w.players[&player];
    assert!(p.tiles.contains(&tile) && p.known.contains(&creation));
    blink(&mut w, player, CLOAK, inside).unwrap();
    let p = &w.players[&player];
    assert_eq!((p.pos.x, p.pos.y), (inside.x, inside.y));
}
