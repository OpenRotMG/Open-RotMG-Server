//! Realm danger judged from remembered encounters with enemies' content-defined terrain bands.
use super::*;

const REGION: f32 = 8.;

#[derive(Default)]
pub(super) struct Bands {
    world: Option<i32>,
    regions: BTreeMap<TilePoint, (u8, Pos)>,
    limit: u8,
    pub snake_source: Option<Pos>,
}

/// Danger tier of a content terrain band: shore 0, low 1, mid 2, high 3, godlands 4.
fn tier(terrain: u8) -> u8 {
    match terrain {
        1 => 4,
        2..=4 => 3,
        5..=7 => 2,
        8..=10 => 1,
        _ => 0,
    }
}
fn suitable(v: &PlayerView, c: &Content) -> u8 {
    match v.character.level {
        20 if events::ready(v, c) => 4,
        15.. => 3,
        10.. => 2,
        _ => 1,
    }
}
fn region(p: Pos) -> TilePoint {
    ((p.x / REGION).floor() as i16, (p.y / REGION).floor() as i16)
}
impl Bands {
    pub fn observe(&mut self, v: &PlayerView, c: &Content) {
        if self.world != Some(v.session.world) {
            *self = Self {
                world: Some(v.session.world),
                ..Self::default()
            };
        }
        self.limit = suitable(v, c);
        if v.map.name != "Realm" {
            return;
        }
        for enemy in v
            .objects
            .iter()
            .filter(|o| o.hp > 0 && c.objects[&o.kind].enemy)
        {
            let tier = tier(c.objects[&enemy.kind].terrain);
            let entry = self
                .regions
                .entry(region(enemy.position))
                .or_insert((tier, enemy.position));
            if tier >= entry.0 {
                *entry = (tier, enemy.position);
            }
            if crate::dungeon::portal_drop(enemy.kind).is_some_and(|(portal, _)| portal == 0x718) {
                self.snake_source = Some(enemy.position);
            }
        }
    }
    /// Highest band remembered around a position; `None` is unobserved ground.
    pub fn known(&self, p: Pos) -> Option<u8> {
        let (x, y) = region(p);
        (x - 1..=x + 1)
            .flat_map(|x| (y - 1..=y + 1).map(move |y| (x, y)))
            .filter_map(|r| self.regions.get(&r))
            .map(|(tier, _)| *tier)
            .max()
    }
    /// Walking may enter unobserved ground; `None` orders below every band.
    pub fn survivable(&self, v: &PlayerView, p: Pos) -> bool {
        v.map.name != "Realm" || self.known(p) <= Some(self.limit)
    }
    /// A far marker may stand among enemies never seen, so low levels only teleport onto observed ground.
    pub fn teleportable(&self, v: &PlayerView, p: Pos) -> bool {
        self.survivable(v, p)
            && (v.map.name != "Realm" || v.character.level >= 20 || self.known(p).is_some())
    }
    /// Exploring from suited ground stays on it; otherwise Venture pulls the bot back across the same region edge every decision.
    pub fn explorable(&self, v: &PlayerView, p: Pos) -> bool {
        let suited = Some(self.limit);
        self.survivable(v, p)
            && (v.map.name != "Realm"
                || self.known(v.position) != suited
                || self.known(p) == suited)
    }
    /// Where to farm next: back out of a band above this character, or up toward its suitable band.
    pub fn venture(&self, v: &PlayerView) -> Option<Pos> {
        let (here, goal) = (self.known(v.position), self.limit);
        if here == Some(goal) {
            return None;
        }
        self.regions
            .values()
            .filter(|(tier, p)| {
                *tier <= goal
                    && (here > Some(goal) || Some(*tier) > here)
                    && self.known(*p) <= Some(goal)
                    && p.distance(v.position) > 8.
            })
            .max_by(|(a, p), (b, q)| {
                a.cmp(b)
                    .then_with(|| q.distance(v.position).total_cmp(&p.distance(v.position)))
            })
            .map(|(_, p)| *p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_level_refuses_observed_high_band_but_geared_veteran_may_go() {
        let (c, mut v) = quest_tests::fixture();
        let mut enemy = v.objects.remove(0);
        let high = c
            .objects
            .values()
            .find(|o| o.enemy && (2..=4).contains(&o.terrain))
            .unwrap();
        let far = Pos {
            x: v.position.x + 40.,
            y: v.position.y,
        };
        enemy.kind = high.kind;
        enemy.hp = 100;
        enemy.position = far;
        v.objects.push(enemy);
        let mut bands = Bands::default();
        v.character.level = 5;
        bands.observe(&v, &c);
        assert!(!bands.survivable(&v, far));
        assert!(
            bands.survivable(&v, v.position),
            "unobserved ground is not refused"
        );
        v.character.level = 15;
        bands.observe(&v, &c);
        assert!(bands.survivable(&v, far));
        assert_eq!(bands.venture(&v).map(|p| (p.x, p.y)), Some((far.x, far.y)));
    }
}
