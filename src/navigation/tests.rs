use super::*;
use std::collections::BTreeSet;

#[test]
fn knowledge_pages_match_ordered_set_and_isolate_snapshots() {
    check_pages::<Arc<Words>>();
    check_pages::<Box<Words>>();
}
fn check_pages<P: Page>() {
    let mut mask = Bits::<P>::new((131, 129));
    let mut expected = BTreeSet::new();
    let mut state = 7u64;
    for step in 0..12000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let p = ((state % 131) as i16, ((state >> 16) % 129) as i16);
        if step % 3 == 0 {
            assert_eq!(mask.remove(&p), expected.remove(&p));
        } else {
            assert_eq!(mask.insert(p), expected.insert(p));
        }
        if step % 137 == 0 {
            assert_eq!(
                mask.points().collect::<Vec<_>>(),
                expected.iter().copied().collect::<Vec<_>>()
            );
            let saved = mask.clone();
            let old = saved.points().collect::<Vec<_>>();
            mask.insert((63, 64));
            expected.insert((63, 64));
            assert_eq!(saved.points().collect::<Vec<_>>(), old);
        }
        assert_eq!(mask.len(), expected.len());
        assert_eq!(mask.contains(&p), expected.contains(&p));
    }
    assert!(!mask.contains(&(-1, 0)));
    assert!(!mask.contains(&(131, 0)));
}

#[test]
fn hidden_geometry_other_viewers_and_old_snapshots_cannot_change_answers() {
    let speeds = BTreeMap::from([(1, 1.), (2, 0.)]);
    let a = Geometry::new((130, 130), vec![1; 130 * 130], &speeds);
    let mut hidden = vec![2; 130 * 130];
    for x in 0..65 {
        for y in 0..65 {
            hidden[y * 130 + x] = 1;
        }
    }
    let b = Geometry::new((130, 130), hidden, &speeds);
    let points: Vec<_> = (0..65).flat_map(|x| (0..65).map(move |y| (x, y))).collect();
    let mut left = Arc::new(Tiles::new(a.clone(), points.clone()));
    let right = Arc::new(Tiles::new(b, points));
    assert_eq!(
        serde_json::to_value(&left).unwrap(),
        serde_json::to_value(&right).unwrap()
    );
    let own = Walkable::new(left.clone());
    let other = Walkable::new(right);
    for x in 0..130 {
        for y in 0..130 {
            assert_eq!(own.contains(&(x, y)), other.contains(&(x, y)));
        }
    }
    let before = left.clone();
    Arc::make_mut(&mut left).discover(&a, [(65, 64)]);
    assert_ne!(left.version(), before.version());
    assert!(!before.contains(&(65, 64)));
    assert!(left.contains(&(65, 64)));
    assert!(Arc::ptr_eq(
        left.geometry.as_ref().unwrap(),
        before.geometry.as_ref().unwrap()
    ));
    let second_bot = Tiles::new(a, [(0, 0)]);
    assert_ne!(second_bot.version(), before.version());
    assert!(!second_bot.contains(&(64, 64)));
    let mut obstacles = Walkable::new(left.clone());
    assert!(obstacles.remove(&(64, 64)));
    assert!(!obstacles.remove(&(64, 64)));
    assert!(!obstacles.contains(&(64, 64)));
    assert!(Walkable::new(left).contains(&(64, 64)));
    obstacles.insert((64, 64));
    assert!(obstacles.contains(&(64, 64)));
    assert!(!obstacles.remove(&(129, 129)));
}
