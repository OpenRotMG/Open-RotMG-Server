use super::*;
use std::{collections::BTreeSet, path::PathBuf};

fn assets() -> (Content, Arc<Grounds>) {
    let path =
        PathBuf::from(std::env::var("BUILD100_CLIENT_DIR").expect(
            "realm_map tests require BUILD100_CLIENT_DIR pointing to qualified July assets",
        ));
    (
        Content::load(&path).unwrap(),
        Arc::new(Grounds::load(&path).unwrap()),
    )
}
fn snapshot(map: &Map) -> u64 {
    let mut digest = 0u64;
    for tile in map.tiles() {
        digest = digest.wrapping_mul(31).wrapping_add(u64::from(tile.ground));
    }
    for object in map.objects() {
        digest = digest
            .wrapping_mul(31)
            .wrapping_add(object.kind as u64)
            .wrapping_mul(31)
            .wrapping_add(u64::from(object.x))
            .wrapping_mul(31)
            .wrapping_add(u64::from(object.y));
    }
    digest
}

#[test]
fn realm_map_fixed_seed_corpus_connected_irregular_july_islands() {
    let (content, grounds) = assets();
    let mut snapshots = BTreeSet::new();
    for seed in [0, 1, 2, 42, 100, 20100726, u32::MAX as u64, u64::MAX] {
        let map = realm(seed, &content, Arc::clone(&grounds)).unwrap();
        assert_eq!(map.dimensions(), (256, 256));
        assert_eq!(map.seed, seed);
        assert_eq!(map.kind.name(), "Realm of the Mad God");
        assert_eq!(
            snapshot(&map),
            snapshot(&realm(seed, &content, Arc::clone(&grounds)).unwrap())
        );
        snapshots.insert(snapshot(&map));
        let reached = map.reachable();
        let mut terrains = BTreeSet::new();
        let mut used_grounds = BTreeSet::new();
        let mut left_coast = BTreeSet::new();
        for y in 0..256 {
            let mut left = None;
            for x in 0..256 {
                let tile = map.tile(x, y).unwrap();
                let ground = map.ground(x, y).unwrap();
                assert_eq!(ground.id, tile.ground);
                used_grounds.insert(tile.ground);
                if map.walkable(x, y) && ground.damage.is_none() {
                    assert!(
                        reached[(y * 256 + x) as usize],
                        "seed={seed} unreachable ({x},{y})"
                    );
                    terrains.insert(tile.region.terrain().unwrap());
                    left.get_or_insert(x);
                }
                if x == 0 || y == 0 || x == 255 || y == 255 {
                    assert!(!map.walkable(x, y));
                }
                if tile.reserved {
                    assert!(map.walkable(x, y) && !ground.sink && ground.damage.is_none());
                }
            }
            if let Some(left) = left {
                left_coast.insert(left);
            }
        }
        assert!(left_coast.len() > 40, "seed={seed} rectangular coastline");
        for terrain in [
            "ShoreSand",
            "LowPlains",
            "LowForest",
            "MidPlains",
            "MidForest",
            "HighPlains",
            "HighForest",
            "Mountains",
        ] {
            assert!(terrains.contains(terrain), "seed={seed} missing {terrain}");
        }
        for ground in [0x70, 0x72, 0x73, 0x6a, 0x6b] {
            assert!(
                used_grounds.contains(&ground),
                "seed={seed} missing ground {ground:#x}"
            );
        }
        assert!(!map.objects().is_empty());
        eprintln!(
            "realm_map seed={seed} dimensions=256x256 spawn={:?} objects={} content_digest={:016x}",
            map.spawn,
            map.objects().len(),
            snapshot(&map)
        );
    }
    assert_eq!(snapshots.len(), 8);
}

#[test]
fn realm_map_spawn_hazard_safety_and_reserved_routes() {
    let (content, grounds) = assets();
    for seed in [0, 42, 100, u64::MAX] {
        let map = realm(seed, &content, Arc::clone(&grounds)).unwrap();
        assert_eq!(
            map.tile(map.spawn.0 as i32, map.spawn.1 as i32)
                .unwrap()
                .region,
            Region::ShoreSand
        );
        assert!(map.can_move(map.spawn, map.spawn));
        for (i, tile) in map.tiles().iter().enumerate() {
            let point = ((i % 256) as f32 + 0.5, (i / 256) as f32 + 0.5);
            if map.grounds.get(tile.ground).unwrap().damage.is_some() {
                assert!((point.0 - map.spawn.0).hypot(point.1 - map.spawn.1) >= 8.);
            }
        }
        for p in map.objects() {
            let point = p.position();
            assert!((point.0 - map.spawn.0).hypot(point.1 - map.spawn.1) >= 10.);
            assert!(!map.tile(i32::from(p.x), i32::from(p.y)).unwrap().reserved);
        }
        // Follow only reserved dry cells, independently from the full traversability flood.
        let mut queue = VecDeque::from([(map.spawn.0 as i32, map.spawn.1 as i32)]);
        let mut visited = BTreeSet::new();
        let mut reaches_mountains = false;
        while let Some((x, y)) = queue.pop_front() {
            if !visited.insert((x, y)) {
                continue;
            }
            let tile = map.tile(x, y).unwrap();
            reaches_mountains |= tile.region == Region::Mountains;
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if map.tile(nx, ny).is_ok_and(|t| t.reserved) {
                    assert!(map.can_move(
                        (x as f32 + 0.5, y as f32 + 0.5),
                        (nx as f32 + 0.5, ny as f32 + 0.5)
                    ));
                    if !visited.contains(&(nx, ny)) {
                        queue.push_back((nx, ny));
                    }
                }
            }
        }
        assert!(
            reaches_mountains,
            "seed={seed} reserved route ends before Godlands"
        );
    }
}

#[test]
fn realm_map_ground_content_ids_and_properties() {
    let (content, grounds) = assets();
    assert_eq!(grounds.types.len(), 115);
    for id in [0x00, 0x10, 0x20, 0x30, 0xbc, 0xff] {
        assert!(grounds.get(id).unwrap().no_walk);
    }
    for id in [0x70, 0x72, 0x73] {
        assert!(grounds.get(id).unwrap().sink);
    }
    assert_eq!(grounds.get(0x70).unwrap().damage, Some((30, 60)));
    assert!(!grounds.get(0x6b).unwrap().no_walk);
    assert!(grounds.get(0x6b).unwrap().damage.is_none());
    assert!(grounds.get(0x71).is_err());
    assert!(content.named("Nexus").is_err());
    assert!(content.get(0x100).unwrap().occupy_square);
    assert!(content.get(0x100).unwrap().is_static);
    for id in 0x802..=0x805 {
        assert!(content.get(id).unwrap().occupy_square);
        assert!(!content.get(id).unwrap().is_static);
    }
    assert_eq!(
        content.get(0x806).unwrap().equipment,
        [0xa4f, -1, -1, -1, -1, -1, -1, -1]
    );
}

#[test]
fn realm_map_tutorial_four_proximities_and_exit_accessible_without_kills() {
    let (content, grounds) = assets();
    let map = tutorial(100, &content, Arc::clone(&grounds)).unwrap();
    assert_eq!(map.dimensions(), (32, 32));
    assert_eq!(map.kind.name(), "Tutorial");
    assert_eq!(map.spawn, (4.5, 5.5));
    let reached = map.reachable();
    for (id, name, radius) in [
        (0x81b, "Minimap Sign", 3.),
        (0x81c, "Hitpoints Sign", 3.),
        (0x807, "Tutorial Loot Generator", 1.),
        (0x70e, "Glowing Realm Portal", 1.),
    ] {
        let p = map.objects().iter().find(|p| p.kind == id).unwrap();
        assert_eq!(content.get(id).unwrap().name, name);
        let target = p.position();
        let mut near = false;
        for (i, &accessible) in reached.iter().enumerate() {
            if !accessible {
                continue;
            }
            let center = ((i % 32) as f32 + 0.5, (i / 32) as f32 + 0.5);
            let dx = target.0 - center.0;
            let dy = target.1 - center.1;
            let d = dx.hypot(dy);
            let point = if d == 0. {
                center
            } else {
                (center.0 + dx / d * 0.49, center.1 + dy / d * 0.49)
            };
            near |= (point.0 - target.0).hypot(point.1 - target.1) < radius
                && map.can_move(center, point);
        }
        assert!(near, "{name} proximity is inaccessible");
    }
    for id in [0x800, 0x801, 0x802, 0x803, 0x804, 0x805, 0x807, 0x70e] {
        assert!(map.objects().iter().any(|p| p.kind == id));
    }
    assert!(
        !map.objects().iter().any(|p| p.kind == 0x806),
        "loot bag is generated at runtime, not pre-awarded"
    );
}

#[test]
fn realm_map_collision_bounds_nonfinite_nowalk_and_sweep() {
    let (content, grounds) = assets();
    let map = tutorial(0, &content, Arc::clone(&grounds)).unwrap();
    assert!(map.tile(-1, 4).is_err());
    assert!(map.tile(32, 4).is_err());
    assert!(!map.can_move(map.spawn, (f32::NAN, 5.)));
    assert!(!map.can_move((f32::INFINITY, 5.), map.spawn));
    assert!(!map.can_move(map.spawn, (-0.1, 5.5)));
    assert!(!map.walkable(0, 0));
    assert!(map.walkable(24, 23));
    assert!(map.can_move((23.5, 23.5), (25.5, 23.5)));
    assert!(
        map.walkable(19, 7),
        "non-Static gun does not register a Wall occupant"
    );
    assert!(
        !map.can_move((12.5, 5.5), (14.5, 5.5)),
        "NoWalk boundary sweep"
    );
}

#[test]
fn realm_map_malformed_grounds_fail_explicitly() {
    for xml in [
        "<GroundTypes/>",
        "<Objects/>",
        "<GroundTypes><Ground type='0x100' id='Later'/></GroundTypes>",
        "<GroundTypes><Ground type='-1' id='Negative'/></GroundTypes>",
        "<GroundTypes><Ground type='1,2' id='Many'/></GroundTypes>",
        "<GroundTypes><Ground type='1' id='A'/><Ground type='1' id='B'/></GroundTypes>",
        "<GroundTypes><Ground type='1'/></GroundTypes>",
        "<GroundTypes><Ground type='1' id='A'><MinDamage>30</MinDamage></Ground></GroundTypes>",
        "<GroundTypes><Ground type='1' id='A'><MinDamage>60</MinDamage><MaxDamage>30</MaxDamage></Ground></GroundTypes>",
        "<GroundTypes><Ground type='1' id='A'><Speed>0.5</Speed></Ground></GroundTypes>",
        "<GroundTypes><Ground type='1' id='A'><SpeedModifier/></Ground></GroundTypes>",
    ] {
        assert!(Grounds::parse(xml).is_err(), "accepted {xml}");
    }
}

#[test]
fn realm_map_malformed_maps_fail_without_arena_substitution() {
    let (content, grounds) = assets();
    let fresh = || tutorial(42, &content, Arc::clone(&grounds)).unwrap();
    for case in 0..10 {
        let map = fresh();
        let mut tiles = map.tiles.to_vec();
        let mut objects = map.objects.to_vec();
        let mut spawn = map.spawn;
        match case {
            0 => {
                tiles.pop();
            }
            1 => {
                tiles[0].ground = 0x71;
            }
            2 => {
                spawn = (f32::NAN, 5.5);
            }
            3 => {
                spawn = (0.5, 0.5);
            }
            4 => {
                objects.push(Placement {
                    kind: 0x100,
                    x: 32,
                    y: 5,
                });
            }
            5 => {
                objects.push(objects[0]);
            }
            6 => {
                objects[0].kind = 0x7fff;
            }
            7 => {
                tiles[33].ground = 0x46;
            } // isolated safe cell behind walls
            8 => {
                tiles[5 * 32 + 4].ground = 0x70;
            }
            9 => {
                tiles[0].ground = 0x46;
            }
            _ => unreachable!(),
        }
        let error = Map::assemble(
            Kind::Tutorial,
            42,
            spawn,
            tiles,
            objects,
            &content,
            Arc::clone(&grounds),
        )
        .err()
        .expect("malformed map accepted");
        eprintln!("malformed case={case}: {error:#}");
    }
    let mut wrong = Grounds::load(&PathBuf::from(
        std::env::var("BUILD100_CLIENT_DIR").unwrap(),
    ))
    .unwrap();
    wrong.types.get_mut(&0x6b).unwrap().name = "Later ice".into();
    assert!(realm(42, &content, Arc::new(wrong)).is_err());
}

#[test]
fn realm_map_native_occupancy_registration_not_fulloccupy_metadata() {
    let (mut content, grounds) = assets();
    let map = tutorial(0, &content, grounds.clone()).unwrap();
    for (x, y) in [(24, 23), (8, 21), (19, 7)] {
        assert!(
            map.walkable(x, y),
            "non-Static object must not register a square blocker at {x},{y}"
        );
    }
    assert!(map.can_move((23.5, 23.5), (25.5, 23.5)));
    let xml = std::fs::read_to_string(
        PathBuf::from(std::env::var("BUILD100_CLIENT_DIR").unwrap()).join("objects.xml"),
    )
    .unwrap();
    content = Content::parse(&xml.replace("</Objects>", "<Object type='0x7f00' id='FullOccupy metadata'><Class>GameObject</Class><FullOccupy/><OccupySquare/></Object><Object type='0x7f01' id='Static FullOccupy metadata'><Class>GameObject</Class><Static/><FullOccupy/></Object></Objects>")).unwrap();
    let objects = vec![
        Placement {
            kind: 0x7f00,
            x: 5,
            y: 5,
        },
        Placement {
            kind: 0x107,
            x: 7,
            y: 5,
        },
        Placement {
            kind: 0x120,
            x: 9,
            y: 5,
        },
        Placement {
            kind: 0x7f01,
            x: 11,
            y: 5,
        },
    ];
    let map = Map::assemble(
        Kind::Tutorial,
        0,
        map.spawn,
        map.tiles.to_vec(),
        objects,
        &content,
        grounds,
    )
    .unwrap();
    assert!(!map.walkable(0, 0), "NoWalk remains blocked");
    assert!(
        map.walkable(5, 5),
        "FullOccupy has no independent registration effect"
    );
    assert!(
        map.walkable(11, 5),
        "Static FullOccupy without OccupySquare is not a movement blocker"
    );
    assert!(
        !map.walkable(7, 5),
        "supplied Static OccupySquare rock blocks its cell"
    );
    assert!(
        !map.can_move((6.5, 5.5), (8.5, 5.5)),
        "cannot sweep through a registered rock"
    );
    assert!(
        map.can_move((6.4, 5.5), (6.6, 5.5)),
        "non-Wall rock does not expand into neighbor half-cells"
    );
    assert!(!map.walkable(9, 5), "supplied Static Wall remains blocked");
    assert!(
        !map.can_move((8.4, 5.5), (8.6, 5.5)),
        "registered Wall half-cell edge"
    );
    assert!(
        !map.can_move((8.4, 6.6), (8.6, 6.4)),
        "registered Wall diagonal corner"
    );
}
