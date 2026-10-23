use super::*;
use crate::{database, wire::Reader, world::shore_runtime_tests as shore};
const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const WILLOW_X: f32 = 20.920_02;
fn packet(id: u8, f: impl FnOnce(&mut Writer)) -> Packet {
    let mut w = Writer::new();
    f(&mut w);
    Packet { id, body: w.0 }
}
fn movement(x: f32, y: f32) -> Packet {
    packet(5, |w| {
        w.f32(x).f32(y);
    })
}
fn drain(out: &mut mpsc::Receiver<Vec<Packet>>) -> Vec<Packet> {
    let mut all = vec![];
    while let Ok(p) = out.try_recv() {
        all.extend(p);
    }
    all
}
type MoveCase = (
    Simulation,
    u64,
    i32,
    mpsc::Receiver<Vec<Packet>>,
    u64,
    mpsc::Receiver<Vec<Packet>>,
);
fn setup() -> MoveCase {
    let (content, grounds) = shore::load(true);
    let mut s = Simulation::normal(
        content,
        grounds,
        Arc::new(Mutex::new(
            database(std::path::Path::new(":memory:")).unwrap(),
        )),
        31,
    )
    .unwrap();
    let mut enter = |guid: &str, game_id| {
        let (out, receive) = mpsc::channel(OUTPUT_CAPACITY);
        let session = s
            .join(
                protocol::Hello {
                    guid: guid.into(),
                    game_id,
                },
                out,
            )
            .unwrap();
        s.input(
            session,
            packet(3, |w| {
                w.i16(0x30e).utf("MovePaced");
            }),
            Instant::now(),
        )
        .unwrap();
        (session, s.sessions[&session].avatar.unwrap(), receive)
    };
    let (a, avatar, mut out) = enter(A, -1);
    let (b, _, mut peer) = enter(B, 0);
    let world = s.sessions[&a].world;
    assert_eq!(s.worlds[&world].map.as_ref().unwrap().seed, 32);
    let before = s.worlds[&world].clone();
    let p = s.worlds.get_mut(&world).unwrap().character_mut(avatar);
    p.x = WILLOW_X;
    p.y = 18.172981;
    s.commit(&s.worlds[&world], &before).unwrap();
    s.input(a, movement(WILLOW_X, 18.172981), Instant::now())
        .unwrap();
    drain(&mut out);
    drain(&mut peer);
    (s, a, avatar, out, b, peer)
}
async fn inspect(
    commands: &mpsc::Sender<Command>,
    f: impl FnOnce(&mut Simulation) + Send + 'static,
) {
    let (done, ready) = oneshot::channel();
    commands
        .send(Command::Inspect(Box::new(move |s| {
            f(s);
            done.send(()).unwrap();
        })))
        .await
        .unwrap();
    ready.await.unwrap();
}
async fn submit(
    commands: &mpsc::Sender<Command>,
    session: u64,
    packet: Packet,
    received: Instant,
) -> oneshot::Receiver<Result<(), String>> {
    let (reply, receive) = oneshot::channel();
    commands
        .send(Command::Input {
            session,
            packet,
            received,
            reply,
        })
        .await
        .unwrap();
    receive
}

#[tokio::test]
async fn move_pacing_willow_compression_real_owner_funding_and_held_dispatch() {
    let (s, a, avatar, mut out, b, mut peer) = setup();
    let saved = character(&s.db, A).unwrap().unwrap();
    let original = saved.clone();
    let (commands, rx) = mpsc::channel(64);
    let task = tokio::spawn(s.run(rx, false));
    tokio::time::sleep(Duration::from_nanos(215_787_441)).await;
    let mut encoder = crate::wire::Codec::server_with_keys(b"hPvJ(R}Mp$-cg", b"zCV2tsY0]s;|]");
    let p = movement(WILLOW_X, 20.08114);
    let bytes = encoder.frame(p.id, p.body);
    let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(
        axum::extract::ws::Message::Binary(bytes.into()),
    )]);
    let mut inputs = crate::input::receive(stream);
    let frame = inputs.recv().await.unwrap().unwrap();
    let axum::extract::ws::Message::Binary(ref bytes) = frame.message else {
        panic!("binary")
    };
    let mut codec = crate::protocol::server_codec();
    let p = codec.feed(bytes).unwrap().pop().unwrap();
    let mut waiting = submit(&commands, a, p, frame.received).await;
    inspect(&commands, move |s| {
        assert_eq!(
            s.deferred_weapons.len(),
            1,
            "fundable Willow shortage must wait, not reject"
        );
        let world = s.sessions[&a].world;
        assert_eq!(s.worlds[&world].character(avatar), &saved);
        assert_eq!(character(&s.db, A).unwrap().unwrap(), saved);
        assert_eq!(s.worlds[&world].elapsed, 0.);
    })
    .await;
    assert!(waiting.try_recv().is_err());
    assert!(drain(&mut out).is_empty());
    let other = submit(
        &commands,
        b,
        packet(7, |w| {
            w.utf("").i32(-1).utf("responsive");
        }),
        Instant::now(),
    )
    .await;
    other.await.unwrap().unwrap();
    assert!(drain(&mut peer).iter().any(|p| p.id == 7));
    let (wire, mut received) = mpsc::channel(8);
    let mut socket = Box::pin(futures_util::sink::unfold(
        wire,
        |wire, message| async move {
            wire.send(message)
                .await
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            Ok::<_, std::io::Error>(wire)
        },
    ));
    let output = submit(&commands, a, movement(WILLOW_X, 18.172981), frame.received).await;
    assert!(output.await.unwrap().unwrap_err().contains("backpressure"));
    // An ordinary owner tick is serviced while this input still owns its raw-frame permit.
    let (reply, receive) = oneshot::channel();
    commands
        .send(Command::Tick(Duration::from_millis(200), reply))
        .await
        .unwrap();
    receive.await.unwrap().unwrap();
    crate::input_reply(&mut socket, &mut codec, &mut out, waiting, a)
        .await
        .unwrap()
        .unwrap();
    let mut decoder = crate::wire::Codec::server_with_keys(b"hPvJ(R}Mp$-cg", b"zCV2tsY0]s;|]");
    let mut sent = vec![];
    while let Ok(axum::extract::ws::Message::Binary(bytes)) = received.try_recv() {
        sent.extend(decoder.feed(&bytes).unwrap());
    }
    assert!(sent.iter().any(|p| p.id == 13));
    inspect(&commands, move |s| {
        let world = s.sessions[&a].world;
        let p = s.worlds[&world].character(avatar);
        assert_eq!((p.x, p.y), (WILLOW_X, 20.08114));
        assert_eq!(character(&s.db, A).unwrap().unwrap(), *p);
        assert_eq!(p.resources_carry, original.resources_carry);
        assert!(s.deferred_weapons.is_empty());
    })
    .await;
    submit(&commands, a, movement(WILLOW_X, 20.08114), frame.received)
        .await
        .await
        .unwrap()
        .unwrap();
    drop(frame);
    assert!(inputs.recv().await.unwrap().is_none());
    drop(commands);
    task.await.unwrap().unwrap();
    // Decode a normal packet too, so the movement and outgoing paths share the production codec.
    let p = sent.iter().find(|p| p.id == 13).unwrap();
    assert!(Reader::new(&p.body).i16().unwrap() > 0);
}

fn command(s: &mut Simulation, a: u64, packet: Packet) -> oneshot::Receiver<Result<(), String>> {
    let (reply, receive) = oneshot::channel();
    s.submit_input(a, packet, Instant::now(), reply).unwrap();
    receive
}
fn pending(s: &Simulation, a: u64) -> &PendingMove {
    s.deferred_weapons[&a].movement.as_ref().unwrap()
}
async fn wake(s: &mut Simulation, a: u64) {
    tokio::time::sleep_until(pending(s, a).due.into()).await;
    s.release_weapons(Instant::now()).unwrap();
}

#[test]
fn move_pacing_invalid_and_over_bound_reject_without_sql_or_publication() {
    let (mut s, a, avatar, mut out, _, _peer) = setup();
    let world = s.sessions[&a].world;
    let saved = character(&s.db, A).unwrap().unwrap();
    s.db.lock().unwrap().execute_batch("CREATE TRIGGER reject_move BEFORE UPDATE ON build100_characters BEGIN SELECT RAISE(FAIL,'unexpected premature SQL'); END;").unwrap();
    for p in [
        movement(WILLOW_X, 28.),
        Packet {
            id: 5,
            body: [f32::NAN.to_be_bytes(), 20_f32.to_be_bytes()].concat(),
        },
        Packet {
            id: 5,
            body: [f32::INFINITY.to_be_bytes(), 20_f32.to_be_bytes()].concat(),
        },
        movement(-1., 20.),
        Packet {
            id: 5,
            body: vec![0; 7],
        },
        Packet {
            id: 5,
            body: vec![0; 9],
        },
    ] {
        assert!(command(&mut s, a, p).try_recv().unwrap().is_err());
        assert!(s.deferred_weapons.is_empty());
        assert_eq!(s.worlds[&world].character(avatar), &saved);
        assert_eq!(character(&s.db, A).unwrap().unwrap(), saved);
        assert!(drain(&mut out).is_empty());
    }
    let mut waiting = command(&mut s, a, movement(WILLOW_X, 19.07298));
    assert!(waiting.try_recv().is_err());
    assert_eq!(character(&s.db, A).unwrap().unwrap(), saved);
    assert!(drain(&mut out).is_empty());
}

#[tokio::test]
async fn move_pacing_sql_failure_preserves_pending_anchor_and_exact_retry() {
    let (mut s, a, avatar, mut out, _, _peer) = setup();
    let world = s.sessions[&a].world;
    let saved = character(&s.db, A).unwrap().unwrap();
    let mut waiting = command(&mut s, a, movement(WILLOW_X, 19.07298));
    let (due, boundary, receipt) = (
        pending(&s, a).due,
        pending(&s, a).boundary,
        pending(&s, a).received,
    );
    s.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_move BEFORE UPDATE ON build100_characters BEGIN SELECT RAISE(FAIL,'MOVE commit failure'); END;").unwrap();
    tokio::time::sleep_until(due.into()).await;
    assert!(
        s.release_weapons(Instant::now())
            .unwrap_err()
            .is::<StorageFailure>()
    );
    assert!(waiting.try_recv().is_err());
    assert_eq!(
        (
            pending(&s, a).due,
            pending(&s, a).boundary,
            pending(&s, a).received
        ),
        (due, boundary, receipt)
    );
    assert_eq!(s.worlds[&world].character(avatar), &saved);
    assert_eq!(character(&s.db, A).unwrap().unwrap(), saved);
    assert!(drain(&mut out).is_empty());
    s.db.lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_move")
        .unwrap();
    s.release_weapons(Instant::now()).unwrap();
    assert!(waiting.try_recv().unwrap().is_ok());
    assert_eq!(s.worlds[&world].character(avatar).y, 19.07298);
    assert_eq!(
        character(&s.db, A).unwrap().unwrap(),
        *s.worlds[&world].character(avatar)
    );
    assert!(s.deferred_weapons.is_empty());
    drain(&mut out);
    s.release_weapons(Instant::now()).unwrap();
    assert!(drain(&mut out).is_empty());
}

#[tokio::test]
async fn move_pacing_late_funded_wake_spends_at_actual_time_and_no_idle_bank() {
    let (mut s, a, avatar, mut out, _, _peer) = setup();
    let world = s.sessions[&a].world;
    let mut waiting = command(&mut s, a, movement(WILLOW_X, 19.07298));
    tokio::time::sleep_until((pending(&s, a).boundary + Duration::from_millis(200)).into()).await;
    let actual = Instant::now();
    s.release_weapons(actual).unwrap();
    assert!(waiting.try_recv().unwrap().is_ok());
    assert_eq!(s.worlds[&world].character(avatar).y, 19.07298);
    drain(&mut out);
    let mut next = command(&mut s, a, movement(WILLOW_X, 19.97298));
    assert!(
        next.try_recv().is_err(),
        "nominal wake must not leave an idle bank after actual spend"
    );
    assert_eq!(s.worlds[&world].character(avatar).y, 19.07298);
    wake(&mut s, a).await;
    assert!(next.try_recv().unwrap().is_ok());
}

#[tokio::test]
async fn move_pacing_stat_change_unfunded_late_wake_does_not_roll_boundary() {
    let (mut s, a, avatar, mut out, _, _peer) = setup();
    let world = s.sessions[&a].world;
    s.worlds
        .get_mut(&world)
        .unwrap()
        .character_mut(avatar)
        .stats
        .insert(22, 75);
    let saved = s.worlds[&world].character(avatar).clone();
    let mut waiting = command(&mut s, a, movement(saved.x, saved.y + 0.99));
    let boundary = pending(&s, a).boundary;
    s.worlds
        .get_mut(&world)
        .unwrap()
        .character_mut(avatar)
        .stats
        .insert(22, 0);
    tokio::time::sleep_until((boundary + Duration::from_millis(10)).into()).await;
    s.release_weapons(Instant::now()).unwrap();
    assert_eq!(
        waiting.try_recv().unwrap().unwrap_err(),
        "movement exceeds elapsed speed allowance"
    );
    assert!(s.deferred_weapons.is_empty());
    assert_eq!(s.worlds[&world].character(avatar).y, saved.y);
    assert!(drain(&mut out).is_empty());
}

#[tokio::test]
async fn move_pacing_speed_change_retry_keeps_first_boundary_and_receipt() {
    let (mut s, a, avatar, mut out, _, _peer) = setup();
    let world = s.sessions[&a].world;
    let saved = s.worlds[&world].character(avatar).clone();
    let mut waiting = command(&mut s, a, movement(saved.x, saved.y + 0.9));
    let (first_due, boundary, receipt) = (
        pending(&s, a).due,
        pending(&s, a).boundary,
        pending(&s, a).received,
    );
    s.worlds
        .get_mut(&world)
        .unwrap()
        .character_mut(avatar)
        .stats
        .insert(22, 0);
    tokio::time::sleep_until(first_due.into()).await;
    s.release_weapons(Instant::now()).unwrap();
    assert!(waiting.try_recv().is_err());
    assert_eq!(
        (pending(&s, a).boundary, pending(&s, a).received),
        (boundary, receipt)
    );
    assert!(pending(&s, a).due > first_due && pending(&s, a).due <= boundary);
    assert_eq!(s.worlds[&world].character(avatar).y, saved.y);
    assert!(drain(&mut out).is_empty());
    wake(&mut s, a).await;
    assert!(waiting.try_recv().unwrap().is_ok());
}

#[test]
fn move_pacing_owner_generation_detach_reply_idle_and_death_cancel_without_catchup() {
    for mutation in 0..9 {
        let (mut s, a, avatar, mut out, b, _peer) = setup();
        let world = s.sessions[&a].world;
        let saved = character(&s.db, A).unwrap().unwrap();
        let mut response = command(&mut s, a, movement(saved.x, saved.y + 0.9));
        match mutation {
            0 => s.detach(a, false).unwrap(),
            1 => {
                crate::world::progression_runtime_tests::lethal_enemy_bullet(
                    s.worlds.get_mut(&world).unwrap(),
                    avatar,
                );
                s.tick(Duration::ZERO).unwrap();
                assert!(!character(&s.db, A).unwrap().unwrap().alive);
            }
            2 => s.sessions.get_mut(&a).unwrap().generation = Some(2),
            3 => s.sessions.get_mut(&a).unwrap().world = 99,
            4 => {
                s.owners.insert(A.into(), b);
            }
            5 => {
                s.worlds
                    .get_mut(&world)
                    .unwrap()
                    .character_mut(avatar)
                    .generation = 2
            }
            6 => {
                response.close();
            }
            7 => {
                out.close();
                s.tick(Duration::ZERO).unwrap();
            }
            _ => s.tick(INPUT_TIMEOUT).unwrap(),
        }
        s.cancel_lost_weapons();
        if mutation == 6 {
            assert_eq!(
                response.try_recv(),
                Err(oneshot::error::TryRecvError::Closed)
            );
        } else {
            assert!(response.try_recv().unwrap().is_err());
        }
        assert!(s.deferred_weapons.is_empty());
        s.release_weapons(Instant::now()).unwrap();
        if mutation != 1 {
            assert_eq!(character(&s.db, A).unwrap().unwrap(), saved);
            if let Some(room) = s.worlds.get(&world)
                && let Some(p) = room.players.get(&avatar)
            {
                assert_eq!(p.character.y, saved.y);
                assert_eq!(p.character.resources_carry, saved.resources_carry);
            }
        } else {
            let dead = character(&s.db, A).unwrap().unwrap();
            s.detach(a, false).unwrap();
            let (out, mut output) = mpsc::channel(OUTPUT_CAPACITY);
            let fresh = s
                .join(
                    protocol::Hello {
                        guid: A.into(),
                        game_id: 0,
                    },
                    out,
                )
                .unwrap();
            s.input(
                fresh,
                packet(3, |w| {
                    w.i16(0x30e).utf("Replacement");
                }),
                Instant::now(),
            )
            .unwrap();
            assert_eq!(
                character(&s.db, A).unwrap().unwrap().generation,
                dead.generation + 1
            );
            assert_ne!(s.sessions[&fresh].avatar, Some(avatar));
            drain(&mut output);
            s.release_weapons(Instant::now()).unwrap();
            assert!(drain(&mut output).is_empty());
        }
    }
}

fn fire(s: &Simulation, world: i32, avatar: i32, id: u8) -> Packet {
    let p = s.worlds[&world].character(avatar);
    let kind = p.stats[&8] as i16;
    let item = s.content.get(kind).unwrap();
    let visual = s
        .content
        .named(&item.normal_projectile(0).unwrap().visual)
        .unwrap()
        .id as u8;
    packet(8, |w| {
        w.u8(id)
            .i32(avatar)
            .u8(visual)
            .i16(kind)
            .f32(p.x + 0.3)
            .f32(p.y)
            .f32(0.)
            .i16(0);
    })
}
#[tokio::test]
async fn move_pacing_shared_weapon_slot_and_cancel_preserve_incomplete_paid_burst() {
    let (mut s, a, avatar, mut out, _, _peer) = setup();
    let world = s.sessions[&a].world;
    let p = s.worlds.get_mut(&world).unwrap().character_mut(avatar);
    p.stats.extend([(8, 0xa98), (28, 17)]);
    let first = fire(&s, world, avatar, 21);
    assert!(command(&mut s, a, first).try_recv().unwrap().is_ok());
    assert!(s.worlds[&world].weapon_deadline(avatar).is_none());
    let p = s.worlds[&world].character(avatar).clone();
    let mut waiting = command(&mut s, a, movement(p.x, p.y + 0.9));
    let original = (
        pending(&s, a).due,
        pending(&s, a).boundary,
        pending(&s, a).received,
    );
    let second = fire(&s, world, avatar, 22);
    assert!(
        command(&mut s, a, second)
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("backpressure")
    );
    assert_eq!(
        (
            pending(&s, a).due,
            pending(&s, a).boundary,
            pending(&s, a).received
        ),
        original
    );
    waiting.close();
    s.cancel_lost_weapons();
    assert_eq!(
        waiting.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    );
    let second = fire(&s, world, avatar, 22);
    assert!(command(&mut s, a, second).try_recv().unwrap().is_ok());
    let packet = fire(&s, world, avatar, 23);
    assert!(command(&mut s, a, packet).try_recv().unwrap().is_ok());
    let packet = fire(&s, world, avatar, 24);
    let mut weapon = command(&mut s, a, packet);
    assert!(weapon.try_recv().is_err());
    assert!(s.deferred_weapons[&a].movement.is_none());
    assert!(
        command(&mut s, a, movement(p.x, p.y))
            .try_recv()
            .unwrap()
            .unwrap_err()
            .contains("backpressure")
    );
    tokio::time::sleep_until(s.weapon_deadline().unwrap().into()).await;
    s.release_weapons(Instant::now()).unwrap();
    assert!(weapon.try_recv().unwrap().is_ok());
    assert_eq!(s.worlds[&world].character(avatar).y, p.y);
    assert!(s.deferred_weapons.is_empty());
    drain(&mut out);
}
