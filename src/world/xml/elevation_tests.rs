use super::*;
use rand::RngCore;
use sha2::{Digest, Sha256};

#[test]
fn realm_elevation_thresholds_source_zero_and_absence_before_mutation() {
    for threshold in [200u8, 150, 145, 140, 130, 120, 0] {
        let (mut w, _, id) = fixture(&format!(
            "<Behavior minAltitude=\"{threshold}\">StayAbove</Behavior>"
        ));
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        for height in [threshold, threshold + 1] {
            Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap()).fill(height);
            assert!(
                !w.ready(Service::Movement, &b, step(&w, id, b.local))
                    .unwrap()
            );
        }
        if threshold > 0 {
            let heights = Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap());
            heights.fill(threshold - 1);
            heights[64 * 128 + 65] = threshold;
            assert!(
                w.ready(Service::Movement, &b, step(&w, id, b.local))
                    .unwrap()
            );
            assert_eq!(
                w.xml_environment_goal(&b, w.entities[&id].pos, 0x600, scope(&w, id))
                    .unwrap()
                    .unwrap()
                    .x,
                65.5
            );
        }
        Arc::make_mut(&mut w.map).elevation = None;
        let ids = w.ids.load(Ordering::Relaxed);
        let rng = w.rng.clone().next_u64();
        let error = format!(
            "{:#}",
            w.ready(Service::Movement, &b, step(&w, id, b.local))
                .unwrap_err()
        );
        assert!(
            error.contains("missing source elevation capability")
                && error.contains("object=Pirate")
                && error.contains("rules.xml:1"),
            "{error}"
        );
        assert!(
            w.spawn_entity(0x600, Pos { x: 65.5, y: 64.5 }, vec![])
                .is_err()
        );
        assert_eq!(w.ids.load(Ordering::Relaxed), ids);
        assert_eq!(w.rng.clone().next_u64(), rng);
        assert!(w.xml.motions.is_empty());
    }
}

#[test]
fn realm_elevation_actual_sources_compiled_native_directory_and_reconstructed_layers() -> Result<()>
{
    use crate::content::game::geometry::Geometry;
    let root = Path::new("data");
    let content = Arc::new(crate::world::test_content());
    let game = content.game.as_ref().unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("realm-elevations.json"))?)?;
    let mut observed = Vec::new();
    let sources = crate::terrain::realms::sources(root)?;
    assert_eq!(sources.len(), 16);
    let retained: usize = game
        .maps
        .iter()
        .map(|m| match &m.geometry {
            Geometry::Encoded(a) => a.retained_encoded_bytes(),
            Geometry::Reconstruction(r) => r.retained_encoded_bytes(),
            _ => 0,
        })
        .sum();
    assert!(retained < 16 * 1024 * 1024);
    for source in sources {
        let name = source.name().to_owned();
        if name.starts_with("realm-fsod-") {
            let expected = manifest["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["map"] == format!("{name}.wmap"))
                .unwrap();
            let binding = game.maps.iter().find(|m| m.name == name).unwrap();
            let Geometry::Encoded(encoded) = &binding.geometry else {
                panic!("FSoD must use linked encoded map")
            };
            let linked = encoded.decode(&content)?;
            let linked_hash = format!("{:x}", Sha256::digest(linked.elevation.as_ref().unwrap()));
            assert_eq!(linked_hash, expected["expanded_sha256"]);
            drop(linked);
            let native = World::with_ids_context(
                content.clone(),
                source.open(&content)?,
                Arc::new(AtomicI32::new(1000)),
                Some(WorldDomain::Realm),
            )?;
            let hash = format!(
                "{:x}",
                Sha256::digest(
                    native
                        .map
                        .elevation
                        .as_ref()
                        .context("native FSoD layer lost")?
                )
            );
            assert_eq!(
                hash, expected["expanded_sha256"],
                "height changed through native map admission/arrivals"
            );
            drop(native);
            let result =
                Directory::configured(content.clone(), 16, 12351, Some(12351), vec![source], 1);
            match result {
                Ok(directory) => {
                    let world = &directory.worlds[&-3];
                    assert_eq!(world.realm_layout.as_deref(), Some(name.as_str()));
                    assert_eq!(world.xml.domain, Some(WorldDomain::Realm));
                    assert_eq!(format!("{:x}", Sha256::digest(world.map.elevation.as_ref().unwrap())), hash);
                    // reserve_realm_events (base HEAD world.rs:1412) moves placed event roots out of `entities`.
                    let cube = world.event_reserves.iter().find(|r| r.root.kind == 0xd59).context("FSoD Cube God root not reserved")?;
                    assert!(world.map.elevation_at(cube.root.pos.x, cube.root.pos.y).is_some());
                    observed.push(serde_json::json!({"layout":name,"constructed":true,"height_sha256":hash,"entities":world.entities.len(),"native_map_admission":true}));
                }
                Err(error) => observed.push(serde_json::json!({"layout":name,"constructed":false,"height_sha256":hash,"native_map_admission":true,"error":format!("{error:#}")})),
            }
        } else {
            let binding = game.maps.iter().find(|m| m.name == name).unwrap();
            let Geometry::Reconstruction(recipe) = &binding.geometry else {
                panic!("reviewed source must remain reconstruction")
            };
            let layer = manifest["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| {
                    l["map"]
                        .as_str()
                        .unwrap()
                        .ends_with(&format!("{name}.wmap"))
                })
                .unwrap();
            assert_eq!(layer["provenance"]["kind"], "reconstructed");
            let heights: Vec<u8> =
                serde_json::from_value(layer["provenance"]["terrain_heights"].clone())?;
            let linked = recipe.decode(&content)?;
            let native = source.open(&content)?;
            let elevation = native
                .elevation
                .as_ref()
                .context("reconstructed layer lost")?;
            assert_eq!(linked.elevation.as_deref(), Some(&elevation[..]));
            let hash = format!("{:x}", Sha256::digest(elevation));
            assert_eq!(hash, layer["expanded_sha256"]);
            assert!(
                native
                    .cells
                    .iter()
                    .zip(elevation.iter())
                    .all(|(cell, height)| heights[cell.terrain as usize] == *height),
                "{name}: authored height must follow the terrain band table"
            );
            drop((linked, native));
            let directory =
                Directory::configured(content.clone(), 16, 12351, Some(12351), vec![source], 1)?;
            let world = &directory.worlds[&-3];
            let cube = world
                .event_reserves
                .iter()
                .find(|r| r.root.kind == 0xd59)
                .context("Cube God root not reserved")?;
            assert!(
                world
                    .map
                    .elevation_at(cube.root.pos.x, cube.root.pos.y)
                    .is_some()
            );
            observed.push(serde_json::json!({"layout":name,"constructed":true,"height_sha256":hash,"entities":world.entities.len(),"provenance":"reconstructed"}));
        }
    }
    std::fs::write(
        Path::new(&std::env::var("QA_RESULTS_DIR")?).join("realm-elevation-construction.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"qualification":"actual one-seed Directory construction for all unchanged16sources; no native-rendered or historical acceptance","seed":12351,"retained_compiled_geometry_bytes":retained,"maps":observed}),
        )?,
    )?;
    assert_eq!(
        observed.iter().filter(|r| r["constructed"] == true).count(),
        16,
        "{observed:?}"
    );
    Ok(())
}

#[test]
fn realm_elevation_reconstructed_bands_select_fsod_altitude_tiers() -> Result<()> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read("data/realm-elevations.json")?)?;
    let tables: BTreeSet<Vec<u8>> = manifest["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["provenance"]["kind"] == "reconstructed")
        .map(|l| serde_json::from_value(l["provenance"]["terrain_heights"].clone()).unwrap())
        .collect();
    // scripts/reconstruct-realm-elevations.py: lower median of realm-fsod-1..3 heights per terrain byte.
    assert_eq!(
        tables,
        BTreeSet::from([vec![0, 254, 169, 175, 176, 69, 85, 92, 17, 26, 27, 3, 7]])
    );
    let heights = tables.first().unwrap();
    let bands = |threshold: u8| {
        (0..13)
            .filter(|band| heights[*band] >= threshold)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bands(200),
        [1],
        "compiled StayAbove default keeps to Mountains"
    );
    assert_eq!(bands(150), [1, 2, 3, 4]);
    assert_eq!(bands(93), [1, 2, 3, 4]);
    assert_eq!(bands(92), [1, 2, 3, 4, 7]);
    assert_eq!(bands(70), [1, 2, 3, 4, 6, 7]);
    assert_eq!(bands(28), [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(bands(11), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    assert_eq!(bands(1), (1..13).collect::<Vec<_>>());
    Ok(())
}
