//! One current errand per bot, the fixed events it emits and the CCTV flags derived from them.
use super::*;
use serde::Serialize;
use serde_json::{Value, json};

const EVENTS: usize = 32;
const NO_PROGRESS_MS: u64 = 10 * 60_000;
const LOOP_WINDOW_MS: u64 = 10 * 60_000;
const FLEE_WINDOW_MS: u64 = 30 * 60_000;

pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("plan wall clock predates epoch")
        .as_millis() as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Owner {
    Trade,
    Bank,
    Event,
    Call,
    Gift,
    Goal,
}
impl Owner {
    pub(super) const ERRANDS: [Self; 5] =
        [Self::Trade, Self::Event, Self::Bank, Self::Call, Self::Gift];
    fn budget_ms(self) -> u64 {
        match self {
            Self::Trade => 10 * 60_000,
            Self::Bank => 3 * 60_000,
            Self::Event => 15 * 60_000,
            Self::Call => 30_000,
            Self::Gift => 2 * 60_000,
            Self::Goal => u64::MAX,
        }
    }
    // A finished market visit must not restart on the next Nexus arrival.
    fn rest_ms(self) -> u64 {
        match self {
            Self::Trade => 10 * 60_000,
            Self::Bank => 5 * 60_000,
            Self::Gift => 60_000,
            Self::Event | Self::Call | Self::Goal => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Step {
    pub owner: Owner,
    pub target: String,
    pub started: u64,
    pub deadline: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Started,
    Completed,
    Aborted,
    Progress,
    Trade,
    Death,
    Escape,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Event {
    pub seq: u64,
    pub at: u64,
    pub kind: Kind,
    pub owner: Option<Owner>,
    pub reason: String,
}
impl Event {
    pub(super) fn text(&self) -> String {
        let owner = self
            .owner
            .map_or(String::new(), |o| format!(" {o:?}").to_lowercase());
        format!("{:?}{owner}: {}", self.kind, self.reason)
    }
}

/// Outcome of asking an errand's policy for this decision.
pub(super) enum Errand {
    Act(Activity, Decision),
    Yield,
    Done(&'static str),
    Abort(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Checkpoint {
    world: i32,
    experience: i32,
    stats: i32,
    fame: i32,
    items: Vec<i32>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Plan {
    pub step: Option<Step>,
    pub events: VecDeque<Event>,
    pub last_progress: u64,
    pub engaged: bool,
    rest: BTreeMap<Owner, u64>,
    checkpoint: Option<Checkpoint>,
    settled_trade: Option<u64>,
    goal: Option<(u64, i64, u64)>,
    seq: u64,
}
impl Plan {
    pub(super) fn owner(&self) -> Option<Owner> {
        self.step.as_ref().map(|s| s.owner)
    }
    pub(super) fn idle(&self) -> bool {
        self.step.is_none()
    }
    pub(super) fn resting(&self, owner: Owner, now: u64) -> bool {
        self.rest.get(&owner).is_some_and(|until| now < *until)
    }
    pub(super) fn record(&mut self, now: u64, kind: Kind, owner: Option<Owner>, reason: String) {
        self.seq += 1;
        let event = Event {
            seq: self.seq,
            at: now,
            kind,
            owner,
            reason,
        };
        tracing::info!(event=%serde_json::to_string(&event).expect("plan event serialization"), "bot plan event");
        if matches!(kind, Kind::Completed | Kind::Progress | Kind::Trade) {
            self.last_progress = now;
        }
        self.events.push_back(event);
        if self.events.len() > EVENTS {
            self.events.pop_front();
        }
    }
    pub(super) fn start(&mut self, now: u64, owner: Owner, target: String) {
        let deadline = now.saturating_add(owner.budget_ms());
        self.record(now, Kind::Started, Some(owner), target.clone());
        self.step = Some(Step {
            owner,
            target,
            started: now,
            deadline,
        });
    }
    fn finish(&mut self, now: u64, kind: Kind, reason: &str) {
        if let Some(step) = self.step.take() {
            self.rest
                .insert(step.owner, now.saturating_add(step.owner.rest_ms()));
            self.record(now, kind, Some(step.owner), reason.into());
        }
    }
    pub(super) fn complete(&mut self, now: u64, reason: &str) {
        self.finish(now, Kind::Completed, reason);
    }
    pub(super) fn abort(&mut self, now: u64, reason: &str) {
        self.finish(now, Kind::Aborted, reason);
    }
    /// Whether the main goal's metric failed to improve within its patience.
    pub(super) fn stalled(&mut self, progress: Option<(u64, i64, u64)>, now: u64) -> bool {
        match (progress, self.goal) {
            (Some((id, metric, patience)), Some((goal, best, since))) if id == goal => {
                if metric > best {
                    self.goal = Some((id, metric, now));
                }
                metric <= best && now.saturating_sub(since) >= patience
            }
            (Some((id, metric, _)), _) => {
                self.goal = Some((id, metric, now));
                false
            }
            (None, _) => {
                self.goal = None;
                false
            }
        }
    }
    pub(super) fn overdue(&self, now: u64) -> bool {
        self.step.as_ref().is_some_and(|s| now >= s.deadline)
    }
    /// Progress hooks: XP, permanent stats, fame, a new item kind, a new area or a settled trade.
    pub(super) fn observe(&mut self, v: &PlayerView, market: &pursuits::market::Market, now: u64) {
        if self.last_progress == 0 {
            self.last_progress = now;
        }
        let mut items: Vec<_> = v
            .character
            .items
            .iter()
            .copied()
            .filter(|k| *k >= 0)
            .collect();
        items.sort_unstable();
        let current = Checkpoint {
            world: v.session.world,
            experience: v.character.experience,
            stats: v.character.base_stats.iter().sum(),
            fame: v.character.fame,
            items,
        };
        if let Some(old) = self.checkpoint.replace(current.clone()) {
            let reason = if current.world != old.world {
                Some(format!("entered {}", v.map.name))
            } else if current.stats > old.stats {
                Some("permanent stat increased".into())
            } else if current.items.iter().any(|k| !old.items.contains(k)) {
                Some("new item held".into())
            } else if current.experience > old.experience || current.fame > old.fame {
                // Combat XP arrives every few seconds; one event per minute keeps the log readable.
                (now.saturating_sub(self.last_progress) >= 60_000)
                    .then(|| "gained experience".into())
            } else {
                None
            };
            if current.experience > old.experience || current.fame > old.fame {
                self.last_progress = now;
            }
            if let Some(reason) = reason {
                self.record(now, Kind::Progress, None, reason);
            }
        }
        if let Some(b) = &market.bargain
            && b.resolution.as_deref() == Some("settled_all_installments_transaction_identified")
            && self.settled_trade != Some(b.terms.version)
        {
            self.settled_trade = Some(b.terms.version);
            self.record(now, Kind::Trade, None, "exchange settled".into());
        }
    }
    pub(super) fn public(&self, now: u64) -> Value {
        json!({"flag":self.flag(now),"last_progress_at":self.last_progress,"step":self.step,
            "events":self.events.iter().rev().take(10).collect::<Vec<_>>()})
    }
    pub(super) fn flag(&self, now: u64) -> Option<Value> {
        let minutes = |since: u64| now.saturating_sub(since) / 60_000;
        let escapes: Vec<_> = self
            .events
            .iter()
            .filter(|e| e.kind == Kind::Escape && now.saturating_sub(e.at) < FLEE_WINDOW_MS)
            .collect();
        if escapes.len() > 3 {
            let last = escapes.last().expect("counted escapes");
            return Some(json!({"kind":"fleeing","since_ms":escapes[0].at,
                "text":format!("Keeps fleeing: {} Nexus escapes in 30m, last for {}.",escapes.len(),last.reason)}));
        }
        let mut aborts: BTreeMap<(Option<Owner>, &str), Vec<u64>> = BTreeMap::new();
        for e in self
            .events
            .iter()
            .filter(|e| e.kind == Kind::Aborted && now.saturating_sub(e.at) < LOOP_WINDOW_MS)
        {
            aborts.entry((e.owner, &e.reason)).or_default().push(e.at);
        }
        if let Some(((owner, reason), times)) = aborts.iter().find(|(_, t)| t.len() >= 3) {
            let owner = owner.map_or("step".into(), |o| format!("{o:?}").to_lowercase());
            return Some(json!({"kind":"looping","since_ms":times[0],
                "text":format!("Looping for {}m: {owner} aborted {} times ({reason}).",minutes(times[0]),times.len())}));
        }
        if !self.engaged && now.saturating_sub(self.last_progress) >= NO_PROGRESS_MS {
            let doing = self
                .step
                .as_ref()
                .map_or("its goal".into(), |s| s.target.clone());
            return Some(json!({"kind":"no_progress","since_ms":self.last_progress,
                "text":format!("No progress for {}m while pursuing {doing}.",minutes(self.last_progress))}));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_derive_from_progress_aborts_and_escapes() {
        let mut plan = Plan {
            last_progress: 1,
            ..Plan::default()
        };
        assert_eq!(plan.flag(NO_PROGRESS_MS), None);
        assert_eq!(
            plan.flag(NO_PROGRESS_MS + 1).unwrap()["kind"],
            "no_progress"
        );
        plan.engaged = true;
        assert_eq!(
            plan.flag(NO_PROGRESS_MS + 1),
            None,
            "fighting, trading or idling is not a stall"
        );
        for at in [10, 20, 30] {
            plan.start(at, Owner::Bank, "Vault".into());
            plan.abort(at, "left the Vault before banking");
        }
        plan.complete(40, "unrelated");
        let looping = plan.flag(100).unwrap();
        assert_eq!(looping["kind"], "looping");
        assert_eq!(looping["since_ms"], 10);
        assert_eq!(plan.flag(LOOP_WINDOW_MS + 20), None, "aborts age out");
        for at in 0..4 {
            plan.record(at, Kind::Escape, None, "critical_health".into());
        }
        assert_eq!(plan.flag(100).unwrap()["kind"], "fleeing");
        plan.events.retain(|e| e.kind != Kind::Escape || e.at > 0);
        assert_eq!(
            plan.flag(100).unwrap()["kind"],
            "looping",
            "three escapes are not fleeing"
        );
    }
}
