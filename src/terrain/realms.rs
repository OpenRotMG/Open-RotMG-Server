use crate::content::{Content, Map};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{collections::VecDeque, path::Path, sync::Arc};

const ASSETS: [(&str, &str); 3] = [
    (
        "realm-fsod-1",
        "7c6045ce772be1d2805a97fc4c45380d1095cdc4236ec7a2366ae0de4df80df9",
    ),
    (
        "realm-fsod-2",
        "bd1b448df6dede4a2a3ae6c36c6f47322c660e6cb1ee614175ad7853c4813f62",
    ),
    (
        "realm-fsod-3",
        "b63d971c5dc9d0d2c703903e138664565d26ff3b7131db0f2ae9686eaaf2c4ad",
    ),
];

pub(crate) enum Source {
    Encoded(&'static str, Vec<u8>),
    #[cfg(test)]
    Fixture(Arc<Map>),
}
impl Source {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Encoded(name, _) => name,
            #[cfg(test)]
            Self::Fixture(_) => "procedural-test-fixture",
        }
    }
    pub(crate) fn open(&self, content: &Content) -> Result<Arc<Map>> {
        match self {
            Self::Encoded(name, bytes) => {
                let started = std::time::Instant::now();
                let mut map = Map::decode(bytes, content)
                    .with_context(|| format!("decode Realm layout {name}"))?;
                ensure!(
                    map.width == 2048 && map.height == 2048,
                    "{name}: expected 2048-square layout"
                );
                prepare_arrivals(&mut map, content)
                    .with_context(|| format!("prepare Realm layout {name}"))?;
                tracing::info!(
                    layout = name,
                    width = map.width,
                    height = map.height,
                    scenery = map.cells.iter().filter(|c| c.object.is_some()).count(),
                    elapsed_ms = started.elapsed().as_millis(),
                    "private-server Realm layout opened"
                );
                Ok(Arc::new(map))
            }
            #[cfg(test)]
            Self::Fixture(map) => Ok(map.clone()),
        }
    }
}
pub(crate) fn sources(path: &Path) -> Result<Vec<Source>> {
    ASSETS
        .into_iter()
        .map(|(name, expected)| {
            let file = path.join(format!("{name}.wmap"));
            let bytes = std::fs::read(&file)
                .with_context(|| format!("read Realm layout {}", file.display()))?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            ensure!(
                hash == expected,
                "Realm layout {name} hash mismatch: expected {expected}, got {hash}"
            );
            tracing::info!(layout = name, sha256 = hash, "Realm asset verified");
            Ok(Source::Encoded(name, bytes))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Cell;

    #[test]
    fn arrivals_reject_remote_islands_and_missing_habitats() {
        let content = Content::load(Path::new("data")).unwrap();
        let mut map = Map {
            width: 64,
            height: 64,
            spawns: vec![],
            cells: vec![
                Cell {
                    ground: 0xbc,
                    object: None,
                    config: String::new(),
                    terrain: 0,
                    region: 0
                };
                64 * 64
            ],
        };
        for (x0, y0, size) in [
            (16, 16, 32),
            (30, 4, 5),
            (30, 55, 5),
            (4, 30, 5),
            (55, 30, 5),
        ] {
            for y in y0..y0 + size {
                for x in x0..x0 + size {
                    let cell = &mut map.cells[y * 64 + x];
                    cell.ground = 0xbe;
                    cell.terrain = 11;
                    cell.region = 1;
                }
            }
        }
        map.cells[32 * 64 + 32].terrain = 1;
        prepare_arrivals(&mut map, &content).unwrap();
        assert_eq!(map.spawns.len(), 4);
        assert!(
            map.spawns
                .iter()
                .all(|(x, y)| (17.0..47.0).contains(x) && (17.0..47.0).contains(y))
        );
        assert_eq!(map.cells.iter().filter(|c| c.region == 1).count(), 36);
        map.cells[32 * 64 + 32].terrain = 0;
        assert!(
            prepare_arrivals(&mut map, &content)
                .unwrap_err()
                .to_string()
                .contains("Godlands")
        );
    }

    #[test]
    fn missing_and_modified_assets_fail_instead_of_generating() {
        let path =
            std::env::temp_dir().join(format!("openrotmg-realm-assets-{}", rand::random::<u64>()));
        std::fs::create_dir(&path).unwrap();
        assert!(sources(&path).is_err());
        std::fs::write(path.join("realm-fsod-1.wmap"), b"modified").unwrap();
        let error = sources(&path).err().unwrap().to_string();
        assert!(error.contains("realm-fsod-1 hash mismatch"), "{error}");
        std::fs::remove_dir_all(path).unwrap();
    }
}
