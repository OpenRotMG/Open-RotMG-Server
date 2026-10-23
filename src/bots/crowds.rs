use super::*;
use crate::world::view::PlayerMarker;
#[cfg(test)]
mod tests;

pub(super) struct Crowd {
    pub knows_minimap: bool,
    contacts: BTreeMap<i32, (PlayerMarker, u64)>,
    approaching: Option<(i32, i32)>,
    completed: BTreeSet<(i32, i32)>,
}
impl Crowd {
    pub fn new(knows_minimap: bool) -> Self {
        Self {
            knows_minimap,
            contacts: BTreeMap::new(),
            approaching: None,
            completed: BTreeSet::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.contacts.len()
    }
    pub fn approach_selected(&mut self, id: i32) {
        let peer = &self
            .contacts
            .get(&id)
            .expect("selected passive crowd cue missing")
            .0;
        self.approaching = Some((peer.id, peer.account));
    }
    pub fn attend(
        &mut self,
        v: &PlayerView,
        content: &Content,
        cognition: &pursuits::Cognition,
        inspecting: bool,
    ) {
        let published = |(id, account): &(i32, i32)| {
            v.player_markers
                .iter()
                .any(|p| p.id == *id && p.account == *account)
        };
        self.completed.retain(published);
        if self.approaching.is_some_and(|peer| !published(&peer)) {
            self.approaching = None;
        }
        self.contacts.retain(|id, (_, seen)| {
            v.time_ms.saturating_sub(*seen) < 6000 && v.player_markers.iter().any(|p| p.id == *id)
        });
        let occupied = v
            .objects
            .iter()
            .filter(|o| content.objects[&o.kind].enemy && o.hp > 0)
            .count()
            + usize::from(inspecting);
        let budget = cognition.attention.saturating_sub(occupied);
        let mut cues: Vec<_> = v
            .player_markers
            .iter()
            .filter(|p| self.knows_minimap || v.objects.iter().any(|o| o.id == p.id))
            .collect();
        cues.sort_by(|a, b| {
            a.position
                .distance(v.position)
                .total_cmp(&b.position.distance(v.position))
                .then(a.id.cmp(&b.id))
        });
        for cue in cues.into_iter().take(budget) {
            if !self.contacts.contains_key(&cue.id) {
                tracing::info!(
                    account = v.session.account,
                    peer = cue.account,
                    object = cue.id,
                    world = v.session.world,
                    "autonomous crowd cue noticed"
                );
            }
            self.contacts.insert(cue.id, (cue.clone(), v.time_ms));
            if self.approaching == Some((cue.id, cue.account))
                && let Some(reached) = v.objects.iter().find(|p| {
                    p.id == cue.id
                        && p.account == Some(cue.account)
                        && p.hp > 0
                        && p.position.distance(v.position) <= 7.
                })
            {
                self.completed.insert((cue.id, cue.account));
                self.approaching = None;
                tracing::info!(
                    account = v.session.account,
                    character = v.session.character,
                    player = v.session.player,
                    world = v.session.world,
                    tick = v.tick,
                    time_ms = v.time_ms,
                    peer = cue.account,
                    object = cue.id,
                    distance = reached.position.distance(v.position),
                    "autonomous crowd approach completed"
                );
            }
        }
    }
    pub fn destination<'a>(
        &'a self,
        v: &PlayerView,
        group: Option<&adventures::Adventure>,
    ) -> Option<&'a PlayerMarker> {
        if !matches!(v.map.name.as_str(), "Realm" | "Pirate Cave" | "Snake Pit") {
            return None;
        }
        self.contacts
            .values()
            .filter(|(p, seen)| {
                *seen == v.time_ms
                    && !self.completed.contains(&(p.id, p.account))
                    && (group.is_some_and(|g| {
                        !g.finished && g.members.iter().any(|m| m.account == p.account)
                    }) || self
                        .contacts
                        .values()
                        .any(|(q, _)| q.id != p.id && q.position.distance(p.position) < 14.))
            })
            .min_by(|(a, _), (b, _)| {
                // Different accounts approach different edges; nobody owns the outing's destination.
                let score = |p: &PlayerMarker| {
                    p.position.distance(v.position)
                        + ((p.account as u32 ^ v.session.account as u32).wrapping_mul(2654435761)
                            % 9) as f32
                };
                score(a).total_cmp(&score(b)).then(a.id.cmp(&b.id))
            })
            .map(|(p, _)| p)
    }
}
