use super::tests::{arena, ch};
use super::*;

#[test]
fn level_less_boss_quests_survive_map_spawn_and_removal() {
    for kind in [crate::dungeon::abyss::MALPHAS, 0xe32, 0x1740] {
        let mut w = realm();
        assert!(w.content.objects[&kind].quest);
        assert_eq!(w.content.objects[&kind].quest_level, None);
        Arc::make_mut(&mut w.map).cells[0].object = Some(kind);
        let mut w = World::new(w.content.clone(), w.map.clone()).unwrap();
        let id = w.enter(ch(&w), None).unwrap();
        let boss = *w.entities.iter().find(|(_, e)| e.kind == kind).unwrap().0;
        for level in [1, 20] {
            w.players.get_mut(&id).unwrap().character.level = level;
            assert_eq!(w.quest_target(id), Some(boss));
            assert!(w.visible(id).contains(&boss));
        }
        w.remove_entity(boss);
        assert_eq!(w.quest_target(id), None);
        let boss = w
            .spawn_entity(kind, Pos { x: 20.5, y: 20.5 }, vec![])
            .unwrap();
        assert_eq!(w.quest_target(id), Some(boss));
        w.entities.get_mut(&boss).unwrap().hp = 0;
        assert_eq!(w.quest_target(id), None);
    }
}

#[test]
fn abyss_lava_reaches_the_arena_without_cutting_off_the_boss() {
    use crate::dungeon::abyss::{self, BRIDGE, LAVA, MALPHAS};
    let content = tests::world().content;
    let map = abyss::generate(&content, 12351).unwrap();
    let boss = map
        .cells
        .iter()
        .position(|c| c.object == Some(MALPHAS))
        .unwrap();
    let (bx, by) = (boss as i32 % map.width, boss as i32 / map.width);
    let exposed = (by - 18..=by + 18)
        .flat_map(|y| (bx - 18..=bx + 18).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let c = &map.cells[(y * map.width + x) as usize];
            c.ground == LAVA && c.object != Some(BRIDGE)
        })
        .count();
    assert!(
        exposed > 100,
        "the arena must not be exempt from the lava overlay: {exposed}"
    );
    let (sx, sy) = map.spawns[0];
    let start = sy as usize * map.width as usize + sx as usize;
    let mut seen = BTreeSet::from([start]);
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i as i32 % map.width, i as i32 / map.width);
        for (x, y) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if x < 0 || y < 0 || x >= map.width || y >= map.height {
                continue;
            }
            let next = (y * map.width + x) as usize;
            let c = &map.cells[next];
            if c.ground != 0xfe
                && (c.ground != LAVA || c.object == Some(BRIDGE))
                && seen.insert(next)
            {
                queue.push_back(next);
            }
        }
    }
    assert!(
        seen.contains(&boss),
        "spawn must retain a lava-safe route to Malphas"
    );
}

fn realm() -> World {
    let mut w = arena(6);
    w.safe = false;
    w
}

#[test]
fn quest_window_offers_the_scorpion_queen_to_a_level_one_character() {
    let mut w = realm();
    let id = w.enter(ch(&w), None).unwrap();
    let pos = w.players[&id].pos;
    let scorpion = w.spawn_entity(0x604, pos, vec![]).unwrap();
    w.players.get_mut(&id).unwrap().character.level = 1;
    assert_eq!(w.quest_target(id), Some(scorpion));
    let lich = w.spawn_entity(0x91b, pos, vec![]).unwrap();
    assert_eq!(
        w.quest_target(id),
        Some(scorpion),
        "the Lich quest starts at level 5"
    );
    w.entities.get_mut(&lich).unwrap().hp = 0;
    assert_eq!(w.quest_target(id), Some(scorpion));
}

#[test]
fn quest_window_excludes_quests_outside_their_documented_levels() {
    let mut w = realm();
    let id = w.enter(ch(&w), None).unwrap();
    let pos = w.players[&id].pos;
    let scorpion = w.spawn_entity(0x604, pos, vec![]).unwrap();
    w.players.get_mut(&id).unwrap().character.level = 6;
    assert_eq!(
        w.quest_target(id),
        Some(scorpion),
        "the Scorpion Queen quest covers levels 1 to 6"
    );
    w.players.get_mut(&id).unwrap().character.level = 20;
    assert_eq!(
        w.quest_target(id),
        None,
        "a level 20 character is not offered the Scorpion Queen quest"
    );
    let lich = w.spawn_entity(0x91b, pos, vec![]).unwrap();
    assert_eq!(w.quest_target(id), Some(lich));
    w.players.get_mut(&id).unwrap().character.level = 1;
    assert_eq!(
        w.quest_target(id),
        Some(scorpion),
        "a level 1 character is offered the Scorpion Queen quest"
    );
    w.entities.get_mut(&scorpion).unwrap().hp = 0;
    assert_eq!(
        w.quest_target(id),
        None,
        "a level 1 character is not offered the Lich quest"
    );
}
