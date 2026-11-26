use super::*;
use std::time::Duration;

pub(super) fn fixture(ground: u8, wall: bool) -> (World, i32) {
    fixture_map(ground, wall.then_some((9, 8)), 32)
}

fn fixture_map(ground: u8, wall: Option<(usize, usize)>, size: usize) -> (World, i32) {
    let content = Arc::new(test_content());
    let mut map = Map {
        elevation: Some(vec![200; size * size].into()),
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
    let mut world = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    let player = test_player(&mut world);
    (world, player)
}

pub(super) fn test_player(world: &mut World) -> i32 {
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
    id
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
fn audit_observation_preserves_work_results_and_mutations() {
    let (mut world, id) = fixture(0x18, false);
    let hp = world.players[&id].character.hp;
    let result = world.audit_native(
        id,
        "move",
        serde_json::json!({"tick":1,"time":1000,"x":8.5,"y":8.5}),
        |w| {
            w.players.get_mut(&id).unwrap().character.hp -= 1;
            Ok(42)
        },
    );
    assert_eq!(result.unwrap(), 42);
    assert_eq!(world.players[&id].character.hp, hp - 1);
    let rejected: Result<()> = world.audit_native(
        id,
        "item",
        serde_json::json!({"time":1000,"object":id,"slot":4,"item":2595,"x":8.5,"y":8.5}),
        |_| Err(controller("audit-test rejection")),
    );
    assert_eq!(rejected.unwrap_err().to_string(), "audit-test rejection");
    assert_eq!(world.players[&id].character.hp, hp - 1);
}

#[test]
fn lair_slime_children_start_on_the_defeated_parents_floor() {
    for (parent, child, count) in [(0xd9b, 0xd9c, 4), (0xd9c, 0xd9d, 4), (0xd9e, 0xd9f, 6)] {
        let (mut world, _) = fixture(0x18, true);
        world.encounter = true;
        let pos = Pos { x: 8.9, y: 8.5 };
        assert!(world.walkable(pos));
        let id = world.spawn_entity(parent, pos, vec![]).unwrap();
        world.defeat_entity(id).unwrap();
        for _ in 0..4 {
            crate::world::xml::tests::drain_events(&mut world).unwrap();
        }
        let children: Vec<_> = world.actors().filter(|e| e.kind == child).collect();
        assert_eq!(children.len(), count);
        // XML MetamorphoseAtDeath keeps each child within its default 0.1-tile posJitter of the corpse.
        assert!(
            children
                .iter()
                .all(|e| e.pos.distance(pos) <= 0.1 + 1e-4 && world.walkable(e.pos))
        );
    }
}

#[test]
fn native_entry_uses_ingress_time_and_keeps_monotonic_moves() {
    let (mut world, id) = fixture(0x18, false);
    let character = world.players.remove(&id).unwrap().character;
    world.clock.set_paused(true);
    world.attack_epoch = world.clock.now() - Duration::from_millis(1000);
    world.now = 1050;
    let (packets, _packets) = mpsc::channel(128);
    let (lost, _lost) = watch::channel(None);
    let id = world
        .enter(
            character,
            Some(Output {
                trace: None,
                packets,
                lost,
            }),
        )
        .unwrap();
    assert_eq!(world.players[&id].last_move_at, 1000);
    assert_eq!(
        native(&mut world, id, 8.5, 8.5, 10, 1001).unwrap(),
        NativeInput::Applied
    );
    assert!(
        native(&mut world, id, 8.5, 8.5, 11, 1000)
            .unwrap_err()
            .to_string()
            .contains("native ingress clock regressed")
    );
    let player = world.players.remove(&id).unwrap();
    let id = world
        .enter_admitted(
            player.character,
            player.tx,
            None,
            player.presentation,
            player.legendary,
            None,
            Controller::Autonomous,
        )
        .unwrap();
    assert_eq!(world.players[&id].last_move_at, 1050);
    let character = world.players.remove(&id).unwrap().character;
    let id = world.enter(character, None).unwrap();
    assert_eq!(world.players[&id].last_move_at, 1050);
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
    for (template, allowed) in [
        ("ocean-trench", true),
        ("spider-den", true),
        ("tomb-of-the-ancients", true),
        ("vault", false),
        ("tutorial", false),
    ] {
        world.xml.template = Some(template.into());
        let name = "Renamed world";
        world.name = name.into();
        assert_eq!(world.allow_teleport(), allowed, "{template}");
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
fn slowed_overrides_speed_and_speedy_until_expiry_but_not_paralysis() {
    for speed in [0, 50, 75] {
        let (mut world, id) = fixture(0x18, false);
        world.now = 1000;
        world.players.get_mut(&id).unwrap().character.base[4] = speed;
        world
            .apply_condition(id, crate::condition::SPEEDY, 6000)
            .unwrap();
        world
            .apply_condition(id, crate::condition::SLOWED, 3000)
            .unwrap();
        world.now = 2000;
        world
            .apply_condition(id, crate::condition::SLOWED, 500)
            .unwrap();
        assert_eq!(
            world.players[&id].statuses[&crate::condition::SLOWED],
            (2000, 4000)
        );
        for now in [2000, 3999] {
            assert_eq!(
                world.players[&id].speed(&world.content, now).unwrap(),
                0.004
            );
        }
        world
            .apply_condition(id, crate::condition::PARALYZED, 1000)
            .unwrap();
        assert_eq!(world.players[&id].speed(&world.content, 2999).unwrap(), 0.);
        assert_eq!(
            world.players[&id].speed(&world.content, 3000).unwrap(),
            0.004
        );
        let normal = 0.004 + 0.0056 * speed as f32 / 75.;
        assert!(
            (world.players[&id].speed(&world.content, 4000).unwrap() - normal * 1.5).abs()
                < 0.000001
        );
        assert!(
            (world.players[&id].speed(&world.content, 7000).unwrap() - normal).abs() < 0.000001
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
    p.tx = Some(Output {
        trace: None,
        packets: tx,
        lost,
    });
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

#[test]
fn ordinary_reproduction_waits_until_the_parent_is_out_of_view() {
    for kind in [0x602, 0x659, 0x652] {
        let (mut world, player) = fixture_map(0x18, None, 128);
        let terrain = world.content.objects[&kind].terrain;
        for cell in &mut Arc::make_mut(&mut world.map).cells {
            cell.terrain = terrain;
        }
        world.rng = rand::rngs::StdRng::seed_from_u64(42);
        let center = Pos { x: 64.5, y: 64.5 };
        let parent = world.place_population_solo(kind, center).unwrap();
        world.reproduction.insert((parent, 0), 0);
        let p = world.players.get_mut(&player).unwrap();
        p.pos = Pos {
            x: center.x + 19.99,
            y: center.y,
        };
        p.paused = true;
        world
            .apply_condition(player, crate::condition::INVISIBLE, 60000)
            .unwrap();
        world.reproduce(parent, 1).unwrap();
        assert_eq!(world.population.len(), 1, "watched {kind:#x} reproduced");
        assert_eq!(world.reproduction[&(parent, 0)], 60000);
        world.players.get_mut(&player).unwrap().pos.x = center.x + 30.;
        world.reproduce(parent, 60000).unwrap();
        assert_eq!(
            world.population.len(),
            1,
            "refusal must consume the normal cooldown"
        );
        world.reproduce(parent, 1).unwrap();
        assert!(
            world.population.len() > 1,
            "unwatched {kind:#x} cannot recover"
        );
        assert!(
            world
                .population
                .iter()
                .skip(1)
                .all(|slot| { slot.pos.distance(world.players[&player].pos) >= 20. })
        );
    }
}

#[test]
fn ordinary_reproduction_cannot_enter_any_players_view() {
    for kind in [0x602, 0x659, 0x652] {
        let (mut world, first) = fixture_map(0x18, None, 128);
        for cell in &mut Arc::make_mut(&mut world.map).cells {
            cell.terrain = world.content.objects[&kind].terrain;
        }
        let center = Pos { x: 64.5, y: 64.5 };
        let parent = world.place_population_solo(kind, center).unwrap();
        for i in 0..8 {
            let player = if i == 0 {
                first
            } else {
                let mut ch = world.players[&first].character.clone();
                ch.id = i + 1;
                ch.account = i + 1;
                world.enter(ch, None).unwrap()
            };
            let angle = i as f32 * std::f32::consts::TAU / 8.;
            world.players.get_mut(&player).unwrap().pos = Pos {
                x: center.x + 21. * angle.cos(),
                y: center.y + 21. * angle.sin(),
            };
        }
        world.reproduction.insert((parent, 0), 0);
        world.reproduce(parent, 1).unwrap();
        assert_eq!(
            world.population.len(),
            1,
            "unseen {kind:#x} spawned into a player's view"
        );
    }
}

#[test]
fn reproduced_families_keep_their_entire_footprint_out_of_view() {
    let mut births = 0;
    for seed in 0..8 {
        let (mut world, player) = fixture_map(0x18, None, 128);
        for cell in &mut Arc::make_mut(&mut world.map).cells {
            cell.terrain = 1;
        }
        world.rng = rand::rngs::StdRng::seed_from_u64(seed);
        let parent = world
            .place_population_solo(0x652, Pos { x: 64.5, y: 64.5 })
            .unwrap();
        world.players.get_mut(&player).unwrap().pos = Pos { x: 85., y: 64.5 };
        world.reproduction.insert((parent, 0), 0);
        world.reproduce(parent, 1).unwrap();
        births += world.population.len() - 1;
        assert!(
            world
                .population
                .iter()
                .skip(1)
                .all(|slot| { slot.pos.distance(world.players[&player].pos) >= 20. }),
            "family crossed the view boundary for seed {seed}"
        );
    }
    assert!(births > 0, "safe family reproduction was disabled");
}

#[test]
fn snakes_reproduce_back_to_kithio_density() {
    let (mut world, _) = fixture(0x18, false);
    for cell in &mut Arc::make_mut(&mut world.map).cells {
        cell.terrain = 11;
    }
    let snakes: Vec<_> = [
        (24.5, 24.5),
        (25.5, 24.5),
        (24.5, 25.5),
        (23.5, 24.5),
        (24.5, 23.5),
    ]
    .map(|(x, y)| world.place_population_solo(0x602, Pos { x, y }).unwrap())
    .into();
    let count = |world: &World| world.actors().filter(|e| e.kind == 0x602).count();
    let pass = |world: &mut World| {
        for _ in 0..20 {
            world.now += 50;
            world.reproduce_active().unwrap();
        }
    };
    let cycle = |world: &mut World| {
        world.now += 60000;
        pass(world);
        pass(world);
    };
    cycle(&mut world);
    assert_eq!(count(&world), 5, "densityMax 5 within 10 tiles is full");
    world.remove_entity(snakes[0]);
    cycle(&mut world);
    assert_eq!(count(&world), 5, "survivors refill the missing Snake");
}

#[test]
fn oryx_culls_excess_away_from_players() {
    let (mut world, player) = fixture(0x18, false);
    for cell in &mut Arc::make_mut(&mut world.map).cells {
        cell.terrain = 11;
    }
    world.population_regions[11].target = 2;
    let near = world.players[&player].pos;
    let kept = world.place_population_solo(0x602, near).unwrap();
    for x in [20.5, 23.5, 26.5, 29.5] {
        world
            .place_population_solo(0x602, Pos { x, y: 24.5 })
            .unwrap();
    }
    world.cull_population(11);
    assert_eq!(world.population_count(11), 2);
    assert!(world.entities.contains_key(&kept));
}

#[test]
fn oryx_counts_family_members_once_summoned() {
    // Metal Golem's eager Gray Satellites 1-2 are camp members its XML does not spawn.
    let (mut world, _) = super::population::camp_fixture(0x62d);
    assert_eq!(world.oryx_counted(7).len(), 1, "inactive leader only");
    assert_eq!(world.oryx_counts()[7], world.oryx_counted(7).len());
    let leader = world.camp_members(0).find(|e| e.kind == 0x62d).unwrap().id;
    world.entities.get_mut(&leader).unwrap().activated = Some(0);
    assert_eq!(world.oryx_counted(7).len(), 3, "active leader summoned");
    assert_eq!(world.oryx_counts()[7], world.oryx_counted(7).len());
    world.remove_entity(leader);
    assert_eq!(world.oryx_counted(7).len(), 2, "orphans still count");
    assert_eq!(world.oryx_counts()[7], world.oryx_counted(7).len());
}
