// Phase comparator: LoE bc11dff7, Kage Kami Setpiece; selected XML overrides its stats.
// The March2012 build122.2.1 notes establish the hidden grave and probabilistic Manor entrance.
use super::*;
use crate::{
    condition,
    dungeon::manor::{KAGE, MINE},
};

impl World {
    pub(super) fn populate_manor_entry(&mut self) -> Result<()> {
        for kind in [KAGE, MINE, crate::dungeon::manor::GRAVE] {
            crate::encounter::validate(&self.content, kind)?;
            crate::loot::validate(&self.content, kind)?;
        }
        // The pinned SetPieces.cs uses Random.Next(2,3), whose upper bound is exclusive.
        for _ in 0..2 {
            let pos = self.place_named_setpiece(KAGE, &[4, 3])?;
            let id = self.spawn_entity(KAGE, pos, vec![])?;
            self.entities.get_mut(&id).unwrap().stage = Some(0);
            tracing::info!(entity = id, ?pos, "hidden Kage Kami graveyard placed");
        }
        Ok(())
    }

    fn manor_entry_phase(&mut self, id: i32, stage: u64) {
        let e = self.entities.get_mut(&id).unwrap();
        e.stage = Some(stage);
        e.activated = Some(self.now);
        e.next_aux = self.now;
        e.home = e.pos;
        e.goal = None;
        e.condition &= !condition::bit(condition::INVINCIBLE);
        if matches!(stage, 1 | 3) {
            e.condition |= condition::bit(condition::INVINCIBLE);
        }
        e.extra_stats = vec![(61, if stage == 3 { 2 } else { 1 })];
        tracing::info!(entity = id, stage, "Kage Kami phase entered");
        if stage == 1 && self.rng.gen_bool(0.5) {
            let packet = chat::text_packet("#Kage Kami", id, 5, "Kyoufu no kage!");
            for p in self
                .players
                .values()
                .filter(|p| p.pos.distance(self.entities[&id].pos) < 20.)
            {
                send(p, packet.id, packet.body.clone());
            }
        }
    }

    pub(super) fn manor_entry_mechanics(
        &mut self,
        id: i32,
        target: Option<Pos>,
        elapsed: u64,
    ) -> Result<()> {
        self.entities.get_mut(&id).unwrap().stage.get_or_insert(0);
        let e = &self.entities[&id];
        let stage = e.stage.expect("initialized Manor entrance phase");
        if e.kind == MINE {
            if stage == 0 && (elapsed >= 4000 || target.is_some_and(|p| p.distance(e.pos) < 3.)) {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(1);
                e.activated = Some(self.now);
                e.expires.set(Some(self.now + 50), id, &mut self.timed_ids);
            }
            return Ok(());
        }
        let next = match stage {
            0 if e.hp * 10 < self.content.objects[&KAGE].hp * 9 => Some(1),
            1 if elapsed >= 2000 => Some(2),
            2 if elapsed >= 5000 => Some(3),
            3 if elapsed >= 4000 => Some(2),
            0..=3 => None,
            _ => unreachable!("invalid Kage phase"),
        };
        if let Some(next) = next {
            self.manor_entry_phase(id, next);
        }
        let e = &self.entities[&id];
        if matches!(e.stage, Some(2 | 3))
            && self.now >= e.next_aux
            && e.conditions(self.now) & condition::bit(condition::STUNNED) == 0
            && let Some(position) = target.filter(|p| p.distance(e.pos) < 5.)
        {
            self.manor_toss(id, MINE, position, 4050)?;
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 2000;
        }
        Ok(())
    }
}
