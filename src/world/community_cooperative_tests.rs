use super::*;
use crate::content::{PRIEST, WIZARD};
use std::time::Duration;

pub(super) fn receive(
    d: &mut Directory,
    world: i32,
    account: i32,
    class: u16,
) -> (i32, mpsc::Receiver<Packet>) {
    let w = d.worlds.get_mut(&world).unwrap();
    let mut c = super::tests::ch(w);
    c.id = account + 100;
    c.account = account;
    c.class = class;
    c.base = w.content.classes[&class].base;
    c.items = w.content.objects[&class].equipment.clone();
    let (packets, mut rx) = mpsc::channel(128);
    let (lost, _) = watch::channel(false);
    let id = w.enter(c, Some(Output { packets, lost })).unwrap();
    while rx.try_recv().is_ok() {}
    (id, rx)
}
fn setup() -> (
    Directory,
    i32,
    i32,
    mpsc::Receiver<Packet>,
    mpsc::Receiver<Packet>,
) {
    let mut d = super::tests::directory();
    let (a, mut ar) = receive(&mut d, -2, 1, WIZARD);
    let (b, mut br) = receive(&mut d, -2, 2, PRIEST);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.now = 600;
    w.updates(a, false).unwrap();
    w.updates(b, false).unwrap();
    while ar.try_recv().is_ok() {}
    while br.try_recv().is_ok() {}
    (d, a, b, ar, br)
}
#[test]
fn community_crowd_named_teleport_preserves_native_guards_and_shared_chat_throttle() {
    let (mut d, a, b, _ar, _br) = setup();
    let destination = d.worlds[&-2].players[&b].pos;
    let start = d.worlds[&-2].players[&a].pos;
    let now = Instant::now();
    for (mode, expected) in [
        "Cannot teleport while paused.",
        "Cannot teleport to a paused player.",
        "Teleport target is busy, dead, departing or quarantined.",
        "Teleport target is busy, dead, departing or quarantined.",
        "Teleport target is busy, dead, departing or quarantined.",
        "Teleport target has not been observed.",
        "Teleport target's position is blocked.",
        "Cannot teleport in this world.",
        "Position correction awaits GOTO acknowledgement.",
        "Busy, dead, departing or quarantined players cannot teleport.",
    ]
    .into_iter()
    .enumerate()
    {
        let w = d.worlds.get_mut(&-2).unwrap();
        w.name = if mode == 7 { "Vault" } else { "Nexus" }.into();
        let p = w.players.get_mut(&a).unwrap();
        p.paused = mode == 0;
        p.pending = mode == 9;
        if mode == 5 {
            p.known.remove(&b);
        } else {
            p.known.insert(b);
        }
        if mode == 8 {
            reconciliation::goto(p, true, vec![]);
        }
        let q = w.players.get_mut(&b).unwrap();
        q.character.name = "Benny".into();
        q.paused = mode == 1;
        q.pending = mode == 2;
        q.death_pending = mode == 3;
        q.persistence_failure = (mode == 4).then(|| "uncertain commit".into());
        q.pos = if mode == 6 {
            Pos { x: -1., y: -1. }
        } else {
            destination
        };
        assert_eq!(
            d.chat(
                a,
                "/teleport Benny".into(),
                now + Duration::from_secs(mode as u64)
            )
            .unwrap_err()
            .to_string(),
            expected
        );
        assert!(d.teleports.is_empty());
        assert_eq!(d.worlds[&-2].players[&a].pos.distance(start), 0.);
        if mode == 8 {
            d.worlds
                .get_mut(&-2)
                .unwrap()
                .acknowledge_goto(a, 0)
                .unwrap();
        }
    }
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&a)
        .unwrap()
        .pending = false;
    let next = now + Duration::from_secs(10);
    for _ in 0..3 {
        assert_eq!(
            d.chat(a, "/teleport Missing".into(), next)
                .unwrap_err()
                .to_string(),
            "Player not found in this world."
        );
    }
    assert!(
        d.chat(a, "/who".into(), next)
            .unwrap_err()
            .to_string()
            .contains("Chat rate limit")
    );
    assert!(d.teleports.is_empty());
}
#[test]
fn community_crowd_teleport_rejects_wrong_world_unseen_self_blocked_and_lifecycle_targets_without_charging()
 {
    let (mut d, a, b, _ar, _br) = setup();
    let now = Instant::now();
    let (other, _rx) = receive(&mut d, -3, 3, WIZARD);
    assert!(d.teleport(a, a, now).is_err());
    assert!(d.teleport(a, other, now).is_err());
    assert!(d.teleport(a, -999, now).is_err());
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&a)
        .unwrap()
        .known
        .remove(&b);
    assert!(d.teleport(a, b, now).is_err());
    d.worlds.get_mut(&-2).unwrap().updates(a, false).unwrap();
    for mode in 0..6 {
        let p = d.worlds.get_mut(&-2).unwrap().players.get_mut(&b).unwrap();
        p.pending = mode == 0;
        p.death_pending = mode == 1;
        p.departing = mode == 2;
        p.persistence_failure = (mode == 3).then(|| "uncertain commit".into());
        p.character.alive = mode != 4;
        p.character.hp = if mode == 5 { 0 } else { 100 };
        assert!(d.teleport(a, b, now).is_err());
        assert!(d.teleports.is_empty());
    }
    let w = d.worlds.get_mut(&-2).unwrap();
    let p = w.players.get_mut(&b).unwrap();
    p.character.hp = 100;
    let previous = p.pos;
    p.pos = Pos { x: -1., y: -1. };
    assert!(d.teleport(a, b, now).is_err());
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&b)
        .unwrap()
        .pos = previous;
    d.teleport(a, b, now).unwrap();
    let (a, _ar) = receive(&mut d, -1, 4, WIZARD);
    let (b, _br) = receive(&mut d, -1, 5, WIZARD);
    assert!(d.teleport(a, b, now).is_err());
}
