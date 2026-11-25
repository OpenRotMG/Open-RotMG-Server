// Oasis.cs / SpawnGroup.cs, realm-src@1855c017 (GPL-3.0; data/REALM-SRC-LICENSE), NR-Core and Skilly.
use super::*;
use crate::encounter::landmarks;

impl World {
    pub(super) fn landmark_bind(&mut self, id: i32) -> Result<Option<i32>> {
        let e = &self.entities[&id];
        if let Some(tag) = *e.quest {
            return Ok(Some(tag));
        }
        let kind = e.kind;
        let root = landmarks::owner(kind)?;
        if landmarks::ROOTS.contains(&kind) {
            self.entities
                .get_mut(&id)
                .unwrap()
                .quest
                .set(Some(id), id, &mut self.quest_ids);
            return Ok(Some(id));
        }
        if matches!(kind, 0x65a..=0x65e) {
            return Ok(None);
        }
        let mut roots = self.entities.values().filter(|e| e.kind == root);
        let owner = roots.next().map(|e| e.id);
        ensure!(
            roots.next().is_none(),
            "map-born landmark follower has ambiguous ownership"
        );
        if let Some(owner) = owner {
            let tag = self
                .landmark_bind(owner)?
                .context("landmark root lacks ownership")?;
            self.entities
                .get_mut(&id)
                .unwrap()
                .quest
                .set(Some(tag), id, &mut self.quest_ids);
            return Ok(Some(tag));
        }
        Ok(None)
    }
}
