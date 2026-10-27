use super::*;
use std::time::Duration;

fn fixture(ground: u8, wall: bool) -> (World, i32) {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let content = Arc::new(Content::load(&data).unwrap());
    let mut map = Map {
        width: 32,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            32 * 32
        ],
        spawns: vec![(8.5, 8.5)],
    };
    if wall {
        map.cells[8 * 32 + 9].ground = 255;
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
