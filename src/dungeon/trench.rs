use crate::content::{Cell, Content, Map};
use anyhow::{Result, ensure};
use rand::{Rng, SeedableRng};

pub const HERMIT: u16 = 0xd61;
pub const THESSAL: u16 = 0x1706;
pub const VENT: u16 = 0x731;

pub(super) fn generate_for(
    content: &Content,
    generator: &crate::content::game::geometry::Generator,
    seed: u64,
) -> Result<Map> {
    let [coral, vent, wall, boss, retreat] = generator.placements("cavern")?;
    let packs = generator.placements::<12>("chamber-packs")?;
    let [left_guard, right_guard] = generator.placements("chamber-guards")?;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut map = Map {
        elevation: None,
        width: 110,
        height: 100,
        cells: vec![
            Cell {
                ground: 0xff,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            11000
        ],
        spawns: vec![(12.5, 12.5)],
    };
    // The original procedural generator is unavailable; reconstruct its connected underwater caverns.
    let rooms: [(i32, i32); 13] = [
        (12, 12),
        (32, 12),
        (52, 12),
        (72, 12),
        (92, 12),
        (92, 32),
        (72, 32),
        (52, 32),
        (32, 32),
        (12, 32),
        (12, 52),
        (32, 52),
        (64, 76),
    ];
    let index = |x: i32, y: i32| (y * 110 + x) as usize;
    for (i, &(cx, cy)) in rooms.iter().enumerate() {
        let radius = if i == 12 { 17 } else { rng.gen_range(5..=8) };
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                if (x - cx).pow(2) + (y - cy).pow(2) <= radius * radius {
                    map.cells[index(x, y)].ground = 0x2e;
                }
            }
        }
        // Build 122.0.1 shows coral; this offset remains reconstructed: https://steamcommunity.com/sharedfiles/filedetails/?id=23486568
        if i != 12 {
            map.cells[index(cx - 4, cy)].object = Some(coral);
        }
    }
    for pair in rooms.windows(2) {
        let (mut x, mut y) = pair[0];
        let (tx, ty) = pair[1];
        let horizontal = rng.gen_bool(0.5);
        let mut steps = 0;
        loop {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    map.cells[index(x + dx, y + dy)].ground = 0x2e;
                }
            }
            if steps % 6 == 0 {
                map.cells[index(x, y)].object = Some(vent);
            }
            if (x, y) == (tx, ty) {
                break;
            }
            if x != tx && (horizontal || y == ty) {
                x += (tx - x).signum();
            } else {
                y += (ty - y).signum();
            }
            steps += 1;
        }
    }
    for (i, &(x, y)) in rooms.iter().enumerate().skip(1).take(11) {
        let pack = &packs[i % 4 * 3..][..3];
        for (j, kind) in pack.iter().enumerate() {
            map.cells[index(x - 2 + j as i32 * 2, y + 2)].object = Some(*kind);
        }
        map.cells[index(x, y)].object = Some(vent);
        for &(dx, dy, kind) in &[(-3, -2, left_guard), (3, -2, right_guard)] {
            map.cells[index(x + dx, y + dy)].object = Some(kind);
        }
    }
    for y in 1..99 {
        for x in 1..109 {
            if map.cells[index(x, y)].ground == 0xff
                && [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                    .iter()
                    .any(|&(x, y)| map.cells[index(x, y)].ground == 0x2e)
            {
                map.cells[index(x, y)].object = Some(wall);
            }
        }
    }
    for (dx, dy) in [(14, 0), (-14, 0), (0, 14), (0, -14)] {
        map.cells[index(64 + dx, 76 + dy)].object = Some(vent);
    }
    map.cells[index(64, 76)].object = Some(boss);
    // FSoD oceantrench.jm fills the Spawn region with 21 vents: a 5x5 block without its corners.
    for dy in -2..=2i32 {
        for dx in -2..=2i32 {
            if dx.abs() + dy.abs() < 4 {
                map.cells[index(12 + dx, 12 + dy)].object = Some(vent);
            }
        }
    }
    map.cells[index(12, 9)].object = Some(retreat);
    map.cells[index(12, 12)].region = 1;
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Ocean Trench ground"
        );
        if let Some(kind) = c.object {
            content.get(kind)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        "generated reconstructed Ocean Trench layout"
    );
    Ok(map)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    #[test]
    fn coral_dressing_preserves_trench_layout_and_encounters() {
        let content = crate::world::test_content();
        let coral = content.get(0x1711).unwrap();
        assert!(coral.static_object);
        assert!(!coral.blocks && !coral.blocks_sight && !coral.full_occupy);
        assert!(!coral.occupy_square && !coral.enemy_occupy_square);
        let mut baseline = Sha256::new();
        let mut counts = Vec::new();
        for seed in (0..64).chain([12351, u64::MAX]) {
            let mut map =
                crate::dungeon::generate_selected(&content, "ocean-trench", seed).unwrap();
            let mut count = 0;
            for cell in &mut map.cells {
                if cell.object == Some(0x1711) {
                    assert_eq!(cell.ground, 0x2e, "coral outside OceanFloor, seed {seed}");
                    cell.object = None;
                    count += 1;
                }
            }
            counts.push((seed, count));
            baseline.update(serde_json::to_vec(&map).unwrap());
        }
        let fingerprint = format!("{:x}", baseline.finalize());
        for (seed, count) in counts {
            assert_eq!(count, 12, "one coral per non-boss room, seed {seed}");
        }
        // A scenery fix must not reshuffle geometry, vents or encounter spawns.
        assert_eq!(
            fingerprint,
            "71a9a55baf027e38e8ccc1346c37eff73b4710b87b62daa9d62db51f0fbd4261"
        );
    }

    #[test]
    fn spawn_room_holds_the_reference_vent_cluster() {
        let content = crate::world::test_content();
        for seed in (0..16).chain([12351, u64::MAX]) {
            let map = crate::dungeon::generate_selected(&content, "ocean-trench", seed).unwrap();
            let vents = (10..=14)
                .flat_map(|y| (10..=14).map(move |x| (x, y)))
                .filter(|&(x, y)| map.cells[y * 110 + x].object == Some(super::VENT))
                .count();
            assert!(vents >= 21, "{vents} vents at spawn, seed {seed}");
        }
    }
}

#[cfg(test)]
#[test]
fn xml_cutover_trench_chamber_packs_read_linked_geometry_objects() {
    use std::{path::Path, sync::Arc};
    let source = crate::world::test_content();
    let seed = 12351;
    let baseline = crate::dungeon::generate_selected(&source, "ocean-trench", seed).unwrap();
    let data = Path::new("data");
    let mut inputs = source
        .game
        .as_ref()
        .unwrap()
        .inputs
        .keys()
        .map(|file| (file.clone(), std::fs::read(data.join(file)).unwrap()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let geometry = inputs.get_mut("game/123.5.1/geometry.xml").unwrap();
    let xml = std::str::from_utf8(geometry).unwrap();
    let prefix = "role=\"chamber-packs\" objects=\"";
    let expected = format!("{prefix}Fishman Warrior,");
    assert_eq!(xml.matches(&expected).count(), 1);
    *geometry = xml
        .replacen(&expected, &format!("{prefix}Fishman,"), 1)
        .into_bytes();
    let mut changed = source;
    changed.game = Some(Arc::new(
        crate::content::game::compile_fixture_at(&changed, inputs, data).unwrap(),
    ));
    let actual = crate::dungeon::generate_selected(&changed, "ocean-trench", seed).unwrap();
    assert_eq!(
        (actual.width, actual.height, &actual.spawns),
        (baseline.width, baseline.height, &baseline.spawns)
    );
    let mut replaced = 0;
    for (before, after) in baseline.cells.iter().zip(&actual.cells) {
        assert_eq!(
            (before.ground, before.region, &before.config),
            (after.ground, after.region, &after.config)
        );
        if before.object == Some(0x1700) {
            assert_eq!(after.object, Some(0x1708));
            replaced += 1;
        } else {
            assert_eq!(after.object, before.object);
        }
    }
    assert_eq!(replaced, 2);
}
