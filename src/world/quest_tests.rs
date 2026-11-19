use super::*;

fn quest_fixture() -> (World, i32, i32, i32) {
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
    (w, player, lich, ent)
}

#[test]
fn quest_selection_keeps_level_distance_identity_and_eligibility_separate() {
    let (mut w, player, lich, ent) = quest_fixture();
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

#[test]
fn autonomous_quest_target_keeps_the_selected_arrow_across_a_distance_tie() {
    let (mut w, player, lich, ent) = quest_fixture();
    assert_eq!(w.quest_target(player), Some(lich));
    let mut inputs = bot_inputs::Inputs::default();
    inputs.preferred_quest = Some(lich);
    w.players.get_mut(&player).unwrap().inputs = Some(inputs);
    w.players.get_mut(&player).unwrap().pos.x = 29.5;
    assert_eq!(
        w.quest_target(player),
        Some(ent),
        "native arrow must switch"
    );
    w.players.get_mut(&player).unwrap().controller = Controller::Autonomous;
    assert_eq!(
        w.quest_target(player),
        Some(lich),
        "the chosen quest is still close"
    );
    w.players.get_mut(&player).unwrap().pos.x = 41.5;
    assert_eq!(
        w.quest_target(player),
        Some(ent),
        "the other quest is clearly closer"
    );
    let p = w.players.get_mut(&player).unwrap();
    p.pos.x = 27.5;
    p.inputs
        .as_mut()
        .unwrap()
        .unreachable_targets
        .insert(lich, 10);
    assert_eq!(
        w.quest_target(player),
        Some(ent),
        "the closest quest is unreachable"
    );
    w.now = 10;
    assert_eq!(
        w.quest_target(player),
        Some(lich),
        "route exclusion expired"
    );
    w.entities.get_mut(&lich).unwrap().hp = 0;
    assert_eq!(
        w.quest_target(player),
        Some(ent),
        "a dead quest cannot be kept"
    );
    w.remove_entity(lich);
    assert_eq!(
        w.quest_target(player),
        Some(ent),
        "a vanished quest cannot be kept"
    );
}
