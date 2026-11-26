use super::*;
use crate::{
    loot::evaluator::{self, Contribution, DefeatContext, Owner},
    store::world_events::{Event, LIMITS, Record, TerminalActions, TerminalPayload},
};

// Per World: one batched IMMEDIATE drain commits ~9,200 events/s vs ~1,060 serial (release, journal-batch-throughput,
// Qc297a364); permits only bound plan memory, which the server-wide allocation bound below already caps.
pub(crate) const PENDING_LIMIT: usize = 64;
// Server-wide: 8 retained maximum plans fit 64 MiB (journal test), whichever worlds retain them.
pub(crate) const PENDING_ALLOCATION_LIMIT: usize = 8 * LIMITS.max_allocations.get();
type EntitlementExpiry = (usize, i32, u64, u64);
pub(crate) struct DeliveryAttempt {
    pub(crate) event: Event,
    pub(crate) expiry: Vec<EntitlementExpiry>,
    published: BTreeSet<usize>,
}
pub(in crate::world) struct Permit {
    count: Arc<std::sync::atomic::AtomicUsize>,
    allocations: Option<(Arc<std::sync::atomic::AtomicUsize>, usize)>,
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
        if let Some((total, count)) = &self.allocations {
            total.fetch_sub(*count, Ordering::AcqRel);
        }
    }
}
/// Terminals wait for their own journal receipt or for bounded journal/progress capacity; these waits are the commit
/// protocol, not failures, and never count toward the preflight failure window.
#[derive(Debug)]
pub(in crate::world) struct TerminalWait(pub String);
impl std::fmt::Display for TerminalWait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for TerminalWait {}
pub(in crate::world) fn pending_level(error: &anyhow::Error) -> tracing::Level {
    if error.downcast_ref::<TerminalWait>().is_some() {
        tracing::Level::DEBUG
    } else {
        tracing::Level::ERROR
    }
}
/// Retried every tick, so commit-protocol waits stay at DEBUG (capture/publication are INFO); other causes are ERROR.
pub(in crate::world) fn log_pending(actor: i32, error: &anyhow::Error, message: &str) {
    if pending_level(error) == tracing::Level::DEBUG {
        tracing::debug!(actor, error = %format!("{error:#}"), "{message}");
    } else {
        tracing::error!(actor, error = %format!("{error:#}"), "{message}");
    }
}
pub(in crate::world) struct Pending {
    pub event: Event,
    pub receipt: bool,
    pub progress_done: BTreeSet<i32>,
    pub next_retry: Instant,
}
#[derive(Clone)]
pub(super) struct Recipient {
    pub player: i32,
    pub character: i32,
    pub position: Pos,
    pub boosted: bool,
    /// Base world.rs:6321-6346: the quest arrow targets the defeated actor, or its quest line's final form.
    pub quest_arrow: bool,
    pub qualified: bool,
}
#[derive(Clone)]
pub(super) struct Facts {
    pub now: u64,
    pub position: Pos,
    pub kind: u16,
    pub ledger: Vec<Contribution>,
    pub recipients: Vec<Recipient>,
    pub killer: Option<i32>,
    pub seed: u64,
}
fn scope(scope: Scope) -> serde_json::Value {
    serde_json::json!({"world":scope.identity.world,"actor":scope.identity.actor,"parent":scope.identity.parent,"encounter":scope.identity.encounter,"issuer":scope.issuer,"order":scope.order,"listener":scope.listener,"generation":scope.generation})
}
/// Base HEAD 79a4c92 src/world.rs god_loot_bags scatter: one defeat's bags must not hide each other.
/// Staying inside the corpse tile keeps the corpse cell's walkability; event+first ordinal keeps republish identical.
fn bag_position(corpse: Pos, event: &str, ordinal: usize) -> Pos {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(format!("{event}:{ordinal}"));
    let coordinate = |value: f32, byte: u8| {
        let low = (value - 0.5).max(value.floor());
        let high = (value + 0.5).min(value.floor() + 1.);
        low + (high - low) * f32::from(byte) / 256.
    };
    Pos {
        x: coordinate(corpse.x, digest[0]),
        y: coordinate(corpse.y, digest[1]),
    }
}
/// The kill statistic's owner: the final blow, or for a self-ending the source rewards (dropLoot/giveXP), the largest
/// eligible damager, as base finish_timed_defeat did (world.rs:6371-6399).
fn kill_owner(plan: &TerminalPlan, killer: Option<i32>, ledger: &[Contribution]) -> Option<i32> {
    match plan.reason {
        TerminalReason::Defeated => killer,
        _ if credited_ending(plan) => ledger
            .iter()
            .filter(|c| c.alive && !c.departing && c.effective_damage > 0)
            .max_by_key(|c| (c.effective_damage, std::cmp::Reverse(c.character)))
            .map(|c| c.character),
        _ => None,
    }
}
/// Defeats and rewarded self-endings (dropLoot/giveXP) end an actor as a kill; other endings credit nobody.
fn placement_held(
    entities: &entity_store::Entities,
    now: u64,
    (bag, slot): (i32, usize),
    deadline: u64,
) -> bool {
    now >= deadline
        || entities
            .get(&bag)
            .is_some_and(|e| e.items.get(slot).is_some_and(|item| *item >= 0))
}
fn credited_ending(plan: &TerminalPlan) -> bool {
    match plan.reason {
        TerminalReason::Defeated => true,
        TerminalReason::Decay | TerminalReason::Explode => plan.rewards.loot || plan.rewards.xp,
        _ => false,
    }
}
pub(in crate::world) const PREFLIGHT_RETRY_MS: u64 = 60_000;
impl World {
    pub(in crate::world) fn xml_bound_terminal_retry(&mut self, actor: i32, error: &anyhow::Error) {
        if pending_level(error) == tracing::Level::DEBUG {
            self.xml.retry_since.remove(&actor);
            log_pending(actor, error, "XML terminal awaits durable Store progress");
            return;
        }
        let first = !self.xml.retry_since.contains_key(&actor);
        let since = *self.xml.retry_since.entry(actor).or_insert(self.now);
        if self.now.saturating_sub(since) >= PREFLIGHT_RETRY_MS {
            if self.xml.abandon_physical.insert(actor) {
                tracing::error!(actor,error=%format!("{error:#}"),"XML terminal retry window elapsed; abandon only impossible physical effects");
            } else {
                tracing::error!(actor,error=%format!("{error:#}"),"XML terminal still pending; holding exact defeat until durable settlement");
            }
            self.xml.retry_since.insert(actor, self.now);
        } else if first {
            tracing::error!(actor,error=%format!("{error:#}"),"XML terminal preflight remains pending");
        } else {
            tracing::debug!(actor,error=%format!("{error:#}"),"XML terminal preflight remains pending");
        }
    }
    pub(super) fn terminal_physical<T>(
        &self,
        actor: i32,
        subject: impl FnOnce() -> serde_json::Value,
        result: Result<T>,
        abandoned: &mut Vec<serde_json::Value>,
    ) -> Result<Option<T>> {
        match result {
            Ok(effect) => Ok(Some(effect)),
            Err(error) if self.xml.abandon_physical.contains(&actor) => {
                let evidence = serde_json::json!({"effect":subject(),"disposition":"physical_effect_abandoned","preflight":format!("{error:#}")});
                tracing::debug!(actor, %evidence, "recording abandoned terminal physical effect");
                abandoned.push(evidence);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    /// Committed entitlements whose bags can never be placed stay unpicked in the journal; only the permit is freed.
    fn bound_delivery(
        &mut self,
        event: &Event,
        ordinal: usize,
        error: anyhow::Error,
    ) -> Result<()> {
        let first = !self.xml.delivery_since.contains_key(&event.id);
        let since = *self
            .xml
            .delivery_since
            .entry(event.id.clone())
            .or_insert(self.now);
        if self.now.saturating_sub(since) < PREFLIGHT_RETRY_MS {
            // One ERROR when a delivery first sticks and one at the bounded abandon; retries in between are DEBUG.
            if first {
                tracing::error!(event=%event.id, ordinal, error=%format!("{error:#}"), "entitlement bag placement blocked; retrying until the bounded abandon");
            } else {
                tracing::debug!(event=%event.id, ordinal, error=%format!("{error:#}"), "entitlement bag placement still blocked");
            }
            return Ok(());
        }
        let evidence = serde_json::json!({"world":self.xml.instance,"event":event.id,"first_unplaced_ordinal":ordinal,"since_ms":since,"failed_ms":self.now,"disposition":"physical_effect_abandoned","preflight":format!("{error:#}")});
        tracing::error!(event=%event.id, %evidence, "entitlement bags still unplaceable after retry window; committed entitlements stay unpicked for operator reconciliation, permit released");
        self.xml.delivery_since.remove(&event.id);
        self.xml.blocked_deliveries.remove(&event.id);
        self.xml.deliveries.remove(&event.id);
        self.release_terminal_event(&event.id);
        Ok(())
    }
    /// A clear actor without hit points holds no ledger (Tomb Boss Anchor, actors.xml:6044); as in base tomb.rs:463-480
    /// its clear credits the damagers of the defeats its compiled noneExist predicates wait on.
    fn xml_clear_waits(&self) -> BTreeSet<u16> {
        let Some(boss) = self
            .xml_world_template()
            .filter(|template| template.clear_key.is_some())
            .and_then(|template| template.clear.as_ref().map(|rule| rule.boss))
            .filter(|boss| !self.content.objects[boss].enemy)
            .filter(|boss| self.entities.values().any(|e| e.kind == *boss))
        else {
            return BTreeSet::new();
        };
        let states = &self.xml_game().definitions[&boss].states;
        (states.iter().flat_map(|s| &s.transitions))
            .flat_map(|t| &t.predicates)
            .filter_map(|p| match p {
                crate::content::game::Predicate::NoneExist(kinds) => Some(kinds),
                _ => None,
            })
            .flatten()
            .copied()
            .collect()
    }
    pub(in crate::world) fn xml_terminal_custody(&self, id: i32) -> bool {
        self.xml.terminal_leases.contains_key(&id)
    }
    pub(super) fn terminal_capacity_ready(&self) -> bool {
        self.xml.terminal_capacity.load(Ordering::Acquire) < PENDING_LIMIT
    }
    pub(super) fn reserve_terminal_actor(&mut self, actor: i32) -> Result<()> {
        if self.xml.terminal_leases.contains_key(&actor) {
            return Ok(());
        }
        self.xml.terminal_capacity.fetch_update(Ordering::AcqRel,Ordering::Acquire,|count|(count<PENDING_LIMIT).then_some(count+1)).map_err(|_|TerminalWait(format!("terminal backpressure actor={actor} pending_events={:?}; original actor/plan retained before publication",self.retained_terminal_ids())))?;
        self.xml.terminal_leases.insert(
            actor,
            (
                Permit {
                    count: self.xml.terminal_capacity.clone(),
                    allocations: None,
                },
                None,
            ),
        );
        Ok(())
    }
    fn retained_terminal_ids(&self) -> Vec<&str> {
        self.xml
            .terminals
            .values()
            .map(|p| p.event.id.as_str())
            .chain(self.xml.stamps.values().map(|p| p.event.id.as_str()))
            .chain(self.xml.deliveries.keys().map(String::as_str))
            .chain(self.xml.terminal_publications.iter().map(String::as_str))
            .collect()
    }
    pub(super) fn release_terminal_event(&mut self, event: &str) {
        if self.xml.deliveries.contains_key(event)
            || self.xml.terminal_publications.contains(event)
            || self.xml.terminals.values().any(|p| p.event.id == event)
            || self.xml.stamps.values().any(|p| p.event.id == event)
        {
            return;
        }
        self.xml
            .terminal_leases
            .retain(|_, (_, id)| id.as_deref() != Some(event));
        self.trim_terminal_claims();
    }
    fn trim_terminal_claims(&mut self) {
        while self.xml.claims.len() > CHILD_LIMIT + self.xml.terminal_leases.len() {
            let actor = self
                .xml
                .claims
                .iter()
                .find(|actor| !self.xml.terminal_leases.contains_key(actor))
                .copied()
                .expect("claim history exceeds protected custody");
            self.xml.claims.remove(&actor);
        }
    }
    pub(in crate::world) fn reserve_terminal_damage(
        &mut self,
        target: i32,
        raw: i32,
        piercing: bool,
    ) -> Result<()> {
        if self.terminal_damage_admission(target, raw, piercing)? {
            self.reserve_terminal_actor(target)?;
        }
        Ok(())
    }
    pub(in crate::world) fn terminal_damage_admission(
        &self,
        target: i32,
        raw: i32,
        piercing: bool,
    ) -> Result<bool> {
        let Some(e) = self.entities.get(&target) else {
            return Ok(false);
        };
        let damage = crate::condition::damage(
            raw,
            self.content.objects[&e.kind].defense,
            e.conditions(self.now),
            piercing,
        );
        ensure!(
            damage < e.hp
                || self.terminal_capacity_ready()
                || self.xml.terminal_leases.contains_key(&target)
                || self.xml.retry_terminals.contains(&target),
            "terminal backpressure actor={target} pending_events={:?}; lethal damage refused before ledger/HP/effects",
            self.retained_terminal_ids()
        );
        Ok(damage >= e.hp && e.hp > 0)
    }
    pub(super) fn terminal_return_route(&self) -> serde_json::Value {
        match self.return_route {
            Some(DungeonReturn::Random) => serde_json::json!({"destination":"random-live-realm"}),
            Some(DungeonReturn::Realm(id)) => {
                serde_json::json!({"destination":"original-realm","world":id})
            }
            None => serde_json::Value::Null,
        }
    }
    pub(crate) fn due_terminal_events(&mut self) -> Vec<Event> {
        let now = Instant::now();
        let mut events = vec![];
        let terminals = self
            .xml
            .terminals
            .values_mut()
            .map(|p| (&mut p.next_retry, &p.event));
        let stamps = self
            .xml
            .stamps
            .values_mut()
            .filter(|p| !p.published)
            .map(|p| (&mut p.next_retry, &p.event));
        for (retry, event) in terminals.chain(stamps) {
            if *retry <= now {
                *retry = now + std::time::Duration::from_secs(1);
                events.push(event.clone());
            }
        }
        events
    }
    pub(crate) fn terminal_journal_ids(&self) -> Vec<String> {
        self.retained_terminal_ids()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
    /// Recipient progress a staged terminal holds settles through its Store journal receipt, not an account reservation.
    pub(crate) fn terminal_holds_progress(
        &self,
        snapshot: &crate::store::Character,
    ) -> Result<bool> {
        let held = serde_json::to_value(snapshot)?;
        for pending in self.xml.terminals.values() {
            let progress = &pending.event.terminal_payload()?.actions["progress"];
            if !pending.progress_done.contains(&snapshot.id)
                && progress
                    .as_array()
                    .is_some_and(|p| p.iter().any(|entry| entry[0] == held))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// The player's in-flight progress, and the level assists it acknowledges, belong to a staged terminal until its receipt.
    pub(crate) fn progress_held_by_terminal(&self, player: i32) -> Result<bool> {
        let p = self
            .players
            .get(&player)
            .context("credit recipient missing")?;
        match &p.progress_snapshot {
            Some(snapshot) if p.pending => self.terminal_holds_progress(snapshot),
            _ => Ok(false),
        }
    }
    #[cfg(test)]
    pub(crate) fn pending_terminal_events(&self) -> Vec<Event> {
        self.xml
            .terminals
            .values()
            .map(|p| p.event.clone())
            .chain(
                self.xml
                    .stamps
                    .values()
                    .filter(|p| !p.published)
                    .map(|p| p.event.clone()),
            )
            .collect()
    }
    pub(super) fn capture_terminal_facts(&mut self, plan: &TerminalPlan, e: &Entity) -> Result<()> {
        self.reserve_terminal_actor(e.id)?;
        let identity = plan.origin.identity;
        if !self.xml.facts.contains_key(&e.id) {
            self.xml_protector_record(e);
            let ledger = e
                .damage
                .iter()
                .map(|(character, damage)| {
                    let p = self.players.values().find(|p| p.character.id == *character);
                    Contribution {
                        character: *character,
                        world: identity.world,
                        encounter: identity.encounter,
                        effective_damage: u64::try_from(*damage).expect("checked damage"),
                        alive: p.is_some_and(|p| p.character.alive && !p.death_pending),
                        departing: p.is_none_or(|p| p.departing),
                    }
                })
                .collect::<Vec<Contribution>>();
            if self.xml_clear_waits().contains(&e.kind) {
                // Captured, not receipted: the waiting clear actor's noneExist fires once this defeat's HP reaches 0.
                self.xml.clear_participants.extend(
                    ledger
                        .iter()
                        .filter(|c| c.effective_damage > 0)
                        .map(|c| c.character),
                );
                ensure!(
                    self.xml.clear_participants.len() <= LIMITS.max_contributions.get(),
                    "clear participation exceeds the contribution bound actor={}",
                    e.id
                );
            }
            let killer = match &plan.context {
                Some(TerminalContext::PlayerKill { player, .. }) => {
                    self.players.get(player).map(|p| p.character.id)
                }
                _ => None,
            };
            let owner = kill_owner(plan, killer, &ledger);
            let recipients = self
                .players
                .iter()
                .filter(|(_, p)| {
                    p.character.alive
                        && !p.death_pending
                        && !p.departing
                        && (p.pos.distance(e.pos) < 25. || Some(p.character.id) == owner)
                })
                .map(|(id, p)| Recipient {
                    player: *id,
                    character: p.character.id,
                    position: p.pos,
                    boosted: p.character.xp_boost.active,
                    quest_arrow: p.quest == Some(e.id),
                    qualified: p.quest == Some(e.id)
                        || (crate::quest::final_root(e.kind)
                            && e.quest.is_some()
                            && p.qualified_quest == *e.quest),
                })
                .collect::<Vec<_>>();
            let seed = self.rng.r#gen::<u64>();
            for recipient in &recipients {
                self.players
                    .get_mut(&recipient.player)
                    .unwrap()
                    .terminal_captures += 1;
            }
            self.entities.get_mut(&e.id).unwrap().reserved = true;
            self.xml.facts.insert(
                e.id,
                Facts {
                    now: self.now,
                    position: e.pos,
                    kind: e.kind,
                    ledger,
                    recipients,
                    killer,
                    seed,
                },
            );
        }
        Ok(())
    }
    pub(super) fn stage_terminal(
        &mut self,
        plan: &TerminalPlan,
        e: &Entity,
        births: &[(u16, Pos, bool)],
        mut abandoned: Vec<serde_json::Value>,
    ) -> Result<()> {
        // Ordinary damage may precede the receipt; only durable terminal effects are gated here.
        if let Some(pending) = self.xml.terminals.get(&e.id) {
            return Err(TerminalWait(format!(
                "terminal event {} awaiting durable receipt; actor={} world={}",
                pending.event.id, e.id, self.xml.instance
            ))
            .into());
        }
        self.capture_terminal_facts(plan, e)?;
        let identity = plan.origin.identity;
        let facts = self.xml.facts[&e.id].clone();
        ensure!(
            facts.kind == e.kind
                && serde_json::to_value(facts.position)? == serde_json::to_value(e.pos)?,
            "captured terminal source kind/position changed"
        );
        let owner = kill_owner(plan, facts.killer, &facts.ledger);
        let ledger = facts.ledger;
        let killer = facts.killer;
        let seed = facts.seed;
        let recipients = facts
            .recipients
            .iter()
            .map(|recipient| {
                let p = self.players.get(&recipient.player).context(
                    "captured terminal recipient departed before progress reconciliation",
                )?;
                ensure!(
                    p.character.id == recipient.character,
                    "captured terminal character changed"
                );
                Ok((recipient, p))
            })
            .collect::<Result<Vec<_>>>()?;
        if !recipients
            .iter()
            .all(|(_, p)| !p.pending && !p.ordinary_rewards_queued())
        {
            return Err(TerminalWait(format!(
                "terminal progress backpressure actor={}; original defeat facts/seed retained",
                e.id
            ))
            .into());
        }
        // Event-local XP draws cannot rewind or consume unrelated World RNG while persistence is pending.
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let template = self.xml_world_template();
        let clear_rule = template
            .and_then(|t| t.clear.as_ref())
            .filter(|r| r.boss == e.kind);
        let clear = clear_rule
            .and(template.and_then(|t| t.clear_key))
            .map(|key| key.index());
        let defeat = DefeatContext {
            world: identity.world,
            encounter: identity.encounter,
            actor: e.id,
            event: plan.origin.generation,
            kind: e.kind,
        };
        let (allocation, planning_error) = if plan.rewards.loot {
            match evaluator::evaluate(self.xml_game(), defeat, &ledger, seed, LIMITS) {
                Ok(plan) => (Some(plan), None),
                Err(error) => (None, Some(format!("{error:#}"))),
            }
        } else {
            (None, None)
        };
        let planned = allocation
            .as_ref()
            .map_or(0, |plan| plan.allocations().len());
        let definition = &self.xml_game().definitions[&e.kind];
        let xp =
            (f64::from(self.content.objects[&e.kind].hp) * definition.xp_multiplier / 10.).ceil();
        ensure!(
            xp.is_finite() && (0. ..=f64::from(i32::MAX)).contains(&xp),
            "terminal XML XP is unrepresentable actor={}",
            e.id
        );
        // A transforming quest actor (a successor inherits the quest) completes nothing, as with base's lethal transforms.
        let quest_ends = credited_ending(plan) && self.xml_quest_heir(births).is_none();
        let mut progress = Vec::new();
        let mut players = Vec::new();
        let mut quest_complete = Vec::new();
        for (recipient, p) in recipients {
            let id = recipient.player;
            let mut ch = p.character.clone();
            let before = ch.level;
            if Some(ch.id) == owner {
                ch.kills = ch
                    .kills
                    .checked_add(1)
                    .context("terminal kill counter overflow")?;
                if self.content.objects[&e.kind].quest {
                    ch.quests = ch
                        .quests
                        .checked_add(1)
                        .context("terminal quest counter overflow")?;
                }
                super::super::enemy_damage::record_character_lifetime_kill(
                    &mut ch,
                    &self.content.objects[&e.kind],
                    plan.reason == TerminalReason::Defeated,
                )?;
            }
            if plan.rewards.xp && recipient.position.distance(e.pos) < 25. {
                let active = ch.xp_boost.active;
                // An earlier pending reward may have reached level 20 and ended the boost (store.rs gain_experience).
                ch.xp_boost.active = recipient.boosted && ch.level < 20;
                ch.gain_experience(&self.content, xp as i32, &mut rng)?;
                ch.xp_boost.active = active && ch.level < 20;
            }
            let damaged = e.damage.get(&ch.id).is_some_and(|d| *d > 0);
            if quest_ends && recipient.position.distance(e.pos) < 25. {
                if recipient.qualified && damaged {
                    ch.lifetime.qualified_quests = ch
                        .lifetime
                        .qualified_quests
                        .checked_add(1)
                        .context("qualified quest counter overflow")?;
                }
                if recipient.quest_arrow {
                    quest_complete.push(id);
                }
            }
            if let Some(index) = clear
                .filter(|_| recipient.position.distance(e.pos) < 25.)
                .filter(|index| {
                    *index == 0 || damaged || self.xml.clear_participants.contains(&ch.id)
                })
            {
                let count = std::iter::once(&mut ch.pirate_caves)
                    .chain(ch.dungeons.iter_mut())
                    .chain(std::iter::once(&mut ch.abysses))
                    .chain(std::iter::once(&mut ch.trenches))
                    .chain(std::iter::once(&mut ch.spider_dens))
                    .chain(std::iter::once(&mut ch.mad_labs))
                    .chain(std::iter::once(&mut ch.jungles))
                    .chain(std::iter::once(&mut ch.manors))
                    .chain(std::iter::once(&mut ch.beaches))
                    .chain(std::iter::once(&mut ch.davy_lockers))
                    .chain(std::iter::once(&mut ch.tombs))
                    .nth(index)
                    .context("clear progress key absent")?;
                *count = count
                    .checked_add(1)
                    .context("terminal clear counter overflow")?;
            }
            let assists = if e.damage.get(&ch.id).is_some_and(|d| *d > 0) {
                killer
                    .filter(|giver| *giver != ch.id)
                    .map(|giver| {
                        (before + 1..=ch.level)
                            .map(|level| LevelAssist {
                                reached_level: level as u8,
                                giver,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            };
            if ch != p.character {
                players.push(id);
                progress.push((ch, assists));
            }
        }
        let clear_registration = clear_rule.is_some();
        let portals = births
            .iter()
            .filter(|(kind, _, _)| self.xml_is_portal(*kind))
            .map(|(kind, pos, _)| Ok((*kind, *pos, self.xml_portal_intent(*kind)?)))
            .collect::<Result<Vec<_>>>()?;
        let clear_portal = if let Some(crate::content::game::ClearRoute::RealmExit(kind)) =
            clear_rule.map(|rule| rule.route)
        {
            let placement = self.clear_portal_position(e.pos, false).and_then(|pos| {
                self.xml_preflight_portal(kind, pos, "journaled native dungeon clear entitlement")?;
                Ok((kind, pos))
            });
            self.terminal_physical(
                e.id,
                || serde_json::json!({"kind":kind,"intent":"dungeon_clear_portal","origin":facts.position}),
                placement,
                &mut abandoned,
            )?
        } else {
            None
        };
        let local = progress.is_empty()
            && planned == 0
            && planning_error.is_none()
            && births.is_empty()
            && abandoned.is_empty()
            && plan.hooks.is_empty()
            && plan.announcements.is_empty()
            && clear.is_none()
            && !clear_registration
            && clear_portal.is_none()
            && portals.is_empty()
            && quest_complete.is_empty()
            && e.quest.is_none();
        if local {
            // Decay waves end ~1.1k satellites at once: building and hashing an unused journal event was the tick cost.
            tracing::debug!(actor = e.id, kind = e.kind, reason = ?plan.reason, contributions = ledger.len(), recipients = facts.recipients.len(), world = %self.xml.instance, "effect-free XML terminal publishes without a journal event");
            return Ok(());
        }
        let actions = TerminalActions {
            planning_error,
            instance: self.xml.instance.clone(),
            actor: e.id,
            loot: plan.rewards.loot,
            progress,
            publication: serde_json::json!({"position":e.pos,"defeat_recipients":facts.recipients.iter().map(|r|serde_json::json!({"player":r.player,"character":r.character,"position":r.position,"xp_boost_active":r.boosted})).collect::<Vec<_>>(),"kind":e.kind,"template":self.xml.template,"return_route":self.terminal_return_route(),"births":births,"abandoned_births":abandoned,"announcements":plan.announcements.iter().map(|a|serde_json::json!({"owner":{"scope":scope(a.key.owner.scope),"local":a.key.owner.local},"generation":a.key.generation,"category":format!("{:?}",a.key.category),"list":a.list,"choice":a.choice,"text":a.text,"source":a.source})).collect::<Vec<_>>(),"hooks":plan.hooks.iter().map(|h|serde_json::json!({"owner":{"scope":scope(h.owner.scope),"local":h.owner.local},"behavior":h.behavior,"seed":h.seed})).collect::<Vec<_>>(),"initiator":plan.behavior,"xp":plan.rewards.xp,"clear":clear,"clear_registration":clear_registration,"clear_portal":clear_portal,"portals":portals,"quest_complete":quest_complete,"reason":format!("{:?}",plan.reason),"parent":identity.parent,"encounter":identity.encounter,"now":facts.now}),
        };
        let event = Event::terminal(
            &self.xml_game().fingerprint,
            TerminalPayload {
                defeat,
                ledger,
                seed,
                allocation,
                actions: serde_json::to_value(&actions)?,
            },
        )?;
        if self
            .xml
            .terminal_allocations
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |total| {
                (total + planned <= PENDING_ALLOCATION_LIMIT).then_some(total + planned)
            })
            .is_err()
        {
            return Err(TerminalWait(format!(
                "terminal allocation backpressure actor={} planned={planned} server_pending={} limit={PENDING_ALLOCATION_LIMIT}; original defeat facts/seed retained",
                e.id,
                self.xml.terminal_allocations.load(Ordering::Acquire)
            ))
            .into());
        }
        self.xml
            .terminal_leases
            .get_mut(&e.id)
            .unwrap()
            .0
            .allocations = Some((self.xml.terminal_allocations.clone(), planned));
        for (player, (snapshot, assists)) in players.into_iter().zip(&actions.progress) {
            let p = self.players.get_mut(&player).unwrap();
            p.pending = true;
            p.progress_started = true;
            p.progress_snapshot = Some(snapshot.clone());
            p.level_assists_prepared = assists.clone();
        }
        self.entities.get_mut(&e.id).unwrap().reserved = true;
        tracing::info!(event=%event.id,actor=e.id,world=%self.xml.instance,"terminal intent captured before native publication");
        let id = event.id.clone();
        self.xml.terminal_leases.get_mut(&e.id).unwrap().1 = Some(id.clone());
        self.xml.terminals.insert(
            e.id,
            Pending {
                event,
                receipt: false,
                progress_done: BTreeSet::new(),
                next_retry: Instant::now(),
            },
        );
        Err(TerminalWait(format!("terminal event {id} awaiting durable receipt")).into())
    }
    pub(crate) fn finish_terminal_event(&mut self, record: Record) -> Result<()> {
        if record.event.kind == "stamp" {
            return self.finish_stamp_event(record);
        }
        let payload = record.event.terminal_payload()?;
        let actions: TerminalActions = serde_json::from_value(payload.actions.clone())?;
        ensure!(
            matches!(record.status, 1 | 2)
                && actions.instance == self.xml.instance
                && record.event.content == self.xml_game().fingerprint,
            "terminal receipt scope/catalogue/commit mismatch event={}",
            record.event.id
        );
        ensure!(
            actions.publication["return_route"] == self.terminal_return_route(),
            "terminal receipt original return route changed event={}",
            record.event.id
        );
        let actor = actions.actor;
        if !self.xml.terminals.contains_key(&actor)
            && (record.status == 2 || self.xml.claims.contains(&actor))
        {
            return Ok(());
        }
        let pending = self
            .xml
            .terminals
            .get_mut(&actor)
            .context("terminal receipt has no original pending native plan")?;
        ensure!(
            pending.event == record.event,
            "terminal receipt differs from captured plan/seed/ledger"
        );
        pending.receipt = true;
        if let Some((kind, pos)) = serde_json::from_value::<Option<(u16, Pos)>>(
            actions.publication["clear_portal"].clone(),
        )? && !self.xml.claims.contains(&actor)
            && !self.xml.abandon_physical.contains(&actor)
        {
            self.xml_preflight_portal(kind, pos, "committed clear intent preflight")?;
        }
        if let Some(entity) = self.entities.get(&actor) {
            ensure!(
                entity.kind == payload.defeat.kind
                    && serde_json::to_value(entity.pos)? == actions.publication["position"],
                "terminal receipt original source identity/position changed event={}",
                record.event.id
            );
        }
        if self.xml.claims.contains(&actor) {
            self.xml.actors.remove(&actor);
        }
        if !self.xml.claims.contains(&actor) {
            let mut executor = self.xml_actor(actor)?;
            let result = executor.retry_terminal(self);
            if !executor.retired() {
                self.xml.actors.insert(actor, executor);
            }
            result?;
        }
        let publication = &actions.publication;
        if !self.xml.terminals[&actor].progress_done.contains(&-1) {
            if publication["clear_registration"] == true || !publication["clear"].is_null() {
                self.cleared = true;
                self.xml.clear_participants.clear();
                self.publish_endgame_exits();
            }
            for player in serde_json::from_value::<Vec<i32>>(publication["quest_complete"].clone())?
            {
                if let Some(p) = self.players.get(&player) {
                    // This exact notification also starts the native client's quest-arrow completion delay.
                    let mut out = Writer::new();
                    out.i32(player).utf("Quest Complete!").u32(0xff00ff00);
                    send(p, wire::NOTIFICATION, out.0);
                }
            }
            self.xml
                .terminals
                .get_mut(&actor)
                .unwrap()
                .progress_done
                .insert(-1);
        }
        for (snapshot, _) in actions.progress {
            if self.xml.terminals[&actor]
                .progress_done
                .contains(&snapshot.id)
            {
                continue;
            }
            let player = self
                .players
                .iter()
                .find(|(_, p)| p.character.id == snapshot.id)
                .map(|(id, _)| *id)
                .context(
                    "terminal progress owner departed before receipt; original event retained",
                )?;
            self.finish_progress(player, snapshot.clone())?;
            self.xml
                .terminals
                .get_mut(&actor)
                .unwrap()
                .progress_done
                .insert(snapshot.id);
        }
        if (publication["clear_registration"] == true || !publication["clear"].is_null())
            && let Some(exit) = self.clear_exit.take()
        {
            self.entities
                .get_mut(&exit)
                .context("committed clear portal missing")?
                .published = true;
        }
        if !payload.allocations().is_empty() {
            self.xml
                .deliveries
                .insert(record.event.id.clone(), record.event.clone());
        }
        self.xml
            .terminal_publications
            .insert(record.event.id.clone());
        // A defeated stamp source never re-runs its Map acknowledgement, so settle its published stamp here.
        if self
            .xml
            .stamps
            .get(&actor)
            .is_some_and(|stamp| stamp.published)
        {
            let stamp = self.xml.stamps.remove(&actor).unwrap().event.id;
            self.release_terminal_event(&stamp);
        }
        self.xml.terminals.remove(&actor);
        self.release_terminal_facts(actor)
    }
    pub(super) fn finish_local_terminal(&mut self, actor: i32) -> Result<()> {
        self.release_terminal_facts(actor)?;
        self.xml.terminal_leases.remove(&actor);
        self.trim_terminal_claims();
        Ok(())
    }
    fn release_terminal_facts(&mut self, actor: i32) -> Result<()> {
        if let Some(facts) = self.xml.facts.remove(&actor) {
            for recipient in facts.recipients {
                let p = self
                    .players
                    .get_mut(&recipient.player)
                    .context("captured terminal recipient lost before publication")?;
                p.terminal_captures = p
                    .terminal_captures
                    .checked_sub(1)
                    .context("terminal capture hold lost")?;
            }
        }
        Ok(())
    }
    pub(crate) fn world_instance(&self) -> String {
        self.xml.instance.clone()
    }
    pub(crate) fn terminal_publications(&self) -> Vec<String> {
        self.xml.terminal_publications.iter().cloned().collect()
    }
    pub(crate) fn acknowledge_terminal_publication(&mut self, id: String) {
        self.xml.terminal_publications.remove(&id);
        self.release_terminal_event(&id);
    }
    pub(crate) fn entitlement_expiries(&self, event: &Event) -> Vec<EntitlementExpiry> {
        self.xml
            .bag_entitlements
            .iter()
            .filter(|((bag, _), (id, _, _, deadline))| {
                id == &event.id && self.now >= *deadline && !self.entities.contains_key(bag)
            })
            .map(|((bag, _), (_, ordinal, _, deadline))| (*ordinal, *bag, *deadline, self.now))
            .collect()
    }
    fn delivery_attempt(&self, event: &Event) -> DeliveryAttempt {
        DeliveryAttempt {
            event: event.clone(),
            expiry: self.entitlement_expiries(event),
            published: self
                .xml
                .bag_entitlements
                .iter()
                .filter(|(slot, (id, _, _, deadline))| {
                    id == &event.id && placement_held(&self.entities, self.now, **slot, *deadline)
                })
                .map(|(_, (_, ordinal, _, _))| *ordinal)
                .collect(),
        }
    }
    pub(crate) fn delivery_batch(&self) -> Vec<DeliveryAttempt> {
        self.xml
            .deliveries
            .values()
            .map(|event| self.delivery_attempt(event))
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn delivery_events(&self) -> Vec<Event> {
        self.xml.deliveries.values().cloned().collect()
    }
    #[cfg(test)]
    pub(crate) fn publish_entitlements(
        &mut self,
        event: Event,
        rows: Vec<crate::store::world_events::Entitlement>,
    ) -> Result<()> {
        self.publish_delivery(self.delivery_attempt(&event), rows)
    }
    pub(crate) fn publish_delivery(
        &mut self,
        attempt: DeliveryAttempt,
        rows: Vec<crate::store::world_events::Entitlement>,
    ) -> Result<()> {
        let DeliveryAttempt {
            event, published, ..
        } = attempt;
        let payload = event.terminal_payload()?;
        let actions: TerminalActions = serde_json::from_value(payload.actions.clone())?;
        ensure!(
            actions.instance == self.xml.instance,
            "loot delivery origin unavailable event={}; retain for reconciliation",
            event.id
        );
        ensure!(
            rows.windows(2)
                .all(|pair| pair[0].ordinal < pair[1].ordinal),
            "entitlement delivery requires original ascending ordinals"
        );
        // A bag can expire during the Store read; keep its placement proof until the Store settles it.
        let entities = &self.entities;
        let now = self.now;
        self.xml
            .bag_entitlements
            .retain(|slot, (owner_event, ordinal, _, deadline)| {
                owner_event != &event.id
                    || placement_held(entities, now, *slot, *deadline)
                        && rows
                            .binary_search_by_key(ordinal, |row| row.ordinal)
                            .is_ok()
            });
        if rows.is_empty() {
            self.xml.blocked_deliveries.remove(&event.id);
            self.xml.deliveries.remove(&event.id);
            self.release_terminal_event(&event.id);
            return Ok(());
        }
        if self.bag_expiry.len() >= 128 {
            if self.xml.blocked_deliveries.insert(event.id.clone()) {
                tracing::warn!(event=%event.id,live_bags=self.bag_expiry.len(),"native bag backpressure; exact undelivered suffix retained");
            }
            return Ok(());
        }
        let mut groups: BTreeMap<Owner, Vec<crate::store::world_events::Entitlement>> =
            BTreeMap::new();
        for row in rows {
            let allocation = payload
                .allocations()
                .get(row.ordinal)
                .context("delivery ordinal absent from immutable allocation")?;
            ensure!(
                allocation.item() == row.item && allocation.owner() == row.owner,
                "delivery item/owner differs from plan"
            );
            // A pickup can publish and erase its slot after the Store answer was queued.
            if published.contains(&row.ordinal) {
                continue;
            }
            groups.entry(row.owner).or_default().push(row);
        }
        let pos: Pos = serde_json::from_value(actions.publication["position"].clone())?;
        let source = self.content.get(payload.defeat.kind)?.name.clone();
        let mut batches = 0;
        for (_, rows) in groups {
            for chunk in rows.chunks(8) {
                if self.bag_expiry.len() >= 128 {
                    if self.xml.blocked_deliveries.insert(event.id.clone()) {
                        tracing::warn!(event=%event.id,live_bags=self.bag_expiry.len(),"native bag backpressure; exact undelivered suffix retained");
                    }
                    return Ok(());
                }
                if batches >= 4 {
                    return Ok(());
                }
                self.xml.blocked_deliveries.remove(&event.id);
                let bag = match self.loot_bag(
                    bag_position(pos, &event.id, chunk[0].ordinal),
                    chunk[0].account,
                    chunk.iter().map(|r| i32::from(r.item)).collect(),
                ) {
                    Ok(bag) => bag,
                    Err(error) => return self.bound_delivery(&event, chunk[0].ordinal, error),
                };
                self.xml.delivery_since.remove(&event.id);
                let e = self.entities.get_mut(&bag).unwrap();
                e.drop_slots = (0..chunk.len()).collect();
                e.drop_source = Some(source.clone());
                for (slot, row) in chunk.iter().enumerate() {
                    self.xml.bag_entitlements.insert(
                        (bag, slot),
                        (
                            event.id.clone(),
                            row.ordinal,
                            row.owner,
                            self.bag_expiry[&bag],
                        ),
                    );
                }
                batches += 1;
            }
        }
        Ok(())
    }
    pub(crate) fn check_entitlement_slot(&self, player: i32, slot: Slot) -> Result<()> {
        let character = self
            .players
            .get(&player)
            .context("item owner absent")?
            .character
            .id;
        if let Some(deadline) = self.bag_expiry.get(&slot.object) {
            ensure!(
                *deadline > self.now,
                "native ground container lifetime expired"
            );
        }
        if let Some((_, _, Owner::Character(owner), _)) =
            self.xml.bag_entitlements.get(&(slot.object, slot.slot))
        {
            ensure!(
                *owner == character,
                "private entitlement belongs to original durable character"
            );
        }
        Ok(())
    }
}
