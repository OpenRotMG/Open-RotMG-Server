//! Players go where the people are: a crowd on the minimap beats exploring alone.
use super::*;
use crate::world::view::PlayerMarker;
#[cfg(test)]
mod tests;

pub(super) struct Crowd {
    pub knows_minimap: bool,
    pub bands: bands::Bands,
}
impl Crowd {
    pub fn new(knows_minimap: bool) -> Self {
        Self {
            knows_minimap,
            bands: bands::Bands::default(),
        }
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
            .filter(|p| p.position.distance(position) < 12.)
            .count()
    }
    /// The crowd worth joining and its size. Cautious players only teleport where they know they can survive;
    /// ambitious ones trust any crowd of three, which is how fame trains drew in players below their level.
    pub fn gathering<'a>(
        &self,
        v: &'a PlayerView,
        ambitious: bool,
        eligible: impl Fn(Pos) -> bool,
    ) -> Option<(&'a PlayerMarker, usize)> {
        if v.map.name != "Realm" {
            return None;
        }
        let here = self.bands.known(v.position);
        let markers: Vec<_> = self.markers(v).collect();
        markers
            .iter()
            .map(|p| {
                let crowd = markers
                    .iter()
                    .filter(|q| q.position.distance(p.position) < 12.)
                    .count();
                (*p, crowd)
            })
            .filter(|(p, crowd)| {
                let distance = p.position.distance(v.position);
                // Live: lone targets moved on, so bots chained after each other instead of clustering.
                let crowded = *crowd >= 2;
                // A crowd nearby is usually in the player's own area, even on ground it has not seen.
                let own_area = crowded && distance <= 60. && self.bands.survivable(v, p.position);
                distance > 7.
                    && (crowded || distance <= 40.)
                    && eligible(p.position)
                    && self.bands.worthwhile(p.position)
                    && (self.bands.teleportable(v, p.position)
                        || own_area
                        || ambitious && *crowd >= 3)
            })
            .max_by(|(a, x), (b, y)| {
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
                score(a, *x).total_cmp(&score(b, *y)).then(b.id.cmp(&a.id))
            })
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
    pub fn shortcut<'a>(&self, v: &'a PlayerView, objective: Pos) -> Option<&'a PlayerMarker> {
        if !v.map.allow_teleport || matches!(v.map.name.as_str(), "Realm" | "Nexus") {
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
