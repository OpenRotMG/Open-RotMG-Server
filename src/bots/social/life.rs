use super::*;
use serde::{Deserialize, Serialize};

const HEALTH: i32 = 0xa22;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Interaction {
    HealedBy,
    ReceivedHealth,
    GaveHealth,
    ReceivedEquipment,
    GaveEquipment,
    Refused,
    Recognized,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Episode {
    pub account: i32,
    pub name: String,
    pub own_character: i32,
    pub interaction: Interaction,
    pub amount: i32,
}
#[derive(Clone, Debug, PartialEq)]
struct Gift {
    account: i32,
    player: i32,
    name: String,
    giving: bool,
    kind: i32,
    item_name: String,
    equipment: Option<equipment::Candidate>,
    until: u64,
    session: [i32; 3],
    requested: bool,
    consent: bool,
    acceptance: Option<crate::store::GiftAcceptance>,
    cancelled: bool,
}
impl Gift {
    fn matches(&self, acceptance: &crate::store::GiftAcceptance, v: &PlayerView) -> bool {
        self.session == [v.session.world, v.session.player, v.session.character]
            && acceptance.session == self.session
            && acceptance.own.account == v.session.account
            && acceptance.own.character == v.session.character
            && acceptance.peer.account == self.account
            && acceptance.peer_player == self.player
            && acceptance.peer_name == self.name
            && acceptance.giving == self.giving
            && acceptance.kind == self.kind
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct Life {
    pub episodes: VecDeque<Episode>,
    #[serde(skip)]
    gift: Option<Gift>,
    #[serde(skip)]
    drop: Option<equipment::Candidate>,
    #[serde(skip)]
    pub(in crate::bots) outgoing: Option<String>,
    #[serde(skip)]
    heal_notice: i32,
    #[serde(skip)]
    heal_tick: u64,
    #[serde(skip)]
    heal_origin: Option<(f32, f32)>,
    #[serde(skip)]
    heal_request: Option<(i32, u64)>,
    #[serde(skip)]
    next_request: u64,
    #[serde(skip)]
    waiting: Option<(i32, u64)>,
    #[serde(skip)]
    recognized: BTreeSet<i32>,
    #[serde(skip)]
    private_trial: Option<(i32, u64)>,
}
fn health(v: &PlayerView) -> usize {
    v.character
        .items
        .iter()
        .skip(4)
        .filter(|k| **k == HEALTH)
        .count()
}
impl Life {
    pub(super) fn equipment_due(&self, v: &PlayerView) -> bool {
        !self.gift_active() && self.outgoing.is_none() && v.elapsed_ms >= self.next_request
    }
    pub(super) fn offer_equipment(
        &mut self,
        v: &PlayerView,
        c: &Content,
        candidate: equipment::Candidate,
        drop: bool,
    ) -> Result<()> {
        ensure!(
            self.equipment_due(v),
            "equipment generosity already occupied"
        );
        self.next_request = v.elapsed_ms + 60000;
        tracing::info!(
            account = v.session.account,
            ?candidate,
            drop,
            "equipment generosity initial opportunity; not delivery"
        );
        if drop {
            self.drop = Some(candidate);
            return Ok(());
        }
        let item_name = c.get(u16::try_from(candidate.kind)?)?.name.clone();
        let reason = match candidate.reason {
            equipment::Reason::MissingEquipment { equipped_slot } => {
                format!(
                    "missing {}",
                    ["weapon", "ability", "armor", "ring"][equipped_slot]
                )
            }
            equipment::Reason::PublicUpgrade { previous, .. } => {
                c.get(u16::try_from(previous)?)?.name.clone()
            }
        };
        self.outgoing = Some(format!(
            "{}, spare {} for your {}; want it?",
            candidate.recipient_name, item_name, reason
        ));
        self.gift = Some(Gift {
            account: candidate.recipient_account,
            player: candidate.recipient_player,
            name: candidate.recipient_name.clone(),
            giving: true,
            kind: candidate.kind,
            item_name,
            equipment: Some(candidate),
            until: v.elapsed_ms + 15000,
            session: [v.session.world, v.session.player, v.session.character],
            requested: false,
            consent: false,
            acceptance: None,
            cancelled: false,
        });
        Ok(())
    }
    pub(in crate::bots) fn cancel_composition(&mut self) {
        self.outgoing = None;
        if let Some(gift) = &mut self.gift {
            gift.cancelled = true;
        }
    }
    pub(in crate::bots) fn gift_active(&self) -> bool {
        self.gift.is_some() || self.drop.is_some()
    }
    pub(in crate::bots) fn bind(&mut self) {
        self.gift = None;
        self.drop = None;
        self.outgoing = None;
        self.heal_notice = 0;
        self.heal_tick = 0;
        self.heal_origin = None;
        self.heal_request = None;
        self.waiting = None;
        self.next_request = 0;
        self.recognized.clear();
        self.private_trial = None;
    }
    pub(crate) fn validate(&self, own: i32) -> Result<()> {
        ensure!(
            self.episodes.len() <= 32
                && self.episodes.iter().all(|e| e.account > 0
                    && e.account != own
                    && !e.name.is_empty()
                    && e.name.len() <= 16
                    && e.own_character > 0
                    && e.amount >= 0),
            "invalid factual acquaintance episodes"
        );
        Ok(())
    }
    fn remember(
        &mut self,
        v: &PlayerView,
        account: i32,
        name: String,
        interaction: Interaction,
        amount: i32,
    ) {
        let episode = Episode {
            account,
            name,
            own_character: v.session.character,
            interaction,
            amount,
        };
        if self.episodes.back() == Some(&episode) {
            return;
        }
        tracing::info!(
            account = v.session.account,
            character = v.session.character,
            peer = account,
            ?episode,
            "autonomous factual social episode"
        );
        self.episodes.push_back(episode);
        if self.episodes.len() > 32 {
            self.episodes.pop_front();
        }
    }
    pub(in crate::bots) fn known(&self, account: i32) -> bool {
        self.episodes.iter().any(|e| {
            e.account == account
                && matches!(
                    e.interaction,
                    Interaction::HealedBy
                        | Interaction::ReceivedHealth
                        | Interaction::GaveHealth
                        | Interaction::ReceivedEquipment
                        | Interaction::GaveEquipment
                )
        })
    }
    fn wants_health(v: &PlayerView) -> bool {
        health(v) < 2 && v.character.items.iter().skip(4).any(|k| *k < 0)
    }
    fn generous(
        &self,
        v: &PlayerView,
        p: &crate::world::view::VisibleObject,
        willingness: f32,
    ) -> bool {
        health(v) > 2
            && v.character.hp * 4 >= v.character.stats[0] * 3
            && (willingness >= 6. || p.account.is_some_and(|a| self.known(a)))
    }
    pub(in crate::bots) fn receive(
        &mut self,
        packet: &Packet,
        v: &PlayerView,
        own_name: &str,
        willingness: f32,
        incoming_trade: bool,
    ) -> Result<bool> {
        if incoming_trade && packet.id == wire::TEXT {
            tracing::info!(
                account = v.session.account,
                reason = "active_incoming_trade",
                "canonical social gift uptake declined"
            );
            return Ok(false);
        }
        if self.heal_tick != v.tick {
            self.heal_notice = 0;
            self.heal_origin = None;
        }
        let mut r = Reader::new(&packet.body);
        if packet.id == wire::NOTIFICATION {
            let target = r.i32()?;
            let text = r.utf()?;
            let color = r.i32()? as u32;
            r.done()?;
            if target == v.session.player
                && color == 0xff00ff00
                && let Some(amount) = text.strip_prefix('+')
            {
                self.heal_notice = amount.parse().context("invalid native healing amount")?;
                self.heal_tick = v.tick;
            }
            return Ok(false);
        }
        if packet.id == wire::SHOW_EFFECT {
            let effect = r.u8()?;
            let target = r.i32()?;
            let a = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            r.f32()?;
            r.f32()?;
            r.i32()?;
            r.done()?;
            if effect == 1 && target == v.session.player && self.heal_notice > 0 {
                self.heal_origin = Some((a.x, a.y));
            }
            if effect == 5
                && self.heal_notice > 0
                && let Some(origin) = self.heal_origin.take()
            {
                if let Some(p) = v.objects.iter().find(|p| {
                    p.id == target
                        && p.kind == PRIEST
                        && p.position.distance(Pos {
                            x: origin.0,
                            y: origin.1,
                        }) < 0.5
                        && p.hp > 0
                }) && let Some(account) = p.account
                {
                    let amount = self.heal_notice;
                    self.remember(v, account, p.name.clone(), Interaction::HealedBy, amount);
                    self.outgoing = Some(format!("{}, thanks for the heal", p.name));
                }
                self.heal_notice = 0;
            }
            return Ok(false);
        }
        if packet.id == 12 && self.gift.is_some() {
            let result = r.i32()?;
            r.utf()?;
            r.done()?;
            let g = self.gift.take().unwrap();
            self.outgoing = None;
            tracing::info!(
                account = v.session.account,
                peer = g.account,
                result,
                "gift ended without a verified transaction receipt"
            );
            return Ok(false);
        }
        if packet.id != wire::TEXT {
            return Ok(false);
        }
        let name = r.utf()?;
        let actor = r.i32()?;
        r.i32()?;
        r.u8()?;
        let recipient = r.utf()?;
        let text = r.utf()?;
        r.utf()?;
        r.done()?;
        if actor == v.session.player || !(recipient.is_empty() || recipient == own_name) {
            return Ok(false);
        }
        let Some(p) = v
            .objects
            .iter()
            .find(|p| p.id == actor && p.name == name && p.hp > 0 && !p.equipped.is_empty())
        else {
            return Ok(false);
        };
        let Some(account) = p.account else {
            return Ok(false);
        };
        let Some(body) = text.strip_prefix(&format!("{own_name}, ")) else {
            return Ok(false);
        };
        match body {
            "heal please" => {
                self.heal_request = Some((account, v.elapsed_ms + 10000));
                if v.character.class != PRIEST || v.character.items[1] < 0 {
                    return Ok(true);
                }
            }
            "spare health?" => {
                if self.gift.is_some() {
                    return Ok(true);
                }
                if self.generous(v, p, willingness) {
                    self.gift = Some(Gift {
                        account,
                        player: actor,
                        name: name.clone(),
                        giving: true,
                        kind: HEALTH,
                        item_name: "health".into(),
                        equipment: None,
                        consent: false,
                        acceptance: None,
                        cancelled: false,
                        until: v.elapsed_ms + 15000,
                        session: [v.session.world, v.session.player, v.session.character],
                        requested: false,
                    });
                    self.outgoing = Some(format!("{name}, take health"));
                } else {
                    self.outgoing = Some(format!("{name}, keeping my supplies"));
                    self.remember(v, account, name, Interaction::Refused, 0);
                }
            }
            "take health" => {
                if self.gift.is_some() {
                    return Ok(true);
                }
                if Self::wants_health(v) {
                    self.gift = Some(Gift {
                        account,
                        player: actor,
                        name: name.clone(),
                        giving: false,
                        kind: HEALTH,
                        item_name: "health".into(),
                        equipment: None,
                        consent: true,
                        acceptance: None,
                        cancelled: false,
                        until: v.elapsed_ms + 15000,
                        session: [v.session.world, v.session.player, v.session.character],
                        requested: false,
                    });
                    self.outgoing = Some(format!("{name}, yes health"));
                } else {
                    self.outgoing = Some(format!("{name}, no thanks"));
                    self.remember(v, account, name, Interaction::Refused, 0);
                }
            }
            "yes health" => {
                if let Some(g) = self.gift.as_mut().filter(|g| {
                    g.giving
                        && g.kind == HEALTH
                        && g.account == account
                        && g.player == actor
                        && g.name == name
                        && v.elapsed_ms < g.until
                        && !g.cancelled
                }) {
                    g.consent = true;
                }
            }
            "no thanks" | "keeping my supplies" => {
                if let Some(g) = self
                    .gift
                    .as_mut()
                    .filter(|g| g.account == account && g.player == actor)
                {
                    g.cancelled = true;
                    self.outgoing = None;
                }
                self.next_request = v.elapsed_ms + 60000;
            }
            _ => {
                if self.outgoing.is_some() {
                    return Ok(false);
                }
                if let Some(g) = self.gift.as_mut().filter(|g| {
                    g.giving
                        && g.kind != HEALTH
                        && g.account == account
                        && g.player == actor
                        && g.name == name
                        && !g.cancelled
                        && v.elapsed_ms < g.until
                        && g.session == [v.session.world, v.session.player, v.session.character]
                }) && body == format!("yes {}", g.item_name)
                {
                    g.consent = true;
                } else {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    pub(in crate::bots) fn healing(
        &self,
        v: &PlayerView,
        c: &Content,
        willingness: f32,
    ) -> Result<Option<Action>> {
        let reserve = v.character.items[1] >= 0
            && v.character.mp < c.get(v.character.items[1] as u16)?.mana_cost * 2
            && v.character.hp * 4 >= v.character.stats[0] * 3;
        super::healing_selected(v, c, |p| {
            !reserve
                && (p.account.is_some_and(|a| {
                    self.known(a)
                        || self
                            .heal_request
                            .is_some_and(|(who, until)| a == who && v.elapsed_ms < until)
                }) || willingness >= 6.)
        })
    }
    pub(in crate::bots) fn trying_loadout(&mut self, v: &PlayerView) -> bool {
        if v.character.items[0] < 0 {
            return false;
        }
        let (weapon, since) = *self.private_trial.get_or_insert_with(|| {
            tracing::info!(
                account = v.session.account,
                character = v.session.character,
                weapon = v.character.items[0],
                revision = v.character.inventory_revision,
                "autonomous private loadout trial before next outing"
            );
            (v.character.items[0], v.elapsed_ms)
        });
        weapon == v.character.items[0] && v.elapsed_ms.saturating_sub(since) < 1500
    }
    pub(in crate::bots) fn decide(
        &mut self,
        v: &PlayerView,
        grid: &Grid,
        willingness: f32,
        banking: bool,
    ) -> Option<Decision> {
        if self.gift.is_some() {
            return Some(idle());
        }
        if v.elapsed_ms >= self.next_request
            && let Some(p) = v.objects.iter().find(|p| {
                p.hp > 0
                    && p.account.is_some_and(|a| self.known(a))
                    && v.position.distance(p.position) < 10.
            })
        {
            if v.character.hp * 2 < v.character.stats[0] && p.kind == PRIEST {
                self.outgoing = Some(format!("{}, heal please", p.name));
            } else if Self::wants_health(v) {
                self.outgoing = Some(format!("{}, spare health?", p.name));
            }
            self.next_request = v.elapsed_ms + 60000;
        }
        if let Some(p) = v.objects.iter().find(|p| {
            p.account
                .is_some_and(|a| self.known(a) && !self.recognized.contains(&a))
                && p.hp > 0
        }) {
            let account = p.account.unwrap();
            self.recognized.insert(account);
            self.outgoing
                .get_or_insert_with(|| format!("{}, good to see you again", p.name));
            self.remember(v, account, p.name.clone(), Interaction::Recognized, 0);
        }
        if v.map.name == "Nexus"
            && !banking
            && let Some(p) = v
                .objects
                .iter()
                .filter(|p| p.kind == 0x712)
                .max_by_key(|p| p.usable)
        {
            if !p.usable {
                let (portal, since) = *self.waiting.get_or_insert((p.id, v.elapsed_ms));
                if portal != p.id {
                    self.waiting = Some((p.id, v.elapsed_ms));
                }
                if v.elapsed_ms == since {
                    self.outgoing = Some(
                        if willingness >= 6. {
                            "Realm is full, looking for a Vault"
                        } else {
                            "Realm is full, waiting here"
                        }
                        .into(),
                    );
                    tracing::info!(
                        account = v.session.account,
                        portal = p.id,
                        name = p.name,
                        "autonomous full entrance observed"
                    );
                }
                if willingness >= 6. && v.elapsed_ms.saturating_sub(since) >= 5000 {
                    if let Some(vault) = v.objects.iter().find(|p| p.kind == 0x720 && p.usable) {
                        let near = v.position.distance(vault.position) < 1.4;
                        let mut d = idle();
                        d.goal = Goal::Portal(vault.id);
                        d.movement = if near {
                            None
                        } else {
                            grid.towards(v.position, vault.position)
                        };
                        d.journey = near.then_some(Journey::Portal(vault.id));
                        return Some(d);
                    }
                    let next = grid
                        .reachable(v.position)
                        .iter()
                        .copied()
                        .filter(|t| grid.frontier(*t, v.map.width, v.map.height))
                        .min_by(|a, b| {
                            center(*a)
                                .distance(v.position)
                                .total_cmp(&center(*b).distance(v.position))
                        });
                    if let Some(next) = next {
                        let mut d = idle();
                        d.goal = Goal::Explore(next.0, next.1);
                        d.movement = grid.towards(v.position, center(next));
                        return Some(d);
                    }
                }
                return Some(idle());
            }
            if self.waiting.take().is_some() {
                tracing::info!(
                    account = v.session.account,
                    portal = p.id,
                    name = p.name,
                    "autonomous vacancy observed; reconsidering Realm entry"
                );
            }
        }
        None
    }
}

fn idle() -> Decision {
    Decision {
        goal: Goal::Wait,
        movement: None,
        aim: None,
        item: None,
        journey: None,
        chat: None,
    }
}

async fn recover_gift_receipt(
    app: &crate::server::App,
    brain: &mut Brain,
    v: &PlayerView,
) -> Result<bool> {
    let session = [v.session.world, v.session.player, v.session.character];
    if let Some(g) = brain.mind.social.gift.clone()
        && let Some(acceptance) = g.acceptance.clone()
    {
        ensure!(
            g.matches(&acceptance, v),
            "gift acceptance belongs to another offer/session"
        );
        let verified = app.store.gift_committed(acceptance.clone()).await?;
        let current = app
            .world
            .directory({
                let acceptance = acceptance.clone();
                move |d| {
                    ensure!(
                        d.location(session[1])? == session[0],
                        "gift acceptance world changed"
                    );
                    d.worlds[&session[0]].gift_acceptance_current(&acceptance)
                }
            })
            .await?;
        if verified {
            let settled = app.world.player_view(session[1]).await?;
            ensure!(
                [
                    settled.session.world,
                    settled.session.player,
                    settled.session.character
                ] == session
                    && settled.session.account == acceptance.own.account,
                "gift receipt completion session changed"
            );
            if !settled.lifecycle.busy
                && !settled.lifecycle.quarantined
                && !settled.lifecycle.departing
                && settled.lifecycle.alive
                && settled.character.inventory_revision > acceptance.own.revision
            {
                ensure!(
                    brain.mind.social.gift.as_ref() == Some(&g),
                    "gift offer changed while verifying its receipt"
                );
                brain.mind.social.gift = None;
                brain.mind.social.outgoing = None;
                brain.mind.social.remember(
                    &settled,
                    g.account,
                    g.name.clone(),
                    match (g.kind == HEALTH, g.giving) {
                        (true, true) => Interaction::GaveHealth,
                        (true, false) => Interaction::ReceivedHealth,
                        (false, true) => Interaction::GaveEquipment,
                        (false, false) => Interaction::ReceivedEquipment,
                    },
                    1,
                );
                if !g.giving {
                    brain.mind.social.outgoing =
                        Some(format!("{}, thanks for the {}", g.name, g.item_name));
                }
                tracing::info!(account=settled.session.account,peer=g.account,transaction=%acceptance.transaction,"gift exact transaction receipt applied once");
                return Ok(false);
            }
        }
        if !verified && !current {
            tracing::info!(account=v.session.account,peer=g.account,transaction=%acceptance.transaction,"gift native acceptance invalidated before commitment");
            brain.mind.social.gift.as_mut().unwrap().acceptance = None;
        }
        return Ok(verified || current);
    }
    Ok(false)
}

pub(in crate::bots) async fn receive_life(
    app: &crate::server::App,
    brain: &mut Brain,
    packet: &Packet,
    v: &PlayerView,
) -> Result<bool> {
    let session = [v.session.world, v.session.player, v.session.character];
    ensure!(
        session == [brain.session.0, brain.session.1, brain.session.2]
            && v.session.account == brain.mind.account,
        "social packet belongs to a replaced session"
    );
    if matches!(packet.id, 12 | 23 | 67) {
        if packet.id == 12
            && brain
                .mind
                .social
                .gift
                .as_ref()
                .is_some_and(|g| g.acceptance.is_some())
        {
            let mut r = Reader::new(&packet.body);
            r.i32()?;
            r.utf()?;
            r.done()?;
        }
        let protected = recover_gift_receipt(app, brain, v).await?;
        // A legacy result cannot identify the native transaction whose receipt is still pending.
        if packet.id == 12 && protected {
            return Ok(false);
        }
    }
    brain.mind.social.receive(
        packet,
        v,
        &brain.mind.market.name,
        brain.profile.companions,
        brain.mind.market.incoming.is_some(),
    )
}

pub(in crate::bots) async fn cancel_gift(
    app: &crate::server::App,
    brain: &mut Brain,
    v: &PlayerView,
) -> Result<bool> {
    brain.mind.social.drop = None;
    let Some(g) = brain.mind.social.gift.as_mut() else {
        return Ok(false);
    };
    g.cancelled = true;
    if g.kind != HEALTH {
        brain.mind.social.next_request = v.elapsed_ms + 60000;
    }
    // A mutual native accept may already own the commit; keep its receipt binding until completion.
    if !super::revoke_native_market(app, brain).await? {
        return Ok(true);
    }
    if recover_gift_receipt(app, brain, v).await? {
        return Ok(true);
    }
    brain.mind.social.gift = None;
    brain.mind.social.outgoing = None;
    brain.mind.market.trade = None;
    Ok(false)
}

pub(in crate::bots) async fn life(
    app: &crate::server::App,
    brain: &mut Brain,
    v: &PlayerView,
) -> Result<bool> {
    let binding = [v.session.world, v.session.player, v.session.character];
    ensure!(
        binding == [brain.session.0, brain.session.1, brain.session.2]
            && v.session.account == brain.mind.account
            && brain
                .mind
                .social
                .gift
                .as_ref()
                .is_none_or(|g| g.session == binding),
        "gift action belongs to a replaced session"
    );
    if recover_gift_receipt(app, brain, v).await? {
        return Ok(true);
    }
    if let Some(candidate) = brain.mind.social.drop.take() {
        generosity::drop_equipment(app, brain, v, candidate).await?;
        return Ok(true);
    }
    if brain.mind.social.gift.as_ref().is_some_and(|g| g.cancelled) {
        return cancel_gift(app, brain, v).await;
    }
    if v.elapsed_ms >= brain.mind.market.next_message_ms
        && let Some(text) = brain.mind.social.outgoing.clone()
    {
        if !brain.compose(app, v, composer::Source::Gift, &text).await? {
            return Ok(brain.composing(composer::Source::Gift).is_some());
        }
        let id = v.session.player;
        let revision = v.character.inventory_revision;
        let gift = brain.mind.social.gift.clone();
        let social = brain.mind.social.clone();
        let generosity = brain.profile.companions;
        let check = brain.equipment_check(v, gift.as_ref().and_then(|g| g.equipment.as_ref()));
        let content = app.content.clone();
        let recipient = if matches!(
            text.as_str(),
            "Realm is full, looking for a Vault" | "Realm is full, waiting here"
        ) {
            None
        } else {
            text.split_once(',').map(|(name, _)| name.to_owned())
        };
        let received = app.world.clock.now();
        let published = app
            .world
            .directory(move |d| {
                let current = d.player_view(id)?;
                ensure!(
                    [current.session.world, id, current.session.character] == binding
                        && current.character.inventory_revision == revision,
                    "gift speech session or stock changed"
                );
                ensure!(
                    recipient.as_ref().is_none_or(|name| current
                        .objects
                        .iter()
                        .any(|p| &p.name == name && p.hp > 0)),
                    "gift speech recipient left"
                );
                if text.starts_with("Realm is full, ") {
                    ensure!(
                        current.objects.iter().any(|p| p.kind == 0x712 && !p.usable),
                        "full entrance speech intention expired"
                    );
                }
                if let Some(g) = gift {
                    if let Some(candidate) = &g.equipment {
                        ensure!(
                            check.current(candidate, &current, &content)?,
                            "equipment speech candidate expired"
                        );
                    }
                    let peer = current.objects.iter().find(|p| {
                        p.id == g.player
                            && p.account == Some(g.account)
                            && p.name == g.name
                            && p.hp > 0
                    });
                    ensure!(
                        !g.cancelled
                            && current.elapsed_ms < g.until
                            && peer.is_some_and(|p| current.position.distance(p.position) < 2.
                                && if g.giving {
                                    g.equipment.is_some()
                                        || social.generous(&current, p, generosity)
                                } else {
                                    Life::wants_health(&current)
                                }),
                        "gift speech intention expired"
                    );
                }
                d.submit_bot_chat(id, text, received)
            })
            .await;
        brain.close_composition(app, published.is_ok()).await?;
        brain.mind.social.outgoing = None;
        match published {
            Ok(()) => {
                tracing::info!(
                    account = v.session.account,
                    "canonical gift protocol published without generation"
                );
                brain.mind.market.next_message_ms = v.elapsed_ms + 1500;
            }
            Err(error) => {
                tracing::warn!(account=v.session.account,%error,"social publication rejected; no assent inferred");
                return cancel_gift(app, brain, v).await;
            }
        }
    }
    let Some(mut g) = brain.mind.social.gift.clone() else {
        return Ok(false);
    };
    let peer = v
        .objects
        .iter()
        .find(|p| p.id == g.player && p.account == Some(g.account) && p.name == g.name && p.hp > 0);
    let allowed = v.elapsed_ms < g.until
        && peer.is_some_and(|p| {
            v.position.distance(p.position) < 2.
                && if g.giving {
                    g.equipment.is_some()
                        || brain.mind.social.generous(v, p, brain.profile.companions)
                } else {
                    Life::wants_health(v)
                }
        });
    if !allowed
        || g.equipment
            .as_ref()
            .is_some_and(|candidate| candidate.kind != g.kind)
    {
        return cancel_gift(app, brain, v).await;
    }
    let check = brain.equipment_check(v, g.equipment.as_ref());
    if let Some(candidate) = &g.equipment
        && !check.current(candidate, v, &app.content)?
    {
        return cancel_gift(app, brain, v).await;
    }
    use crate::world::TradeAction;
    let action = if let Some(t) = &mut brain.mind.market.trade {
        if t.peer != g.name || (g.kind != HEALTH && !g.consent) {
            Some(TradeAction::Cancel)
        } else {
            let donor = usize::from(!g.giving);
            let mut offer = [false; 12];
            if let Some(slot) = t.items[donor]
                .iter()
                .enumerate()
                .skip(4)
                .find(|(slot, k)| {
                    **k == g.kind
                        && g.equipment
                            .as_ref()
                            .is_none_or(|candidate| candidate.slot == *slot)
                })
                .map(|(i, _)| i)
            {
                offer[slot] = true;
            }
            if !offer.iter().any(|b| *b) {
                Some(TradeAction::Cancel)
            } else {
                let (mine, theirs) = if g.giving {
                    (offer, [false; 12])
                } else {
                    ([false; 12], offer)
                };
                if !t.changed || t.offers[0] != mine {
                    t.changed = true;
                    t.offers[0] = mine;
                    t.ready = v.time_ms + 3000;
                    Some(TradeAction::Change { offer: mine })
                } else if t.offers[1] == theirs
                    && v.time_ms >= t.ready
                    && v.clock_at >= t.ready_at
                    && g.acceptance.is_none()
                {
                    Some(TradeAction::Accept { mine, theirs })
                } else {
                    None
                }
            }
        }
    } else if !g.requested && g.consent && brain.mind.social.outgoing.is_none() {
        g.requested = true;
        Some(TradeAction::Request {
            name: g.name.clone(),
        })
    } else {
        None
    };
    brain.mind.social.gift = Some(g.clone());
    if let Some(action) = action {
        let cancel = matches!(action, TradeAction::Cancel);
        let changed = matches!(action, TradeAction::Change { .. });
        let id = v.session.player;
        let session = [v.session.world, id, v.session.character];
        let content = app.content.clone();
        let result = app
            .world
            .directory(move |d| {
                ensure!(d.location(id)? == session[0], "gift action world changed");
                if let Some(candidate) = &g.equipment {
                    ensure!(
                        g.consent
                            && !g.cancelled
                            && check.current(candidate, &d.player_view(id)?, &content)?,
                        "equipment action candidate expired"
                    );
                }
                d.worlds
                    .get_mut(&session[0])
                    .context("gift world missing")?
                    .autonomous_gift(
                        id,
                        session,
                        g.player,
                        (g.account, g.name),
                        g.giving,
                        g.kind,
                        action,
                    )
            })
            .await;
        match result {
            Ok(outcome) => {
                if let Some(acceptance) = outcome.acceptance {
                    let gift = brain
                        .mind
                        .social
                        .gift
                        .as_mut()
                        .context("gift disappeared during native acceptance")?;
                    ensure!(
                        gift.matches(&acceptance, v),
                        "native gift acceptance counterparty/session changed"
                    );
                    tracing::info!(account=v.session.account,peer=gift.account,transaction=%acceptance.transaction,"gift native acceptance witness bound; not a payment receipt");
                    gift.acceptance = Some(acceptance);
                }
                if let Some(plan) = outcome.plan {
                    crate::server::persist_trade(app, plan).await?;
                } else if changed {
                    let acknowledged = app.world.player_view(id).await?;
                    let t = brain
                        .mind
                        .market
                        .trade
                        .as_mut()
                        .context("gift trade disappeared after offer acknowledgement")?;
                    t.ready = acknowledged.time_ms + 3000;
                    t.ready_at = acknowledged.clock_at + std::time::Duration::from_millis(3000);
                }
            }
            Err(error) => {
                tracing::warn!(account=v.session.account,%error,"guarded gift rejected");
                return cancel_gift(app, brain, v).await;
            }
        }
        if cancel {
            return cancel_gift(app, brain, v).await;
        }
    }
    Ok(brain.mind.social.gift.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::runtime::tests::actor;
    use crate::world::TradeAction;

    #[tokio::test]
    async fn gift_kind_health_history_requires_commit_and_applies_once() {
        let (dir, app) = crate::server::test_app();
        let (_owner, id, _output, mut brain) = actor(
            &app,
            "gift-health",
            "Donor",
            [HEALTH, -1, -1, -1, -1, -1, -1, -1],
        )
        .await;
        let (_peer_owner, peer, _peer_output, _) =
            actor(&app, "gift-health-peer", "Recipient", [-1; 8]).await;
        let view = app.world.player_view(id).await.unwrap();
        let recipient = app.world.player_view(peer).await.unwrap();
        let session = [view.session.world, id, view.session.character];
        let binding = (recipient.session.account, "Recipient".to_owned());
        let mut mine = [false; 12];
        mine[4] = true;
        let theirs = [false; 12];
        let setup_binding = binding.clone();
        app.world
            .directory(move |d| {
                let w = d.worlds.get_mut(&session[0]).unwrap();
                w.players.get_mut(&peer).unwrap().pos = w.players[&id].pos;
                w.updates(id, true)?;
                w.updates(peer, true)?;
                w.request_trade(peer, w.players[&id].character.name.clone())?;
                w.autonomous_gift(
                    id,
                    session,
                    peer,
                    setup_binding.clone(),
                    true,
                    HEALTH,
                    TradeAction::Request {
                        name: setup_binding.1.clone(),
                    },
                )?;
                w.autonomous_gift(
                    id,
                    session,
                    peer,
                    setup_binding,
                    true,
                    HEALTH,
                    TradeAction::Change { offer: mine },
                )?;
                Ok(())
            })
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(3050)).await;
        let witness = app
            .world
            .directory(move |d| {
                Ok(d.worlds
                    .get_mut(&session[0])
                    .unwrap()
                    .autonomous_gift(
                        id,
                        session,
                        peer,
                        binding,
                        true,
                        HEALTH,
                        TradeAction::Accept { mine, theirs },
                    )?
                    .acceptance
                    .unwrap())
            })
            .await
            .unwrap();
        let gift = Gift {
            account: recipient.session.account,
            player: peer,
            name: witness.peer_name.clone(),
            giving: true,
            kind: HEALTH,
            item_name: "health".into(),
            equipment: None,
            until: view.time_ms + 30000,
            session,
            requested: true,
            consent: true,
            acceptance: Some(witness.clone()),
            cancelled: false,
        };
        let mut other_kind = witness;
        other_kind.kind = 0xa97;
        assert!(!gift.matches(&other_kind, &view));
        brain.mind.social.gift = Some(gift);
        assert!(recover_gift_receipt(&app, &mut brain, &view).await.unwrap());
        assert!(brain.mind.social.episodes.is_empty());
        let plan = app
            .world
            .directory(move |d| {
                Ok(d.worlds
                    .get_mut(&session[0])
                    .unwrap()
                    .accept_trade(peer, theirs, mine)?
                    .unwrap())
            })
            .await
            .unwrap();
        crate::server::persist_trade(&app, plan).await.unwrap();
        let settled = app.world.player_view(id).await.unwrap();
        assert!(
            !recover_gift_receipt(&app, &mut brain, &settled)
                .await
                .unwrap()
        );
        assert!(brain.mind.social.gift.is_none());
        assert_eq!(brain.mind.social.episodes.len(), 1);
        assert_eq!(
            brain.mind.social.episodes[0].interaction,
            Interaction::GaveHealth
        );
        assert!(
            !recover_gift_receipt(&app, &mut brain, &settled)
                .await
                .unwrap()
        );
        assert_eq!(brain.mind.social.episodes.len(), 1);
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "life/equipment_tests.rs"]
mod equipment_tests;
