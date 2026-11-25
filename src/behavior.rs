//! Generic execution of linked rules. Native services must reject unsupported effects, not emulate legacy AI.
use crate::content::game::{
    AdmissionRole, AnnouncementRules, Behavior, BehaviorKind as K, Condition, Definition,
    Field as F, LocalId, Predicate, Source, StateId, TerminalAnnouncements, TextPart, Value,
};
use anyhow::{Context, Result, bail, ensure};
use rand::{Rng, RngCore, SeedableRng, rngs::StdRng};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Identity {
    pub world: u64,
    pub actor: i32,
    pub parent: Option<i32>,
    pub encounter: Option<u64>,
}
/// A listener executes issuer rules on itself; namespace prevents colliding issuer/listener locals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub identity: Identity,
    pub issuer: i32,
    pub order: Option<LocalId>,
    /// Distinct repeated FollowOrders expansions on one actor must never alias native/local state.
    pub listener: Option<LocalId>,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Token {
    pub scope: Scope,
    pub local: LocalId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline {
    pub owner: Token,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub delta_ms: u64,
    pub dormant: bool,
    pub stasis: bool,
    pub stunned: bool,
}
#[derive(Clone, Debug)]
pub struct Offer {
    pub issuer: Identity,
    pub definition: Arc<Definition>,
    pub local: LocalId,
    pub state: StateId,
    pub generation: u64,
    pub targets: Option<Vec<u16>>,
    pub distance: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Movement,
    Attack,
    Spawn,
    Aura,
    Heal,
    Map,
    Ground,
    Delete,
    DamageRecord,
    Visual,
    Speech,
    Sound,
    Visibility,
    Breath,
    RemoveOnEmpty,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalReason {
    Decay,
    Metamorphose,
    Explode,
    Defeated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rewards {
    pub loot: bool,
    pub xp: bool,
}
#[derive(Clone, Debug)]
pub struct DeathHook {
    pub owner: Token,
    pub behavior: Behavior,
    pub seed: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalContext {
    NonPlayer,
    /// World must construct this from the actual damage event, never parent/nearest-player guesses.
    PlayerKill {
        world: u64,
        player: i32,
        name: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnnouncementCategory {
    New,
    Killed,
    Death,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnnouncementKey {
    pub owner: Token,
    pub generation: u64,
    pub category: AnnouncementCategory,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub key: AnnouncementKey,
    pub list: u32,
    pub choice: usize,
    pub text: String,
    pub source: Source,
}
#[derive(Clone, Debug)]
pub struct TerminalPlan {
    pub origin: Scope,
    pub context: Option<TerminalContext>,
    pub announcements: Vec<Announcement>,
    pub reason: TerminalReason,
    pub rewards: Rewards,
    pub behavior: Option<Behavior>,
    pub hooks: Vec<DeathHook>,
}
/// The world owns once-only terminal commit, child caps/release and durable rewards (phase 4).
#[derive(Debug)]
pub enum Effect<'a> {
    Enter(StateId),
    Exit(StateId),
    /// Also cancels this owner's native motion and world lifecycle deadlines.
    ForgetLocal(Token),
    ScheduleLifecycle {
        deadline: Deadline,
        delay_ms: u64,
    },
    Condition {
        owner: Token,
        condition: &'a Condition,
        enabled: bool,
    },
    GlobalTag {
        symbol: u32,
        present: bool,
        notify: bool,
    },
    PublishOrder {
        state: StateId,
        local: LocalId,
        generation: u64,
        targets: Option<&'a [u16]>,
    },
    WithdrawOrder(LocalId),
    ClearAltTexture,
    Size(f64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionOutcome {
    /// Unsuccessful attempt: native candidates must be preflighted or fully rolled back, including
    /// motion/position, packets, conditions/tags, spawns/caps and terminal effects. Core rewinds
    /// only this attempt's local RNG; it spends no opportunity, cooldown or bucket claim.
    Blocked,
    /// Retained native operation: freeze this actor and retry only this opportunity until applied.
    Pending,
    Applied,
    /// Explicit nondeath retirement (for example RemoveOnEmpty), never a reward-bearing defeat.
    Removed,
}
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub token: Token,
    pub active_ms: u64,
    pub age_ms: u64,
    pub delta_ms: u64,
    pub emissions: u64,
    /// Say chooses the linked semicolon alternative only after a passing chance/preflight.
    pub text_choice: Option<usize>,
}
/// Native implementation uses numeric linked fields only. `ready` is read-only: known blocked/no-target
/// checks precede RNG draws. `apply` must apply an effect, return side-effect-free `Blocked` for a
/// rejected candidate, or return a contextual error. A real error poisons the actor, not a blocked attempt.
/// World queries include only live eligible actors in scope.identity.world. Parent and encounter
/// membership are distinct, immutable ownership selectors, never nearest-encounter substitutions.
pub trait World {
    fn players(&self, scope: Scope) -> Result<Vec<f64>>;
    fn hp_fraction(&self, scope: Scope) -> Result<f64>;
    fn exists(&self, scope: Scope, kinds: &[u16]) -> Result<bool>;
    fn ground(&self, scope: Scope) -> Result<u8>;
    fn global_tag(&self, scope: Scope, symbol: u32) -> Result<bool>;
    fn player_says(&self, scope: Scope, text: &str) -> Result<bool>;
    /// Return current live offers; issuer retirement/state exit must invalidate discovery immediately.
    fn orders(&self, scope: Scope) -> Result<Vec<Offer>>;
    fn size(&self, scope: Scope) -> Result<f64>;
    fn ready(&self, service: Service, behavior: &Behavior, step: Step) -> Result<bool>;
    /// Advance an already-started movement without restarting it or drawing RNG, even while cooling.
    /// Return true only for observed ongoing movement; false allows a new start or the next bucket alternative.
    fn advance(&mut self, behavior: &Behavior, step: Step) -> Result<bool>;
    fn apply(
        &mut self,
        service: Service,
        behavior: &Behavior,
        step: Step,
        rng: &mut StdRng,
    ) -> Result<ActionOutcome>;
    fn effect(&mut self, scope: Scope, effect: Effect<'_>) -> Result<()>;
    /// World-scoped publication and activation-key claim must be idempotent even after an error.
    fn announce(&mut self, scope: Scope, announcement: &Announcement) -> Result<()>;
    /// The terminal claim commits together with the plan's announcements.
    fn terminal(&mut self, scope: Scope, plan: &TerminalPlan) -> Result<()>;
}
/// Seeded on first draw: most of a Realm's executors never draw, and an inline StdRng is 320 bytes.
#[derive(Clone, Debug)]
enum LazyRng {
    Seed(u64),
    Rng(Box<StdRng>),
}
impl LazyRng {
    fn get(&mut self) -> &mut StdRng {
        if let Self::Seed(seed) = *self {
            *self = Self::Rng(Box::new(StdRng::seed_from_u64(seed)));
        }
        let Self::Rng(rng) = self else { unreachable!() };
        rng
    }
}
impl RngCore for LazyRng {
    fn next_u32(&mut self) -> u32 {
        self.get().next_u32()
    }
    fn next_u64(&mut self) -> u64 {
        self.get().next_u64()
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.get().fill_bytes(dest)
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.get().try_fill_bytes(dest)
    }
}
#[derive(Debug)]
struct Local {
    next: u64,
    emissions: u64,
    opportunities: u64,
    condition: bool,
    expires: Option<u64>,
    rng: LazyRng,
    listener: Option<Box<Actor>>,
}
#[derive(Debug)]
pub struct Actor {
    definition: Arc<Definition>,
    root: StateId,
    kind: u16,
    scope: Scope,
    seed: u64,
    rng: LazyRng,
    path: Vec<StateId>,
    entered: BTreeMap<StateId, u64>,
    trials: BTreeMap<(StateId, usize), u64>,
    generations: BTreeMap<StateId, u64>,
    epoch: u64,
    locals: BTreeMap<LocalId, Local>,
    tags: BTreeSet<u32>,
    now: u64,
    started: bool,
    // A state entered by transition updates next tick; its zero-delay lifecycle must follow that update.
    transitioning: bool,
    retired: bool,
    failed: bool,
    waiting: Option<LocalId>,
    // Boxed: an inline plan (with its initiator Behavior) would sit in every idle executor.
    pending: Option<Box<TerminalPlan>>,
    admissions: BTreeMap<AnnouncementKey, Announcement>,
    admission_failed: bool,
}
impl Actor {
    pub fn new(definition: Arc<Definition>, identity: Identity, seed: u64) -> Result<Self> {
        let context = format!(
            "{}:{} object={} kind={:#x}: Actor admission",
            definition.source.file,
            definition.source.line,
            definition.source.object,
            definition.kind
        );
        let role = definition
            .admission_role()
            .with_context(|| context.clone())?;
        ensure!(
            role == AdmissionRole::Scripted,
            "{context}: requires Scripted admission, got {role:?}"
        );
        validate(&definition)?;
        let root = definition.root;
        let kind = definition.kind;
        Ok(Self::scoped(
            definition,
            root,
            kind,
            Scope {
                identity,
                issuer: identity.actor,
                order: None,
                listener: None,
                generation: 0,
            },
            seed,
        ))
    }
    fn scoped(
        definition: Arc<Definition>,
        root: StateId,
        kind: u16,
        scope: Scope,
        seed: u64,
    ) -> Self {
        Self {
            definition,
            root,
            kind,
            scope,
            seed,
            rng: LazyRng::Seed(seed),
            path: vec![],
            entered: BTreeMap::new(),
            trials: BTreeMap::new(),
            generations: BTreeMap::new(),
            epoch: 0,
            locals: BTreeMap::new(),
            tags: BTreeSet::new(),
            now: 0,
            started: false,
            transitioning: false,
            retired: false,
            failed: false,
            waiting: None,
            pending: None,
            admissions: BTreeMap::new(),
            admission_failed: false,
        }
    }
    pub fn path(&self) -> &[StateId] {
        &self.path
    }
    pub fn active_ms(&self) -> u64 {
        self.now
    }
    pub fn retired(&self) -> bool {
        self.retired
    }
    /// Started, healthy and with nothing queued: a dormant or stasised tick does no work.
    pub fn idle_when_dormant(&self) -> bool {
        self.started
            && !self.failed
            && !self.retired
            && self.waiting.is_none()
            && self.admissions.is_empty()
    }
    #[cfg(test)]
    pub fn has_tag(&self, symbol: u32) -> bool {
        self.tags.contains(&symbol)
    }
    pub fn definition(&self) -> &Arc<Definition> {
        &self.definition
    }
    fn context(&self, source: &Source) -> String {
        format!(
            "{source} world={} actor={} issuer={} order={:?} listener={:?} generation={}",
            self.scope.identity.world,
            self.scope.identity.actor,
            self.scope.issuer,
            self.scope.order,
            self.scope.listener,
            self.scope.generation
        )
    }
    fn token(&self, local: LocalId) -> Token {
        Token {
            scope: self.scope,
            local,
        }
    }
    fn state(&self, id: StateId) -> Result<&crate::content::game::State> {
        linked_state(&self.definition, id)
    }
    fn sample_announcement(
        &mut self,
        b: &Behavior,
        generation: u64,
        rules: &AnnouncementRules,
        category: AnnouncementCategory,
        choices: &[Vec<TextPart>],
        context: Option<&TerminalContext>,
    ) -> Result<Announcement> {
        ensure!(
            !choices.is_empty(),
            "empty linked announcement category {category:?}"
        );
        if category == AnnouncementCategory::Killed {
            let Some(TerminalContext::PlayerKill { .. }) = context else {
                bail!("killed announcement needs explicit player-kill context")
            };
        }
        if let Some(context) = context {
            validate_terminal_context(self.scope, context)?;
        }
        let local = self
            .locals
            .get_mut(&b.local)
            .context("announcement local absent")?;
        let choice = if choices.len() == 1 {
            0
        } else {
            local.rng.gen_range(0..choices.len())
        };
        let mut text = String::new();
        for part in &choices[choice] {
            match part {
                TextPart::Literal(literal) => text.push_str(literal),
                TextPart::Killer => match context {
                    Some(TerminalContext::PlayerKill { name, .. }) => text.push_str(name),
                    _ => bail!("announcement has unbound KILLER"),
                },
                _ => bail!("announcement has unbound token {part:?}"),
            }
        }
        Ok(Announcement {
            key: AnnouncementKey {
                owner: self.token(b.local),
                generation,
                category,
            },
            list: rules.list,
            choice,
            text,
            source: b.source.clone(),
        })
    }
    fn flush_admissions(&mut self, world: &mut impl World) -> Result<()> {
        while let Some((key, announcement)) = self
            .admissions
            .first_key_value()
            .map(|(k, a)| (*k, a.clone()))
        {
            if let Err(error) = world.announce(announcement.key.owner.scope, &announcement) {
                self.admission_failed = true;
                return Err(error).with_context(|| self.context(&announcement.source));
            }
            self.admissions.remove(&key);
        }
        Ok(())
    }
    /// Only publication failures are resumable here; generic executor failures remain poisoned.
    pub fn retry_admissions(&mut self, world: &mut impl World) -> Result<()> {
        ensure!(
            !self.failed || self.admission_failed,
            "executor failed outside admission publication"
        );
        self.flush_admissions(world)?;
        for local in self.locals.values_mut() {
            if let Some(listener) = local.listener.as_mut().filter(|l| l.admission_failed) {
                listener.retry_admissions(world)?;
            }
        }
        self.admission_failed = false;
        self.failed = false;
        Ok(())
    }
    fn enter(&mut self, mut id: StateId, world: &mut impl World) -> Result<()> {
        loop {
            let definition = Arc::clone(&self.definition);
            let state = linked_state(&definition, id)?;
            ensure!(!self.path.contains(&id), "linked state cycle");
            self.path.push(id);
            self.entered.insert(id, self.now);
            self.epoch = self
                .epoch
                .checked_add(1)
                .context("state generation overflow")?;
            self.generations.insert(id, self.epoch);
            for b in &state.behaviors {
                validate_schedule(b).with_context(|| self.context(&b.source))?;
                // MakeMinions Clear() zeroes its cooldown, so even startWithMinions=false emits on the first update (behavior.cc:1064-1095).
                let next = time(b, F::Offset)?.unwrap_or(0);
                ensure!(
                    self.locals
                        .insert(
                            b.local,
                            Local {
                                next: self.now.checked_add(next).context("deadline overflow")?,
                                emissions: 0,
                                opportunities: 0,
                                condition: false,
                                expires: None,
                                rng: LazyRng::Seed(local_seed(self.seed, b.local)),
                                listener: None
                            }
                        )
                        .is_none(),
                    "duplicate active behavior local {}",
                    b.local
                );
            }
            world
                .effect(self.scope, Effect::Enter(id))
                .with_context(|| self.context(&state.source))?;
            for b in &state.behaviors {
                if b.kind == K::OryxTaunt {
                    let rules = b
                        .announcement
                        .as_ref()
                        .context("OryxTaunt missing linked announcement rules")?;
                    let announcement = self.sample_announcement(
                        b,
                        self.generations[&id],
                        rules,
                        AnnouncementCategory::New,
                        &rules.admission,
                        None,
                    )?;
                    ensure!(
                        self.admissions
                            .insert(announcement.key, announcement)
                            .is_none(),
                        "duplicate admission activation key"
                    );
                }
                if matches!(b.kind, K::Decay | K::Metamorphose) {
                    world
                        .effect(
                            self.scope,
                            Effect::ScheduleLifecycle {
                                deadline: Deadline {
                                    owner: self.token(b.local),
                                    generation: self.epoch,
                                },
                                delay_ms: required(b, F::Time, time)?
                                    .max(u64::from(self.transitioning)),
                            },
                        )
                        .with_context(|| self.context(&b.source))?;
                }
                if b.kind == K::ConditionEffect
                    && b.bucket.is_none()
                    && time(b, F::Offset)?.unwrap_or(0) == 0
                {
                    let token = self.token(b.local);
                    let local = self
                        .locals
                        .get_mut(&b.local)
                        .context("missing condition local")?;
                    enable_condition(b, token, self.now, local, world)?;
                    local.emissions = 1;
                    local.next = self
                        .now
                        .checked_add(time(b, F::Cooldown)?.unwrap_or(0))
                        .context("condition cooldown overflow")?;
                }
                // behavior.cc builds protectors in the behavior constructor, not on a later (dormant) update.
                if b.kind == K::MaintainProtectors {
                    let step = Step {
                        token: self.token(b.local),
                        active_ms: self.now,
                        age_ms: 0,
                        delta_ms: 0,
                        emissions: 0,
                        text_choice: None,
                    };
                    let local = self
                        .locals
                        .get_mut(&b.local)
                        .context("missing protector local")?;
                    if world.ready(Service::Spawn, b, step)?
                        && world.apply(Service::Spawn, b, step, local.rng.get())?
                            == ActionOutcome::Applied
                    {
                        local.emissions = 1;
                        local.next = self
                            .now
                            .checked_add(time(b, F::Cooldown)?.unwrap_or(0))
                            .context("protector cooldown overflow")?;
                    }
                }
            }
            match state.children.first() {
                Some(child) => id = *child,
                None => return Ok(()),
            }
        }
    }
    fn exit(&mut self, world: &mut impl World) -> Result<()> {
        let id = self.path.pop().context("exit without active state")?;
        let definition = Arc::clone(&self.definition);
        let state = linked_state(&definition, id)?;
        for b in &state.behaviors {
            match self.locals.remove(&b.local) {
                Some(local) => self
                    .release(b, local, world)
                    .with_context(|| self.context(&b.source))?,
                None => ensure!(self.retired, "missing active behavior local {}", b.local),
            }
        }
        for order in &state.orders {
            world.effect(self.scope, Effect::WithdrawOrder(order.local))?;
        }
        self.entered.remove(&id);
        self.trials.retain(|(state, _), _| *state != id);
        self.generations.remove(&id);
        world
            .effect(self.scope, Effect::Exit(id))
            .with_context(|| self.context(&state.source))
    }
    pub fn cleanup(&mut self, world: &mut impl World) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "terminal plan pending; retry it before nondeath cleanup"
        );
        ensure!(
            self.admissions.is_empty() && !self.admission_failed,
            "admission publication pending; retry before cleanup"
        );
        self.retired = true;
        while !self.path.is_empty() {
            self.exit(world)?;
        }
        if self.scope.order.is_none() {
            self.tags.clear();
        }
        Ok(())
    }
    pub fn tick(&mut self, mut frame: Frame, world: &mut impl World) -> Result<()> {
        ensure!(
            !self.failed,
            "executor previously failed; refusing effect replay"
        );
        if self.retired {
            return Ok(());
        }
        let result = (|| {
            // Admission is t0 even if the actor starts dormant: lifecycle deadlines still exist.
            if !self.started {
                self.enter(self.root, world)?;
                self.started = true;
                frame.delta_ms = 0;
            }
            self.flush_admissions(world)?;
            if (frame.dormant || frame.stasis) && self.waiting.is_none() {
                return Ok(());
            }
            self.update(frame, world)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result.with_context(|| self.context(&self.definition.source))
    }
    fn update(&mut self, frame: Frame, world: &mut impl World) -> Result<()> {
        self.update_shared(frame, &mut BTreeSet::new(), world)
            .map(|_| ())
    }
    fn update_shared(
        &mut self,
        frame: Frame,
        buckets: &mut BTreeSet<u32>,
        world: &mut impl World,
    ) -> Result<bool> {
        if let Some(local) = self.waiting {
            let definition = Arc::clone(&self.definition);
            let (state, behavior) = self
                .path
                .iter()
                .find_map(|state| {
                    definition.states[*state as usize]
                        .behaviors
                        .iter()
                        .find(|b| b.local == local)
                        .map(|b| (*state, b))
                })
                .context("pending native opportunity left its active state")?;
            return self.run(behavior, state, frame, 0, buckets, world);
        }
        let delta = if self.started { frame.delta_ms } else { 0 };
        self.now = self
            .now
            .checked_add(delta)
            .context("active clock overflow")?;
        if !self.started {
            self.enter(self.root, world)?;
            self.started = true;
        }
        self.flush_admissions(world)?;
        // Publish before listeners update, including ancestor orders that survive child transitions.
        for id in self.path.clone() {
            for order in &self.state(id)?.orders {
                world.effect(
                    self.scope,
                    Effect::PublishOrder {
                        state: order.state,
                        local: order.local,
                        generation: self.generations[&id],
                        targets: order.targets.as_deref(),
                    },
                )?;
            }
        }
        let success = self.behaviors(frame, delta, buckets, world)?;
        if self.retired || self.pending.is_some() || self.waiting.is_some() {
            return Ok(success);
        }
        let definition = Arc::clone(&self.definition);
        for id in self.path.clone() {
            let state = &definition.states[id as usize];
            for (index, transition) in state.transitions.iter().enumerate() {
                if self
                    .transition_matches(id, index, transition, world)
                    .with_context(|| self.context(&transition.source))?
                {
                    let destination = transition
                        .choose_destination(true, &mut self.rng)
                        .context("empty transition")?;
                    let parent = self.state(destination)?.parent;
                    ensure!(
                        parent == state.parent,
                        "transition destination is not a sibling"
                    );
                    while self.path.last().copied() != parent && !self.path.is_empty() {
                        self.exit(world)?;
                    }
                    self.transitioning = true;
                    let entered = self.enter(destination, world);
                    self.transitioning = false;
                    entered?;
                    self.flush_admissions(world)?;
                    return Ok(success);
                }
            }
        }
        Ok(success)
    }
    fn behaviors(
        &mut self,
        frame: Frame,
        delta: u64,
        buckets: &mut BTreeSet<u32>,
        world: &mut impl World,
    ) -> Result<bool> {
        let mut success = false;
        let definition = Arc::clone(&self.definition);
        for id in self.path.clone().into_iter().rev() {
            let state = &definition.states[id as usize];
            for b in &state.behaviors {
                if b.kind != K::FollowOrders
                    && b.bucket.is_some_and(|bucket| buckets.contains(&bucket))
                {
                    continue;
                }
                let result = self
                    .run(b, id, frame, delta, buckets, world)
                    .with_context(|| self.context(&b.source))?;
                success |= result;
                if result && let Some(bucket) = b.bucket {
                    buckets.insert(bucket);
                }
                if self.retired || self.pending.is_some() || self.waiting.is_some() {
                    return Ok(success);
                }
            }
        }
        Ok(success)
    }
    fn run(
        &mut self,
        b: &Behavior,
        state: StateId,
        frame: Frame,
        delta: u64,
        buckets: &mut BTreeSet<u32>,
        world: &mut impl World,
    ) -> Result<bool> {
        if matches!(
            b.kind,
            K::OryxTaunt | K::MetamorphoseAtDeath | K::ReleaseSpawnAtDeath | K::VictoryAtDeath
        ) {
            return Ok(false);
        }
        let age = self.now - self.entered[&state];
        let mut local = self
            .locals
            .remove(&b.local)
            .context("missing active behavior local")?;
        let result = (|| {
            if local.expires.is_some_and(|expiry| self.now >= expiry) && local.condition {
                world.effect(
                    self.scope,
                    Effect::Condition {
                        owner: self.token(b.local),
                        condition: condition(b)?,
                        enabled: false,
                    },
                )?;
                local.condition = false;
                local.expires = None;
            }
            let mut step = Step {
                token: self.token(b.local),
                active_ms: self.now,
                age_ms: age,
                delta_ms: delta,
                emissions: local.emissions,
                text_choice: None,
            };
            if service(b.kind) == Some(Service::Movement)
                && local.emissions > 0
                && world.advance(b, step)?
            {
                return Ok(true);
            }
            if b.kind != K::FollowOrders && self.now < local.next {
                return Ok(false);
            }
            if frame.stunned
                && stun_gated(b)
                && !matches!(b.emission, Some(crate::content::game::Emission::Terminal))
            {
                return Ok(false);
            }
            let success = match b.kind {
                K::SetTag | K::ClearTag => {
                    let symbol = symbol(b)?;
                    if b.kind == K::SetTag {
                        self.tags.insert(symbol);
                    } else {
                        self.tags.remove(&symbol);
                    }
                    if boolean(b, F::SetGlobalTag)?.unwrap_or(false) {
                        world.effect(
                            self.scope,
                            Effect::GlobalTag {
                                symbol,
                                present: true,
                                notify: false,
                            },
                        )?;
                    }
                    true
                }
                K::SetGlobalTag | K::ClearGlobalTag => {
                    world.effect(
                        self.scope,
                        Effect::GlobalTag {
                            symbol: symbol(b)?,
                            present: b.kind == K::SetGlobalTag,
                            notify: boolean(b, F::SendNotification)?.unwrap_or(false),
                        },
                    )?;
                    true
                }
                K::ConditionEffect => {
                    if !local.condition
                        && (local.emissions == 0 || b.fields.contains_key(&F::Cooldown))
                    {
                        enable_condition(b, step.token, self.now, &mut local, world)?;
                        true
                    } else {
                        false
                    }
                }
                K::ChangeSize => {
                    let rate = required(b, F::Rate, number)?;
                    let min = required(b, F::MinSize, number)?;
                    let max = required(b, F::MaxSize, number)?;
                    let size = world.size(self.scope)?;
                    ensure!(size.is_finite(), "nonfinite native size");
                    let changed = size + rate * (delta as f64 / 200.);
                    ensure!(changed.is_finite(), "size change overflow");
                    world.effect(self.scope, Effect::Size(changed.clamp(min, max)))?;
                    true
                }
                K::Decay | K::Metamorphose => false,
                K::FollowOrders => self.follow(b, &mut local, frame, delta, buckets, world)?,
                K::Explode
                    if matches!(b.emission, Some(crate::content::game::Emission::Terminal)) =>
                {
                    if age < time(b, F::InitialDelay)?.unwrap_or(0)
                        || !world.ready(Service::Attack, b, step)?
                    {
                        false
                    } else {
                        self.terminal(
                            TerminalReason::Explode,
                            rewards(b)?,
                            Some(b),
                            Some(TerminalContext::NonPlayer),
                            world,
                        )?;
                        true
                    }
                }
                _ => {
                    let service = service(b.kind)
                        .context("executor control operation reached native routing")?;
                    if b.kind == K::SetAltTexture && local.emissions > 0 {
                        return Ok(false);
                    }
                    if b.kind == K::Explode && age < time(b, F::InitialDelay)?.unwrap_or(0) {
                        return Ok(false);
                    }
                    if b.kind == K::PlaceMap
                        && let Some(Value::Limit(Some(max))) = b.fields.get(&F::MaxPlace)
                        && local.emissions >= u64::from(*max)
                    {
                        return Ok(false);
                    }
                    if b.kind == K::Say
                        && !b.fields.contains_key(&F::Cooldown)
                        && local.opportunities > 0
                    {
                        return Ok(false);
                    }
                    if !world.ready(service, b, step)? {
                        if b.kind == K::Heal {
                            // Release 7.0 behavior.cc:1271-1273 polls again in one second when no target needs healing.
                            local.next = self
                                .now
                                .checked_add(1000)
                                .context("idle Heal deadline overflow")?;
                        }
                        return Ok(false);
                    }
                    let attempt_rng = local.rng.clone();
                    if let Some(prob) = number(b, F::Prob)?
                        && (prob == 0. || (prob < 1. && local.rng.r#gen::<f64>() >= prob))
                    {
                        if b.kind == K::Say {
                            local.opportunities = local
                                .opportunities
                                .checked_add(1)
                                .context("opportunity count overflow")?;
                            local.next = self
                                .now
                                .checked_add(cooldown(b, &mut local.rng)?)
                                .context("opportunity deadline overflow")?;
                        }
                        return Ok(false);
                    }
                    if b.kind == K::Say {
                        let Some(Value::TextChoices(choices)) = b.fields.get(&F::Text) else {
                            bail!("missing linked Say text choices")
                        };
                        ensure!(!choices.is_empty(), "empty Say alternatives");
                        step.text_choice = Some(if choices.len() == 1 {
                            0
                        } else {
                            local.rng.gen_range(0..choices.len())
                        });
                    }
                    match world.apply(service, b, step, local.rng.get())? {
                        ActionOutcome::Blocked => {
                            // The snapshot precedes successful chance/text/native draws, not Say misses.
                            local.rng = attempt_rng;
                            if b.kind == K::Taunt {
                                // Release 7.0 behavior.cc:775-780 resets the timer before DoTaunt.
                                local.next = self
                                    .now
                                    .checked_add(taunt_interval())
                                    .context("Taunt deadline overflow")?;
                            }
                            return Ok(false);
                        }
                        ActionOutcome::Pending => {
                            self.waiting = Some(b.local);
                            local.rng = attempt_rng;
                            return Ok(false);
                        }
                        ActionOutcome::Removed => self.cleanup(world)?,
                        ActionOutcome::Applied => {
                            self.waiting = None;
                        }
                    }
                    if b.kind == K::Explode && !self.retired {
                        self.terminal(
                            TerminalReason::Explode,
                            rewards(b)?,
                            Some(b),
                            Some(TerminalContext::NonPlayer),
                            world,
                        )?;
                    }
                    true
                }
            };
            if success {
                if b.kind == K::Say {
                    local.opportunities = local
                        .opportunities
                        .checked_add(1)
                        .context("opportunity count overflow")?;
                }
                local.emissions = local
                    .emissions
                    .checked_add(1)
                    .context("emission count overflow")?;
                if b.kind != K::FollowOrders {
                    let cooldown = cooldown(b, &mut local.rng)?;
                    local.next = self
                        .now
                        .checked_add(cooldown)
                        .context("cooldown deadline overflow")?;
                }
            }
            Ok(success)
        })();
        // This invocation owns its removed local until terminal/nondeath cleanup has completed.
        if self.retired {
            self.release(b, local, world)?;
        } else {
            self.locals.insert(b.local, local);
        }
        result
    }
    fn follow(
        &mut self,
        b: &Behavior,
        local: &mut Local,
        frame: Frame,
        delta: u64,
        buckets: &mut BTreeSet<u32>,
        world: &mut impl World,
    ) -> Result<bool> {
        let mut offers = world.orders(self.scope)?;
        for offer in &offers {
            ensure!(
                offer.distance.is_finite() && offer.distance >= 0.,
                "invalid order distance"
            );
        }
        let range = required(b, F::AcquireRange, number)?;
        let listens: Vec<u16> = [F::ListenIds, F::GroupIds]
            .into_iter()
            .filter_map(|f| b.fields.get(&f))
            .map(|v| match v {
                Value::Objects(ids) => Ok(ids.as_slice()),
                _ => bail!("invalid linked order selector"),
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .copied()
            .collect();
        offers.retain(|o| {
            o.issuer.world == self.scope.identity.world
                && o.issuer.actor != self.scope.identity.actor
                && o.targets
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&self.kind))
                && (listens.is_empty() || listens.contains(&o.definition.kind))
                && o.distance <= range
        });
        offers.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.issuer.actor.cmp(&b.issuer.actor))
                .then(a.local.cmp(&b.local))
        });
        let offer = offers.first();
        let same = match (local.listener.as_ref(), offer) {
            (Some(listener), Some(offer)) => {
                listener.scope.issuer == offer.issuer.actor
                    && listener.scope.order == Some(offer.local)
                    && listener.scope.generation == offer.generation
                    && Arc::ptr_eq(&listener.definition, &offer.definition)
                    && listener.root == offer.state
            }
            (None, None) => true,
            _ => false,
        };
        if !same {
            if let Some(mut listener) = local.listener.take() {
                listener.cleanup(world)?;
            }
            if let Some(offer) = offer.filter(|_| self.now >= local.next) {
                ensure!(
                    self.scope.order.is_none(),
                    "recursive FollowOrders is not a bounded order graph"
                );
                local.next = self
                    .now
                    .checked_add(required(b, F::Cooldown, time)?)
                    .context("order acquisition deadline overflow")?;
                let scope = Scope {
                    identity: self.scope.identity,
                    issuer: offer.issuer.actor,
                    order: Some(offer.local),
                    listener: Some(b.local),
                    generation: offer.generation,
                };
                local.listener = Some(Box::new(Self::scoped(
                    offer.definition.clone(),
                    offer.state,
                    self.kind,
                    scope,
                    local_seed(
                        local_seed(self.seed ^ offer.issuer.actor as u64, offer.local),
                        b.local,
                    ),
                )));
            }
        }
        if b.bucket.is_some_and(|bucket| buckets.contains(&bucket)) {
            return Ok(false);
        }
        let Some(listener) = local.listener.as_mut() else {
            return Ok(false);
        };
        // A shared bucket set gives ordered rules priority over the listener's fallback movement.
        listener.tags = std::mem::take(&mut self.tags);
        let result = listener.update_shared(
            Frame {
                delta_ms: delta,
                ..frame
            },
            buckets,
            world,
        );
        self.tags = std::mem::take(&mut listener.tags);
        if listener.admission_failed {
            self.admission_failed = true;
        }
        if let Some(plan) = listener.pending.take() {
            self.merge_terminal(plan, world)?;
        }
        result
    }
    fn transition_matches(
        &mut self,
        state: StateId,
        index: usize,
        transition: &crate::content::game::Transition,
        world: &impl World,
    ) -> Result<bool> {
        let age = self.now - self.entered[&state];
        for predicate in transition
            .predicates
            .iter()
            .filter(|p| !matches!(p, Predicate::Random(_)))
        {
            if !self.matches(predicate, age, world)? {
                return Ok(false);
            }
        }
        if let Some(random) = transition
            .predicates
            .iter()
            .find(|p| matches!(p, Predicate::Random(_)))
        {
            let next = self.trials.entry((state, index)).or_insert(self.now);
            if self.now < *next {
                return Ok(false);
            }
            // DOCX271 gives random=p per .2s. Immediate-first/eligibility/reset rules are authored,
            // but a 50ms native tick must not multiply recovered probability opportunities by four.
            *next = self
                .now
                .checked_add(200)
                .context("random trial deadline overflow")?;
            return self.matches(random, age, world);
        }
        Ok(true)
    }
    fn matches(&mut self, p: &Predicate, age: u64, world: &impl World) -> Result<bool> {
        Ok(match p {
            Predicate::After(ms) => age >= *ms,
            Predicate::PlayerWithin(proximity) => {
                proximity.matches_eligible_distances(world.players(self.scope)?)?
            }
            Predicate::NoPlayerWithin(radius) => {
                let distances = world.players(self.scope)?;
                ensure!(
                    distances.iter().all(|d| d.is_finite() && *d >= 0.),
                    "invalid player distance"
                );
                distances.into_iter().all(|d| d > *radius)
            }
            Predicate::HpBelow(fraction) => {
                let hp = world.hp_fraction(self.scope)?;
                ensure!(
                    hp.is_finite() && (0.0..=1.).contains(&hp),
                    "invalid native HP fraction"
                );
                hp < *fraction
            }
            Predicate::Random(prob) => {
                *prob >= 1. || (*prob > 0. && self.rng.r#gen::<f64>() < *prob)
            }
            Predicate::Exists(kinds) => world.exists(self.scope, kinds)?,
            Predicate::NoneExist(kinds) => !world.exists(self.scope, kinds)?,
            Predicate::OnGround(grounds) => grounds.contains(&world.ground(self.scope)?),
            Predicate::Tag {
                symbol,
                global,
                present,
            } => {
                (if *global {
                    world.global_tag(self.scope, *symbol)?
                } else {
                    self.tags.contains(symbol)
                }) == *present
            }
            Predicate::PlayerSays(text) => world.player_says(self.scope, text)?,
        })
    }
    fn order_live(&self, world: &impl World) -> Result<bool> {
        let Some(order) = self.scope.order else {
            return Ok(true);
        };
        Ok(world.orders(self.scope)?.into_iter().any(|offer| {
            offer.issuer.world == self.scope.identity.world
                && offer.issuer.actor == self.scope.issuer
                && offer.local == order
                && offer.generation == self.scope.generation
                && offer.state == self.root
                && Arc::ptr_eq(&offer.definition, &self.definition)
                && offer
                    .targets
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&self.kind))
        }))
    }
    fn merge_terminal(&mut self, plan: Box<TerminalPlan>, world: &mut impl World) -> Result<()> {
        let mut combined = self.plan(
            plan.reason,
            plan.rewards,
            plan.behavior.as_ref(),
            plan.context.as_ref(),
            world,
            Some(plan.origin),
        )?;
        combined.hooks.extend(plan.hooks);
        combined.announcements.extend(plan.announcements);
        combined.origin = plan.origin;
        self.pending = Some(Box::new(combined));
        self.retry_terminal(world)
    }
    /// Called by the world's due-deadline queue even while AI is dormant/stasised. Stale canceled
    /// generations return false; an expiry for another world/actor is an error, never a substitution.
    pub fn expire_lifecycle(&mut self, deadline: Deadline, world: &mut impl World) -> Result<bool> {
        ensure!(
            deadline.owner.scope.identity == self.scope.identity,
            "lifecycle expiry belongs to another actor/world"
        );
        ensure!(
            !self.failed,
            "executor previously failed; retry the pending terminal plan, not expiry"
        );
        if self.retired {
            return Ok(false);
        }
        let result = (|| {
            if !self.order_live(world)? {
                self.cleanup(world)?;
                return Ok(false);
            }
            if deadline.owner.scope != self.scope {
                for id in self.locals.keys().copied().collect::<Vec<_>>() {
                    let Some(listener) = self.locals.get_mut(&id).and_then(|l| l.listener.as_mut())
                    else {
                        continue;
                    };
                    if listener.scope == deadline.owner.scope {
                        let applied = listener.expire_lifecycle(deadline, world)?;
                        if let Some(plan) = listener.pending.take() {
                            self.merge_terminal(plan, world)?;
                        }
                        return Ok(applied);
                    }
                }
                return Ok(false);
            }
            for state in self.path.clone() {
                if self.generations[&state] != deadline.generation {
                    continue;
                }
                if let Some(b) = self
                    .state(state)?
                    .behaviors
                    .iter()
                    .find(|b| b.local == deadline.owner.local)
                    .cloned()
                {
                    ensure!(
                        matches!(b.kind, K::Decay | K::Metamorphose),
                        "world deadline targets a non-lifecycle behavior"
                    );
                    let reason = if b.kind == K::Decay {
                        TerminalReason::Decay
                    } else {
                        TerminalReason::Metamorphose
                    };
                    self.terminal(
                        reason,
                        rewards(&b)?,
                        Some(&b),
                        Some(TerminalContext::NonPlayer),
                        world,
                    )?;
                    return Ok(true);
                }
            }
            Ok(false)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result.with_context(|| self.context(&self.definition.source))
    }
    fn plan(
        &mut self,
        reason: TerminalReason,
        rewards: Rewards,
        behavior: Option<&Behavior>,
        context: Option<&TerminalContext>,
        world: &impl World,
        exclude: Option<Scope>,
    ) -> Result<TerminalPlan> {
        let mut hooks = Vec::new();
        let mut announcements = Vec::new();
        if reason != TerminalReason::Metamorphose {
            for id in self.path.clone().into_iter().rev() {
                let definition = Arc::clone(&self.definition);
                for b in &linked_state(&definition, id)?.behaviors {
                    if let Some(rules) = &b.announcement
                        && let TerminalAnnouncements::Death { killed, death } = &rules.terminal
                    {
                        let (category, choices) = match context {
                            Some(TerminalContext::PlayerKill { .. }) => {
                                (AnnouncementCategory::Killed, killed)
                            }
                            Some(TerminalContext::NonPlayer) => {
                                (AnnouncementCategory::Death, death)
                            }
                            None => bail!(
                                "{}: terminal announcement needs explicit player-kill or nonplayer context",
                                self.context(&b.source)
                            ),
                        };
                        announcements.push(self.sample_announcement(
                            b,
                            self.generations[&id],
                            rules,
                            category,
                            choices,
                            context,
                        )?);
                    }
                    if matches!(
                        b.kind,
                        K::MetamorphoseAtDeath | K::ReleaseSpawnAtDeath | K::VictoryAtDeath
                    ) {
                        let probability = number(b, F::Prob)?.unwrap_or(1.);
                        let local = self
                            .locals
                            .get_mut(&b.local)
                            .context("missing death local")?;
                        if probability >= 1.
                            || (probability > 0. && local.rng.r#gen::<f64>() < probability)
                        {
                            hooks.push(DeathHook {
                                owner: self.token(b.local),
                                behavior: b.clone(),
                                seed: local_seed(self.seed ^ self.scope.generation, b.local),
                            });
                        }
                    }
                }
            }
        }
        if reason != TerminalReason::Metamorphose {
            for local in self.locals.values_mut() {
                if let Some(listener) = local
                    .listener
                    .as_mut()
                    .filter(|l| !l.retired && Some(l.scope) != exclude)
                    && listener.order_live(world)?
                {
                    let plan = listener.plan(reason, rewards, behavior, context, world, None)?;
                    hooks.extend(plan.hooks);
                    announcements.extend(plan.announcements);
                }
            }
        }
        Ok(TerminalPlan {
            origin: self.scope,
            context: context.cloned(),
            announcements,
            reason,
            rewards,
            behavior: behavior.cloned(),
            hooks,
        })
    }
    fn terminal(
        &mut self,
        reason: TerminalReason,
        rewards: Rewards,
        b: Option<&Behavior>,
        context: Option<TerminalContext>,
        world: &mut impl World,
    ) -> Result<()> {
        if self.retired {
            return Ok(());
        }
        if self.pending.is_none() {
            let context = if reason == TerminalReason::Defeated {
                context
            } else {
                Some(TerminalContext::NonPlayer)
            };
            self.pending = Some(Box::new(self.plan(
                reason,
                rewards,
                b,
                context.as_ref(),
                world,
                None,
            )?));
        }
        // Ordered rules request retirement of the listener, whose own active death hooks must join
        // the same world transaction. The outer actor merges the request before any native commit.
        if self.scope.order.is_some() {
            return Ok(());
        }
        self.retry_terminal(world)
    }
    /// A rejected atomic world claim retries these exact sampled outcomes, never tick/actions/RNG.
    /// World must use actor lifetime identity as its once-only claim, independent of terminal reason.
    pub fn retry_terminal(&mut self, world: &mut impl World) -> Result<()> {
        if self.retired {
            ensure!(
                !self.failed,
                "terminal committed but executor cleanup failed"
            );
            return Ok(());
        }
        let plan = self.pending.as_ref().context("no terminal plan to retry")?;
        world.terminal(self.scope, plan)?;
        self.retired = true;
        self.pending = None;
        let result = self.finish(world);
        self.failed = result.is_err();
        result
    }
    fn release(&mut self, b: &Behavior, mut local: Local, world: &mut impl World) -> Result<()> {
        if let Some(listener) = &mut local.listener {
            listener.cleanup(world)?;
        }
        if local.condition {
            world.effect(
                self.scope,
                Effect::Condition {
                    owner: self.token(b.local),
                    condition: condition(b)?,
                    enabled: false,
                },
            )?;
        }
        if b.kind == K::SetAltTexture {
            // Release 7.0 behavior.cc:2589-2591 clears the host texture even if Update never ran.
            world.effect(self.scope, Effect::ClearAltTexture)?;
        }
        world.effect(self.scope, Effect::ForgetLocal(self.token(b.local)))
    }
    fn finish(&mut self, world: &mut impl World) -> Result<()> {
        while !self.path.is_empty() {
            self.exit(world)?;
        }
        self.tags.clear();
        Ok(())
    }
    #[cfg(test)]
    fn needs_terminal_context(&self, world: &impl World) -> Result<Option<Source>> {
        let path = if self.started {
            self.path.clone()
        } else {
            let mut path = vec![self.root];
            while let Some(child) = self.state(*path.last().unwrap())?.children.first() {
                path.push(*child);
            }
            path
        };
        for state in path {
            for b in &self.state(state)?.behaviors {
                if b.announcement.as_ref().is_some_and(|rules| {
                    matches!(rules.terminal, TerminalAnnouncements::Death { .. })
                }) {
                    return Ok(Some(b.source.clone()));
                }
            }
        }
        for local in self.locals.values() {
            if let Some(listener) = local.listener.as_ref().filter(|l| !l.retired)
                && listener.order_live(world)?
                && let Some(source) = listener.needs_terminal_context(world)?
            {
                return Ok(Some(source));
            }
        }
        Ok(None)
    }
    /// The legacy caller signature is safe only when active rules require no death attribution.
    #[cfg(test)]
    pub fn defeated(&mut self, world: &mut impl World) -> Result<()> {
        if !self.retired
            && !self.failed
            && let Some(source) = self.needs_terminal_context(world)?
        {
            bail!(
                "{}: use defeated_with_context with actual player-kill or explicit NonPlayer attribution",
                self.context(&source)
            );
        }
        self.defeat(None, world)
    }
    pub fn defeated_with_context(
        &mut self,
        context: TerminalContext,
        world: &mut impl World,
    ) -> Result<()> {
        validate_terminal_context(self.scope, &context)?;
        self.defeat(Some(context), world)
    }
    fn defeat(&mut self, context: Option<TerminalContext>, world: &mut impl World) -> Result<()> {
        ensure!(!self.failed, "executor previously failed");
        if self.retired {
            return Ok(());
        }
        let result = (|| {
            if !self.started {
                self.enter(self.root, world)?;
                self.started = true;
            }
            self.flush_admissions(world)?;
            self.terminal(
                TerminalReason::Defeated,
                Rewards {
                    loot: true,
                    xp: true,
                },
                None,
                context,
                world,
            )
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
fn validate_terminal_context(scope: Scope, context: &TerminalContext) -> Result<()> {
    if let TerminalContext::PlayerKill {
        world,
        player,
        name,
    } = context
    {
        ensure!(
            *world == scope.identity.world,
            "player-kill context belongs to another world"
        );
        ensure!(
            *player > 0 && !name.trim().is_empty(),
            "player-kill context needs actual player identity and nonempty name"
        );
    }
    Ok(())
}
fn linked_state(definition: &Definition, id: StateId) -> Result<&crate::content::game::State> {
    definition
        .states
        .get(id as usize)
        .filter(|s| s.id == id)
        .with_context(|| format!("invalid linked state {id}"))
}
fn local_seed(seed: u64, local: u32) -> u64 {
    seed.wrapping_add(u64::from(local).wrapping_mul(0x9e3779b97f4a7c15))
}
/// A declared executor read; GameData load already refused any behavior that lacks it.
#[track_caller]
fn required<T>(b: &Behavior, f: F, read: fn(&Behavior, F) -> Result<Option<T>>) -> Result<T> {
    debug_assert!(
        crate::content::game::executor_reads(b.kind).contains(&f),
        "undeclared executor read {:?} {f:?} at {}",
        b.kind,
        std::panic::Location::caller()
    );
    read(b, f)?.with_context(|| format!("{:?} unmaterialized {f:?}", b.kind))
}
fn time(b: &Behavior, f: F) -> Result<Option<u64>> {
    match b.fields.get(&f) {
        None => Ok(None),
        Some(Value::Millis(n)) => Ok(Some(*n)),
        _ => bail!("invalid linked milliseconds {f:?}"),
    }
}
fn number(b: &Behavior, f: F) -> Result<Option<f64>> {
    match b.fields.get(&f) {
        None => Ok(None),
        Some(Value::Number(n)) => Ok(Some(*n)),
        _ => bail!("invalid linked number {f:?}"),
    }
}
fn boolean(b: &Behavior, f: F) -> Result<Option<bool>> {
    match b.fields.get(&f) {
        None => Ok(None),
        Some(Value::Bool(n)) => Ok(Some(*n)),
        _ => bail!("invalid linked boolean {f:?}"),
    }
}
fn enable_condition(
    b: &Behavior,
    owner: Token,
    now: u64,
    local: &mut Local,
    world: &mut impl World,
) -> Result<()> {
    let duration = required(b, F::Duration, time)?;
    world.effect(
        owner.scope,
        Effect::Condition {
            owner,
            condition: condition(b)?,
            enabled: true,
        },
    )?;
    local.condition = true;
    local.expires = if duration == 0 {
        None
    } else {
        Some(
            now.checked_add(duration)
                .context("condition expiry overflow")?,
        )
    };
    Ok(())
}
fn condition(b: &Behavior) -> Result<&Condition> {
    match b.fields.get(&F::Effect) {
        Some(Value::Condition(c)) => Ok(c),
        _ => bail!("missing linked condition"),
    }
}
fn symbol(b: &Behavior) -> Result<u32> {
    match b.fields.get(&F::TagName) {
        Some(Value::Symbol(s)) => Ok(*s),
        _ => bail!("missing linked tag"),
    }
}
fn rewards(b: &Behavior) -> Result<Rewards> {
    Ok(Rewards {
        loot: required(b, F::DropLoot, boolean)?,
        xp: required(b, F::GiveXP, boolean)?,
    })
}
/// Recovered behavior.cc checks STUNNED in every Shoot but only in targeted GrenadeToss/ObjectToss (behavior.cc:445,2265,2377).
fn stun_gated(b: &Behavior) -> bool {
    b.kind == K::Shoot
        || matches!(b.kind, K::GrenadeToss | K::ObjectToss)
            && !matches!(
                b.fields.get(&F::Type),
                Some(Value::Mode(crate::content::game::Mode::Auto))
            )
}
/// Exhaustive vocabulary partition: adding a compiler opcode forces an explicit executor decision.
pub fn service(kind: K) -> Option<Service> {
    Some(match kind {
        K::BackAndForth
        | K::Buzz
        | K::Charge
        | K::Circle
        | K::Follow
        | K::KeepDistance
        | K::Move
        | K::Orbit
        | K::Protect
        | K::Shadow
        | K::StayAbove
        | K::StayBack
        | K::StayCloseToSpawn
        | K::StayInTerrain
        | K::Swirl
        | K::Walk
        | K::Wander => Service::Movement,
        K::Shoot | K::Explode | K::GrenadeToss | K::Zap | K::TrapTrigger => Service::Attack,
        K::MakeMinions | K::MaintainProtectors | K::ObjectToss => Service::Spawn,
        K::ConditionEffectAura | K::ClearConditionEffectAura => Service::Aura,
        K::Heal | K::HealZap => Service::Heal,
        K::PlaceMap => Service::Map,
        K::ChangeGround => Service::Ground,
        K::DeleteObject => Service::Delete,
        K::CopyDamageRecord => Service::DamageRecord,
        K::ShowEffect | K::SetAltTexture => Service::Visual,
        K::Say | K::Taunt => Service::Speech,
        K::PlaySound => Service::Sound,
        K::RemainVisible => Service::Visibility,
        K::RestoresBreath => Service::Breath,
        K::RemoveOnEmpty => Service::RemoveOnEmpty,
        K::ChangeSize
        | K::ClearGlobalTag
        | K::ClearTag
        | K::ConditionEffect
        | K::Decay
        | K::FollowOrders
        | K::OryxTaunt
        | K::Metamorphose
        | K::MetamorphoseAtDeath
        | K::ReleaseSpawnAtDeath
        | K::SetGlobalTag
        | K::SetTag
        | K::VictoryAtDeath => return None,
    })
}
/// Integration must run this over all admitted definitions before opening the world/store.
/// This supplements strict XML compilation with executor scheduling constraints.
pub fn validate(definition: &Definition) -> Result<()> {
    for b in definition.states.iter().flat_map(|s| &s.behaviors) {
        validate_schedule(b).with_context(|| format!("{}", b.source))?;
    }
    Ok(())
}
fn validate_schedule(b: &Behavior) -> Result<()> {
    ensure!(
        b.announcement.is_some() == (b.kind == K::OryxTaunt),
        "OryxTaunt requires its exact linked announcement rules; other behaviors must not carry them"
    );
    ensure!(
        !b.fields.contains_key(&F::TilDeath),
        "Decay tilDeath semantics unresolved: recovered numeric values are not a boolean no-decay flag"
    );
    if b.kind == K::Shoot {
        ensure!(
            !(boolean(b, F::CooldownJitter)?.unwrap_or(false)
                && boolean(b, F::NoCooldownJitter)?.unwrap_or(false)),
            "contradictory cooldownJitter=true and noCooldownJitter=true"
        );
    }
    Ok(())
}
const fn taunt_interval() -> u64 {
    20_000
}
fn cooldown(b: &Behavior, rng: &mut impl Rng) -> Result<u64> {
    validate_schedule(b)?;
    let base = if b.kind == K::Taunt {
        taunt_interval()
    } else {
        time(b, F::Cooldown)?.unwrap_or(0)
    };
    let jitter =
        if base > 0 && b.kind == K::Shoot && boolean(b, F::CooldownJitter)?.unwrap_or(false) {
            Some(base as f64 / 2.)
        } else if b.kind == K::TrapTrigger {
            time(b, F::CooldownJitter)?
                .filter(|j| *j > 0)
                .map(|j| j as f64)
        } else {
            None
        };
    match jitter {
        None => Ok(base),
        Some(jitter) => jitter_delay(base, jitter, rng.r#gen::<f64>()),
    }
}
// Later DECA documents amplitudes (Shoot 50..150%, trap C±J), not the old engine's distribution.
// Uniform draws and floor-to-ms quantization are explicit authored migration choices.
fn jitter_delay(base: u64, jitter: f64, u: f64) -> Result<u64> {
    ensure!(
        u.is_finite() && (0.0..1.).contains(&u) && jitter.is_finite() && jitter >= 0.,
        "invalid jitter draw/range"
    );
    let delay = (base as f64 - jitter + 2. * jitter * u).max(0.).floor();
    ensure!(
        delay.is_finite() && delay < u64::MAX as f64,
        "jitter deadline overflow"
    );
    Ok(delay as u64)
}
