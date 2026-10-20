use super::tests::{arena, ch};
use super::*;

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
