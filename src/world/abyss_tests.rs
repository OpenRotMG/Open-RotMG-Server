use super::*;
use crate::dungeon::abyss::{self, FLOOR, IDOL, LAVA, MALPHAS};

#[test]
fn abyss_optional_rooms_restore_locally_without_main_completion() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let mut counts = BTreeSet::new();
    for seed in 0..32 {
        let map = abyss::generate(&content, seed).unwrap();
        let homes: Vec<_> = map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(IDOL))
            .map(|(i, c)| {
                (
                    Pos {
                        x: (i as i32 % map.width) as f32 + 0.5,
                        y: (i as i32 / map.width) as f32 + 0.5,
                    },
                    c.region,
                )
            })
            .collect();
        counts.insert(homes.len());
        println!("Abyss seed={seed} idols={}", homes.len());
        if homes.len() < 2 {
            continue;
        }
        let mut w = World::new(content.clone(), Arc::new(map)).unwrap();
        let boss = w.entities.values().find(|e| e.kind == MALPHAS).unwrap().id;
        let original = w.map.clone();
        for (home, region) in &homes {
            let entrance: Vec<_> = original
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| c.region == *region && c.ground == FLOOR)
                .map(|(i, _)| i)
                .collect();
            assert!(!entrance.is_empty());
            w.idol_entrance(*home, false).unwrap();
            assert!(entrance.iter().all(|i| w.map.cells[*i].ground == LAVA));
            assert!(
                original
                    .cells
                    .iter()
                    .zip(&w.map.cells)
                    .all(|(a, b)| a.region == *region || a.ground == b.ground)
            );
            w.idol_entrance(*home, true).unwrap();
            w.abyss_step();
            assert!(
                original
                    .cells
                    .iter()
                    .zip(&w.map.cells)
                    .all(|(a, b)| a.ground == b.ground)
            );
            assert!(w.entities.contains_key(&boss));
            assert!(!w.entities.values().any(|e| e.kind == 0x704));
        }
    }
    assert!(counts.contains(&0) && counts.contains(&1) && counts.iter().any(|n| *n >= 2));
}
