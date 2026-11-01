use super::*;

pub(super) struct Role {
    pub kind: u16,
    pub maximum: usize,
    pub initial: usize,
    pub interval: Option<u64>,
}
pub(super) struct Family {
    pub leader: u16,
    pub terrain: u8,
    pub roles: &'static [Role],
    pub random_initial: bool,
}
impl Family {
    pub(super) fn owner(&self, kind: u16) -> u16 {
        match (self.leader, kind) {
            (0x640, 0x63a..=0x63c) => 0x63e,
            (0x650, 0x64a..=0x64d) => 0x64f,
            (0x206, 0x206) => 0x209,
            _ => self.leader,
        }
    }
    pub(super) fn size(&self) -> usize {
        1 + self.roles.iter().map(|r| r.maximum).sum::<usize>()
    }
    fn roll_initial(&self, rng: &mut impl Rng) -> Vec<usize> {
        self.roles
            .iter()
            .map(|r| {
                if self.random_initial {
                    rng.gen_range(r.initial..=r.maximum)
                } else {
                    r.initial
                }
            })
            .collect()
    }
}
macro_rules! family {
    (@script $leader:expr,$terrain:expr; $( $kind:expr,$maximum:expr );+ $(;)?) => {
        Family{leader:$leader,terrain:$terrain,random_initial:false,roles:&[$(Role{kind:$kind,maximum:$maximum,initial:0,interval:None}),+]}
    };
    (@random $leader:expr,$terrain:expr; $( $kind:expr,$maximum:expr,$initial:expr,$interval:expr );+ $(;)?) => {
        Family { leader:$leader, terrain:$terrain, random_initial:true, roles:&[$(Role {kind:$kind,maximum:$maximum,initial:$initial,interval:Some($interval)}),+] }
    };
    (@once $leader:expr,$terrain:expr; $( $kind:expr,$count:expr );+ $(;)?) => {
        Family {leader:$leader,terrain:$terrain,random_initial:false,roles:&[$(Role{kind:$kind,maximum:$count,initial:$count,interval:None}),+]}
    };
    ($leader:expr,$terrain:expr; $( $kind:expr,$maximum:expr,$initial:expr,$interval:expr );+ $(;)?) => {
        Family { leader:$leader, terrain:$terrain, random_initial:false, roles:&[$(Role {kind:$kind,maximum:$maximum,initial:$initial,interval:Some($interval)}),+] }
    };
}
pub(super) const FAMILIES: &[Family] = &[
    family!(@once 0x601,11; 0x600,2),
    family!(0x604,11; 0x603,10,5,10000),
    family!(0x953,11; 0x954,5,3,8000),
    family!(0x60d,8; 0x60a,2,2,10000; 0x60c,3,3,8000),
    family!(0x617,9; 0x616,4,2,12000; 0x615,3,2,6000),
    family!(0x61a,9; 0x619,4,2,12000; 0x618,3,2,6000),
    family!(0x609,10; 0x605,2,2,15000; 0x606,4,4,7000; 0x607,1,1,8000),
    family!(0x614,10; 0x612,7,3,12000; 0x613,7,3,12000),
    family!(0x652,1; 0x653,5,3,10000),
    family!(@once 0x905,1; 0x906,1; 0x907,1),
    family!(0x62c,6; 0x629,3,2,10000; 0x62a,2,1,10000; 0x62b,2,1,10000),
    family!(0x61f,7; 0x61c,3,2,10000; 0x61d,2,1,10000; 0x61e,2,1,10000),
    family!(0x649,2; 0x647,4,3,10000; 0x648,3,2,10000),
    family!(0x640,3; 0x63e,1,1,10000; 0x63f,1,1,10000; 0x63a,3,2,10000; 0x63b,3,2,10000; 0x63c,3,2,10000),
    family!(@once 0x900,3; 0x901,1; 0x902,2),
    family!(@once 0x645,4; 0x642,2; 0x643,4; 0x644,2),
    family!(0x650,4; 0x64f,1,1,10000; 0x64a,4,2,10000; 0x64b,3,2,10000; 0x64c,2,1,10000; 0x64d,2,1,10000),
    family!(@once 0x632,5; 0x6d2,1; 0x6d5,1; 0x6d6,1; 0x6d7,1),
    family!(@once 0x633,5; 0x6d2,1; 0x6d5,1; 0x6d6,1; 0x6d7,1),
    Family {
        leader: 0x62d,
        terrain: 7,
        random_initial: false,
        roles: &[
            Role {
                kind: 0x62d,
                maximum: 2,
                initial: 0,
                interval: Some(10000),
            },
            Role {
                kind: 0x6d2,
                maximum: 1,
                initial: 1,
                interval: None,
            },
            Role {
                kind: 0x6d5,
                maximum: 1,
                initial: 1,
                interval: None,
            },
            Role {
                kind: 0x6d6,
                maximum: 1,
                initial: 1,
                interval: None,
            },
            Role {
                kind: 0x6d7,
                maximum: 1,
                initial: 1,
                interval: None,
            },
        ],
    },
    family!(@once 0x62e,7; 0x6d2,1; 0x6d5,1; 0x6d6,1; 0x6d7,1),
    family!(@once 0x62f,6; 0x6d2,1; 0x6d5,1; 0x6d6,1; 0x6d7,1),
    family!(@once 0x631,6; 0x6d2,1; 0x6d5,1; 0x6d6,1; 0x6d7,1),
    family!(@random 0x623,5; 0x622,3,1,10000; 0x620,3,1,10000),
    family!(@random 0x947,6; 0x949,2,1,10000; 0x948,5,2,10000),
    family!(@once 0x945,7; 0x946,1),
    family!(@random 0x637,7; 0x635,3,1,10000; 0x636,2,1,10000; 0x639,2,1,10000),
    family!(0x206,4; 0x206,2,0,60000),
    family!(@script 0x20e,6; 0x20f,5),
    family!(@script 0x950,6; 0x951,5),
    family!(@script 0x957,3; 0x958,3; 0x959,4; 0x960,4; 0x961,4),
    family!(@once 0x903,2; 0x904,5),
    family!(@script 0x7f00,9; 0x7f01,5),
];
pub(super) struct Camp {
    pub start: usize,
    pub family: &'static Family,
    next: Vec<Option<u64>>,
    births: Vec<usize>,
}

fn formation(center: Pos, size: usize) -> Vec<Pos> {
    (0..size)
        .map(|i| {
            if i == 0 {
                center
            } else {
                let angle = (i - 1) as f32 * std::f32::consts::TAU / (size - 1) as f32;
                Pos {
                    x: center.x + 2. * angle.cos(),
                    y: center.y + 2. * angle.sin(),
                }
            }
        })
        .collect()
}

fn member_positions(center: Pos, terrain: u8) -> Vec<Pos> {
    let inland = (2..=7).contains(&terrain);
    let angles = if inland { 16 } else { 32 };
    let rings = if inland { 4 } else { 1 };
    (0..angles * rings)
        .map(|i| {
            let angle = (i % angles) as f32 * std::f32::consts::TAU / angles as f32;
            let radius = if inland { (1 + i / angles) as f32 } else { 2. };
            Pos {
                x: center.x + radius * angle.cos(),
                y: center.y + radius * angle.sin(),
            }
        })
        .collect()
}
/// Kithio Reproduce(children, densityRadius, densityMax, coolDown) on ordinary Realm parents: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Reproduce.cs
// Golem and Swarm entries count themselves against densityMax 1, so they never fire.
pub(super) const REPRODUCTION: &[(u16, u16, f32, usize, u64)] = &[
    (0x601, 0x600, 10., 5, 60000),
    (0x601, 0x601, 10., 5, 60000),
    (0x602, 0x602, 10., 5, 60000),
    (0x604, 0x604, 40., 2, 60000),
    (0x60e, 0x60e, 10., 5, 60000),
    (0x60f, 0x60f, 10., 5, 60000),
    (0x610, 0x610, 10., 5, 60000),
    (0x624, 0x624, 10., 2, 60000),
    (0x625, 0x625, 10., 2, 60000),
    (0x626, 0x626, 10., 2, 60000),
    (0x205, 0x205, 10., 5, 60000),
    (0x20d, 0x20d, 10., 5, 60000),
    (0x20e, 0x20e, 10., 5, 60000),
    (0x204, 0x204, 20., 5, 20000),
    (0x202, 0x202, 15., 3, 45000),
    (0x649, 0x649, 10., 2, 60000),
    (0x651, 0x651, 10., 3, 60000),
    (0x652, 0x652, 10., 2, 60000),
    (0x654, 0x654, 10., 3, 60000),
    (0x655, 0x655, 10., 3, 60000),
    (0x656, 0x656, 10., 3, 60000),
    (0x657, 0x657, 10., 3, 60000),
    (0x658, 0x658, 10., 2, 60000),
    (0x659, 0x659, 10., 3, 60000),
    (0x91a, 0x91a, 10., 4, 60000),
];

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reproduction {
    Density,
    NoSite,
    Born(i32),
}

impl World {
    pub(super) fn reproduce(&mut self, id: i32, delta: u64) -> Result<()> {
        let e = self
            .entities
            .get(&id)
            .context("reproducing parent missing")?;
        // Gray Blob reproduces only while searching and Djinn only while Idle.
        if e.population_slot.is_none()
            || e.kind == 0x20d && e.stage == Some(1)
            || e.kind == 0x91a && self.djinn_attacking.contains(&id)
        {
            return Ok(());
        }
        let (kind, center) = (e.kind, e.pos);
        for (entry, &(_, child, radius, maximum, cooldown)) in
            REPRODUCTION.iter().enumerate().filter(|(_, r)| r.0 == kind)
        {
            let remaining = self.reproduction.entry((id, entry)).or_insert(cooldown);
            // Reproduce checks the stored clock before subtracting the active tick, with no catch-up bursts.
            if *remaining > 0 {
                *remaining = remaining.saturating_sub(delta);
                continue;
            }
            *remaining = cooldown;
            let outcome = self.try_reproduce(child, center, radius, maximum)?;
            tracing::debug!(
                entity = id,
                kind,
                child,
                ?outcome,
                "ordinary reproduction evaluated"
            );
        }
        Ok(())
    }

    pub(super) fn reproduce_active(&mut self) {
        // Each tick advances one twentieth of the parents, so the per-second pass never lands on one tick.
        let bucket = self.reproduction_bucket;
        self.reproduction_bucket = (bucket + 1) % self.reproduction_at.len();
        let delta = self.now - self.reproduction_at[bucket];
        self.reproduction_at[bucket] = self.now;
        // Kithio ticks behaviors only in 16-tile chunks within three chunks of a player: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/Collision.cs#L187-L213
        let chunk = |p: Pos| ((p.x / 16.) as i32, (p.y / 16.) as i32);
        let players: Vec<_> = self.players.values().map(|p| chunk(p.pos)).collect();
        let parents: Vec<_> = self
            .population
            .iter()
            .filter_map(|slot| slot.entity)
            .filter(|id| id.rem_euclid(20) as usize == bucket)
            .filter(|id| {
                let e = &self.entities[id];
                let (x, y) = chunk(e.pos);
                REPRODUCTION.iter().any(|r| r.0 == e.kind)
                    && players
                        .iter()
                        .any(|(px, py)| (x - px).abs() <= 3 && (y - py).abs() <= 3)
            })
            .collect();
        for id in parents {
            if self.entities.contains_key(&id)
                && let Err(error) = self.reproduce(id, delta)
            {
                tracing::error!(entity = id, %error, "ordinary reproduction refused");
            }
        }
    }

    fn try_reproduce(
        &mut self,
        kind: u16,
        center: Pos,
        radius: f32,
        maximum: usize,
    ) -> Result<Reproduction> {
        // CountEntity includes the parent and every same-type enemy, not just its camp or spawn group.
        if self
            .actors_near(center, radius)
            .filter(|e| e.kind == kind)
            .count()
            >= maximum
        {
            return Ok(Reproduction::Density);
        }
        let terrain = self.content.get(kind)?.terrain;
        // Piratess pirates stand in for Kithio's Oryx-table Pirates, not a Piratess Spawn, so reproduced Piratesses arrive alone.
        let family = FAMILIES.iter().find(|f| f.leader == kind && kind != 0x601);
        for _ in 0..10 {
            let angle = self.rng.gen_range(0.0..std::f32::consts::TAU);
            let pos = Pos {
                x: center.x + radius / 2. * angle.cos(),
                y: center.y + radius / 2. * angle.sin(),
            };
            if !self.family_ground(pos, terrain) {
                continue;
            }
            let positions = match family {
                Some(family) => self.family_layout(pos, family, &[]),
                None => Some(vec![pos]),
            };
            let Some(positions) = positions else {
                continue;
            };
            let id = match family {
                Some(family) => {
                    let slot = self.population.len();
                    self.place_camp(family, positions)?;
                    self.population[slot]
                        .entity
                        .context("new family lacks its root")?
                }
                None => self.place_population_solo(kind, pos)?,
            };
            return Ok(Reproduction::Born(id));
        }
        Ok(Reproduction::NoSite)
    }

    pub(super) fn camp_members(&self, camp: usize) -> impl Iterator<Item = &Entity> {
        let c = self.camps.get(camp).expect("ordinary camp index missing");
        self.population[c.start..c.start + c.family.size()]
            .iter()
            .enumerate()
            .filter_map(move |(offset, slot)| {
                assert_eq!(slot.camp, Some(camp), "population camp range changed");
                slot.entity.map(|id| {
                    let e = self.entities.get(&id).expect("live camp member missing");
                    assert_eq!(e.camp, Some(camp), "entity camp ownership changed");
                    assert_eq!(
                        e.population_slot,
                        Some(c.start + offset),
                        "entity population slot changed"
                    );
                    e
                })
            })
    }
    pub(super) fn camp_member(&self, id: i32, kind: u16) -> Result<Option<i32>> {
        let e = self
            .entities
            .get(&id)
            .context("camp member entity missing")?;
        let camp = e.camp.context("entity has no ordinary camp")?;
        let c = self
            .camps
            .get(camp)
            .context("ordinary camp index missing")?;
        Ok(self.population[c.start..c.start + c.family.size()]
            .iter()
            .filter(|s| s.kind == kind)
            .filter_map(|s| s.entity)
            .filter(|id| self.entities.contains_key(id))
            .min_by(|a, b| {
                self.entities[a]
                    .pos
                    .distance(e.pos)
                    .total_cmp(&self.entities[b].pos.distance(e.pos))
            }))
    }

    pub(super) fn population_terrain(&self, slot: &Spawn) -> u8 {
        match slot.camp {
            Some(camp) => self.camps[camp].family.terrain,
            None => self.content.objects[&slot.kind].terrain,
        }
    }
    pub(super) fn population_count(&self, terrain: u8) -> usize {
        self.population_counts[terrain as usize]
    }
    pub(super) fn family_ground(&self, pos: Pos, terrain: u8) -> bool {
        self.map
            .cell(pos.x, pos.y)
            .is_some_and(|c| c.terrain == terrain && c.object.is_none())
            && self.walkable(pos)
            && self.outside_setpieces(pos)
            && self
                .entities_near(pos, 0.75)
                .all(|e| e.population_slot.is_none())
    }
    pub(super) fn place_member(&mut self, camp: usize, slot: usize, pos: Pos) -> Result<()> {
        let kind = if matches!(self.population[slot].kind, 0x6d2..=0x6d4) {
            self.rng.gen_range(0x6d2..=0x6d4)
        } else {
            self.population[slot].kind
        };
        self.population[slot].kind = kind;
        let id = self.spawn_entity(kind, pos, vec![])?;
        let home = self.population[self.camps[camp].start].pos;
        let e = self.entities.get_mut(&id).unwrap();
        // Group affiliation cannot use summon parenthood, which deletes surviving ordinary followers.
        e.camp = Some(camp);
        e.home = home;
        self.population[slot].entity = Some(id);
        self.mark_population(id, slot)?;
        tracing::debug!(camp, entity = id, kind, "family member created");
        Ok(())
    }
    pub(super) fn validate_family(&self, family: &Family) -> Result<()> {
        for kind in std::iter::once(family.leader).chain(family.roles.iter().map(|r| r.kind)) {
            ensure!(
                self.content.get(kind)?.terrain == family.terrain
                    || (self.content.get(kind)?.terrain == 0
                        && matches!(
                            kind,
                            0x6d2..=0x6d7 | 0x951 | 0x958 | 0x959 | 0x960 | 0x961 | 0x7f01
                        )
                        && kind != family.leader),
                "family contradicts XML habitat"
            );
            crate::encounter::validate(&self.content, kind)?;
            crate::loot::validate(&self.content, kind)?;
        }
        ensure!(
            family
                .roles
                .iter()
                .all(|r| r.initial <= r.maximum && r.interval.is_none_or(|interval| interval > 0)),
            "invalid family role bounds"
        );
        ensure!(family.size() <= 32, "family exceeds bounded slot count");
        Ok(())
    }
    pub(super) fn family_layout(
        &self,
        center: Pos,
        family: &Family,
        occupied: &[Pos],
    ) -> Option<Vec<Pos>> {
        if !(2..=7).contains(&family.terrain) {
            let positions = formation(center, family.size());
            return positions
                .iter()
                .all(|p| {
                    self.family_ground(*p, family.terrain)
                        && occupied.iter().all(|q| p.distance(*q) >= 0.75)
                })
                .then_some(positions);
        }
        // Thin, wooded inland habitats need a contour-aware footprint rather than a mandatory empty circle.
        let mut positions = vec![center];
        for p in member_positions(center, family.terrain) {
            if self.family_ground(p, family.terrain)
                && positions.iter().all(|q| p.distance(*q) >= 0.75)
                && occupied.iter().all(|q| p.distance(*q) >= 0.75)
            {
                positions.push(p);
            }
            if positions.len() == family.size() {
                return Some(positions);
            }
        }
        None
    }
    pub(super) fn place_camp(
        &mut self,
        family: &'static Family,
        positions: Vec<Pos>,
    ) -> Result<()> {
        ensure!(
            positions.len() == family.size(),
            "family footprint does not match reserved capacity"
        );
        let initials = family.roll_initial(&mut self.rng);
        let initial_count = 1 + initials.iter().sum::<usize>();
        let camp = self.camps.len();
        let start = self.population.len();
        self.camps.push(Camp {
            start,
            family,
            births: initials.clone(),
            next: family
                .roles
                .iter()
                .map(|r| r.interval.map(|interval| self.now + interval))
                .collect(),
        });
        let members: Vec<_> = std::iter::once((family.leader, true))
            .chain(
                family
                    .roles
                    .iter()
                    .zip(&initials)
                    .flat_map(|(r, n)| (0..r.maximum).map(move |i| (r.kind, i < *n))),
            )
            .collect();
        for ((kind, initial), pos) in members.into_iter().zip(positions) {
            let slot = self.population.len();
            self.population.push(Spawn {
                kind,
                pos,
                entity: None,
                camp: Some(camp),
            });
            if initial {
                self.place_member(camp, slot, pos)?;
            }
        }
        tracing::debug!(
            camp,
            leader = family.leader,
            terrain = family.terrain,
            initial = initial_count,
            capacity = family.size(),
            "bounded family allocated"
        );
        Ok(())
    }

    pub(super) fn repopulate_families(&mut self) -> Result<()> {
        // Only player-near owners reproduce; querying occupied cells avoids scanning dormant camps.
        let active: BTreeSet<_> = self
            .players
            .values()
            .filter(|p| p.character.alive && !p.death_pending && !p.departing && !p.paused)
            .flat_map(|p| self.actors_near(p.pos, 20.))
            .filter(|e| e.population_slot.is_some())
            .filter_map(|e| e.camp)
            .collect();
        for camp in active {
            if self.fauna_births.iter().any(|birth| birth.camp == camp) {
                continue;
            }
            let start = self.camps[camp].start;
            let family = self.camps[camp].family;
            let slots = start..start + family.size();
            let mut from = start + 1;
            for (role, r) in family.roles.iter().enumerate() {
                let to = from + r.maximum;
                let Some(interval) = r.interval else {
                    from = to;
                    continue;
                };
                // Spawn caps lifetime births, not survivors: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Spawn.cs#L30-L70
                if family.leader == 0x614 && self.camps[camp].births[role] >= r.maximum {
                    from = to;
                    continue;
                }
                let owner_kind = family.owner(r.kind);
                let owner = slots
                    .clone()
                    .filter_map(|i| self.population[i].entity)
                    .find(|id| self.entities.get(id).is_some_and(|e| e.kind == owner_kind));
                let Some(leader) = owner else {
                    from = to;
                    continue;
                };
                let center = self.entities[&leader].pos;
                if !self.players.values().any(|p| {
                    p.character.alive
                        && !p.death_pending
                        && !p.departing
                        && !p.paused
                        && p.pos.distance(center) < 20.
                }) {
                    from = to;
                    continue;
                }
                let mut deadline =
                    self.camps[camp].next[role].context("reinforcing role lacks its clock")?;
                if family.leader == 0x206 {
                    deadline = deadline.max(
                        self.entities[&leader]
                            .activated
                            .context("adult dragon lacks its growth clock")?
                            + interval,
                    );
                }
                if self.now >= deadline {
                    self.camps[camp].next[role] = Some(self.now + interval);
                    if let Some(slot) = (from..to).find(|i| self.population[*i].entity.is_none()) {
                        let pos = member_positions(center, family.terrain)
                            .into_iter()
                            .find(|p| {
                                self.family_ground(*p, family.terrain)
                                    && self.players.values().all(|q| q.pos.distance(*p) >= 0.75)
                                    && self
                                        .actors()
                                        .filter(|e| e.camp == Some(camp))
                                        .all(|e| e.pos.distance(*p) >= 0.75)
                            });
                        if let Some(pos) = pos {
                            self.place_member(camp, slot, pos)?;
                            self.camps[camp].births[role] += 1;
                            tracing::debug!(
                                camp,
                                leader,
                                role,
                                kind = r.kind,
                                interval,
                                births = self.camps[camp].births[role],
                                "family role reinforced"
                            );
                        } else {
                            tracing::debug!(
                                camp,
                                role,
                                "reinforcement refused: no clear habitat position"
                            );
                        }
                    }
                }
                from = to;
            }
        }
        Ok(())
    }
}
