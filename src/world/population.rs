use super::*;
use crate::content::game::CampRule;

pub(super) struct Camp {
    pub start: usize,
    pub family: Arc<CampRule>,
    pub terrain: u8,
    /// Last placed leader; members born or first woken after its death still belong to its encounter.
    pub leader: Option<i32>,
    next: Vec<Option<u64>>,
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
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reproduction {
    PlayerNearby,
    Density,
    NoSite,
    Born(i32),
}

impl World {
    pub(super) fn outside_player_view(&self, pos: Pos) -> bool {
        self.players
            .values()
            .all(|p| p.pos.distance(pos) >= PLAYER_VIEW_RADIUS)
    }

    pub(super) fn reproduce(&mut self, id: i32, delta: u64) -> Result<()> {
        let e = self
            .entities
            .get(&id)
            .context("reproducing parent missing")?;
        if e.population_slot.is_none() {
            return Ok(());
        }
        let (kind, center) = (e.kind, e.pos);
        let game = self
            .content
            .game
            .as_ref()
            .context("Reproduce requires linked GameData")?
            .clone();
        let Some(rules) = game.realm_rules()?.reproduction.get(&kind) else {
            return Ok(());
        };
        for (entry, rule) in rules.iter().enumerate() {
            let (child, radius, maximum, cooldown) =
                (rule.child, rule.radius, rule.maximum, rule.cooldown);
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

    pub(super) fn reproduce_active(&mut self) -> Result<()> {
        // Each tick advances one twentieth of the parents, so the per-second pass never lands on one tick.
        let bucket = self.reproduction_bucket;
        self.reproduction_bucket = (bucket + 1) % self.reproduction_at.len();
        let delta = self.now - self.reproduction_at[bucket];
        self.reproduction_at[bucket] = self.now;
        let rules = &self.realm_rules()?.reproduction;
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
                rules.contains_key(&e.kind)
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
        Ok(())
    }

    fn try_reproduce(
        &mut self,
        kind: u16,
        center: Pos,
        radius: f32,
        maximum: usize,
    ) -> Result<Reproduction> {
        // Offscreen regrowth follows this comparator; the owner selected our existing view radius, not its 30 tiles: https://github.com/kaos00723/betterSkillys/blob/069d4816f45d151c7df14be84d0d04ee11d31307/source/WorldServer/logic/behaviors/Reproduce.cs#L71
        if !self.outside_player_view(center) {
            return Ok(Reproduction::PlayerNearby);
        }
        // CountEntity includes the parent and every same-type enemy, not just its camp or spawn group.
        if self
            .actors_near(center, radius)
            .filter(|e| e.kind == kind)
            .count()
            >= maximum
        {
            return Ok(Reproduction::Density);
        }
        let terrain = self.population_habitat(kind)?;
        let (family, footprint) = self.population_placement(kind)?;
        for _ in 0..10 {
            let angle = self.rng.gen_range(0.0..std::f32::consts::TAU);
            let pos = Pos {
                x: center.x + radius / 2. * angle.cos(),
                y: center.y + radius / 2. * angle.sin(),
            };
            if !self.family_ground(pos, terrain) {
                continue;
            }
            let positions = self.family_layout(pos, terrain, footprint, &[]);
            let Some(positions) = positions else {
                continue;
            };
            if !positions.iter().all(|pos| self.outside_player_view(*pos)) {
                continue;
            }
            let id = match &family {
                Some(family) => {
                    let slot = self.population.len();
                    self.place_camp(family.clone(), terrain, positions)?;
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

    pub(super) fn population_terrain(&self, slot: &Spawn) -> u8 {
        match slot.camp {
            Some(camp) => self.camps[camp].terrain,
            None => self
                .population_habitat(slot.kind)
                .expect("registered population habitat"),
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
        let kind = self.population[slot].kind;
        let id = self.spawn_entity(kind, pos, vec![])?;
        let home = self.population[self.camps[camp].start].pos;
        let e = self.entities.get_mut(&id).unwrap();
        // Group affiliation cannot use summon parenthood, which deletes surviving ordinary followers.
        e.camp = Some(camp);
        e.home = home;
        self.population[slot].entity = Some(id);
        if slot == self.camps[camp].start {
            self.camps[camp].leader = Some(id);
        }
        self.mark_population(id, slot)?;
        tracing::debug!(camp, entity = id, kind, "family member created");
        Ok(())
    }
    pub(super) fn family_layout(
        &self,
        center: Pos,
        terrain: u8,
        footprint: usize,
        occupied: &[Pos],
    ) -> Option<Vec<Pos>> {
        if footprint == 1 {
            return Some(vec![center]);
        }
        if !(2..=7).contains(&terrain) {
            let positions = formation(center, footprint);
            return positions
                .iter()
                .all(|p| {
                    self.family_ground(*p, terrain)
                        && occupied.iter().all(|q| p.distance(*q) >= 0.75)
                })
                .then_some(positions);
        }
        // Thin, wooded inland habitats need a contour-aware footprint rather than a mandatory empty circle.
        let mut positions = vec![center];
        for p in member_positions(center, terrain) {
            if self.family_ground(p, terrain)
                && positions.iter().all(|q| p.distance(*q) >= 0.75)
                && occupied.iter().all(|q| p.distance(*q) >= 0.75)
            {
                positions.push(p);
            }
            if positions.len() == footprint {
                return Some(positions);
            }
        }
        None
    }
    pub(super) fn place_camp(
        &mut self,
        family: Arc<CampRule>,
        terrain: u8,
        positions: Vec<Pos>,
    ) -> Result<()> {
        ensure!(
            positions.len() >= family.size(),
            "family footprint smaller than placement capacity"
        );
        let members = std::iter::once((family.leader, true)).chain(
            family
                .roles
                .iter()
                .flat_map(|r| (0..r.maximum).map(move |i| (r.kind, i < r.initial))),
        );
        for ((kind, _), pos) in members.clone().zip(&positions) {
            self.xml_preflight_admission(kind, *pos)?;
        }
        let camp = self.camps.len();
        self.camps.push(Camp {
            leader: None,
            terrain,
            start: self.population.len(),
            next: family
                .roles
                .iter()
                .map(|r| r.interval.map(|interval| self.now + interval))
                .collect(),
            family: family.clone(),
        });
        for ((kind, initial), pos) in members.zip(positions) {
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
            terrain,
            capacity = family.size(),
            "authored camp allocated"
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
            let start = self.camps[camp].start;
            let family = self.camps[camp].family.clone();
            let terrain = self.camps[camp].terrain;
            let slots = start..start + family.size();
            let mut from = start + 1;
            for (role, r) in family.roles.iter().enumerate() {
                let to = from + r.maximum;
                let Some(interval) = r.interval else {
                    from = to;
                    continue;
                };
                let owner = slots
                    .clone()
                    .filter_map(|i| self.population[i].entity)
                    .find(|id| {
                        self.entities
                            .get(id)
                            .is_some_and(|e| e.kind == family.leader)
                    });
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
                let deadline =
                    self.camps[camp].next[role].context("reinforcing role lacks its clock")?;
                if self.now >= deadline {
                    self.camps[camp].next[role] = Some(self.now + interval);
                    if let Some(slot) = (from..to).find(|i| self.population[*i].entity.is_none()) {
                        let pos = member_positions(center, terrain).into_iter().find(|p| {
                            self.family_ground(*p, terrain)
                                    && self.players.values().all(|q| q.pos.distance(*p) >= 0.75)
                                    // Per candidate, a whole-world actor scan made reinforcements 25-40 ms tick spikes.
                                    && self
                                        .camp_members(camp)
                                        .all(|e| e.pos.distance(*p) >= 0.75)
                        });
                        if let Some(pos) = pos {
                            self.place_member(camp, slot, pos)?;
                            tracing::debug!(
                                camp,
                                leader,
                                role,
                                kind = r.kind,
                                interval,
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

#[cfg(test)]
pub(super) fn camp_fixture(leader: u16) -> (World, Arc<CampRule>) {
    let (mut world, _) = super::movement_tests::fixture(0x18, false);
    let (family, footprint) = world.population_placement(leader).unwrap();
    let family = family.unwrap();
    let terrain = world.population_habitat(leader).unwrap();
    for cell in &mut Arc::make_mut(&mut world.map).cells {
        cell.terrain = terrain;
    }
    let positions = world
        .family_layout(Pos { x: 16.5, y: 8.5 }, terrain, footprint, &[])
        .unwrap();
    world
        .place_camp(family.clone(), terrain, positions)
        .unwrap();
    (world, family)
}

#[cfg(test)]
#[test]
fn camp_members_are_every_live_entity_of_their_camp() {
    let (mut world, _) = camp_fixture(0x62d);
    let scanned = |w: &World| -> BTreeSet<i32> {
        w.entities
            .values()
            .filter(|e| e.camp == Some(0))
            .map(|e| e.id)
            .collect()
    };
    let slotted = |w: &World| -> BTreeSet<i32> { w.camp_members(0).map(|e| e.id).collect() };
    assert!(scanned(&world).len() > 1);
    assert_eq!(slotted(&world), scanned(&world));
    world.remove_entity(*slotted(&world).iter().nth(1).unwrap());
    assert_eq!(slotted(&world), scanned(&world));
}
