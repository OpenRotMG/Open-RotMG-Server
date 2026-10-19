use super::*;
use std::ops::Deref;

#[derive(Default)]
pub struct Entities {
    values: BTreeMap<i32, Entity>,
    pub(super) promotions: u64,
    shared: usize,
}

impl Deref for Entities {
    type Target = BTreeMap<i32, Entity>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl Entities {
    pub fn insert(&mut self, id: i32, entity: Entity) -> Option<Entity> {
        self.shared += usize::from(matches!(entity.state, EntityState::Shared(_)));
        let old = self.values.insert(id, entity);
        self.shared -= usize::from(
            old.as_ref()
                .is_some_and(|e| matches!(e.state, EntityState::Shared(_))),
        );
        old
    }
    pub fn get_mut(&mut self, id: &i32) -> Option<&mut Entity> {
        let entity = self.values.get_mut(id)?;
        if entity.promote() {
            self.promotions += 1;
            self.shared -= 1;
            tracing::debug!(
                entity = id,
                kind = entity.kind,
                "scenery promoted for mutation"
            );
        }
        Some(entity)
    }
    pub fn remove(&mut self, id: &i32) -> Option<Entity> {
        let entity = self.values.remove(id)?;
        self.shared -= usize::from(matches!(entity.state, EntityState::Shared(_)));
        Some(entity)
    }
    // inspect receives &&mut Entity, which cannot promote the selected entity.
    #[allow(clippy::manual_inspect)]
    pub fn matching_mut(
        &mut self,
        mut predicate: impl FnMut(&Entity) -> bool,
    ) -> impl Iterator<Item = &mut Entity> {
        let promotions = &mut self.promotions;
        let shared = &mut self.shared;
        self.values
            .values_mut()
            .filter(move |e| predicate(e))
            .map(move |e| {
                if e.promote() {
                    *promotions += 1;
                    *shared -= 1;
                }
                e
            })
    }
    pub(super) fn shared(&self) -> usize {
        self.shared
    }
}

#[derive(Clone, Debug)]
pub(super) enum EntityState {
    Owned(Box<EntityData>),
    Shared(Arc<EntityData>),
}

impl Entity {
    pub(super) fn share(
        &mut self,
        templates: &mut BTreeMap<(u16, String), Arc<EntityData>>,
        config: &str,
    ) {
        let key = (self.kind, config.to_owned());
        let data = templates
            .entry(key)
            .or_insert_with(|| Arc::new((**self).clone()));
        self.state = EntityState::Shared(data.clone());
    }
    fn promote(&mut self) -> bool {
        if let EntityState::Shared(data) = &self.state {
            self.state = EntityState::Owned(Box::new((**data).clone()));
            true
        } else {
            false
        }
    }
}

impl std::ops::Deref for Entity {
    type Target = EntityData;
    fn deref(&self) -> &EntityData {
        match &self.state {
            EntityState::Owned(data) => data,
            EntityState::Shared(data) => data,
        }
    }
}
impl std::ops::DerefMut for Entity {
    fn deref_mut(&mut self) -> &mut EntityData {
        match &mut self.state {
            EntityState::Owned(data) => data,
            EntityState::Shared(_) => panic!("shared scenery mutation must use Entities::get_mut"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promotion_isolates_siblings_and_keeps_shared_counts_exact() {
        let mut w = super::super::tests::world();
        let (x, y) = w.map.spawns[0];
        let pos = Pos { x, y };
        let ids = [
            w.spawn_entity(0x105, pos, vec![]).unwrap(),
            w.spawn_entity(0x105, pos, vec![]).unwrap(),
        ];
        let (shared, promotions) = (w.entities.shared(), w.entities.promotions);
        let mut templates = BTreeMap::new();
        for id in ids {
            let mut e = w.entities.remove(&id).unwrap();
            e.share(&mut templates, "");
            assert!(w.entities.insert(id, e).is_none());
        }
        assert_eq!(w.entities.shared(), shared + 2);
        let peer_stats = w.stats(ids[1], 0).unwrap();
        let peer_state = serde_json::to_vec(&w.entities[&ids[1]]).unwrap();
        w.apply_condition(ids[0], crate::condition::SLOWED, 1000)
            .unwrap();
        w.entities.get_mut(&ids[0]).unwrap().name = "changed".into();
        assert_eq!(w.entities.promotions, promotions + 1);
        assert_eq!(w.entities.shared(), shared + 1);
        assert_eq!(w.stats(ids[1], 0).unwrap(), peer_stats);
        assert_eq!(
            serde_json::to_vec(&w.entities[&ids[1]]).unwrap(),
            peer_state
        );
        for e in w.entities.matching_mut(|e| e.id == ids[1]) {
            e.size = 77;
        }
        assert_eq!(w.entities.promotions, promotions + 2);
        assert_eq!(w.entities.shared(), shared);
        w.remove_entity(ids[0]);
        w.remove_entity(ids[1]);
        assert_eq!(w.entities.shared(), shared);
    }
}
