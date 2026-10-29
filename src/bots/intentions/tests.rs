use super::*;

fn fixture() -> (Content, PlayerView) {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.objects.clear();
    v.quest = None;
    (c, v)
}

#[test]
fn item_selection_is_useful_priced_missing_and_varied_without_guessed_targets() {
    let (c, mut v) = fixture();
    let mut i = Intentions::default();
    i.observe(&v, &c, "Mossy").unwrap();
    let target = i
        .item_target()
        .expect("item-minded identity selects an upgrade");
    assert!(c.item_values.quote_range(target).is_ok());
    assert!(!v.character.items.contains(&target));
    assert_eq!(
        progression::priority(&v, &c, target, false, None).unwrap(),
        140
    );
    let item = c.get(target as u16).unwrap();
    assert!(
        matches!(&i.main.as_ref().unwrap().objective, Objective::Item { reason, .. } if reason.contains(&item.name))
    );
    let mut malformed = serde_json::to_value(&i).unwrap();
    malformed["main"]["objective"]["item"]
        .as_object_mut()
        .unwrap()
        .remove("reason");
    assert!(serde_json::from_value::<Intentions>(malformed).is_err());
    v.character.items[4] = target;
    assert_ne!(
        item_upgrade(&v, &c, "Mossy").unwrap().map(|(k, _)| k),
        Some(target)
    );
    let mut ordinary = Intentions::default();
    ordinary.observe(&v, &c, "Bayleaf").unwrap();
    assert_eq!(
        ordinary.main.as_ref().unwrap().objective,
        Objective::Reach20
    );
    v.character.level = 20;
    ordinary.end("test next milestone");
    ordinary.observe(&v, &c, "Bayleaf").unwrap();
    assert_eq!(ordinary.main.as_ref().unwrap().objective, Objective::Speed);
    v.character.base_stats[4] = c.classes[&v.character.class].caps[4];
    ordinary.end("test next milestone");
    ordinary.observe(&v, &c, "Bayleaf").unwrap();
    assert_eq!(ordinary.main.as_ref().unwrap().objective, Objective::Fame);

    // Owning the strongest eligible gear leaves no useful researched upgrade to guess.
    for slot in 0..4 {
        let slot_type = c.get(v.character.class).unwrap().slots[slot];
        for kind in c
            .item_values
            .kinds()
            .filter(|k| c.item_values.quote_range(*k).is_ok())
        {
            if c.get(kind as u16).unwrap().slot_type == slot_type
                && progression::quality(&v, &c, kind).unwrap()
                    > progression::quality(&v, &c, v.character.items[slot]).unwrap()
            {
                v.character.items[slot] = kind;
            }
        }
    }
    i.end("test no missing upgrade");
    i.observe(&v, &c, "Mossy").unwrap();
    assert_eq!(i.item_target(), None);
    assert_eq!(i.main.as_ref().unwrap().objective, Objective::Fame);
}

#[test]
fn item_goal_is_invalidated_on_character_change_and_death_not_restored() {
    let (c, mut v) = fixture();
    let mut i = Intentions::default();
    i.observe(&v, &c, "Mossy").unwrap();
    let old = i.main.as_ref().unwrap().id;
    v.session.character += 1;
    i.bind(&v);
    assert!(i.main.is_none());
    assert!(i.transition.contains("character changed"));
    i.observe(&v, &c, "Mossy").unwrap();
    assert!(i.main.as_ref().unwrap().id > old);
    assert_eq!(i.main.as_ref().unwrap().character, v.session.character);
    let new = i.main.as_ref().unwrap().id;
    v.lifecycle.death_pending = true;
    i.observe(&v, &c, "Mossy").unwrap();
    assert!(i.main.is_none());
    assert!(i.transition.contains("character died"));
    v.lifecycle.death_pending = false;
    v.lifecycle.alive = false;
    i.observe(&v, &c, "Mossy").unwrap();
    assert!(i.main.is_none());
    v.lifecycle.alive = true;
    v.session.character += 1;
    i.bind(&v);
    i.observe(&v, &c, "Mossy").unwrap();
    assert!(i.main.as_ref().unwrap().id > new);
    assert!(!v.character.items.contains(&i.item_target().unwrap()));
}

#[test]
fn stalled_goal_is_abandoned_and_set_aside_for_the_next_objective() {
    let (c, v) = fixture();
    let mut i = Intentions::default();
    i.observe(&v, &c, "Bayleaf").unwrap();
    assert_eq!(i.main.as_ref().unwrap().objective, Objective::Reach20);
    i.abandon("no XP in 15 minutes", plan::now());
    i.observe(&v, &c, "Bayleaf").unwrap();
    assert_eq!(
        i.main.as_ref().unwrap().objective,
        Objective::Fame,
        "Speed waits for level 20"
    );
    i.validate().unwrap();
}
