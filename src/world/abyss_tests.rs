use super::{tests, *};
use crate::{
    condition,
    dungeon::abyss::{self, BRIDGE, IDOL, MALPHAS},
};

fn fixture() -> (World, i32) {
    let mut w = tests::world();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for c in &mut Arc::make_mut(&mut w.map).cells {
        c.ground = 0x61;
        c.object = None;
    }
    let mut ch = tests::ch(&w);
    ch.base = w.content.classes[&ch.class].caps;
    ch.hp = ch.base[0];
    ch.mp = ch.base[1];
    let id = w
        .enter_at(
            ch,
            None,
            Some(Pos { x: 12.5, y: 12.5 }),
            AccountPresentation {
                guild: None,
                fame: 0,
                best_fame: BTreeMap::new(),
                credits: 0,
            },
        )
        .unwrap();
    (w, id)
}

fn attack_arena(kind: u16) -> (World, i32, i32) {
    let (mut w, player) = fixture();
    w.encounter = true;
    w.name = "Abyss of Demons".into();
    let id = w
        .spawn_entity(kind, Pos { x: 12.5, y: 20.5 }, vec![])
        .unwrap();
    let e = w.entities.get_mut(&id).unwrap();
    e.condition |= condition::bit(condition::PARALYZED);
    e.next_aux = u64::MAX / 2;
    (w, player, id)
}

fn attack_tick(w: &mut World, id: i32, now: u64) -> Vec<(u8, f32)> {
    w.bullets.clear();
    w.now = now;
    let packets = w.npc_step();
    let decoded = attack_packets(w, id, &packets);
    let children: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.parent == Some(id))
        .map(|e| e.id)
        .collect();
    for child in children {
        w.remove_entity(child);
    }
    decoded
}

fn attack_packets(w: &World, id: i32, packets: &[(i32, Vec<u8>)]) -> Vec<(u8, f32)> {
    let bullets: Vec<_> = w.bullets.iter().filter(|b| b.owner == id).collect();
    let mut decoded = vec![];
    for (_, body) in packets.iter().filter(|(owner, _)| *owner == id) {
        let mut r = crate::wire::Reader::new(body);
        let first = r.u8().unwrap();
        assert_eq!(r.i32().unwrap(), id);
        let projectile = r.u8().unwrap();
        let start = Pos {
            x: r.f32().unwrap(),
            y: r.f32().unwrap(),
        };
        let angle = r.f32().unwrap();
        let damage = i32::from(r.i16().unwrap());
        let (count, gap) = if body.len() == 25 {
            (r.u8().unwrap(), r.f32().unwrap())
        } else {
            (1, 0.)
        };
        r.done().unwrap();
        for n in 0..count {
            let bullet = bullets[decoded.len()];
            assert_eq!(bullet.id, first.wrapping_add(n));
            assert_eq!(bullet.damage, damage);
            assert_eq!(bullet.start.distance(start), 0.);
            assert!((bullet.angle - (angle + f32::from(n) * gap)).abs() < 0.00001);
            assert_eq!(
                serde_json::to_value(&bullet.spec).unwrap(),
                serde_json::to_value(
                    &w.content.objects[&bullet.source_kind].projectiles[&projectile]
                )
                .unwrap()
            );
            decoded.push((projectile, bullet.angle));
        }
    }
    assert_eq!(decoded.len(), bullets.len());
    decoded
}

fn attack_decoy(w: &mut World, player: i32) -> i32 {
    let p = w.players.get_mut(&player).unwrap();
    p.character.class = 0x324;
    p.character.base = w.content.classes[&0x324].caps;
    p.character.mp = p.character.base[1];
    p.character.items = w.content.get(0x324).unwrap().equipment.clone();
    p.character.items[1] = 0xae3;
    p.controller = Controller::Human;
    w.native_use_item(
        player,
        w.now as i32,
        Slot {
            object: player,
            slot: 1,
            kind: 0xae3,
        },
        Pos { x: 14.5, y: 20.5 },
    )
    .unwrap();
    *w.ability_state.objects.keys().next().unwrap()
}

#[test]
fn abyss_attack_phase_clocks_rearm_across_six_cycles() {
    let (mut w, _, boss) = attack_arena(MALPHAS);
    w.entities.get_mut(&boss).unwrap().activated = Some(0);
    let mut firsts = [None; 6];
    for now in (0..6 * 50200).step_by(50) {
        let cycle = now / 50200;
        let phase = abyss::phase(now);
        let age = now % 50200 - [0, 9000, 11500, 23500, 36500, 39000, 47000][phase as usize];
        let shots = attack_tick(&mut w, boss, now);
        let mut expected = vec![];
        let mut fan = |projectile, count, gap: f32, center: f32| {
            for n in 0..count {
                expected.push((
                    projectile,
                    center + (n as f32 - (count - 1) as f32 / 2.) * gap.to_radians(),
                ));
            }
        };
        match phase {
            0 if age % 900 == 0 => fan(0, 1, 0., -std::f32::consts::FRAC_PI_2 + 1f32.to_radians()),
            2 => {
                if age % 900 == 0 {
                    fan(0, 1, 0., -std::f32::consts::FRAC_PI_2 + 0.6f32.to_radians());
                }
                if age % 1200 == 0 {
                    fan(2, 6, 60., 0.);
                }
            }
            3 if age >= 1800 => {
                if (age - 1800) % 900 == 0 {
                    fan(3, 1, 0., -std::f32::consts::FRAC_PI_2);
                }
                if (age - 1800) % 700 == 0 {
                    fan(
                        4,
                        3,
                        120.,
                        (((age - 1800) / 1400 % 3) as f32 * 40. + 0.7).to_radians(),
                    );
                }
                if [3200, 7400, 11600].contains(&age) {
                    fan(3, 1, 0., -std::f32::consts::FRAC_PI_2);
                }
            }
            5 if age % 2100 == 0 => fan(0, 1, 0., -std::f32::consts::FRAC_PI_2),
            _ => {}
        }
        assert_eq!(
            shots.len(),
            expected.len(),
            "cycle={cycle} phase={phase} age={age}: {shots:?}"
        );
        for (actual, expected) in shots.iter().zip(expected) {
            assert_eq!(actual.0, expected.0);
            assert!(
                (actual.1 - expected.1).abs() < 0.00001,
                "cycle={cycle} phase={phase} age={age}: {actual:?} expected={expected:?}"
            );
        }
        if shots.iter().any(|(projectile, _)| *projectile == 2) && firsts[cycle as usize].is_none()
        {
            firsts[cycle as usize] = Some(age);
        }
        if cycle == 0 && !shots.is_empty() {
            println!("full-cycle phase={phase} age={age} emitted={shots:?}");
        }
    }
    println!("six actual full-cycle first-ring offsets: {firsts:?}");
    assert_eq!(firsts, [Some(0); 6]);
}

#[test]
fn abyss_attack_ranges_do_not_gate_fixed_rings_or_projectile_travel() {
    for (now, aimed, fixed, count) in [
        (12600, 0, 2, 6),
        (0, 0, 2, 0),
        (25300, 3, 4, 3),
        (39000, 0, 2, 0),
    ] {
        for distance in [8.01, 7.99, 19.99] {
            let (mut w, player, boss) = attack_arena(MALPHAS);
            w.entities.get_mut(&boss).unwrap().activated = Some(0);
            w.players.get_mut(&player).unwrap().pos = Pos {
                x: 12.5,
                y: 20.5 + distance,
            };
            let shots = attack_tick(&mut w, boss, now);
            println!("range now={now} distance={distance}: {shots:?}");
            assert_eq!(
                shots.iter().filter(|(p, _)| *p == aimed).count(),
                usize::from(distance <= 8.)
            );
            assert_eq!(shots.iter().filter(|(p, _)| *p == fixed).count(), count);
            for b in w.bullets.iter().filter(|b| b.owner == boss) {
                let selected = w.content.objects[&MALPHAS]
                    .projectiles
                    .values()
                    .find(|p| {
                        serde_json::to_value(*p).unwrap() == serde_json::to_value(&b.spec).unwrap()
                    })
                    .unwrap();
                assert_eq!(b.spec.lifetime, selected.lifetime);
                assert_eq!(b.spec.speed, selected.speed);
                assert!(
                    (b.position(now + selected.lifetime as u64).distance(b.start)
                        - selected.speed * selected.lifetime / 10000.)
                        .abs()
                        < 0.0001
                );
            }
        }
    }
    for (activated, distance, wakes) in [
        (false, 10.99, true),
        (false, 11.01, false),
        (true, 19.99, true),
        (true, 20.01, false),
    ] {
        let (mut w, player, boss) = attack_arena(MALPHAS);
        if activated {
            w.entities.get_mut(&boss).unwrap().activated = Some(0);
        }
        w.players.get_mut(&player).unwrap().pos = Pos {
            x: 12.5,
            y: 20.5 + distance,
        };
        attack_tick(&mut w, boss, 50);
        assert_eq!(
            w.entities[&boss].stage.is_some(),
            wakes,
            "wake activated={activated} distance={distance}"
        );
    }
}

#[test]
fn abyss_attack_guard_schedule_is_scoped_to_abyss() {
    for (name, range, interval) in [
        ("Abyss of Demons", 10., 500),
        ("Realm", 12., 1000),
        ("Snake Pit", 12., 1000),
    ] {
        for distance in [range - 0.01, range + 0.01] {
            let (mut w, player, guard) = attack_arena(0x651);
            w.name = name.into();
            w.players.get_mut(&player).unwrap().pos = Pos {
                x: 12.5,
                y: 20.5 + distance,
            };
            let mut times = vec![];
            for now in (0..=2000).step_by(50) {
                let shots = attack_tick(&mut w, guard, now);
                if shots.is_empty() {
                    continue;
                }
                assert_eq!(shots.len(), 3);
                assert!((shots[1].1 - shots[0].1 - 15f32.to_radians()).abs() < 0.00001);
                assert!(
                    w.bullets
                        .iter()
                        .filter(|b| b.owner == guard)
                        .all(|b| b.damage == 45 && b.spec.armor_piercing)
                );
                times.push(now);
            }
            println!("guard world={name} distance={distance} timestamps={times:?}");
            assert_eq!(
                times,
                if distance <= range {
                    (0..=2000).step_by(interval).collect::<Vec<_>>()
                } else {
                    vec![]
                }
            );
        }
    }
}

#[test]
fn abyss_attack_cloak_decoys_and_fixed_channels_remain_independent() {
    for (kind, expire) in [
        (MALPHAS, false),
        (IDOL, false),
        (MALPHAS, true),
        (IDOL, true),
    ] {
        let (mut w, player, id) = attack_arena(kind);
        if kind == IDOL {
            let home = w.entities[&id].home;
            let i = home.y as usize * w.map.width as usize + home.x as usize;
            let map = Arc::make_mut(&mut w.map);
            map.cells[i].region = 16;
            map.cells[i + 1].region = 16;
            map.cells[i + 1].ground = abyss::FLOOR;
            map.cells[i + 1].object = Some(BRIDGE);
            w.spawn_entity(
                BRIDGE,
                Pos {
                    x: home.x + 1.,
                    y: home.y,
                },
                vec![],
            )
            .unwrap();
            w.damage_enemy(player, id, 1, true, &[], None).unwrap();
        }
        w.entities.get_mut(&id).unwrap().activated = Some(0);
        w.apply_condition(player, condition::INVISIBLE, 60000)
            .unwrap();
        let now = if kind == MALPHAS { 11500 } else { 0 };
        let shots = attack_tick(&mut w, id, now);
        let aimed = if kind == MALPHAS { 0 } else { 3 };
        assert!(!shots.is_empty());
        assert!(shots.iter().all(|(p, _)| *p != aimed));
        let decoy = attack_decoy(&mut w, player);
        assert_eq!(w.decoy_positions().count(), 1);
        let shots = attack_tick(&mut w, id, now + 1400);
        let decoy_pos = w.entities[&decoy].pos;
        let origin = w.entities[&id].pos;
        let heading = (decoy_pos.y - origin.y).atan2(decoy_pos.x - origin.x);
        let targeted: Vec<_> = shots.iter().filter(|(p, _)| *p == aimed).collect();
        assert!(!targeted.is_empty());
        for (n, (_, angle)) in targeted.iter().enumerate() {
            let offset = if kind == MALPHAS {
                0.6f32.to_radians()
            } else {
                (n as f32 % 3. - 1.) * 10f32.to_radians()
            };
            assert!((angle - heading - offset).abs() < 0.00001);
        }
        if expire {
            w.now = now + 3000;
            w.ability_step();
        } else {
            w.remove_entity(decoy);
        }
        assert_eq!(w.decoy_positions().count(), 0);
        let shots = attack_tick(&mut w, id, now + 4200);
        assert!(shots.iter().all(|(p, _)| *p != aimed));
        let mut teammate = tests::ch(&w);
        teammate.id = 2;
        teammate.account = 2;
        let visible = w.enter(teammate, None).unwrap();
        w.players.get_mut(&visible).unwrap().pos = Pos { x: 14.5, y: 20.5 };
        let shots = attack_tick(&mut w, id, now + 5600);
        assert!(shots.iter().any(|(p, _)| *p == aimed));
        let fixed: Vec<_> = shots.iter().filter(|(p, _)| *p == 2).collect();
        let (count, gap) = if kind == MALPHAS {
            (6, 60f32)
        } else {
            (4, 90f32)
        };
        assert_eq!(fixed.len(), count);
        for (n, (_, angle)) in fixed.iter().enumerate() {
            assert!(
                (angle - (n as f32 - (count - 1) as f32 / 2.) * gap.to_radians()).abs() < 0.00001
            );
        }
        println!("fixed/aimed cloak/live/decoyExpired={expire}/teammate kind={kind:#x}: {shots:?}");
    }
}

#[test]
fn abyss_attack_stun_stasis_and_inactivity_resume_without_catchup() {
    for mode in [condition::STUNNED, condition::STASIS, condition::INVISIBLE] {
        let (mut w, player, boss) = attack_arena(MALPHAS);
        w.entities.get_mut(&boss).unwrap().activated = Some(0);
        assert_eq!(
            attack_tick(&mut w, boss, 11500)
                .iter()
                .filter(|(p, _)| *p == 2)
                .count(),
            6
        );
        w.now = 11500;
        if mode == condition::INVISIBLE {
            w.apply_condition(player, mode, 10000).unwrap();
        } else if mode == condition::STASIS {
            w.apply_condition(boss, mode, 10000).unwrap();
            assert_eq!(
                w.entities[&boss].conditions(w.now) & condition::bit(mode),
                0
            );
            // Selected Malphas is immune; a disclosed forced condition isolates the shared suspension path.
            w.entities.get_mut(&boss).unwrap().condition |= condition::bit(mode);
        } else {
            w.apply_condition(boss, mode, 10000).unwrap();
        }
        for now in (11550..=12000).step_by(50) {
            let shots = attack_tick(&mut w, boss, now);
            if mode != condition::INVISIBLE {
                assert!(shots.is_empty());
            }
        }
        if mode != condition::INVISIBLE {
            let e = w.entities.get_mut(&boss).unwrap();
            e.statuses.clear();
            e.condition &= !condition::bit(condition::STASIS);
        }
        let due = if mode == condition::STASIS {
            13200
        } else {
            12700
        };
        assert!(
            attack_tick(&mut w, boss, due - 50)
                .iter()
                .all(|(p, _)| *p != 2)
        );
        assert_eq!(
            attack_tick(&mut w, boss, due)
                .iter()
                .filter(|(p, _)| *p == 2)
                .count(),
            6
        );
        let late = attack_tick(&mut w, boss, due + 3500);
        assert_eq!(late.iter().filter(|(p, _)| *p == 2).count(), 6);
        assert!(
            attack_tick(&mut w, boss, due + 3550)
                .iter()
                .all(|(p, _)| *p != 2)
        );
        println!("mode={mode} ring resumed at={due}, delayed tick emits one");
    }
    let (mut w, player, boss) = attack_arena(MALPHAS);
    w.entities.get_mut(&boss).unwrap().activated = Some(0);
    attack_tick(&mut w, boss, 11500);
    w.players.get_mut(&player).unwrap().pos = Pos { x: 12.5, y: 50.5 };
    for now in (11550..=12000).step_by(50) {
        assert!(attack_tick(&mut w, boss, now).is_empty());
    }
    w.players.get_mut(&player).unwrap().pos = Pos { x: 12.5, y: 12.5 };
    assert!(
        attack_tick(&mut w, boss, 13150)
            .iter()
            .all(|(p, _)| *p != 2)
    );
    assert_eq!(
        attack_tick(&mut w, boss, 13200)
            .iter()
            .filter(|(p, _)| *p == 2)
            .count(),
        6
    );
}

fn lifecycle_boss() -> (World, i32, i32) {
    let (mut w, player) = fixture();
    w.encounter = true;
    w.name = "Abyss of Demons".into();
    let boss = w
        .spawn_entity(MALPHAS, Pos { x: 12.5, y: 12.5 }, vec![])
        .unwrap();
    w.entities.get_mut(&boss).unwrap().condition |= condition::bit(condition::PARALYZED);
    w.entities.get_mut(&boss).unwrap().activated = Some(0);
    (w, player, boss)
}

fn owned(w: &World, parent: i32, kind: u16) -> Vec<i32> {
    w.entities
        .values()
        .filter(|e| e.parent == Some(parent) && e.kind == kind)
        .map(|e| e.id)
        .collect()
}

fn lifecycle_tick(w: &mut World, now: u64) {
    w.now = now;
    w.mechanic_expiry();
    w.npc_step();
}

#[test]
fn abyss_lifecycle_flamer_threshold_and_lethal_paths_match_approved_policy() {
    for remaining in [201, 200, 199, 0] {
        let (mut w, player, id) = attack_arena(0x90b);
        let (packets, mut rx) = mpsc::channel(4096);
        let (lost, _loss) = watch::channel(false);
        w.players.get_mut(&player).unwrap().tx = Some(Output { packets, lost });
        w.players.get_mut(&player).unwrap().known.insert(id);
        if remaining == 0 {
            w.players.get_mut(&player).unwrap().controller = Controller::Human;
        }
        w.now = 100;
        let maximum = w.entities[&id].hp;
        assert_eq!(
            w.damage_enemy(player, id, maximum - remaining, true, &[], Some(1))
                .unwrap(),
            maximum - remaining
        );
        if remaining == 0 {
            assert!(!w.entities.contains_key(&id));
            assert_eq!(
                w.bullets.iter().filter(|b| b.owner == id).count(),
                8,
                "lethal must burst before defeat, not clamp HP"
            );
            let mut codes = vec![];
            while let Ok(packet) = rx.try_recv() {
                codes.push(packet.id);
            }
            assert!(
                codes.iter().position(|code| *code == 19).unwrap()
                    < codes.iter().position(|code| *code == 47).unwrap()
            );
            assert_eq!(w.players[&player].character.kills, 1);
            let bullet = w.bullets.iter().find(|b| b.owner == id).unwrap().clone();
            let hp = w.players[&player].character.hp;
            w.native_player_hit(player, id, bullet.id).unwrap();
            assert_eq!(w.players[&player].character.hp, hp);
            w.acknowledge_shoot(player, 100).unwrap();
            let damage = condition::damage(
                bullet.damage,
                w.defense(player).unwrap(),
                w.players[&player].conditions(w.now),
                bullet.spec.armor_piercing,
            );
            w.native_player_hit(player, id, bullet.id).unwrap();
            assert_eq!(w.players[&player].character.hp, hp - damage);
            w.native_player_hit(player, id, bullet.id).unwrap();
            assert_eq!(w.players[&player].character.hp, hp - damage);
        } else {
            w.apply_condition(player, condition::INVISIBLE, 10000)
                .unwrap();
            lifecycle_tick(&mut w, 399);
            assert!(w.bullets.iter().all(|b| b.owner != id));
            lifecycle_tick(&mut w, 400);
            assert_eq!(w.entities.contains_key(&id), remaining == 201);
            assert_eq!(
                w.bullets.iter().filter(|b| b.owner == id).count(),
                if remaining == 201 { 0 } else { 8 }
            );
        }
        if remaining > 0 {
            let mut flashes = 0;
            while let Ok(packet) = rx.try_recv() {
                if packet.id == wire::SHOW_EFFECT && packet.body[0] == 15 {
                    let mut r = crate::wire::Reader::new(&packet.body);
                    assert_eq!(r.u8().unwrap(), 15);
                    assert_eq!(r.i32().unwrap(), id);
                    assert!((r.f32().unwrap() - 0.3).abs() < 0.00001);
                    assert_eq!(r.f32().unwrap(), 1.);
                    assert_eq!(r.f32().unwrap(), 0.);
                    assert_eq!(r.f32().unwrap(), 0.);
                    assert_eq!(r.i32().unwrap() as u32, 0xffff0000);
                    r.done().unwrap();
                    flashes += 1;
                }
            }
            assert_eq!(flashes, usize::from(remaining <= 200));
        }
        println!("Flamer boundary remaining={remaining} verified");
    }
    let (mut w, p, id) = attack_arena(0x90b);
    w.now = 100;
    w.damage_enemy(p, id, 800, true, &[], None).unwrap();
    w.now = 200;
    w.damage_enemy(p, id, 1, true, &[], None).unwrap();
    assert_eq!(w.entities[&id].hp, 199);
    assert!(
        matches!(
            w.entities[&id].abyss,
            Some(super::abyss::Runtime::Flamer {
                terminal: Some(400),
                ..
            })
        ),
        "further nonlethal damage cannot rearm deadline"
    );
    w.damage_enemy(p, id, 199, true, &[], None).unwrap();
    assert!(!w.entities.contains_key(&id));
    assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 8);
    lifecycle_tick(&mut w, 500);
    assert_eq!(w.players[&p].character.kills, 1);
}

#[test]
fn abyss_lifecycle_flamer_terminal_burst_is_target_independent_and_once_only() {
    for mode in [
        0,
        condition::INVISIBLE,
        condition::STUNNED,
        condition::STASIS,
        255,
    ] {
        let (mut w, player, id) = attack_arena(0x90b);
        w.now = 100;
        w.damage_enemy(player, id, 800, true, &[], None).unwrap();
        if mode == 255 {
            w.leave(player).unwrap();
        } else if mode == condition::INVISIBLE {
            w.apply_condition(player, mode, 10000).unwrap();
        } else if mode != 0 {
            w.apply_condition(id, mode, 10000).unwrap();
        }
        lifecycle_tick(&mut w, 399);
        assert!(w.bullets.iter().all(|b| b.owner != id));
        lifecycle_tick(&mut w, 400);
        let burst: Vec<_> = w.bullets.iter().filter(|b| b.owner == id).collect();
        assert_eq!(burst.len(), 8, "mode={mode}");
        for (i, b) in burst.iter().enumerate() {
            assert!((b.angle - (i as f32 - 3.5) * 45f32.to_radians()).abs() < 0.00001);
        }
        assert!(!w.entities.contains_key(&id));
        lifecycle_tick(&mut w, 500);
        assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 8);
        if mode != 255 {
            assert_eq!(w.players[&player].character.kills, 1);
        }
        println!("Flamer mode={mode} terminal=400 eight fixed rays, once");
    }
}

#[test]
fn abyss_lifecycle_kill_and_cleanup_credit_are_distinct() {
    for kind in [0x908, 0x90b] {
        for cleanup in [false, true] {
            let (mut w, player, id) = attack_arena(kind);
            let xp = w.players[&player].experience_queued;
            if cleanup {
                w.damage_enemy(player, id, 1, true, &[], None).unwrap();
                w.remove_entity(id);
            } else {
                w.damage_enemy(player, id, 5000, true, &[], None).unwrap();
            }
            lifecycle_tick(&mut w, 1000);
            assert_eq!(w.players[&player].character.kills, u64::from(!cleanup));
            assert_eq!(
                w.players[&player].experience_queued > xp,
                !cleanup && kind == 0x908
            );
            assert!(!w.cleared);
            assert!(w.players[&player].clears_queued.iter().all(|n| *n == 0));
            if cleanup {
                assert!(
                    w.entities.values().all(|e| e.items.is_empty()),
                    "cleanup cannot publish loot"
                );
            }
        }
    }
    for (armed_at, kills) in [(46700, 1), (46900, 0)] {
        let (mut w, p, boss) = lifecycle_boss();
        w.now = armed_at;
        let f = w
            .spawn_entity(0x90b, Pos { x: 14.5, y: 12.5 }, vec![])
            .unwrap();
        w.entities
            .get_mut(&f)
            .unwrap()
            .parent
            .set(Some(boss), f, &mut w.timed_ids);
        w.damage_enemy(p, f, 800, true, &[], None).unwrap();
        w.now = 46950;
        w.step();
        assert!(!w.entities.contains_key(&f));
        assert_eq!(
            w.players[&p].character.kills, kills,
            "World::step equal terminal/dismissal precedence"
        );
        assert_eq!(
            w.bullets.iter().filter(|b| b.owner == f).count(),
            if kills == 1 { 8 } else { 0 }
        );
        println!("Flamer armed={armed_at} dismissal=47000 kills={kills}");
    }
}

#[test]
fn abyss_lifecycle_births_and_throws_are_staggered_owned_and_reserved() {
    let (mut w, p, boss) = lifecycle_boss();
    w.apply_condition(p, condition::INVISIBLE, 100000).unwrap();
    for (now, protectors, missiles) in [
        (0, 0, 0),
        (999, 0, 0),
        (1000, 1, 0),
        (1799, 1, 0),
        (1800, 1, 1),
        (2000, 2, 1),
        (3000, 3, 1),
        (3600, 3, 2),
    ] {
        lifecycle_tick(&mut w, now);
        assert_eq!(owned(&w, boss, 0x908).len(), protectors, "at={now}");
        assert_eq!(owned(&w, boss, 0x909).len(), missiles, "at={now}");
    }
    let first = owned(&w, boss, 0x908)[0];
    w.remove_entity(first);
    lifecycle_tick(&mut w, 4000);
    assert_eq!(owned(&w, boss, 0x908).len(), 3);
    let (tx, mut rx) = mpsc::channel(4096);
    let (lost, _loss) = watch::channel(false);
    let p = w.players.values_mut().next().unwrap();
    p.tx = Some(Output { packets: tx, lost });
    p.known.insert(boss);
    for (now, count) in [
        (39000, 0),
        (39499, 0),
        (39500, 1),
        (40000, 2),
        (40499, 2),
        (40500, 7),
        (41000, 8),
        (41500, 9),
    ] {
        lifecycle_tick(&mut w, now);
        assert_eq!(
            owned(&w, boss, 0x90b).len(),
            count,
            "Flamer landing/admission at={now}"
        );
    }
    let mut throws = 0;
    while let Ok(packet) = rx.try_recv() {
        if packet.id == wire::SHOW_EFFECT && packet.body[0] == 4 {
            throws += 1;
        }
    }
    assert_eq!(throws, 4);
    let inner = owned(&w, boss, 0x90b)
        .into_iter()
        .find(|id| {
            matches!(
                w.entities[id].abyss,
                Some(super::abyss::Runtime::Flamer { outer: false, .. })
            )
        })
        .unwrap();
    let player = *w.players.keys().next().unwrap();
    w.damage_enemy(player, inner, 1000, true, &[], None)
        .unwrap();
    lifecycle_tick(&mut w, 42000);
    assert_eq!(owned(&w, boss, 0x90b).len(), 9, "inner refill one/deadline");
    lifecycle_tick(&mut w, 47000);
    assert!(owned(&w, boss, 0x90b).is_empty());
    for remove_parent in [false, true] {
        let (mut w, _, boss) = lifecycle_boss();
        w.now = 39000;
        w.abyss_mechanics(boss, None, 39000).unwrap();
        assert_eq!(w.abyss_pending_count(), 4);
        if remove_parent {
            w.remove_entity(boss);
        } else {
            w.now = 47000;
            w.abyss_mechanics(boss, None, 47000).unwrap();
        }
        assert_eq!(
            w.abyss_pending_count(),
            0,
            "phase exit/parent removal releases each reservation"
        );
        assert!(owned(&w, boss, 0x90b).is_empty());
    }
    let (mut w, _, boss) = lifecycle_boss();
    w.now = 39000;
    w.abyss_mechanics(boss, None, 39000).unwrap();
    w.move_entity(boss, Pos { x: 13.5, y: 12.5 }, 2.).unwrap();
    w.now = 40500;
    w.abyss_mechanics(boss, None, 40500).unwrap();
    for pos in [
        Pos { x: 18.5, y: 12.5 },
        Pos { x: 12.5, y: 18.5 },
        Pos { x: 6.5, y: 12.5 },
        Pos { x: 12.5, y: 6.5 },
    ] {
        let e = w
            .entities
            .values()
            .find(|e| e.kind == 0x90b && e.pos.distance(pos) < 0.001)
            .unwrap();
        assert_eq!(e.parent, Some(boss));
        assert_eq!(e.activated, Some(40500));
    }
}

#[test]
fn abyss_lifecycle_births_share_capacity_with_upstream_pending_tosses() {
    let (mut w, _, boss) = lifecycle_boss();
    let position = w.entities[&boss].pos;
    for _ in 0..256 {
        w.manor_toss(boss, crate::dungeon::manor::ROCK, position, 5000)
            .unwrap();
    }
    assert_eq!(w.summon_occupancy(), 256);
    w.now = 1000;
    w.abyss_mechanics(boss, None, 1000).unwrap();
    assert!(owned(&w, boss, 0x908).is_empty());
    assert_eq!(w.summon_occupancy(), 256);
    w.manor_births.pop().unwrap();
    w.now = 2000;
    w.abyss_mechanics(boss, None, 2000).unwrap();
    assert_eq!(owned(&w, boss, 0x908).len(), 1);
    assert_eq!(w.summon_occupancy(), 256);
}

#[test]
fn abyss_lifecycle_protector_orbit_tracks_moving_and_slowed_parent() {
    let (mut w, _, boss) = lifecycle_boss();
    w.entities.get_mut(&boss).unwrap().condition |= condition::bit(condition::STASIS);
    let c = w
        .spawn_entity(0x908, Pos { x: 21.5, y: 12.5 }, vec![])
        .unwrap();
    w.entities
        .get_mut(&c)
        .unwrap()
        .parent
        .set(Some(boss), c, &mut w.timed_ids);
    for slowed in [false, true] {
        if slowed {
            w.apply_condition(c, condition::SLOWED, 200000).unwrap();
        }
        for n in 0..20 {
            let old = w.entities[&c].pos;
            let anchor = Pos {
                x: 12.5,
                y: 12.5 + n as f32 * 0.01,
            };
            w.move_entity(boss, anchor, 1.).unwrap();
            let initial = (old.y - anchor.y).atan2(old.x - anchor.x);
            w.now += 50;
            w.npc_step();
            let e = &w.entities[&c];
            let expected = initial + 4. / 9. * 0.05 * if slowed { 0.5 } else { 1. };
            assert!((e.phase - expected).abs() < 0.00001);
            assert!((e.goal.unwrap().distance(anchor) - 9.).abs() < 0.001);
            assert!(e.pos.distance(old) <= if slowed { 0.10001 } else { 0.20001 });
        }
    }
    w.entities.get_mut(&c).unwrap().statuses.clear();
    let anchor = w.entities[&boss].pos;
    for _ in 0..1000 {
        w.now += 50;
        w.npc_step();
    }
    assert!((w.entities[&c].pos.distance(anchor) - 9.).abs() < 0.05);
    w.apply_condition(c, condition::PARALYZED, 1000).unwrap();
    let old = w.entities[&c].pos;
    let angle = w.entities[&c].phase;
    for _ in 0..10 {
        w.now += 50;
        w.npc_step();
    }
    assert_eq!(w.entities[&c].pos.distance(old), 0.);
    assert_eq!(w.entities[&c].phase, angle);
    let (mut w, p, boss) = lifecycle_boss();
    w.entities.get_mut(&boss).unwrap().condition = 0;
    w.players.get_mut(&p).unwrap().pos = Pos { x: 18.5, y: 12.5 };
    let c = w
        .spawn_entity(0x908, Pos { x: 12.5, y: 21.5 }, vec![])
        .unwrap();
    w.entities
        .get_mut(&c)
        .unwrap()
        .parent
        .set(Some(boss), c, &mut w.timed_ids);
    for slowed_parent in [false, true] {
        if slowed_parent {
            w.apply_condition(boss, condition::SLOWED, 1000).unwrap();
        }
        let old_parent = w.entities[&boss].pos;
        let old_child = w.entities[&c].pos;
        w.now += 50;
        w.npc_step();
        let anchor = w.entities[&boss].pos;
        let distance = old_parent.distance(anchor);
        assert!((distance - 2.405 * 0.05 * if slowed_parent { 0.5 } else { 1. }).abs() < 0.00001);
        let angle = (old_child.y - anchor.y).atan2(old_child.x - anchor.x);
        assert!((w.entities[&c].phase - angle - 4. / 9. * 0.05).abs() < 0.00001);
        assert!((w.entities[&c].goal.unwrap().distance(anchor) - 9.).abs() < 0.001);
    }
    let pos = w.entities[&c].pos;
    let i = pos.y as usize * w.map.width as usize + pos.x as usize;
    Arc::make_mut(&mut w.map).cells[i].ground = 0xfe;
    let old = w.entities[&c].pos;
    w.now += 50;
    w.npc_step();
    assert_eq!(
        w.entities[&c].pos.distance(old),
        0.,
        "blocked Protector cannot teleport through collision"
    );
    assert_eq!(w.entities[&c].parent, Some(boss));
}

#[test]
fn abyss_lifecycle_orphans_remain_owned_stationary_and_attack_intentionally() {
    let (mut w, p, boss) = lifecycle_boss();
    for n in 0..=60 {
        lifecycle_tick(&mut w, n * 50);
    }
    let children: Vec<_> = owned(&w, boss, 0x908)
        .iter()
        .map(|id| (*id, w.entities[id].pos))
        .collect();
    assert_eq!(children.len(), 3);
    w.entities.get_mut(&boss).unwrap().condition = 0;
    w.damage_enemy(p, boss, 10000, true, &[], None).unwrap();
    for _ in 0..30 {
        w.bullets.clear();
        w.step();
    }
    for (id, pos) in &children {
        assert_eq!(w.entities[id].pos.distance(*pos), 0.);
        assert_eq!(w.entities[id].parent, Some(boss));
        assert!(w.entities[id].orbit.is_none());
    }
    let c = children[0].0;
    w.players.get_mut(&p).unwrap().pos = Pos {
        x: children[0].1.x - 1.,
        y: children[0].1.y,
    };
    w.bullets.clear();
    w.now += 1200;
    w.npc_step();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == c).count(), 3);
    w.damage_enemy(p, c, 5000, true, &[], None).unwrap();
    assert_eq!(w.players[&p].character.kills, 2);
}

#[test]
fn abyss_lifecycle_phase_entry_anchor_replaces_original_home_leash() {
    let (mut w, p, boss) = lifecycle_boss();
    w.move_entity(boss, Pos { x: 20.5, y: 20.5 }, 100.).unwrap();
    w.entities.get_mut(&boss).unwrap().condition = 0;
    w.players.get_mut(&p).unwrap().pos = Pos { x: 20.5, y: 18.5 };
    w.now = 11500;
    w.npc_step();
    let anchor = Pos { x: 20.5, y: 20.5 };
    assert!(
        w.entities[&boss].goal.unwrap().distance(anchor) <= 2.01,
        "wander must use phase entry rather than original home"
    );
    w.move_entity(boss, Pos { x: 14.5, y: 20.5 }, 100.).unwrap();
    w.now = 11550;
    w.npc_step();
    assert_eq!(w.entities[&boss].goal.unwrap().distance(anchor), 0.);
    w.move_entity(boss, Pos { x: 12.5, y: 12.5 }, 100.).unwrap();
    w.now = 61700;
    w.npc_step();
    assert!(
        w.entities[&boss]
            .goal
            .unwrap()
            .distance(Pos { x: 12.5, y: 12.5 })
            <= 2.01
    );
    let (mut w, p, boss) = lifecycle_boss();
    w.entities.get_mut(&boss).unwrap().condition = 0;
    w.players.get_mut(&p).unwrap().pos = Pos { x: 18.5, y: 12.5 };
    let old = w.entities[&boss].pos;
    w.npc_step();
    assert!((w.entities[&boss].pos.distance(old) - 2.405 * 0.05).abs() < 0.00001);
    for (entry, speed) in [
        (0, 2.405),
        (9000, 2.96),
        (11500, 5.18),
        (23500, 1.85),
        (36500, 2.96),
        (39000, 2.96),
        (47000, 2.96),
    ] {
        let (mut w, p, boss) = lifecycle_boss();
        w.apply_condition(p, condition::INVISIBLE, 100000).unwrap();
        w.entities.get_mut(&boss).unwrap().condition = 0;
        w.now = entry;
        w.abyss_mechanics(boss, None, entry).unwrap();
        w.npc_step();
        w.entities.get_mut(&boss).unwrap().goal = Some(Pos { x: 14.5, y: 12.5 });
        let old = w.entities[&boss].pos;
        w.now += 50;
        w.npc_step();
        assert!(
            (w.entities[&boss].pos.distance(old) - speed * 0.05).abs() < 0.00001,
            "entry={entry}"
        );
        w.apply_condition(boss, condition::SLOWED, 1000).unwrap();
        let old = w.entities[&boss].pos;
        w.now += 50;
        w.npc_step();
        assert!(
            (w.entities[&boss].pos.distance(old) - speed * 0.025).abs() < 0.00001,
            "slowed entry={entry}"
        );
        w.apply_condition(boss, condition::PARALYZED, 1000).unwrap();
        let old = w.entities[&boss].pos;
        w.now += 50;
        w.npc_step();
        assert_eq!(w.entities[&boss].pos.distance(old), 0.);
    }
    for entry in [0, 39000, 47000] {
        let (mut w, p, boss) = lifecycle_boss();
        w.entities.get_mut(&boss).unwrap().condition = 0;
        let target = Pos { x: 15.5, y: 12.5 };
        w.players.get_mut(&p).unwrap().pos = target;
        w.now = entry;
        w.npc_step();
        let e = &w.entities[&boss];
        if entry == 47000 {
            assert!(
                e.pos.x < 12.5,
                "exhaustion retreats from visible target inside4"
            );
        } else {
            assert_eq!(
                e.goal.unwrap().distance(target),
                0.,
                "visible chase precedes wander"
            );
            assert!(e.pos.x > 12.5);
        }
    }
}

#[test]
fn abyss_lifecycle_growth_shield_and_birth_clocks_are_independent() {
    let (mut w, p, boss) = lifecycle_boss();
    for (now, size, shield) in [
        (11500, 89, false),
        (11600, 89, false),
        (11650, 78, false),
        (23500, 89, true),
        (24850, 100, true),
        (25300, 111, true),
        (26700, 122, false),
        (28100, 133, true),
    ] {
        w.now = now;
        w.abyss_mechanics(boss, None, now).unwrap();
        assert_eq!(w.entities[&boss].size, size, "time={now}");
        assert_eq!(
            w.damage_enemy(p, boss, 1, true, &[], None).unwrap(),
            i32::from(!shield)
        );
    }
    let (mut w, _, boss) = lifecycle_boss();
    for now in (0..=3000).step_by(50) {
        lifecycle_tick(&mut w, now);
    }
    assert_eq!(owned(&w, boss, 0x908).len(), 3);
    assert_eq!(w.entities[&boss].size, 100);
    let (mut w, p, boss) = lifecycle_boss();
    let mut expected_size: i32 = 100;
    let mut next_growth = None;
    let mut last_phase = None;
    for now in (0..50200).step_by(50) {
        let phase = abyss::phase(now);
        let age = now - [0, 9000, 11500, 23500, 36500, 39000, 47000][phase as usize];
        if last_phase != Some(phase) {
            last_phase = Some(phase);
            next_growth = matches!(phase, 2..=5).then_some(now);
        }
        if next_growth.is_some_and(|deadline| now >= deadline) {
            let (target, step) = match phase {
                2 => (30, 11),
                3 => (140, 11),
                4 => (100, 11),
                5 => (100, 14),
                _ => unreachable!(),
            };
            expected_size += (target - expected_size).clamp(-step, step);
            next_growth = (expected_size != target).then_some(now + 150);
        }
        w.now = now;
        w.abyss_mechanics(boss, None, now).unwrap();
        assert_eq!(w.entities[&boss].size, expected_size, "size at={now}");
        let shield = match phase {
            0 => !(2000..6000).contains(&age),
            1 | 4 | 5 => true,
            3 => {
                (0..3200).contains(&age)
                    || (4600..7400).contains(&age)
                    || (8800..11600).contains(&age)
            }
            _ => false,
        };
        assert_eq!(
            w.damage_enemy(p, boss, 1, true, &[], None).unwrap(),
            i32::from(!shield),
            "real shield acceptance at={now}"
        );
    }
}

#[tokio::test]
async fn abyss_route_completion_identity_waits_for_progress_commit() {
    for commit in [false, true] {
        let mut d = tests::directory();
        let origin = d.instance("abyss-of-demons", None).unwrap();
        let w = d.worlds.get_mut(&origin).unwrap();
        let store =
            crate::store::Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
        let account = store.list("abyss-route-clear".into()).await.unwrap().id;
        let ch = store
            .create(account, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let player = w.enter(ch, None).unwrap();
        let boss = w.entities.values().find(|e| e.kind == MALPHAS).unwrap().id;
        let pos = w.entities[&boss].pos;
        w.players.get_mut(&player).unwrap().pos = pos;
        w.entities.get_mut(&boss).unwrap().activated = Some(0);
        w.now = 2000;
        w.abyss_mechanics(boss, pos, 2000).unwrap();
        let hp = w.entities[&boss].hp;
        assert_eq!(
            w.damage_enemy(player, boss, hp, true, &[], None).unwrap(),
            hp
        );
        assert_eq!(
            w.damage_enemy(player, boss, hp, true, &[], None).unwrap(),
            0
        );
        let exit = w.clear_exit.unwrap();
        assert_eq!(w.entities[&exit].kind, 0x704);
        assert_eq!(
            w.content.objects[&w.entities[&exit].kind].name,
            "Realm Portal"
        );
        assert_eq!(w.entities[&exit].size, w.content.objects[&0x704].size);
        assert!(w.walkable(w.entities[&exit].pos));
        assert!(w.entities[&exit].pos.distance(pos) <= 2.01);
        assert_eq!(w.players[&player].character.abysses, 0);
        assert!(!w.entities[&exit].published);
        w.players.get_mut(&player).unwrap().pos = w.entities[&exit].pos;
        let snapshot = w.pending_progress_where(|_, _| true).pop().unwrap().1;
        assert_eq!(snapshot.abysses, 1);
        assert!(w.pending_progress_where(|_, _| true).is_empty());
        assert!(
            d.destination(player, Some(exit))
                .unwrap_err()
                .to_string()
                .contains("not published")
        );
        let w = d.worlds.get_mut(&origin).unwrap();
        if commit {
            store.progress(snapshot.clone()).await.unwrap();
            assert!(
                !w.entities[&exit].published,
                "Store ACK alone must await World publication"
            );
            w.finish_progress(player, snapshot.clone()).unwrap();
            assert!(w.finish_progress(player, snapshot).is_err());
            assert!(w.entities[&exit].published);
            assert!(w.clear_exit.is_none());
            assert_eq!(w.entities.values().filter(|e| e.kind == 0x704).count(), 1);
            assert_eq!(w.players[&player].character.abysses, 1);
            assert!(w.pending_progress_where(|_, _| true).is_empty());
            let id = w.players[&player].character.id;
            assert_eq!(store.load(account, id).await.unwrap().abysses, 1);
            let (return_account, return_character, realm) =
                d.destination(player, Some(exit)).unwrap();
            assert_eq!((return_account, return_character), (account, id));
            assert!(d.endgame.realms.contains_key(&realm));
            assert!(d.worlds[&realm].admission_open);
            println!(
                "Malphas fullHP={hp} real damage; exit={exit} kind=0x704 staged->StoreACK->published clear=1"
            );
        } else {
            let mut stale = snapshot;
            stale.revision += 1;
            let failed = store.progress(stale).await;
            assert!(failed.is_err());
            assert!(w.committed(player, failed).is_err());
            assert!(!w.entities[&exit].published);
            assert_eq!(w.players[&player].character.abysses, 0);
            assert!(d.destination(player, Some(exit)).is_err());
        }
    }
}

fn loot_arena(kind: u16) -> (World, i32, i32) {
    let (mut w, p, enemy) = attack_arena(kind);
    if kind == MALPHAS {
        w.entities.get_mut(&enemy).unwrap().activated = Some(0);
        w.now = 2000;
        w.abyss_mechanics(enemy, None, 2000).unwrap();
    } else if kind == IDOL {
        let pos = w.entities[&enemy].pos;
        let i = pos.y as usize * w.map.width as usize + pos.x as usize;
        Arc::make_mut(&mut w.map).cells[i].region = 16;
        let i = i + 1;
        Arc::make_mut(&mut w.map).cells[i].region = 16;
        Arc::make_mut(&mut w.map).cells[i].object = Some(BRIDGE);
        w.spawn_entity(
            BRIDGE,
            Pos {
                x: pos.x + 1.,
                y: pos.y,
            },
            vec![],
        )
        .unwrap();
    }
    (w, p, enemy)
}

fn loot_player(w: &mut World, account: i32) -> i32 {
    let mut ch = tests::ch(w);
    ch.account = account;
    ch.id = account;
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 12.5, y: 12.5 };
    p
}

fn allocated(w: &World) -> BTreeMap<Option<i32>, Vec<i32>> {
    let mut result = BTreeMap::<_, Vec<_>>::new();
    for id in w.bag_expiry.keys() {
        let e = &w.entities[id];
        let owner = e
            .extra_stats
            .iter()
            .find(|(stat, _)| *stat == 54)
            .map(|(_, owner)| *owner);
        let items = result.entry(owner).or_default();
        items.extend(e.items.iter().copied().filter(|item| *item >= 0));
        assert!(e.published && !e.reserved);
        assert_eq!(e.items.len(), 8);
    }
    for items in result.values_mut() {
        items.sort();
    }
    result
}

fn expected_loot(
    w: &World,
    kind: u16,
    seed: u64,
    owners: &[Option<i32>],
) -> BTreeMap<Option<i32>, Vec<i32>> {
    use rand::{SeedableRng, rngs::StdRng};
    let mut rng = StdRng::seed_from_u64(seed);
    let mut result = BTreeMap::new();
    for owner in owners {
        let mut items = Vec::new();
        for item in crate::loot::rewards(&w.content, kind, &mut rng) {
            let item = i32::from(item);
            if !items.contains(&item) || matches!(item, 0xa22 | 0xa23) {
                items.push(item);
            }
        }
        items.sort();
        if !items.is_empty() {
            result.insert(*owner, items);
        }
    }
    result
}

#[test]
fn abyss_loot_legitimate_minion_kills_drop_supplies_but_expiry_never_does() {
    use rand::{SeedableRng, rngs::StdRng};
    for kind in [0x908, 0x90b] {
        let mut hits = 0;
        let mut misses = 0;
        for seed in 0..64 {
            let (mut w, p, e) = loot_arena(kind);
            if kind == 0x908 {
                let boss = w
                    .spawn_entity(MALPHAS, Pos { x: 16.5, y: 20.5 }, vec![])
                    .unwrap();
                w.entities
                    .get_mut(&e)
                    .unwrap()
                    .parent
                    .set(Some(boss), e, &mut w.timed_ids);
                w.abyss_mechanics(e, None, 0).unwrap();
                w.remove_entity(boss);
                assert!(matches!(
                    w.entities[&e].abyss,
                    Some(super::abyss::Runtime::Protector { .. })
                ));
            }
            let xp = w.players[&p].experience_queued;
            if kind == 0x90b {
                w.damage_enemy(p, e, 800, true, &[], None).unwrap();
            }
            let expected = expected_loot(&w, kind, seed, &[None]);
            w.rng = StdRng::seed_from_u64(seed);
            if kind == 0x90b {
                w.now = 300;
                w.abyss_terminal_step();
                assert_eq!(w.bullets.iter().filter(|b| b.owner == e).count(), 8);
            } else {
                w.damage_enemy(p, e, 10000, true, &[], None).unwrap();
            }
            assert!(!w.entities.contains_key(&e));
            assert_eq!(
                allocated(&w),
                expected,
                "{kind:#x} seed={seed}: public rows once"
            );
            if expected.is_empty() {
                misses += 1;
            } else {
                hits += 1;
            }
            assert!(
                allocated(&w)
                    .values()
                    .flatten()
                    .all(|i| matches!(i, 0xa22 | 0xa23))
            );
            assert_eq!(w.players[&p].character.kills, 1);
            assert_eq!(
                w.players[&p].experience_queued > xp,
                kind == 0x908,
                "selected Flamer XP=0 retained"
            );
            let before = allocated(&w);
            assert_eq!(w.damage_enemy(p, e, 10000, true, &[], None).unwrap(), 0);
            w.abyss_terminal_step();
            assert_eq!(allocated(&w), before);
        }
        assert!(hits > 0 && misses > 0);
        println!("minion={kind:#x} seeds=0..64 hits={hits} misses={misses}");
    }
    for kind in [0x908, 0x90b] {
        let (mut w, p, e) = loot_arena(kind);
        w.damage_enemy(p, e, 1, true, &[], None).unwrap();
        w.entities
            .get_mut(&e)
            .unwrap()
            .expires
            .set(Some(w.now), e, &mut w.timed_ids);
        w.mechanic_expiry();
        w.mechanic_expiry();
        assert!(!w.entities.contains_key(&e));
        assert!(w.bag_expiry.is_empty());
        assert_eq!(w.players[&p].character.kills, 0);
        assert_eq!(w.players[&p].experience_queued, 0);
    }
    for mode in 0..4 {
        let (mut w, p, boss) = lifecycle_boss();
        let f = w
            .spawn_entity(0x90b, Pos { x: 14.5, y: 12.5 }, vec![])
            .unwrap();
        w.entities
            .get_mut(&f)
            .unwrap()
            .parent
            .set(Some(boss), f, &mut w.timed_ids);
        if mode == 0 {
            w.damage_enemy(p, f, 1, true, &[], None).unwrap();
            w.remove_entity(f);
        } else if mode == 1 {
            w.now = 46900;
            w.damage_enemy(p, f, 800, true, &[], None).unwrap();
            lifecycle_tick(&mut w, 47000);
        } else if mode == 2 {
            w.remove_entity(boss);
            lifecycle_tick(&mut w, 1000);
        } else {
            lifecycle_tick(&mut w, 39000);
            assert!(w.abyss_pending_count() > 0);
            w.remove_entity(boss);
        }
        lifecycle_tick(&mut w, 49000);
        assert!(w.bag_expiry.is_empty(), "cleanup mode={mode}");
        assert_eq!(w.players[&p].character.kills, 0);
        assert_eq!(w.players[&p].experience_queued, 0);
    }
    let (mut w, p, missile) = loot_arena(0x909);
    w.damage_enemy(p, missile, 10000, true, &[], None).unwrap();
    lifecycle_tick(&mut w, 50);
    lifecycle_tick(&mut w, 700);
    assert!(!w.entities.contains_key(&missile));
    assert!(w.bag_expiry.is_empty());
    assert_eq!(w.players[&p].character.kills, 0);
}

#[test]
fn abyss_loot_ten_equal_contributors_follow_approved_eligibility_and_guarantees() {
    use rand::{SeedableRng, rngs::StdRng};
    for (kind, each) in [(MALPHAS, 1000), (IDOL, 2500)] {
        let (mut w, first, e) = loot_arena(kind);
        let mut players = vec![first];
        players.extend((2..=10).map(|a| loot_player(&mut w, a)));
        let expected = expected_loot(&w, kind, 12351, &(1..=10).map(Some).collect::<Vec<_>>());
        for p in &players[..9] {
            assert_eq!(w.damage_enemy(*p, e, each, true, &[], None).unwrap(), each);
        }
        assert_eq!(w.entities[&e].damage.len(), 9);
        w.rng = StdRng::seed_from_u64(12351);
        assert_eq!(
            w.damage_enemy(players[9], e, each, true, &[], None)
                .unwrap(),
            each
        );
        assert_eq!(allocated(&w), expected);
        assert_eq!(allocated(&w).len(), 10);
        for a in 1..=10 {
            assert_eq!(
                allocated(&w)[&Some(a)]
                    .iter()
                    .filter(|i| **i == 0xa34)
                    .count(),
                1
            );
        }
        for p in players {
            assert_eq!(
                w.players[&p].clears_queued.iter().sum::<u64>(),
                u64::from(kind == MALPHAS)
            );
        }
        assert_eq!(w.cleared, kind == MALPHAS);
        if kind == MALPHAS {
            assert!(!w.entities[&w.clear_exit.unwrap()].published);
        }
    }
}

#[test]
fn abyss_loot_threshold_boundaries_and_account_groups_use_approved_units() {
    use rand::{SeedableRng, rngs::StdRng};
    for (kind, threshold) in [(MALPHAS, 100), (IDOL, 250)] {
        let (mut w, first, e) = loot_arena(kind);
        let damages = [0, 1, threshold - 1, threshold, threshold + 1];
        let mut players = vec![first];
        players.extend((2..=10).map(|a| loot_player(&mut w, a)));
        for (p, damage) in players.iter().zip(damages) {
            assert_eq!(
                w.damage_enemy(*p, e, damage, true, &[], None).unwrap(),
                damage
            );
        }
        // Admission disallows parallel sessions; these adversarial metadata fixtures test dedup without weakening that guard.
        let mut duplicate = w.players[&players[3]].character.clone();
        assert!(w.enter(duplicate.clone(), None).is_err());
        duplicate.account = 99;
        let alias = w.enter(duplicate, None).unwrap();
        w.players.get_mut(&alias).unwrap().character.account = 4;
        for p in &players[5..8] {
            w.damage_enemy(*p, e, threshold, true, &[], None).unwrap();
        }
        w.players.get_mut(&players[5]).unwrap().character.alive = false;
        w.players.get_mut(&players[6]).unwrap().death_pending = true;
        w.players.get_mut(&players[7]).unwrap().departing = true;
        w.damage_enemy(players[8], e, threshold, true, &[], None)
            .unwrap();
        w.players.remove(&players[8]);
        let last = players[9];
        let remaining = w.entities[&e].hp;
        w.rng = StdRng::seed_from_u64(12351);
        assert_eq!(
            w.damage_enemy(last, e, 100000, true, &[], None).unwrap(),
            remaining,
            "capped actual damage"
        );
        assert_eq!(
            allocated(&w),
            expected_loot(&w, kind, 12351, &[Some(4), Some(5), Some(10)])
        );
        assert_eq!(
            allocated(&w)[&Some(4)]
                .iter()
                .filter(|i| **i == 0xa34)
                .count(),
            1
        );
        let (mut w, first, e) = loot_arena(kind);
        let second = loot_player(&mut w, 2);
        let finisher = loot_player(&mut w, 3);
        w.damage_enemy(first, e, threshold / 2, true, &[], None)
            .unwrap();
        w.damage_enemy(second, e, threshold - threshold / 2, true, &[], None)
            .unwrap();
        w.players.get_mut(&second).unwrap().character.account = 1;
        w.rng = StdRng::seed_from_u64(12351);
        w.damage_enemy(finisher, e, 100000, true, &[], None)
            .unwrap();
        assert_eq!(
            allocated(&w),
            expected_loot(&w, kind, 12351, &[Some(1), Some(3)])
        );
        assert_eq!(
            allocated(&w)[&Some(1)]
                .iter()
                .filter(|i| **i == 0xa34)
                .count(),
            1
        );
        let (mut w, first, e) = loot_arena(kind);
        let alias = loot_player(&mut w, 2);
        let finisher = loot_player(&mut w, 3);
        w.damage_enemy(first, e, threshold - 1, true, &[], None)
            .unwrap();
        w.players.get_mut(&alias).unwrap().character.id = 1;
        w.players.get_mut(&alias).unwrap().character.account = 1;
        w.damage_enemy(finisher, e, 100000, true, &[], None)
            .unwrap();
        assert_eq!(
            allocated(&w).keys().copied().collect::<Vec<_>>(),
            vec![Some(3)],
            "duplicate character cannot double below-threshold contribution"
        );
        let (mut w, first, e) = loot_arena(kind);
        let finisher = loot_player(&mut w, 2);
        let hp = w.entities[&e].hp;
        w.damage_enemy(first, e, hp - 1, true, &[], None).unwrap();
        assert_eq!(
            w.damage_enemy(finisher, e, 100000, true, &[], None)
                .unwrap(),
            1
        );
        assert_eq!(
            allocated(&w).keys().copied().collect::<Vec<_>>(),
            vec![Some(1)],
            "one actual damage, not overkill raw, controls eligibility"
        );
    }
}

#[test]
fn abyss_loot_delivery_is_account_scoped_and_respects_selected_bag_constraints() {
    use rand::{SeedableRng, rngs::StdRng};
    let (mut w, owner, e) = loot_arena(MALPHAS);
    let other = loot_player(&mut w, 2);
    let seed = (0..10000)
        .find(|seed| expected_loot(&w, MALPHAS, *seed, &[Some(1)])[&Some(1)].len() > 8)
        .unwrap();
    w.rng = StdRng::seed_from_u64(seed);
    w.damage_enemy(owner, e, 10000, true, &[], None).unwrap();
    assert_eq!(allocated(&w), expected_loot(&w, MALPHAS, seed, &[Some(1)]));
    assert!(w.bag_expiry.len() > 1);
    let bags: Vec<_> = w.bag_expiry.keys().copied().collect();
    for bag in bags {
        let e = w.entities[&bag].clone();
        let appearance = e
            .items
            .iter()
            .filter(|i| **i >= 0)
            .map(|i| w.content.objects[&(*i as u16)].bag_type)
            .max()
            .unwrap();
        assert_eq!(
            e.kind,
            [0x500, 0x503, 0x507, 0x508, 0x509][appearance as usize]
        );
        for p in [owner, other] {
            w.players.get_mut(&p).unwrap().pos = e.pos;
        }
        assert!(w.visible(owner).contains(&bag));
        assert!(!w.visible(other).contains(&bag));
        assert!(
            !w.inspect_map(Some(other)).unwrap()["entities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["id"] == bag)
        );
        let from = Slot {
            object: bag,
            slot: 0,
            kind: e.items[0],
        };
        assert!(
            w.swap(
                other,
                from,
                Slot {
                    object: other,
                    slot: 4,
                    kind: -1
                }
            )
            .is_err()
        );
        assert!(w.use_item(other, from, e.pos).is_err());
        assert!(!w.entities[&bag].reserved);
        let plan = w
            .swap(
                owner,
                from,
                Slot {
                    object: owner,
                    slot: 4,
                    kind: w.players[&owner].character.items[4],
                },
            )
            .unwrap();
        assert!(w.entities[&bag].reserved);
        w.finish_inventory(plan, Ok(w.players[&owner].character.revision + 1))
            .unwrap();
    }
    assert!(
        w.loot_bag(w.players[&owner].pos, None, vec![0xaeb])
            .is_err(),
        "no public soulbound packaging"
    );
}

#[test]
fn abyss_loot_rewards_do_not_change_clear_publication_or_duplicate_defeats() {
    let (mut w, p, e) = loot_arena(MALPHAS);
    let low = loot_player(&mut w, 2);
    w.damage_enemy(low, e, 1, true, &[], None).unwrap();
    w.damage_enemy(p, e, 100000, true, &[], None).unwrap();
    let before = allocated(&w);
    assert_eq!(before.keys().copied().collect::<Vec<_>>(), vec![Some(1)]);
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    assert!(w.players[&low].clears_queued.contains(&1));
    assert!(w.players[&p].clears_queued.contains(&1));
    assert_eq!(w.damage_enemy(p, e, 100000, true, &[], None).unwrap(), 0);
    w.abyss_terminal_step();
    assert_eq!(allocated(&w), before);
    let snapshots = w.pending_progress_where(|_, _| true);
    for (id, snapshot) in snapshots {
        assert_eq!(snapshot.abysses, 1);
        w.finish_progress(id, snapshot.clone()).unwrap();
        assert!(w.finish_progress(id, snapshot).is_err());
    }
    assert!(w.entities[&exit].published);
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x704).count(), 1);
    assert_eq!(w.players[&p].character.abysses, 1);
    assert_eq!(w.players[&low].character.abysses, 1);
    assert!(w.pending_progress_where(|_, _| true).is_empty());
    assert_eq!(allocated(&w), before);
}

fn abyss_companion_flamer(hp: i32, prior: bool) {
    let (mut w, player) = fixture();
    w.encounter = true;
    w.name = "Abyss of Demons".into();
    let pos = w.players[&player].pos;
    let id = w
        .spawn_entity(
            0x90b,
            Pos {
                x: pos.x + 2.,
                y: pos.y,
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&id).unwrap().condition |= condition::bit(condition::PARALYZED);
    w.apply_condition(player, condition::INVISIBLE, 10000)
        .unwrap();
    let character = w.players[&player].character.id;
    let hit = condition::damage(35, w.content.objects[&0x90b].defense, 0, false);
    w.entities.get_mut(&id).unwrap().hp = hp + i32::from(prior);
    if prior {
        assert_eq!(w.damage_enemy(player, id, 1, true, &[], None).unwrap(), 1);
    }
    let (packets, mut rx) = mpsc::channel(4096);
    let (lost, _loss) = watch::channel(false);
    let p = w.players.get_mut(&player).unwrap();
    p.controller = Controller::Human;
    p.tx = Some(Output { packets, lost });
    p.known.insert(id);
    p.character.items[5] = 0xaa9;
    let plan = w
        .use_item(
            player,
            Slot {
                object: player,
                slot: 5,
                kind: 0xaa9,
            },
            pos,
        )
        .unwrap()
        .unwrap();
    w.step();
    assert_eq!(w.entities[&id].hp, hp, "uncommitted egg cannot attack");
    let revision = plan.revision + 1;
    w.finish_inventory(plan, Ok(revision)).unwrap();
    assert_eq!(w.players[&player].character.items[5], -1);
    assert!(w.entities.values().any(|e| e.kind == 0x1400 && e.published));
    let xp = w.players[&player].experience_queued;
    w.step();
    let remaining = hp - hit;
    if remaining > 0 {
        let e = &w.entities[&id];
        assert_eq!(e.hp, remaining);
        assert_eq!(
            e.damage,
            if prior {
                BTreeMap::from([(character, 1)])
            } else {
                BTreeMap::new()
            }
        );
        assert!(
            matches!(e.abyss, Some(super::abyss::Runtime::Flamer { attacker, .. }) if attacker == prior.then_some(character))
        );
        if remaining <= 200 {
            assert!(World::abyss_flamer_armed(e));
            for _ in 0..5 {
                w.step();
            }
            assert!(w.entities.contains_key(&id));
            w.step();
            assert!(
                !w.entities.contains_key(&id),
                "unattributed deadline must defeat, not skip"
            );
        }
    }
    let terminal = remaining <= 200;
    if terminal {
        assert!(!w.entities.contains_key(&id));
        assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 8);
        w.step();
        assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 8);
    }
    assert_eq!(
        w.players[&player].character.kills,
        u64::from(prior && terminal && remaining > 0)
    );
    assert_eq!(w.players[&player].experience_queued, xp);
    if !prior {
        assert!(
            w.entities.values().all(|e| e.items.is_empty()),
            "no invented loot contributor"
        );
    }
    let deaths = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|p| p.id == 47 && p.body[7] == 1)
        .count();
    assert_eq!(deaths, usize::from(terminal));
    println!(
        "companion committed hp={hp} hit={hit} prior={prior} terminal={terminal} deaths={deaths}"
    );
}

#[test]
fn abyss_companion_flamer_normal_world_step() {
    abyss_companion_flamer(1000, false);
}

#[test]
fn abyss_companion_flamer_threshold_world_step() {
    abyss_companion_flamer(220, false);
}

#[test]
fn abyss_companion_flamer_lethal_world_step() {
    abyss_companion_flamer(1, false);
}

#[test]
fn abyss_companion_flamer_retains_real_attacker_world_step() {
    abyss_companion_flamer(220, true);
}

fn abyss_projectile_flamer(batch: usize) -> (World, i32, i32) {
    let (mut w, player) = fixture();
    w.encounter = true;
    w.name = "Abyss of Demons".into();
    w.players.get_mut(&player).unwrap().controller = Controller::Human;
    w.apply_condition(player, condition::INVISIBLE, 10000)
        .unwrap();
    let pos = w.players[&player].pos;
    let id = w
        .spawn_entity(
            0x90b,
            Pos {
                x: pos.x + 1.,
                y: pos.y,
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&id).unwrap().hp = 1;
    w.entities.get_mut(&id).unwrap().condition |= condition::bit(condition::PARALYZED);
    w.now = 1000;
    let kind = w.players[&player].character.items[0] as u16;
    let bullet = w.players[&player].next_bullet;
    w.shoot(
        player,
        1000,
        bullet,
        kind,
        Pos {
            x: pos.x + 0.3,
            y: pos.y,
        },
        0.,
    )
    .unwrap();
    assert_eq!(w.bullets.len(), 1);
    let mut filler = w.bullets[0].clone();
    filler.owner = -90;
    filler.friendly = false;
    filler.start = Pos { x: 2.5, y: 2.5 };
    filler.pos = filler.start;
    filler.spec.speed = 0.;
    filler.spec.lifetime = 1000.;
    for _ in 1..batch {
        w.bullets.push(filler.clone());
    }
    (w, player, id)
}

#[test]
fn abyss_projectile_lethal_flamer_world_step_retains_burst() {
    for batch in [1, 2040] {
        let (mut w, player, id) = abyss_projectile_flamer(batch);
        let (packets, _rx) = mpsc::channel(4096);
        let (lost, _loss) = watch::channel(false);
        w.players.get_mut(&player).unwrap().tx = Some(Output { packets, lost });
        w.players.get_mut(&player).unwrap().known.insert(id);
        w.step();
        assert!(
            !w.entities.contains_key(&id),
            "real friendly collision must defeat"
        );
        assert_eq!(w.players[&player].character.kills, 1);
        let burst: Vec<_> = w
            .bullets
            .iter()
            .filter(|b| b.owner == id)
            .cloned()
            .collect();
        assert_eq!(
            burst.len(),
            8,
            "collision-born rays must survive detached batch merge"
        );
        assert_eq!(w.bullets.len(), batch - 1 + 8);
        for (i, b) in burst.iter().enumerate() {
            assert_eq!(b.born, 1050);
            assert_eq!(
                b.pos.distance(b.start),
                0.,
                "newborn is not processed in emission tick"
            );
            assert!((b.angle - (i as f32 - 3.5) * 45f32.to_radians()).abs() < 0.00001);
        }
        // Native snapshots remain authoritative after the shooter is removed; ACK is real.
        let hp = w.players[&player].character.hp;
        w.native_player_hit(player, id, burst[0].id).unwrap();
        assert_eq!(w.players[&player].character.hp, hp);
        w.acknowledge_shoot(player, 100).unwrap();
        w.native_player_hit(player, id, burst[0].id).unwrap();
        let hit_hp = w.players[&player].character.hp;
        assert!(hit_hp < hp);
        w.native_player_hit(player, id, burst[0].id).unwrap();
        assert_eq!(w.players[&player].character.hp, hit_hp);
        // A real simulated victim on a ray proves next-step collision, not only retention.
        let mut ch = tests::ch(&w);
        ch.id += 1;
        ch.account += 1;
        ch.base = w.content.classes[&ch.class].caps;
        ch.hp = ch.base[0];
        let victim = w
            .enter_at(
                ch,
                None,
                Some(burst[0].position(1100)),
                AccountPresentation {
                    guild: None,
                    fame: 0,
                    best_fame: BTreeMap::new(),
                    credits: 0,
                },
            )
            .unwrap();
        let victim_hp = w.players[&victim].character.hp;
        w.safe = false;
        w.step();
        assert!(w.players[&victim].character.hp < victim_hp);
        assert!(
            w.bullets
                .iter()
                .filter(|b| b.owner == id)
                .all(|b| b.pos.distance(b.position(1100)) < 0.00001)
        );
        assert_eq!(w.players[&player].character.kills, 1);
        let expiry = burst[0].born + burst[0].spec.lifetime as u64;
        while w.now < expiry {
            w.step();
        }
        assert!(w.bullets.iter().all(|b| b.owner != id));
        assert_eq!(w.players[&player].character.kills, 1);
        println!(
            "friendly collision batch={batch}: eight deferred rays, next-step collision, native ACK/replay, one defeat, lifetime expiry"
        );
    }
}

#[test]
fn abyss_projectile_lethal_flamer_world_step_rejects_over_cap() {
    let (mut w, _, id) = abyss_projectile_flamer(2041);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.step()))
        .expect_err("2041 detached +8 must refuse");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .unwrap();
    assert!(
        message.contains("terminal projectile budget exhausted"),
        "{message}"
    );
    assert!(
        w.bullets.iter().all(|b| b.owner != id),
        "no partial burst on refusal"
    );
    // The panicked World is discarded, never resumed as a healthy simulation.
}

#[test]
fn abyss_armed_missile_nonparalyzed_world_step_stays_stationary() {
    let (mut w, player) = fixture();
    w.encounter = true;
    w.name = "Abyss of Demons".into();
    w.players.get_mut(&player).unwrap().controller = Controller::Human;
    let id = w
        .spawn_entity(0x909, Pos { x: 17.5, y: 12.5 }, vec![])
        .unwrap();
    let initial = w.entities[&id].pos;
    w.step();
    assert!(
        w.entities[&id].pos.distance(initial) > 0.,
        "baseline pre-arm chase must move"
    );
    assert!(w.entities[&id].stage.is_none());
    w.damage_enemy(player, id, 1, true, &[], None).unwrap();
    let armed_pos = w.entities[&id].pos;
    w.step();
    let armed_at = w.now;
    assert_eq!(w.entities[&id].stage, Some(1));
    assert_eq!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::PARALYZED),
        0
    );
    for _ in 0..12 {
        assert_eq!(
            w.entities[&id].pos.distance(armed_pos),
            0.,
            "armed missile moved at {}",
            w.now
        );
        assert!(w.bullets.iter().all(|b| b.owner != id));
        w.step();
    }
    assert_eq!(w.now, armed_at + 600);
    assert_eq!(w.entities[&id].pos.distance(armed_pos), 0.);
    assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 8);
    w.step();
    assert!(!w.entities.contains_key(&id));
    assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 8);
    assert_eq!(w.players[&player].character.kills, 0);
    assert!(w.entities.values().all(|e| e.items.is_empty()));
    println!(
        "non-paralyzed missile moved before arm, stationary600ms, one eight-ray burst, expiry+50ms/no kill"
    );
}

#[test]
fn abyss_generated_abyss_guard_activity_twenty_tiles_realm_unchanged() {
    let content = tests::world().content.clone();
    for name in ["Abyss of Demons", "Realm"] {
        for distance in [12., 16., 19.5, 20.] {
            let map = abyss::generate(&content, 0).unwrap();
            let mut w = World::new(content.clone(), Arc::new(map)).unwrap();
            w.name = name.into();
            w.encounter = true;
            let guard = w.entities.values().find(|e| e.kind == 0x651).unwrap().id;
            assert!(
                w.entities[&guard].parent.is_none(),
                "generated guards are not boss-owned children"
            );
            let position = w.entities[&guard].pos;
            for other in w
                .entities
                .keys()
                .copied()
                .filter(|id| *id != guard)
                .collect::<Vec<_>>()
            {
                w.remove_entity(other);
            }
            let pos = [
                (distance, 0.),
                (-distance, 0.),
                (0., distance),
                (0., -distance),
            ]
            .into_iter()
            .map(|(dx, dy)| Pos {
                x: position.x + dx,
                y: position.y + dy,
            })
            .find(|pos| w.walkable(*pos))
            .expect("seeded arena must have walkable exact-range player position");
            let ch = tests::ch(&w);
            let player = w
                .enter_at(
                    ch,
                    None,
                    Some(pos),
                    AccountPresentation {
                        guild: None,
                        fame: 0,
                        best_fame: BTreeMap::new(),
                        credits: 0,
                    },
                )
                .unwrap();
            assert_eq!(w.players[&player].pos.distance(position), distance);
            w.step();
            let active = name == "Abyss of Demons" && distance < 20.;
            assert_eq!(
                w.entities[&guard].activated.is_some(),
                active,
                "{name} generated guard distance={distance}"
            );
            assert!(
                w.bullets.iter().all(|b| b.owner != guard),
                "activity is not ten-tile aimed acquisition"
            );
            println!(
                "generated guard world={name} distance={distance} active={active}, outside aimed range"
            );
        }
    }
}

#[test]
fn abyss_cowardice_routes_living_character_to_realm_not_nexus() {
    let mut d = tests::directory();
    for template in ["snake-pit", "abyss-of-demons"] {
        let origin = d.instance(template, None).unwrap();
        let w = d.worlds.get_mut(&origin).unwrap();
        let player = w.enter(tests::ch(w), None).unwrap();
        let w = &d.worlds[&origin];
        let character = w.players[&player].character.clone();
        let portal = w.entities.values().find(|e| e.kind == 0x703).unwrap().id;
        assert_eq!(
            d.destination(player, None).unwrap().2,
            -2,
            "real Nexus escape changed"
        );
        let (account, id, destination) = d.destination(player, Some(portal)).unwrap();
        println!("{template} Cowardice destination: {destination}");
        assert!(
            d.endgame.realms.contains_key(&destination) && d.worlds[&destination].admission_open,
            "Cowardice must return to an open Realm, not Nexus"
        );
        assert_eq!((account, id), (character.account, character.id));
        let (time, grant, _) = d.issue(account, id, destination).unwrap();
        let ch = d.worlds.get_mut(&origin).unwrap().leave(player).unwrap();
        assert_eq!(
            d.consume(account, destination, time, grant.clone()).unwrap(),
            character.id
        );
        let arrival = d
            .worlds
            .get_mut(&destination)
            .unwrap()
            .enter_admitted(
                ch,
                None,
                None,
                AccountPresentation {
                    guild: None,
                    fame: 0,
                    best_fame: BTreeMap::new(),
                    credits: 0,
                },
                Some(grant),
            )
            .unwrap();
        let ch = &d.worlds[&destination].players[&arrival].character;
        assert_eq!(
            (ch.account, ch.id, ch.hp, ch.alive),
            (character.account, character.id, character.hp, true)
        );
        d.worlds
            .get_mut(&destination)
            .unwrap()
            .leave(arrival)
            .unwrap();
    }
}
