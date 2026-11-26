use super::*;
use crate::endgame::{CHAMBER, INCANTATION, LOCKED_CELLAR, ORYX};

fn use_incantation(d: &mut Directory, world: i32, player: i32, kind: u16) -> Result<InventoryPlan> {
    d.worlds
        .get_mut(&world)
        .unwrap()
        .use_item(
            player,
            Slot {
                object: player,
                slot: 4,
                kind: kind.into(),
            },
            Pos { x: 0., y: 0. },
        )?
        .context("expected inventory transaction")
}

fn chamber() -> (Directory, i32, Vec<mpsc::Receiver<Packet>>, [i32; 3]) {
    let content = Arc::new(crate::world::test_content());
    let mut d = Directory::test_with_seed(std::path::Path::new("data"), content, 16, 867).unwrap();
    d.clock.set_paused(true);
    let world = d.instance(CHAMBER, None, None).unwrap();
    let mut receivers = vec![];
    let mut players = vec![];
    for account in 1..=3 {
        let mut ch = Character::fresh(d.content.get(0x323).unwrap()).unwrap();
        ch.account = account;
        ch.id = account;
        ch.name = format!("Witness{account}");
        ch.items[4] = INCANTATION.into();
        let (packets, inbox) = mpsc::channel(128);
        let (lost, _) = watch::channel(None);
        players.push(
            d.worlds
                .get_mut(&world)
                .unwrap()
                .enter(
                    ch,
                    Some(Output {
                        trace: None,
                        packets,
                        lost,
                    }),
                )
                .unwrap(),
        );
        receivers.push(inbox);
    }
    (d, world, receivers, players.try_into().unwrap())
}
/// Defeats Oryx through the journal and gathers every player at the locked cellar.
fn defeat_oryx(d: &mut Directory, world: i32) -> (i32, Pos) {
    let boss = d.worlds[&world]
        .entities
        .values()
        .find(|e| e.kind == ORYX)
        .unwrap()
        .id;
    let w = d.worlds.get_mut(&world).unwrap();
    w.entities.get_mut(&boss).unwrap().hp = 0;
    w.defeat_entity(boss).unwrap();
    for _ in 0..8 {
        crate::world::xml::tests::drain_events(w).unwrap();
    }
    let locked = w
        .entities
        .values()
        .find(|e| e.kind == LOCKED_CELLAR)
        .unwrap()
        .id;
    let pos = w.entities[&locked].pos;
    for p in w.players.values_mut() {
        p.pos = pos;
    }
    (locked, pos)
}

#[test]
fn chamber_unlock_rejects_and_publishes_once_to_everyone() {
    let (mut d, world, mut receivers, [_, owner, other]) = chamber();
    let snapshot = d.worlds[&world].players[&owner].character.clone();
    assert!(
        use_incantation(&mut d, world, owner, INCANTATION)
            .err()
            .unwrap()
            .to_string()
            .contains("defeated Oryx")
    );
    let (locked, pos) = defeat_oryx(&mut d, world);
    let w = d.worlds.get_mut(&world).unwrap();
    w.players.get_mut(&owner).unwrap().pos.x += 3.01;
    assert!(
        use_incantation(&mut d, world, owner, INCANTATION)
            .err()
            .unwrap()
            .to_string()
            .contains("nearby")
    );
    assert!(
        use_incantation(&mut d, world, owner, 0xa22)
            .err()
            .unwrap()
            .to_string()
            .contains("ownership")
    );
    d.worlds
        .get_mut(&world)
        .unwrap()
        .players
        .get_mut(&owner)
        .unwrap()
        .pos = pos;
    assert_eq!(
        d.worlds[&world].players[&owner].character.items,
        snapshot.items
    );
    for inbox in &mut receivers {
        while inbox.try_recv().is_ok() {}
    }
    let plan = use_incantation(&mut d, world, owner, INCANTATION).unwrap();
    let opened = plan.unlock.unwrap().opened;
    let revision = plan.revision + 1;
    let plan = d.stage_unlock(plan).unwrap();
    let replay = plan.clone();
    assert!(receivers.iter_mut().all(|inbox| inbox.try_recv().is_err()));
    d.finish_inventory(plan, Ok(revision)).unwrap();
    assert!(d.finish_inventory(replay, Ok(revision)).is_err());
    let w = &d.worlds[&world];
    assert_eq!(w.players[&owner].character.items[4], -1);
    assert_eq!(w.players[&other].character.items[4], i32::from(INCANTATION));
    assert!(!w.entities.contains_key(&locked));
    assert!(w.entities[&opened].published && !w.entities[&opened].reserved);
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x700).count(), 1);
    let destination = d.portals[&opened].destination.unwrap();
    for player in [owner, other] {
        assert_eq!(d.destination(player, Some(opened)).unwrap().2, destination);
    }
    assert_eq!(d.worlds[&destination].name, "Wine Cellar");
    let text = chat::text_packet("", -1, 0, "Wine Cellar unlocked by Witness2");
    let mut note = Writer::new();
    note.i32(owner).utf("Unlocked by Witness2").u32(0xff00ff00);
    for inbox in &mut receivers {
        let packets: Vec<_> = std::iter::from_fn(|| inbox.try_recv().ok()).collect();
        assert_eq!(
            packets
                .iter()
                .filter(|p| p.id == text.id && p.body == text.body)
                .count(),
            1,
            "one unlock announcement per participant, only after commit"
        );
        assert_eq!(
            packets
                .iter()
                .filter(|p| p.id == wire::NOTIFICATION && p.body == note.0)
                .count(),
            1
        );
    }
    assert!(use_incantation(&mut d, world, other, INCANTATION).is_err());
    let realm = -3;
    let mut ch = snapshot;
    ch.account = 4;
    ch.id = 4;
    let wrong = d.worlds.get_mut(&realm).unwrap().enter(ch, None).unwrap();
    assert!(
        use_incantation(&mut d, realm, wrong, INCANTATION)
            .err()
            .unwrap()
            .to_string()
            .contains("defeated Oryx")
    );
}
