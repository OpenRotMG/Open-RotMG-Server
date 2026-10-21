use crate::{
    content::Content,
    wire::{self, Packet, Reader},
    world::{
        Action, Pos, Slot,
        view::{OutputState, PlayerView},
    },
};
use anyhow::{Context, Result, ensure};
use rand::{Rng, SeedableRng};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) mod adventures;
mod combat;
mod crowds;
pub(crate) mod dialogue;
mod events;
#[cfg(test)]
mod events_tests;
mod progression;
pub(crate) mod pursuits;
mod social;

#[derive(Clone, Copy)]
struct Profile {
    // Keep persisted registry keys independent of the names visible to players.
    identity: &'static str,
    name: &'static str,
    recovery: f32,
    resources: f32,
    companions: f32,
    reaction: u64,
    healer: bool,
}
// Names transcribed from Whcxy3RJKIA 4:46, fubGzGmVqpI 9:33 and Ex_Ix3dlB4U 5:33 (YouTube).
const PROFILES: [Profile; 6] = [
    Profile {
        identity: "Bayleaf",
        name: "Triceps",
        recovery: 0.62,
        resources: 4.,
        companions: 3.,
        reaction: 6,
        healer: true,
    },
    Profile {
        identity: "Mossy",
        name: "Biceps",
        recovery: 0.35,
        resources: 8.,
        companions: 3.,
        reaction: 4,
        healer: false,
    },
    Profile {
        identity: "Trellis",
        name: "BattleCrus",
        recovery: 0.48,
        resources: 3.,
        companions: 8.,
        reaction: 5,
        healer: false,
    },
    Profile {
        identity: "Juniper",
        name: "Kadabu",
        recovery: 0.55,
        resources: 3.,
        companions: 7.,
        reaction: 5,
        healer: true,
    },
    Profile {
        identity: "Reed",
        name: "ExElite",
        recovery: 0.42,
        resources: 4.,
        companions: 5.,
        reaction: 6,
        healer: false,
    },
    Profile {
        identity: "Bramble",
        name: "Pixl",
        recovery: 0.38,
        resources: 6.,
        companions: 6.,
        reaction: 4,
        healer: false,
    },
];
#[derive(Clone, Debug, PartialEq, Eq)]
enum Goal {
    Recover,
    Remedy(i32),
    Dodge,
    Fight(i32),
    Hunt(i32),
    Venture,
    Bank(i32),
    Break(i32),
    Loot(i32),
    Follow(i32),
    Gather(i32),
    Portal(i32),
    Revisit(i16, i16),
    Survey(i16, i16),
    Explore(i16, i16),
    Wait,
}
#[derive(Debug)]
enum Journey {
    Portal(i32),
    Nexus,
    Regroup(i32),
}
struct Decision {
    goal: Goal,
    movement: Option<Pos>,
    aim: Option<f32>,
    item: Option<Action>,
    journey: Option<Journey>,
    chat: Option<&'static str>,
}
struct Brain {
    profile: Profile,
    mind: pursuits::Mind,
    group: Option<adventures::Adventure>,
    group_lost_since: Option<u64>,
    crowd: crowds::Crowd,
    offered: Option<adventures::Offer>,
    proposing: bool,
    session: (i32, i32, i32),
    last_tick: Option<u64>,
    goal: Goal,
    rng: rand::rngs::StdRng,
    heading: f32,
    previous: Pos,
    stationary: u64,
    destination: Option<Pos>,
    blocked: BTreeMap<(i16, i16), u64>,
    cover: BTreeSet<(i16, i16)>,
    visits: BTreeMap<(i16, i16), u64>,
    companion: Option<i32>,
    agreement: Option<dialogue::Agreement>,
    invitation_until: u64,
    next_decision: u64,
    recover_until: u64,
    tracks: BTreeMap<i32, (Pos, u64, Pos)>,
    next_teleport: u64,
    next_bank: u64,
    inland_reached: bool,
    nexus_until: u64,
    portal: Option<(i32, u64)>,
    hunting: Option<Pos>,
    last_encounter: u64,
    navigation: Option<Arc<Grid>>,
    navigation_rebuilds: u64,
    navigation_reuses: u64,
    performance: crate::performance::Window,
}
impl Brain {
    fn new(profile: Profile, view: &PlayerView) -> Self {
        let seed = view.session.account as u64;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let heading = rng.gen_range(0.0..std::f32::consts::TAU);
        let mut mind = pursuits::Mind::new(view.session.account);
        let index = PROFILES
            .iter()
            .position(|p| p.identity == profile.identity)
            .expect("registered cognitive profile");
        mind.cognition = pursuits::Cognition {
            attention: [4, 8, 2, 6, 3, 5][index],
            velocity_step: [1., 0.25, 2., 0.5, 2., 1.][index],
            directions: [8, 16, 8, 16, 4, 8][index],
            pickup_clearance: [1.3, 0.8, 1.05, 1.5, 0.65, 0.95][index],
        };
        mind.market.name = profile.name.into();
        mind.events.knows_blue = [true, false, false, true, false, false][index];
        Self {
            profile,
            mind,
            group: None,
            group_lost_since: None,
            crowd: crowds::Crowd::new([true, true, false, true, false, true][index]),
            offered: None,
            proposing: false,
            session: (
                view.session.world,
                view.session.player,
                view.session.character,
            ),
            last_tick: None,
            goal: Goal::Wait,
            rng,
            heading,
            previous: view.position,
            stationary: view.tick,
            destination: None,
            blocked: BTreeMap::new(),
            cover: BTreeSet::new(),
            visits: BTreeMap::new(),
            companion: None,
            agreement: None,
            invitation_until: 0,
            next_decision: view.tick + seed % profile.reaction,
            recover_until: 0,
            tracks: BTreeMap::new(),
            next_teleport: 0,
            next_bank: 0,
            inland_reached: false,
            nexus_until: if view.map.name == "Nexus" {
                view.time_ms + 10000
            } else {
                0
            },
            portal: None,
            hunting: None,
            last_encounter: view.time_ms,
            navigation: None,
            navigation_rebuilds: 0,
            navigation_reuses: 0,
            performance: crate::performance::Window::default(),
        }
    }
    fn receive(
        &mut self,
        packet: Packet,
        view: &PlayerView,
        legacy_invitations: bool,
    ) -> Result<bool> {
        if packet.id != wire::TEXT {
            return Ok(false);
        }
        let mut r = Reader::new(&packet.body);
        r.utf()?;
        let actor = r.i32()?;
        r.i32()?;
        r.u8()?;
        let recipient = r.utf()?;
        let text = r.utf()?;
        r.utf()?;
        r.done()?;
        if legacy_invitations
            && actor > 0
            && actor != self.session.1
            && recipient.is_empty()
            && view
                .objects
                .iter()
                .any(|o| o.id == actor && o.hp > 0 && !o.equipped.is_empty())
            && text.eq_ignore_ascii_case(&format!("{} follow me", self.profile.name))
        {
            self.companion = Some(actor);
            self.invitation_until = view.time_ms + 60000;
            return Ok(true);
        }
        Ok(false)
    }
    fn grid(&mut self, v: &PlayerView, content: &Content) -> Grid {
        let observed: BTreeSet<_> = v
            .objects
            .iter()
            .filter(|o| o.blocks && !content.objects[&o.kind].enemy)
            .map(|o| tile(o.position))
            .collect();
        // Only nearby observed absence clears remembered cover, not an out-of-view removal.
        self.cover
            .retain(|point| center(*point).distance(v.position) >= 19. || observed.contains(point));
        self.cover.extend(observed);
        let mut grid = Grid::new(v);
        for point in self
            .cover
            .iter()
            .chain(self.blocked.keys())
            .filter(|point| **point != tile(v.position))
        {
            grid.walkable.remove(point);
        }
        if self.mind.events.knows_blue && v.map.name == "Mad Lab" {
            for pool in v.objects.iter().filter(|p| {
                p.kind == crate::dungeon::lab::HEXXER
                    && ((v.position.x as i32 - p.position.x as i32).abs() >= 2
                        || (v.position.y as i32 - p.position.y as i32).abs() >= 2)
            }) {
                grid.walkable.retain(|point| {
                    (i32::from(point.0) - pool.position.x as i32).abs() >= 2
                        || (i32::from(point.1) - pool.position.y as i32).abs() >= 2
                });
            }
            grid.walkable.insert(tile(v.position));
        }
        grid
    }
    fn decide(&mut self, v: &PlayerView, content: &Content) -> Result<Decision> {
        ensure!(
            self.session == (v.session.world, v.session.player, v.session.character),
            "controller session changed"
        );
        ensure!(
            self.last_tick.is_none_or(|last| v.tick > last),
            "controller observation clock did not advance"
        );
        ensure!(
            v.lifecycle.alive
                && !v.lifecycle.death_pending
                && !v.lifecycle.departing
                && !v.lifecycle.quarantined
                && matches!(v.lifecycle.output, OutputState::Live),
            "controller is not live"
        );
        ensure!(
            v.tiles.iter().any(|t| (t.x, t.y) == tile(v.position)),
            "controller own terrain missing"
        );
        ensure!(
            v.projectiles
                .iter()
                .all(|b| b.speed > 0. && b.lifetime_ms > 0.),
            "projectile trajectory cannot be inferred from observation"
        );
        self.last_tick = Some(v.tick);
        self.next_decision = v.tick + self.profile.reaction;
        self.blocked.retain(|_, until| *until > v.tick);
        if v.time_ms >= self.invitation_until {
            self.companion = None;
            self.agreement = None;
        }
        if self.previous.distance(v.position) > 0.15 {
            self.stationary = v.tick;
        }
        if v.tick.saturating_sub(self.stationary) > 30
            && let Some(destination) = self.destination
        {
            self.blocked.insert(tile(destination), v.tick + 100);
            tracing::warn!(player=v.session.player,tick=v.tick,goal=?self.goal,x=destination.x,y=destination.y,"autonomous route made no progress; temporarily excluded");
            self.heading = self.rng.gen_range(0.0..std::f32::consts::TAU);
            self.stationary = v.tick;
        }
        self.previous = v.position;
        self.visits.insert(tile(v.position), v.tick);
        let mut decision = Decision {
            goal: Goal::Wait,
            movement: None,
            aim: None,
            item: None,
            journey: None,
            chat: None,
        };
        if v.lifecycle.busy {
            return Ok(decision);
        }
        self.mind.reconcile(v)?;
        self.mind.events.observe(v);
        let party = social::companions(v, self.group.as_ref());
        if self.group.is_some() && party.is_empty() {
            self.group_lost_since.get_or_insert(v.time_ms);
        } else {
            self.group_lost_since = None;
        }
        decision.item = self
            .mind
            .social
            .healing(v, content, self.profile.companions)?;
        if decision.item.is_none()
            && v.character.hp as f32 / v.character.stats[0] as f32 >= self.profile.recovery
        {
            decision.item = progression::mana(v, content)?;
        }
        // A live bargain reserves its stock before ordinary consumption/equipment arbitration.
        if self
            .mind
            .market
            .bargain
            .as_ref()
            .is_some_and(|b| b.resolution.is_none())
        {
            let grid = self.grid(v, content);
            if let Some(market) =
                self.mind
                    .market
                    .decide(v, content, self.mind.future_class, &grid)?
            {
                return Ok(market);
            }
        }
        let inspecting = self.mind.inspecting(v, content);
        let perception = combat::Perception::attend(v, &self.mind.cognition, inspecting);
        let equipment = if decision.item.is_none() {
            self.mind.assess(v, content)?
        } else {
            None
        };
        let equipping = equipment.is_some();
        if equipping {
            decision.item = equipment;
        }
        let enemies: Vec<_> = v
            .objects
            .iter()
            .filter(|o| {
                content.objects[&o.kind].enemy
                    && o.hp > 0
                    && !matches!(
                        o.kind,
                        crate::dungeon::lab::HEXXER
                            | crate::dungeon::lab::DEHEXXER
                            | crate::dungeon::lab::BUBBLE
                    )
            })
            .collect();
        let nearest = enemies.iter().min_by(|a, b| {
            a.position
                .distance(v.position)
                .total_cmp(&b.position.distance(v.position))
        });
        let quarry = enemies
            .iter()
            .filter(|enemy| {
                progression::can_fight(v, content, enemy)
                    && enemy.position.distance(v.position) <= 32.
            })
            .min_by(|a, b| {
                a.position
                    .distance(v.position)
                    .total_cmp(&b.position.distance(v.position))
            });
        if let Some(enemy) = quarry {
            self.hunting = Some(enemy.position);
            self.last_encounter = v.time_ms;
        }
        self.tracks
            .retain(|_, (_, seen, _)| v.time_ms.saturating_sub(*seen) < 1000);
        for enemy in enemies.iter().take(self.mind.cognition.attention) {
            let mut velocity = Pos { x: 0., y: 0. };
            if let Some((previous, time, _)) = self.tracks.get(&enemy.id) {
                let elapsed = v.time_ms.saturating_sub(*time) as f32 / 1000.;
                if elapsed > 0. && elapsed < 1. && previous.distance(enemy.position) < elapsed * 8.
                {
                    velocity = Pos {
                        x: (enemy.position.x - previous.x) / elapsed,
                        y: (enemy.position.y - previous.y) / elapsed,
                    };
                }
            }
            self.tracks.insert(
                enemy.id,
                (enemy.position, v.time_ms, perception.velocity(velocity)),
            );
            if self.tracks.len() > 32 {
                let first = *self.tracks.keys().next().expect("bounded tracks");
                self.tracks.remove(&first);
            }
        }
        let weapon = content.get(v.character.items[0] as u16)?;
        let projectile = weapon
            .projectiles
            .get(&0)
            .context("weapon projectile missing")?;
        let range = projectile.speed * projectile.lifetime / 10000.;
        let health = v.character.hp as f32 / v.character.stats[0] as f32;
        let danger = combat::clearance(v, v.position, &perception);
        let threatened = danger < self.mind.cognition.pickup_clearance;
        if health < self.profile.recovery {
            self.recover_until = v.tick + 60;
        }
        let outmatched = enemies.iter().any(|enemy| {
            !progression::can_fight(v, content, enemy) && enemy.position.distance(v.position) < 8.
        });
        let retreat = outmatched
            || v.tick < self.recover_until
            || (matches!(self.goal, Goal::Recover)
                && health < (self.profile.recovery + 0.15).min(0.9));
        let mut grid = self.grid(v, content);
        if !retreat {
            for enemy in enemies
                .iter()
                .filter(|enemy| !progression::can_fight(v, content, enemy))
            {
                grid.walkable
                    .retain(|point| center(*point).distance(enemy.position) >= 8.);
            }
        }
        // Cover, failed routes and dangerous enemies must invalidate navigation before reuse.
        let grid = match &self.navigation {
            Some(old)
                if old.walkable == grid.walkable
                    && old.known == grid.known
                    && old.dimensions == grid.dimensions =>
            {
                self.navigation_reuses += 1;
                old.clone()
            }
            _ => {
                self.navigation_rebuilds += 1;
                let grid = Arc::new(grid);
                self.navigation = Some(grid.clone());
                grid
            }
        };
        let remedy = self
            .mind
            .events
            .remedy(v)
            .filter(|blue| grid.next(v.position, blue.position).is_some());
        if events::shared(&v.map.name) {
            tracing::info!(
                account = v.session.account,
                character = v.session.character,
                tick = v.tick,
                time_ms = v.time_ms,
                hp = v.character.hp,
                max_hp = v.character.stats[0],
                conditions = v.character.conditions,
                blue = remedy.map(|p| p.id),
                blue_distance = remedy.map(|p| v.position.distance(p.position)),
                threatened,
                retreat,
                outmatched,
                "autonomous event safety observation"
            );
        }
        if threatened
            && v.objects.iter().any(|o| o.contents.contains(&0xa22))
            && v.map.name == "Realm"
        {
            tracing::info!(
                account = v.session.account,
                danger,
                acceptance = self.mind.cognition.pickup_clearance,
                "autonomous exposed supply declined"
            );
            decision.goal = Goal::Recover;
            decision.journey = Some(Journey::Nexus);
            return Ok(decision);
        }
        if retreat || threatened {
            decision.goal = if retreat {
                Goal::Recover
            } else if let Some(blue) = remedy {
                Goal::Remedy(blue.id)
            } else {
                Goal::Dodge
            };
            let held_heal = v
                .character
                .items
                .iter()
                .filter(|kind| **kind >= 0)
                .any(|kind| content.objects[&(*kind as u16)].activate == "Heal");
            if ((health < self.profile.recovery && !held_heal)
                || (outmatched && nearest.is_some_and(|e| e.position.distance(v.position) < 4.)))
                && decision.item.is_none()
                && v.map.name != "Nexus"
            {
                decision.journey = Some(Journey::Nexus);
                return Ok(decision);
            }
            let opponent = if retreat {
                None
            } else {
                combat::target(v, content, &grid, &enemies, range, projectile.passes_cover)
            };
            if threatened || nearest.is_some_and(|e| e.position.distance(v.position) < range * 1.3)
            {
                let focus = if retreat {
                    nearest.map(|e| e.position)
                } else if let Some(blue) = remedy {
                    Some(blue.position)
                } else {
                    opponent.map(|e| e.position)
                };
                let spacing = if !retreat && remedy.is_some() {
                    0.
                } else {
                    range
                };
                decision.movement =
                    combat::movement(v, &grid, &enemies, focus, spacing, retreat, &perception);
            }
            if retreat {
                for (slot, kind) in v
                    .character
                    .items
                    .iter()
                    .enumerate()
                    .skip(4)
                    .filter(|(_, k)| **k >= 0)
                {
                    let item = content.get(*kind as u16)?;
                    if decision.item.is_none()
                        && item.activate == "Heal"
                        && v.character.stats[0] - v.character.hp >= (item.heal as f32 * 0.35) as i32
                    {
                        decision.item = Some(Action::Use {
                            slot,
                            x: v.position.x,
                            y: v.position.y,
                        });
                        break;
                    }
                }
            }
            if let Some(enemy) = opponent {
                combat::attack(
                    v,
                    content,
                    &grid,
                    enemy,
                    self.tracks
                        .get(&enemy.id)
                        .map_or(Pos { x: 0., y: 0. }, |track| track.2),
                    &perception,
                    &mut decision,
                )?;
            }
            return Ok(decision);
        }
        if (equipping && remedy.is_none()) || (v.map.name == "Vault" && inspecting) {
            return Ok(decision);
        }
        self.mind
            .market
            .decide(v, content, self.mind.future_class, &grid)?;
        if let Some(project) = self.mind.supply(v, content, &grid)? {
            return Ok(project);
        }
        if let Some(mut event) =
            self.mind
                .events
                .decide(v, content, &grid, self.mind.cognition.attention)?
        {
            if decision.item.is_some() {
                event.item = decision.item;
            }
            return Ok(event);
        }
        if decision.item.is_none() && nearest.is_none_or(|e| e.position.distance(v.position) > 5.) {
            decision.item = progression::consume(v, content)?;
            if decision.item.is_some() {
                return Ok(decision);
            }
        }
        if (self.proposing
            || self
                .offered
                .as_ref()
                .is_some_and(|o| o.until > v.elapsed_ms))
            && peaceful(&enemies, v.position)
        {
            self.destination = None;
            self.stationary = v.tick;
            return Ok(decision);
        }
        if v.map.name == "Vault"
            && self.agreement.is_none()
            && let Some(banking) = progression::bank(v, content, &grid, self.mind.future_class)?
        {
            self.next_bank = v.time_ms + 300000;
            if matches!(banking.journey, Some(Journey::Nexus)) && self.mind.social.trying_loadout(v)
            {
                decision.aim = Some(self.heading);
                return Ok(decision);
            }
            return Ok(banking);
        }
        let banking = v.time_ms >= self.next_bank && progression::stash(v, content)?;
        if self.agreement.is_none()
            && peaceful(&enemies, v.position)
            && let Some(social) =
                self.mind
                    .social
                    .decide(v, &grid, self.profile.companions, banking)
        {
            return Ok(social);
        }
        if v.map.name == "Nexus" && self.agreement.is_none() && v.time_ms < self.nexus_until {
            return Ok(decision);
        }
        if v.map.name == "Realm"
            && self.agreement.is_none()
            && banking
            && v.character
                .items
                .iter()
                .skip(4)
                .filter(|k| **k >= 0)
                .count()
                >= 6
            && peaceful(&enemies, v.position)
            && health >= 0.75
        {
            decision.journey = Some(Journey::Nexus);
            if self.group.is_some() {
                decision.chat = Some("nexus");
            }
            return Ok(decision);
        }
        let mut choices = Vec::new();
        let event_area = self.mind.events.area(v);
        if let Some(area) = event_area {
            let (x, y) = tile(area);
            choices.push((97., Goal::Survey(x, y), area));
        }
        let inland = Pos {
            x: v.map.width as f32 * 0.5,
            y: v.map.height as f32 * 0.5,
        };
        if v.map.name == "Realm" && v.position.distance(inland) <= 12. {
            self.inland_reached = true;
        }
        if v.map.name == "Realm" && progression::ready(v, content, 0x718) && !self.inland_reached {
            choices.push((60. + self.profile.resources, Goal::Venture, inland));
        }
        for enemy in &enemies {
            if !progression::can_fight(v, content, enemy)
                || events::shared(&v.map.name) && !progression::vulnerable(enemy)
            {
                continue;
            }
            let d = enemy.position.distance(v.position);
            if event_area.is_some() && !events::objective(enemy.kind) && d > range {
                continue;
            }
            let protected = !progression::vulnerable(enemy);
            let score = 45. - d * 2.
                + if d < range {
                    35. + (enemy.max_hp as f32).sqrt().min(20.)
                } else {
                    0.
                };
            choices.push((score, Goal::Fight(enemy.id), enemy.position));
            if events::shared(&v.map.name)
                && progression::vulnerable(enemy)
                && events::objective(enemy.kind)
            {
                choices.push((110., Goal::Hunt(enemy.id), enemy.position));
            }
            if v.quest == Some(enemy.id)
                && !protected
                && (v.character.level >= 3 || d <= 48.)
                && (v.map.name != "Realm" || !progression::ready(v, content, 0x718) || d <= 32.)
            {
                choices.push((
                    if v.map.name == "Snake Pit" || events::shared(&v.map.name) {
                        97.
                    } else {
                        55. + self.profile.resources
                    },
                    Goal::Hunt(enemy.id),
                    enemy.position,
                ));
            }
        }
        {
            let needs_mana = v.character.items[1] >= 0
                && v.character.mp / 2 < content.get(v.character.items[1] as u16)?.mana_cost;
            for bag in v
                .objects
                .iter()
                .filter(|o| content.objects[&o.kind].class == "Container")
            {
                if progression::pickup(v, content, bag, self.mind.future_class)?.is_some() {
                    // A distant objective must not outrank replenishing an exhausted ability.
                    let replenishing = needs_mana
                        && peaceful(&enemies, v.position)
                        && bag
                            .contents
                            .iter()
                            .filter(|kind| **kind >= 0)
                            .any(|kind| content.objects[&(*kind as u16)].activate == "Magic");
                    let score = if replenishing {
                        98.
                    } else {
                        (40. + 16. * self.profile.resources
                            / (1. + v.position.distance(bag.position)))
                        .min(96.)
                    };
                    choices.push((score, Goal::Loot(bag.id), bag.position));
                }
            }
        }
        let peaceful = nearest.is_none_or(|e| e.position.distance(v.position) > 5.);
        for portal in v
            .objects
            .iter()
            .filter(|p| p.usable)
            .filter(|p| match v.map.name.as_str() {
                "Nexus" => {
                    if banking {
                        p.kind == 0x720
                    } else {
                        matches!(p.kind, 0x704 | 0x70e | 0x71c | 0x712)
                    }
                }
                // The committed exit is public even when a straggler missed personal clear credit.
                "Pirate Cave" | "Snake Pit" => p.kind == 0x71d,
                "Oryx's Castle" => p.kind == 0xd7b && events::ready(v, content),
                "Oryx's Chamber" => p.kind == 0x71d || p.kind == 0x700 && events::ready(v, content),
                "Wine Cellar" | "Mad Lab" => matches!(p.kind, 0x71d | 0x712),
                "Realm" => {
                    progression::ready(v, content, p.kind)
                        && self.group.as_ref().is_none_or(|g| {
                            p.kind == 0x890 || !g.finished && g.purpose.portal() == p.kind
                        })
                }
                _ => false,
            })
        {
            let exiting =
                portal.kind == 0x71d || portal.kind == 0x712 && events::shared(&v.map.name);
            if exiting || ((peaceful || matches!(portal.kind, 0xd7b | 0x700)) && health >= 0.75) {
                let reward_waiting = choices.iter().any(|(_, goal, _)| {
                    if let Goal::Loot(id) = goal {
                        v.objects
                            .iter()
                            .find(|bag| bag.id == *id)
                            .is_some_and(|bag| {
                                bag.contents.iter().filter(|kind| **kind >= 0).any(|kind| {
                                    content.objects[&(*kind as u16)].stat_increase.is_some()
                                })
                            })
                    } else {
                        false
                    }
                });
                choices.push((
                    if exiting {
                        if reward_waiting
                            || events::shared(&v.map.name)
                                && choices
                                    .iter()
                                    .any(|(_, goal, _)| matches!(goal, Goal::Loot(_)))
                        {
                            0.
                        } else {
                            99.
                        }
                    } else if portal.kind == 0x700 {
                        101.
                    } else {
                        95.
                    },
                    Goal::Portal(portal.id),
                    portal.position,
                ));
            }
        }
        if let Some(agreement) = &self.agreement {
            let destination = match agreement.waiting {
                Some(position) => Some((Goal::Gather(v.session.player), position)),
                None => v
                    .objects
                    .iter()
                    .find(|p| p.name == agreement.partner && p.hp > 0)
                    .map(|p| (Goal::Follow(p.id), p.position)),
            };
            if let Some((goal, position)) = destination {
                choices.push((110., goal, position));
            }
        }
        for player in v
            .objects
            .iter()
            .filter(|o| content.objects[&o.kind].class == "Player")
        {
            let d = player.position.distance(v.position);
            if d > 4. && (self.companion == Some(player.id)) {
                let score = 100.;
                choices.push((score, Goal::Follow(player.id), player.position));
            }
        }
        self.crowd
            .attend(v, content, &self.mind.cognition, inspecting);
        if self.agreement.is_none()
            && self.companion.is_none()
            && let Some(marker) = self.crowd.destination(v, self.group.as_ref())
        {
            let distance = marker.position.distance(v.position);
            if distance > 7. {
                let score = 54. + self.profile.companions;
                if !choices.iter().any(|(s, _, _)| *s > score)
                    && peaceful
                    && distance > 24.
                    && v.map.allow_teleport
                    && v.time_ms >= self.next_teleport
                {
                    self.next_teleport = v.time_ms + 10000;
                    decision.goal = Goal::Follow(marker.id);
                    decision.journey = Some(Journey::Regroup(marker.id));
                    tracing::info!(
                        account = v.session.account,
                        target = marker.id,
                        world = v.session.world,
                        "autonomous attended crowd travel"
                    );
                    let peer = marker.id;
                    self.crowd.approach_selected(peer);
                    return Ok(decision);
                }
                choices.push((score, Goal::Follow(marker.id), marker.position));
            }
        }
        // Revisiting an emptied room must not compete with progressing through the dungeon.
        if v.map.name == "Realm"
            && let Some(place) = self.hunting
            && v.time_ms.saturating_sub(self.last_encounter)
                > ((if v.character.level < 3 {
                    60000.
                } else {
                    240000.
                }) / self.profile.resources) as u64
        {
            if v.position.distance(place) > 3. {
                let (x, y) = tile(place);
                choices.push((30., Goal::Revisit(x, y), center((x, y))));
            } else {
                self.hunting = None;
            }
        }
        // Exploration's maximum score is 20; a clear higher-ranked route makes its full-map search irrelevant.
        if !choices.iter().any(|(score, goal, position)| {
            score + if *goal == self.goal { 8. } else { 0. } > 20.
                && !self.blocked.contains_key(&tile(*position))
                && grid.segment(v.position, *position)
        }) {
            let reachable = grid.reachable(v.position);
            if let Goal::Explore(x, y) = self.goal {
                let pos = center((x, y));
                if pos.distance(v.position) > 0.7 && reachable.contains(&(x, y)) {
                    choices.push((12., self.goal.clone(), pos));
                }
            }
            if !choices
                .iter()
                .any(|(_, g, _)| matches!(g, Goal::Explore(..)))
            {
                let next = reachable
                    .iter()
                    .filter(|t| !self.blocked.contains_key(t))
                    .filter(|t| {
                        center(**t).distance(v.position) > 3.
                            && center(**t).distance(v.position) < 24.
                    })
                    .max_by(|a, b| {
                        let frontier = |t| grid.frontier(t, v.map.width, v.map.height);
                        let priority = frontier(**a)
                            .cmp(&frontier(**b))
                            .then_with(|| self.visits.get(*b).cmp(&self.visits.get(*a)));
                        if !priority.is_eq() {
                            return priority;
                        }
                        let value = |t| {
                            let p = center(t);
                            quarry.map_or_else(
                                || {
                                    (p.x - v.position.x) * self.heading.cos()
                                        + (p.y - v.position.y) * self.heading.sin()
                                },
                                |enemy| -p.distance(enemy.position),
                            )
                        };
                        value(**a).total_cmp(&value(**b))
                    });
                if let Some((x, y)) = next {
                    choices.push((12., Goal::Explore(*x, *y), center((*x, *y))));
                }
            }
        }
        for (score, goal, _) in &mut choices {
            if *goal == self.goal {
                *score += 8.;
            }
        }
        choices.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, goal, destination) in choices {
            if self.blocked.contains_key(&tile(destination)) {
                if self.agreement.as_ref().is_some_and(|a| a.waiting.is_some()) {
                    return Ok(decision);
                }
                continue;
            }
            let movement = match goal {
                Goal::Gather(_) if v.position.distance(destination) < 0.7 => None,
                Goal::Survey(..) if v.position.distance(destination) <= 8. => None,
                Goal::Portal(id) if v.position.distance(destination) <= 2. => {
                    decision.journey = Some(Journey::Portal(id));
                    None
                }
                Goal::Fight(_) | Goal::Hunt(_)
                    if v.position.distance(destination) < range
                        && shot_clear(v, &grid, destination, projectile.passes_cover) =>
                {
                    combat::movement(
                        v,
                        &grid,
                        &enemies,
                        Some(destination),
                        range,
                        false,
                        &perception,
                    )
                }
                Goal::Follow(_) if v.position.distance(destination) < 4.5 => None,
                Goal::Loot(id) if v.position.distance(destination) < 1.4 => {
                    let bag = v
                        .objects
                        .iter()
                        .find(|o| o.id == id)
                        .context("selected bag missing")?;
                    if decision.item.is_none() {
                        decision.item = Some(
                            progression::pickup(v, content, bag, self.mind.future_class)?
                                .context("selected bag has no collectable item")?,
                        );
                    }
                    if let Some(action) = &decision.item {
                        self.mind.events.collecting(v, action);
                    }
                    if let Some(Action::Swap { from, to }) = &decision.item
                        && to.kind != -1
                    {
                        tracing::info!(
                            player = v.session.player,
                            held = to.kind,
                            collected = from.kind,
                            "autonomous exchanging carried spare for needed consumable"
                        );
                    }
                    None
                }
                _ => match if matches!(goal, Goal::Hunt(_) | Goal::Venture | Goal::Survey(..)) {
                    grid.towards(v.position, destination)
                } else {
                    grid.next(v.position, destination)
                } {
                    Some(pos) => Some(pos),
                    None => {
                        if self.agreement.as_ref().is_some_and(|a| a.waiting.is_some()) {
                            return Ok(decision);
                        }
                        if matches!(goal, Goal::Hunt(_))
                            && let Some(mut clearing) = progression::clear_cover(
                                v,
                                content,
                                &grid,
                                destination,
                                range,
                                projectile.passes_cover,
                            )
                        {
                            clearing.item = decision.item;
                            return Ok(clearing);
                        }
                        self.blocked.insert(tile(destination), v.tick + 100);
                        continue;
                    }
                },
            };
            if let Goal::Portal(portal) = goal
                && self.goal != goal
                && v.map.name == "Realm"
            {
                let portal = v
                    .objects
                    .iter()
                    .find(|p| p.id == portal)
                    .context("selected portal missing")?;
                decision.chat = Some(match portal.kind {
                    0x717 => "pc here",
                    0x718 => "snake here",
                    0x890 => "lab here",
                    kind => anyhow::bail!("unsupported autonomous dungeon call {kind:#x}"),
                });
            }
            if let Goal::Follow(id) = goal
                && v.position.distance(destination) > 8.
                && v.time_ms >= self.next_teleport
                && decision.item.is_none()
                && v.objects
                    .iter()
                    .any(|p| p.id == id && p.hp * 4 >= p.max_hp * 3)
                && enemies
                    .iter()
                    .all(|e| e.position.distance(destination) > 3.)
            {
                decision.journey = Some(Journey::Regroup(id));
                self.next_teleport = v.time_ms + 10000;
            }
            if self.agreement.is_none()
                && self.companion.is_none()
                && let Goal::Follow(id) = goal
            {
                self.crowd.approach_selected(id);
            }
            decision.goal = goal;
            decision.movement = movement;
            if (self.agreement.is_some()
                && matches!(decision.goal, Goal::Follow(_) | Goal::Gather(_))
                || matches!(decision.goal, Goal::Survey(..)))
                && let Some(enemy) =
                    combat::target(v, content, &grid, &enemies, range, projectile.passes_cover)
            {
                combat::attack(
                    v,
                    content,
                    &grid,
                    enemy,
                    self.tracks
                        .get(&enemy.id)
                        .map_or(Pos { x: 0., y: 0. }, |track| track.2),
                    &perception,
                    &mut decision,
                )?;
            }
            if let Goal::Fight(id) | Goal::Hunt(id) = decision.goal {
                let enemy = enemies
                    .iter()
                    .find(|e| e.id == id)
                    .copied()
                    .context("selected enemy missing")?;
                let enemy = if matches!(decision.goal, Goal::Hunt(_))
                    && v.map.name == "Snake Pit"
                    && v.position.distance(enemy.position) >= range
                {
                    combat::target(v, content, &grid, &enemies, range, projectile.passes_cover)
                } else {
                    Some(enemy)
                };
                if let Some(enemy) = enemy {
                    combat::attack(
                        v,
                        content,
                        &grid,
                        enemy,
                        self.tracks
                            .get(&enemy.id)
                            .map_or(Pos { x: 0., y: 0. }, |track| track.2),
                        &perception,
                        &mut decision,
                    )?;
                }
            }
            return Ok(decision);
        }
        self.heading += 0.7;
        Ok(decision)
    }
}
fn angle(a: Pos, b: Pos) -> f32 {
    (b.y - a.y).atan2(b.x - a.x)
}
fn tile(p: Pos) -> (i16, i16) {
    (p.x.floor() as i16, p.y.floor() as i16)
}
fn center(t: (i16, i16)) -> Pos {
    Pos {
        x: f32::from(t.0) + 0.5,
        y: f32::from(t.1) + 0.5,
    }
}
fn peaceful(enemies: &[&crate::world::view::VisibleObject], position: Pos) -> bool {
    enemies
        .iter()
        .all(|enemy| enemy.position.distance(position) > 5.)
}
fn hostile(v: &PlayerView, owner: i32) -> bool {
    owner != v.session.player
        && !v
            .objects
            .iter()
            .any(|o| o.id == owner && !o.equipped.is_empty())
}
fn shot_clear(v: &PlayerView, grid: &Grid, target: Pos, passes_cover: bool) -> bool {
    sight(v, grid, target, passes_cover, None)
}
fn sight(
    v: &PlayerView,
    grid: &Grid,
    target: Pos,
    passes_cover: bool,
    endpoint: Option<i32>,
) -> bool {
    let steps = (v.position.distance(target) * 8.).ceil().max(1.) as u32;
    (0..=steps).all(|step| {
        let f = step as f32 / steps as f32;
        let point = tile(Pos {
            x: v.position.x + (target.x - v.position.x) * f,
            y: v.position.y + (target.y - v.position.y) * f,
        });
        grid.known.contains(&point)
            && (passes_cover
                || !v
                    .objects
                    .iter()
                    .any(|o| o.blocks && Some(o.id) != endpoint && tile(o.position) == point))
    })
}
fn own(v: &PlayerView, slot: usize) -> Slot {
    Slot {
        object: v.session.player,
        slot,
        kind: v.character.items[slot],
    }
}
type TilePoint = (i16, i16);
#[derive(Default)]
struct Grid {
    walkable: BTreeSet<TilePoint>,
    known: BTreeSet<TilePoint>,
    dimensions: (i32, i32),
    reached: Mutex<Option<(TilePoint, Arc<BTreeSet<TilePoint>>)>>,
    frontiers: Mutex<BTreeMap<TilePoint, bool>>,
    searches: AtomicU64,
    cache_hits: AtomicU64,
    reachable_nodes: AtomicU64,
    route_nodes: AtomicU64,
}
impl Grid {
    fn new(v: &PlayerView) -> Self {
        let mut walkable: BTreeSet<_> = v
            .tiles
            .iter()
            .filter(|t| t.ground_speed > 0.)
            .map(|t| (t.x, t.y))
            .collect();
        for object in v.objects.iter().filter(|o| o.blocks) {
            walkable.remove(&tile(object.position));
        }
        Self {
            walkable,
            known: v.tiles.iter().map(|t| (t.x, t.y)).collect(),
            dimensions: (v.map.width, v.map.height),
            ..Self::default()
        }
    }
    fn towards(&self, from: Pos, target: Pos) -> Option<Pos> {
        if self.walkable.contains(&tile(target)) {
            return self.next(from, target);
        }
        let reachable = self.reachable(from);
        let next = reachable
            .iter()
            .filter(|point| {
                center(**point).distance(from) > 3.
                    && self.frontier(**point, self.dimensions.0, self.dimensions.1)
            })
            .min_by(|a, b| {
                center(**a)
                    .distance(target)
                    .total_cmp(&center(**b).distance(target))
            })?;
        self.next(from, center(*next))
    }
    fn reachable(&self, from: Pos) -> Arc<BTreeSet<TilePoint>> {
        let start = tile(from);
        let mut cache = self.reached.lock().expect("navigation cache poisoned");
        if let Some((origin, reached)) = &*cache
            && (*origin == start
                || (self.walkable.contains(origin)
                    && self.walkable.contains(&start)
                    && reached.contains(&start)))
        {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            return reached.clone();
        }
        let mut queue = VecDeque::from([start]);
        let mut unvisited: HashSet<_> = self.walkable.iter().copied().collect();
        unvisited.remove(&start);
        let mut reached = vec![start];
        while let Some((x, y)) = queue.pop_front() {
            for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if unvisited.remove(&next) {
                    reached.push(next);
                    queue.push_back(next);
                }
            }
        }
        let reached: Arc<BTreeSet<TilePoint>> = Arc::new(reached.into_iter().collect());
        *cache = Some((start, reached.clone()));
        self.searches.fetch_add(1, Ordering::Relaxed);
        self.reachable_nodes
            .fetch_add(reached.len() as u64, Ordering::Relaxed);
        reached
    }
    fn frontier(&self, (x, y): TilePoint, width: i32, height: i32) -> bool {
        assert_eq!(
            (width, height),
            self.dimensions,
            "frontier dimensions changed"
        );
        assert!(
            x >= 0 && y >= 0 && i32::from(x) < width && i32::from(y) < height,
            "frontier outside map"
        );
        *self
            .frontiers
            .lock()
            .expect("frontier cache poisoned")
            .entry((x, y))
            .or_insert_with(|| {
                [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
                    .iter()
                    .any(|n| {
                        n.0 >= 0
                            && n.1 >= 0
                            && i32::from(n.0) < width
                            && i32::from(n.1) < height
                            && !self.known.contains(n)
                    })
            })
    }
    fn segment(&self, from: Pos, to: Pos) -> bool {
        let steps = (from.distance(to) * 4.).ceil().max(1.) as i32;
        self.walkable.contains(&tile(to))
            && (0..=steps).all(|i| {
                let f = i as f32 / steps as f32;
                self.walkable.contains(&tile(Pos {
                    x: from.x + (to.x - from.x) * f,
                    y: from.y + (to.y - from.y) * f,
                }))
            })
    }
    fn metrics(&self) -> serde_json::Value {
        let reached = self.reached.lock().expect("navigation cache poisoned");
        serde_json::json!({"known_tiles":self.known.len(),"walkable_tiles":self.walkable.len(),
            "reachable_tiles":reached.as_ref().map_or(0, |(_,r)|r.len()),
            "frontier_entries":self.frontiers.lock().expect("frontier cache poisoned").len(),
            "searches":self.searches.load(Ordering::Relaxed),"cache_hits":self.cache_hits.load(Ordering::Relaxed),
            "reachable_nodes_visited":self.reachable_nodes.load(Ordering::Relaxed),"route_nodes_visited":self.route_nodes.load(Ordering::Relaxed)})
    }
    fn next(&self, from: Pos, to: Pos) -> Option<Pos> {
        let start = tile(from);
        let finish = tile(to);
        if !self.walkable.contains(&finish) {
            return None;
        }
        // A distant sampling grid can miss the nearby waypoint's blocked cell.
        let fraction = 4. / from.distance(to).max(4.);
        let waypoint = Pos {
            x: from.x + (to.x - from.x) * fraction,
            y: from.y + (to.y - from.y) * fraction,
        };
        if self.segment(from, to) && self.segment(from, waypoint) {
            return Some(waypoint);
        }
        if start == finish {
            return None;
        }
        let mut queue = VecDeque::from([start]);
        let mut parent = HashMap::from([(start, start)]);
        let mut visited = 0;
        while let Some((x, y)) = queue.pop_front() {
            visited += 1;
            if (x, y) == finish {
                break;
            }
            for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if self.walkable.contains(&next) && !parent.contains_key(&next) {
                    parent.insert(next, (x, y));
                    queue.push_back(next);
                }
            }
        }
        self.route_nodes.fetch_add(visited, Ordering::Relaxed);
        let mut next = finish;
        while *parent.get(&next)? != start {
            next = *parent.get(&next)?;
        }
        let target = center(next);
        if self.segment(from, target) {
            Some(target)
        } else {
            self.segment(from, center(start)).then_some(center(start))
        }
    }
}

#[cfg(test)]
mod performance_tests;
#[cfg(test)]
mod tests;

mod runtime;
pub(crate) const OUTPUT_CAPACITY: usize = 512;
pub use runtime::start;
pub fn profiles() -> Vec<&'static str> {
    PROFILES.iter().map(|p| p.name).collect()
}
fn selected_profile(name: &str) -> Result<Profile> {
    PROFILES
        .iter()
        .copied()
        .find(|p| p.name == name)
        .context("unknown actual gameplay profile")
}
pub fn validate_test_class(class: u16, profile: &str) -> Result<()> {
    ensure!(
        matches!(class, crate::content::WIZARD | crate::content::PRIEST),
        "this source implements only Wizard and Priest gameplay controllers"
    );
    selected_profile(profile)?;
    Ok(())
}
pub async fn start_test(
    app: &crate::server::App,
    character: crate::store::Character,
    world: i32,
    position: Pos,
    profile: &str,
) -> Result<()> {
    validate_test_class(character.class, profile)?;
    runtime::start_selected(app, selected_profile(profile)?, character, world, position).await
}
