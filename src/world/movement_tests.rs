use super::*;
use std::time::Duration;

pub(super) fn fixture(ground: u8, wall: bool) -> (World, i32) {
    fixture_map(ground, wall.then_some((9, 8)), 32)
}

fn fixture_map(ground: u8, wall: Option<(usize, usize)>, size: usize) -> (World, i32) {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let content = Arc::new(Content::load(&data).unwrap());
    let mut map = Map {
        width: size as i32,
        height: size as i32,
        cells: vec![
            crate::content::Cell {
                ground,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            size * size
        ],
        spawns: vec![(8.5, 8.5)],
    };
    if let Some((x, y)) = wall {
        map.cells[y * size + x].ground = 255;
    }
    let mut world = World::new(content, Arc::new(map)).unwrap();
    let character: Character = serde_json::from_value(serde_json::json!({
        "schema":1,"lifetime":crate::store::FameLifetime::complete(),
        "class":0x300,"base":[200,100,15,0,75,15,15,10],
        "id":1,"account":1,"name":"Movement","level":20,"exp":0,
        "fame":0,"hp":200,"mp":100,"revision":0,"alive":true,
        "items":[0xa14,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
    }))
    .unwrap();
    let id = world.enter(character, None).unwrap();
    let p = world.players.get_mut(&id).unwrap();
    p.controller = Controller::Human;
    p.last_move = 1000;
    p.last_move_at = 1000;
    p.movement_credit = 0.78;
    (world, id)
}

fn native(
    world: &mut World,
    id: i32,
    x: f32,
    y: f32,
    time: i32,
    received: u64,
) -> Result<NativeInput> {
    world.native_move_received(
        id,
        Pos { x, y },
        (time, world.attack_epoch + Duration::from_millis(received)),
    )
}

#[test]
fn hobbit_mage_wakes_inside_twelve_tiles_and_emits_three_offset_rings() {
    let (mut world, player) = fixture(0x18, false);
    world.encounter = true;
    let mage = world
        .spawn_entity(0x617, Pos { x: 20.5, y: 8.5 }, vec![])
        .unwrap();
    world.npc_step();
    assert!(world.entities[&mage].activated.is_none());
    world.players.get_mut(&player).unwrap().pos.x = 9.;
    world.npc_step();
    assert!(world.entities[&mage].activated.is_some());
    for (age, index) in [(0, 0), (400, 1), (800, 2), (1200, 0)] {
        let shots = crate::encounter::attacks::schedule(0x617, 200, age)
            .unwrap()
            .snapshot(1.);
        assert_eq!(shots.len(), 15);
        for (i, (projectile, angle)) in shots.into_iter().enumerate() {
            assert_eq!(projectile, index);
            assert!((angle.to_degrees() - (i as f32 * 24. + index as f32 * 8.)).abs() < 0.001);
        }
    }
}

#[test]
fn goblin_attacks_keep_independent_channels_and_phase_prediction() {
    use crate::encounter::attacks::Prediction;
    for (hp, stage, intervals, prediction) in [
        (280, 0, [1050, 1350], 0.35),
        (182, 0, [1050, 1350], 0.35),
        (181, 1, [1050, 1050], 0.25),
    ] {
        assert_eq!(crate::encounter::stage(0x614, hp, 0), stage);
        let schedule = crate::encounter::attacks::schedule(0x614, hp, 0).unwrap();
        assert_eq!(schedule.state, stage);
        for (attack, interval) in schedule.attacks.iter().zip(intervals) {
            assert_eq!(attack.interval, Some(interval));
            assert_eq!(attack.range, 8.);
            assert!(matches!(attack.predict, Prediction::Lead(p) if p == prediction));
        }
    }
    let (mut world, player) = fixture(0x18, false);
    let target = world.players[&player].pos;
    let previous = Pos {
        x: target.x,
        y: target.y - 0.5,
    };
    world
        .players
        .get_mut(&player)
        .unwrap()
        .white_demon_aim_history = [previous, target];
    let pos = Pos {
        x: target.x - 4.,
        y: target.y,
    };
    let mage = world.spawn_entity(0x614, pos, vec![]).unwrap();
    for (hp, factor) in [(280, 0.35), (181, 0.25)] {
        let schedule = crate::encounter::attacks::schedule(0x614, hp, 0).unwrap();
        let shots = world
            .scheduled_attacks(mage, Some(target), &schedule)
            .unwrap();
        assert_eq!(shots.len(), 2);
        for (index, angle) in shots {
            let expected = -(previous.y - pos.y).atan2(previous.x - pos.x) / 0.1
                * (world.content.objects[&0x614].projectiles[&index].speed / 100.)
                * factor;
            assert!((angle - expected).abs() < 0.00001);
        }
    }
    for (age, count) in [(0, 2), (1050, 1), (1350, 1), (2100, 1)] {
        assert_eq!(
            crate::encounter::phased_attacks(0x614, 280, 0., age).len(),
            count
        );
    }
}

#[test]
fn goblin_birth_budget_survives_escort_deaths_and_parent_loss() {
    let (mut world, player) = fixture(0x18, false);
    for cell in &mut Arc::make_mut(&mut world.map).cells {
        cell.terrain = 10;
    }
    world.population_regions[10].limit = 100;
    let family = super::population::FAMILIES
        .iter()
        .find(|f| f.leader == 0x614)
        .unwrap();
    let center = Pos { x: 16.5, y: 8.5 };
    let positions = world.family_layout(center, family, &[]).unwrap();
    world.place_camp(family, positions).unwrap();
    let mage = world.camp_members(0).find(|e| e.kind == 0x614).unwrap().id;
    let mut born: BTreeSet<_> = world.camp_members(0).map(|e| e.id).collect();
    for turn in 1..=6 {
        for kind in [0x612, 0x613] {
            let victim = world.camp_members(0).find(|e| e.kind == kind).unwrap().id;
            world.remove_entity(victim);
        }
        world.now = turn * 12000;
        world.repopulate_families().unwrap();
        born.extend(world.camp_members(0).map(|e| e.id));
    }
    assert_eq!(
        born.len(),
        15,
        "Mage plus seven lifetime births of each escort"
    );
    assert_eq!(crate::encounter::stage(0x614, 182, 0), 0);
    let escort = world.camp_members(0).find(|e| e.kind == 0x612).unwrap().id;
    assert!(matches!(
        world
            .family_movement(escort, Some(world.players[&player].pos), 0)
            .unwrap()
            .0,
        crate::encounter::Motion::Protect { .. } | crate::encounter::Motion::Wander
    ));
    assert!(matches!(
        world
            .family_movement(escort, Some(world.players[&player].pos), 1200)
            .unwrap()
            .0,
        crate::encounter::Motion::Orbit { radius: 7., .. }
    ));
    world.entities.get_mut(&mage).unwrap().hp = 181;
    assert!(matches!(
        world
            .family_movement(escort, Some(world.players[&player].pos), 1200)
            .unwrap()
            .0,
        crate::encounter::Motion::Chase { stop: 1.5, .. }
    ));
    world.remove_entity(mage);
    let survivors: BTreeSet<_> = world.camp_members(0).map(|e| e.id).collect();
    world.now += 12000;
    world.repopulate_families().unwrap();
    assert_eq!(
        world.camp_members(0).map(|e| e.id).collect::<BTreeSet<_>>(),
        survivors
    );
}

#[test]
fn undead_hobbit_mage_has_separate_ring_activation_and_aimed_shot_ranges() {
    let (mut world, player) = fixture(0x18, false);
    world.encounter = true;
    let mage = world
        .spawn_entity(0x61a, Pos { x: 20.5, y: 8.5 }, vec![])
        .unwrap();
    world.npc_step();
    assert!(world.entities[&mage].activated.is_none());
    for (age, distance, aimed) in [(0, 11., false), (1000, 9., true), (2000, 11., false)] {
        world.now = age;
        let pos = world.entities[&mage].pos;
        let target = Pos {
            x: pos.x - distance,
            y: pos.y,
        };
        let schedule = crate::encounter::attacks::schedule(0x61a, 270, age).unwrap();
        let shots = world
            .scheduled_attacks(mage, Some(target), &schedule)
            .unwrap();
        assert_eq!(
            shots.iter().filter(|(p, _)| *p == 3).count(),
            usize::from(aimed)
        );
        if age == 0 {
            assert_eq!(shots.iter().filter(|(p, _)| *p == 0).count(), 15);
        }
    }
    world.players.get_mut(&player).unwrap().pos.x = 9.;
    world.npc_step();
    assert!(world.entities[&mage].activated.is_some());
}

#[test]
fn sand_phantom_proximity_cycle_preserves_credit_without_a_death() {
    use crate::encounter::Motion;
    let (mut world, player) = fixture(0x18, false);
    let pos = Pos { x: 12.5, y: 8.5 };
    let phantom = world.spawn_entity(0x6c5, pos, vec![]).unwrap();
    let xp = world.players[&player].character.exp;
    let old = world.entities.get_mut(&phantom).unwrap();
    old.hp = 17;
    old.damage.insert(player, 50);
    let far = Pos { x: 7.5, y: 8.5 };
    world.fauna_mechanics(phantom, Some(far), 50000).unwrap();
    assert_eq!(world.entities[&phantom].hp, 17);
    world.fauna_mechanics(phantom, None, 50000).unwrap();
    let near = Pos { x: 9.5, y: 8.5 };
    world.fauna_mechanics(phantom, Some(near), 0).unwrap();
    assert!(!world.entities.contains_key(&phantom));
    let wisp = world
        .entities
        .values()
        .find(|e| e.kind == 0x7f22)
        .unwrap()
        .id;
    assert_eq!(world.entities[&wisp].hp, 150);
    assert_eq!(world.entities[&wisp].damage[&player], 50);
    for age in [0, 799, 1600] {
        assert_eq!(
            world.fauna_movement(wisp, Some(near), age).unwrap(),
            (
                Motion::Retreat { distance: 5. },
                crate::encounter::nr_speed(0.6)
            )
        );
    }
    for (target, age) in [(Some(near), 800), (Some(far), 0), (None, 0)] {
        assert_eq!(
            world.fauna_movement(wisp, target, age).unwrap(),
            (Motion::Wander, crate::encounter::nr_speed(0.9))
        );
    }
    world.fauna_mechanics(wisp, None, 6499).unwrap();
    assert!(world.entities.contains_key(&wisp));
    world.now = 6500;
    world.fauna_mechanics(wisp, None, 6500).unwrap();
    assert!(!world.entities.contains_key(&wisp));
    let returned = world.entities.values().find(|e| e.kind == 0x6c5).unwrap();
    assert_eq!(returned.hp, 230);
    assert_eq!(returned.damage[&player], 50);
    assert_eq!((returned.pos.x, returned.pos.y), (pos.x, pos.y));
    assert_eq!((returned.home.x, returned.home.y), (pos.x, pos.y));
    assert_eq!(returned.activated, Some(6500));
    assert_eq!(
        world
            .entities
            .values()
            .filter(|e| matches!(e.kind, 0x6c5 | 0x7f22))
            .count(),
        1
    );
    assert_eq!(world.players[&player].character.exp, xp);
}

#[test]
fn swarm_directed_shots_use_constant_angular_prediction_and_cooldown() {
    let (mut world, player) = fixture(0x18, false);
    let target = world.players[&player].pos;
    let previous = Pos {
        x: target.x,
        y: target.y - 0.5,
    };
    world
        .players
        .get_mut(&player)
        .unwrap()
        .white_demon_aim_history = [previous, target];
    let start = Pos {
        x: target.x - 3.,
        y: target.y,
    };
    let swarm = world.spawn_entity(0x955, start, vec![]).unwrap();
    let heading = (target.y - start.y).atan2(target.x - start.x);
    let expected = heading
        + (heading - (previous.y - start.y).atan2(previous.x - start.x)) / 0.1 * (65. / 100.) * 0.1;
    let mut fired = vec![];
    for age in (0..3000).step_by(50) {
        world.now = age;
        let schedule = crate::encounter::fauna::schedule(0x955, 360, age);
        for (projectile, angle) in world
            .scheduled_attacks(swarm, Some(target), &schedule)
            .unwrap()
        {
            assert_eq!(projectile, 0);
            assert!(
                (angle - expected).abs() < 0.00001,
                "age={age}: {angle} != {expected}"
            );
            fired.push(age);
        }
    }
    assert_eq!(fired, [0, 550, 1100, 1650, 2200, 2750]);
}

#[test]
fn native_berserk_shots_survive_position_correction() {
    for (until, condition) in [
        (0, 0),
        (3000, 0),
        (1900, 0),
        (1000, 0),
        (3000, crate::condition::bit(crate::condition::DAZED)),
    ] {
        let (mut world, id) = fixture(0x18, false);
        world.now = 2000;
        let p = world.players.get_mut(&id).unwrap();
        p.statuses.insert(crate::condition::BERSERK, (0, until));
        p.lab_conditions = condition;
        let kind = p.character.items[0] as u16;
        let start = Pos {
            x: p.pos.x + 0.3,
            y: p.pos.y,
        };
        let period = (1.
            / (0.0015 + 0.0065 * 15. / 75.)
            / if until > 1000 { 1.5 } else { 1. }
            / f64::from(world.content.get(kind).unwrap().rate))
        .floor() as i32;
        let shoot = |world: &mut World, time, bullet| {
            world.native_shoot_received(
                id,
                (time, world.attack_epoch + Duration::from_millis(2000)),
                bullet,
                kind,
                start,
                0.,
            )
        };
        shoot(&mut world, 2000, 1).unwrap();
        let shots = world.players[&id].character.shots;
        let projectiles = world.bullets.len();
        world
            .correct_native_shot(id, 2000 + period, 2, kind, "fixture correction")
            .unwrap();
        world
            .players
            .get_mut(&id)
            .unwrap()
            .goto_acks
            .push_back((true, Instant::now()));
        assert_eq!(
            shoot(&mut world, 2000 + 2 * period, 3).unwrap(),
            NativeInput::DiscardedUntilGotoAck
        );
        let p = &world.players[&id];
        assert_eq!(p.next_bullet, 4);
        assert_eq!(p.character.shots, shots);
        assert_eq!(world.bullets.len(), projectiles);
        let error = shoot(&mut world, 2001 + 2 * period, 4).unwrap_err();
        assert!(error.to_string().contains("discarded attack too fast"));
        assert_eq!(world.players[&id].next_bullet, 4);
        world.acknowledge_goto(id, 2000 + 2 * period).unwrap();
        assert_eq!(
            shoot(&mut world, 2000 + 3 * period, 4).unwrap(),
            NativeInput::Applied
        );
        assert_eq!(world.players[&id].character.shots, shots + 1);
    }
}

#[test]
fn ocean_trench_advertises_player_teleports_without_enabling_restricted_worlds() {
    let (mut world, _) = fixture(0x18, false);
    for (name, allowed) in [
        ("Ocean Trench", true),
        ("Vault", false),
        ("Tutorial", false),
    ] {
        world.name = name.into();
        assert_eq!(world.allow_teleport(), allowed, "{name}");
        let packet = world.map_info();
        let mut r = wire::Reader::new(&packet);
        r.i32().unwrap();
        r.i32().unwrap();
        assert_eq!(r.utf().unwrap(), name);
        r.i32().unwrap();
        r.i32().unwrap();
        assert_eq!(r.u8().unwrap(), u8::from(allowed), "{name} MAPINFO");
    }
}

#[test]
fn native_shots_trust_client_placement_without_moving_the_player() {
    for start in [
        Pos {
            x: 49.52554,
            y: 22.446217,
        },
        Pos { x: 49.8, y: 23.5 },
    ] {
        let (mut world, id) = fixture_map(0x18, Some((49, 23)), 64);
        world.now = 9000;
        let p = world.players.get_mut(&id).unwrap();
        p.pos = Pos {
            x: 50.370243,
            y: 22.5,
        };
        p.last_move = 10214295;
        p.last_move_at = 8931;
        let kind = p.character.items[0] as u16;
        assert!(world.player_step_valid(
            world.players[&id].pos,
            Pos {
                x: 49.820675,
                y: 22.5
            }
        ));
        assert!(
            world
                .shoot(id, 10214371, 1, kind, start, -2.9613369)
                .is_err()
        );
        for (time, bullet) in [(10214371, 1), (10215371, 2)] {
            assert_eq!(
                world
                    .native_shoot_received(
                        id,
                        (time, world.attack_epoch + Duration::from_millis(9000)),
                        bullet,
                        kind,
                        start,
                        -2.9613369,
                    )
                    .unwrap(),
                NativeInput::Applied
            );
        }
        let p = &world.players[&id];
        assert_eq!(
            (
                p.pos.x,
                p.pos.y,
                p.last_move,
                p.last_move_at,
                p.movement_credit
            ),
            (50.370243, 22.5, 10214295, 8931, 0.78)
        );
        assert_eq!(p.character.shots, 2);
        assert!(p.goto_acks.is_empty());
        assert_eq!(world.bullets.len(), 2);
        assert!(
            world
                .bullets
                .iter()
                .all(|b| b.owner == id && b.start.x == start.x && b.start.y == start.y)
        );
    }
}

#[test]
fn native_movement_trusts_client_positions_despite_speed_delay_or_terrain() {
    for (label, wall, x, time, received) in [
        ("speed", false, 12.5, 1100, 1100),
        ("batched ingress", false, 10.5, 1350, 1000),
        ("long interruption", false, 11.5, 7000, 7000),
        ("intervening obstacle", true, 10.5, 1350, 1350),
        ("collision disagreement", true, 9.5, 1100, 1100),
    ] {
        let (mut world, id) = fixture(0x18, wall);
        assert_eq!(
            native(&mut world, id, x, 8.5, time, received).unwrap(),
            NativeInput::Applied,
            "{label}"
        );
        let p = &world.players[&id];
        assert_eq!(
            (
                p.pos.x,
                p.pos.y,
                p.last_move,
                p.last_move_at,
                p.movement_credit
            ),
            (x, 8.5, time, received, 0.78),
            "{label}"
        );
        assert!(p.goto_acks.is_empty(), "{label}");
    }
}

#[test]
fn native_movement_rejects_malformed_or_inactive_updates_without_mutation() {
    for (x, y, time, received) in [
        (f32::NAN, 8.5, 1100, 1100),
        (8.5, f32::INFINITY, 1100, 1100),
        (-0.01, 8.5, 1100, 1100),
        (32., 8.5, 1100, 1100),
        (8.5, 32., 1100, 1100),
        (-1., 8.5, 1100, 1100),
        (9., 8.5, -1, 1100),
        (9., 8.5, 999, 1100),
        (9., 8.5, 1100, 999),
    ] {
        let (mut world, id) = fixture(0x18, false);
        assert!(native(&mut world, id, x, y, time, received).is_err());
        let p = &world.players[&id];
        assert_eq!(
            (p.pos.x, p.pos.y, p.last_move, p.last_move_at),
            (8.5, 8.5, 1000, 1000)
        );
    }
    for state in 0..4 {
        let (mut world, id) = fixture(0x18, false);
        let p = world.players.get_mut(&id).unwrap();
        match state {
            0 => p.character.alive = false,
            1 => p.departing = true,
            2 => p.death_pending = true,
            _ => p.persistence_failure = Some("held".into()),
        }
        assert!(native(&mut world, id, 9., 8.5, 1100, 1100).is_err());
        assert_eq!(world.players[&id].pos.x, 8.5);
    }
}

#[test]
fn native_sentinel_and_intentional_goto_ack_keep_their_protocol_roles() {
    let (mut world, id) = fixture(0x18, false);
    native(&mut world, id, -1., -1., 7000, 7000).unwrap();
    assert_eq!(world.players[&id].pos.x, 8.5);
    assert_eq!(world.players[&id].last_move, 7000);
    let (tx, mut rx) = mpsc::channel(16);
    let (lost, _) = watch::channel(None);
    let p = world.players.get_mut(&id).unwrap();
    p.tx = Some(Output { packets: tx, lost });
    let mut body = Writer::new();
    body.i32(id).f32(p.pos.x).f32(p.pos.y);
    reconciliation::goto(p, true, body.0);
    assert_eq!(rx.try_recv().unwrap().id, wire::GOTO);
    assert_eq!(
        native(&mut world, id, 20., 8.5, 7100, 7100).unwrap(),
        NativeInput::DiscardedUntilGotoAck
    );
    assert_eq!(world.players[&id].last_move, 7000);
    world.acknowledge_goto(id, 7100).unwrap();
    let p = world.players.get_mut(&id).unwrap();
    p.paused = true;
    p.lab_conditions = PARALYZED;
    native(&mut world, id, 21., 8.5, 7200, 7200).unwrap();
    assert_eq!(world.players[&id].pos.x, 21.);
    assert!(world.players[&id].goto_acks.is_empty());
    assert!(rx.try_recv().is_err());
}

#[test]
fn server_driven_movement_still_enforces_speed_ingress_and_collision() {
    for (wall, x, time, received) in [
        (false, 12.5, 1100, 1100),
        (false, 10.5, 1350, 1000),
        (true, 10.5, 1350, 1350),
    ] {
        let (mut world, id) = fixture(0x18, wall);
        assert!(
            world
                .move_timed(id, Pos { x, y: 8.5 }, time, received)
                .is_err()
        );
        let p = &world.players[&id];
        assert_eq!(
            (p.pos.x, p.last_move, p.last_move_at, p.movement_credit),
            (8.5, 1000, 1000, 0.78)
        );
    }
    let (mut world, id) = fixture(0x18, false);
    world
        .move_timed(id, Pos { x: 9.46, y: 8.5 }, 1100, 1100)
        .unwrap();
    assert!((world.players[&id].movement_credit - 0.78).abs() < 0.001);
}
