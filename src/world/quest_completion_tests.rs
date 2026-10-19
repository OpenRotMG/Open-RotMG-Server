use super::*;

#[test]
fn defeated_swarm_completes_assigned_quests_once_before_retargeting() {
    let mut w = tests::arena(2);
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    let first = w
        .spawn_entity(0x955, Pos { x: 5.5, y: 1.5 }, vec![])
        .unwrap();
    let next = w
        .spawn_entity(0x955, Pos { x: 7.5, y: 1.5 }, vec![])
        .unwrap();
    let mut participants = Vec::new();
    for (index, position) in [
        Pos { x: 1.5, y: 1.5 },
        Pos { x: 1.5, y: 1.5 },
        Pos { x: 1.5, y: 81.5 },
        Pos { x: 11.5, y: 1.5 },
    ]
    .into_iter()
    .enumerate()
    {
        let mut character = tests::ch(&w);
        character.account = index as i32 + 1;
        character.id = index as i32 + 1;
        character.level = 5;
        let (packets, mut rx) = mpsc::channel(512);
        let (lost, _) = watch::channel(false);
        let id = w.enter(character, Some(Output { packets, lost })).unwrap();
        w.players.get_mut(&id).unwrap().pos = position;
        w.updates(id, true).unwrap();
        assert_eq!(
            w.players[&id].quest,
            Some(if index == 3 { next } else { first })
        );
        while rx.try_recv().is_ok() {}
        participants.push((id, rx));
    }
    let killer = participants[0].0;
    w.damage_enemy(killer, first, 1, true, &[], Some(1))
        .unwrap();
    for (_, rx) in &mut participants {
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).all(|p| p.id != wire::NOTIFICATION));
    }
    w.damage_enemy(killer, first, 10000, true, &[], Some(2))
        .unwrap();
    assert!(!w.entities.contains_key(&first));
    assert_eq!(w.players[&killer].character.quests, 1);
    assert_eq!(
        w.damage_enemy(killer, first, 10000, true, &[], Some(3))
            .unwrap(),
        0
    );
    for (index, (id, rx)) in participants.iter_mut().enumerate() {
        w.updates(*id, false).unwrap();
        let packets: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let notes: Vec<_> = packets
            .iter()
            .filter(|p| p.id == wire::NOTIFICATION)
            .collect();
        if index < 2 {
            let mut expected = Writer::new();
            expected.i32(*id).utf("Quest Complete!").u32(0xff00ff00);
            assert_eq!(
                notes.len(),
                1,
                "assigned nearby player lost native quest completion"
            );
            assert_eq!(notes[0].body, expected.0);
            assert!(
                packets
                    .iter()
                    .position(|p| p.id == wire::NOTIFICATION)
                    .unwrap()
                    < packets.iter().position(|p| p.id == 34).unwrap()
            );
            assert_eq!(w.players[id].quest, Some(next));
            assert!(w.players[id].experience_queued > 0);
        } else {
            assert!(
                notes.is_empty(),
                "remote or differently assigned player received completion"
            );
        }
    }
    assert_eq!(w.players[&participants[1].0].character.quests, 0);
    let experience = w.players[&killer].experience_queued;
    w.remove_entity(next);
    for (id, rx) in &mut participants {
        w.updates(*id, false).unwrap();
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).all(|p| p.id != wire::NOTIFICATION));
    }
    assert_eq!(w.players[&killer].experience_queued, experience);
    assert_eq!(w.players[&killer].character.quests, 1);
}
