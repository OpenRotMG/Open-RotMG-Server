use super::tests::{ch, world};
use super::*;

#[test]
fn shared_tick_encodings_match_each_recipients_bytes_and_refresh_each_phase() {
    let mut w = world();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    let mut viewers = Vec::new();
    for account in 1..=2 {
        let mut character = ch(&w);
        character.account = account;
        character.id = account;
        let (packets, rx) = wire::outbox::channel(4096);
        let (lost, _) = watch::channel(false);
        let id = w.enter(character, Some(Output { packets, lost })).unwrap();
        w.players.get_mut(&id).unwrap().presentation.credits = account * 101;
        viewers.push((id, rx));
    }
    let pos = w.players[&viewers[0].0].pos;
    let enemy = w.spawn_entity(0x804, pos, vec![]).unwrap();
    for _ in 0..3 {
        w.entities.get_mut(&enemy).unwrap().hp += 1;
        w.players
            .get_mut(&viewers[0].0)
            .unwrap()
            .presentation
            .credits += 3;
        w.step();
        w.step();
        for (id, rx) in &mut viewers {
            let mut ticks = Vec::new();
            while let Ok(packet) = rx.try_recv() {
                if packet.id == wire::NEW_TICK {
                    ticks.push(packet);
                }
            }
            assert_eq!(ticks.len(), 1);
            let packet = ticks.pop().unwrap();
            let p = &w.players[id];
            assert_eq!(p.known.len(), 3);
            let mut expected = Writer::new();
            expected.0.extend_from_slice(&packet.body[..8]);
            expected.i16(3);
            for entity in &p.known {
                expected.0.extend(w.stats(*entity, *id).unwrap());
            }
            assert_eq!(packet.body, expected.0, "recipient {id}");
        }
    }
}

#[test]
fn incremental_first_seen_matches_full_retain_and_entry_updates() {
    let mut w = world();
    let id = w.enter(ch(&w), None).unwrap();
    let mut reference = w.players[&id].known_since.clone();
    let mut seed = 39417_u64;
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let pos = Pos {
            x: (seed as u32 % w.map.width as u32) as f32 + 0.5,
            y: ((seed >> 32) as u32 % w.map.height as u32) as f32 + 0.5,
        };
        w.players.get_mut(&id).unwrap().pos = pos;
        w.now += 50;
        let mut visible = w.visible(id);
        visible.extend(w.players.keys().copied());
        reference.retain(|entity, _| visible.contains(entity));
        for entity in &visible {
            reference.entry(*entity).or_insert(w.now);
        }
        w.updates(id, false).unwrap();
        assert_eq!(w.players[&id].known_since, reference);
    }
}
