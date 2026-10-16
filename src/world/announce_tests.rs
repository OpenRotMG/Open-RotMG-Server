use super::*;
use crate::content::WIZARD;
use crate::wire::Reader;

use super::tests::{directory, receive};

fn announcements(rx: &mut mpsc::Receiver<Packet>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|p| p.id == wire::TEXT)
        .map(|p| {
            let mut r = Reader::new(&p.body);
            let name = r.utf().unwrap().to_owned();
            assert_eq!(r.i32().unwrap(), -1);
            assert_eq!(r.i32().unwrap(), 0);
            assert_eq!(r.u8().unwrap(), 0);
            assert_eq!(r.utf().unwrap(), "");
            let text = r.utf().unwrap().to_owned();
            assert_eq!(r.utf().unwrap(), "");
            r.done().unwrap();
            format!("{name} {text}")
        })
        .collect()
}

#[test]
fn announcement_reaches_ready_players_in_every_world_once() {
    let mut d = directory();
    let (_, mut nexus) = receive(&mut d, -2, 1, WIZARD);
    let (_, mut realm) = receive(&mut d, -3, 2, WIZARD);
    assert_eq!(
        d.announce("Server restarting in 20 seconds to apply an update."),
        2
    );
    for rx in [&mut nexus, &mut realm] {
        assert_eq!(
            announcements(rx),
            ["@ANNOUNCEMENT Server restarting in 20 seconds to apply an update."]
        );
    }
}

#[test]
fn announcement_skips_departing_players_and_players_with_uncertain_saves() {
    let mut d = directory();
    let (departing, mut gone) = receive(&mut d, -2, 1, WIZARD);
    let (failed, mut broken) = receive(&mut d, -3, 2, WIZARD);
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&departing)
        .unwrap()
        .departing = true;
    d.worlds
        .get_mut(&-3)
        .unwrap()
        .players
        .get_mut(&failed)
        .unwrap()
        .persistence_failure = Some("uncertain commit".into());
    assert_eq!(d.announce("Restarting now"), 0);
    assert!(announcements(&mut gone).is_empty());
    assert!(announcements(&mut broken).is_empty());
}
