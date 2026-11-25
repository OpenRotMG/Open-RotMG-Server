use super::{GameData, Path};
use std::collections::BTreeMap;

pub(crate) fn selected<const N: usize>(files: [(&str, Vec<u8>); N]) -> BTreeMap<String, Vec<u8>> {
    files
        .into_iter()
        .map(|(file, bytes)| (file.into(), bytes))
        .chain(["objects.xml", "grounds.xml"].map(|file| {
            (
                file.into(),
                std::fs::read(Path::new("data").join(file)).unwrap(),
            )
        }))
        .collect()
}

pub(crate) fn production(game: &GameData, root: &Path) -> BTreeMap<String, Vec<u8>> {
    game.inputs
        .keys()
        .map(|file| (file.clone(), std::fs::read(root.join(file)).unwrap()))
        .collect()
}
