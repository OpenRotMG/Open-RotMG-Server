use super::*;
use std::io::Write;

#[test]
fn source_elevation_fsod_pinned_layers_row_major_zero_and_negative_contracts() -> Result<()> {
    let root = Path::new("data");
    let sources: Manifest = serde_json::from_slice(&std::fs::read(root.join(MANIFEST))?)?;
    let expected = [
        (
            "3178bd76de061dd536ec64c012de145338ed55dbb910f60f5bc78c4b514b3d0b",
            592,
            4,
            0,
        ),
        (
            "54ba4c8b58763e98d81928031f6aa7c9380ead5f138e2fa645f6716df9a0b507",
            838,
            4,
            0,
        ),
        (
            "4bcd687a18af01322f51c274b2eee498123643b8337c504861d69bfce0da3eee",
            879,
            3,
            7,
        ),
    ];
    let recovered: Vec<_> = sources
        .layers
        .iter()
        .filter(|l| matches!(l.provenance, Provenance::Recovered { .. }))
        .collect();
    assert_eq!(recovered.len(), 3);
    for (layer, (expected_hash, x, forward, transposed)) in recovered.into_iter().zip(expected) {
        let source = SourceElevation::load(root, &layer.map, &layer.map_sha256, [2048, 2048])?;
        let elevation = source.decode()?;
        assert_eq!(hash(&elevation), expected_hash);
        assert_eq!(elevation.len(), 2048 * 2048);
        assert_eq!(elevation[137 * 2048 + x], forward);
        assert_eq!(elevation[x * 2048 + 137], transposed);
        assert_eq!(elevation[0], 0, "source zero is not missing height");
        assert_eq!(
            elevation
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            256
        );
        assert_eq!(source.input_sha256.len(), 2);
        assert_eq!(
            source.retained_encoded_bytes(),
            std::fs::metadata(root.join(&layer.file))?.len() as usize
        );
        assert!(SourceElevation::load(root, &layer.map, &"0".repeat(64), [2048, 2048]).is_err());
        assert!(SourceElevation::load(root, &layer.map, &layer.map_sha256, [2048, 2047]).is_err());
    }
    assert!(
        SourceElevation::load(root, "authored-fixture.wmap", &"0".repeat(64), [2048, 2048])
            .is_err()
    );
    let directory = Path::new(&std::env::var("QA_RESULTS_DIR")?).join("source-elevation-contract");
    std::fs::create_dir(&directory)?;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(MANIFEST))?)?;
    let layer = &sources.layers[0];
    std::fs::copy(root.join(&layer.file), directory.join(&layer.file))?;
    let write = |value: &serde_json::Value| {
        std::fs::write(directory.join(MANIFEST), serde_json::to_vec(value).unwrap())
    };
    write(&manifest)?;
    let load = || SourceElevation::load(&directory, &layer.map, &layer.map_sha256, [2048, 2048]);
    load()?;
    manifest["schema"] = 1.into();
    write(&manifest)?;
    assert!(format!("{:#}", load().unwrap_err()).contains("unsupported source elevation contract"));
    manifest["schema"] = 2.into();
    manifest["layers"][0]["provenance"]["source_format"] = "authored-fixture".into();
    write(&manifest)?;
    assert!(format!("{:#}", load().unwrap_err()).contains("invalid source elevation provenance"));
    manifest["layers"][0]["provenance"]["source_format"] = "fsod-wmap-v2".into();
    manifest["layers"][0]["expanded_sha256"] = "0".repeat(64).into();
    write(&manifest)?;
    assert!(format!("{:#}", load().unwrap_err()).contains("layer checksum"));
    manifest["layers"][0]["expanded_sha256"] = layer.expanded_sha256.clone().into();
    write(&manifest)?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(directory.join(&layer.file))?
        .write_all(b"trailing")?;
    assert!(format!("{:#}", load().unwrap_err()).contains("source elevation checksum"));
    std::fs::remove_file(directory.join(&layer.file))?;
    assert!(format!("{:#}", load().unwrap_err()).contains(&layer.file));
    std::fs::remove_file(directory.join(MANIFEST))?;
    assert!(format!("{:#}", load().unwrap_err()).contains(MANIFEST));
    Ok(())
}

#[test]
fn authored_fixture_elevation_retains_computed_surface_not_production_source() -> Result<()> {
    let content = crate::content::Content::load(Path::new("data"))?;
    let map = crate::terrain::generate(&content, 12351, 192)?;
    let center = 96.;
    let radius = 192. * 0.44;
    let phase =
        (super::super::mix(12351) >> 11) as f64 / ((1u64 << 53) as f64) * std::f64::consts::TAU;
    let height = map
        .elevation
        .as_ref()
        .context("authored fixture height absent")?;
    assert_eq!(height.len(), map.cells.len());
    for y in 0..192 {
        for x in 0..192 {
            let (dx, dy) = (x as f64 + 0.5 - center, y as f64 + 0.5 - center);
            let angle = dy.atan2(dx);
            let coast = radius
                * (1. + 0.045 * (angle * 3. + phase).sin() + 0.025 * (angle * 5. - phase).cos());
            let unit = ((coast - dx.hypot(dy)) / radius).clamp(0., 1.);
            assert_eq!(
                map.elevation_at(x as f32 + 0.5, y as f32 + 0.5),
                Some((unit * 255.).round() as u8)
            );
        }
    }
    assert_eq!(map.elevation_at(0.5, 0.5), Some(0));
    assert!(map.elevation_at(96.5, 96.5).unwrap() > 200);
    Ok(())
}
