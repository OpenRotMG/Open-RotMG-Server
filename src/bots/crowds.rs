use super::*;
use crate::world::view::PlayerMarker;
mod cooperation;
#[cfg(test)]
mod tests;
pub(super) use cooperation::available as cooperative_available;

pub(super) struct Crowd {
    pub knows_minimap: bool,
    pub bands: bands::Bands,
    binding: Option<[i32; 4]>,
    contacts: BTreeMap<i32, (PlayerMarker, u64)>,
    approaching: Option<(i32, i32)>,
    company: BTreeMap<(i32, i32), (u64, bool)>,
    cooperation: cooperation::Cooperation,
}
impl Crowd {
    pub fn new(knows_minimap: bool) -> Self {
        Self {
            knows_minimap,
            bands: bands::Bands::default(),
            binding: None,
            contacts: BTreeMap::new(),
            approaching: None,
            company: BTreeMap::new(),
            cooperation: cooperation::Cooperation::default(),
        }
    }
    pub fn observe(&mut self, v: &PlayerView, content: &Content) {
        self.bands.observe(v, content);
        let binding = cooperation::binding(v);
        if self.binding != Some(binding) {
            self.release(v, "session_changed");
            self.contacts.clear();
            self.company.clear();
            self.binding = Some(binding);
        }
        self.cooperation.observe(
            cooperation::binding(v),
            v.elapsed_ms,
            v.objects
                .iter()
                .filter(|p| content.objects[&p.kind].enemy)
                .map(|p| (p.id, p.kind)),
        );
        let bands = &self.bands;
        self.cooperation
            .company(v.elapsed_ms, |peer| visible(bands, v, peer), |_| true);
    }
    pub fn receive(&mut self, packet: &Packet, v: &PlayerView, content: &Content) -> Result<()> {
        self.cooperation.receive(
            packet,
            cooperation::binding(v),
            v.elapsed_ms,
            |id| {
                v.objects
                    .iter()
                    .find(|p| p.id == id && content.objects[&p.kind].enemy)
                    .map(|p| p.kind)
            },
            |id| {
                v.objects
                    .iter()
                    .find(|p| p.id == id && p.hp > 0)
                    .and_then(|p| p.account)
            },
        )
    }
    pub fn cooperator(
        &mut self,
        v: &PlayerView,
        eligible: impl Fn(Pos) -> bool,
    ) -> Option<PlayerMarker> {
        let bands = &self.bands;
        let ((id, account), position) =
            self.cooperation
                .company(v.elapsed_ms, |peer| visible(bands, v, peer), eligible)?;
        let name = v
            .objects
            .iter()
            .find(|p| p.id == id)
            .expect("validated cooperative peer missing")
            .name
            .clone();
        Some(PlayerMarker {
            id,
            account,
            position,
            name,
        })
    }
    pub fn release(&mut self, v: &PlayerView, reason: &'static str) {
        self.cooperation.release(v.elapsed_ms, reason);
        self.approaching = None;
        for (peer, (until, active)) in &mut self.company {
            if *active {
                *active = false;
                *until = v.time_ms + 30_000;
                tracing::info!(
                    account = v.session.account,
                    target = peer.0,
                    peer = peer.1,
                    reason,
                    "autonomous crowd company released"
                );
            }
        }
    }
    pub fn len(&self) -> usize {
        self.contacts.len()
    }
    pub fn keeping_company(&self, id: i32) -> bool {
        self.company
            .iter()
            .any(|((peer, _), (_, active))| *peer == id && *active)
    }
    pub fn approach_selected(&mut self, id: i32) {
        let (peer, seen) = self
            .contacts
            .get(&id)
            .expect("selected passive crowd cue missing");
        let key = (peer.id, peer.account);
        // Bound the whole outing, including a moving stranger we never catch up with.
        if self.company.len() < 4 && !self.company.contains_key(&key) {
            self.company.insert(key, (*seen + 90_000, true));
            self.approaching = Some(key);
        }
    }
    pub fn attend(
        &mut self,
        v: &PlayerView,
        content: &Content,
        cognition: &pursuits::Cognition,
        inspecting: bool,
    ) {
        self.company
            .retain(|_, (until, active)| *active || v.time_ms < *until);
        for (peer, (until, active)) in &mut self.company {
            if !*active {
                continue;
            }
            let marker = v.player_markers.iter().find(|p| (p.id, p.account) == *peer);
            let reason = if v.time_ms >= *until {
                Some("company_window_ended")
            } else if marker.is_none()
                && self.contacts.get(&peer.0).is_none_or(|(p, seen)| {
                    p.account != peer.1 || v.time_ms.saturating_sub(*seen) >= 6000
                })
            {
                Some("lost_public_marker")
            } else if v
                .objects
                .iter()
                .any(|p| p.id == peer.0 && (p.account != Some(peer.1) || p.hp <= 0))
            {
                Some("peer_dead_or_identity_changed")
            } else if marker.is_some_and(|p| !self.bands.survivable(v, p.position)) {
                Some("unsafe_target")
            } else if marker.is_some_and(|p| p.position.distance(v.position) > 36.) {
                Some("separated")
            } else {
                None
            };
            if let Some(reason) = reason {
                *active = false;
                *until = v.time_ms + 30_000;
                tracing::info!(
                    account = v.session.account,
                    target = peer.0,
                    peer = peer.1,
                    reason,
                    "autonomous crowd company released"
                );
            }
        }
        if self
            .approaching
            .is_some_and(|peer| self.company.get(&peer).is_none_or(|(_, active)| !*active))
        {
            self.approaching = None;
        }
        self.contacts
            .retain(|_, (_, seen)| v.time_ms.saturating_sub(*seen) < 6000);
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
            .filter(|p| {
                v.objects
                    .iter()
                    .find(|o| o.id == p.id)
                    .is_none_or(|o| o.account == Some(p.account) && o.hp > 0)
            })
            .collect();
        cues.sort_by(|a, b| {
            a.position
                .distance(v.position)
                .total_cmp(&b.position.distance(v.position))
                .then(a.id.cmp(&b.id))
        });
        for (index, cue) in cues.into_iter().enumerate() {
            let peer = (cue.id, cue.account);
            if index >= budget && self.approaching != Some(peer) && !self.keeping_company(cue.id) {
                continue;
            }
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
                tracing::info!(
                    account = v.session.account,
                    target = cue.id,
                    peer = cue.account,
                    distance = reached.position.distance(v.position),
                    reason = "approach_arrived",
                    "autonomous crowd approach arrived"
                );
                self.approaching = None;
            }
        }
    }
    /// Players gathered around a position: the whole minimap for those who read it, otherwise the screen.
    pub fn near(&self, v: &PlayerView, position: Pos) -> usize {
        v.player_markers
            .iter()
            .filter(|p| p.position.distance(position) < 12.)
            .filter(|p| self.knows_minimap || v.objects.iter().any(|o| o.id == p.id))
            .count()
    }
    /// Players heading for a Realm quest teleport into the crowd around it, then walk in.
    pub fn nearest_to<'a>(
        &self,
        v: &'a PlayerView,
        objective: Pos,
        eligible: impl Fn(Pos) -> bool,
    ) -> Option<&'a PlayerMarker> {
        if !self.knows_minimap || !v.map.allow_teleport {
            return None;
        }
        let own = v.position.distance(objective);
        v.player_markers
            .iter()
            .filter(|p| {
                // Live: half of level-20 deaths came within 10 seconds of landing beside the boss.
                (10. ..=16.).contains(&p.position.distance(objective))
                    && p.position.distance(objective) + 16. < own
                    && self.bands.teleportable(v, p.position)
                    && eligible(p.position)
            })
            .min_by(|a, b| {
                a.position
                    .distance(objective)
                    .total_cmp(&b.position.distance(objective))
            })
    }
    pub fn shortcut(
        &self,
        v: &PlayerView,
        group: Option<&adventures::Adventure>,
        objective: Pos,
    ) -> Option<&PlayerMarker> {
        if !v.map.allow_teleport || matches!(v.map.name.as_str(), "Realm" | "Nexus") {
            return None;
        }
        // Rushers shorten the existing dungeon quest: https://anticosmic.blogspot.com/2012/04/fame-farming.html#tunnel_rat
        self.attended_destination(
            v,
            group,
            |_| false,
            |_| true,
            |p| {
                p.distance(v.position) > 24.
                    && p.distance(objective) + 16. < v.position.distance(objective)
            },
        )
    }
    pub fn destination<'a>(
        &'a self,
        v: &PlayerView,
        group: Option<&adventures::Adventure>,
        known: impl Fn(i32) -> bool,
        eligible_position: impl Fn(Pos) -> bool,
    ) -> Option<&'a PlayerMarker> {
        if !matches!(v.map.name.as_str(), "Realm" | "Pirate Cave" | "Snake Pit")
            || !self.cooperation.passive_available(v.elapsed_ms)
        {
            return None;
        }
        self.attended_destination(
            v,
            group,
            known,
            |account| can_approach(v.session.world, v.session.account, account),
            eligible_position,
        )
    }
    fn attended_destination(
        &self,
        v: &PlayerView,
        group: Option<&adventures::Adventure>,
        known: impl Fn(i32) -> bool,
        eligible: impl Fn(i32) -> bool,
        eligible_position: impl Fn(Pos) -> bool,
    ) -> Option<&PlayerMarker> {
        self.contacts
            .values()
            .filter(|(p, seen)| {
                *seen == v.time_ms
                    && eligible(p.account)
                    && eligible_position(p.position)
                    && (self.company.len() < 4 || self.company.contains_key(&(p.id, p.account)))
                    && self
                        .company
                        .get(&(p.id, p.account))
                        .is_none_or(|(_, active)| *active)
            })
            .min_by(|(a, _), (b, _)| {
                // Different accounts approach different edges; nobody owns the outing's destination.
                let score = |p: &PlayerMarker| {
                    let shared = group.is_some_and(|g| {
                        !g.finished && g.members.iter().any(|m| m.account == p.account)
                    }) || self.contacts.values().any(|(q, seen)| {
                        *seen == v.time_ms && q.id != p.id && q.position.distance(p.position) < 14.
                    });
                    p.position.distance(v.position) - if shared { 8. } else { 0. }
                        + ((p.account as u32 ^ v.session.account as u32).wrapping_mul(2654435761)
                            % 9) as f32
                        - if known(p.account) { 3. } else { 0. }
                };
                // Small score changes do not justify abandoning company already being approached.
                let approaching = |p: &PlayerMarker| {
                    self.approaching == Some((p.id, p.account)) || self.keeping_company(p.id)
                };
                approaching(b)
                    .cmp(&approaching(a))
                    .then_with(|| score(a).total_cmp(&score(b)))
                    .then(a.id.cmp(&b.id))
            })
            .map(|(p, _)| p)
    }
}

pub(super) fn can_approach(world: i32, account: i32, peer: i32) -> bool {
    // A shared world rank prevents cycles without making account age a lifetime leadership rule.
    let rank = |account| (persona::seed(&format!("{world}:{account}")), account);
    rank(peer) < rank(account)
}

fn visible(
    bands: &bands::Bands,
    v: &PlayerView,
    peer: (i32, i32),
) -> std::result::Result<Pos, &'static str> {
    let p = cooperation::visible(v, peer)?;
    if bands.survivable(v, p) {
        Ok(p)
    } else {
        Err("unsafe_target")
    }
}
