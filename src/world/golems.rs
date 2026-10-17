use super::*;
use crate::encounter::{Anchor, Motion};
pub(super) const ROOTS: &[u16] = &[0x62d, 0x62e, 0x62f, 0x631, 0x632, 0x633];

impl World {
    pub(super) fn golem_movement(&self, id: i32, elapsed: u64) -> Result<(Motion, f32)> {
        let e = &self.entities[&id];
        let camp = e.camp.context("golem lacks its bounded satellite camp")?;
        if ROOTS.contains(&e.kind) {
            return Ok(if e.pos.distance(e.home) > 16. {
                (
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 1.,
                    },
                    10.,
                )
            } else {
                crate::encounter::movement(e.kind, e.hp, elapsed)
            });
        }
        let root = self
            .entities
            .values()
            .filter(|x| x.camp == Some(camp) && ROOTS.contains(&x.kind))
            .min_by(|a, b| e.pos.distance(a.pos).total_cmp(&e.pos.distance(b.pos)));
        if matches!(e.kind, 0x6d5..=0x6d7)
            && let Some(hub) = self
                .entities
                .values()
                .filter(|x| x.camp == Some(camp) && matches!(x.kind, 0x6d2..=0x6d4))
                .min_by(|a, b| e.pos.distance(a.pos).total_cmp(&e.pos.distance(b.pos)))
        {
            return Ok((
                Motion::StrictOrbit {
                    anchor: Anchor::CampEntity(hub.id),
                    radius: 1.,
                },
                10.,
            ));
        }
        Ok(match root {
            Some(root) => (
                Motion::StrictOrbit {
                    anchor: Anchor::CampEntity(root.id),
                    radius: 3.,
                },
                5.,
            ),
            None => (Motion::Stationary, 0.),
        })
    }
    pub(super) fn orphan_satellites(&mut self, removed: &Entity) {
        if !ROOTS.contains(&removed.kind) {
            return;
        }
        let Some(camp) = removed.camp else {
            return;
        };
        if self
            .entities
            .values()
            .any(|e| e.camp == Some(camp) && ROOTS.contains(&e.kind))
        {
            return;
        }
        for e in self
            .entities
            .values_mut()
            .filter(|e| e.camp == Some(camp) && matches!(e.kind, 0x6d2..=0x6d7))
        {
            e.expires = Some(self.now + 30000);
        }
        tracing::debug!(
            camp,
            until = self.now + 30000,
            "orphaned owned golem satellites scheduled to decay"
        );
    }
}
