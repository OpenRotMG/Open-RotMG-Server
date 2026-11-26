#[test]
fn xml_production_closure_loads() {
    let content = crate::world::test_content();
    let game = content.game.as_ref().unwrap();
    assert!(!game.reachable.is_empty());
    for &kind in &game.reachable {
        game.admission(
            kind,
            game.definitions[&kind].admission_role().unwrap(),
            "production closure",
        )
        .unwrap();
    }
}
