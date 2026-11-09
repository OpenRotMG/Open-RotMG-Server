// Phase comparator: LoE bc11dff7, Kage Kami Setpiece; selected XML overrides its stats.
// The March2012 build122.2.1 notes establish the hidden grave.
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
        let from = e.stage;
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
        // Phase A runs 5.7-10 s in 2012 footage, probably a time limit cut short by damage; a random 6-10 s stands in for the rule.
        if stage == 2 {
            e.reaction_until = self.now + self.rng.gen_range(6000..=10000);
        }
        tracing::info!(entity = id, stage, "Kage Kami phase entered");
        // One taunt at the end of the reveal, a different line in every 2012 fight (tH9A8yA4XQI t=104.2, -JoNbNKdfGc t=233, …).
        if stage == 2 && from == Some(1) {
            let line = *[
                "Kyoufu no kage!",
                "Fear me!!!",
                "You shall pay!!!",
                "You shall pay for sullying this hallowed earth!",
                "You dare to desecrate this sacred ground?",
                "You brought this upon yourself!",
            ]
            .choose(&mut self.rng)
            .unwrap();
            let packet = chat::text_packet("#Kage Kami", id, 5, line);
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
            // One mine burst with a player within ≈3 tiles about 0.5 s after landing; an untriggered one is gone within the 4 s kept here.
            if stage == 0
                && (elapsed >= 4000
                    || elapsed >= 500 && target.is_some_and(|p| p.distance(e.pos) < 3.))
            {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(1);
                e.activated = Some(self.now);
                e.expires.set(Some(self.now + 50), id, &mut self.timed_ids);
            }
            return Ok(());
        }
        // He shows himself at the first hits (≈150 damage within 0.6 s, tH9A8yA4XQI 2012-04-03), stays invulnerable 2.4 s, then
        // alternates a vulnerable hooded phase A and an invulnerable ghost phase B that chases for 6.4-8.1 s.
        let next = match stage {
            0 if e.hp < self.content.objects[&KAGE].hp => Some(1),
            1 if elapsed >= 2400 => Some(2),
            2 if self.now >= e.reaction_until => Some(3),
            3 if elapsed >= 7000 => Some(2),
            0..=3 => None,
            _ => unreachable!("invalid Kage phase"),
        };
        if let Some(next) = next {
            self.manor_entry_phase(id, next);
        }
        let e = &self.entities[&id];
        // Phase A throws a Specter Mine at a player about once a second; the throw is short.
        if e.stage == Some(2)
            && self.now >= e.next_aux
            && e.conditions(self.now) & condition::bit(condition::STUNNED) == 0
            && let Some(position) = target.filter(|p| p.distance(e.pos) < 10.)
        {
            self.manor_toss_in(id, MINE, position, 4050, 250)?;
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 1000;
        }
        Ok(())
    }
}
