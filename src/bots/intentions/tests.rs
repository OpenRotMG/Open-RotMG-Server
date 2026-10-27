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
    mind.intentions.as_mut().unwrap().observe(&v, &c).unwrap();
    v.objects.push(bag(3, 0xa1f, 11.5));
    mind.intentions
        .as_mut()
        .unwrap()
        .selected(&v, &c, &Goal::Loot(3), false, None)
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
