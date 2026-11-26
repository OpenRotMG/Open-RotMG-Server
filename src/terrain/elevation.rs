use super::prepare::{bounded_read, inflate, safe_path, valid_hash};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(crate) const MANIFEST: &str = "realm-elevations.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u8,
    orientation: String,
    units: String,
    layers: Vec<Layer>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    map: String,
    map_sha256: String,
    dimensions: [usize; 2],
    provenance: Provenance,
    file: String,
    sha256: String,
    expanded_sha256: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Provenance {
    Recovered {
        source_format: String,
        source_url: String,
        source_sha256: String,
        source_git_blob: String,
    },
    /// Authored from the map's terrain bands; never presented as original per-cell height.
    Reconstructed {
        method: String,
        calibration: Vec<String>,
        terrain_heights: [u8; 13],
    },
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SourceElevation {
    layer: Layer,
    pub(crate) input_sha256: BTreeMap<String, String>,
    #[serde(skip)]
    bytes: Arc<[u8]>,
}
fn cells([width, height]: [usize; 2]) -> usize {
    width * height
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl SourceElevation {
    pub(crate) fn load(
        root: &Path,
        map: &str,
        map_sha256: &str,
        dimensions: [usize; 2],
    ) -> Result<Self> {
        let result = (|| {
            let raw = bounded_read(&safe_path(root, MANIFEST)?, 64 * 1024)?;
            let manifest: Manifest = serde_json::from_slice(&raw)?;
            ensure!(
                manifest.schema == 2
                    && manifest.orientation == "row-major"
                    && manifest.units == "u8-0-255",
                "unsupported source elevation contract"
            );
            let recovered: Vec<_> = manifest
                .layers
                .iter()
                .filter(|l| matches!(l.provenance, Provenance::Recovered { .. }))
                .map(|l| l.file.clone())
                .collect();
            let mut layers = manifest.layers.into_iter().filter(|l| l.map == map);
            let layer = layers.next().context("missing source elevation binding")?;
            ensure!(
                layers.next().is_none(),
                "duplicate source elevation binding"
            );
            ensure!(
                layer.map_sha256 == map_sha256
                    && valid_hash(map_sha256)
                    && layer.dimensions == dimensions,
                "source elevation map hash/dimension mismatch"
            );
            ensure!(
                valid_hash(&layer.sha256)
                    && valid_hash(&layer.expanded_sha256)
                    && match &layer.provenance {
                        Provenance::Recovered {
                            source_format,
                            source_url,
                            source_sha256,
                            source_git_blob,
                        } => {
                            source_format == "fsod-wmap-v2"
                                && valid_hash(source_sha256)
                                && source_git_blob.len() == 40
                                && source_git_blob.bytes().all(|b| b.is_ascii_hexdigit())
                                && source_url.starts_with("https://")
                        }
                        Provenance::Reconstructed {
                            method,
                            calibration,
                            ..
                        } => {
                            !method.is_empty()
                                && !calibration.is_empty()
                                && calibration.iter().all(|file| recovered.contains(file))
                        }
                    },
                "invalid source elevation provenance/checksum"
            );
            let bytes = bounded_read(&safe_path(root, &layer.file)?, cells(dimensions) + 4096)?;
            ensure!(
                hash(&bytes) == layer.sha256,
                "source elevation checksum mismatch"
            );
            let inputs = BTreeMap::from([
                (MANIFEST.to_owned(), hash(&raw)),
                (layer.file.clone(), layer.sha256.clone()),
            ]);
            let source = Self {
                layer,
                input_sha256: inputs,
                bytes: bytes.into(),
            };
            source.decode()?;
            Ok(source)
        })();
        result.with_context(|| format!("{MANIFEST} map={map} source elevation"))
    }
    pub(crate) fn decode(&self) -> Result<Arc<[u8]>> {
        let cells = cells(self.layer.dimensions);
        inflate(
            &self.bytes,
            cells,
            cells + 4096,
            &self.layer.expanded_sha256,
        )
        .map(Arc::from)
        .with_context(|| format!("{} map={} elevation", self.layer.file, self.layer.map))
    }
}
