use super::*;
use crate::{
    database,
    wire::Reader,
    world::{shore_runtime_tests as shore, spell_runtime_tests as spell},
};
const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn packet(id: u8, f: impl FnOnce(&mut Writer)) -> Packet {
    let mut w = Writer::new();
    f(&mut w);
    Packet { id, body: w.0 }
}
type OwnerCase = (
    Simulation,
    u64,
    i32,
    mpsc::Receiver<Vec<Packet>>,
    u64,
    mpsc::Receiver<Vec<Packet>>,
);
fn setup() -> OwnerCase {
    let (c, g) = shore::load(true);
    let mut s = Simulation::normal(
        c,
        g,
        Arc::new(Mutex::new(
            database(std::path::Path::new(":memory:")).unwrap(),
        )),
        100,
    )
    .unwrap();
    let mut enter = |guid: &str| {
        let (out, receive) = mpsc::channel(OUTPUT_CAPACITY);
        let id = s
            .join(
                protocol::Hello {
                    guid: guid.into(),
                    game_id: 0,
                },
                out,
            )
            .unwrap();
        s.input(
            id,
            packet(3, |w| {
                w.i16(0x30e).utf("Paced");
            }),
            Instant::now(),
        )
        .unwrap();
        (id, s.sessions[&id].avatar.unwrap(), receive)
    };
    let (a, avatar, mut out) = enter(A);
    let (b, _, mut peer) = enter(B);
    let before = s.worlds[&0].clone();
    s.worlds
        .get_mut(&0)
        .unwrap()
        .character_mut(avatar)
        .stats
        .extend([(28, 17), (8, 0xa98)]);
    s.commit(&s.worlds[&0], &before).unwrap();
    drain(&mut out);
    drain(&mut peer);
    (s, a, avatar, out, b, peer)
}
fn fire(s: &Simulation, avatar: i32, id: u8) -> Packet {
    let p = s.worlds[&0].character(avatar);
    let kind = p.stats[&8] as i16;
    let visual = s
        .content
        .named(
            &s.content
                .get(kind)
                .unwrap()
                .normal_projectile(0)
                .unwrap()
                .visual,
        )
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
fn drain(out: &mut mpsc::Receiver<Vec<Packet>>) -> Vec<Packet> {
    let mut all = vec![];
    while let Ok(p) = out.try_recv() {
        all.extend(p);
    }
    all
}
fn ids(events: &[Packet], owner: i32) -> Vec<u8> {
    events
        .iter()
        .filter(|p| p.id == 8)
        .filter_map(|p| {
            let mut r = Reader::new(&p.body);
            let id = r.u8().unwrap();
            (r.i32().unwrap() == owner).then_some(id)
        })
        .collect()
}
fn command(
    s: &mut Simulation,
    session: u64,
    p: Packet,
    received: Instant,
    now: Instant,
) -> oneshot::Receiver<Result<(), String>> {
    let (reply, receive) = oneshot::channel();
    let result = s.input_at(session, p, received, now);
    s.reply_input(session, result, reply).unwrap();
    receive
}
fn waiting(
    s: &mut Simulation,
    a: u64,
    avatar: i32,
    base: Instant,
) -> oneshot::Receiver<Result<(), String>> {
    for id in [21, 22] {
        let p = fire(s, avatar, id);
        let time = base + Duration::from_nanos(if id == 21 { 0 } else { 77_104 });
        assert!(command(s, a, p, time, time).try_recv().unwrap().is_ok());
    }
    let p = fire(s, avatar, 23);
    assert!(
        command(
            s,
            a,
            p,
            base + Duration::from_nanos(335_496_014),
            base + Duration::from_nanos(335_496_014)
        )
        .try_recv()
        .unwrap()
        .is_ok()
    );
    let p = fire(s, avatar, 24);
    command(
        s,
        a,
        p,
        base + Duration::from_nanos(335_937_358),
        base + Duration::from_nanos(335_937_358),
    )
}

#[test]
fn cadence_pacing_owner_measured_pair_sql_begin_failure_exact_retry() {
    {
        let (mut s, a, avatar, mut out, _, mut peer) = setup();
        let base = Instant::now();
        let mut response = waiting(&mut s, a, avatar, base);
        drain(&mut out);
        drain(&mut peer);
        let before = shore::snapshot(&s.worlds[&0]);
        let saved = character(&s.db, A).unwrap().unwrap();
        let due = base + Duration::from_nanos(336_077_104);
        s.release_weapons(due - Duration::from_nanos(1)).unwrap();
        assert!(response.try_recv().is_err());
        s.db.lock()
            .unwrap()
            .execute_batch("BEGIN IMMEDIATE")
            .unwrap();
        {
            assert!(s.release_weapons(due).unwrap_err().is::<StorageFailure>());
            assert_eq!(shore::snapshot(&s.worlds[&0]), before);
            assert_eq!(s.worlds[&0].weapon_deadline(avatar), Some(due));
            assert_eq!(character(&s.db, A).unwrap().unwrap(), saved);
            assert!(response.try_recv().is_err());
            assert!(drain(&mut out).is_empty() && drain(&mut peer).is_empty());
            s.db.lock().unwrap().execute_batch("ROLLBACK").unwrap();
        }
        s.release_weapons(due).unwrap();
        assert!(response.try_recv().unwrap().is_ok());
        assert_eq!(ids(&drain(&mut peer), avatar), [23, 24]);
        assert!(ids(&drain(&mut out), avatar).is_empty());
        s.release_weapons(due).unwrap();
        assert!(drain(&mut peer).is_empty());
        assert!(spell::server_cursors(&s.worlds[&0]).is_empty());
    }
}

#[test]
fn cadence_pacing_owner_cancellation_and_late_wake_no_idle_credit() {
    for mutation in 0..7 {
        let (mut s, a, avatar, mut out, _, mut peer) = setup();
        let base = Instant::now();
        let mut response = waiting(&mut s, a, avatar, base);
        drain(&mut out);
        drain(&mut peer);
        let rng = spell::snapshot(&s.worlds[&0]);
        match mutation {
            0 => s.detach(a, false).unwrap(),
            1 => {
                s.worlds.get_mut(&0).unwrap().character_mut(avatar).alive = false;
                s.cancel_lost_weapons();
            }
            2 => {
                s.sessions.get_mut(&a).unwrap().generation = Some(2);
                s.cancel_lost_weapons();
            }
            3 => {
                s.worlds
                    .get_mut(&0)
                    .unwrap()
                    .character_mut(avatar)
                    .stats
                    .insert(11, 0xa29);
                s.release_weapons(base + Duration::from_secs(1)).unwrap();
            }
            4 => {
                s.worlds
                    .get_mut(&0)
                    .unwrap()
                    .character_mut(avatar)
                    .stats
                    .insert(25, 1 << 20);
                s.release_weapons(base + Duration::from_secs(1)).unwrap();
            }
            6 => {
                s.sessions.get_mut(&a).unwrap().world = 99;
                s.cancel_lost_weapons();
            }
            _ => {
                s.content.objects.get_mut(&0xa98).unwrap().rate_of_fire = None;
                s.release_weapons(base + Duration::from_secs(1)).unwrap();
            }
        }
        assert!(response.try_recv().unwrap().is_err());
        assert!(ids(&drain(&mut peer), avatar).is_empty());
        if mutation != 0 {
            assert_eq!(spell::snapshot(&s.worlds[&0]), rng);
            assert!(s.worlds[&0].weapon_deadline(avatar).is_none());
        }
    }
    let (mut s, a, avatar, mut out, _, mut peer) = setup();
    let base = Instant::now();
    let mut response = waiting(&mut s, a, avatar, base);
    drain(&mut out);
    drain(&mut peer);
    let late = base + Duration::from_secs(10);
    s.release_weapons(late).unwrap();
    assert!(response.try_recv().unwrap().is_ok());
    for id in [25, 26] {
        let p = fire(&s, avatar, id);
        let mut next = command(&mut s, a, p, late, late);
        if id == 25 {
            assert!(next.try_recv().unwrap().is_ok());
        } else {
            assert!(next.try_recv().is_err());
        }
    }
    assert_eq!(s.weapon_deadline(), Some(late + Duration::from_millis(336)));
}

#[test]
fn cadence_pacing_owner_actual_hazard_death_replacement_and_closed_reply_dispose_pending() {
    let (mut s, a, avatar, mut out, _, mut peer) = setup();
    let base = Instant::now();
    let mut response = waiting(&mut s, a, avatar, base);
    drain(&mut out);
    drain(&mut peer);
    let map = s.worlds[&0].map.as_ref().unwrap();
    let width = usize::from(map.dimensions().0);
    let lava = map.tiles().iter().position(|t| t.ground == 0x70).unwrap();
    let before = s.worlds[&0].clone();
    let p = s.worlds.get_mut(&0).unwrap().character_mut(avatar);
    p.x = (lava % width) as f32 + 0.5;
    p.y = (lava / width) as f32 + 0.5;
    p.stats.insert(1, 1);
    s.commit(&s.worlds[&0], &before).unwrap();
    s.tick(Duration::from_millis(500)).unwrap();
    assert!(response.try_recv().unwrap().is_err());
    assert!(!character(&s.db, A).unwrap().unwrap().alive);
    assert!(s.worlds[&0].weapon_deadline(avatar).is_none());
    assert!(ids(&drain(&mut peer), avatar).is_empty());
    s.detach(a, false).unwrap();
    let (output, mut fresh_out) = mpsc::channel(OUTPUT_CAPACITY);
    let fresh = s
        .join(
            protocol::Hello {
                guid: A.into(),
                game_id: 0,
            },
            output,
        )
        .unwrap();
    s.input(
        fresh,
        packet(3, |w| {
            w.i16(0x30e).utf("NewPaced");
        }),
        Instant::now(),
    )
    .unwrap();
    let replacement = s.sessions[&fresh].avatar.unwrap();
    assert_ne!(replacement, avatar);
    let saved = character(&s.db, A).unwrap().unwrap();
    assert_eq!(saved.generation, 2);
    assert_eq!(saved.stats[&8], 0xa97);
    assert!(s.worlds[&0].weapon_deadline(replacement).is_none());
    drain(&mut fresh_out);
    let (mut s, a, avatar, _out, _, _peer) = setup();
    let response = waiting(&mut s, a, avatar, Instant::now());
    drop(response);
    s.cancel_lost_weapons();
    assert!(s.deferred_weapons.is_empty());
    assert!(s.worlds[&0].weapon_deadline(avatar).is_none());
}

#[tokio::test]
async fn cadence_pacing_same_completing_frame_multiple_groups_keep_original_receipt() {
    let (mut s, a, avatar, mut out, _, mut peer) = setup();
    let received = Instant::now();
    for id in [21, 22] {
        let p = fire(&s, avatar, id);
        s.input(a, p, received).unwrap();
    }
    drain(&mut out);
    drain(&mut peer);
    let mut encoder = crate::wire::Codec::server_with_keys(b"hPvJ(R}Mp$-cg", b"zCV2tsY0]s;|]");
    let mut bytes = vec![];
    for id in 23..=28 {
        let p = fire(&s, avatar, id);
        bytes.extend(encoder.frame(p.id, p.body));
    }
    let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(
        axum::extract::ws::Message::Binary(bytes.into()),
    )]);
    let mut input = crate::input::receive(stream);
    let frame = input.recv().await.unwrap().unwrap();
    let receipt = frame.received;
    let axum::extract::ws::Message::Binary(ref bytes) = frame.message else {
        panic!("binary")
    };
    let decoded = crate::protocol::server_codec().feed(bytes).unwrap();
    assert_eq!(decoded.len(), 6);
    let (commands, rx) = mpsc::channel(64);
    let task = tokio::spawn(s.run(rx, false));
    for p in decoded {
        let (reply, receive) = oneshot::channel();
        commands
            .send(Command::Input {
                session: a,
                packet: p,
                received: receipt,
                reply,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), receive)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        ids(&drain(&mut peer), avatar),
        (23..=28).collect::<Vec<_>>()
    );
    drop(frame);
    assert!(input.recv().await.unwrap().is_none());
    drop(commands);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn cadence_pacing_real_owner_deadline_backpressure_and_other_session_responsive() {
    let (mut s, a, avatar, mut out, b, mut peer) = setup();
    let received = Instant::now();
    for id in [21, 22] {
        let p = fire(&s, avatar, id);
        s.input(a, p, received).unwrap();
    }
    drain(&mut out);
    drain(&mut peer);
    let first = fire(&s, avatar, 23);
    let last = fire(&s, avatar, 24);
    let other = s.worlds[&0].character(s.sessions[&b].avatar.unwrap());
    let movement = packet(5, |w| {
        w.f32(other.x).f32(other.y);
    });
    let (commands, rx) = mpsc::channel(64);
    let task = tokio::spawn(s.run(rx, false));
    let (r, wait) = oneshot::channel();
    commands
        .send(Command::Input {
            session: a,
            packet: first,
            received,
            reply: r,
        })
        .await
        .unwrap();
    wait.await.unwrap().unwrap();
    let (r, mut waiting) = oneshot::channel();
    commands
        .send(Command::Input {
            session: a,
            packet: last,
            received,
            reply: r,
        })
        .await
        .unwrap();
    let (done, ready) = oneshot::channel();
    commands
        .send(Command::Inspect(Box::new(move |s| {
            assert_eq!(s.deferred_weapons.len(), 1);
            assert!(s.sessions.contains_key(&b));
            assert_eq!(s.worlds[&0].elapsed, 0.);
            done.send(()).unwrap();
        })))
        .await
        .unwrap();
    ready.await.unwrap();
    let (reply, receive) = oneshot::channel();
    commands
        .send(Command::Input {
            session: b,
            packet: movement,
            received: Instant::now(),
            reply,
        })
        .await
        .unwrap();
    receive.await.unwrap().unwrap();
    assert!(waiting.try_recv().is_err());
    assert!(peer.try_recv().is_err());
    tokio::time::timeout(Duration::from_secs(2), &mut waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(ids(&drain(&mut peer), avatar), [23, 24]);
    let (done, ready) = oneshot::channel();
    commands
        .send(Command::Inspect(Box::new(move |s| {
            assert_eq!(s.worlds[&0].elapsed, 0.);
            assert!(s.deferred_weapons.is_empty());
            done.send(()).unwrap();
        })))
        .await
        .unwrap();
    ready.await.unwrap();
    drop(commands);
    task.await.unwrap().unwrap();
}
