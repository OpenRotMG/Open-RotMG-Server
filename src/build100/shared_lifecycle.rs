use super::*;
use crate::{database, wire::Reader, world};
use tokio::task::JoinHandle;

// Authored structural fixtures exercise the real serialized owner, not native capture bytes.
const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
struct Client {
    session: u64,
    avatar: i32,
    output: mpsc::Receiver<Vec<Packet>>,
}
struct Harness {
    commands: mpsc::Sender<Command>,
    db: Database,
    task: JoinHandle<Result<()>>,
}
fn packet(id: u8, f: impl FnOnce(&mut Writer)) -> Packet {
    let mut w = Writer::new();
    f(&mut w);
    Packet { id, body: w.0 }
}
fn load() -> Packet {
    packet(25, |w| {
        w.i32(0).utf("Advisory");
    })
}
fn create() -> Packet {
    packet(3, |w| {
        w.i16(0x30e).utf("Fixture");
    })
}
fn fire(avatar: i32, id: u8) -> Packet {
    packet(8, |w| {
        w.u8(id)
            .i32(avatar)
            .u8(41)
            .i16(0xa97)
            .f32(16.8)
            .f32(16.5)
            .f32(0.)
            .i16(32000);
    })
}
fn drain(client: &mut Client) -> Vec<Packet> {
    let mut packets = vec![];
    while let Ok(batch) = client.output.try_recv() {
        packets.extend(batch);
    }
    packets
}
fn objects(packets: &[Packet], kind: i16) -> Vec<i32> {
    packets
        .iter()
        .filter(|p| p.id == 10)
        .filter_map(|p| {
            let mut r = Reader::new(&p.body);
            let got = r.i16().unwrap();
            r.utf().unwrap();
            let id = r.i32().unwrap();
            (got == kind).then_some(id)
        })
        .collect()
}
fn serial(packets: &[Packet]) -> i32 {
    protocol::pong(
        &packets
            .iter()
            .find(|p| p.id == 22)
            .expect("native Ping22")
            .body,
    )
    .unwrap()
}
impl Harness {
    fn new() -> Self {
        Self::with_db(Arc::new(Mutex::new(
            database(std::path::Path::new(":memory:")).unwrap(),
        )))
    }
    fn with_db(db: Database) -> Self {
        let (commands, receiver) = mpsc::channel(64);
        let task =
            tokio::spawn(Simulation::new(world::tests::content(), db.clone()).run(receiver, false));
        Self { commands, db, task }
    }
    async fn join(&self, guid: &str, game_id: i32) -> Result<Client, String> {
        self.join_capacity(guid, game_id, OUTPUT_CAPACITY).await
    }
    async fn join_capacity(
        &self,
        guid: &str,
        game_id: i32,
        capacity: usize,
    ) -> Result<Client, String> {
        let (output, receive) = mpsc::channel(capacity);
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Join {
                hello: protocol::Hello {
                    guid: guid.into(),
                    game_id,
                },
                output,
                reply,
            })
            .await
            .unwrap();
        result.await.unwrap().map(|session| Client {
            session,
            avatar: 0,
            output: receive,
        })
    }
    async fn input(&self, session: u64, packet: Packet) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Input {
                received: Instant::now(),
                session,
                packet,
                reply,
            })
            .await
            .unwrap();
        result.await.unwrap()
    }
    async fn enter(&self, guid: &str) -> (Client, Vec<Packet>) {
        let mut client = self.join(guid, 0).await.unwrap();
        self.input(client.session, create()).await.unwrap();
        let packets = drain(&mut client);
        client.avatar = Reader::new(&packets.iter().find(|p| p.id == 1).unwrap().body)
            .i32()
            .unwrap();
        (client, packets)
    }
    async fn load(&self, client: &mut Client) -> Vec<Packet> {
        self.input(client.session, load()).await.unwrap();
        let packets = drain(client);
        client.avatar = Reader::new(&packets.iter().find(|p| p.id == 1).unwrap().body)
            .i32()
            .unwrap();
        packets
    }
    async fn tick(&self, seconds: f32) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Tick(Duration::from_secs_f32(seconds), reply))
            .await
            .unwrap();
        result.await.unwrap()
    }
    async fn inspect<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Simulation) -> T + Send + 'static,
    ) -> T {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Inspect(Box::new(move |s| {
                let _ = reply.send(f(s));
            })))
            .await
            .unwrap();
        result.await.unwrap()
    }
    async fn detach(&self, session: u64) {
        self.commands
            .send(Command::Detach { session })
            .await
            .unwrap();
        self.inspect(|_| ()).await;
    }
    async fn stop(self) {
        drop(self.commands);
        self.task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn shared_lifecycle_mutual_visibility_same_enemies_one_clock_and_disconnect() {
    let h = Harness::new();
    let (mut a, pa) = h.enter(A).await;
    let (mut b, pb) = h.enter(B).await;
    assert_ne!(a.avatar, b.avatar);
    assert_eq!(objects(&pa, 0x600), objects(&pb, 0x600));
    assert_eq!(objects(&pb, 0x30e), vec![a.avatar, b.avatar]);
    assert_eq!(objects(&drain(&mut a), 0x30e), vec![b.avatar]);
    h.tick(0.2).await.unwrap();
    let ta = drain(&mut a);
    let tb = drain(&mut b);
    assert_eq!(
        ta.iter().find(|p| p.id == 13).unwrap().body,
        tb.iter().find(|p| p.id == 13).unwrap().body
    );
    assert_eq!(h.inspect(|s| s.worlds[&1].elapsed).await, 0.2);
    h.detach(a.session).await;
    assert!(
        drain(&mut b)
            .iter()
            .any(|p| p.id == 11 && Reader::new(&p.body).i32().unwrap() == a.avatar)
    );
    h.tick(0.2).await.unwrap();
    assert_eq!(
        h.inspect(|s| (s.worlds[&1].elapsed, s.worlds[&1].players.len()))
            .await,
        (0.4, 1)
    );
    assert!(drain(&mut b).iter().any(|p| p.id == 13));
    h.stop().await;
}
#[tokio::test]
async fn shared_lifecycle_duplicate_guid_pending_and_loaded_are_exclusive() {
    let h = Harness::new();
    let mut a = h.join(A, -1).await.unwrap();
    assert!(
        h.join(A, 0)
            .await
            .unwrap_err_string()
            .contains("already connected")
    );
    h.input(a.session, create()).await.unwrap();
    drain(&mut a);
    assert!(h.join(A, -1).await.is_err());
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.owners.len(), s.worlds[&1].players.len()))
            .await,
        (1, 1, 1)
    );
    h.stop().await;
}
// Avoid a Debug requirement on the receiver-bearing successful join value.
trait JoinError {
    fn unwrap_err_string(self) -> String;
}
impl JoinError for Result<Client, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Ok(_) => panic!("join unexpectedly accepted"),
            Err(e) => e,
        }
    }
}

#[tokio::test]
async fn shared_lifecycle_distinct_owners_bullet_rollover_and_forged_ownership() {
    let h = Harness::new();
    let (mut a, _) = h.enter(A).await;
    let (mut b, _) = h.enter(B).await;
    drain(&mut a);
    for id in [255, 0] {
        h.input(a.session, fire(a.avatar, id)).await.unwrap();
        h.input(b.session, fire(b.avatar, id)).await.unwrap();
    }
    let bullets = h.inspect(|s| s.worlds[&1].fixture_bullets()).await;
    assert_eq!(bullets.len(), 4);
    assert!(bullets.contains(&(a.avatar, 255)) && bullets.contains(&(b.avatar, 255)));
    assert!(h.input(a.session, fire(a.avatar, 255)).await.is_err());
    assert!(h.input(a.session, fire(b.avatar, 1)).await.is_err());
    assert!(
        h.input(
            a.session,
            packet(15, |w| {
                w.i32(b.avatar).u8(4).i32(-1).f32(16.5).f32(16.5);
            })
        )
        .await
        .is_err()
    );
    assert!(
        h.input(
            a.session,
            packet(14, |w| {
                w.i32(b.avatar).u8(4).i16(0xa22).i32(a.avatar).u8(5).i16(-1);
            })
        )
        .await
        .is_err()
    );
    h.input(
        a.session,
        packet(9, |w| {
            w.i32(1_000_101).u8(0).u8(0).i16(32000).u8(0).i32(a.avatar);
        }),
    )
    .await
    .unwrap();
    assert_eq!(character(&h.db, A).unwrap().unwrap().stats[&6], 0);
    assert_eq!(drain(&mut a).iter().filter(|p| p.id == 8).count(), 2); // only the other avatar's shots
    assert_eq!(drain(&mut b).iter().filter(|p| p.id == 8).count(), 2);
    h.tick(1.05).await.unwrap();
    drain(&mut a);
    drain(&mut b);
    h.input(a.session, fire(a.avatar, 255)).await.unwrap();
    h.input(b.session, fire(b.avatar, 255)).await.unwrap();
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_contested_loot_one_bag_one_committed_pickup() {
    let h = Harness::new();
    let (mut a, _) = h.enter(A).await;
    let (mut b, _) = h.enter(B).await;
    drain(&mut a);
    h.inspect(|s| s.worlds.get_mut(&1).unwrap().fixture_collision(false))
        .await;
    h.tick(0.05).await.unwrap();
    let pa = drain(&mut a);
    let pb = drain(&mut b);
    let bags = objects(&pa, 0x500);
    assert_eq!(bags.len(), 1);
    assert_eq!(bags, objects(&pb, 0x500));
    assert_eq!(character(&h.db, A).unwrap().unwrap().stats[&6], 5);
    assert_eq!(character(&h.db, B).unwrap().unwrap().stats[&6], 5);
    let swap = |avatar| {
        packet(14, |w| {
            w.i32(bags[0]).u8(0).i16(0xa22).i32(avatar).u8(5).i16(-1);
        })
    };
    // Queue both before awaiting either result: actual actor command ordering chooses the winner.
    let (ra, aa) = oneshot::channel();
    let (rb, ab) = oneshot::channel();
    h.commands
        .send(Command::Input {
            received: Instant::now(),
            session: a.session,
            packet: swap(a.avatar),
            reply: ra,
        })
        .await
        .unwrap();
    h.commands
        .send(Command::Input {
            received: Instant::now(),
            session: b.session,
            packet: swap(b.avatar),
            reply: rb,
        })
        .await
        .unwrap();
    assert!(aa.await.unwrap().is_ok());
    assert!(ab.await.unwrap().unwrap_err().contains("stale inventory"));
    assert_eq!(character(&h.db, A).unwrap().unwrap().stats[&13], 0xa22);
    assert_eq!(character(&h.db, B).unwrap().unwrap().stats[&13], -1);
    h.tick(0.05).await.unwrap();
    assert_eq!(character(&h.db, A).unwrap().unwrap().stats[&6], 5);
    assert_eq!(character(&h.db, B).unwrap().unwrap().stats[&6], 5);
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_simultaneous_death_reward_commit_once_and_restart() {
    let path = std::env::temp_dir().join(format!(
        "build100-lifecycle-{}.sqlite",
        rand::random::<u64>()
    ));
    let db = Arc::new(Mutex::new(database(&path).unwrap()));
    let h = Harness::with_db(db.clone());
    let (mut a, _) = h.enter(A).await;
    let (mut b, _) = h.enter(B).await;
    drain(&mut a);
    h.inspect(|s| s.worlds.get_mut(&1).unwrap().fixture_collision(true))
        .await;
    h.tick(0.05).await.unwrap();
    let dead = character(&db, A).unwrap().unwrap();
    let rewarded = character(&db, B).unwrap().unwrap();
    assert!(!dead.alive);
    assert_eq!(dead.stats[&1], 0);
    assert_eq!(dead.stats[&6], 0);
    assert_eq!(rewarded.stats[&6], 5);
    let pa = drain(&mut a);
    let pb = drain(&mut b);
    assert_eq!(
        pa.iter()
            .filter(|p| p.id == 11 && Reader::new(&p.body).i32().unwrap() == a.avatar)
            .count(),
        1
    );
    assert_eq!(objects(&pb, 0x500).len(), 1);
    h.tick(0.05).await.unwrap();
    drain(&mut a);
    drain(&mut b);
    assert_eq!(character(&db, B).unwrap().unwrap().stats[&6], 5);
    h.stop().await;
    drop(db);
    let db = Arc::new(Mutex::new(database(&path).unwrap()));
    let h = Harness::with_db(db.clone());
    assert!(!character(&db, A).unwrap().unwrap().alive);
    assert_eq!(character(&db, B).unwrap().unwrap().stats[&6], 5);
    let mut a = h.join(A, 0).await.unwrap();
    drain(&mut a);
    assert!(
        h.input(a.session, load())
            .await
            .unwrap_err()
            .contains("permanently dead")
    );
    h.input(a.session, create()).await.unwrap();
    assert_eq!(character(&db, A).unwrap().unwrap().generation, 2);
    h.stop().await;
    drop(db);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn shared_lifecycle_injected_second_write_failure_rolls_back_all_before_output() {
    let h = Harness::new();
    let (mut a, _) = h.enter(A).await;
    let (mut b, _) = h.enter(B).await;
    drain(&mut a);
    let before_a = character(&h.db, A).unwrap().unwrap();
    let before_b = character(&h.db, B).unwrap().unwrap();
    h.db.lock().unwrap().execute_batch(&format!("CREATE TRIGGER fail_second BEFORE UPDATE ON build100_characters WHEN NEW.guid='{B}' BEGIN SELECT RAISE(ABORT,'injected second mutation failure'); END;")).unwrap();
    h.inspect(|s| s.worlds.get_mut(&1).unwrap().fixture_collision(true))
        .await;
    let error = h.tick(0.05).await.unwrap_err();
    assert!(error.contains("injected second mutation failure"));
    assert!(h.task.await.unwrap().is_err());
    assert_eq!(character(&h.db, A).unwrap().unwrap(), before_a);
    assert_eq!(character(&h.db, B).unwrap().unwrap(), before_b);
    assert!(drain(&mut a).is_empty());
    assert!(drain(&mut b).is_empty());
    assert!(a.output.is_closed() && b.output.is_closed());
}
#[tokio::test]
async fn shared_lifecycle_injected_create_failure_has_no_success_or_unsaved_avatar() {
    let h = Harness::new();
    let mut a = h.join(A, 0).await.unwrap();
    drain(&mut a);
    h.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON build100_characters BEGIN SELECT RAISE(ABORT,'injected insert failure'); END;").unwrap();
    assert!(
        h.input(a.session, create())
            .await
            .unwrap_err()
            .contains("injected insert failure")
    );
    assert!(h.task.await.unwrap().is_err());
    assert!(character(&h.db, A).unwrap().is_none());
    assert!(drain(&mut a).is_empty());
}

#[tokio::test]
async fn shared_lifecycle_native_portal_handoff_before_close_late_drop_and_return() {
    let h = Harness::new();
    let (mut old, initial) = h.enter(A).await;
    let portal = initial
        .iter()
        .find(|p| {
            p.id == 10 && {
                let mut r = Reader::new(&p.body);
                r.i16().unwrap() == 0x700
            }
        })
        .unwrap();
    let mut r = Reader::new(&portal.body);
    assert_eq!(r.i16().unwrap(), 0x700);
    r.utf().unwrap();
    assert_eq!(r.i32().unwrap(), 1_000_099);
    assert_eq!((r.f32().unwrap(), r.f32().unwrap()), (16.5, 18.));
    assert_eq!(
        (r.i16().unwrap(), r.u8().unwrap(), r.i32().unwrap()),
        (1, 23, 2)
    );
    r.done().unwrap();
    assert!(h.join(B, 2).await.is_err());
    assert!(h.join(A, 999).await.is_err());
    let saved = character(&h.db, A).unwrap().unwrap();
    let mut next = h.join(A, 2).await.unwrap();
    assert!(old.output.is_closed());
    drain(&mut old);
    assert!(h.join(A, 2).await.is_err()); // pending destination has consumed the lease
    h.detach(old.session).await;
    let entry = h.load(&mut next).await;
    assert_ne!(old.avatar, next.avatar);
    assert!(objects(&entry, 0x600).iter().all(|&id| id > 2_000_000));
    h.detach(old.session).await;
    assert!(
        h.input(old.session, fire(old.avatar, 0))
            .await
            .unwrap_err()
            .contains("stale session")
    );
    assert_eq!(
        h.inspect(|s| (
            s.sessions.len(),
            s.owners.len(),
            s.worlds[&1].players.len(),
            s.worlds[&2].players.len()
        ))
        .await,
        (1, 1, 0, 1)
    );
    assert_eq!(character(&h.db, A).unwrap().unwrap(), saved);
    let mut back = h.join(A, 1).await.unwrap();
    h.load(&mut back).await;
    h.detach(next.session).await;
    assert_eq!(
        h.inspect(|s| (s.worlds[&1].players.len(), s.worlds[&2].players.len()))
            .await,
        (1, 0)
    );
    assert!(h.join(A, 0).await.is_err());
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_detach_before_join_one_use_five_second_lease() {
    let h = Harness::new();
    let (a, _) = h.enter(A).await;
    h.detach(a.session).await;
    h.tick(4.999).await.unwrap();
    let mut next = h.join(A, 2).await.unwrap();
    h.load(&mut next).await;
    h.detach(a.session).await;
    assert!(h.join(A, 2).await.is_err());
    let (b, _) = h.enter(B).await;
    h.detach(b.session).await;
    h.tick(5.).await.unwrap();
    assert!(h.join(B, 2).await.is_err());
    let mut b = h.join(B, 0).await.unwrap();
    h.load(&mut b).await;
    assert_eq!(h.inspect(|s| s.worlds.len()).await, 2);
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_unreachable_dead_and_stale_generation_destination_rejected() {
    let h = Harness::new();
    let (mut a, _) = h.enter(A).await;
    h.input(
        a.session,
        packet(5, |w| {
            w.f32(16.5).f32(15.7);
        }),
    )
    .await
    .unwrap();
    drain(&mut a);
    assert!(h.join(A, 2).await.is_err());
    h.detach(a.session).await;
    assert!(h.join(A, 2).await.is_err());
    let mut a = h.join(A, 0).await.unwrap();
    h.load(&mut a).await;
    h.detach(a.session).await;
    let mut saved = character(&h.db, A).unwrap().unwrap();
    saved.generation += 1;
    h.db.lock()
        .unwrap()
        .execute(
            "UPDATE build100_characters SET state=? WHERE guid=?",
            [serde_json::to_string(&saved).unwrap(), A.into()],
        )
        .unwrap();
    assert!(
        h.join(A, 2)
            .await
            .unwrap_err_string()
            .contains("stale transfer generation")
    );
    saved.generation -= 1;
    saved.alive = false;
    h.db.lock()
        .unwrap()
        .execute(
            "UPDATE build100_characters SET state=? WHERE guid=?",
            [serde_json::to_string(&saved).unwrap(), A.into()],
        )
        .unwrap();
    assert!(h.join(A, 2).await.is_err());
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_slow_closed_consumers_do_not_freeze_or_leak_owners() {
    let h = Harness::new();
    let (mut slow, _) = h.enter(A).await;
    let (mut fast, _) = h.enter(B).await;
    drain(&mut slow);
    for _ in 0..12 {
        h.tick(0.2).await.unwrap();
        assert!(drain(&mut fast).iter().any(|p| p.id == 13));
    }
    assert!(slow.output.is_closed());
    assert_eq!(
        h.inspect(|s| (s.owners.len(), s.worlds[&1].players.len()))
            .await,
        (1, 1)
    );
    let mut new = h.join(A, 0).await.unwrap();
    h.load(&mut new).await;
    let new_session = new.session;
    drop(new.output);
    h.tick(0.05).await.unwrap();
    assert!(
        !h.inspect(move |s| s.sessions.contains_key(&new_session))
            .await
    );
    h.detach(slow.session).await;
    assert_eq!(h.inspect(|s| s.owners.len()).await, 1);
    h.stop().await;
}
#[tokio::test]
async fn shared_lifecycle_initial_output_overflow_never_publishes_ghost_arrival() {
    let h = Harness::new();
    let (mut observer, _) = h.enter(B).await;
    let mut full = h.join_capacity(A, 0, 1).await.unwrap(); // Map fills its entire output queue.
    h.input(full.session, create()).await.unwrap();
    assert!(full.output.is_closed());
    assert!(objects(&drain(&mut observer), 0x30e).is_empty());
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.worlds[&1].players.len()))
            .await,
        (1, 1)
    );
    assert!(character(&h.db, A).unwrap().unwrap().alive);
    drain(&mut full);
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_native_ping_pong_tokens_false_stale_and_inactive_cleanup() {
    let h = Harness::new();
    let (mut a, _) = h.enter(A).await;
    let (mut b, _) = h.enter(B).await;
    drain(&mut a);
    assert!(
        h.input(
            a.session,
            packet(23, |w| {
                w.i32(1);
            })
        )
        .await
        .is_err()
    );
    h.tick(5.).await.unwrap();
    let sa = serial(&drain(&mut a));
    let sb = serial(&drain(&mut b));
    assert_ne!(sa, sb);
    assert!(
        h.input(
            a.session,
            packet(23, |w| {
                w.i32(sb);
            })
        )
        .await
        .is_err()
    );
    assert!(
        h.input(
            a.session,
            Packet {
                id: 23,
                body: vec![0; 5]
            }
        )
        .await
        .is_err()
    );
    h.input(
        a.session,
        packet(23, |w| {
            w.i32(sa);
        }),
    )
    .await
    .unwrap();
    assert!(
        h.input(
            a.session,
            packet(23, |w| {
                w.i32(sa);
            })
        )
        .await
        .is_err()
    );
    h.tick(5.).await.unwrap();
    let fresh = serial(&drain(&mut a));
    drain(&mut b);
    assert!(
        h.input(
            a.session,
            packet(23, |w| {
                w.i32(sa);
            })
        )
        .await
        .is_err()
    );
    h.input(
        a.session,
        packet(23, |w| {
            w.i32(fresh);
        }),
    )
    .await
    .unwrap();
    h.tick(5.).await.unwrap();
    drain(&mut b);
    let fresh = serial(&drain(&mut a));
    h.input(
        a.session,
        packet(23, |w| {
            w.i32(fresh);
        }),
    )
    .await
    .unwrap();
    assert!(b.output.is_closed());
    assert_eq!(h.inspect(|s| s.owners.len()).await, 1);
    let new = h.join(B, 0).await.unwrap();
    assert!(
        h.input(
            b.session,
            packet(23, |w| {
                w.i32(sb);
            })
        )
        .await
        .is_err()
    );
    h.detach(new.session).await;
    // Genuine Pong does not extend the independent last-native-input bound.
    for _ in 0..8 {
        h.tick(5.).await.unwrap();
        let fresh = serial(&drain(&mut a));
        h.input(
            a.session,
            packet(23, |w| {
                w.i32(fresh);
            }),
        )
        .await
        .unwrap();
    }
    h.tick(5.).await.unwrap();
    assert!(a.output.is_closed());
    assert_eq!(h.inspect(|s| s.owners.len()).await, 0);
    assert!(h.join(A, 0).await.is_ok());
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_population_is_bounded_and_unknown_game_never_creates_world() {
    let h = Harness::new();
    let mut clients = vec![];
    for id in 0..MAX_SESSIONS {
        clients.push(h.join(&format!("{id:040x}"), 0).await.unwrap());
    }
    assert!(h.join(A, 0).await.unwrap_err_string().contains("capacity"));
    assert!(h.join(A, 900).await.is_err());
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.worlds.len())).await,
        (MAX_SESSIONS, 2)
    );
    h.detach(clients[0].session).await;
    assert!(h.join(A, 0).await.is_ok());
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_first_attach_after_idle_publishes_initial_population() {
    let h = Harness::new();
    h.tick(0.25).await.unwrap();
    let (_, first) = h.enter(A).await;
    let (_, second) = h.enter(B).await;
    assert_eq!(objects(&first, 0x600).len(), 3);
    assert_eq!(objects(&first, 0x600), objects(&second, 0x600));
    assert_eq!(h.inspect(|s| s.worlds[&1].elapsed).await, 0.25);
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_native_handoff_replaces_owner_at_session_capacity() {
    let h = Harness::new();
    let (old, _) = h.enter(A).await;
    let mut pending = vec![];
    for id in 0..MAX_SESSIONS - 1 {
        pending.push(h.join(&format!("{id:040x}"), 0).await.unwrap());
    }
    let mut next = h.join(A, 2).await.unwrap();
    h.load(&mut next).await;
    h.detach(old.session).await;
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.owners.len(), s.worlds[&2].players.len()))
            .await,
        (MAX_SESSIONS, MAX_SESSIONS, 1)
    );
    assert!(old.output.is_closed());
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_cascade_closed_receivers_cleanup_keeps_actor_alive() {
    let h = Harness::new();
    let (a, _) = h.enter(A).await;
    let (b, _) = h.enter(B).await;
    drop(a.output);
    drop(b.output);
    h.tick(0.05).await.unwrap();
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.owners.len(), s.worlds[&1].players.len()))
            .await,
        (0, 0, 0)
    );
    assert!(!h.task.is_finished());
    let mut rejoined = h.join(A, 0).await.unwrap();
    h.load(&mut rejoined).await;
    h.tick(0.05).await.unwrap();
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_cascade_arrival_finishes_before_removal_publication() {
    let h = Harness::new();
    let mut slow = h.join_capacity(A, 0, 1).await.unwrap();
    drain(&mut slow);
    h.input(slow.session, create()).await.unwrap();
    drain(&mut slow);
    let (mut observer, _) = h.enter(B).await; // Arrival fills slow's queue.
    let mut newcomer = h
        .join_capacity("cccccccccccccccccccccccccccccccccccccccc", 0, 1)
        .await
        .unwrap();
    drain(&mut newcomer); // Its own initialization must fit before arrival fan-out.
    h.input(newcomer.session, create()).await.unwrap();
    let initial = drain(&mut newcomer);
    let avatar = Reader::new(&initial.iter().find(|p| p.id == 1).unwrap().body)
        .i32()
        .unwrap();
    let events: Vec<_> = drain(&mut observer)
        .iter()
        .filter_map(|p| {
            let mut r = Reader::new(&p.body);
            match p.id {
                10 => {
                    r.i16().unwrap();
                    r.utf().unwrap();
                    (r.i32().unwrap() == avatar).then_some(10)
                }
                11 => (r.i32().unwrap() == avatar).then_some(11),
                _ => None,
            }
        })
        .collect();
    assert_eq!(
        events,
        vec![10, 11],
        "arrival must precede its only removal"
    );
    assert!(slow.output.is_closed() && newcomer.output.is_closed());
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.owners.len(), s.worlds[&1].players.len()))
            .await,
        (1, 1, 1)
    );
    assert!(character(&h.db, A).unwrap().unwrap().alive);
    assert!(
        character(&h.db, "cccccccccccccccccccccccccccccccccccccccc")
            .unwrap()
            .unwrap()
            .alive
    );
    h.tick(0.2).await.unwrap();
    assert!(drain(&mut observer).iter().any(|p| p.id == 13));
    assert!(!h.task.is_finished());
    h.stop().await;
}

#[tokio::test]
async fn shared_lifecycle_cascade_ping_overflow_keeps_snapshot_and_actor_alive() {
    let h = Harness::new();
    let mut first = h.join_capacity(A, 0, 1).await.unwrap();
    drain(&mut first);
    h.input(first.session, create()).await.unwrap();
    drain(&mut first);
    let mut second = h.join_capacity(B, 0, 1).await.unwrap();
    drain(&mut second);
    h.input(second.session, create()).await.unwrap(); // Both loaded output queues are full.
    h.tick(5.).await.unwrap();
    assert!(first.output.is_closed() && second.output.is_closed());
    assert_eq!(
        h.inspect(|s| (s.sessions.len(), s.owners.len(), s.worlds[&1].players.len()))
            .await,
        (0, 0, 0)
    );
    assert!(!h.task.is_finished());
    let mut rejoined = h.join(B, 0).await.unwrap();
    h.load(&mut rejoined).await;
    h.tick(0.05).await.unwrap();
    h.stop().await;
}

#[path = "realm_entry_tests.rs"]
mod realm_entry_tests;
