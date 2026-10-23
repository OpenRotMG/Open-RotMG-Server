use super::*;
use std::collections::BTreeSet;

#[test]
fn optional_treasure_rooms_are_reachable_and_have_complete_local_encounters() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let mut counts = BTreeSet::new();
    for seed in 0..128 {
        let map = generate(&content, seed).unwrap();
        let buttons: Vec<_> = map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(0xe27))
            .map(|(i, _)| i)
            .collect();
        assert!(buttons.len() <= 3, "seed {seed}: excessive treasure rooms");
        counts.insert(buttons.len());
        let spawners: Vec<_> = map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(0xe28))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            spawners.len(),
            buttons.len(),
            "seed {seed}: incomplete room"
        );
        assert!(
            !map.cells.iter().any(|c| c.object == Some(0xe26)),
            "seed {seed}: Guard present before switch activation"
        );
        assert_eq!(
            map.cells.iter().filter(|c| c.object == Some(0xe25)).count(),
            buttons.len() * 10
        );
        let seen = crate::dungeon::tests::reachable(&map, &content);
        for button in buttons {
            assert!(seen.contains(&button), "seed {seed}: unreachable button");
            let center = button + 12 * map.width as usize;
            assert!(
                spawners.contains(&center),
                "seed {seed}: missing local Guard spawner"
            );
            assert!(seen.contains(&center), "seed {seed}: unreachable Guard");
            for dy in -16i32..=16 {
                for dx in -7i32..=7 {
                    let cell = &map.cells[(center as i32 + dy * map.width + dx) as usize];
                    if dy.abs() == 7 && dx.abs() > 1 {
                        assert_eq!(cell.object, Some(0x170));
                    } else {
                        assert_eq!(cell.ground, 0xca);
                        assert!(
                            matches!(cell.object, None | Some(BUTTON | SPAWNER | DART)),
                            "seed {seed}: obstructed treasure floor at {dx},{dy}"
                        );
                    }
                }
            }
            for row in [-4, -2, 0, 2, 4] {
                for side in [-7, 7] {
                    let index = center as i32 + row * map.width + side;
                    assert_eq!(
                        map.cells[index as usize].object,
                        Some(0xe25),
                        "seed {seed}: missing inward dart source"
                    );
                }
            }
        }
    }
    assert_eq!(
        counts,
        BTreeSet::from([0, 1, 2, 3]),
        "treasure rooms never generated, or optional variation collapsed"
    );
}
