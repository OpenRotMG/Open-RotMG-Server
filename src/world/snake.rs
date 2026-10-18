use super::*;
use crate::condition;
use crate::dungeon::snake::{BOSS, PET, SWARM, phase, shield};

impl World {
    pub(super) fn snake_mechanics(&mut self, id: i32, elapsed: u64) -> Result<()> {
        let (stage, age) = phase(elapsed);
        let changed = self.entities[&id].stage != Some(stage);
        let e = self.entities.get_mut(&id).unwrap();
        e.condition &= !condition::bit(condition::INVULNERABLE);
        if shield(elapsed) {
            e.condition |= condition::bit(condition::INVULNERABLE);
        }
        if changed {
            e.stage = Some(stage);
            e.next_aux = self.now;
            tracing::info!(
                entity = id,
                kind = BOSS,
                stage,
                hp = e.hp,
                "Stheno timed phase entered"
            );
        }
        self.spawn_children(id, PET, 4, 10.)?;
        if stage % 3 != 1 || age >= 16000 {
            let children: Vec<_> = self
                .actors()
                .filter(|e| e.parent == Some(id) && e.kind == SWARM)
                .map(|e| e.id)
                .collect();
            for child in children {
                self.remove_entity(child);
            }
        } else if self.now >= self.entities[&id].next_aux {
            let count = self
                .actors()
                .filter(|e| e.parent == Some(id) && e.kind == SWARM)
                .count();
            self.spawn_children(id, SWARM, (count + 1).min(8), 2.5)?;
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 750;
        }
        Ok(())
    }
}
