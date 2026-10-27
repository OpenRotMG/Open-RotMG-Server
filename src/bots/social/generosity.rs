use super::*;
use equipment::{Candidate, Policy};

#[derive(Clone)]
pub(super) struct Check {
    market: pursuits::market::Market,
    policy: Policy,
    cognition: pursuits::Cognition,
    recovery: f32,
    recover_until: u64,
}
impl Check {
    fn select(&self, v: &PlayerView, c: &Content) -> Result<Option<Candidate>> {
        let mut policy = self.policy.clone();
        let perception = combat::Perception::attend(v, &self.cognition, false);
        policy.available &= v.tick >= self.recover_until
            && v.character.hp as f32 / v.character.stats[0] as f32 >= self.recovery
            && combat::clearance(v, v.position, &perception) >= self.cognition.pickup_clearance
            && v.objects.iter().all(|o| {
                !c.objects[&o.kind].enemy || o.hp <= 0 || v.position.distance(o.position) > 12.
            });
        for bag in v
            .objects
            .iter()
            .filter(|o| c.objects[&o.kind].class == "Container")
        {
            if progression::pickup(v, c, bag, policy.future_class, Some(&self.market))?.is_some() {
                policy.available = false;
            }
        }
        equipment::select(v, c, &self.market, &policy)
    }
    pub(super) fn current(
        &self,
        candidate: &Candidate,
        v: &PlayerView,
        c: &Content,
    ) -> Result<bool> {
        Ok(self.select(v, c)?.as_ref() == Some(candidate))
    }
}
impl Brain {
    pub(super) fn equipment_check(&self, v: &PlayerView, candidate: Option<&Candidate>) -> Check {
        let mut market = self.mind.market.clone();
        // Only this offer's unchanged native window is exempt from its own stock lock.
        if let (Some(candidate), Some(t)) = (candidate, market.trade.as_ref())
            && t.binding
                == [
                    candidate.world,
                    candidate.own_player,
                    candidate.own_character,
                ]
            && t.peer == candidate.recipient_name
            && t.items[0] == v.character.items
            && candidate.inventory_revision == v.character.inventory_revision
        {
            market.trade = None;
        }
        let available = self.agreement.is_none()
            && self.companion.is_none()
            && self.group.is_none()
            && !self.proposing
            && self
                .offered
                .as_ref()
                .is_none_or(|o| o.until <= v.elapsed_ms)
            && !market.reserves_stock()
            && self
                .mind
                .intentions
                .as_ref()
                .is_some_and(|i| i.call_available(v));
        Check {
            market,
            policy: Policy {
                future_class: self.mind.future_class,
                current_goal: self.mind.intentions.as_ref().and_then(|i| i.item_target()),
                willingness: self.profile.companions,
                available,
            },
            cognition: self.mind.cognition.clone(),
            recovery: self.profile.recovery,
            recover_until: self.recover_until,
        }
    }
    pub(in crate::bots) fn equipment_opportunity(
        &mut self,
        v: &PlayerView,
        c: &Content,
    ) -> Result<bool> {
        if !self.mind.social.equipment_due(v) {
            return Ok(false);
        }
        let Some(candidate) = self.equipment_check(v, None).select(v, c)? else {
            return Ok(false);
        };
        let drop = self.rng.gen_ratio(1, 4);
        self.mind.social.offer_equipment(v, c, candidate, drop)?;
        Ok(true)
    }
}

pub(super) async fn drop_equipment(
    app: &crate::server::App,
    brain: &mut Brain,
    v: &PlayerView,
    candidate: Candidate,
) -> Result<()> {
    let check = brain.equipment_check(v, Some(&candidate));
    let content = app.content.clone();
    let id = v.session.player;
    let account = v.session.account;
    let kind = candidate.kind;
    let result = crate::server::guarded_inventory(app, id, move |w, current| {
        ensure!(
            check.current(&candidate, &current, &content)?,
            crate::server::lifecycle::controller("equipment drop candidate expired")
        );
        w.autonomous_action(
            id,
            Action::Drop {
                slot: crate::world::Slot {
                    object: id,
                    slot: candidate.slot,
                    kind: candidate.kind,
                },
            },
        )
    })
    .await?;
    match result {
        Ok(true) => tracing::info!(
            account,
            kind,
            "equipment surplus removed into an ordinary ground bag; no delivery inferred"
        ),
        Ok(false) => anyhow::bail!("equipment drop produced no inventory plan"),
        Err(error) => {
            tracing::warn!(account, kind, %error, "equipment drop rejected; no delivery inferred")
        }
    }
    Ok(())
}
