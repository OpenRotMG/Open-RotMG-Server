use super::*;

fn notifications(inbox: &mut mpsc::Receiver<Packet>) -> Vec<(i32, String, u32)> {
    std::iter::from_fn(|| inbox.try_recv().ok())
        .filter(|packet| packet.id == wire::NOTIFICATION)
        .map(|packet| {
            let mut reader = wire::Reader::new(&packet.body);
            (
                reader.i32().unwrap(),
                reader.utf().unwrap(),
                reader.i32().unwrap() as u32,
            )
        })
        .collect()
}

#[test]
fn class_quest_notification_follows_each_first_committed_threshold_once() {
    for (before, after, best, expected) in [
        (18, 19, 0, false),
        (19, 20, 0, true),
        (149, 150, 20, true),
        (399, 400, 150, true),
        (799, 800, 400, true),
        (1999, 2000, 800, true),
        (19, 20, 20, false),
        (149, 150, 400, false),
        (2000, 2001, 2000, false),
        (19, 801, 0, true),
    ] {
        let (mut world, player) = movement_tests::fixture(0x18, false);
        let p = world.players.get_mut(&player).unwrap();
        p.character.exp = before * 1000;
        p.character.fame = before;
        p.presentation.best_fame.insert(p.character.class, best);
        let origin = p.pos;
        let template = p.character.clone();
        let mut inboxes = Vec::new();
        for (index, distance) in [0., 24., 25., 10.].into_iter().enumerate() {
            let id = if index == 0 {
                player
            } else {
                let mut ch = template.clone();
                ch.id += index as i32;
                ch.account += index as i32;
                world.enter(ch, None).unwrap()
            };
            let (packets, inbox) = mpsc::channel(64);
            let (lost, _) = watch::channel(None);
            let p = world.players.get_mut(&id).unwrap();
            p.tx = Some(Output { packets, lost });
            if index < 3 {
                p.known.insert(player);
            } else {
                p.known.remove(&player);
            }
            p.pos = Pos {
                x: origin.x + distance,
                y: origin.y,
            };
            inboxes.push(inbox);
        }
        world
            .players
            .get_mut(&player)
            .unwrap()
            .queue_experience((after - before) * 1000, None, false)
            .unwrap();
        let (_, snapshot) = world
            .pending_progress_where(|id, _| id == player)
            .pop()
            .unwrap();
        assert!(
            inboxes
                .iter_mut()
                .all(|inbox| notifications(inbox).is_empty())
        );
        world.committed(player, Ok(())).unwrap();
        world.finish_progress(player, snapshot.clone()).unwrap();
        for (index, inbox) in inboxes.iter_mut().enumerate() {
            let want = if expected && index < 2 {
                vec![(player, "Class Quest Complete!".into(), 0xff00ff00)]
            } else {
                vec![]
            };
            assert_eq!(
                notifications(inbox),
                want,
                "{before}->{after}, best={best}, observer={index}"
            );
        }
        assert!(world.finish_progress(player, snapshot).is_err());
        assert!(
            inboxes
                .iter_mut()
                .all(|inbox| notifications(inbox).is_empty())
        );
    }
}

#[test]
fn class_quest_failed_progress_does_not_notify_or_advance_record() {
    let (mut world, player) = movement_tests::fixture(0x18, false);
    let p = world.players.get_mut(&player).unwrap();
    p.character.exp = 19999;
    p.character.fame = 19;
    let (packets, mut inbox) = mpsc::channel(64);
    let (lost, _) = watch::channel(None);
    p.tx = Some(Output { packets, lost });
    p.queue_experience(1, None, false).unwrap();
    world
        .pending_progress_where(|id, _| id == player)
        .pop()
        .unwrap();
    assert!(
        world
            .committed::<()>(player, Err(anyhow::anyhow!("injected progress failure")))
            .is_err()
    );
    assert!(notifications(&mut inbox).is_empty());
    let p = &world.players[&player];
    assert_eq!(p.character.fame, 19);
    assert_eq!(
        p.presentation
            .next_quest(p.character.class, p.character.fame),
        20
    );
}
