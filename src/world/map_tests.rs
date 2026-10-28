use super::*;

#[test]
fn serialized_development_map_preserves_schema_and_private_visibility() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
        width: 8,
        height: 8,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            64
        ],
        spawns: vec![(4.5, 4.5)],
    };
    let mut world = World::new(content, Arc::new(map)).unwrap();
    let mut character = Character::fresh(world.content.get(0x30e).unwrap()).unwrap();
    character.account = 101;
    character.id = 1001;
    let player = world.enter(character, None).unwrap();
    let public = world
        .loot_bag(Pos { x: 2.5, y: 2.5 }, None, vec![0xa1f])
        .unwrap();
    let own = world
        .loot_bag(Pos { x: 3.5, y: 2.5 }, Some(101), vec![0xa1f])
        .unwrap();
    let other = world
        .loot_bag(Pos { x: 4.5, y: 2.5 }, Some(202), vec![0xa1f])
        .unwrap();
    for (observer, account) in [(None, -1), (Some(player), 101)] {
        let actual: serde_json::Value =
            serde_json::from_slice(&world.inspect_map(observer).unwrap()).unwrap();
        let entities: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.accessible(account))
            .collect();
        assert_eq!(
            actual,
            serde_json::json!({"width":8,"height":8,"blocked":[],"entities":entities})
        );
        let ids: Vec<_> = actual["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_i64().unwrap() as i32)
            .collect();
        assert!(ids.contains(&public));
        assert_eq!(ids.contains(&own), observer.is_some());
        assert!(!ids.contains(&other));
    }
    world.entities.get_mut(&public).unwrap().pos.x = 2.3;
    let actual: serde_json::Value =
        serde_json::from_slice(&world.inspect_map(None).unwrap()).unwrap();
    let serialized = actual["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == public)
        .unwrap();
    assert_eq!(
        serialized["pos"]["x"].as_f64().unwrap() as f32,
        world.entities[&public].pos.x
    );
    assert!(world.inspect_map(Some(i32::MAX)).is_err());
}
