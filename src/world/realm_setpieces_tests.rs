use super::*;
use std::collections::VecDeque;

fn accessible(w: &World, start: Pos) -> bool {
    let start = (start.x as i32, start.y as i32);
    let mut queue = VecDeque::from([start]);
    let mut seen = BTreeSet::from([start]);
    while let Some((x, y)) = queue.pop_front() {
        if (x - start.0).abs().max((y - start.1).abs()) >= 18 {
            return true;
        }
        for p in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if !seen.contains(&p)
                && w.walkable(Pos {
                    x: p.0 as f32 + 0.5,
                    y: p.1 as f32 + 0.5,
                })
            {
                seen.insert(p);
                queue.push_back(p);
            }
        }
    }
    false
}
#[test]
fn named_setpieces_are_accessible_and_preserve_templates_camps_and_closure() {
    let content = tests::world().content;
    for seed in [1, 2, 42, 12351] {
        let original = Arc::new(crate::terrain::generate(&content, seed, 512).unwrap());
        let mut w = World::new(content.clone(), original.clone()).unwrap();
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.populate_shore().unwrap();
        w.populate_inland().unwrap();
        w.populate_realm_heroes().unwrap();
        let protected: Vec<_> = w
            .entities
            .values()
            .filter(|e| {
                let o = &content.objects[&e.kind];
                o.enemy || !o.static_object
            })
            .map(|e| (e.id, e.kind, e.pos, e.hp))
            .collect();
        let slots: Vec<_> = w.population.iter().map(|s| s.pos).collect();
        w.populate_named_quests().unwrap();
        assert!(!Arc::ptr_eq(&w.map, &original));
        for (id, kind, pos, hp) in protected {
            let e = &w.entities[&id];
            assert_eq!((e.kind, e.pos.x, e.pos.y, e.hp), (kind, pos.x, pos.y, hp));
        }
        for pos in slots {
            let old = original.cell(pos.x, pos.y).unwrap();
            let new = w.map.cell(pos.x, pos.y).unwrap();
            assert_eq!(
                (old.ground, old.object, old.terrain),
                (new.ground, new.object, new.terrain)
            );
        }
        assert_eq!(w.hero_identities().len(), 10);
        let roots: Vec<_> = w
            .entities
            .values()
            .filter(|e| crate::quest::ROOTS.contains(&e.kind))
            .collect();
        assert_eq!(roots.len(), 4);
        for e in &roots {
            let expected = match e.kind {
                0x91f => 0x46,
                0x91b => 0xad,
                0x928 => 0x60,
                0x675 => 0xf2,
                _ => unreachable!(),
            };
            assert_eq!(w.map.cell(e.pos.x, e.pos.y).unwrap().ground, expected);
            assert!(
                accessible(&w, e.pos),
                "setpiece {:#x} sealed on seed {seed}",
                e.kind
            );
            assert_eq!(e.hp, content.objects[&e.kind].hp);
            assert_eq!(e.quest, Some(e.id));
            assert!(
                roots
                    .iter()
                    .all(|other| other.id == e.id || other.pos.distance(e.pos) >= 32.)
            );
            let area: Vec<_> = w
                .entities
                .values()
                .filter(|p| p.pos.distance(e.pos) < 18.)
                .map(|p| p.kind)
                .collect();
            match e.kind {
                0x91f => assert!(area.iter().filter(|k| **k == 0x194).count() > 20),
                0x91b => {
                    assert!(area.contains(&0x160) && area.contains(&0x162));
                    assert!(area.contains(&0x17d) || area.contains(&0x182));
                }
                0x928 => assert!(area.iter().filter(|k| **k == 0x120).count() > 100),
                0x675 => assert!(w.map.cells.iter().filter(|c| c.ground == 0xf2).count() > 200),
                _ => unreachable!(),
            }
        }
    }
}
#[test]
fn temple_walls_are_damageable_cover_not_npcs_or_quest_rewards() {
    let mut d = Directory::test_prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        12351,
    )
    .unwrap();
    let w = d.worlds.get_mut(&-3).unwrap();
    let wall = w.entities.values().find(|e| e.kind == 0x162).unwrap().id;
    let pos = w.entities[&wall].pos;
    assert!(!w.walkable(pos));
    let p = w.enter(tests::ch(w), None).unwrap();
    let exp = w.players[&p].character.exp;
    w.now = 1000;
    w.npc_step();
    assert_eq!(
        (w.entities[&wall].pos.x, w.entities[&wall].pos.y),
        (pos.x, pos.y)
    );
    w.damage_enemy(p, wall, 1000, true, &[], None).unwrap();
    assert!(!w.entities.contains_key(&wall));
    assert!(w.walkable(pos));
    assert_eq!(w.players[&p].character.exp, exp);
    assert!(w.critical_deaths.is_empty());
    assert_eq!(w.hero_identities().len(), 14);
}
#[test]
fn missing_setpiece_space_errors_without_plain_terrain_boss_fallback() {
    let mut w = tests::world();
    let old = w.entities.len();
    let map = w.map.clone();
    let error = w.populate_named_quests().unwrap_err().to_string();
    assert!(error.contains("setpiece"), "{error}");
    assert_eq!(w.entities.len(), old);
    assert!(Arc::ptr_eq(&w.map, &map));
    assert!(
        w.entities
            .values()
            .all(|e| !crate::quest::ROOTS.contains(&e.kind))
    );
}
