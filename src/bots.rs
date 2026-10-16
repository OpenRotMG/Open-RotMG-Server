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
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

mod combat;
mod progression;
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
    leader: &'static str,
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
        leader: "Biceps",
        healer: true,
    },
    Profile {
        identity: "Mossy",
        name: "Biceps",
        recovery: 0.35,
        resources: 8.,
        companions: 3.,
        reaction: 4,
        leader: "Biceps",
        healer: false,
    },
    Profile {
        identity: "Trellis",
        name: "BattleCrus",
        recovery: 0.48,
        resources: 3.,
        companions: 8.,
        reaction: 5,
        leader: "Biceps",
        healer: false,
    },
    Profile {
        identity: "Juniper",
        name: "Kadabu",
        recovery: 0.55,
        resources: 3.,
        companions: 7.,
        reaction: 5,
        leader: "ExElite",
        healer: true,
    },
    Profile {
        identity: "Reed",
        name: "ExElite",
        recovery: 0.42,
        resources: 4.,
        companions: 5.,
        reaction: 6,
        leader: "ExElite",
        healer: false,
    },
    Profile {
        identity: "Bramble",
        name: "Pixl",
        recovery: 0.38,
        resources: 6.,
        companions: 6.,
        reaction: 4,
        leader: "ExElite",
        healer: false,
    },
];
#[derive(Clone, Debug, PartialEq, Eq)]
enum Goal {
    Recover,
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
    invitation_until: u64,
    next_decision: u64,
    recover_until: u64,
    tracks: BTreeMap<i32, (Pos, u64, Pos)>,
    next_teleport: u64,
    next_bank: u64,
    inland_reached: bool,
    regroup_call: Option<(Journey, u64)>,
    nexus_until: u64,
    next_call: u64,
    requested_until: u64,
    portal: Option<(i32, u64)>,
    leader_seen: Option<(i32, Pos, u64)>,
    contacts: BTreeMap<i32, (Pos, u64)>,
    hunting: Option<Pos>,
    last_encounter: u64,
}
impl Brain {
    fn new(profile: Profile, view: &PlayerView) -> Self {
        let seed = view.session.account as u64;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let heading = rng.gen_range(0.0..std::f32::consts::TAU);
        Self {
            profile,
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
            invitation_until: 0,
            next_decision: view.tick + seed % profile.reaction,
            recover_until: 0,
            tracks: BTreeMap::new(),
            next_teleport: 0,
            next_bank: 0,
            inland_reached: false,
            regroup_call: None,
            nexus_until: if view.map.name == "Nexus" {
                view.time_ms + 10000
            } else {
                0
            },
            next_call: view.time_ms + 10000,
            requested_until: 0,
            portal: None,
            leader_seen: None,
            contacts: BTreeMap::new(),
            hunting: None,
            last_encounter: view.time_ms,
        }
    }
    fn receive(&mut self, packet: Packet, view: &PlayerView) -> Result<bool> {
        if packet.id != wire::TEXT {
            return Ok(false);
        }
        let mut r = Reader::new(&packet.body);
        let name = r.utf()?;
        let actor = r.i32()?;
        r.i32()?;
        r.u8()?;
        let recipient = r.utf()?;
        let text = r.utf()?;
        r.utf()?;
        r.done()?;
        if actor > 0 && actor != self.session.1 && recipient.is_empty() {
            let leader = self.profile.name == self.profile.leader;
            if (!leader
                && name == self.profile.leader
                && self.companion.is_none()
                && (text == "nexus"
                    || (text == "tp here"
                        && view.objects.iter().any(|o| o.id == actor)
                        && self.requested_until > view.time_ms
                        && self.leader_seen.is_none_or(|(_, _, seen)| {
                            view.time_ms.saturating_sub(seen) >= 10000
                        }))))
                || (leader
                    && text == "tp?"
                    && PROFILES
                        .iter()
                        .any(|p| p.leader == self.profile.name && p.name == name))
            {
                let journey = if text == "nexus" {
                    Journey::Nexus
                } else {
                    Journey::Regroup(actor)
                };
                self.regroup_call = Some((journey, view.time_ms + 5000));
            }
        }
        if recipient.is_empty()
            && name == self.profile.leader
            && matches!(text.as_str(), "pc here" | "snake here")
            && self.leader_seen.is_some_and(|(id, _, seen)| {
                id == actor && view.time_ms.saturating_sub(seen) < 2000
            })
            && let Some(portal) = view
                .objects
                .iter()
                .find(|o| o.kind == if text == "pc here" { 0x717 } else { 0x718 })
        {
            self.portal = Some((portal.id, view.time_ms + 30000));
        }
        if actor > 0
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
        let party = social::companions(v, self.profile);
        self.contacts
            .retain(|_, (_, seen)| v.time_ms.saturating_sub(*seen) < 10000);
        for peer in &party {
            self.contacts.insert(peer.id, (peer.position, v.time_ms));
            if peer.name == self.profile.leader {
                self.leader_seen = Some((peer.id, peer.position, v.time_ms));
            }
        }
        if v.map.name == "Realm"
            && self.regroup_call.as_ref().is_some_and(|(journey, until)| {
                matches!(journey, Journey::Nexus) && *until > v.time_ms
            })
        {
            self.regroup_call = None;
            decision.journey = Some(Journey::Nexus);
            return Ok(decision);
        }
        decision.item = social::healing(v, content)?;
        if decision.item.is_none()
            && v.character.hp as f32 / v.character.stats[0] as f32 >= self.profile.recovery
        {
            decision.item = progression::mana(v, content)?;
        }
        let enemies: Vec<_> = v
            .objects
            .iter()
            .filter(|o| content.objects[&o.kind].enemy && o.hp > 0)
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
            .retain(|id, _| enemies.iter().any(|e| e.id == *id));
        for enemy in &enemies {
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
            self.tracks
                .insert(enemy.id, (enemy.position, v.time_ms, velocity));
        }
        let weapon = content.get(v.character.items[0] as u16)?;
        let projectile = weapon
            .projectiles
            .get(&0)
            .context("weapon projectile missing")?;
        let range = projectile.speed * projectile.lifetime / 10000.;
        let health = v.character.hp as f32 / v.character.stats[0] as f32;
        let threatened = combat::clearance(v, v.position) < 1.05;
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
        if retreat || threatened {
            decision.goal = if retreat { Goal::Recover } else { Goal::Dodge };
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
                } else {
                    opponent.map(|e| e.position)
                };
                decision.movement = combat::movement(v, &grid, &enemies, focus, range, retreat);
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
                    self.tracks[&enemy.id].2,
                    &mut decision,
                )?;
            }
            return Ok(decision);
        }
        let class = content.get(v.character.class)?;
        for (from, kind) in v
            .character
            .items
            .iter()
            .enumerate()
            .skip(4)
            .filter(|(_, k)| **k >= 0)
        {
            let item = content.get(*kind as u16)?;
            if decision.item.is_none()
                && let Some(to) = class
                    .slots
                    .iter()
                    .take(4)
                    .position(|slot| *slot == item.slot_type)
                && upgrade(content, *kind, v.character.items[to])?
            {
                decision.item = Some(Action::Swap {
                    from: own(v, from),
                    to: own(v, to),
                });
                return Ok(decision);
            }
        }
        if decision.item.is_none() && nearest.is_none_or(|e| e.position.distance(v.position) > 5.) {
            decision.item = progression::consume(v, content)?;
            if decision.item.is_some() {
                return Ok(decision);
            }
        }
        if v.map.name == "Vault"
            && let Some(banking) = progression::bank(v, content, &grid)?
        {
            self.next_bank = v.time_ms + 300000;
            return Ok(banking);
        }
        let banking = v.time_ms >= self.next_bank && progression::stash(v, content)?;
        if v.map.name == "Nexus"
            && (v.time_ms < self.nexus_until
                || (!banking
                    && v.time_ms < self.nexus_until + 30000
                    && if self.profile.name == self.profile.leader {
                        party.len() < 2
                    } else {
                        !party.iter().any(|peer| peer.name == self.profile.leader)
                    }))
        {
            return Ok(decision);
        }
        if v.map.name == "Realm"
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
            return Ok(decision);
        }
        if v.map.name == "Realm"
            && health >= 0.75
            && decision.item.is_none()
            && nearest.is_none_or(|e| e.position.distance(v.position) > 5.)
        {
            if let Some((journey, _)) = self.regroup_call.take().filter(|(_, t)| *t > v.time_ms) {
                if self.profile.name == self.profile.leader {
                    if v.time_ms >= self.next_call {
                        let Journey::Regroup(actor) = journey else {
                            anyhow::bail!("leader received an invalid reunion request");
                        };
                        if v.objects.iter().any(|o| o.id == actor) {
                            decision.chat = Some("tp here");
                        } else {
                            decision.chat = Some("nexus");
                            decision.journey = Some(Journey::Nexus);
                        }
                        self.next_call = v.time_ms + 10000;
                    }
                } else if v.time_ms >= self.next_teleport {
                    decision.journey = Some(journey);
                    self.next_teleport = v.time_ms + 10000;
                }
                if decision.journey.is_some() {
                    return Ok(decision);
                }
            } else if self.profile.name != self.profile.leader
                && self
                    .leader_seen
                    .is_none_or(|(_, _, seen)| v.time_ms.saturating_sub(seen) >= 10000)
                && v.time_ms >= self.next_call
            {
                decision.chat = Some("tp?");
                self.requested_until = v.time_ms + 5000;
                self.next_call = v.time_ms + 30000;
            }
        }
        let mut choices = Vec::new();
        let inland = Pos {
            x: v.map.width as f32 * 0.5,
            y: v.map.height as f32 * 0.5,
        };
        if v.map.name == "Realm" && v.position.distance(inland) <= 12. {
            self.inland_reached = true;
        }
        if v.map.name == "Realm"
            && progression::ready(v, content, 0x718)
            && !self.inland_reached
            && (self.profile.name == self.profile.leader
                || !party.iter().any(|peer| peer.name == self.profile.leader))
        {
            choices.push((60. + self.profile.resources, Goal::Venture, inland));
        }
        for enemy in &enemies {
            if !progression::can_fight(v, content, enemy) {
                continue;
            }
            let d = enemy.position.distance(v.position);
            let protected = !progression::vulnerable(enemy);
            let score = 45. - d * 2.
                + if d < range {
                    35. + (enemy.max_hp as f32).sqrt().min(20.)
                } else {
                    0.
                };
            choices.push((score, Goal::Fight(enemy.id), enemy.position));
            if v.quest == Some(enemy.id)
                && !protected
                && (v.character.level >= 3 || d <= 48.)
                && (v.map.name != "Realm" || !progression::ready(v, content, 0x718) || d <= 32.)
                && (v.map.name != "Realm"
                    || self.profile.name == self.profile.leader
                    || !party.iter().any(|peer| peer.name == self.profile.leader))
            {
                choices.push((
                    if v.map.name == "Snake Pit" {
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
                if progression::pickup(v, content, bag)?.is_some() {
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
        for portal in v.objects.iter().filter(|p| match v.map.name.as_str() {
            "Nexus" => {
                if banking {
                    p.kind == 0x720
                } else {
                    matches!(p.kind, 0x704 | 0x70e | 0x71c)
                        && (self.profile.name == self.profile.leader
                            || v.time_ms >= self.nexus_until + 30000
                            || self.leader_seen.is_some_and(|(_, position, seen)| {
                                v.time_ms.saturating_sub(seen) < 10000
                                    && position.distance(p.position) < 8.
                            }))
                }
            }
            // The committed exit is public even when a straggler missed personal clear credit.
            "Pirate Cave" | "Snake Pit" => p.kind == 0x71d,
            "Realm" => {
                progression::ready(v, content, p.kind)
                    && (p.kind == 0x717
                        || party.iter().all(|peer| {
                            peer.max_hp >= 350
                                && peer.hp * 4 >= peer.max_hp * 3
                                && peer.level.is_some_and(|level| level >= 14)
                                && peer.equipped[0] >= 0
                                && content.objects[&(peer.equipped[0] as u16)]
                                    .tier
                                    .is_some_and(|tier| tier >= 4)
                        }))
                    && (self
                        .portal
                        .is_some_and(|(id, until)| id == p.id && until > v.time_ms)
                        || (self.profile.name == self.profile.leader
                            && party.len() >= 2
                            && party.iter().all(|q| q.position.distance(p.position) < 8.)))
            }
            _ => false,
        }) {
            let exiting = portal.kind == 0x71d;
            if exiting || (peaceful && health >= 0.75) {
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
                        if reward_waiting { 0. } else { 99. }
                    } else {
                        95.
                    },
                    Goal::Portal(portal.id),
                    portal.position,
                ));
            }
        }
        if self.profile.name == self.profile.leader
            && let Some(straggler) = party
                .iter()
                .filter(|p| p.position.distance(v.position) > 10. || p.hp * 10 < p.max_hp * 7)
                .max_by(|a, b| {
                    a.position
                        .distance(v.position)
                        .total_cmp(&b.position.distance(v.position))
                })
        {
            choices.push((75., Goal::Gather(straggler.id), v.position));
        }
        for player in v
            .objects
            .iter()
            .filter(|o| content.objects[&o.kind].class == "Player")
        {
            let d = player.position.distance(v.position);
            if d > 4.
                && (self.companion == Some(player.id)
                    || (self.profile.name != self.profile.leader
                        && player.name == self.profile.leader))
            {
                let score = if self.companion == Some(player.id) {
                    // An accepted invitation must outrank an existing crew-follow commitment.
                    100.
                } else {
                    45. + self.profile.companions * (d - 4.).min(4.)
                };
                choices.push((score, Goal::Follow(player.id), player.position));
            }
        }
        if self.profile.name == self.profile.leader {
            for (id, (position, _)) in &self.contacts {
                if !party.iter().any(|p| p.id == *id) && v.position.distance(*position) > 3. {
                    choices.push((75., Goal::Gather(*id), *position));
                }
            }
        } else if let Some((id, position, seen)) = self.leader_seen
            && v.time_ms.saturating_sub(seen) < 10000
            && !party.iter().any(|p| p.id == id)
            && v.position.distance(position) > 1.
        {
            choices.push((70., Goal::Follow(id), position));
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
                continue;
            }
            let movement = match goal {
                Goal::Gather(_) if v.position.distance(destination) < 0.7 => None,
                Goal::Portal(id) if v.position.distance(destination) <= 2. => {
                    decision.journey = Some(Journey::Portal(id));
                    None
                }
                Goal::Fight(_) | Goal::Hunt(_)
                    if v.position.distance(destination) < range
                        && shot_clear(v, &grid, destination, projectile.passes_cover) =>
                {
                    combat::movement(v, &grid, &enemies, Some(destination), range, false)
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
                            progression::pickup(v, content, bag)?
                                .context("selected bag has no collectable item")?,
                        );
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
                _ => match if matches!(goal, Goal::Hunt(_) | Goal::Venture) {
                    grid.towards(v.position, destination)
                } else {
                    grid.next(v.position, destination)
                } {
                    Some(pos) => Some(pos),
                    None => {
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
                && self.profile.name == self.profile.leader
            {
                let portal = v
                    .objects
                    .iter()
                    .find(|p| p.id == portal)
                    .context("selected portal missing")?;
                decision.chat = Some(match portal.kind {
                    0x717 => "pc here",
                    0x718 => "snake here",
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
            decision.goal = goal;
            decision.movement = movement;
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
                        self.tracks[&enemy.id].2,
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
fn upgrade(content: &Content, new: i32, old: i32) -> Result<bool> {
    let item = content.get(new as u16)?;
    if item.unsupported_equipment {
        return Ok(false);
    }
    if old < 0 {
        return Ok(true);
    }
    Ok(matches!((item.tier,content.get(old as u16)?.tier),(Some(a),Some(b)) if a>b))
}
fn useful(content: &Content, v: &PlayerView, kind: i32) -> Result<bool> {
    let item = content.get(kind as u16)?;
    if matches!(item.activate.as_str(), "Heal" | "Magic")
        || progression::potion_index(content, kind)?.is_some()
    {
        return Ok(v.character.items.iter().filter(|k| **k == kind).count() < 2);
    }
    if progression::spare(content, kind)?
        && !v.character.items.contains(&kind)
        && item.tier.is_some_and(|tier| tier >= 6)
    {
        return Ok(true);
    }
    let class = content.get(v.character.class)?;
    match class
        .slots
        .iter()
        .take(4)
        .position(|s| *s == item.slot_type)
    {
        Some(slot) => upgrade(content, kind, v.character.items[slot]),
        None => Ok(false),
    }
}
struct Grid {
    walkable: BTreeSet<(i16, i16)>,
    known: BTreeSet<(i16, i16)>,
    dimensions: (i32, i32),
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
    fn reachable(&self, from: Pos) -> BTreeSet<(i16, i16)> {
        let start = tile(from);
        let mut queue = VecDeque::from([start]);
        let mut reached = BTreeSet::from([start]);
        while let Some((x, y)) = queue.pop_front() {
            for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if self.walkable.contains(&next) && reached.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        reached
    }
    fn frontier(&self, (x, y): (i16, i16), width: i32, height: i32) -> bool {
        [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
            .iter()
            .any(|n| {
                n.0 >= 0
                    && n.1 >= 0
                    && i32::from(n.0) < width
                    && i32::from(n.1) < height
                    && !self.known.contains(n)
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
        while let Some((x, y)) = queue.pop_front() {
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
