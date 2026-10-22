use super::tests::{ch, directory};
use super::*;

#[test]
fn geometry_is_shared_but_discovery_snapshots_and_world_revisions_are_isolated() {
    let mut d = directory();
    let a = {
        let w = d.worlds.get_mut(&-1).unwrap();
        w.enter(ch(w), None).unwrap()
    };
    let b = {
        let w = d.worlds.get_mut(&-1).unwrap();
        let mut c = ch(w);
        c.id = 2;
        c.account = 2;
        w.enter(c, None).unwrap()
    };
    let first = d.player_view(a).unwrap();
    let geometry = d.worlds[&-1].view_geometry_for_test();
    let other = d.player_view(b).unwrap();
    assert!(Arc::ptr_eq(
        &geometry,
        &d.worlds[&-1].view_geometry_for_test()
    ));
    assert_ne!(first.tiles.version(), other.tiles.version());
    let point = (0..d.worlds[&-1].map.width as i16)
        .flat_map(|x| (0..d.worlds[&-1].map.height as i16).map(move |y| (x, y)))
        .find(|p| !first.tiles.contains(p) && !other.tiles.contains(p))
        .unwrap();
    {
        let w = d.worlds.get_mut(&-1).unwrap();
        let p = w.players.get_mut(&a).unwrap();
        assert!(p.tiles.insert(point));
        let ground = w.map.cells[point.1 as usize * w.map.width as usize + point.0 as usize].ground;
        p.view_tiles.borrow_mut().discover(
            &w.map,
            &w.content,
            &[(point.0, point.1, ground)],
            p.tiles.len(),
        );
    }
    let updated = d.player_view(a).unwrap();
    assert!(updated.tiles.contains(&point));
    assert!(!first.tiles.contains(&point));
    assert!(!other.tiles.contains(&point));
    assert_eq!(updated.tiles.len(), first.tiles.len() + 1);
    assert!(Arc::ptr_eq(
        &geometry,
        &d.worlds[&-1].view_geometry_for_test()
    ));
    let serialized = serde_json::to_value(&updated.tiles).unwrap();
    let w = d.worlds.get_mut(&-1).unwrap();
    let index = point.1 as usize * w.map.width as usize + point.0 as usize;
    let cell = &mut Arc::make_mut(&mut w.map).cells[index];
    cell.ground = if cell.ground == 0x61 { 0x72 } else { 0x61 };
    let latest = d.player_view(a).unwrap();
    assert!(!Arc::ptr_eq(
        &geometry,
        &d.worlds[&-1].view_geometry_for_test()
    ));
    assert_eq!(serde_json::to_value(&updated.tiles).unwrap(), serialized);
    assert_ne!(latest.tiles.version(), updated.tiles.version());
    let w = &d.worlds[&-1];
    let expected: Vec<_> = w.players[&a].tiles.iter().map(|&(x,y)| {
        let ground = w.map.cells[y as usize*w.map.width as usize+x as usize].ground;
        serde_json::json!({"x":x,"y":y,"ground":ground,"ground_speed":w.content.grounds[&ground]})
    }).collect();
    assert_eq!(
        serde_json::to_value(&latest.tiles).unwrap(),
        serde_json::json!(expected)
    );
}
