//! Players go where the people are: a crowd on the minimap beats exploring alone.
use super::*;
use crate::world::view::PlayerMarker;
#[cfg(test)]
mod tests;

pub(super) const FOLLOW_REACH: f32 = 4.5;
const CROWD_RANGE: f32 = 12.;
const SOLO_GATHER_RANGE: f32 = 40.;
const SCHOOL_RANGE: f32 = 30.;

pub(super) struct Train {
    pub members: Vec<(i32, Pos)>,
    pub centre: Pos,
    pub aboard: bool,
}
pub(super) struct Crowd {
    pub knows_minimap: bool,
    pub bands: bands::Bands,
    gathered_peer: Option<i32>,
    /// When and where each located player was last sampled, and their velocity since the sample before, in tiles a second.
    motion: BTreeMap<i32, (u64, Pos, Pos)>,
}
impl Crowd {
    pub fn new(knows_minimap: bool) -> Self {
        Self {
            knows_minimap,
            bands: bands::Bands::default(),
            gathered_peer: None,
            motion: BTreeMap::new(),
        }
    }
    /// The velocity of a player walking somewhere: circling a fight nets little ground, and a teleport covers too much.
    pub fn walking(&self, id: i32) -> Option<Pos> {
        self.motion
            .get(&id)
            .map(|m| m.2)
            .filter(|u| (2.5..=12.).contains(&u.x.hypot(u.y)))
    }
    /// Other players this player can locate: the whole minimap for those who read it, otherwise the screen.
    fn markers<'a>(&self, v: &'a PlayerView) -> impl Iterator<Item = &'a PlayerMarker> {
        let minimap = self.knows_minimap;
        v.player_markers
            .iter()
            .filter(move |p| minimap || v.objects.iter().any(|o| o.id == p.id && o.hp > 0))
    }
    /// Players gathered around a position.
    pub fn near(&self, v: &PlayerView, position: Pos) -> usize {
        self.markers(v)
            .filter(|p| p.position.distance(position) < CROWD_RANGE)
            .count()
    }
    pub fn observe_gathering(&mut self, v: &PlayerView, goal: &Goal) {
        let located: Vec<_> = self.markers(v).map(|p| (p.id, p.position)).collect();
        self.motion
            .retain(|id, _| located.iter().any(|(peer, _)| peer == id));
        for (id, position) in located {
            let (at, from, velocity) =
                self.motion
                    .entry(id)
                    .or_insert((v.time_ms, position, Pos { x: 0., y: 0. }));
            if v.time_ms >= *at + 2000 {
                let seconds = (v.time_ms - *at) as f32 / 1000.;
                *velocity = Pos {
                    x: (position.x - from.x) / seconds,
                    y: (position.y - from.y) / seconds,
                };
                (*at, *from) = (v.time_ms, position);
            }
        }
        if self.gathered_peer.is_some_and(|id| {
            !self
                .markers(v)
                .any(|p| p.id == id && p.position.distance(v.position) <= SOLO_GATHER_RANGE)
        }) {
            self.gathered_peer = None;
        }
        // Reaching a peer finishes the approach to their crowd, not just to that marker.
        if let Goal::Follow(id) = goal
            && self
                .markers(v)
                .any(|p| p.id == *id && p.position.distance(v.position) < FOLLOW_REACH)
        {
            self.gathered_peer = Some(*id);
        }
    }
    /// The crowd worth joining and its size. Cautious players only teleport where they know they can survive;
    /// ambitious ones trust any crowd of three, which is how fame trains drew in players below their level.
    pub fn gathering<'a>(
        &self,
        v: &'a PlayerView,
        goal: &Goal,
        ambitious: bool,
        leading: bool,
        eligible: impl Fn(Pos) -> bool,
    ) -> Option<(&'a PlayerMarker, usize)> {
        if v.map.template.as_deref() != Some("realm") {
            return None;
        }
        let here = self.bands.known(v.position);
        let markers: Vec<_> = self.markers(v).collect();
        let gathered = markers.iter().find(|p| Some(p.id) == self.gathered_peer);
        let following = |p: &PlayerMarker| *goal == Goal::Follow(p.id);
        // Fish go the way their neighbours go: a player walking ahead is followed, and draws harder the more who walk its way.
        let school = |p: &PlayerMarker| {
            let u = self.walking(p.id)?;
            let near = |q: &PlayerMarker| q.position.distance(v.position) < SCHOOL_RANGE;
            let ahead =
                (p.position.x - v.position.x) * u.x + (p.position.y - v.position.y) * u.y > 0.;
            (near(p) && ahead).then(|| {
                1 + markers
                    .iter()
                    .filter(|q| {
                        near(q)
                            && self.walking(q.id).is_some_and(|w| {
                                u.x * w.x + u.y * w.y > 0.5 * u.x.hypot(u.y) * w.x.hypot(w.y)
                            })
                    })
                    .count()
            })
        };
        // Standing among others is already the crowd; walking to its far side only shuffles it.
        let settled = leading || self.near(v, v.position) >= 2;
        markers
            .iter()
            .map(|p| {
                let moving = school(p);
                let crowd = moving.unwrap_or_else(|| {
                    markers
                        .iter()
                        .filter(|q| q.position.distance(p.position) < CROWD_RANGE)
                        .count()
                });
                (*p, crowd, moving.is_some())
            })
            .filter(|(p, crowd, moving)| {
                let distance = p.position.distance(v.position);
                // Live: lone targets moved on, so bots chained after each other instead of clustering.
                let crowded = *crowd >= 2;
                // A crowd nearby is usually in the player's own area, even on ground it has not seen.
                let own_area = crowded && distance <= 60. && self.bands.survivable(v, p.position);
                let approaching = following(p) && distance >= FOLLOW_REACH;
                (approaching || distance > 7.)
                    && (*moving
                        || (!settled || following(p))
                            && (crowded || distance <= SOLO_GATHER_RANGE)
                            && gathered.is_none_or(|q| {
                                q.position.distance(p.position) >= CROWD_RANGE + FOLLOW_REACH
                            }))
                    && eligible(p.position)
                    && self.bands.worthwhile(p.position)
                    && (self.bands.teleportable(v, p.position)
                        || own_area
                        || ambitious && *crowd >= 3)
            })
            .max_by(|(a, x, m), (b, y, n)| {
                // Bigger crowds first, then the player's own area, then the nearer one.
                let score = |p: &PlayerMarker, crowd: usize| {
                    crowd as f32 * 10.
                        + if self.bands.known(p.position) == here {
                            8.
                        } else {
                            0.
                        }
                        - p.position.distance(v.position) / 20.
                };
                // Moving peers can trade first place every tick while both are worth joining.
                m.cmp(n)
                    .then(following(a).cmp(&following(b)))
                    .then_with(|| score(a, *x).total_cmp(&score(b, *y)))
                    .then(b.id.cmp(&a.id))
            })
            .map(|(p, crowd, _)| (p, crowd))
    }
    /// Players heading for a Realm quest teleport to someone outside the fight, then walk in.
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
                // Live: landing within 16 tiles killed level 20s within 2 seconds; Cube God minions reach that far.
                (20. ..=30.).contains(&p.position.distance(objective))
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
    /// Godlands crowds linked hand to hand, this player included, biggest first. Equal batches go to the one holding
    /// the lowest id, so every farmer agrees which batch is the train without anyone being made its leader.
    pub fn train(&self, v: &PlayerView) -> Train {
        let mut left: Vec<_> = self
            .markers(v)
            .filter(|p| bands::in_godlands(v, p.position))
            .map(|p| (p.id, p.position))
            .chain([(v.session.player, v.position)])
            .collect();
        let mut batches = vec![];
        while let Some(first) = left.pop() {
            let mut batch = vec![first];
            let mut linked = 0;
            while linked < batch.len() {
                let at = batch[linked].1;
                let near;
                (near, left) = left
                    .into_iter()
                    .partition(|q| q.1.distance(at) < CROWD_RANGE);
                batch.extend(near);
                linked += 1;
            }
            batches.push(batch);
        }
        let members = batches
            .into_iter()
            .min_by_key(|b| (std::cmp::Reverse(b.len()), b.iter().map(|p| p.0).min()))
            .expect("the player is always in a batch");
        let count = members.len() as f32;
        Train {
            centre: Pos {
                x: members.iter().map(|p| p.1.x).sum::<f32>() / count,
                y: members.iter().map(|p| p.1.y).sum::<f32>() / count,
            },
            aboard: members.iter().any(|p| p.0 == v.session.player),
            members,
        }
    }
    /// A farmer away from the train teleports into its middle.
    pub fn godlands<'a>(
        &self,
        v: &'a PlayerView,
        train: &Train,
        clear: impl Fn(Pos) -> bool,
    ) -> Option<&'a PlayerMarker> {
        if !self.knows_minimap
            || !v.map.allow_teleport
            || train.aboard
            || train.centre.distance(v.position) <= 24.
        {
            return None;
        }
        v.player_markers
            .iter()
            .filter(|p| {
                let (x, y) = tile(p.position);
                train.members.iter().any(|m| m.0 == p.id)
                    && self.bands.teleportable(v, p.position)
                    && clear(p.position)
                    // The destination can move before teleport is processed; leave room on safe Godlands ground.
                    && (x - 3..=x + 3).all(|x| {
                        (y - 3..=y + 3).all(|y| v.map.godland_tiles.contains(&(x, y)))
                    })
            })
            .min_by(|a, b| {
                a.position
                    .distance(train.centre)
                    .total_cmp(&b.position.distance(train.centre))
                    .then(a.id.cmp(&b.id))
            })
    }
    pub fn shortcut<'a>(&self, v: &'a PlayerView, objective: Pos) -> Option<&'a PlayerMarker> {
        if !v.map.allow_teleport
            || matches!(v.map.template.as_deref(), Some("realm") | Some("nexus"))
        {
            return None;
        }
        // Rushers shorten the existing dungeon quest: https://anticosmic.blogspot.com/2012/04/fame-farming.html#tunnel_rat
        self.markers(v)
            .filter(|p| {
                p.position.distance(v.position) > 24.
                    && p.position.distance(objective) + 16. < v.position.distance(objective)
            })
            .min_by(|a, b| {
                a.position
                    .distance(objective)
                    .total_cmp(&b.position.distance(objective))
            })
    }
}
