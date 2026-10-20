use crate::content::{Cell, Content, Map};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const SIDE: usize = 2048;
const CELLS: usize = SIDE * SIDE;
const OBJECTS: &str = "90ab4dbddadfc5adfa77f87746c376d6c611b7b3e02ec0ed8ab4f0ad91534b72";
const GROUNDS: &str = "436b596b062c141112790bb3702e4b25e639a5a27541f2a591c462cf9e1b15f7";

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "profile")]
pub(super) enum Recipe {
    #[serde(rename = "classic-v1")]
    ClassicV1(ClassicRecipe),
    #[serde(rename = "reviewed-cells-v1")]
    ReviewedCellsV1(ReviewedRecipe),
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClassicRecipe {
    schema: u8,
    id: String,
    width: usize,
    height: usize,
    orientation: Orientation,
    scenery_seed: u64,
    content: ContentIdentity,
    layers: Layers,
    output: String,
    provenance: Provenance,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewedRecipe {
    schema: u8,
    id: String,
    width: usize,
    height: usize,
    orientation: RowMajor,
    content: ContentIdentity,
    inputs: ReviewedInputs,
    output: String,
    provenance: ImageProvenance,
}
#[derive(Debug, Deserialize, Serialize)]
enum RowMajor {
    #[serde(rename = "row-major")]
    RowMajor,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewedInputs {
    dictionary: Layer,
    indices: Indices,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Indices {
    path: String,
    sha256: String,
    expanded_sha256: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ImageProvenance {
    kind: ImageKind,
    source_sha256: String,
    policy: Layer,
    annotations: Layer,
    status: String,
}
#[derive(Debug, Deserialize, Serialize)]
enum ImageKind {
    #[serde(rename = "image-guided-reconstruction-v1")]
    ImageGuidedReconstructionV1,
}
#[derive(Debug, Deserialize, Serialize)]
enum Orientation {
    #[serde(rename = "x-major")]
    XMajor,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ContentIdentity {
    objects: String,
    grounds: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Layers {
    elevation: Layer,
    moisture: Layer,
    overrides: Layer,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    path: String,
    sha256: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    source_url: String,
    source_sha256: String,
    exporter_url: String,
    exporter_sha256: String,
    emulator: String,
    mode: String,
    seed_variant: String,
    status: String,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    ensure!(!relative.is_empty(), "empty recipe path");
    let mut path = root.to_path_buf();
    for part in Path::new(relative).components() {
        let Component::Normal(name) = part else {
            bail!("unsafe recipe path {relative}");
        };
        path.push(name);
        match path.symlink_metadata() {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "symlink recipe path {relative}"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e).with_context(|| format!("inspect {}", path.display())),
        }
    }
    Ok(path)
}
pub(super) fn bounded_read(path: &Path, max: usize) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    ensure!(
        file.metadata()?.is_file(),
        "not a regular file: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= max,
        "file exceeds {max} bytes: {}",
        path.display()
    );
    Ok(bytes)
}
pub(super) fn valid_id(id: &str) -> bool {
    id.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}
impl Recipe {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::ClassicV1(r) => &r.id,
            Self::ReviewedCellsV1(r) => &r.id,
        }
    }
    pub(super) fn output(&self) -> &str {
        match self {
            Self::ClassicV1(r) => &r.output,
            Self::ReviewedCellsV1(r) => &r.output,
        }
    }
    pub(super) fn profile(&self) -> &'static str {
        match self {
            Self::ClassicV1(_) => "classic-v1",
            Self::ReviewedCellsV1(_) => "reviewed-cells-v1",
        }
    }
    fn status(&self) -> &str {
        match self {
            Self::ClassicV1(r) => &r.provenance.status,
            Self::ReviewedCellsV1(r) => &r.provenance.status,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::ClassicV1(r) => r.validate(),
            Self::ReviewedCellsV1(r) => {
                ensure!(
                    r.schema == 2,
                    "unsupported reviewed recipe schema {}",
                    r.schema
                );
                ensure!(valid_id(&r.id), "invalid Realm ID {}", r.id);
                ensure!(
                    r.width == SIDE && r.height == SIDE,
                    "expected 2048-square cells"
                );
                ensure!(
                    r.content.objects == OBJECTS && r.content.grounds == GROUNDS,
                    "unsupported content identity"
                );
                ensure!(
                    r.output == format!("{}.wmap", r.id),
                    "output must match Realm ID"
                );
                for hash in [
                    &r.inputs.dictionary.sha256,
                    &r.inputs.indices.sha256,
                    &r.inputs.indices.expanded_sha256,
                    &r.provenance.source_sha256,
                    &r.provenance.policy.sha256,
                    &r.provenance.annotations.sha256,
                ] {
                    ensure!(valid_hash(hash), "invalid reviewed source hash");
                }
                ensure!(
                    r.provenance
                        .status
                        .starts_with("Approximate image-guided reconstruction"),
                    "missing approximate reconstruction provenance"
                );
                Ok(())
            }
        }
    }
    fn inputs(&self) -> Vec<&str> {
        match self {
            Self::ClassicV1(r) => vec![
                &r.layers.elevation.path,
                &r.layers.moisture.path,
                &r.layers.overrides.path,
            ],
            Self::ReviewedCellsV1(r) => vec![
                &r.inputs.dictionary.path,
                &r.inputs.indices.path,
                &r.provenance.policy.path,
                &r.provenance.annotations.path,
            ],
        }
    }
}
impl ClassicRecipe {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1,
            "unsupported recipe schema {}",
            self.schema
        );
        ensure!(
            valid_id(&self.id),
            "invalid Realm ID {}: expected lowercase ASCII alphanumeric segments separated by hyphens",
            self.id
        );
        ensure!(
            self.width == SIDE && self.height == SIDE,
            "expected 2048-square layers"
        );
        ensure!(
            self.content.objects == OBJECTS && self.content.grounds == GROUNDS,
            "unsupported content identity"
        );
        ensure!(
            self.output == format!("{}.wmap", self.id),
            "output must match Realm ID"
        );
        let p = &self.provenance;
        ensure!(
            valid_hash(&p.source_sha256) && valid_hash(&p.exporter_sha256),
            "invalid provenance hash"
        );
        ensure!(
            p.source_url.starts_with("https://")
                && p.exporter_url.starts_with("https://")
                && !p.emulator.is_empty()
                && p.mode == "Perlin"
                && !p.seed_variant.is_empty()
                && !p.status.is_empty(),
            "incomplete export provenance"
        );
        Ok(())
    }
}
fn recipes(root: &Path) -> Result<BTreeMap<String, Recipe>> {
    let mut paths = std::fs::read_dir(root)
        .with_context(|| format!("discover recipes in {}", root.display()))?
        .map(|entry| Ok(entry?.path()))
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    let mut found = BTreeMap::new();
    for path in paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
    {
        let name = path
            .file_name()
            .context("recipe filename missing")?
            .to_str()
            .context("non-UTF8 recipe path")?;
        let recipe = read_recipe(root, name)?;
        ensure!(
            found.insert(recipe.id().to_owned(), recipe).is_none(),
            "duplicate Realm recipe ID"
        );
    }
    Ok(found)
}
pub(super) fn read_recipe(root: &Path, relative: &str) -> Result<Recipe> {
    let path = safe_path(root, relative)?;
    let recipe: Recipe = serde_json::from_slice(&bounded_read(&path, 64 * 1024)?)
        .with_context(|| format!("parse recipe {}", path.display()))?;
    recipe
        .validate()
        .with_context(|| format!("validate recipe {}", path.display()))?;
    let mut paths = BTreeSet::new();
    paths.insert(path.clone());
    for relative in std::iter::once(recipe.output()).chain(recipe.inputs()) {
        let input =
            safe_path(root, relative).with_context(|| format!("recipe {}", path.display()))?;
        ensure!(
            paths.insert(input),
            "aliased recipe/input/output path {relative}"
        );
    }
    Ok(recipe)
}
fn expand_layer(bytes: &[u8], count: usize, expected: &str) -> Result<Vec<u8>> {
    ensure!(
        count <= CELLS && valid_hash(expected),
        "invalid layer size/hash"
    );
    inflate(bytes, count, CELLS + 4096, expected)
}
fn inflate(bytes: &[u8], count: usize, compressed_bound: usize, expected: &str) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() <= compressed_bound,
        "compressed layer exceeds bound"
    );
    let mut decoder = flate2::Decompress::new(true);
    let mut raw = Vec::with_capacity(count + 1);
    let status = decoder
        .decompress_vec(bytes, &mut raw, flate2::FlushDecompress::Finish)
        .context("inflate layer")?;
    ensure!(
        status == flate2::Status::StreamEnd,
        "incomplete or oversized layer stream"
    );
    ensure!(
        raw.len() == count,
        "layer length: expected {count}, got {}",
        raw.len()
    );
    ensure!(
        decoder.total_in() as usize == bytes.len(),
        "trailing compressed layer data"
    );
    let actual = sha256(&raw);
    ensure!(
        actual == expected,
        "layer checksum: expected {expected}, got {actual}"
    );
    Ok(raw)
}
fn load_layer(root: &Path, layer: &Layer) -> Result<Vec<u8>> {
    let path = safe_path(root, &layer.path)?;
    let bytes = bounded_read(&path, CELLS + 4096)?;
    expand_layer(&bytes, CELLS, &layer.sha256).with_context(|| format!("layer {}", path.display()))
}
fn transpose(raw: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    ensure!(
        width > 0 && height > 0 && width <= SIDE && height <= SIDE && raw.len() == width * height,
        "invalid x-major dimensions/length"
    );
    Ok((0..height)
        .flat_map(|y| (0..width).map(move |x| raw[x * height + y]))
        .collect())
}

fn habitat(elevation: u8, moisture: u8) -> (u8, u8) {
    let wet = moisture as u16 * 7;
    if elevation >= 204 {
        (if wet > 5 * 255 { 0x6a } else { 0x60 }, 1)
    } else if elevation > 153 {
        if wet > 5 * 255 {
            (0x47, 4)
        } else if wet > 3 * 255 {
            (0x23, 3)
        } else {
            (0xbd, 2)
        }
    } else if elevation > 51 {
        if wet > 5 * 255 {
            (0x56, 7)
        } else if wet > 3 * 255 {
            (0x46, 6)
        } else {
            (0xbd, 5)
        }
    } else if wet > 5 * 255 {
        (0x57, 10)
    } else if wet > 4 * 255 {
        (0x56, 10)
    } else if wet > 3 * 255 {
        (0x48, 9)
    } else {
        (0xbd, 8)
    }
}
fn cell(elevation: u8, moisture: u8, override_byte: u8) -> Result<Cell> {
    ensure!(
        matches!(override_byte & 15, 0 | 1 | 3),
        "unknown polygon flags {override_byte:#x}"
    );
    let (ground, terrain) = match override_byte >> 4 {
        0 => habitat(elevation, moisture),
        // The generic demo's lava and ice are not authority for new gameplay hazards.
        1 | 2 => (0x73, 0),
        3 => (0x6a, 1),
        4 | 6 => (0xbc, 0),
        5 => (0xbc, 0),
        7 => (0x72, 0),
        8 => (0xbe, 11),
        9..=12 => (0xd0, 0),
        _ => bail!("unknown override {override_byte:#x}"),
    };
    Ok(Cell {
        ground,
        object: None,
        config: String::new(),
        terrain,
        region: 0,
    })
}
#[derive(Debug, Serialize)]
struct Mainland {
    removed_components: usize,
    removed_cells: usize,
}
fn neighbors(i: usize, width: usize) -> [usize; 4] {
    let x = i % width;
    [
        i.wrapping_sub(width),
        i + width,
        if x > 0 { i - 1 } else { usize::MAX },
        if x + 1 < width { i + 1 } else { usize::MAX },
    ]
}
fn normalize_mainland(cells: &mut [Cell], overrides: &[u8], width: usize) -> Result<Mainland> {
    let mut labels = vec![0usize; cells.len()];
    let mut counts = Vec::new();
    let mut queue = VecDeque::new();
    // Original ocean codes distinguish lakes from ocean even when their final grounds are identical.
    for start in 0..cells.len() {
        if labels[start] != 0 || overrides[start] >> 4 == 5 {
            continue;
        }
        let label = counts.len() + 1;
        let mut count = 0;
        labels[start] = label;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            count += 1;
            for next in neighbors(i, width) {
                if next < cells.len() && labels[next] == 0 && overrides[next] >> 4 != 5 {
                    labels[next] = label;
                    queue.push_back(next);
                }
            }
        }
        counts.push(count);
    }
    let largest = *counts.iter().max().context("no non-ocean mainland")?;
    ensure!(
        counts.iter().filter(|&&n| n == largest).count() == 1,
        "ambiguous largest mainland"
    );
    let mainland = counts
        .iter()
        .position(|&n| n == largest)
        .context("mainland label missing")?
        + 1;
    let mut removed_cells = 0;
    for (i, c) in cells.iter_mut().enumerate() {
        if labels[i] != 0 && labels[i] != mainland {
            *c = cell(0, 0, 0x50)?;
            removed_cells += 1;
        }
    }
    Ok(Mainland {
        removed_components: counts.len() - 1,
        removed_cells,
    })
}
fn assemble(
    e: &[u8],
    m: &[u8],
    o: &[u8],
    width: usize,
    height: usize,
    seed: u64,
) -> Result<(Map, Mainland)> {
    let e = transpose(e, width, height)?;
    let m = transpose(m, width, height)?;
    let o = transpose(o, width, height)?;
    let mut cells = (0..e.len())
        .map(|i| {
            cell(e[i], m[i], o[i]).with_context(|| format!("cell {},{}", i % width, i / width))
        })
        .collect::<Result<Vec<_>>>()?;
    let mainland = normalize_mainland(&mut cells, &o, width)?;
    let mut coast_distance = vec![u8::MAX; cells.len()];
    let mut queue = VecDeque::new();
    for (i, _) in cells.iter().enumerate().filter(|(_, c)| c.terrain == 11) {
        coast_distance[i] = 0;
        queue.push_back(i);
    }
    while let Some(i) = queue.pop_front() {
        let distance = coast_distance[i] + 1;
        if distance > 8 {
            continue;
        }
        for next in neighbors(i, width) {
            if next < cells.len() && coast_distance[next] > distance {
                coast_distance[next] = distance;
                queue.push_back(next);
            }
        }
    }
    for (i, c) in cells.iter_mut().enumerate() {
        if coast_distance[i] <= 8 && matches!(c.terrain, 9 | 10) && o[i] >> 4 == 0 {
            c.terrain = 12;
        }
        super::decorate(c, super::mix(seed ^ 0xbb67ae8584caa73b ^ i as u64))?;
    }
    Ok((
        Map {
            width: width as i32,
            height: height as i32,
            cells,
            spawns: vec![],
        },
        mainland,
    ))
}

type Entry = (u8, String, String, u8, u8);
fn encode(map: &Map, content: &Content) -> Result<(Vec<u8>, usize)> {
    ensure!(
        map.width > 0
            && map.height > 0
            && map.width <= SIDE as i32
            && map.height <= SIDE as i32
            && map.cells.len() == map.width as usize * map.height as usize,
        "invalid output dimensions/cells"
    );
    let mut entries = BTreeSet::new();
    let mut validated_objects = BTreeSet::new();
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground) && !content.hazards.contains_key(&c.ground),
            "unknown/hazardous output ground {:#x}",
            c.ground
        );
        ensure!(
            c.terrain <= 12 && c.region == 0,
            "unsupported output terrain/region"
        );
        let name = match c.object {
            Some(kind) => {
                let object = content.get(kind)?;
                ensure!(
                    object.static_object && !object.enemy,
                    "non-scenery output object {kind:#x}"
                );
                if validated_objects.insert(kind) {
                    ensure!(
                        content
                            .objects
                            .values()
                            .filter(|o| o.name == object.name)
                            .count()
                            == 1,
                        "ambiguous output object {}",
                        object.name
                    );
                }
                ensure!(!object.name.is_empty(), "empty output object name");
                if !c.config.is_empty() {
                    let size: u16 = c
                        .config
                        .strip_prefix("size:")
                        .context("unsupported scenery configuration")?
                        .parse()?;
                    ensure!(
                        matches!(kind, 0x100..=0x104 | 0x185..=0x18d | 0x197..=0x19f)
                            && (130..=160).contains(&size)
                            && size.is_multiple_of(5)
                            && c.config == format!("size:{size}"),
                        "unsupported scenery size/configuration"
                    );
                }
                object.name.clone()
            }
            None => {
                ensure!(c.config.is_empty(), "configuration without scenery");
                String::new()
            }
        };
        entries.insert((c.ground, name, c.config.clone(), c.terrain, c.region));
    }
    ensure!(
        !entries.is_empty() && entries.len() <= i16::MAX as usize,
        "output dictionary exceeds format limit"
    );
    let dict: BTreeMap<Entry, i16> = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| (e, i as i16))
        .collect();
    let mut raw = Vec::new();
    raw.extend((dict.len() as i16).to_le_bytes());
    for (ground, name, config, terrain, region) in dict.keys() {
        raw.extend((*ground as i16).to_le_bytes());
        for text in [name, config] {
            ensure!(text.len() <= 1024, "output dictionary string exceeds limit");
            let mut length = text.len();
            while length >= 128 {
                raw.push((length as u8 & 127) | 128);
                length >>= 7;
            }
            raw.push(length as u8);
            raw.extend(text.as_bytes());
        }
        raw.extend([*terrain, *region]);
    }
    raw.extend(map.width.to_le_bytes());
    raw.extend(map.height.to_le_bytes());
    for c in &map.cells {
        let name = match c.object {
            Some(kind) => content.get(kind)?.name.clone(),
            None => String::new(),
        };
        raw.extend(dict[&(c.ground, name, c.config.clone(), c.terrain, c.region)].to_le_bytes());
    }
    ensure!(
        raw.len() <= 16 * 1024 * 1024,
        "output exceeds expanded WMAP limit"
    );
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&raw)?;
    Ok((encoder.finish()?, dict.len()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedCell {
    ground: u8,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    object: Option<u16>,
    config: String,
    terrain: u8,
    region: u8,
}
fn dictionary(bytes: &[u8], content: &Content) -> Result<Vec<Cell>> {
    ensure!(
        bytes.len() <= 64 * 1024,
        "reviewed dictionary exceeds bound"
    );
    let entries: Vec<ReviewedCell> =
        serde_json::from_slice(bytes).context("parse reviewed dictionary")?;
    ensure!(
        !entries.is_empty() && entries.len() <= i16::MAX as usize,
        "invalid reviewed dictionary count"
    );
    let cells: Vec<_> = entries
        .into_iter()
        .map(|c| Cell {
            ground: c.ground,
            object: c.object,
            config: c.config,
            terrain: c.terrain,
            region: c.region,
        })
        .collect();
    // Unused entries must pass the same content/configuration guards as output cells.
    for chunk in cells.chunks(SIDE) {
        encode(
            &Map {
                width: chunk.len() as i32,
                height: 1,
                cells: chunk.to_vec(),
                spawns: vec![],
            },
            content,
        )
        .context("validate reviewed dictionary")?;
    }
    Ok(cells)
}
fn hashed_input(root: &Path, path: &str, expected: &str, bound: usize) -> Result<Vec<u8>> {
    let bytes = bounded_read(&safe_path(root, path)?, bound)?;
    let actual = sha256(&bytes);
    ensure!(
        actual == expected,
        "input {path} checksum: expected {expected}, got {actual}"
    );
    Ok(bytes)
}
fn audit(policy: &[u8], annotations: &[u8], provenance: &ImageProvenance) -> Result<()> {
    let policy: serde_json::Value =
        serde_json::from_slice(policy).context("parse source policy")?;
    let annotations: serde_json::Value =
        serde_json::from_slice(annotations).context("parse source annotations")?;
    ensure!(
        annotations["policy_sha256"] == provenance.policy.sha256
            && annotations["source_sha256"] == provenance.source_sha256
            && policy["schema"] == 1
            && policy["selected_client"] == "123.5.1"
            && policy["transform"].is_object()
            && annotations["transform"] == policy["transform"],
        "inconsistent source audit identity/transform"
    );
    let rows = annotations["unknown"]
        .as_array()
        .context("missing UNKNOWN decisions")?;
    ensure!(rows.len() == 101, "missing UNKNOWN decision");
    let mut pixels = 0u64;
    for (i, row) in rows.iter().enumerate() {
        ensure!(
            row["component"].as_u64() == Some(i as u64 + 1)
                && row["status"] == "REVIEWED_RECONSTRUCTION"
                && row["reason"].as_str().is_some_and(|s| !s.is_empty())
                && row["support_sha256"].as_str().is_some_and(valid_hash)
                && row["terrain"].as_u64().is_some_and(|t| t <= 12)
                && row["ground"].as_u64().is_some_and(|g| g <= 255),
            "unreviewed UNKNOWN component {}",
            i + 1
        );
        pixels += row["pixels"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= CELLS as u64)
            .context("invalid UNKNOWN support size")?;
    }
    ensure!(pixels == 20571, "inconsistent UNKNOWN support total");
    Ok(())
}
fn indexed_map(raw: &[u8], dictionary: &[Cell]) -> Result<Map> {
    ensure!(raw.len() == CELLS * 2, "invalid reviewed index length");
    let cells = raw
        .chunks_exact(2)
        .enumerate()
        .map(|(i, bytes)| {
            let index = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
            dictionary.get(index).cloned().with_context(|| {
                format!(
                    "UNKNOWN dictionary index {index} at {},{}",
                    i % SIDE,
                    i / SIDE
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Map {
        width: SIDE as i32,
        height: SIDE as i32,
        cells,
        spawns: vec![],
    })
}
fn reviewed_map(root: &Path, recipe: &ReviewedRecipe, content: &Content) -> Result<Map> {
    let inputs = &recipe.inputs;
    let cells = hashed_input(
        root,
        &inputs.dictionary.path,
        &inputs.dictionary.sha256,
        64 * 1024,
    )?;
    let dictionary = dictionary(&cells, content)?;
    let p = &recipe.provenance;
    let policy = hashed_input(root, &p.policy.path, &p.policy.sha256, 64 * 1024)?;
    let annotations = hashed_input(root, &p.annotations.path, &p.annotations.sha256, 64 * 1024)?;
    audit(&policy, &annotations, p)?;
    let indices = &inputs.indices;
    let compressed = hashed_input(root, &indices.path, &indices.sha256, 2 * CELLS + 4096)?;
    let raw = inflate(
        &compressed,
        2 * CELLS,
        2 * CELLS + 4096,
        &indices.expanded_sha256,
    )?;
    indexed_map(&raw, &dictionary)
}

pub(crate) fn run(args: &[String]) -> Result<bool> {
    if args.first().is_none_or(|a| a != "build-realm") {
        return Ok(false);
    }
    let mut data = None;
    let mut id = None;
    let mut check = false;
    let mut args = args[1..].iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" if data.is_none() => {
                data = Some(PathBuf::from(args.next().context("--data requires DIR")?))
            }
            "--id" if id.is_none() => id = Some(args.next().context("--id requires ID")?.as_str()),
            "--check" if !check => check = true,
            _ => bail!("unknown/duplicate build-realm argument {arg}"),
        }
    }
    let data = data.context("build-realm requires --data DIR")?;
    let id = id.context("build-realm requires --id ID")?;
    let started = std::time::Instant::now();
    let root = safe_path(&data, "realms")?;
    let recipes = recipes(&root)?;
    let recipe = recipes
        .get(id)
        .with_context(|| format!("unknown Realm recipe ID {id}"))?;
    for (name, expected) in [("objects.xml", OBJECTS), ("grounds.xml", GROUNDS)] {
        let actual = sha256(&bounded_read(&safe_path(&data, name)?, 16 * 1024 * 1024)?);
        ensure!(
            actual == expected,
            "unsupported content {name}: expected {expected}, got {actual}"
        );
    }
    let content = Content::load(&data).context("load selected-client content")?;
    let (map, mainland) = match recipe {
        Recipe::ClassicV1(r) => {
            let e = load_layer(&root, &r.layers.elevation)?;
            let m = load_layer(&root, &r.layers.moisture)?;
            let o = load_layer(&root, &r.layers.overrides)?;
            let (map, mainland) = assemble(&e, &m, &o, r.width, r.height, r.scenery_seed)?;
            (map, Some(mainland))
        }
        Recipe::ReviewedCellsV1(r) => (reviewed_map(&root, r, &content)?, None),
    };
    let prepared = started.elapsed();
    let (bytes, dictionary) = encode(&map, &content)?;
    let output = safe_path(&root, recipe.output())?;
    if check {
        ensure!(
            bounded_read(&output, 16 * 1024 * 1024)? == bytes,
            "rebuilt output differs: {}",
            output.display()
        );
    } else {
        std::fs::write(&output, &bytes).with_context(|| format!("write {}", output.display()))?;
    }
    let mut habitats = BTreeMap::<u8, usize>::new();
    for cell in &map.cells {
        *habitats.entry(cell.terrain).or_default() += 1;
    }
    println!(
        "{}",
        serde_json::json!({
            "id":id, "profile":recipe.profile(), "recipe_identity_sha256":sha256(&serde_json::to_vec(recipe)?),
            "status":if check {"checked"} else {"built"}, "output":output,
            "sha256":sha256(&bytes), "bytes":bytes.len(), "width":map.width, "height":map.height,
            "dictionary":dictionary, "habitats":habitats, "mainland":mainland,
            "scenery":map.cells.iter().filter(|c| c.object.is_some()).count(),
            "configured_scenery":map.cells.iter().filter(|c| !c.config.is_empty()).count(),
            "roads":map.cells.iter().filter(|c| c.ground == 0xd0).count(),
            "water":map.cells.iter().filter(|c| matches!(c.ground, 0xbc | 0x72 | 0x73)).count(),
            "prepare_ms":prepared.as_millis(), "total_ms":started.elapsed().as_millis(),
            "reconstruction":recipe.status(),
        })
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zipped(raw: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }
    fn recipe(value: serde_json::Value) -> Result<Recipe> {
        let recipe: Recipe = serde_json::from_value(value)?;
        recipe.validate()?;
        Ok(recipe)
    }

    fn reviewed_value() -> serde_json::Value {
        serde_json::from_str(include_str!("../../data/realms/world02.json")).unwrap()
    }

    #[test]
    fn realm_prepare_reviewed_strict_recipe_and_input_paths() {
        recipe(reviewed_value()).unwrap();
        for (field, value) in [
            ("profile", serde_json::json!("UNKNOWN")),
            ("schema", serde_json::json!(1)),
            ("id", serde_json::json!("../world2")),
            ("width", serde_json::json!(2047)),
            ("height", serde_json::json!(4096)),
            ("orientation", serde_json::json!("x-major")),
            ("output", serde_json::json!("world02.wmap")),
            ("scenery_seed", serde_json::json!(1)),
        ] {
            let mut v = reviewed_value();
            v[field] = value;
            assert!(recipe(v).is_err(), "{field}");
        }
        for path in [
            vec!["inputs"],
            vec!["inputs", "dictionary"],
            vec!["inputs", "indices"],
            vec!["content"],
            vec!["provenance"],
            vec!["provenance", "policy"],
            vec!["provenance", "annotations"],
        ] {
            let mut v = reviewed_value();
            let mut target = &mut v;
            for part in path {
                target = &mut target[part];
            }
            target["UNKNOWN"] = serde_json::json!(0);
            assert!(recipe(v).is_err());
        }
        for path in [
            vec!["content", "objects"],
            vec!["content", "grounds"],
            vec!["inputs", "dictionary", "sha256"],
            vec!["inputs", "indices", "expanded_sha256"],
            vec!["provenance", "source_sha256"],
            vec!["provenance", "kind"],
            vec!["provenance", "status"],
        ] {
            let mut v = reviewed_value();
            let mut target = &mut v;
            for part in path {
                target = &mut target[part];
            }
            *target = serde_json::json!("UNKNOWN");
            assert!(recipe(v).is_err());
        }
        let root = std::env::temp_dir().join(format!("reviewed-paths-{}", rand::random::<u64>()));
        std::fs::create_dir_all(root.join("world02")).unwrap();
        let write = |v: &serde_json::Value| {
            std::fs::write(root.join("world02.json"), serde_json::to_vec(v).unwrap()).unwrap()
        };
        write(&reviewed_value());
        // Metadata validation must work without outputs or canonical inputs present.
        read_recipe(&root, "world02.json").unwrap();
        for path in [
            "../escape",
            "/absolute",
            "world02.json",
            "classic-world-02-reconstruction.wmap",
            "world02/cells.indices.zlib",
            "world02/./cells.indices.zlib",
        ] {
            let mut v = reviewed_value();
            v["inputs"]["dictionary"]["path"] = serde_json::json!(path);
            write(&v);
            assert!(read_recipe(&root, "world02.json").is_err(), "{path}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("missing", root.join("world02/cells.dictionary.json"))
                .unwrap();
            write(&reviewed_value());
            assert!(read_recipe(&root, "world02.json").is_err());
        }
        assert!(hashed_input(&root, "missing", &"0".repeat(64), 64).is_err());
        std::fs::write(root.join("corrupt"), b"bad").unwrap();
        assert!(hashed_input(&root, "corrupt", &"0".repeat(64), 64).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn realm_prepare_reviewed_dictionary_requires_every_safe_field() {
        let content = Content::load(Path::new("data")).unwrap();
        let valid =
            serde_json::json!({"ground":190,"object":null,"config":"","terrain":11,"region":0});
        let parse = |v: serde_json::Value| dictionary(&serde_json::to_vec(&v).unwrap(), &content);
        parse(serde_json::json!([valid.clone(), valid.clone()])).unwrap();
        for field in ["ground", "object", "config", "terrain", "region"] {
            let mut v = valid.clone();
            v.as_object_mut().unwrap().remove(field);
            assert!(parse(serde_json::json!([v])).is_err(), "missing {field}");
        }
        let unknown_ground = (0..=u8::MAX)
            .find(|g| !content.grounds.contains_key(g))
            .unwrap();
        let hazardous = *content.hazards.keys().next().unwrap();
        let enemy = content
            .objects
            .values()
            .find(|o| o.static_object && o.enemy)
            .unwrap()
            .kind;
        let mobile = *content
            .objects
            .iter()
            .find(|(_, o)| !o.static_object)
            .unwrap()
            .0;
        for (field, value) in [
            ("UNKNOWN", serde_json::json!(0)),
            ("ground", serde_json::json!(unknown_ground)),
            ("ground", serde_json::json!(hazardous)),
            ("ground", serde_json::json!(256)),
            ("object", serde_json::json!(enemy)),
            ("object", serde_json::json!(mobile)),
            ("object", serde_json::json!(65535)),
            ("object", serde_json::json!("UNKNOWN")),
            ("config", serde_json::json!("size:150")),
            ("terrain", serde_json::json!(13)),
            ("region", serde_json::json!(1)),
        ] {
            let mut bad = valid.clone();
            bad[field] = value;
            // Even an unused extra entry must fail before indices are expanded.
            assert!(
                parse(serde_json::json!([valid.clone(), bad])).is_err(),
                "{field}"
            );
        }
        assert!(parse(serde_json::json!([])).is_err());
        assert!(dictionary(&vec![b' '; 65537], &content).is_err());
        assert!(dictionary(br#"[{"ground":190,"object":null,"object":null,"config":"","terrain":11,"region":0}]"#, &content).is_err());
    }

    #[test]
    fn realm_prepare_reviewed_indices_bounds_streams_and_hashes() {
        let raw = vec![0u8; 2 * CELLS];
        let hash = sha256(&raw);
        let bytes = zipped(&raw);
        assert_eq!(
            inflate(&bytes, 2 * CELLS, 2 * CELLS + 4096, &hash).unwrap(),
            raw
        );
        for invalid in [
            bytes[..bytes.len() - 1].to_vec(),
            [bytes.as_slice(), &[0]].concat(),
            [bytes.as_slice(), bytes.as_slice()].concat(),
            zipped(&raw[..raw.len() - 1]),
            zipped(&vec![0; raw.len() + 1]),
            vec![0; 2 * CELLS + 4097],
        ] {
            assert!(inflate(&invalid, 2 * CELLS, 2 * CELLS + 4096, &hash).is_err());
        }
        assert!(inflate(&bytes, 2 * CELLS, 2 * CELLS + 4096, &"0".repeat(64)).is_err());
        // Classic layers must not acquire the reviewed profile's larger bound.
        assert!(expand_layer(&bytes, 2 * CELLS, &hash).is_err());
        let mut raw = raw;
        raw[..2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(
            indexed_map(&raw, &[])
                .unwrap_err()
                .to_string()
                .contains("UNKNOWN")
        );
        assert!(indexed_map(&raw[..raw.len() - 1], &[]).is_err());
    }

    #[test]
    fn realm_prepare_reviewed_audit_rejects_unreviewed_or_inconsistent_source() {
        let Recipe::ReviewedCellsV1(r) = recipe(reviewed_value()).unwrap() else {
            panic!("reviewed fixture");
        };
        let policy = include_bytes!("../../data/realms/world02/source-policy.json");
        let annotations: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../data/realms/world02/source-annotations.json"
        ))
        .unwrap();
        audit(
            policy,
            &serde_json::to_vec(&annotations).unwrap(),
            &r.provenance,
        )
        .unwrap();
        for (field, value) in [
            ("policy_sha256", serde_json::json!("0".repeat(64))),
            ("source_sha256", serde_json::json!("0".repeat(64))),
            ("transform", serde_json::json!({})),
            ("unknown", serde_json::json!([])),
        ] {
            let mut v = annotations.clone();
            v[field] = value;
            assert!(audit(policy, &serde_json::to_vec(&v).unwrap(), &r.provenance).is_err());
        }
        for (field, value) in [
            ("status", serde_json::json!("UNKNOWN")),
            ("component", serde_json::json!(102)),
            ("support_sha256", serde_json::json!("UNKNOWN")),
            ("reason", serde_json::json!("")),
            ("pixels", serde_json::json!(0)),
            ("terrain", serde_json::json!(13)),
        ] {
            let mut v = annotations.clone();
            v["unknown"][0][field] = value;
            assert!(audit(policy, &serde_json::to_vec(&v).unwrap(), &r.provenance).is_err());
        }
    }

    #[test]
    fn realm_prepare_reviewed_world02_exact_two_encodes_and_all_fields() {
        let content = Content::load(Path::new("data")).unwrap();
        let root = Path::new("data/realms");
        let Recipe::ReviewedCellsV1(r) = read_recipe(root, "world02.json").unwrap() else {
            panic!("reviewed fixture");
        };
        let map = reviewed_map(root, &r, &content).unwrap();
        let (bytes, count) = encode(&map, &content).unwrap();
        assert_eq!(count, 55);
        assert_eq!(bytes.len(), 452498);
        assert_eq!(
            sha256(&bytes),
            "57ce63ef66783095ed6af1f461c838b7d9b457325040008038d16fe1ebcd5594"
        );
        assert_eq!(bytes, encode(&map, &content).unwrap().0);
        assert_eq!(bytes, std::fs::read(root.join(&r.output)).unwrap());
        let decoded = Map::decode_cells(&bytes, &content).unwrap();
        assert_eq!(decoded.cells.len(), CELLS);
        for (i, (a, b)) in decoded.cells.iter().zip(&map.cells).enumerate() {
            assert_eq!(
                (a.ground, a.object, &a.config, a.terrain, a.region),
                (b.ground, b.object, &b.config, b.terrain, b.region),
                "cell {i}"
            );
        }
        let source = super::super::realms::sources(Path::new("data"))
            .unwrap()
            .into_iter()
            .find(|s| s.name() == r.id)
            .unwrap();
        let arrived = source.open(&content).unwrap();
        assert_eq!(
            arrived.spawns,
            [
                (1024.5, 1676.5),
                (1942.5, 1024.5),
                (1024.5, 226.5),
                (93.5, 1024.5)
            ]
        );
        assert_eq!(
            map.cells
                .iter()
                .zip(&arrived.cells)
                .filter(
                    |(a, b)| (a.ground, a.object, &a.config, a.terrain, a.region)
                        != (b.ground, b.object, &b.config, b.terrain, b.region)
                )
                .count(),
            36
        );
    }
}
