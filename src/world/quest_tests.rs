use super::*;

#[test]
fn quest_selection_keeps_level_distance_identity_and_eligibility_separate() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
        width: 64,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            64 * 32
        ],
        spawns: vec![(28.5, 8.5)],
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    let mut ch = Character::fresh(w.content.get(0x30e).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.level = 10;
    let player = w.enter(ch, None).unwrap();
    let lich = w
        .spawn_entity(0x91b, Pos { x: 8.5, y: 8.5 }, vec![])
        .unwrap();
    let ent = w
        .spawn_entity(0x91f, Pos { x: 48.5, y: 8.5 }, vec![])
        .unwrap();
    assert_eq!(w.content.objects[&0x91b].quest_level, Some(9));
    assert_eq!(w.content.objects[&0x91f].quest_level, Some(11));
    assert_eq!(w.quest_target(player), Some(lich));
    w.players.get_mut(&player).unwrap().pos.x = 29.5;
    assert_eq!(w.quest_target(player), Some(ent));
    w.players.get_mut(&player).unwrap().character.level = 9;
    assert_eq!(w.quest_target(player), Some(lich));
    w.entities.get_mut(&lich).unwrap().hp = 0;
    assert_eq!(w.quest_target(player), Some(ent));
    w.entities.get_mut(&ent).unwrap().published = false;
    assert_eq!(w.quest_target(player), None);
    w.entities.get_mut(&ent).unwrap().published = true;
    w.entities.get_mut(&ent).unwrap().extra_stats.push((54, 2));
    assert_eq!(w.quest_target(player), None);
    w.entities.get_mut(&ent).unwrap().extra_stats.clear();
    w.players.get_mut(&player).unwrap().character.level = 1;
    assert_eq!(w.quest_target(player), None);
    let queen = w
        .spawn_entity(0x604, Pos { x: 30.5, y: 8.5 }, vec![])
        .unwrap();
    assert_eq!(w.quest_target(player), Some(queen));
    w.players.get_mut(&player).unwrap().character.level = 20;
    assert_eq!(w.quest_target(player), Some(ent));
    w.remove_entity(ent);
    assert_eq!(w.quest_target(player), None);
    let malphas = w
        .spawn_entity(0x90a, Pos { x: 50.5, y: 8.5 }, vec![])
        .unwrap();
    assert_eq!(w.content.objects[&0x90a].quest_level, None);
    assert_eq!(w.quest_target(player), Some(malphas));
    w.safe = true;
    assert_eq!(w.quest_target(player), None);
}
