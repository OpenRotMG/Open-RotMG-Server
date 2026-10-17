use super::*;

#[test]
fn reachability_reuses_one_component_and_handles_a_blocked_origin() {
    let grid = Grid {
        walkable: BTreeSet::from([(0, 0), (1, 0), (3, 0), (4, 0)]),
        known: (0..5).map(|x| (x, 0)).collect(),
        dimensions: (5, 1),
        ..Grid::default()
    };
    let from_blocked = grid.reachable(center((2, 0)));
    assert_eq!(from_blocked.len(), 5);
    let left = grid.reachable(center((0, 0)));
    assert_eq!(*left, BTreeSet::from([(0, 0), (1, 0)]));
    assert!(Arc::ptr_eq(&left, &grid.reachable(center((1, 0)))));
    let right = grid.reachable(center((4, 0)));
    assert_eq!(*right, BTreeSet::from([(3, 0), (4, 0)]));
    assert_eq!(grid.reached.lock().unwrap().as_ref().unwrap().1.len(), 2);
    assert_eq!(grid.searches.load(Ordering::Relaxed), 3);
    assert_eq!(grid.cache_hits.load(Ordering::Relaxed), 1);
    for x in 0..5 {
        let expected = [(x - 1, 0), (x + 1, 0), (x, -1), (x, 1)]
            .iter()
            .any(|p| p.0 >= 0 && p.0 < 5 && p.1 == 0 && !grid.known.contains(p));
        assert_eq!(grid.frontier((x, 0), 5, 1), expected);
        assert_eq!(grid.frontier((x, 0), 5, 1), expected);
    }
    assert_eq!(grid.frontiers.lock().unwrap().len(), 5);
}

#[test]
fn navigation_reuse_invalidates_cover_and_knowledge_and_releases_the_old_cache() {
    let content = tests::content();
    let mut v = tests::view(&content);
    let mut brain = Brain::new(PROFILES[1], &v);
    let d = brain.decide(&v, &content).unwrap();
    let first = brain.navigation.as_ref().unwrap().clone();
    if let Some(p) = d.movement {
        v.position = p;
    }
    v.tick += 10;
    v.time_ms += 500;
    brain.decide(&v, &content).unwrap();
    assert!(Arc::ptr_eq(&first, brain.navigation.as_ref().unwrap()));
    let mut wall = tests::object(999, 0x1ce, v.position.x + 1., v.position.y);
    wall.blocks = true;
    v.objects.push(wall);
    v.tick += 10;
    v.time_ms += 500;
    brain.decide(&v, &content).unwrap();
    assert!(!Arc::ptr_eq(&first, brain.navigation.as_ref().unwrap()));
    let weak = Arc::downgrade(&first);
    drop(first);
    assert!(weak.upgrade().is_none());
    let covered = brain.navigation.as_ref().unwrap().clone();
    Arc::make_mut(&mut v.tiles).push(crate::world::view::Tile {
        x: 30,
        y: 0,
        ground: 0,
        ground_speed: 0.,
    });
    v.tick += 10;
    v.time_ms += 500;
    brain.decide(&v, &content).unwrap();
    assert!(!Arc::ptr_eq(&covered, brain.navigation.as_ref().unwrap()));
    assert!(brain.navigation.as_ref().unwrap().known.contains(&(30, 0)));
}

#[test]
#[ignore = "release-mode explored-terrain benchmark; no timing gate"]
fn large_explored_navigation_benchmark() {
    benchmark(false);
}
#[test]
#[ignore = "release-mode explored-terrain cache-miss benchmark; no timing gate"]
fn uncached_explored_navigation_benchmark() {
    benchmark(true);
}
fn benchmark(cache_misses: bool) {
    use sha2::{Digest, Sha256};
    let content = tests::content();
    let mut v = tests::view(&content);
    v.map.width = 220;
    v.map.height = 220;
    v.tiles = (0..200)
        .flat_map(|y| {
            (0..200).map(move |x| crate::world::view::Tile {
                x,
                y,
                ground: 0,
                ground_speed: 1.,
            })
        })
        .collect::<Vec<_>>()
        .into();
    let mut brain = Brain::new(PROFILES[1], &v);
    let mut trace = String::new();
    let start = std::time::Instant::now();
    for _ in 0..24 {
        if cache_misses {
            brain.navigation = None;
        }
        v.tick += 10;
        v.time_ms += 500;
        let d = brain.decide(&v, &content).unwrap();
        trace.push_str(&format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}",
            d.goal, d.movement, d.aim, d.item, d.journey, d.chat
        ));
        brain.goal = d.goal;
        brain.destination = d.movement;
        if let Some(p) = d.movement {
            v.position = p;
        }
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(trace.as_bytes())),
        "a4b61bfdc11e824e23a6bcc0a4589ffe4094ab0830ffcfe7467784847acc17ff"
    );
    println!(
        "navigation_benchmark cache_misses={} tiles={} decisions=24 elapsed_us={} decision_sha256={:x}",
        cache_misses,
        v.tiles.len(),
        start.elapsed().as_micros(),
        Sha256::digest(trace.as_bytes())
    );
}
