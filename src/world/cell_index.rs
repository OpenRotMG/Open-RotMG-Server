const EMPTY: i32 = i32::MIN;
const SECTION: usize = 16;

// Most tiles have at most one entity; tree headers and singleton allocations dwarf the IDs.
#[derive(Clone)]
pub struct CellIndex {
    width: usize,
    heads: Vec<i32>,
    overflow: Vec<Vec<i32>>,
    free: Vec<usize>,
    sections_wide: usize,
    actors: Vec<Vec<i32>>,
}

impl CellIndex {
    pub fn new(width: usize, height: usize) -> Self {
        let sections_wide = width.div_ceil(SECTION);
        Self {
            width,
            heads: vec![EMPTY; width * height],
            overflow: Vec::new(),
            free: Vec::new(),
            sections_wide,
            actors: vec![Vec::new(); sections_wide * height.div_ceil(SECTION)],
        }
    }
    pub fn len(&self) -> usize {
        self.heads.len()
    }
    fn section(&self, cell: usize) -> usize {
        (cell / self.width / SECTION) * self.sections_wide + (cell % self.width) / SECTION
    }
    pub fn ids(&self, cell: usize) -> &[i32] {
        match self.heads[cell] {
            EMPTY => &[],
            head if head >= 0 => std::slice::from_ref(&self.heads[cell]),
            head => &self.overflow[(-head - 1) as usize],
        }
    }
    pub fn insert(&mut self, cell: usize, id: i32, actor: bool) {
        assert!(id >= 0, "cell index requires non-negative entity ids");
        match self.heads[cell] {
            EMPTY => self.heads[cell] = id,
            head if head >= 0 => {
                assert_ne!(head, id, "entity already indexed in cell");
                let mut ids = vec![head.min(id), head.max(id)];
                let slot = match self.free.pop() {
                    Some(slot) => {
                        std::mem::swap(&mut self.overflow[slot], &mut ids);
                        slot
                    }
                    None => {
                        self.overflow.push(ids);
                        self.overflow.len() - 1
                    }
                };
                self.heads[cell] = -(slot as i32) - 1;
            }
            head => {
                let ids = &mut self.overflow[(-head - 1) as usize];
                let at = ids
                    .binary_search(&id)
                    .expect_err("entity already indexed in cell");
                ids.insert(at, id);
            }
        }
        if actor {
            let section = self.section(cell);
            let ids = &mut self.actors[section];
            let at = ids
                .binary_search(&id)
                .expect_err("actor already indexed in section");
            ids.insert(at, id);
        }
    }
    pub fn remove(&mut self, cell: usize, id: i32) -> bool {
        let removed = match self.heads[cell] {
            EMPTY => false,
            head if head >= 0 => {
                let found = head == id;
                if found {
                    self.heads[cell] = EMPTY;
                }
                found
            }
            head => {
                let slot = (-head - 1) as usize;
                let ids = &mut self.overflow[slot];
                let Ok(at) = ids.binary_search(&id) else {
                    return false;
                };
                ids.remove(at);
                if ids.len() == 1 {
                    self.heads[cell] = ids[0];
                    ids.clear();
                    self.free.push(slot);
                }
                true
            }
        };
        if removed {
            let section = self.section(cell);
            if let Ok(at) = self.actors[section].binary_search(&id) {
                self.actors[section].remove(at);
            }
        }
        removed
    }
    pub fn actors_in(
        &self,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) -> impl Iterator<Item = i32> + '_ {
        (y0 / SECTION..=y1 / SECTION).flat_map(move |sy| {
            (x0 / SECTION..=x1 / SECTION)
                .flat_map(move |sx| self.actors[sy * self.sections_wide + sx].iter().copied())
        })
    }
    #[cfg(test)]
    pub fn count(&self, id: i32) -> usize {
        (0..self.heads.len())
            .filter(|cell| self.ids(*cell).contains(&id))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cells_collapse_reuse_overflow_and_move_actors_between_sections() {
        let mut cells = CellIndex::new(32, 32);
        for (id, actor) in [(7, false), (3, true), (5, true)] {
            cells.insert(0, id, actor);
        }
        assert_eq!(cells.ids(0), [3, 5, 7]);
        assert_eq!(cells.actors_in(0, 0, 15, 15).collect::<Vec<_>>(), [3, 5]);
        assert!(cells.remove(0, 5));
        assert!(cells.remove(0, 7));
        assert_eq!(cells.ids(0), [3]);
        assert!(cells.remove(0, 3));
        assert!(cells.ids(0).is_empty());
        assert!(!cells.remove(0, 3));
        assert!(cells.actors_in(0, 0, 15, 15).next().is_none());
        cells.insert(16, 7, false);
        cells.insert(16, 3, true);
        assert_eq!(cells.ids(16), [3, 7]);
        assert_eq!(cells.overflow.len(), 1);
        assert_eq!(cells.actors_in(16, 0, 31, 15).collect::<Vec<_>>(), [3]);
        assert!(cells.remove(16, 3));
        cells.insert(16 * 32, 3, true);
        assert!(cells.actors_in(16, 0, 31, 15).next().is_none());
        assert_eq!(cells.actors_in(0, 16, 15, 31).collect::<Vec<_>>(), [3]);
        assert_eq!(cells.count(3), 1);
    }
}
