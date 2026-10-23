use super::*;
use crate::dungeon::trench;

fn fixture() -> (World, i32) {
    let mut w = tests::arena(6);
    w.name = "Ocean Trench".into();
    let id = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&id).unwrap().pos = Pos { x: 10.5, y: 10.5 };
    (w, id)
}

#[test]
fn oxygen_cadence_depletion_damage_and_ineligible_players() {
    let (mut w, id) = fixture();
    assert_eq!(w.players[&id].oxygen, 100);
    let hp = w.players[&id].character.hp;
    w.now = 50;
    w.ocean_step();
    assert_eq!(w.players[&id].oxygen, 100);
    for event in 1..=50 {
        w.now = event * 150;
        w.ocean_step();
        assert_eq!(w.players[&id].oxygen, 100 - (event * 2) as i32);
        assert_eq!(w.players[&id].character.hp, hp);
    }
    assert_eq!(w.players[&id].oxygen, 0);
    assert_eq!(w.players[&id].character.hp, hp);
    w.now += 150;
    w.ocean_step();
    assert_eq!(w.players[&id].character.hp, hp - 10);
    w.players.get_mut(&id).unwrap().oxygen = 1;
    w.now += 150;
    w.ocean_step();
    assert_eq!(w.players[&id].oxygen, 0);
    assert_eq!(w.players[&id].character.hp, hp - 10);
    for flag in 0..5 {
        let (mut skipped, id) = fixture();
        let p = skipped.players.get_mut(&id).unwrap();
        match flag {
            0 => p.paused = true,
            1 => p.departing = true,
            2 => p.death_pending = true,
            3 => p.death_started = true,
            _ => p.character.alive = false,
        }
        skipped.now = 150;
        skipped.ocean_step();
        assert_eq!(skipped.players[&id].oxygen, 100);
    }
    w.players.get_mut(&id).unwrap().character.hp = 5;
    w.now += 150;
    w.ocean_step();
    let p = &w.players[&id];
    assert_eq!(p.character.hp, 0);
    assert!(p.death_pending);
    assert_eq!(p.killer, "suffocation");
    let mut outside = tests::arena(6);
    let id = outside.enter(tests::ch(&outside), None).unwrap();
    outside.now = 150;
    outside.ocean_step();
    assert_eq!(outside.players[&id].oxygen, -1);
}

#[test]
fn oxygen_vents_have_strict_radius_refill_cap_and_thessal_death_pocket() {
    for (dx, dy, expected) in [
        (0.99, 0., 8),
        (0.5, 0.5, 8),
        (1., 0., 0),
        (1.01, 0., 0),
        (0.75, 0.75, 0),
    ] {
        let (mut w, id) = fixture();
        let origin = w.players[&id].pos;
        w.spawn_entity(trench::VENT, origin, vec![]).unwrap();
        let p = w.players.get_mut(&id).unwrap();
        p.pos.x += dx;
        p.pos.y += dy;
        p.oxygen = 0;
        let hp = p.character.hp;
        w.now = 150;
        w.ocean_step();
        assert_eq!(w.players[&id].oxygen, expected);
        assert_eq!(
            w.players[&id].character.hp,
            hp - if expected == 0 { 10 } else { 0 }
        );
    }
    let (mut w, id) = fixture();
    let pos = w.players[&id].pos;
    let boss = w.spawn_entity(trench::THESSAL, pos, vec![]).unwrap();
    w.players.get_mut(&id).unwrap().oxygen = 0;
    assert!(!w.entities.values().any(|e| e.kind == trench::VENT));
    w.defeat_entity(boss).unwrap();
    assert!(!w.entities.contains_key(&boss));
    let vents: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == trench::VENT)
        .collect();
    assert_eq!(vents.len(), 1);
    assert_eq!((vents[0].pos.x, vents[0].pos.y), (pos.x, pos.y));
    let hp = w.players[&id].character.hp;
    for event in 1..=13 {
        w.now = event * 150;
        w.ocean_step();
    }
    assert_eq!(w.players[&id].oxygen, 100);
    assert_eq!(w.players[&id].character.hp, hp);
}

fn numeric_stats(bytes: &[u8]) -> BTreeMap<u8, i32> {
    let mut reader = wire::Reader::new(bytes);
    reader.i32().unwrap();
    reader.f32().unwrap();
    reader.f32().unwrap();
    let count = reader.i16().unwrap();
    let mut values = BTreeMap::new();
    for _ in 0..count {
        let key = reader.u8().unwrap();
        if matches!(key, 31 | 62) {
            reader.utf().unwrap();
        } else {
            assert!(values.insert(key, reader.i32().unwrap()).is_none());
        }
    }
    reader.done().unwrap();
    values
}

#[test]
fn oxygen_diagnostic_and_changed_self_stat_agree_without_advancing_world() {
    let (mut w, id) = fixture();
    let mut cache = tick_stats::Cache::default();
    assert_eq!(
        numeric_stats(&cache.encode(id, w.stats(id, id).unwrap()))[&64],
        100
    );
    assert!(!numeric_stats(&cache.encode(id, w.stats(id, id).unwrap())).contains_key(&64));
    assert!(!numeric_stats(&w.stats(id, id + 1).unwrap()).contains_key(&64));
    w.now = 150;
    w.ocean_step();
    let observed = w.observe(id).unwrap();
    assert_eq!(observed, w.observe(id).unwrap());
    assert_eq!(observed["player"], id);
    assert_eq!(observed["character"]["hp"], w.players[&id].character.hp);
    assert_eq!(observed["oxygen"], 98);
    assert_eq!(observed["time_ms"], 150);
    assert_eq!(
        numeric_stats(&cache.encode(id, w.stats(id, id).unwrap()))[&64],
        98
    );
    assert_eq!(w.now, 150);
}

#[test]
fn generated_trench_connects_spawn_vents_and_thessal_over_walkable_floor() {
    let content = tests::world().content;
    let map = trench::generate(&content, 12351).unwrap();
    let w = World::new(content, Arc::new(map)).unwrap();
    let start = (12, 12);
    let mut seen = BTreeSet::from([start]);
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some((x, y)) = queue.pop_front() {
        for next in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if w.walkable(Pos {
                x: next.0 as f32 + 0.5,
                y: next.1 as f32 + 0.5,
            }) && seen.insert(next)
            {
                queue.push_back(next);
            }
        }
    }
    for (index, cell) in w.map.cells.iter().enumerate() {
        if cell.ground == 0x2e {
            assert!(
                seen.contains(&(index as i32 % 110, index as i32 / 110)),
                "unreachable floor at {index}"
            );
        }
    }
    assert!(seen.contains(&(64, 76)));
    assert!(
        w.entities
            .values()
            .filter(|e| e.kind == trench::VENT)
            .all(|e| seen.contains(&(e.pos.x as i32, e.pos.y as i32)))
    );
    assert!(
        w.entities
            .values()
            .filter(|e| e.kind == trench::VENT)
            .all(|e| e.pos.distance(Pos { x: 64.5, y: 76.5 }) >= 1.)
    );
}
