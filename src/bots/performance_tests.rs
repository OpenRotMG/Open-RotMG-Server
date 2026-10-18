use super::*;

#[test]
fn reachability_reuses_one_component_and_handles_a_blocked_origin() {
    let grid = Grid {
        walkable: HashSet::from([(0, 0), (1, 0), (3, 0), (4, 0)]),
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
fn obstacle_edits_reuse_terrain_frontiers_and_invalidate_only_connectivity() {
    let content = tests::content();
    let mut v = tests::view(&content);
    v.objects.clear();
    let mut brain = Brain::new(PROFILES[1], &v);
    let first = brain.navigation_grid(&v, &content, &[]);
    first.frontier_tiles();
    first.reachable(v.position);
    assert!(first.walkable.contains(&(2, 11)));
    brain.blocked.insert((2, 11), 100);
    let second = brain.navigation_grid(&v, &content, &[]);
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(first.walkable.contains(&(2, 11)));
    assert!(!second.walkable.contains(&(2, 11)));
    assert_eq!(
        first.frontier_candidates.get(),
        second.frontier_candidates.get()
    );
    assert!(second.reached.lock().unwrap().is_none());
    second.reachable(v.position);
    drop(first);
    drop(second);
    let address = Arc::as_ptr(brain.navigation.as_ref().unwrap());
    brain.blocked.remove(&(2, 11));
    brain.blocked.insert((3, 11), 100);
    let third = brain.navigation_grid(&v, &content, &[]);
    assert_eq!(Arc::as_ptr(&third), address);
    assert!(third.walkable.contains(&(2, 11)));
    assert!(!third.walkable.contains(&(3, 11)));
    assert!(third.reached.lock().unwrap().is_none());
    assert!(third.frontier_candidates.get().is_some());
    let rebuilds = brain.navigation_rebuilds;
    brain.blocked.insert((-999, -999), 100);
    assert!(Arc::ptr_eq(
        &third,
        &brain.navigation_grid(&v, &content, &[])
    ));
    assert_eq!(brain.navigation_rebuilds, rebuilds);
}

#[test]
fn targeted_reachability_matches_full_components_including_distant_detours_and_old_goals() {
    for gap in [Some(75), None] {
        let known: HashSet<_> = (0..81).flat_map(|x| (0..81).map(move |y| (x, y))).collect();
        let grid = Grid {
            walkable: known
                .iter()
                .copied()
                .filter(|&(x, y)| x != 40 || gap.is_some_and(|gap| y >= gap))
                .collect(),
            known,
            dimensions: (81, 81),
            ..Default::default()
        };
        for start in [(20, 40), (40, 40), (45, 40)] {
            let from = center(start);
            let full = grid.reachable(from);
            for extra in [None, Some((20, 79)), Some((79, 79)), Some((40, 40))] {
                let expected: BTreeSet<_> = full
                    .iter()
                    .copied()
                    .filter(|p| {
                        (i32::from(p.0) - i32::from(start.0)).abs() <= 24
                            && (i32::from(p.1) - i32::from(start.1)).abs() <= 24
                            || Some(*p) == extra
                    })
                    .collect();
                let actual = grid.nearby_reachable(from, extra);
                assert_eq!(
                    *actual, expected,
                    "start={start:?}, extra={extra:?}, gap={gap:?}"
                );
                assert!(Arc::ptr_eq(
                    &actual,
                    &grid.nearby_reachable(
                        Pos {
                            x: from.x + 0.2,
                            y: from.y + 0.2
                        },
                        extra
                    )
                ));
                assert!(actual.len() <= 49 * 49 + 1);
            }
        }
    }
}
#[test]
fn local_candidates_match_ordered_full_scan_at_fractional_edges() {
    let points: BTreeSet<_> = (0..500)
        .flat_map(|x| (0..500).map(move |y| (x, y)))
        .collect();
    let grid = Grid::default();
    for from in [
        Pos { x: 0.1, y: 0.9 },
        Pos { x: 24.99, y: 25.01 },
        Pos {
            x: 220.99,
            y: 200.99,
        },
    ] {
        let eligible = |p: TilePoint| {
            let d = center(p).distance(from);
            d > 3. && d < 24.
        };
        let expected: Vec<_> = points.iter().copied().filter(|p| eligible(*p)).collect();
        let before = grid.candidates.load(Ordering::Relaxed);
        assert_eq!(
            grid.nearby(&points, from)
                .filter(|p| eligible(*p))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(grid.candidates.load(Ordering::Relaxed) - before, 49 * 49);
    }
}
#[test]
fn frontier_candidates_preserve_full_scan_routes_and_ties() {
    for seed in 0..32 {
        let mut known: HashSet<_> = (0..41)
            .flat_map(|x| (0..37).map(move |y| (x, y)))
            .filter(|(x, y)| (x * 7 + y * 13 + seed) % 5 != 0)
            .collect();
        let origin = (10 + seed % 7, 12 + seed % 3);
        known.insert(origin);
        let grid = Grid {
            walkable: known
                .iter()
                .filter(|(x, y)| (x + y + seed) % 11 > 1)
                .copied()
                .collect(),
            known,
            dimensions: (44, 40),
            ..Default::default()
        };
        let from = center(origin);
        for target in [(40, 36), (2, 4), (21, 25), (39, 12), (64, 48)] {
            let target = center(target);
            let expected = if grid.walkable.contains(&tile(target)) {
                grid.next(from, target)
            } else {
                let reached = grid.reachable(from);
                reached
                    .iter()
                    .filter(|&&(x, y)| {
                        center((x, y)).distance(from) > 3.
                            && [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
                                .iter()
                                .any(|n| {
                                    n.0 >= 0
                                        && n.1 >= 0
                                        && i32::from(n.0) < 44
                                        && i32::from(n.1) < 40
                                        && !grid.known.contains(n)
                                })
                    })
                    .min_by(|a, b| {
                        center(**a)
                            .distance(target)
                            .total_cmp(&center(**b).distance(target))
                    })
                    .and_then(|p| grid.next(from, center(*p)))
            };
            assert_eq!(
                grid.towards(from, target).map(|p| (p.x, p.y)),
                expected.map(|p| (p.x, p.y)),
                "seed={seed}"
            );
        }
    }
}
#[test]
fn cache_inputs_preserve_visible_cover_failed_routes_and_hazard_geometry() {
    let content = tests::content();
    let mut v = tests::view(&content);
    let mut brain = Brain::new(PROFILES[1], &v);
    for step in 0..24 {
        v.position = Pos {
            x: 1.2 + step as f32 * 0.1,
            y: 8.8,
        };
        brain.blocked.insert((3, 10), 100);
        brain.blocked.insert(tile(v.position), 100);
        let mut wall = tests::object(999, 0x1ce, 2.5, 11.5);
        wall.blocks = true;
        v.objects = if step % 3 == 0 { vec![wall] } else { vec![] };
        let avoid = [Pos {
            x: 20.1 - step as f32 * 0.2,
            y: 19.99,
        }];
        if step % 4 == 0 {
            Arc::make_mut(&mut v.tiles)[1].ground_speed = if step % 8 == 0 { 0. } else { 1. };
        }
        let actual = brain.navigation_grid(&v, &content, &avoid);
        let mut expected = Grid::new(&v);
        expected.walkable.retain(|p| {
            (*p == tile(v.position) || (!brain.cover.contains(p) && !brain.blocked.contains_key(p)))
                && avoid.iter().all(|e| center(*p).distance(*e) >= 8.)
        });
        assert_eq!(actual.walkable, expected.walkable);
        assert_eq!(actual.known, expected.known);
        let repeated = brain.navigation_grid(&v, &content, &avoid);
        assert!(Arc::ptr_eq(&actual, &repeated));
    }
}

#[test]
#[ignore = "release-mode explored-terrain benchmark; no timing gate"]
fn large_explored_navigation_benchmark() {
    benchmark(false, 200);
}
#[test]
#[ignore = "release-mode explored-terrain cache-miss benchmark; no timing gate"]
fn uncached_explored_navigation_benchmark() {
    benchmark(true, 200);
}
#[test]
#[ignore = "release-mode aged-terrain benchmark; no timing gate"]
fn aged_explored_navigation_benchmark() {
    for side in [450, 500] {
        for misses in [false, true] {
            benchmark(misses, side);
        }
    }
}
fn benchmark(cache_misses: bool, side: i16) {
    use sha2::{Digest, Sha256};
    let content = tests::content();
    let mut v = tests::view(&content);
    v.map.width = i32::from(side) + 20;
    v.map.height = i32::from(side) + 20;
    v.tiles = (0..side)
        .flat_map(|y| {
            (0..side).map(move |x| crate::world::view::Tile {
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
