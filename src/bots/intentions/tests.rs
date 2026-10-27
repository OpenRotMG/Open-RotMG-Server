use super::*;
use crate::world::view::VisibleObject;

fn fixture() -> (Content, PlayerView) {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.objects.clear();
    v.quest = None;
    (c, v)
}
fn bag(id: i32, item: i32, x: f32) -> VisibleObject {
    VisibleObject {
        id,
        account: None,
        kind: 0x500,
        name: String::new(),
        position: Pos { x, y: 10.5 },
        hp: 0,
        max_hp: 0,
        level: None,
        equipped: vec![],
        contents: vec![item],
        blocks: false,
        usable: false,
        condition_mask: None,
    }
}

#[tokio::test]
async fn persisted_main_goal_and_detour_reopen_without_json_loss() {
    let (c, mut v) = fixture();
    let directory = std::env::temp_dir().join(format!("bot-intentions-{}", rand::random::<u64>()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("game.db");
    let store = crate::store::Store::open(&path, &c).unwrap();
    let account = store.list("intentions-reopen".into()).await.unwrap();
    v.session.account = account.id;
    let mut mind = pursuits::Mind::new(account.id);
    mind.bind(&v, 1).unwrap();
    mind.intentions
        .as_mut()
        .unwrap()
        .observe(&v, &c, "Mossy")
        .unwrap();
    assert!(mind.intentions.as_ref().unwrap().item_target().is_some());
    v.objects.push(bag(3, 0xa1f, 11.5));
    mind.intentions
        .as_mut()
        .unwrap()
        .selected(&v, &c, &Goal::Loot(3), false, None, None)
        .unwrap();
    mind.revision = store.save_bot_mind(mind.clone()).await.unwrap();
    drop(store);
    let reopened = crate::store::Store::open(&path, &c).unwrap();
    let saved = reopened.bot_mind(account.id).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        serde_json::to_value(mind).unwrap()
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

fn decision(goal: Goal) -> Decision {
    Decision {
        goal,
        movement: None,
        aim: None,
        item: None,
        journey: None,
        chat: None,
    }
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
    assert!(
        i.transition
            .contains("no eligible missing researched upgrade")
    );
}

#[test]
fn item_identity_and_reason_survive_loot_company_combat_dungeon_and_same_character_return() {
    let (c, mut v) = fixture();
    let mut i = Intentions::default();
    i.bind(&v, 7);
    i.observe(&v, &c, "Mossy").unwrap();
    let main = i.main.clone();
    v.objects.push(bag(3, 0xa1f, 11.5));
    i.selected(&v, &c, &Goal::Loot(3), false, None, None)
        .unwrap();
    v.character.items[4] = 0xa1f;
    v.character.inventory_revision += 1;
    i.observe(&v, &c, "Mossy").unwrap();
    assert!(i.detour.is_none());
    for goal in [Goal::Follow(4), Goal::Portal(5)] {
        let payload = i.payload(
            &v,
            Activity::Adventure,
            &decision(Goal::Fight(9)),
            "fight".into(),
            false,
        );
        assert_eq!(payload["goal"]["id"], main.as_ref().unwrap().id);
        v.elapsed_ms += MAIN_MS;
        v.character.experience += 1;
        i.observe(&v, &c, "Mossy").unwrap();
        if goal == Goal::Follow(4) {
            let mut peer = bag(4, -1, 25.5);
            peer.hp = 500;
            v.objects.push(peer);
        } else {
            let mut portal = bag(5, -1, 11.5);
            portal.kind = 0x717;
            v.objects.push(portal);
        }
        i.selected(&v, &c, &goal, false, None, None).unwrap();
        if goal == Goal::Follow(4) {
            v.objects.iter_mut().find(|o| o.id == 4).unwrap().position = v.position;
        } else {
            v.map.name = "Pirate Cave".into();
            v.session.world = 11;
        }
        i.observe(&v, &c, "Mossy").unwrap();
        let p = i.payload(
            &v,
            Activity::Consume,
            &decision(Goal::Wait),
            "consume own potion".into(),
            false,
        );
        assert_eq!(p["mode"], "necessary");
        assert_eq!(i.main, main);
    }
    let mut restored: Intentions =
        serde_json::from_value(serde_json::to_value(&i).unwrap()).unwrap();
    v.elapsed_ms = 0;
    restored.bind(&v, 8);
    restored.observe(&v, &c, "Mossy").unwrap();
    assert_eq!(restored.main, main);
    assert!(restored.detour.is_none());
    assert_eq!(restored.main_remaining_ms, MAIN_MS);
    restored.payload(
        &v,
        Activity::Adventure,
        &decision(Goal::Fight(9)),
        "resume fight".into(),
        false,
    );
    v.elapsed_ms += MAIN_MS;
    v.character.experience += 1;
    restored.observe(&v, &c, "Mossy").unwrap();
    assert!(restored.available(&v));
    assert_eq!(restored.main, main);
    restored.validate().unwrap();
}

#[test]
fn item_completion_requires_committed_own_holdings_and_preserves_dungeon_exit_budget() {
    for carried in [false, true] {
        let (c, mut v) = fixture();
        let mut i = Intentions::default();
        i.observe(&v, &c, "Mossy").unwrap();
        let main = i.main.clone();
        let target = i.item_target().unwrap();
        let mut peer = bag(8, target, 10.5);
        peer.equipped = vec![target];
        v.objects.push(peer);
        i.main_remaining_ms = MAIN_MS;
        assert!(i.eligible(&v, &Goal::Loot(8), false));
        assert_eq!(i.bonus(&v, &Goal::Loot(8)), 30.);
        i.selected(&v, &c, &Goal::Loot(8), false, None, None)
            .unwrap();
        assert!(
            i.detour.is_none(),
            "target loot is main progress, not another detour"
        );
        i.observe(&v, &c, "Mossy").unwrap();
        assert_eq!(
            i.main, main,
            "visible loot, offers and peer equipment are not owned"
        );
        let slot = if carried {
            4
        } else {
            c.get(v.character.class)
                .unwrap()
                .slots
                .iter()
                .take(4)
                .position(|s| *s == c.get(target as u16).unwrap().slot_type)
                .unwrap()
        };
        v.character.items[slot] = target;
        i.observe(&v, &c, "Mossy").unwrap();
        assert_eq!(i.main, main, "unchanged revision cannot prove acquisition");
        v.character.items[slot] = -1;
        v.character.inventory_revision += 1;
        i.observe(&v, &c, "Mossy").unwrap();
        assert_eq!(
            i.main, main,
            "a committed unrelated write does not acquire an offered target"
        );
        v.character.items[slot] = target;
        for guard in 0..3 {
            v.lifecycle.busy = guard == 0;
            v.lifecycle.departing = guard == 1;
            v.lifecycle.quarantined = guard == 2;
            i.observe(&v, &c, "Mossy").unwrap();
            assert_eq!(i.main, main);
        }
        v.lifecycle.quarantined = false;
        v.map.name = "Pirate Cave".into();
        i.detour = Some(Detour {
            runtime: 0,
            world: v.session.world,
            remaining_ms: 1234,
            opportunity: Opportunity::Dungeon {
                portal: 5,
                destination: v.map.name.clone(),
                entered: true,
            },
        });
        i.observe(&v, &c, "Mossy").unwrap();
        assert_ne!(i.main.as_ref().unwrap().id, main.unwrap().id);
        assert_ne!(i.item_target(), Some(target));
        assert!(
            i.transition
                .contains("completed by committed own observation")
        );
        assert_eq!(i.detour.as_ref().unwrap().remaining_ms, 1234);
        i.validate().unwrap();
        v.elapsed_ms += 1234;
        i.observe(&v, &c, "Mossy").unwrap();
        assert!(
            i.expired(),
            "acquisition cannot cancel the entered dungeon's exit budget"
        );
    }
}

#[test]
fn old_milestones_roundtrip_and_rebind_without_invented_item_targets() {
    let (c, mut v) = fixture();
    for (objective, target) in [
        ("reach20", 20),
        ("speed", c.classes[&v.character.class].caps[4]),
        ("fame", 200),
    ] {
        let old = serde_json::json!({"next_id":2,"main":{"id":1,"character":v.session.character,"objective":objective,"target":target},
            "detour":null,"main_remaining_ms":60000,"needs_progress":true,"return_to_main":false,
            "checkpoint":[20000,25,20],"transition":"main goal selected from own character progression"});
        let mut i: Intentions = serde_json::from_value(old.clone()).unwrap();
        i.validate().unwrap();
        i.bind(&v, 2);
        assert_eq!(serde_json::to_value(&i).unwrap(), old);
        v.elapsed_ms = 0;
        i.observe(&v, &c, "Mossy").unwrap();
        assert_eq!(i.main.as_ref().unwrap().id, 1);
        assert_eq!(i.item_target(), None);
        assert_eq!(i.main_remaining_ms, MAIN_MS);
    }
}

#[test]
fn item_goal_is_invalidated_on_character_change_and_death_not_restored() {
    let (c, mut v) = fixture();
    let mut i = Intentions::default();
    i.observe(&v, &c, "Mossy").unwrap();
    let old = i.main.as_ref().unwrap().id;
    v.session.character += 1;
    i.bind(&v, 2);
    assert!(i.main.is_none());
    assert!(i.detour.is_none());
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
    i.bind(&v, 3);
    i.observe(&v, &c, "Mossy").unwrap();
    assert!(i.main.as_ref().unwrap().id > new);
    assert!(!v.character.items.contains(&i.item_target().unwrap()));
}

#[test]
fn obsolete_item_abort_is_explicit_and_preserves_entered_dungeon_budget() {
    let (c, mut v) = fixture();
    let mut i = Intentions::default();
    i.observe(&v, &c, "Mossy").unwrap();
    let target = i.item_target().unwrap();
    let slot_type = c.get(target as u16).unwrap().slot_type;
    let stronger = c
        .objects
        .values()
        .find(|item| {
            !item.unsupported_equipment
                && item.slot_type == slot_type
                && item.class == "Equipment"
                && progression::quality(&v, &c, i32::from(item.kind)).unwrap()
                    > progression::quality(&v, &c, target).unwrap()
        })
        .unwrap();
    v.character.items[4] = i32::from(stronger.kind);
    v.character.inventory_revision += 1;
    v.map.name = "Pirate Cave".into();
    i.detour = Some(Detour {
        runtime: 0,
        world: v.session.world,
        remaining_ms: DETOUR_MS,
        opportunity: Opportunity::Dungeon {
            portal: 5,
            destination: "Pirate Cave".into(),
            entered: true,
        },
    });
    let excursion = i.detour.clone();
    i.observe(&v, &c, "Mossy").unwrap();
    assert_ne!(i.item_target(), Some(target));
    assert!(i.transition.contains("aborted: no longer improves"));
    assert_eq!(i.detour, excursion);
    i.validate().unwrap();
}
