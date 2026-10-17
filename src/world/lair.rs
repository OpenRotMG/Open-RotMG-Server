use super::*;
use crate::encounter::{Motion, nr_speed};

impl World {
    pub(super) fn lair_escort_movement(&mut self, id: i32) -> Result<(Motion, f32)> {
        let e = &self.entities[&id];
        if e.conditions(self.now) & crate::condition::bit(crate::condition::PARALYZED) != 0 {
            return Ok((
                match e.motion {
                    Some(motion) => motion,
                    None => Motion::Stationary,
                },
                0.,
            ));
        }
        let boss = self.escort_anchor(id, 0xd90)?;
        if let Some((boss, anchor)) =
            boss.filter(|(boss, _)| e.pos.distance(self.entities[boss].pos) < 10.)
        {
            let distance = e.pos.distance(self.entities[&boss].pos);
            if (e.stage != Some(2) || distance > 6.) && distance > 1. {
                self.entities.get_mut(&id).unwrap().stage = Some(1);
                return Ok((
                    Motion::Protect {
                        anchor,
                        distance: 1.,
                    },
                    nr_speed(0.625),
                ));
            }
            self.entities.get_mut(&id).unwrap().stage = Some(2);
        } else {
            self.entities.get_mut(&id).unwrap().stage = None;
        }
        Ok((
            Motion::Follow {
                sight: 10.,
                stop: 7.,
                idle: nr_speed(0.25),
            },
            nr_speed(0.75),
        ))
    }
}
