use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    Help,
    Gods,
    Demon,
    Sphinx,
    Snake,
}
impl Activity {
    fn parse(text: &str) -> Option<Self> {
        let text = text.to_ascii_lowercase();
        let words = text.split_whitespace().collect::<Vec<_>>();
        match words.as_slice() {
            ["snake"] | ["snake", "pit"] | ["snake", "here"] | ["tp", "snake"] => Some(Self::Snake),
            ["tp"] => Some(Self::Help),
            ["tp", "gods"] => Some(Self::Gods),
            ["tp", "demon"] => Some(Self::Demon),
            ["tp", "sphynix" | "sphinx"] => Some(Self::Sphinx),
            _ => None,
        }
    }
}
struct Seen {
    world: i32,
    account: i32,
    activity: Activity,
    until: u64,
}
struct Opportunity {
    world: i32,
    actor: i32,
    account: i32,
    name: String,
    activity: Activity,
    until: u64,
}
#[derive(Default)]
pub(super) struct Calls {
    seen: VecDeque<Seen>,
    opportunity: Option<Opportunity>,
    pub(super) last: Option<serde_json::Value>,
}
impl Calls {
    fn record(
        &mut self,
        v: &PlayerView,
        actor: i32,
        activity: Activity,
        status: &str,
        reason: &str,
        motive: Option<&str>,
    ) {
        self.last = Some(serde_json::json!({"actor":actor,"world":v.session.world,
            "observed_ms":v.elapsed_ms,"activity":format!("{activity:?}"),
            "status":status,"reason":reason,"motive":motive}));
        tracing::info!(
            account = v.session.account,
            actor,
            world = v.session.world,
            ?activity,
            status,
            reason,
            motive,
            "public activity call decision"
        );
    }
    pub(super) fn observe(&mut self, v: &PlayerView) {
        self.seen
            .retain(|s| s.world == v.session.world && v.elapsed_ms < s.until);
        if self
            .opportunity
            .as_ref()
            .is_some_and(|o| o.world != v.session.world || v.elapsed_ms >= o.until)
        {
            let o = self.opportunity.take().expect("expired call exists");
            self.record(
                v,
                o.actor,
                o.activity,
                "expired",
                "world_changed_or_expired",
                None,
            );
        }
    }
}
impl Brain {
    fn call_motive(
        &self,
        v: &PlayerView,
        c: &Content,
        activity: Activity,
    ) -> Result<Option<&'static str>> {
        match activity {
            Activity::Help => return Ok(Some("company_or_known_caller")),
            Activity::Gods | Activity::Demon | Activity::Sphinx => {
                return Ok(Some("suitable_realm_combat"));
            }
            Activity::Snake => {}
        }
        if v.character.base_stats[4] < c.classes[&v.character.class].caps[4] {
            return Ok(Some("max_speed"));
        }
        Ok(self
            .mind
            .market
            .needs_resource(v, c, self.mind.future_class, 0xa21)?
            .then_some("speed_for_unfunded_item"))
    }
    fn call_rejection(
        &self,
        v: &PlayerView,
        c: &Content,
        activity: Activity,
        caller: i32,
    ) -> Result<Option<&'static str>> {
        if !v.map.allow_teleport
            || !v.lifecycle.alive
            || v.lifecycle.busy
            || v.lifecycle.death_pending
            || v.lifecycle.departing
            || v.lifecycle.quarantined
            || !matches!(v.lifecycle.output, OutputState::Live)
        {
            return Ok(Some("lifecycle_or_map"));
        }
        if v.character.items[0] < 0 || v.character.hp * 4 < v.character.stats[0] * 3 {
            return Ok(Some("health_or_weapon"));
        }
        if v.time_ms < self.next_teleport
            || v.tick < self.recover_until
            || matches!(self.goal, Goal::Recover | Goal::Dodge | Goal::Remedy(_))
        {
            return Ok(Some("cooldown_or_recovering"));
        }
        if self.committed(v) || self.mind.market.incoming.is_some() {
            return Ok(Some("ordinary_commitment"));
        }
        if !matches!(v.map.name.as_str(), "Realm" | "Nexus")
            || matches!(self.goal, Goal::Portal(id) if v.objects.iter().any(|p| p.id == id && p.usable))
        {
            return Ok(Some("ordinary_dungeon_or_world"));
        }
        if !self.plan.idle() {
            return Ok(Some("current_errand"));
        }
        if self
            .mind
            .market
            .resources(v, c, self.mind.future_class)?
            .is_some_and(|p| {
                matches!(
                    p.state,
                    pursuits::market::acquisition::AcquisitionState::Funded { .. }
                )
            })
        {
            return Ok(Some("funded_item_purchase"));
        }
        let interested = match activity {
            Activity::Help => {
                (self.profile.companions >= 6. || self.mind.social.known(caller))
                    && progression::ready(v, c, 0x717)
            }
            Activity::Gods | Activity::Demon => {
                v.map.name == "Realm" && progression::ready(v, c, 0x718)
            }
            Activity::Sphinx => v.map.name == "Realm" && events::ready(v, c),
            Activity::Snake => v.map.name == "Realm" && progression::ready(v, c, 0x718),
        };
        if !interested {
            return Ok(Some("activity_readiness"));
        }
        if self.call_motive(v, c, activity)?.is_none() {
            return Ok(Some("no_speed_need"));
        }
        if activity == Activity::Snake {
            if !v.character.items.iter().skip(4).any(|k| *k == -1) {
                return Ok(Some("no_reward_space"));
            }
            if !v.character.items.iter().skip(4).any(|k| *k == 0xa22) {
                return Ok(Some("no_health_reserve"));
            }
        }
        let weapon = c.get(v.character.items[0] as u16)?;
        let projectile = weapon
            .projectiles
            .get(&0)
            .context("weapon projectile missing")?;
        let range = projectile.speed * projectile.lifetime / 10000.;
        if v.objects.iter().any(|o| {
            if o.hp <= 0 || !c.objects[&o.kind].enemy {
                return false;
            }
            let distance = o.position.distance(v.position);
            let can_fight = progression::can_fight(v, c, o);
            let engaged = matches!(&self.goal, Goal::Fight(target) | Goal::Hunt(target) if *target == o.id)
                && distance < range * 1.3;
            // Seeing the advertised activity is not itself an ongoing fight.
            distance <= 5.
                || (!can_fight && distance < 8.)
                || (can_fight && progression::vulnerable(o) && (distance < range || engaged))
        }) {
            return Ok(Some("nearby_combat_or_danger"));
        }
        for bag in v
            .objects
            .iter()
            .filter(|o| c.objects[&o.kind].class == "Container")
        {
            if progression::pickup(v, c, bag, self.mind.future_class, Some(&self.mind.market))?
                .is_some()
            {
                return Ok(Some("useful_loot"));
            }
        }
        Ok(None)
    }
    pub(super) fn receive_call(
        &mut self,
        v: &PlayerView,
        c: &Content,
        actor: i32,
        name: &str,
        recipient: &str,
        text: &str,
    ) -> Result<()> {
        let Some(activity) = Activity::parse(text) else {
            return Ok(());
        };
        if !recipient.is_empty()
            || actor <= 0
            || actor == v.session.player
            || self.session != (v.session.world, v.session.player, v.session.character)
        {
            self.calls.record(
                v,
                actor,
                activity,
                "declined",
                "not_public_or_current_session",
                None,
            );
            return Ok(());
        }
        let Some(peer) = v.objects.iter().find(|p| {
            p.id == actor
                && p.name == name
                && p.hp > 0
                && !p.equipped.is_empty()
                && p.account.is_some_and(|a| a > 0 && a != v.session.account)
                && p.position.distance(v.position) < 15.
        }) else {
            self.calls
                .record(v, actor, activity, "declined", "unbound_speaker", None);
            return Ok(());
        };
        let caller = peer.account.expect("bound public account");
        self.calls.observe(v);
        if self
            .calls
            .seen
            .iter()
            .any(|s| s.world == v.session.world && s.account == caller && s.activity == activity)
        {
            return Ok(());
        }
        if self.calls.seen.len() >= 32 {
            self.calls
                .record(v, actor, activity, "declined", "dedup_capacity", None);
            return Ok(());
        }
        self.calls.seen.push_back(Seen {
            world: v.session.world,
            account: caller,
            activity,
            until: v
                .elapsed_ms
                .checked_add(60_000)
                .context("public call dedup deadline overflow")?,
        });
        let rejection = if self.calls.opportunity.is_some() {
            Some("another_call_pending")
        } else if peer.position.distance(v.position) <= 7. {
            Some("already_near_caller")
        } else if !self.crowd.bands.survivable(v, peer.position) {
            Some("unsafe_realm_destination")
        } else {
            self.call_rejection(v, c, activity, caller)?
        };
        let motive = if activity == Activity::Snake || rejection.is_none() {
            self.call_motive(v, c, activity)?
        } else {
            None
        };
        if let Some(reason) = rejection {
            self.calls
                .record(v, actor, activity, "declined", reason, motive);
            return Ok(());
        }
        if self.rng.gen_bool(0.5) {
            self.calls.opportunity = Some(Opportunity {
                world: v.session.world,
                actor,
                account: caller,
                name: name.into(),
                activity,
                until: v
                    .elapsed_ms
                    .checked_add(30_000)
                    .context("public call opportunity deadline overflow")?,
            });
            self.calls
                .record(v, actor, activity, "accepted", "ready_interested", motive);
        } else {
            self.calls
                .record(v, actor, activity, "declined", "chance", motive);
        }
        Ok(())
    }
    pub(super) fn call_target(&mut self, v: &PlayerView, c: &Content) -> Result<Option<i32>> {
        self.calls.observe(v);
        let Some(o) = self.calls.opportunity.take() else {
            return Ok(None);
        };
        // Public identity, not the claimed activity, authorizes the ordinary target lookup.
        let target = v
            .player_markers
            .iter()
            .find(|p| p.id == o.actor && p.account == o.account && p.name == o.name);
        let live = v.objects.iter().find(|p| p.id == o.actor).is_some_and(|p| {
            p.hp > 0
                && !p.equipped.is_empty()
                && p.account == Some(o.account)
                && p.name == o.name
                && self.crowd.bands.survivable(v, p.position)
        });
        let rejection = if o.world != v.session.world
            || !live
            || target.is_none_or(|p| p.position.distance(v.position) <= 7.)
        {
            Some("identity_safety_or_distance_changed")
        } else {
            self.call_rejection(v, c, o.activity, o.account)?
        };
        let motive = if o.activity == Activity::Snake || rejection.is_none() {
            self.call_motive(v, c, o.activity)?
        } else {
            None
        };
        if let Some(reason) = rejection {
            self.calls
                .record(v, o.actor, o.activity, "declined", reason, motive);
            return Ok(None);
        }
        self.calls.record(
            v,
            o.actor,
            o.activity,
            "dispatched",
            "ordinary_teleport_requested_not_joined",
            motive,
        );
        Ok(Some(o.actor))
    }
}

#[cfg(test)]
mod tests;
