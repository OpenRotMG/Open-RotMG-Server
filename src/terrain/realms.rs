use crate::content::{Content, Map};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{collections::VecDeque, path::Path, sync::Arc};

use super::prepare::{bounded_read, read_recipe, safe_path, valid_hash, valid_id};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Baseline {
        id: String,
        path: String,
        sha256: String,
    },
    Reconstruction {
        recipe: String,
        sha256: String,
    },
}

pub(crate) struct Asset {
    pub(crate) id: String,
    pub(crate) bytes: Vec<u8>,
    sha256: String,
    profile: String,
}

pub(crate) enum Source {
    Encoded(Asset),
    #[cfg(test)]
    Fixture(Arc<Map>),
}
impl Source {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Encoded(asset) => &asset.id,
            #[cfg(test)]
            Self::Fixture(_) => "procedural-test-fixture",
        }
    }
    pub(crate) fn open(&self, content: &Content) -> Result<Arc<Map>> {
        match self {
            Self::Encoded(asset) => {
                let name = &asset.id;
                let bytes = &asset.bytes;
                let started = std::time::Instant::now();
                let mut map = Map::decode_cells(bytes, content)
                    .with_context(|| format!("decode Realm layout {name}"))?;
                ensure!(
                    map.width == 2048 && map.height == 2048,
                    "{name}: expected 2048-square layout"
                );
                prepare_arrivals(&mut map, content)
                    .with_context(|| format!("prepare Realm layout {name}"))?;
                tracing::info!(
                    layout = name,
                    sha256 = asset.sha256,
                    profile = asset.profile,
                    width = map.width,
                    height = map.height,
                    scenery = map.cells.iter().filter(|c| c.object.is_some()).count(),
                    elapsed_ms = started.elapsed().as_millis(),
                    "Realm layout opened"
                );
                Ok(Arc::new(map))
            }
            #[cfg(test)]
            Self::Fixture(map) => Ok(map.clone()),
        }
    }
}
pub(crate) fn sources(path: &Path) -> Result<Vec<Source>> {
    let catalog = safe_path(path, "realm-catalog.json")?;
    let entries: Vec<Entry> = serde_json::from_slice(&bounded_read(&catalog, 64 * 1024)?)
        .with_context(|| format!("parse Realm catalog {}", catalog.display()))?;
    ensure!(
        !entries.is_empty(),
        "empty Realm catalog {}",
        catalog.display()
    );
    let mut ids = std::collections::BTreeSet::new();
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let load = || -> Result<Source> {
                let (id, file, expected, profile) = match entry {
                    Entry::Baseline {
                        id,
                        path: relative,
                        sha256,
                    } => (id, safe_path(path, &relative)?, sha256, "fsod".to_owned()),
                    Entry::Reconstruction { recipe, sha256 } => {
                        let root = safe_path(path, "realms")?;
                        let metadata = read_recipe(&root, &recipe).with_context(|| {
                            format!("Realm recipe {recipe} expected output SHA-256 {sha256}")
                        })?;
                        let file = safe_path(&root, &metadata.output)?;
                        (
                            metadata.id,
                            file,
                            sha256,
                            metadata.profile.name().to_owned(),
                        )
                    }
                };
                ensure!(valid_id(&id), "invalid Realm catalog ID {id:?}");
                ensure!(ids.insert(id.clone()), "duplicate Realm catalog ID {id}");
                ensure!(
                    valid_hash(&expected),
                    "Realm {id} {}: invalid SHA-256 {expected:?}",
                    file.display()
                );
                let bytes = bounded_read(&file, 16 * 1024 * 1024).with_context(|| {
                    format!("Realm {id} {} expected SHA-256 {expected}", file.display())
                })?;
                let hash = format!("{:x}", Sha256::digest(&bytes));
                ensure!(
                    hash == expected,
                    "Realm {id} {} hash mismatch: expected {expected}, got {hash}",
                    file.display()
                );
                tracing::info!(layout = id, sha256 = hash, profile, "Realm asset verified");
                Ok(Source::Encoded(Asset {
                    id,
                    bytes,
                    sha256: hash,
                    profile,
                }))
            };
            load().with_context(|| format!("Realm catalog {} entry {index}", catalog.display()))
        })
        .collect()
}

fn prepare_arrivals(map: &mut Map, content: &Content) -> Result<()> {
    let width = map.width as usize;
    let passable: Vec<_> = map
        .cells
        .iter()
        .map(|c| {
            content.grounds[&c.ground] > 0.
                && c.object.is_none_or(|kind| !content.objects[&kind].blocks)
        })
        .collect();
    let start = map
        .cells
        .iter()
        .enumerate()
        .filter(|(i, c)| c.terrain == 1 && passable[*i])
        .min_by_key(|(i, _)| {
            (i % width).abs_diff(width / 2) + (i / width).abs_diff(map.height as usize / 2)
        })
        .map(|(i, _)| i)
        .context("Realm has no walkable Godlands")?;
    // A remote beach island is not a usable arrival even when its surrounding nine cells are clear.
    let mut reachable = vec![false; map.cells.len()];
    let mut queue = VecDeque::from([start]);
    reachable[start] = true;
    while let Some(i) = queue.pop_front() {
        let x = i % width;
        for next in [
            i.wrapping_sub(width),
            i + width,
            if x > 0 { i - 1 } else { usize::MAX },
            if x + 1 < width { i + 1 } else { usize::MAX },
        ] {
            if next < reachable.len() && passable[next] && !reachable[next] {
                reachable[next] = true;
                queue.push_back(next);
            }
        }
    }
    for cell in &mut map.cells {
        if cell.region == 1 {
            cell.region = 0;
        }
    }
    map.spawns.clear();
    for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
        let (x, y) = (1..map.height - 1)
            .flat_map(|y| (1..map.width - 1).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let (rx, ry) = (x - map.width / 2, y - map.height / 2);
                dx * rx + dy * ry > (dx * ry - dy * rx).abs()
                    && reachable[(y * map.width + x) as usize]
                    && (y - 1..=y + 1).all(|ny| {
                        (x - 1..=x + 1).all(|nx| {
                            let cell = &map.cells[(ny * map.width + nx) as usize];
                            cell.terrain == 11 && content.grounds[&cell.ground] > 0.
                        })
                    })
            })
            .min_by_key(|(x, y)| {
                let (rx, ry) = (x - map.width / 2, y - map.height / 2);
                ((dx * ry - dy * rx).abs(), -(dx * rx + dy * ry))
            })
            .with_context(|| format!("Realm has no connected coastal arrival toward {dx},{dy}"))?;
        for ny in y - 1..=y + 1 {
            for nx in x - 1..=x + 1 {
                let cell = &mut map.cells[(ny * map.width + nx) as usize];
                cell.object = None;
                cell.config.clear();
                cell.region = 1;
            }
        }
        map.spawns.push((x as f32 + 0.5, y as f32 + 0.5));
    }
    Ok(())
}
