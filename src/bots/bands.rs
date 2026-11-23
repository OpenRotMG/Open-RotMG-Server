//! Realm danger judged from remembered encounters with enemies' content-defined terrain bands.
use super::*;

const REGION: f32 = 8.;
/// Godlands sightings kept per Realm layout, each at least `SPACING` tiles from the others.
pub(crate) const REMEMBERED: usize = 16;
const SPACING: f32 = 24.;

#[derive(Default)]
pub(super) struct Bands {
    world: Option<i32>,
    regions: BTreeMap<TilePoint, (u8, Pos)>,
    pub limit: u8,
    pub snake_source: Option<Pos>,
}

/// Danger tier of a content terrain band: shore 0, low 1, mid 2, high 3, godlands 4.
pub(super) fn tier(terrain: u8) -> u8 {
    match terrain {
        1 => 4,
        2..=4 => 3,
        5..=7 => 2,
        8..=10 => 1,
        _ => 0,
    }
}
fn suitable(v: &PlayerView, identity: &str) -> u8 {
    let level = v.character.level;
    // Owner, Nov 2026: 90% of level 20s farm the Godlands, 10% fewer per level below, at least 5%.
    let chance = (0.9 - 0.1 * (20 - level) as f64).max(0.05);
    let roll = (super::persona::seed(&format!("{identity}:godlands:{level}")) >> 11) as f64
        / (1u64 << 53) as f64;
    match level {
        _ if roll < chance => 4,
        15.. => 3,
        10.. => 2,
        _ => 1,
    }
}
pub(super) fn in_godlands(v: &PlayerView, p: Pos) -> bool {
    v.map.godland_tiles.contains(&tile(p))
}
pub(super) fn near_godlands(v: &PlayerView, distance: f32) -> bool {
    let (x, y) = tile(v.position);
    let tiles = distance.ceil() as i16;
    (x - tiles..=x + tiles).any(|x| {
        v.map
            .godland_tiles
            .range((x, y - tiles)..=(x, y + tiles))
            .next()
            .is_some()
    })
}
/// Where a restless farmer walks next: the Godlands spot furthest along its course, turning round only at a dead end.
pub(super) fn leg(v: &PlayerView, course: f32, open: impl Fn(Pos) -> bool) -> Option<Pos> {
    let along = |p: &Pos| (p.x - v.position.x) * course.cos() + (p.y - v.position.y) * course.sin();
    let furthest = |reach: f32| {
        v.map
            .godland_route
            .iter()
            .filter(|p| (40. ..=reach).contains(&p.distance(v.position)) && open(**p))
            .max_by(|a, b| along(a).total_cmp(&along(b)))
            .copied()
    };
    let ahead = |reach| furthest(reach).filter(|p| along(p) > 0.);
    // Live: one Realm's Godlands lies in two pieces 145 tiles apart.
    ahead(64.).or_else(|| ahead(200.)).or_else(|| furthest(64.))
}
fn region(p: Pos) -> TilePoint {
    ((p.x / REGION).floor() as i16, (p.y / REGION).floor() as i16)
}
impl Bands {
    /// Returns whether `godlands` learned a new place on this layout.
    pub fn observe(
        &mut self,
        v: &PlayerView,
        c: &Content,
        godlands: &mut BTreeMap<String, BTreeSet<TilePoint>>,
        veteran: bool,
        identity: &str,
    ) -> bool {
        let mut learned = false;
        if self.world != Some(v.session.world) {
            *self = Self {
                world: Some(v.session.world),
                ..Self::default()
            };
            if let Some(layout) = &v.map.layout {
                // Live: 76% of Realm entries remembered no Godlands, though veterans had played these maps for years.
                if veteran
                    && godlands.get(layout).is_none_or(BTreeSet::is_empty)
                    && !v.map.godlands.is_empty()
                {
                    godlands.insert(
                        layout.clone(),
                        v.map.godlands.iter().map(|p| tile(*p)).collect(),
                    );
                    tracing::info!(
                        account = v.session.account,
                        layout,
                        known = v.map.godlands.len(),
                        "autonomous realm godlands known from earlier play"
                    );
                    learned = true;
                }
                if let Some(known) = godlands.get_mut(layout) {
                    let before = known.len();
                    known.retain(|t| in_godlands(v, center(*t)));
                    if known.len() != before {
                        tracing::info!(
                            account = v.session.account,
                            layout,
                            dropped = before - known.len(),
                            "autonomous realm godlands memory outside the Godlands forgotten"
                        );
                        learned = true;
                    }
                }
                let remembered = godlands.get(layout).map_or(0, BTreeSet::len);
                let sightings = godlands
                    .get(layout)
                    .into_iter()
                    .flatten()
                    .map(|t| center(*t));
                let landmarks = v
                    .map
                    .godlands
                    .iter()
                    .copied()
                    .filter(|_| veteran && v.character.level == 20);
                for p in sightings.chain(landmarks) {
                    self.regions.insert(region(p), (4, p));
                }
                tracing::info!(
                    account = v.session.account,
                    layout,
                    remembered,
                    "autonomous realm godlands recalled"
                );
            }
        }
        self.limit = suitable(v, identity);
        if v.map.name != "Realm" {
            return false;
        }
        for enemy in v
            .objects
            .iter()
            .filter(|o| o.hp > 0 && c.objects[&o.kind].enemy)
        {
            // Live: a god wandering into the highlands made bots there believe they stood in the Godlands.
            let tier = match tier(c.objects[&enemy.kind].terrain) {
                4 if !in_godlands(v, enemy.position) => 3,
                tier => tier,
            };
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
            if tier == 4
                && let Some(layout) = &v.map.layout
                && godlands.get(layout).is_none_or(|known| {
                    known.len() < REMEMBERED
                        && known
                            .iter()
                            .all(|t| center(*t).distance(enemy.position) >= SPACING)
                })
            {
                godlands
                    .entry(layout.clone())
                    .or_default()
                    .insert(tile(enemy.position));
                tracing::info!(
                    account = v.session.account,
                    layout,
                    position = ?enemy.position,
                    "autonomous realm godlands remembered"
                );
                learned = true;
            }
        }
        learned
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
    /// Ground at most one band below this character's own still pays; unobserved ground may.
    pub fn worthwhile(&self, p: Pos) -> bool {
        self.known(p).is_none_or(|band| band + 1 >= self.limit)
    }
    /// A far marker may stand among enemies never seen, so low levels only teleport onto observed ground.
    pub fn teleportable(&self, v: &PlayerView, p: Pos) -> bool {
        self.survivable(v, p)
            && (v.map.name != "Realm" || v.character.level >= 20 || self.known(p).is_some())
    }
    /// Where to farm next: back out of a band above this character, or up toward its suitable band.
    pub fn venture(&self, v: &PlayerView) -> Option<Pos> {
        let goal = self.limit;
        let farming_godlands = goal == 4 && v.character.level == 20;
        // Sightings across a band edge must not end a trip before the player reaches the Godlands.
        let here = if farming_godlands {
            if in_godlands(v, v.position) {
                Some(4)
            } else {
                self.known(v.position).map(|band| band.min(3))
            }
        } else {
            self.known(v.position)
        };
        if here == Some(goal) {
            return None;
        }
        self.regions
            .values()
            .filter(|(tier, p)| {
                *tier <= goal
                    && (here > Some(goal) || Some(*tier) > here)
                    && self.known(*p) <= Some(goal)
                    && (p.distance(v.position) > 8. || farming_godlands && *tier == 4)
            })
            .max_by(|(a, p), (b, q)| {
                a.cmp(b)
                    .then_with(|| q.distance(v.position).total_cmp(&p.distance(v.position)))
            })
            .or_else(|| {
                // Live: level 20s below Godlands gear remembered only Godlands and fought beach mobs; their band lies on the way up.
                self.regions
                    .values()
                    .filter(|(tier, p)| {
                        *tier > goal && here < Some(goal) && p.distance(v.position) > 8.
                    })
                    .min_by(|(_, p), (_, q)| {
                        p.distance(v.position).total_cmp(&q.distance(v.position))
                    })
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
        bands.observe(&v, &c, &mut BTreeMap::new(), false, "Tester");
        assert!(!bands.survivable(&v, far));
        assert!(
            bands.survivable(&v, v.position),
            "unobserved ground is not refused"
        );
        v.character.level = 15;
        bands.observe(&v, &c, &mut BTreeMap::new(), false, "Tester");
        assert!(bands.survivable(&v, far));
        assert_eq!(bands.venture(&v).map(|p| (p.x, p.y)), Some((far.x, far.y)));
    }

    #[test]
    fn godlands_draw_most_level_20s_and_fewer_each_level_below() {
        let (_, mut v) = quest_tests::fixture();
        for (level, share) in [(20, 0.9), (19, 0.8), (17, 0.6), (14, 0.3), (8, 0.05)] {
            v.character.level = level;
            let farmers = crate::bots::persona::PERSONAS
                .iter()
                .filter(|p| suitable(&v, p.identity) == 4)
                .count() as f64;
            let actual = farmers / crate::bots::persona::PERSONAS.len() as f64;
            assert!((actual - share).abs() < 0.08, "level {level}: {actual}");
        }
    }

    #[test]
    fn godlands_venture_uses_the_nearer_landmark_and_crosses_the_band_edge() {
        let (c, mut v) = quest_tests::fixture();
        let (destination, remembered) = ((17, 10), (85, 85));
        v.character.level = 20;
        v.map.layout = Some("layout".into());
        v.map.godlands = Arc::new(vec![center(destination)]);
        v.map.godland_tiles = Arc::new(BTreeSet::from([destination, remembered]));
        v.objects.clear();
        let mut memory = BTreeMap::from([("layout".into(), BTreeSet::from([remembered]))]);
        let mut bands = Bands::default();
        bands.observe(&v, &c, &mut memory, true, "Bayleaf");
        assert_eq!(bands.known(v.position), Some(4));
        assert_eq!(bands.venture(&v).map(tile), Some(destination));
        v.position = center(destination);
        assert!(bands.venture(&v).is_none());
    }

    #[test]
    fn a_god_wandering_outside_the_godlands_is_not_the_godlands() {
        let (c, mut v) = quest_tests::fixture();
        let mut god = v.objects.remove(0);
        god.kind = c
            .objects
            .values()
            .find(|o| o.enemy && o.terrain == 1)
            .unwrap()
            .kind;
        (god.hp, god.position) = (100, Pos { x: 90.5, y: 90.5 });
        v.objects.push(god);
        v.map.layout = Some("layout".into());
        v.map.godland_tiles = Arc::new(BTreeSet::from([(10, 10)]));
        let mut memory =
            BTreeMap::from([("layout".to_owned(), BTreeSet::from([(10, 10), (91, 91)]))]);
        let mut bands = Bands::default();
        assert!(bands.observe(&v, &c, &mut memory, false, "Tester"));
        assert_eq!(
            memory["layout"],
            BTreeSet::from([(10, 10)]),
            "memory outside the Godlands is forgotten"
        );
        assert_eq!(bands.known(Pos { x: 90.5, y: 90.5 }), Some(3));
        assert_eq!(bands.known(Pos { x: 10.5, y: 10.5 }), Some(4));
    }

    #[test]
    fn a_godlands_leg_keeps_its_course_crosses_a_gap_and_turns_only_at_the_end() {
        let (_, mut v) = quest_tests::fixture();
        let spot = |x: f32| Pos { x, y: 100.5 };
        v.map.godland_route = Arc::new([52.5, 124.5, 148.5, 252.5, 300.5].map(spot).to_vec());
        v.position = spot(100.5);
        let x = |leg: Option<Pos>| leg.map(|p| p.x);
        assert_eq!(x(leg(&v, 0., |_| true)), Some(148.5));
        assert_eq!(x(leg(&v, std::f32::consts::PI, |_| true)), Some(52.5));
        assert_eq!(x(leg(&v, 0., |p| p.x != 148.5)), Some(300.5));
        v.position = spot(300.5);
        assert_eq!(x(leg(&v, 0., |_| true)), Some(252.5));
    }
}
