use super::*;

#[test]
fn sphinx_site_keeps_the_complete_reference_sand_star() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
        width: 256,
        height: 256,
        cells: vec![
            crate::content::Cell {
                ground: 0xf5,
                object: None,
                config: String::new(),
                terrain: 1,
                region: 0,
            };
            256 * 256
        ],
        spawns: vec![(1.5, 1.5)],
    };
    let mut world = World::new(content, Arc::new(map)).unwrap();
    world.rng = rand::rngs::StdRng::seed_from_u64(12351);
    let boss = world
        .place_named_setpiece(crate::dungeon::tomb::SPHINX, &[1])
        .unwrap();
    let origin = (boss.x as usize - 40, boss.y as usize - 40);
    let rows: Vec<_> = "
        ........#........
        ........#........
        .......###.......
        ...###.###.###...
        ...###########...
        ...###########...
        ....#########....
        ..#############..
        #################
        ..#############..
        ....#########....
        ...###########...
        ...###########...
        ...###.###.###...
        .......###.......
        ........#........
        ........#........
    "
    .split_whitespace()
    .collect();
    for y in 0..81 {
        for x in 0..81 {
            let expected = (32..49).contains(&x)
                && (32..49).contains(&y)
                && rows[y - 32].as_bytes()[x - 32] == b'#';
            let cell = &world.map.cells[(origin.1 + y) * 256 + origin.0 + x];
            assert_eq!(cell.ground == 0xf7, expected, "local tile ({x},{y})");
        }
    }
    assert_eq!(
        world
            .entities
            .values()
            .filter(|e| e.kind == crate::dungeon::tomb::WALL)
            .count(),
        24
    );
}
