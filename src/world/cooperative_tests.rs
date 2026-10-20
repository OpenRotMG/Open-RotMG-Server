use super::*;
use crate::content::{PRIEST, WIZARD};

use super::tests::receive;
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
fn all_death_fame_phase3_teleport_facts() {
    let (mut d, a, b, _ar, _br) = setup();
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&a)
        .unwrap()
        .paused = true;
    assert!(d.teleport(a, b, Instant::now()).is_err());
    assert!(!d.worlds[&-2].players[&a].character.lifetime.teleported);
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&a)
        .unwrap()
        .paused = false;
    d.teleport(a, b, Instant::now()).unwrap();
    assert!(d.worlds[&-2].players[&a].character.lifetime.teleported);
    assert!(!d.worlds[&-2].players[&b].character.lifetime.teleported);
    assert!(!d.worlds[&-2].players[&a].character.lifetime.ability_used);
}
