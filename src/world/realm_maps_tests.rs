use super::*;
use crate::terrain::realms::{Source, sources};
use std::path::Path;

#[test]
fn all_three_full_realms_preserve_water_scenery_and_safe_arrivals() {
    let path = Path::new("data");
    let content = Arc::new(Content::load(path).unwrap());
    for (source, expected_scenery) in sources(path)
        .unwrap()
        .into_iter()
        .zip([264491, 246050, 273177])
    {
        let Source::Encoded(name, bytes) = &source else {
            panic!("expected production asset")
        };
        let original = Map::decode(bytes, &content).unwrap();
        assert_eq!(
            original.cells.iter().filter(|c| c.object.is_some()).count(),
            expected_scenery
        );
        let water: Vec<_> = original
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.ground, 0x72 | 0x73))
            .map(|(i, c)| (i, c.ground))
            .collect();
        let name = *name;
        drop(original);
        let started = Instant::now();
        let mut directory =
            Directory::configured(path, content.clone(), 8, 12351, Some(12351), vec![source])
                .unwrap();
        let world = directory.worlds.get_mut(&-3).unwrap();
        assert_eq!(world.realm_layout, Some(name));
        assert_eq!((world.map.width, world.map.height), (2048, 2048));
        assert_eq!(world.map.spawns.len(), 4);
        for (i, ground) in water {
            assert_eq!(world.map.cells[i].ground, ground, "{name}: water at {i}");
        }
        for &(x, y) in &world.map.spawns {
            for y in y as i32 - 1..=y as i32 + 1 {
                for x in x as i32 - 1..=x as i32 + 1 {
                    let pos = Pos {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    let cell = world.map.cell(pos.x, pos.y).unwrap();
                    assert_eq!((cell.terrain, cell.region, cell.object), (11, 1, None));
                    assert!(world.walkable(pos));
                }
            }
        }
        for (i, cell) in world
            .map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object.is_some())
        {
            let pos = Pos {
                x: (i % 2048) as f32 + 0.5,
                y: (i / 2048) as f32 + 0.5,
            };
            let entity = world
                .entities_at(pos)
                .find(|e| Some(e.kind) == cell.object)
                .unwrap();
            if let Some(size) = cell.config.strip_prefix("size:") {
                assert_eq!(entity.size, size.parse::<i32>().unwrap());
            }
        }
        let player = world.enter(tests::ch(world), None).unwrap();
        assert!(world.players[&player].character.alive);
        let construction_ms = started.elapsed().as_millis();
        let started = Instant::now();
        for _ in 0..4 {
            world.step();
        }
        eprintln!(
            "{name}: construction_ms={construction_ms}, tick_mean_us={}, entities={}",
            started.elapsed().as_micros() / 4,
            world.entities.len()
        );
        assert_eq!(world.tick, 4);
        assert!(world.players[&player].character.alive);
        if name == "realm-fsod-1" {
            let original = world.map.cells[0].region;
            let mut second = directory.make("realm-shore", "Realm", false, None).unwrap();
            assert!(!Arc::ptr_eq(&directory.worlds[&-3].map, &second.map));
            Arc::make_mut(&mut second.map).cells[0].region = 99;
            assert_eq!(directory.worlds[&-3].map.cells[0].region, original);
        }
    }
}

#[test]
fn spatial_queries_keep_passive_scenery_and_transient_actors_indexed() {
    let content = tests::world().content;
    let mut map = Map {
        width: 8,
        height: 8,
        spawns: vec![(4.5, 4.5)],
        cells: vec![
            crate::content::Cell {
                ground: 0xbe,
                object: None,
                config: String::new(),
                terrain: 11,
                region: 0
            };
            64
        ],
    };
    for (x, y, kind) in [(1, 1, 0x107), (3, 3, 0x600), (6, 2, 0x802)] {
        map.cells[y * 8 + x].object = Some(kind);
    }
    let mut world = World::new(content, Arc::new(map)).unwrap();
    let rock = world
        .entities
        .values()
        .find(|e| e.kind == 0x107)
        .unwrap()
        .id;
    assert!(!world.actor_ids.contains(&rock));
    assert_eq!(world.actors().count(), 2);
    assert!(!world.walkable(Pos { x: 1.5, y: 1.5 }));
    for y in 0..8 {
        for x in 0..8 {
            let pos = Pos {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            let indexed: BTreeSet<_> = world.entities_near(pos, 2.).map(|e| e.id).collect();
            let scanned: BTreeSet<_> = world
                .entities
                .values()
                .filter(|e| e.pos.distance(pos) < 2.)
                .map(|e| e.id)
                .collect();
            assert_eq!(indexed, scanned);
        }
    }
    let transient = world
        .spawn_entity(0x107, Pos { x: 6.5, y: 6.5 }, vec![])
        .unwrap();
    assert!(world.actor_ids.contains(&transient));
    world.entities.get_mut(&transient).unwrap().expires = Some(50);
    world.now = 50;
    world.mechanic_expiry();
    assert!(!world.actor_ids.contains(&transient));
    assert!(!world.entities.contains_key(&transient));
    assert!(world.entities_at(Pos { x: 6.5, y: 6.5 }).next().is_none());
}

#[test]
fn realm_layout_selection_happens_at_creation_not_player_entry() {
    let content = tests::world().content;
    let mut directory = Directory::load(Path::new("data"), content, 8).unwrap();
    let Source::Fixture(map) = directory.realms.pop().unwrap() else {
        panic!("expected fixture")
    };
    directory.realms = (0..3)
        .map(|index| {
            let mut marked = (*map).clone();
            marked.cells[0].region = 20 + index;
            Source::Fixture(Arc::new(marked))
        })
        .collect();
    let mut rng = directory.realm_rng.clone();
    let mut selected = BTreeSet::new();
    for _ in 0..12 {
        let index = rng.gen_range(0..3usize);
        selected.insert(index);
        let mut world = directory.make("realm-shore", "Realm", false, None).unwrap();
        assert_eq!(world.map.cells[0].region, 20 + index as u8);
        let layout = world.realm_layout;
        let after_creation = directory.realm_rng.clone();
        world.enter(tests::ch(&world), None).unwrap();
        assert_eq!(world.realm_layout, layout);
        use rand::RngCore;
        assert_eq!(
            directory.realm_rng.clone().next_u64(),
            after_creation.clone().next_u64()
        );
        assert_eq!(
            directory.realm_rng.clone().next_u64(),
            rng.clone().next_u64()
        );
    }
    assert_eq!(selected.len(), 3);
}
