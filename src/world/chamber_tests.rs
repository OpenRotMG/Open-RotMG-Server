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

#[test]
fn chamber_unlock_rejects_rolls_back_and_publishes_once_to_everyone() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
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
                .enter(ch, Some(Output { packets, lost }))
                .unwrap(),
        );
        receivers.push(inbox);
    }
    let [failed, owner, other] = players.try_into().unwrap();
    let snapshot = d.worlds[&world].players[&owner].character.clone();
    assert!(
        use_incantation(&mut d, world, owner, INCANTATION)
            .err()
            .unwrap()
            .to_string()
            .contains("defeated Oryx")
    );
    let boss = d.worlds[&world]
        .entities
        .values()
        .find(|e| e.kind == ORYX)
        .unwrap()
        .id;
    let w = d.worlds.get_mut(&world).unwrap();
    w.entities.get_mut(&boss).unwrap().hp = 0;
    w.defeat_entity(boss).unwrap();
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
    let plan = use_incantation(&mut d, world, failed, INCANTATION).unwrap();
    let opened = plan.unlock.unwrap().opened;
    let plan = d.stage_unlock(plan).unwrap();
    let destination = d.portals[&opened].destination.unwrap();
    assert!(!d.worlds[&world].entities[&opened].published);
    assert!(d.destination(other, Some(opened)).is_err());
    assert!(
        use_incantation(&mut d, world, other, INCANTATION)
            .err()
            .unwrap()
            .to_string()
            .contains("reserves")
    );
    assert!(receivers.iter_mut().all(|inbox| inbox.try_recv().is_err()));
    assert!(
        d.finish_inventory(plan, Err(anyhow::anyhow!("injected commit failure")))
            .is_err()
    );
    assert!(!d.worlds.contains_key(&destination));
    assert!(!d.portals.contains_key(&opened));
    assert!(!d.worlds[&world].entities[&locked].reserved);
    assert!(!d.worlds[&world].entities.contains_key(&opened));
    assert_eq!(
        d.worlds[&world].players[&failed].character.items[4],
        i32::from(INCANTATION)
    );
    assert!(
        d.worlds[&world].players[&failed]
            .persistence_failure
            .is_some()
    );
    for inbox in &mut receivers {
        for packet in std::iter::from_fn(|| inbox.try_recv().ok()) {
            assert_ne!(packet.id, wire::TEXT);
            assert_ne!(packet.id, wire::NOTIFICATION);
        }
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
    for inbox in receivers.iter_mut().skip(1) {
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
