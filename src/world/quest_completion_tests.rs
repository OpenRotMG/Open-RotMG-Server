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

#[test]
fn all_death_fame_phase3_qualified_quest() {
    let mut w = tests::arena(2);
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    let e = w
        .spawn_entity(0x955, Pos { x: 5.5, y: 1.5 }, vec![])
        .unwrap();
    let mut players = Vec::new();
    for n in 1..=3 {
        let mut ch = tests::ch(&w);
        ch.id = n;
        ch.account = n;
        ch.level = 5;
        let id = w.enter(ch, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = Pos { x: 1.5, y: 1.5 };
        w.updates(id, true).unwrap();
        assert_eq!(w.players[&id].quest, Some(e));
        players.push(id);
    }
    w.damage_enemy(players[0], e, 1, true, &[], None).unwrap();
    w.damage_enemy(players[1], e, 10000, true, &[], None)
        .unwrap();
    assert_eq!(
        w.players[&players[0]].character.lifetime.qualified_quests,
        1
    );
    assert_eq!(
        w.players[&players[1]].character.lifetime.qualified_quests,
        1
    );
    assert_eq!(
        w.players[&players[2]].character.lifetime.qualified_quests,
        0
    );
    assert!(w.players[&players[2]].experience_queued > 0);
}

#[test]
fn all_death_fame_phase3_quest_boundaries_and_transformations() {
    let mut w = tests::arena(2);
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    let pos = Pos { x: 5.5, y: 1.5 };
    let e = w.spawn_entity(0x955, pos, vec![]).unwrap();
    let mut players = vec![];
    for n in 1..=8 {
        let mut ch = tests::ch(&w);
        ch.id = n;
        ch.account = n;
        ch.level = 5;
        let id = w.enter(ch, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = pos;
        w.updates(id, true).unwrap();
        if n != 2 {
            w.damage_enemy(id, e, 1, true, &[], None).unwrap();
        }
        match n {
            3 => w.players.get_mut(&id).unwrap().quest = None,
            4 => w.players.get_mut(&id).unwrap().death_pending = true,
            5 => w.players.get_mut(&id).unwrap().character.hp = 0,
            6 => w.players.get_mut(&id).unwrap().pos = Pos { x: 30.5, y: 1.5 },
            7 => w.players.get_mut(&id).unwrap().pos = Pos { x: 30.499, y: 1.5 },
            _ => {}
        }
        players.push(id);
    }
    w.damage_enemy(players[0], e, 10000, true, &[], None)
        .unwrap();
    for (n, id) in players.iter().enumerate() {
        assert_eq!(
            w.players[id].character.lifetime.qualified_quests,
            u64::from(matches!(n, 0 | 6 | 7)),
            "participant {n}"
        );
    }
    assert_eq!(
        w.damage_enemy(players[0], e, 10000, true, &[], None)
            .unwrap(),
        0
    );
    for id in &players {
        assert!(w.players[id].character.lifetime.qualified_quests <= 1);
    }
    let id = players[0];
    let root = w.spawn_entity(0x675, pos, vec![]).unwrap();
    w.entities.get_mut(&root).unwrap().quest = Some(root);
    w.players.get_mut(&id).unwrap().quest = Some(root);
    w.players.get_mut(&id).unwrap().qualified_quest = Some(root);
    let before = w.players[&id].character.lifetime.kills;
    w.damage_enemy(id, root, 100000, true, &[], None).unwrap();
    assert_eq!(
        w.players[&id].character.lifetime.kills, before,
        "transformation is not a kill"
    );
    let egg = w
        .entities
        .values()
        .find(|e| e.kind == 0x6c2 && e.quest == Some(root))
        .unwrap()
        .id;
    w.quest_mechanics(egg).unwrap();
    w.now += 2000;
    w.quest_mechanics(egg).unwrap();
    let hp = w.entities[&egg].hp;
    w.damage_enemy(id, egg, hp, true, &[], None).unwrap();
    let reborn = w
        .entities
        .values()
        .find(|e| e.kind == 0x6c1 && e.quest == Some(root))
        .unwrap()
        .id;
    w.damage_enemy(id, reborn, 100000, true, &[], None).unwrap();
    assert_eq!(w.players[&id].character.lifetime.kills, before + 1);
    assert_eq!(
        w.players[&id].character.lifetime.qualified_quests, 2,
        "assigned encounter survives its transformations"
    );
}

#[test]
fn all_death_fame_phase3_telemetry_never_recomputes_progression() {
    let mut w = tests::world();
    // Deliberately prepared, noncanonical XP/level/base-Fame values must survive a telemetry-only save verbatim.
    let mut ch = tests::ch(&w);
    ch.exp = 2000;
    ch.level = 1;
    ch.fame = 7777;
    ch.xp_boost = crate::store::XpBoost {
        active: false,
        bonus_exp: 1000,
    };
    let id = w.enter(ch, None).unwrap();
    let before = w.players[&id].character.clone();
    let mut rng = w.rng.clone();
    let snapshot = w.pending_progress().pop().unwrap().1;
    assert_eq!(snapshot, before);
    assert_eq!(
        rand::RngCore::next_u64(&mut w.rng),
        rand::RngCore::next_u64(&mut rng)
    );
    w.finish_progress(id, snapshot).unwrap();
    assert!(w.pending_progress().is_empty());
    assert_eq!(w.players[&id].character, before);
}
