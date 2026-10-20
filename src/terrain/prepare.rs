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

const SOURCE_LOCAL_IDENTITIES: [(&str, &str); 4] = [
    (
        "classic-world-04-reconstruction",
        "b22ff9156730a358e39c83ed37d11dadf86f282cd16e663e905f19eaa0cd20f5",
    ),
    (
        "classic-world-08-reconstruction",
        "d0a544f4943991aa1265b7ebdfecb3a846215f297c1317ffb718ddd89e8d92bc",
    ),
    (
        "classic-world-10-reconstruction",
        "d7f3b0d274f4f6b0cf6093a336b8adbfaf322e9ce6d07c970b3219a8d24afd48",
    ),
    (
        "classic-world-13-reconstruction",
        "15cb5c88982275f3e72d4dc0bc9fb940177662013cacd6b76c1b95aeca46ecd1",
    ),
];

fn source_local_transform() -> serde_json::Value {
    serde_json::json!({
        "kind": "source-local-v1",
        "source_dimensions": [2000, 2000],
        "target_dimensions": [2000, 2000],
        "mapping": "tile_xy=source_xy",
        "historical_coordinates": "UNKNOWN",
        "status": "One source pixel per engineered cell does not prove original extent, pitch or origin."
    })
}

impl ReviewedRecipe {
    fn dimensions(&self) -> Result<[usize; 2]> {
        let source = SOURCE_LOCAL_IDENTITIES
            .iter()
            .find(|(id, _)| *id == self.id);
        let expected = match self.schema {
            2 => {
                ensure!(
                    source.is_none(),
                    "reserved source-local ID {} requires schema 3, not schema 2 padding",
                    self.id
                );
                [SIDE, SIDE]
            }
            3 => {
                ensure!(
                    source.is_some_and(|(_, hash)| *hash == self.provenance.source_sha256),
                    "unapproved source-local identity {} source {}",
                    self.id,
                    self.provenance.source_sha256
                );
                [2000, 2000]
            }
            schema => bail!("unsupported reviewed recipe schema {schema}"),
        };
        ensure!(
            [self.width, self.height] == expected,
            "{} reviewed-cells-v1 schema {} source {}: expected dimensions {expected:?}, actual {:?}",
            self.id,
            self.schema,
            self.provenance.source_sha256,
            [self.width, self.height]
        );
        Ok(expected)
    }
    fn audit_inputs(&self, root: &Path) -> Result<Vec<u8>> {
        self.dimensions()?;
        let p = &self.provenance;
        let policy = hashed_input(root, &p.policy.path, &p.policy.sha256, 64 * 1024)?;
        let annotations =
            hashed_input(root, &p.annotations.path, &p.annotations.sha256, 64 * 1024)?;
        audit_scoped(&policy, &annotations, p, self.schema == 3)?;
        Ok(policy)
    }
}

const WORLD01_SOURCE: &str = "7404dfa562627e015160a88820017cfbcfb2c6de3676a18dcca3395bd4fc2cc1";
const WORLD01_LAVA_SUPPORT: &str =
    "93f3410a461d0daa70d6efc79938a5a0938f69018bd32e9c5667bf6ee25a2b17";

struct World01Lava;
impl World01Lava {
    fn for_recipe(
        r: &ReviewedRecipe,
        policy: &serde_json::Value,
        content: &Content,
    ) -> Result<Option<Self>> {
        let id = "classic-world-01-reconstruction";
        if r.id != id
            && r.provenance.source_sha256 != WORLD01_SOURCE
            && policy.get("world01_lava").is_none()
        {
            return Ok(None);
        }
        ensure!(
            r.id == id
                && r.provenance.source_sha256 == WORLD01_SOURCE
                && r.width == SIDE
                && r.height == SIDE
                && r.content.objects == OBJECTS
                && r.content.grounds == GROUNDS
                && policy["source_sha256"] == WORLD01_SOURCE
                && policy["selected_client"] == "123.5.1"
                && policy["transform"]
                    == serde_json::json!({"crop":[26,26,2074,2074],"historical_coordinates":"UNKNOWN","source_dimensions":[2100,2100],"tile":"source_pixel-26"})
                && policy["world01_lava"]
                    == serde_json::json!({"rgb":"d44a00","pixels":703,"support_sha256_le_u32":WORLD01_LAVA_SUPPORT,"cell":{"ground":244,"object":null,"config":"","terrain":1,"region":0}}),
            "unapproved World1 lava source/content/transform/cell support"
        );
        ensure!(
            matches!(content.hazards.get(&244), Some(h) if h.name == "Lava Blend" && h.min == 30 && h.max == 60)
                && content.grounds.get(&244) == Some(&0.666)
                && content.pushes.get(&244) == Some(&(0.0, 0.0)),
            "selected World1 Lava Blend definition drift: hazard={:?}, speed={:?}, push={:?}",
            content.hazards.get(&244).map(|h| (&h.name, h.min, h.max)),
            content.grounds.get(&244),
            content.pushes.get(&244)
        );
        Ok(Some(Self))
    }
    fn accepts(&self, c: &Cell) -> bool {
        c.ground == 244
            && c.object.is_none()
            && c.config.is_empty()
            && c.terrain == 1
            && c.region == 0
    }
    fn validate_support(&self, map: &Map) -> Result<()> {
        ensure!(
            map.width == SIDE as i32 && map.height == SIDE as i32 && map.cells.len() == CELLS,
            "invalid World1 lava map dimensions"
        );
        let mut support = Sha256::new();
        let mut count = 0;
        for (i, c) in map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.ground == 244)
        {
            ensure!(
                self.accepts(c),
                "unapproved World1 lava cell at {},{}",
                i % SIDE,
                i / SIDE
            );
            support.update((((i / SIDE + 26) * 2100 + i % SIDE + 26) as u32).to_le_bytes());
            count += 1;
        }
        ensure!(
            count == 703 && format!("{:x}", support.finalize()) == WORLD01_LAVA_SUPPORT,
            "World1 lava support differs from approved source positions"
        );
        Ok(())
    }
}

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
    pub(super) fn content_identity(&self) -> Option<[String; 2]> {
        match self {
            Self::ReviewedCellsV1(r) if r.schema == 3 => {
                Some([r.content.objects.clone(), r.content.grounds.clone()])
            }
            _ => None,
        }
    }
    pub(super) fn dimensions(&self) -> [usize; 2] {
        match self {
            Self::ClassicV1(r) => [r.width, r.height],
            Self::ReviewedCellsV1(r) => [r.width, r.height],
        }
    }
    pub(super) fn contract(&self) -> String {
        let (schema, source) = match self {
            Self::ClassicV1(r) => (r.schema, &r.provenance.source_sha256),
            Self::ReviewedCellsV1(r) => (r.schema, &r.provenance.source_sha256),
        };
        format!(
            "{} {} schema {schema} source {source}",
            self.id(),
            self.profile()
        )
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
                r.dimensions()?;
                ensure!(valid_id(&r.id), "invalid Realm ID {}", r.id);
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
        .with_context(|| format!("validate recipe {}: {}", path.display(), recipe.contract()))?;
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
    if let Recipe::ReviewedCellsV1(r) = &recipe
        && r.schema == 3
    {
        r.audit_inputs(root).with_context(|| recipe.contract())?;
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
fn entry(
    c: &Cell,
    content: &Content,
    validated_objects: &mut BTreeSet<u16>,
    lava: Option<&World01Lava>,
) -> Result<Entry> {
    ensure!(
        content.grounds.contains_key(&c.ground)
            && (!content.hazards.contains_key(&c.ground) || lava.is_some_and(|l| l.accepts(c))),
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
                object.static_object && (!object.enemy || object.passive_destructible()),
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
            if !(c.config.is_empty()
                || kind == 0x1dd
                    && matches!(
                        c.config.as_str(),
                        "conn:16843265"
                            | "conn:16908545"
                            | "conn:16908802"
                            | "conn:33620225"
                            | "conn:33620482"
                            | "conn:33685762"
                            | "conn:33686017"
                            | "conn:33686018"
                    ))
            {
                let size: u16 = c
                    .config
                    .strip_prefix("size:")
                    .context("unsupported scenery configuration")?
                    .parse()?;
                ensure!(
                    ((matches!(kind, 0x100..=0x104 | 0x185..=0x18d | 0x197..=0x19f)
                        && (130..=160).contains(&size)
                        && size.is_multiple_of(5))
                        || (kind == 0x18e && matches!(size, 140 | 160))
                        || (kind == 0x194 && matches!(size, 120 | 140)))
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

    ensure!(
        name.len() <= 1024 && c.config.len() <= 1024,
        "output dictionary string exceeds limit"
    );
    Ok((c.ground, name, c.config.clone(), c.terrain, c.region))
}
fn encode(map: &Map, content: &Content) -> Result<(Vec<u8>, usize)> {
    encode_scoped(map, content, None)
}
fn encode_scoped(
    map: &Map,
    content: &Content,
    lava: Option<&World01Lava>,
) -> Result<(Vec<u8>, usize)> {
    ensure!(
        map.width > 0
            && map.height > 0
            && map.width <= SIDE as i32
            && map.height <= SIDE as i32
            && map.cells.len() == map.width as usize * map.height as usize,
        "invalid output dimensions/cells"
    );
    if let Some(lava) = lava {
        lava.validate_support(map)?;
    }
    let mut entries = BTreeSet::new();
    let mut validated_objects = BTreeSet::new();
    for c in &map.cells {
        entries.insert(entry(c, content, &mut validated_objects, lava)?);
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
    dictionary_scoped(bytes, content, None)
}
fn dictionary_scoped(
    bytes: &[u8],
    content: &Content,
    lava: Option<&World01Lava>,
) -> Result<Vec<Cell>> {
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
    let mut validated_objects = BTreeSet::new();
    for c in &cells {
        entry(c, content, &mut validated_objects, lava).context("validate reviewed dictionary")?;
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
#[cfg(test)]
fn audit(policy: &[u8], annotations: &[u8], provenance: &ImageProvenance) -> Result<()> {
    audit_scoped(policy, annotations, provenance, false)
}
fn audit_scoped(
    policy: &[u8],
    annotations: &[u8],
    provenance: &ImageProvenance,
    source_local: bool,
) -> Result<()> {
    let cell_count = if source_local { 2000 * 2000 } else { CELLS };
    let policy: serde_json::Value =
        serde_json::from_slice(policy).context("parse source policy")?;
    let annotations: serde_json::Value =
        serde_json::from_slice(annotations).context("parse source annotations")?;
    if source_local {
        ensure!(
            policy["schema"] == 2
                && policy["transform"] == source_local_transform()
                && annotations["transform"] == source_local_transform(),
            "source-local-v1 requires exact policy/annotations transform and support audit schema 2"
        );
    }
    let (support_key, expected_count, expected_pixels) = match policy["schema"].as_u64() {
        Some(1) => ("component", 101, 20571),
        Some(2) => {
            ensure!(
                policy["source_sha256"] == provenance.source_sha256
                    && policy["audit"]["kind"] == "reviewed-supports-v1",
                "inconsistent reviewed support audit"
            );
            let count = policy["audit"]["supports"]
                .as_u64()
                .filter(|n| (1..=4096).contains(n))
                .context("invalid reviewed support count")?;
            let pixels = policy["audit"]["pixels"]
                .as_u64()
                .filter(|n| (1..=cell_count as u64).contains(n))
                .context("invalid reviewed support total")?;
            ("support", count, pixels)
        }
        _ => bail!("unsupported source audit schema"),
    };
    ensure!(
        annotations["policy_sha256"] == provenance.policy.sha256
            && annotations["source_sha256"] == provenance.source_sha256
            && policy["selected_client"] == "123.5.1"
            && policy["transform"].is_object()
            && annotations["transform"] == policy["transform"],
        "inconsistent source audit identity/transform"
    );
    let rows = annotations["unknown"]
        .as_array()
        .context("missing UNKNOWN decisions")?;
    ensure!(
        rows.len() as u64 == expected_count,
        "missing UNKNOWN decision"
    );
    let mut pixels = 0u64;
    for (i, row) in rows.iter().enumerate() {
        ensure!(
            row[support_key].as_u64() == Some(i as u64 + 1)
                && row["status"] == "REVIEWED_RECONSTRUCTION"
                && row["reason"].as_str().is_some_and(|s| !s.is_empty())
                && row["support_sha256"].as_str().is_some_and(valid_hash)
                && row["terrain"].as_u64().is_some_and(|t| t <= 12)
                && row["ground"].as_u64().is_some_and(|g| g <= 255),
            "unreviewed UNKNOWN component {}",
            i + 1
        );
        if source_local {
            // Metadata typing is not proof of a source-position/RGB/alpha partition.
            let fields = ["ground", "object", "config", "terrain", "region"]
                .into_iter()
                .filter_map(|key| row.get(key).map(|value| (key.to_owned(), value.clone())))
                .collect::<serde_json::Map<_, _>>();
            let _: ReviewedCell = serde_json::from_value(serde_json::Value::Object(fields))
                .with_context(|| {
                    format!(
                        "source-local support {} requires all five typed fields",
                        i + 1
                    )
                })?;
        }
        pixels += row["pixels"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= cell_count as u64)
            .context("invalid UNKNOWN support size")?;
    }
    ensure!(
        pixels == expected_pixels,
        "inconsistent UNKNOWN support total"
    );
    Ok(())
}
fn indexed_map(raw: &[u8], dictionary: &[Cell], dimensions: [usize; 2]) -> Result<Map> {
    let [width, height] = dimensions;
    let bytes = index_bytes(dimensions)?;
    ensure!(
        raw.len() == bytes,
        "invalid reviewed index length for {dimensions:?}: expected {bytes}, actual {}",
        raw.len()
    );
    let cells = raw
        .chunks_exact(2)
        .enumerate()
        .map(|(i, bytes)| {
            let index = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
            dictionary.get(index).cloned().with_context(|| {
                format!(
                    "UNKNOWN dictionary index {index} at {},{}",
                    i % width,
                    i / width
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Map {
        width: width as i32,
        height: height as i32,
        cells,
        spawns: vec![],
    })
}
fn index_bytes([width, height]: [usize; 2]) -> Result<usize> {
    ensure!(
        width > 0 && height > 0 && width <= SIDE && height <= SIDE,
        "invalid reviewed dimensions {width}x{height}"
    );
    width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(2))
        .context("reviewed index byte count overflow")
}
fn reviewed_map(
    root: &Path,
    recipe: &ReviewedRecipe,
    content: &Content,
) -> Result<(Map, Option<World01Lava>)> {
    let dimensions = recipe.dimensions()?;
    let bytes = index_bytes(dimensions)?;
    let inputs = &recipe.inputs;
    let cells = hashed_input(
        root,
        &inputs.dictionary.path,
        &inputs.dictionary.sha256,
        64 * 1024,
    )?;
    let policy = recipe.audit_inputs(root).with_context(|| {
        format!(
            "{} reviewed-cells-v1 schema {} source {}",
            recipe.id, recipe.schema, recipe.provenance.source_sha256
        )
    })?;
    let policy: serde_json::Value = serde_json::from_slice(&policy)?;
    let lava = World01Lava::for_recipe(recipe, &policy, content)?;
    let dictionary = match lava.as_ref() {
        Some(lava) => dictionary_scoped(&cells, content, Some(lava))?,
        None => dictionary(&cells, content)?,
    };
    let indices = &inputs.indices;
    let compressed = hashed_input(root, &indices.path, &indices.sha256, bytes + 4096)?;
    let raw = inflate(&compressed, bytes, bytes + 4096, &indices.expanded_sha256)?;
    Ok((indexed_map(&raw, &dictionary, dimensions)?, lava))
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
    let (map, mainland, lava) = match recipe {
        Recipe::ClassicV1(r) => {
            let e = load_layer(&root, &r.layers.elevation)?;
            let m = load_layer(&root, &r.layers.moisture)?;
            let o = load_layer(&root, &r.layers.overrides)?;
            let (map, mainland) = assemble(&e, &m, &o, r.width, r.height, r.scenery_seed)?;
            (map, Some(mainland), None)
        }
        Recipe::ReviewedCellsV1(r) => {
            let (map, lava) =
                reviewed_map(&root, r, &content).with_context(|| recipe.contract())?;
            (map, None, lava)
        }
    };
    let prepared = started.elapsed();
    let (bytes, dictionary) = match lava.as_ref() {
        Some(lava) => encode_scoped(&map, &content, Some(lava))?,
        None => encode(&map, &content)?,
    };
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

    // These metadata and cell fixtures are synthetic, not approved source payloads.
    fn source_local_values(
        index: usize,
    ) -> (serde_json::Value, serde_json::Value, serde_json::Value) {
        let (id, source) = SOURCE_LOCAL_IDENTITIES[index];
        let mut value = reviewed_value();
        value["schema"] = serde_json::json!(3);
        value["id"] = serde_json::json!(id);
        value["output"] = serde_json::json!(format!("{id}.wmap"));
        value["width"] = serde_json::json!(2000);
        value["height"] = serde_json::json!(2000);
        value["provenance"]["source_sha256"] = serde_json::json!(source);
        let policy = serde_json::json!({
            "schema":2, "selected_client":"123.5.1", "source_sha256":source,
            "transform":source_local_transform(),
            "audit":{"kind":"reviewed-supports-v1","supports":1,"pixels":4_000_000}
        });
        value["provenance"]["policy"]["sha256"] =
            serde_json::json!(sha256(&serde_json::to_vec(&policy).unwrap()));
        let annotations = serde_json::json!({
            "source_sha256":source, "policy_sha256":value["provenance"]["policy"]["sha256"],
            "transform":source_local_transform(),
            "unknown":[{"support":1,"pixels":4_000_000,"support_sha256":"0".repeat(64),
                "status":"REVIEWED_RECONSTRUCTION","reason":"Synthetic geometry test only",
                "ground":190,"object":null,"config":"","terrain":11,"region":0}]
        });
        value["provenance"]["annotations"]["sha256"] =
            serde_json::json!(sha256(&serde_json::to_vec(&annotations).unwrap()));
        (value, policy, annotations)
    }
    fn write_json(root: &Path, relative: &str, value: &serde_json::Value) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn write_source_local(
        root: &Path,
        value: &serde_json::Value,
        policy: &serde_json::Value,
        annotations: &serde_json::Value,
    ) {
        write_json(root, "synthetic.json", value);
        for (key, data) in [("policy", policy), ("annotations", annotations)] {
            write_json(
                root,
                value["provenance"][key]["path"].as_str().unwrap(),
                data,
            );
        }
    }

    #[test]
    fn realm_prepare_source_local_four_geometry_contracts_and_identity_negatives() {
        let root =
            std::env::temp_dir().join(format!("source-local-metadata-{}", rand::random::<u64>()));
        for index in 0..4 {
            let (value, policy, annotations) = source_local_values(index);
            write_source_local(&root, &value, &policy, &annotations);
            assert_eq!(
                read_recipe(&root, "synthetic.json").unwrap().dimensions(),
                [2000, 2000]
            );
            for (field, invalid) in [
                ("schema", serde_json::json!(2)),
                ("schema", serde_json::json!(4)),
                ("profile", serde_json::json!("classic-v1")),
                ("orientation", serde_json::json!("x-major")),
            ] {
                let mut bad = value.clone();
                bad[field] = invalid;
                assert!(recipe(bad).is_err(), "{field}");
            }
            for field in ["width", "height"] {
                for invalid in [0i64, -1, 1999, 2001, 2048, 2100, i64::MAX] {
                    let mut bad = value.clone();
                    bad[field] = serde_json::json!(invalid);
                    assert!(recipe(bad).is_err(), "{field} {invalid}");
                }
            }
            for id in [
                "unknown-source",
                "classic-world-01-reconstruction",
                "classic-world-02-reconstruction",
                "classic-world-03-reconstruction",
            ] {
                let mut bad = value.clone();
                bad["id"] = serde_json::json!(id);
                bad["output"] = serde_json::json!(format!("{id}.wmap"));
                assert!(recipe(bad).is_err(), "{id}");
            }
            for (parent, field) in [
                ("content", "objects"),
                ("content", "grounds"),
                ("provenance", "source_sha256"),
            ] {
                let mut bad = value.clone();
                bad[parent][field] = serde_json::json!("0".repeat(64));
                assert!(recipe(bad).is_err(), "{parent}.{field}");
            }
            let mut padded = value.clone();
            padded["schema"] = serde_json::json!(2);
            padded["width"] = serde_json::json!(2048);
            padded["height"] = serde_json::json!(2048);
            assert!(recipe(padded).is_err());
            for key in ["policy", "annotations"] {
                let mut bad = value.clone();
                bad["provenance"][key]["sha256"] = serde_json::json!("0".repeat(64));
                write_json(&root, "synthetic.json", &bad);
                assert!(read_recipe(&root, "synthetic.json").is_err(), "{key} hash");
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn realm_prepare_source_local_audit_exact_transform_and_explicit_fields() {
        let (value, policy, annotations) = source_local_values(0);
        let Recipe::ReviewedCellsV1(r) = recipe(value).unwrap() else {
            panic!()
        };
        let check = |p: &serde_json::Value, a: &serde_json::Value| {
            audit_scoped(
                &serde_json::to_vec(p).unwrap(),
                &serde_json::to_vec(a).unwrap(),
                &r.provenance,
                true,
            )
        };
        check(&policy, &annotations).unwrap();
        for key in source_local_transform().as_object().unwrap().keys() {
            let mut p = policy.clone();
            let mut a = annotations.clone();
            p["transform"].as_object_mut().unwrap().remove(key);
            a["transform"] = p["transform"].clone();
            assert!(check(&p, &a).is_err(), "missing {key}");
            p["transform"][key] = serde_json::json!("CHANGED");
            a["transform"] = p["transform"].clone();
            assert!(check(&p, &a).is_err(), "changed {key}");
        }
        let mut p = policy.clone();
        let mut a = annotations.clone();
        p["transform"]["padding"] = serde_json::json!(0);
        a["transform"] = p["transform"].clone();
        assert!(check(&p, &a).is_err());
        for (field, invalid) in [
            ("selected_client", serde_json::json!("123.5.0")),
            ("source_sha256", serde_json::json!("0".repeat(64))),
            ("schema", serde_json::json!(1)),
        ] {
            let mut p = policy.clone();
            p[field] = invalid;
            assert!(check(&p, &annotations).is_err(), "{field}");
        }
        for (field, invalid) in [
            ("object", serde_json::json!("UNKNOWN")),
            ("config", serde_json::json!(null)),
            ("region", serde_json::json!(256)),
            ("ground", serde_json::json!(256)),
            ("terrain", serde_json::json!(13)),
            ("status", serde_json::json!("UNKNOWN")),
        ] {
            let mut a = annotations.clone();
            a["unknown"][0][field] = invalid;
            assert!(check(&policy, &a).is_err(), "{field}");
        }
        for field in ["ground", "object", "config", "terrain", "region"] {
            let mut a = annotations.clone();
            a["unknown"][0].as_object_mut().unwrap().remove(field);
            assert!(check(&policy, &a).is_err(), "missing {field}");
        }
        for count in [4_000_001, CELLS] {
            let mut p = policy.clone();
            let mut a = annotations.clone();
            p["audit"]["pixels"] = serde_json::json!(count);
            a["unknown"][0]["pixels"] = serde_json::json!(count);
            assert!(check(&p, &a).is_err());
            assert!(check(&policy, &a).is_err());
        }
    }

    #[test]
    fn realm_prepare_source_local_synthetic_indices_stride_length_and_stream() {
        let dimensions = [2000, 2000];
        let size = index_bytes(dimensions).unwrap();
        assert_eq!(size, 8_000_000);
        let dictionary: Vec<_> = (0..6)
            .map(|region| Cell {
                ground: 190,
                object: None,
                config: String::new(),
                terrain: 11,
                region,
            })
            .collect();
        let mut raw = vec![0; size];
        let positions = [0, 1999, 2000, 3_998_000, 3_999_999];
        for (n, i) in positions.into_iter().enumerate() {
            raw[i * 2..i * 2 + 2].copy_from_slice(&(n as u16 + 1).to_le_bytes());
        }
        let map = indexed_map(&raw, &dictionary, dimensions).unwrap();
        assert_eq!([map.width, map.height], [2000, 2000]);
        assert_eq!(map.cells.len(), 4_000_000);
        for (n, i) in positions.into_iter().enumerate() {
            assert_eq!(
                map.cell((i % 2000) as f32, (i / 2000) as f32)
                    .unwrap()
                    .region,
                n as u8 + 1
            );
        }
        assert!(map.cell(2000., 1999.).is_none());
        assert!(map.cell(1999., 2000.).is_none());
        drop(map);
        for bad in [
            &raw[..size - 1],
            &raw[..size - 2],
            &[raw.as_slice(), &[0, 0]].concat(),
            &vec![0; CELLS * 2],
        ] {
            assert!(indexed_map(bad, &dictionary, dimensions).is_err());
        }
        let bytes = zipped(&raw);
        let hash = sha256(&raw);
        assert_eq!(inflate(&bytes, size, size + 4096, &hash).unwrap(), raw);
        for bad in [
            bytes[..bytes.len() - 1].to_vec(),
            [bytes.as_slice(), &[0]].concat(),
            [bytes.as_slice(), bytes.as_slice()].concat(),
            zipped(&raw[..size - 1]),
            zipped(&[raw.as_slice(), &[0, 0]].concat()),
            vec![0; size + 4097],
        ] {
            assert!(inflate(&bad, size, size + 4096, &hash).is_err());
        }
        assert!(inflate(&bytes, size, size + 4096, &"0".repeat(64)).is_err());
        raw[size - 2..].copy_from_slice(&6u16.to_le_bytes());
        assert!(
            indexed_map(&raw, &dictionary, dimensions)
                .unwrap_err()
                .to_string()
                .contains("6 at 1999,1999")
        );
    }

    #[test]
    fn realm_source_open_source_local_synthetic_shape_policy_and_baseline_rejection() {
        let root =
            std::env::temp_dir().join(format!("source-local-open-{}", rand::random::<u64>()));
        let realms = root.join("realms");
        let (mut value, policy, annotations) = source_local_values(0);
        let cells = serde_json::json!([
            {"ground":190,"object":null,"config":"","terrain":11,"region":0},
            {"ground":190,"object":null,"config":"","terrain":1,"region":0}
        ]);
        let mut raw = vec![0; 8_000_000];
        raw[(1000 * 2000 + 1000) * 2] = 1;
        let compressed = zipped(&raw);
        value["inputs"]["dictionary"]["sha256"] =
            serde_json::json!(sha256(&serde_json::to_vec(&cells).unwrap()));
        value["inputs"]["indices"]["sha256"] = serde_json::json!(sha256(&compressed));
        value["inputs"]["indices"]["expanded_sha256"] = serde_json::json!(sha256(&raw));
        write_source_local(&realms, &value, &policy, &annotations);
        write_json(
            &realms,
            value["inputs"]["dictionary"]["path"].as_str().unwrap(),
            &cells,
        );
        std::fs::write(
            realms.join(value["inputs"]["indices"]["path"].as_str().unwrap()),
            &compressed,
        )
        .unwrap();
        let content = Content::load(Path::new("data")).unwrap();
        let Recipe::ReviewedCellsV1(r) = read_recipe(&realms, "synthetic.json").unwrap() else {
            panic!()
        };
        let (map, lava) = reviewed_map(&realms, &r, &content).unwrap();
        assert!(lava.is_none());
        let (bytes, _) = encode(&map, &content).unwrap();
        assert_eq!(bytes, encode(&map, &content).unwrap().0);
        let decoded = Map::decode_cells(&bytes, &content).unwrap();
        assert_eq!(decoded.cells.len(), 4_000_000);
        for (a, b) in map.cells.iter().zip(&decoded.cells) {
            assert_eq!(
                (a.ground, a.object, &a.config, a.terrain, a.region),
                (b.ground, b.object, &b.config, b.terrain, b.region)
            );
        }
        drop(decoded);
        drop(map);
        for field in ["sha256", "expanded_sha256"] {
            let mut bad = value.clone();
            bad["inputs"]["indices"][field] = serde_json::json!("0".repeat(64));
            let Recipe::ReviewedCellsV1(r) = recipe(bad).unwrap() else {
                panic!()
            };
            assert!(reviewed_map(&realms, &r, &content).is_err(), "{field}");
        }
        std::fs::write(realms.join(value["output"].as_str().unwrap()), &bytes).unwrap();
        let entry = serde_json::json!({"kind":"reconstruction","recipe":"synthetic.json","sha256":sha256(&bytes)});
        write_json(
            &root,
            "realm-catalog.json",
            &serde_json::json!([entry.clone()]),
        );
        let sources = super::super::realms::sources(&root).unwrap();
        let opened = sources[0].open(&content).unwrap();
        assert_eq!([opened.width, opened.height], [2000, 2000]);
        assert_eq!(opened.cells.len(), 4_000_000);
        assert_eq!(opened.spawns.len(), 4);
        drop(opened);
        let originals = [
            std::fs::read_to_string("data/objects.xml").unwrap(),
            std::fs::read_to_string("data/grounds.xml").unwrap(),
        ];
        for (name, data) in ["objects.xml", "grounds.xml"].into_iter().zip(&originals) {
            std::fs::write(root.join(name), data).unwrap();
        }
        let selected = Content::load(&root).unwrap();
        assert_eq!(selected.xml_sha256, content.xml_sha256);
        drop(sources[0].open(&selected).unwrap());
        for (file, needle, replacement, is_ground) in [
            (
                "grounds.xml",
                "<Ground type=\"0xbe\" id=\"Dark Sand\">",
                "<Ground type=\"0xbe\" id=\"Dark Sand\"><Speed>0.9</Speed>",
                true,
            ),
            (
                "objects.xml",
                "<Object type=\"0x0100\" id=\"Tree A\">",
                "<Object type=\"0x0100\" id=\"Tree A\"><Size>110</Size>",
                false,
            ),
        ] {
            let original = &originals[usize::from(is_ground)];
            assert_eq!(original.matches(needle).count(), 1);
            std::fs::write(root.join(file), original.replacen(needle, replacement, 1)).unwrap();
            let drifted = Content::load(&root).unwrap();
            if is_ground {
                assert_eq!(drifted.grounds[&190], 0.9);
            } else {
                assert_eq!(drifted.get(256).unwrap().size, 110);
            }
            let error = sources[0].open(&drifted).unwrap_err().to_string();
            assert!(
                error.contains("loaded XML identity mismatch"),
                "{file}: {error}"
            );
            assert!(error.contains("schema 3 source"), "{error}");
            std::fs::write(root.join(file), original).unwrap();
        }
        drop(sources);
        for key in ["policy", "annotations"] {
            write_json(
                &realms,
                value["provenance"][key]["path"].as_str().unwrap(),
                &serde_json::json!({}),
            );
            assert!(
                super::super::realms::sources(&root).is_err(),
                "runtime {key} hash"
            );
            write_source_local(&realms, &value, &policy, &annotations);
        }
        let mut corrupt = entry.clone();
        corrupt["sha256"] = serde_json::json!("0".repeat(64));
        write_json(&root, "realm-catalog.json", &serde_json::json!([corrupt]));
        assert!(super::super::realms::sources(&root).is_err());
        write_json(
            &root,
            "realm-catalog.json",
            &serde_json::json!([{"kind":"baseline","id":"synthetic-baseline","path":format!("realms/{}", value["output"].as_str().unwrap()),"sha256":sha256(&bytes)}]),
        );
        let sources = super::super::realms::sources(&root).unwrap();
        let error = sources[0].open(&content).unwrap_err().to_string();
        assert!(
            error.contains("expected dimensions [2048, 2048]"),
            "{error}"
        );
        assert!(error.contains("actual [2000, 2000]"), "{error}");
        let legacy = reviewed_value();
        write_json(&realms, "legacy.json", &legacy);
        std::fs::write(realms.join(legacy["output"].as_str().unwrap()), &bytes).unwrap();
        write_json(
            &root,
            "realm-catalog.json",
            &serde_json::json!([{"kind":"reconstruction","recipe":"legacy.json","sha256":sha256(&bytes)}]),
        );
        let sources = super::super::realms::sources(&root).unwrap();
        let error = sources[0].open(&content).unwrap_err().to_string();
        assert!(error.contains("schema 2 source"), "{error}");
        assert!(
            error.contains("expected dimensions [2048, 2048]"),
            "{error}"
        );
        // No Godlands: a dimension error must precede any arrival error.
        let mut wrong_shape = 1i16.to_le_bytes().to_vec();
        wrong_shape.extend(190i16.to_le_bytes());
        wrong_shape.extend([0, 0, 11, 0]);
        wrong_shape.extend(2048i32.to_le_bytes());
        wrong_shape.extend(2048i32.to_le_bytes());
        wrong_shape.resize(wrong_shape.len() + CELLS * 2, 0);
        let wrong_shape = zipped(&wrong_shape);
        std::fs::write(realms.join(value["output"].as_str().unwrap()), &wrong_shape).unwrap();
        let mut wrong_entry = entry;
        wrong_entry["sha256"] = serde_json::json!(sha256(&wrong_shape));
        write_json(
            &root,
            "realm-catalog.json",
            &serde_json::json!([wrong_entry]),
        );
        let sources = super::super::realms::sources(&root).unwrap();
        let error = sources[0].open(&content).unwrap_err().to_string();
        assert!(error.contains("schema 3 source"), "{error}");
        assert!(
            error.contains("expected dimensions [2000, 2000]"),
            "{error}"
        );
        assert!(error.contains("actual [2048, 2048]"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
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
    fn realm_prepare_world01_lava_requires_source_cells_and_exact_positions() {
        let mut content = Content::load(Path::new("data")).unwrap();
        let mut value = reviewed_value();
        value["id"] = serde_json::json!("classic-world-01-reconstruction");
        value["output"] = serde_json::json!("classic-world-01-reconstruction.wmap");
        value["provenance"]["source_sha256"] = serde_json::json!(WORLD01_SOURCE);
        let Recipe::ReviewedCellsV1(recipe) = serde_json::from_value(value.clone()).unwrap() else {
            panic!()
        };
        let binding: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../data/realms/world01/lava-support.json"
        ))
        .unwrap();
        assert_eq!(binding["source_sha256"], WORLD01_SOURCE);
        assert_eq!(binding["support_sha256_le_u32"], WORLD01_LAVA_SUPPORT);
        let policy = serde_json::json!({"source_sha256":WORLD01_SOURCE,"selected_client":"123.5.1",
            "transform":{"crop":[26,26,2074,2074],"historical_coordinates":"UNKNOWN","source_dimensions":[2100,2100],"tile":"source_pixel-26"},
            "world01_lava":{"rgb":"d44a00","pixels":703,"support_sha256_le_u32":WORLD01_LAVA_SUPPORT,"cell":{"ground":244,"object":null,"config":"","terrain":1,"region":0}}});
        assert_eq!(binding["cell"], policy["world01_lava"]["cell"]);
        let lava = World01Lava::for_recipe(&recipe, &policy, &content)
            .unwrap()
            .unwrap();
        let c = Cell {
            ground: 244,
            object: None,
            config: String::new(),
            terrain: 1,
            region: 0,
        };
        let safe = Cell {
            ground: 190,
            object: None,
            config: String::new(),
            terrain: 11,
            region: 0,
        };
        let mut map = Map {
            width: SIDE as i32,
            height: SIDE as i32,
            cells: vec![safe.clone(); CELLS],
            spawns: vec![],
        };
        let mut positions = Vec::new();
        for run in binding["source_positions_row_runs"].as_array().unwrap() {
            let (y, x0, x1) = (
                run[0].as_u64().unwrap() as usize,
                run[1].as_u64().unwrap() as usize,
                run[2].as_u64().unwrap() as usize,
            );
            assert!((26..2074).contains(&y) && (26..2074).contains(&x0) && x1 <= 2074 && x0 < x1);
            for x in x0..x1 {
                let target = (y - 26) * SIDE + x - 26;
                positions.push((target, y * 2100 + x));
                assert_eq!(map.cells[target].ground, 190);
                map.cells[target] = c.clone();
            }
        }
        assert_eq!(positions.len(), 703);
        assert!(positions.windows(2).all(|p| p[0].1 < p[1].1));
        let source_le: Vec<_> = positions
            .iter()
            .flat_map(|p| (p.1 as u32).to_le_bytes())
            .collect();
        let source_be: Vec<_> = positions
            .iter()
            .flat_map(|p| (p.1 as u32).to_be_bytes())
            .collect();
        assert_eq!(sha256(&source_le), WORLD01_LAVA_SUPPORT);
        assert_ne!(sha256(&source_be), WORLD01_LAVA_SUPPORT);
        assert_eq!(map.cells[(1086 - 26) * SIDE + 1129 - 26].ground, 244);
        assert_eq!(map.cells[(1086 - 26) * SIDE + 1128 - 26].ground, 190);
        let dict = serde_json::to_vec(&vec![safe.clone(), c.clone()]).unwrap();
        assert!(dictionary(&dict, &content).is_err());
        dictionary_scoped(&dict, &content, Some(&lava)).unwrap();
        let (bytes, _) = encode_scoped(&map, &content, Some(&lava)).unwrap();
        assert_eq!(bytes, encode_scoped(&map, &content, Some(&lava)).unwrap().0);
        let decoded = Map::decode_cells(&bytes, &content).unwrap();
        for (a, b) in map.cells.iter().zip(&decoded.cells) {
            assert_eq!(
                (a.ground, a.object, &a.config, a.terrain, a.region),
                (b.ground, b.object, &b.config, b.terrain, b.region)
            );
        }
        assert!(encode(&map, &content).is_err());
        for (field, val) in [
            ("id", serde_json::json!("other-world")),
            ("width", serde_json::json!(1024)),
            ("height", serde_json::json!(1024)),
        ] {
            let mut bad = value.clone();
            bad[field] = val;
            let Recipe::ReviewedCellsV1(r) = serde_json::from_value(bad).unwrap() else {
                panic!()
            };
            assert!(
                World01Lava::for_recipe(&r, &policy, &content).is_err(),
                "{field}"
            );
        }
        for path in [
            ["provenance", "source_sha256"],
            ["content", "objects"],
            ["content", "grounds"],
        ] {
            let mut bad = value.clone();
            bad[path[0]][path[1]] = serde_json::json!("0".repeat(64));
            let Recipe::ReviewedCellsV1(r) = serde_json::from_value(bad).unwrap() else {
                panic!()
            };
            assert!(World01Lava::for_recipe(&r, &policy, &content).is_err());
        }
        for field in [
            "source_sha256",
            "selected_client",
            "transform",
            "world01_lava",
        ] {
            let mut bad = policy.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(
                World01Lava::for_recipe(&recipe, &bad, &content).is_err(),
                "{field}"
            );
        }
        for (field, val) in [
            ("rgb", serde_json::json!("d44a01")),
            ("pixels", serde_json::json!(704)),
            ("support_sha256_le_u32", serde_json::json!("0".repeat(64))),
        ] {
            let mut bad = policy.clone();
            bad["world01_lava"][field] = val;
            assert!(World01Lava::for_recipe(&recipe, &bad, &content).is_err());
        }
        let first = positions[0].0;
        for (field, val) in [
            ("ground", serde_json::json!(112)),
            ("object", serde_json::json!(339)),
            ("config", serde_json::json!("size:150")),
            ("terrain", serde_json::json!(8)),
            ("region", serde_json::json!(1)),
        ] {
            let mut bad = policy.clone();
            bad["world01_lava"]["cell"][field] = val;
            assert!(World01Lava::for_recipe(&recipe, &bad, &content).is_err());
            let mut cell = serde_json::to_value(&c).unwrap();
            cell[field] = bad["world01_lava"]["cell"][field].clone();
            let cell: ReviewedCell = serde_json::from_value(cell).unwrap();
            let cell = Cell {
                ground: cell.ground,
                object: cell.object,
                config: cell.config,
                terrain: cell.terrain,
                region: cell.region,
            };
            assert!(
                dictionary_scoped(
                    &serde_json::to_vec(&vec![safe.clone(), cell.clone()]).unwrap(),
                    &content,
                    Some(&lava)
                )
                .is_err()
            );
            map.cells[first] = cell;
            assert!(encode_scoped(&map, &content, Some(&lava)).is_err());
            map.cells[first] = c.clone();
        }
        map.cells[first] = safe.clone();
        assert!(encode_scoped(&map, &content, Some(&lava)).is_err());
        assert_eq!(map.cells[first + 1].ground, 190);
        map.cells[first + 1] = c.clone();
        assert!(encode_scoped(&map, &content, Some(&lava)).is_err());
        map.cells[first] = c.clone();
        assert!(encode_scoped(&map, &content, Some(&lava)).is_err());
        map.cells[first + 1] = safe;
        for &kind in content.hazards.keys().filter(|&&g| g != 244) {
            let mut bad = c.clone();
            bad.ground = kind;
            assert!(
                dictionary_scoped(
                    &serde_json::to_vec(&vec![bad]).unwrap(),
                    &content,
                    Some(&lava)
                )
                .is_err()
            );
        }
        let small = Map {
            width: 1,
            height: 1,
            cells: vec![c],
            spawns: vec![],
        };
        assert!(encode_scoped(&small, &content, Some(&lava)).is_err());
        content.hazards.get_mut(&244).unwrap().max = 61;
        assert!(World01Lava::for_recipe(&recipe, &policy, &content).is_err());
        content.hazards.get_mut(&244).unwrap().max = 60;
        content.grounds.insert(244, 1.0);
        assert!(World01Lava::for_recipe(&recipe, &policy, &content).is_err());
        content.grounds.insert(244, 0.666);
        content.pushes.insert(244, (0.33, 0.33));
        assert!(World01Lava::for_recipe(&recipe, &policy, &content).is_err());
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
            .find(|o| o.static_object && o.enemy && !o.passive_destructible())
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
    fn realm_prepare_passive_originals_roundtrip_without_broadening_enemy_policy() {
        let content = Content::load(Path::new("data")).unwrap();
        let cells: Vec<_> = [0x122, 0x162, 0x181f]
            .into_iter()
            .map(|kind| Cell {
                ground: 0x61,
                object: Some(kind),
                config: String::new(),
                terrain: 0,
                region: 0,
            })
            .collect();
        let map = Map {
            width: 3,
            height: 1,
            cells,
            spawns: vec![],
        };
        let (bytes, _) = encode(&map, &content).unwrap();
        assert_eq!(bytes, encode(&map, &content).unwrap().0);
        let decoded = Map::decode_cells(&bytes, &content).unwrap();
        assert_eq!(
            serde_json::to_value(&map.cells).unwrap(),
            serde_json::to_value(&decoded.cells).unwrap()
        );
        for kind in [0x163, 0xd70, 0x91b, 0x91f, 0x928] {
            let mut bad = map.clone();
            bad.cells[0].object = Some(kind);
            assert!(encode(&bad, &content).is_err(), "{kind:#x}");
        }
        for config in ["size:120", "variant:1", "name:Tombstone"] {
            let mut bad = map.clone();
            bad.cells[2].config = config.into();
            assert!(encode(&bad, &content).is_err(), "{config}");
        }
    }

    #[test]
    fn realm_prepare_tomb_connections_match_selected_finite_geometry() {
        let content = Content::load(Path::new("data")).unwrap();
        let mut connections = BTreeSet::new();
        for mut value in [
            33686018u32,
            16908802,
            16843266,
            16908546,
            16843265,
            16843009,
        ] {
            for _ in 0..4 {
                connections.insert(value);
                value = value.rotate_right(8);
            }
        }
        let all_binary: BTreeSet<_> = (0..16)
            .map(|mask| {
                u32::from_le_bytes(std::array::from_fn(|i| {
                    if mask & (1 << i) == 0 { 2 } else { 1 }
                }))
            })
            .collect();
        assert_eq!(connections, all_binary);
        assert_eq!(connections.len(), 16);
        assert!(!connections.contains(&0));
        assert!(!connections.contains(&u32::MAX));
        let binding: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../data/realms/world01/tomb-connections.json"
        ))
        .unwrap();
        let allowed: BTreeSet<u32> = binding["supports"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                r["cell"]["config"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("conn:")
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert_eq!(allowed.len(), 8);
        assert!(allowed.is_subset(&connections));
        let cell = |kind, config: String| Cell {
            ground: 247,
            object: Some(kind),
            config,
            terrain: 0,
            region: 0,
        };
        let map = Map {
            width: 8,
            height: 1,
            cells: allowed
                .iter()
                .map(|c| cell(477, format!("conn:{c}")))
                .collect(),
            spawns: vec![],
        };
        let (bytes, _) = encode(&map, &content).unwrap();
        assert_eq!(bytes, encode(&map, &content).unwrap().0);
        let decoded = Map::decode_cells(&bytes, &content).unwrap();
        assert_eq!(
            serde_json::to_value(&map.cells).unwrap(),
            serde_json::to_value(&decoded.cells).unwrap()
        );
        for config in [
            "conn:0",
            "conn:-1",
            "conn:1",
            "conn: 33686018",
            "conn:33686018\0",
            "size:100;conn:33686018",
            "conn:4294967295",
            "conn:16843008",
            "conn:50529027",
            "conn:302121474",
            "conn:-2113797630",
            "conn:033686018",
            "conn:+33686018",
            "conn:0x02020202",
            "conn:33686018;",
            "conn:33686018;conn:16843009",
            "size:100",
        ] {
            let mut bad = map.clone();
            bad.cells[0] = cell(477, config.into());
            assert!(encode(&bad, &content).is_err(), "{config}");
            assert!(
                dictionary(
                    &serde_json::to_vec(&vec![map.cells[0].clone(), bad.cells[0].clone()]).unwrap(),
                    &content
                )
                .is_err(),
                "unused {config}"
            );
        }
        for value in connections.difference(&allowed) {
            let mut bad = map.clone();
            bad.cells[0] = cell(477, format!("conn:{value}"));
            assert!(encode(&bad, &content).is_err(), "valid but unused {value}");
        }
        for kind in [None, Some(288), Some(6175), Some(398)] {
            for value in &allowed {
                let mut bad = map.clone();
                bad.cells[0].object = kind;
                bad.cells[0].config = format!("conn:{value}");
                assert!(encode(&bad, &content).is_err(), "{kind:?} {value}");
            }
        }
        let mut blank = map;
        blank.cells[0].config.clear();
        encode(&blank, &content).unwrap();
    }

    #[test]
    fn realm_prepare_palm_cherry_sizes_preserve_existing_limits_and_roundtrip() {
        let content = Content::load(Path::new("data")).unwrap();
        let cell = |kind, size| Cell {
            ground: 0x61,
            object: Some(kind),
            config: format!("size:{size}"),
            terrain: 0,
            region: 0,
        };
        let mut cells = Vec::new();
        for kind in (0x100..=0x104).chain(0x185..=0x18d).chain(0x197..=0x19f) {
            for size in [130, 150, 160] {
                cells.push(cell(kind, size));
            }
        }
        for (kind, sizes) in [(0x18e, [140, 160]), (0x194, [120, 140])] {
            for size in sizes {
                cells.push(cell(kind, size));
            }
        }
        let map = Map {
            width: cells.len() as i32,
            height: 1,
            cells,
            spawns: vec![],
        };
        let (bytes, _) = encode(&map, &content).unwrap();
        assert_eq!(bytes, encode(&map, &content).unwrap().0);
        let decoded = Map::decode_cells(&bytes, &content).unwrap();
        assert_eq!(
            serde_json::to_value(&map.cells).unwrap(),
            serde_json::to_value(&decoded.cells).unwrap()
        );
        for (kind, sizes) in [
            (0x18e, vec![0, 120, 130, 150, 180]),
            (0x194, vec![0, 100, 130, 150, 160]),
            (0x100, vec![0, 132, 165]),
            (0x181f, vec![140, 150]),
        ] {
            for size in sizes {
                let mut bad = map.clone();
                bad.cells[0] = cell(kind, size);
                assert!(encode(&bad, &content).is_err(), "{kind:#x} size:{size}");
                assert!(
                    dictionary(
                        &serde_json::to_vec(&vec![map.cells[0].clone(), cell(kind, size)]).unwrap(),
                        &content
                    )
                    .is_err()
                );
            }
        }
        for config in [
            "size:0140",
            "size:+140",
            "size:-140",
            "size:65536",
            "size:140;",
            "size:140;size:160",
            "variant:1",
        ] {
            let mut bad = map.clone();
            bad.cells[0] = cell(0x18e, 140);
            bad.cells[0].config = config.into();
            assert!(encode(&bad, &content).is_err(), "{config}");
        }
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
            indexed_map(&raw, &[], [SIDE, SIDE])
                .unwrap_err()
                .to_string()
                .contains("UNKNOWN")
        );
        assert!(indexed_map(&raw[..raw.len() - 1], &[], [SIDE, SIDE]).is_err());
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
    fn realm_prepare_versioned_support_audit_requires_explicit_consistent_contract() {
        let Recipe::ReviewedCellsV1(r) = recipe(reviewed_value()).unwrap() else {
            panic!("reviewed fixture");
        };
        let mut p: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../data/realms/world02/source-policy.json"
        ))
        .unwrap();
        let mut a: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../data/realms/world02/source-annotations.json"
        ))
        .unwrap();
        p["schema"] = serde_json::json!(2);
        p["source_sha256"] = serde_json::json!(r.provenance.source_sha256);
        p["audit"] = serde_json::json!({"kind":"reviewed-supports-v1","supports":1,"pixels":3});
        let mut row = a["unknown"][0].clone();
        row.as_object_mut().unwrap().remove("component");
        row["support"] = serde_json::json!(1);
        row["pixels"] = serde_json::json!(3);
        a["unknown"] = serde_json::json!([row]);
        let check = |p: &serde_json::Value, a: &serde_json::Value| {
            audit(
                &serde_json::to_vec(p).unwrap(),
                &serde_json::to_vec(a).unwrap(),
                &r.provenance,
            )
        };
        check(&p, &a).unwrap();
        for value in [
            serde_json::json!(0),
            serde_json::json!(3),
            serde_json::Value::Null,
        ] {
            let mut bad = p.clone();
            bad["schema"] = value;
            assert!(check(&bad, &a).is_err());
        }
        for (field, value) in [
            ("kind", serde_json::json!("UNKNOWN")),
            ("supports", serde_json::json!(0)),
            ("supports", serde_json::json!(2)),
            ("supports", serde_json::json!(4097)),
            ("pixels", serde_json::json!(0)),
            ("pixels", serde_json::json!(4)),
            ("pixels", serde_json::json!(CELLS + 1)),
        ] {
            let mut bad = p.clone();
            bad["audit"][field] = value;
            assert!(check(&bad, &a).is_err());
        }
        let mut bad = a.clone();
        bad["unknown"][0]["support"] = serde_json::json!(2);
        assert!(check(&p, &bad).is_err());
        let mut bad = p.clone();
        bad["source_sha256"] = serde_json::json!("0".repeat(64));
        assert!(check(&bad, &a).is_err());
    }

    fn reviewed_roundtrip(
        file: &str,
        count: usize,
        size: usize,
        hash: &str,
    ) -> (Content, Map, ReviewedRecipe) {
        let content = Content::load(Path::new("data")).unwrap();
        let root = Path::new("data/realms");
        let Recipe::ReviewedCellsV1(r) = read_recipe(root, file).unwrap() else {
            panic!("reviewed fixture");
        };
        let (map, lava) = reviewed_map(root, &r, &content).unwrap();
        assert!(lava.is_none());
        let (bytes, actual_count) = encode(&map, &content).unwrap();
        assert_eq!(actual_count, count);
        assert_eq!(bytes.len(), size);
        assert_eq!(sha256(&bytes), hash);
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
        (content, map, r)
    }

    #[test]
    fn realm_prepare_passive_revisions_reverse_to_exact_published_payloads() {
        for (world, count, baseline) in [
            (
                2,
                699,
                "57ce63ef66783095ed6af1f461c838b7d9b457325040008038d16fe1ebcd5594",
            ),
            (
                3,
                922,
                "a6621e55c6a3d603bce33767051936f56aba6b6ba3eccab5251f931934b153af",
            ),
        ] {
            let content = Content::load(Path::new("data")).unwrap();
            let root = Path::new("data/realms");
            let Recipe::ReviewedCellsV1(r) =
                read_recipe(root, &format!("world{world:02}.json")).unwrap()
            else {
                panic!()
            };
            let mut map =
                Map::decode_cells(&std::fs::read(root.join(&r.output)).unwrap(), &content).unwrap();
            let ledger: serde_json::Value = serde_json::from_slice(
                &std::fs::read(root.join(format!("restoration/world{world:02}-passives.json")))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(ledger["source_sha256"], r.provenance.source_sha256);
            assert_eq!(ledger["changed_output_pixels"], count);
            let mut changed = BTreeSet::new();
            for row in ledger["cohorts"].as_array().unwrap() {
                let before: ReviewedCell = serde_json::from_value(row["before"].clone()).unwrap();
                let after: ReviewedCell = serde_json::from_value(row["after"].clone()).unwrap();
                assert_eq!(
                    (before.ground, &before.config, before.terrain, before.region),
                    (after.ground, &after.config, after.terrain, after.region)
                );
                assert!([Some(290), Some(354), Some(6175)].contains(&after.object));
                let mut positions = Vec::new();
                for run in row["runs"].as_array().unwrap() {
                    let y = run[0].as_u64().unwrap() as usize;
                    let start = run[1].as_u64().unwrap() as usize;
                    let stop = run[2].as_u64().unwrap() as usize;
                    assert!((26..2074).contains(&y) && start >= 26 && start < stop && stop <= 2074);
                    for x in start..stop {
                        let i = (y - 26) * 2048 + x - 26;
                        assert!(changed.insert(i));
                        positions.push(i as u32);
                        let cell = &mut map.cells[i];
                        assert_eq!(
                            (
                                cell.ground,
                                cell.object,
                                &cell.config,
                                cell.terrain,
                                cell.region
                            ),
                            (
                                after.ground,
                                after.object,
                                &after.config,
                                after.terrain,
                                after.region
                            ),
                            "world{world} {x},{y}"
                        );
                        cell.object = before.object;
                    }
                }
                assert_eq!(positions.len() as u64, row["count"].as_u64().unwrap());
                assert!(positions.windows(2).all(|p| p[0] < p[1]));
                assert_eq!(
                    sha256(
                        &positions
                            .iter()
                            .flat_map(|i| i.to_le_bytes())
                            .collect::<Vec<_>>()
                    ),
                    row["output_sha256_le_u32"].as_str().unwrap()
                );
            }
            assert_eq!(changed.len(), count as usize);
            assert_eq!(
                sha256(
                    &changed
                        .iter()
                        .flat_map(|i| (*i as u32).to_le_bytes())
                        .collect::<Vec<_>>()
                ),
                ledger["changed_output_support_sha256_le_u32"]
                    .as_str()
                    .unwrap()
            );
            // Reverting only the declared objects must recover every old field, not just matching counts.
            assert_eq!(sha256(&encode(&map, &content).unwrap().0), baseline);
            assert_eq!(map.cells[0].ground, 188);
            map.cells[0].ground = 190;
            assert_ne!(sha256(&encode(&map, &content).unwrap().0), baseline);
        }
    }

    #[test]
    fn realm_prepare_reviewed_world02_exact_two_encodes_and_all_fields() {
        let (content, map, r) = reviewed_roundtrip(
            "world02.json",
            55,
            452520,
            "7f911d27fd4b28640cab696ab5ac817020640480ac66992976fe66a6141fed30",
        );
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

    #[test]
    fn realm_prepare_reviewed_world03_exact_two_encodes_and_all_fields() {
        reviewed_roundtrip(
            "world03.json",
            55,
            442591,
            "0b960c072c0544e9a911842edf36e1afffa91c9c12e410fb9d598fcfb72357a2",
        );
    }
}
