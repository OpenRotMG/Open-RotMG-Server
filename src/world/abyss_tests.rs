use super::*;
use crate::dungeon::abyss::{self, FLOOR, IDOL, LAVA, MALPHAS};

#[test]
fn abyss_optional_rooms_restore_locally_without_main_completion() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let mut counts = BTreeSet::new();
    for seed in [0, 5, 19, 26] {
        let original = Arc::new(abyss::generate(&content, seed).unwrap());
        let mut w = World::new(content.clone(), original.clone()).unwrap();
        w.name = "Abyss of Demons".into();
        w.encounter = true;
        let boss = w.entities.values().find(|e| e.kind == MALPHAS).unwrap().id;
        let idols: Vec<_> = w
            .entities
            .values()
            .filter(|e| e.kind == IDOL)
            .map(|e| (e.id, e.home, w.map.cell(e.home.x, e.home.y).unwrap().region))
            .collect();
        counts.insert(idols.len());
        for (_, home, region) in &idols {
            let before = w.map.clone();
            assert!(
                before
                    .cells
                    .iter()
                    .any(|c| c.region == *region && c.ground == FLOOR)
            );
            w.idol_entrance(*home, false).unwrap();
            assert!(before.cells.iter().zip(&w.map.cells).all(|(a, b)| b.ground
                == if a.region == *region && a.ground == FLOOR {
                    LAVA
                } else {
                    a.ground
                }));
        }
        for (id, _, region) in &idols {
            let before = w.map.clone();
            w.defeat_entity(*id).unwrap();
            w.abyss_step();
            assert!(
                original
                    .cells
                    .iter()
                    .zip(&before.cells)
                    .zip(&w.map.cells)
                    .all(|((original, before), after)| after.ground
                        == if after.region == *region {
                            original.ground
                        } else {
                            before.ground
                        })
            );
            assert!(!w.entities.contains_key(id));
            assert!(w.entities.contains_key(&boss));
            assert!(!w.cleared);
            assert!(!w.entities.values().any(|e| e.kind == 0x704));
        }
    }
    assert_eq!(counts, BTreeSet::from([0, 1, 2, 3]));
}
