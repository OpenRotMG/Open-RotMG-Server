use super::*;
use rand::seq::IteratorRandom;
use std::time::{Duration, Instant};

mod incoming;
mod replies;
use incoming::Incoming;
pub(in crate::bots) use replies::PendingTrade;
use replies::{BoundReply, ReplyKind};

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("market wall clock predates epoch")
        .as_secs()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Goods {
    pub kind: i32,
    pub quantity: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Terms {
    pub version: u64,
    pub accounts: [i32; 2],
    pub names: [String; 2],
    pub goods: [Goods; 2],
    pub meeting: [i32; 3],
    pub expires: u64,
    pub batch: u8,
}
impl Terms {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.version > 0
                && self.accounts.iter().all(|a| *a > 0)
                && self.accounts[0] != self.accounts[1]
                && self.names.iter().all(|n| !n.is_empty() && n.len() <= 16)
                && self.names[0] != self.names[1]
                && (1..=8).contains(&self.batch)
                && self
                    .goods
                    .iter()
                    .all(|g| (0..=65535).contains(&g.kind) && (1..=16).contains(&g.quantity))
                && self.goods[0].kind != self.goods[1].kind
                && self.meeting[0] == -2
                && self.meeting[1..].iter().all(|n| (0..=65535).contains(n)),
            "invalid market terms"
        );
        Ok(())
    }
    pub(crate) fn delivery(&self, ordinal: u8) -> Result<Delivery> {
        self.validate()?;
        let quantities = self.goods.each_ref().map(|g| {
            g.quantity
                .saturating_sub(ordinal.saturating_mul(self.batch))
                .min(self.batch)
        });
        ensure!(quantities != [0, 0], "market delivery already complete");
        Ok(Delivery {
            terms: self.clone(),
            ordinal,
            quantities,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Delivery {
    pub terms: Terms,
    pub ordinal: u8,
    pub quantities: [u8; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Contact {
    account: i32,
    name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Bargain {
    pub terms: Terms,
    pub own_consent: bool,
    pub agreed: bool,
    pub peer_consent: Option<[i32; 3]>,
    pub confirmed: u8,
    #[serde(default)]
    pub consent_revision: u8,
    pub rounds: u8,
    pub resolution: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct MarketSession {
    anchor: TilePoint,
    until: u64,
    next_move: u64,
    destination: Option<TilePoint>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Market {
    #[serde(skip)]
    session: Option<MarketSession>,
    #[serde(skip)]
    session_used: bool,
    #[serde(skip)]
    pub(in crate::bots) name: String,
    #[serde(skip)]
    pub(in crate::bots) trader: bool,
    #[serde(skip)]
    pub(in crate::bots) next_message_ms: u64,
    #[serde(skip)]
    next_request_ms: u64,
    #[serde(default)]
    quote: Option<[Goods; 2]>,
    #[serde(default)]
    preparing: bool,
    contacts: Vec<Contact>,
    pub bargain: Option<Bargain>,
    history: VecDeque<Bargain>,
    vault: Option<Vec<i32>>,
    // Old exports retain this hint without restoring the process-local ad timer.
    #[serde(default, rename = "next_ad")]
    legacy_next_ad: u64,
    #[serde(skip)]
    next_ad: u64,
    #[serde(skip)]
    pub(in crate::bots) outgoing: Option<Message>,
    #[serde(skip)]
    pub(in crate::bots) trade: Option<SessionTrade>,
    #[serde(skip)]
    pub(in crate::bots) incoming: Option<Incoming>,
    #[serde(skip)]
    reply: Option<BoundReply>,
    #[serde(skip)]
    pub(in crate::bots) pending_trade: Option<PendingTrade>,
    #[serde(skip)]
    pub(in crate::bots) revoke_trade: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::bots) enum Channel {
    Local,
    Tell(String),
    Guild,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::bots) struct Message {
    pub text: String,
    pub channel: Channel,
}
impl Message {
    pub(in crate::bots) fn chat(&self) -> String {
        match &self.channel {
            Channel::Local => self.text.clone(),
            Channel::Tell(n) => format!("/tell {n} {}", self.text),
            Channel::Guild => format!("/g {}", self.text),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::bots) struct SessionTrade {
    pub binding: [i32; 3],
    pub peer: String,
    pub items: [Vec<i32>; 2],
    pub tradeable: [bool; 12],
    pub offers: [[bool; 12]; 2],
    pub ready: u64,
    pub ready_at: Instant,
    pub empty_offer_until: Instant,
    pub changed: bool,
}
impl Market {
    pub(crate) fn validate(&self, account: i32) -> Result<()> {
        ensure!(
            self.contacts.len() <= 32
                && self.history.len() <= 8
                && self
                    .vault
                    .as_ref()
                    .is_none_or(|v| v.len() == 8 && v.iter().all(|k| (-1..=65535).contains(k))),
            "invalid market memory bounds"
        );
        ensure!(
            self.quote.as_ref().is_none_or(|q| q
                .iter()
                .all(|g| (0..=65535).contains(&g.kind) && (1..=8).contains(&g.quantity))
                && q[0].kind != q[1].kind),
            "invalid remembered market quote"
        );
        for b in self.bargain.iter().chain(self.history.iter()) {
            b.terms.validate()?;
            ensure!(
                b.terms.accounts.contains(&account)
                    && b.confirmed <= 16
                    && b.consent_revision <= 4
                    && b.resolution.as_ref().is_none_or(|r| r.len() <= 128),
                "invalid account-bound bargain"
            );
        }
        Ok(())
    }
    pub(in crate::bots) fn bind(&mut self) {
        self.session = None;
        self.session_used = false;
        self.incoming = None;
        self.reply = None;
        self.pending_trade = None;
        self.outgoing = None;
        self.trade = None;
        self.next_ad = 0;
        self.revoke_trade = false;
        self.next_request_ms = 0;
        if let Some(b) = &mut self.bargain {
            b.own_consent = false;
            b.peer_consent = None;
        }
    }
    pub(in crate::bots) fn request_due(&mut self, elapsed_ms: u64) -> bool {
        if elapsed_ms < self.next_request_ms {
            return false;
        }
        self.next_request_ms = elapsed_ms + 20_000;
        true
    }
    pub(in crate::bots) fn observe(&mut self, v: &PlayerView) {
        // Publication runs before decide can retire an expired session.
        if self
            .session
            .as_ref()
            .is_some_and(|s| v.elapsed_ms >= s.until)
            && self
                .outgoing
                .as_ref()
                .is_some_and(|m| m.text.starts_with("wts "))
        {
            self.outgoing = None;
        }
        if v.map.name != "Nexus" {
            if v.map.name != "Vault" {
                self.preparing = false;
            }
            self.end_session(v, "left_nexus");
            self.session_used = false;
        }
        for p in v
            .objects
            .iter()
            .filter(|p| p.hp > 0 && !p.equipped.is_empty())
        {
            if let Some(account) = p.account {
                self.contacts
                    .retain(|c| c.account != account && c.name != p.name);
                self.contacts.push(Contact {
                    account,
                    name: p.name.clone(),
                });
                if self.contacts.len() > 32 {
                    self.contacts.remove(0);
                }
            }
        }
        if v.map.name == "Vault"
            && let Some(chest) = v.objects.iter().find(|p| p.kind == 0x504)
        {
            if self.vault.as_ref() != Some(&chest.contents) {
                tracing::info!(account=v.session.account,revision=v.character.inventory_revision,items=?v.character.items,vault=?chest.contents,"market own private stock observed");
            }
            self.vault = Some(chest.contents.clone());
        }
    }
    fn count(&self, v: &PlayerView, kind: i32) -> usize {
        v.character
            .items
            .iter()
            .skip(4)
            .chain(self.vault.iter().flatten())
            .filter(|k| **k == kind)
            .count()
    }
    fn wanted(&self, v: &PlayerView, c: &Content, future: Option<u16>, kind: i32) -> Result<u8> {
        let item = c.get(u16::try_from(kind)?)?;
        if item.soulbound || item.unsupported_equipment {
            return Ok(0);
        }
        let desired = if kind == LIFE {
            if self.gear_want(v, c, future, LIFE_GEAR_TIER)?.is_some() {
                2
            } else {
                0
            }
        } else if kind == DEFENSE && self.gear_want(v, c, future, 8)?.is_some() {
            7
        } else if let Some(index) = progression::potion_index(c, kind)? {
            (c.classes[&v.character.class].caps[index] - v.character.base_stats[index]).clamp(0, 16)
                as usize
        } else if item.activate == "Heal" || (item.activate == "Magic" && v.character.items[1] >= 0)
        {
            2
        } else if progression::priority(v, c, kind, false, future)? >= 60 {
            1
        } else {
            0
        };
        Ok(desired.saturating_sub(self.count(v, kind)).min(16) as u8)
    }
    fn spare(&self, v: &PlayerView, c: &Content, future: Option<u16>, kind: i32) -> Result<u8> {
        let item = c.get(u16::try_from(kind)?)?;
        if item.soulbound || item.unsupported_equipment || item.class != "Equipment" {
            return Ok(0);
        }
        let count = self.count(v, kind);
        let keep = if matches!(kind, SPEED | DEFENSE | LIFE) {
            0
        } else if let Some(index) = progression::potion_index(c, kind)? {
            (c.classes[&v.character.class].caps[index] - v.character.base_stats[index]).clamp(0, 16)
                as usize
        } else if item.activate == "Heal" || (item.activate == "Magic" && v.character.items[1] >= 0)
        {
            2
        } else if progression::priority(v, c, kind, true, future)? >= 60 {
            1
        } else {
            0
        };
        Ok(count.saturating_sub(keep).min(16) as u8)
    }
    fn stock(&self, v: &PlayerView, c: &Content, future: Option<u16>) -> Result<Vec<Goods>> {
        let mut goods = Vec::new();
        for kind in v
            .character
            .items
            .iter()
            .skip(4)
            .chain(self.vault.iter().flatten())
            .filter(|k| **k >= 0)
            .copied()
            .collect::<BTreeSet<_>>()
        {
            let quantity = self.spare(v, c, future, kind)?;
            if quantity > 0 {
                goods.push(Goods { kind, quantity });
            }
        }
        Ok(goods)
    }
    fn reserved_bargain(&self) -> bool {
        self.bargain
            .as_ref()
            .is_some_and(|b| b.resolution.is_none())
    }
    pub(in crate::bots) fn reserves_stock(&self) -> bool {
        self.reserved_bargain()
            || self.incoming.is_some()
            || self.preparing
            || (self.session.is_some() && self.quote.is_some())
    }
    pub(crate) fn wants_speed(&self, v: &PlayerView) -> bool {
        self.quote.as_ref().is_some_and(|q| {
            let batch = if q[1].kind == SPEED {
                Some(q[1].quantity)
            } else if q[0].kind == SPEED && q[1].kind == DEFENSE {
                Some(q[0].quantity)
            } else {
                None
            };
            batch.is_some_and(|n| self.count(v, SPEED) < usize::from(n))
        })
    }
    fn gear_want(
        &self,
        v: &PlayerView,
        c: &Content,
        future: Option<u16>,
        minimum_tier: u8,
    ) -> Result<Option<i32>> {
        let mut wants = Vec::new();
        for item in c.objects.values().filter(|o| {
            o.class == "Equipment" && !o.consumable && o.tier.is_some_and(|t| t >= minimum_tier)
        }) {
            if self.wanted(v, c, future, i32::from(item.kind))? > 0 {
                wants.push(i32::from(item.kind));
            }
        }
        // Period markets span many upgrades, not the first XML item: https://realmpriceguid.blogspot.com/
        Ok((!wants.is_empty()).then(|| wants[style(&self.name) as usize % wants.len()]))
    }
    fn quote(
        &self,
        v: &PlayerView,
        c: &Content,
        future: Option<u16>,
    ) -> Result<Option<[Goods; 2]>> {
        let price = 3 + style(&self.name) % 3;
        let life_price = 5 + (style(&self.name) / 3) % 3;
        let pair = |give, n, want, m| {
            [
                Goods {
                    kind: give,
                    quantity: n,
                },
                Goods {
                    kind: want,
                    quantity: m,
                },
            ]
        };
        let stock = self.stock(v, c, future)?;
        let maxing_speed = self.quote.as_ref().is_some_and(|q| q[1].kind == SPEED)
            && v.character.base_stats[4] < c.classes[&v.character.class].caps[4];
        if maxing_speed {
            if v.character.items.iter().skip(4).any(|k| *k == SPEED) {
                return Ok(None);
            }
            return Ok((self.count(v, DEFENSE) > 0
                && self.wanted(v, c, future, SPEED)? >= price as u8)
                .then(|| pair(DEFENSE, 1, SPEED, price as u8)));
        }
        if self.count(v, DEFENSE) >= life_price as usize && self.wanted(v, c, future, LIFE)? > 0 {
            return Ok(Some(pair(DEFENSE, life_price as u8, LIFE, 1)));
        }
        if self.count(v, SPEED) >= price as usize && self.wanted(v, c, future, DEFENSE)? > 0 {
            return Ok(Some(pair(SPEED, price as u8, DEFENSE, 1)));
        }
        let trading_up =
            self.quote.as_ref().is_some_and(|q| q[0].kind == SPEED) && self.wants_speed(v);
        if self.count(v, DEFENSE) > 0
            && !trading_up
            && self.wanted(v, c, future, SPEED)? >= price as u8
        {
            return Ok(Some(pair(DEFENSE, 1, SPEED, price as u8)));
        }
        // Life savings must not hide already-funded potion exchanges.
        if self.count(v, LIFE) > 0
            && let Some(kind) = self.gear_want(v, c, future, LIFE_GEAR_TIER)?
        {
            return Ok(Some(pair(LIFE, 1, kind, 1)));
        }
        let mut sales = Vec::new();
        for g in stock.into_iter().filter(|g| {
            !matches!(g.kind, SPEED | DEFENSE | LIFE)
                && c.objects[&(g.kind as u16)].tier.is_some_and(|t| t >= 3)
                && !c.objects[&(g.kind as u16)].consumable
        }) {
            let tier = c
                .get(g.kind as u16)?
                .tier
                .context("trade gear without tier")?;
            let payment = if tier >= LIFE_GEAR_TIER {
                LIFE
            } else {
                DEFENSE
            };
            if self.wanted(v, c, future, payment)? == 0 {
                continue;
            }
            sales.push(pair(
                g.kind,
                1,
                payment,
                if tier >= LIFE_GEAR_TIER {
                    1
                } else {
                    (1 + style(&self.name) % 2) as u8
                },
            ));
        }
        Ok((!sales.is_empty()).then(|| sales[style(&self.name) as usize % sales.len()].clone()))
    }
    pub(in crate::bots) fn message_current(
        &self,
        message: &Message,
        v: &PlayerView,
        c: &Content,
        future: Option<u16>,
    ) -> Result<bool> {
        if let Some(current) = self.response_current(message, v, c, future)? {
            return Ok(current);
        }
        if let Some(q) = parse_quote(&message.text, c) {
            Ok(v.map.name == "Nexus"
                && self.incoming.is_none()
                && self.trade.is_none()
                && super::settled(v)
                && v.character
                    .items
                    .iter()
                    .skip(4)
                    .filter(|k| **k == q[0].kind)
                    .count()
                    >= usize::from(q[0].quantity)
                && self
                    .session
                    .as_ref()
                    .is_some_and(|s| v.elapsed_ms < s.until)
                && self.quote(v, c, future)?.as_ref() == Some(&q))
        } else {
            Ok(v.map.name == "Nexus"
                && super::settled(v)
                && self
                    .quote(v, c, future)?
                    .as_ref()
                    .map(|q| goods_text(&q[1], c))
                    .transpose()?
                    .is_some_and(|want| message.text == format!("nty need {want}")))
        }
    }
    fn queue(&mut self, text: String, channel: Channel) {
        self.outgoing = Some(Message { text, channel });
    }
    pub(in crate::bots) fn finish(&mut self, reason: &str) {
        self.outgoing = None;
        self.quote = None;
        self.preparing = false;
        if let Some(b) = &mut self.bargain
            && b.resolution.is_none()
        {
            b.resolution = Some(reason.into());
            b.own_consent = false;
            b.peer_consent = None;
            self.revoke_trade = reason != "settled_all_installments_transaction_identified";
            tracing::info!(terms=%serde_json::to_string(&b).expect("bargain serialization"),reason,"market remaining balance resolved explicitly");
        }
        self.trade = None;
    }

    pub(in crate::bots) fn receive(
        &mut self,
        packet: &Packet,
        v: &PlayerView,
        c: &Content,
        future: Option<u16>,
    ) -> Result<bool> {
        self.receive_observed(packet, v, &v.objects, c, future)
    }
    pub(in crate::bots) fn receive_observed(
        &mut self,
        packet: &Packet,
        v: &PlayerView,
        observed_objects: &[crate::world::view::VisibleObject],
        c: &Content,
        future: Option<u16>,
    ) -> Result<bool> {
        if packet.id != wire::TEXT {
            return self.trade_packet(packet, v);
        }
        let mut r = Reader::new(&packet.body);
        let name = r.utf()?;
        let speaker = r.i32()?;
        r.i32()?;
        r.u8()?;
        let recipient = r.utf()?;
        let text = r.utf()?;
        r.utf()?;
        r.done()?;
        if speaker == v.session.player {
            return Ok(false);
        }
        let channel = if recipient.is_empty() {
            Channel::Local
        } else if recipient == "*Guild*" {
            Channel::Guild
        } else if recipient == self.name.as_str() {
            Channel::Tell(name.clone())
        } else {
            return Ok(false);
        };
        if speaker < 0 && matches!(channel, Channel::Local) {
            return Ok(false);
        }
        let peer = observed_objects.iter().find(|p| {
            p.hp > 0
                && !p.equipped.is_empty()
                && p.name == name
                && (speaker == -1 || p.id == speaker)
        });
        let Some(peer) = peer else {
            return Ok(false);
        };
        let body = text
            .strip_prefix(&format!("{}, ", self.name))
            .unwrap_or(&text);
        let ad=if let Some(counter)=counter_body(&body.trim().to_ascii_lowercase()) {
            parse_advertisement(counter,|s| parse_goods(s,c)).filter(|ad| matches!(ad,Advertisement::Quote(_)))
        } else {
            parse_advertisement(body,|s| parse_goods(s,c))
        };
        let Some(ad)=ad else { return Ok(false); };
        if v.map.name != "Nexus"
            || self.incoming.is_some()
            || self.trade.is_some()
            || self
                .bargain
                .as_ref()
                .is_some_and(|b| b.resolution.is_none())
        {
            return Ok(true);
        }
        let Some(objective) = self.quote(v, c, future)? else {
            return Ok(true);
        };
        let related = match &ad {
            Advertisement::Quote(q) => {
                objective[0].kind == q[1].kind && objective[1].kind == q[0].kind
            }
            Advertisement::Buying(g) => g.kind == objective[0].kind,
            Advertisement::Selling(g) => g.kind == objective[1].kind,
        };
        // Interest alone supplies neither a peer price nor permission to open a trade.
        let Advertisement::Quote(q) = ad else {
            if related
                && self.outgoing.is_none()
                && v.elapsed_ms >= self.next_message_ms
                && let Some(account) = peer.account
                && v.character
                    .items
                    .iter()
                    .skip(4)
                    .filter(|k| **k == objective[0].kind)
                    .count()
                    >= usize::from(objective[0].quantity)
            {
                self.respond(
                    v,
                    (peer.id, account, name),
                    ReplyKind::Counter(objective),
                    c,
                )?;
            }
            return Ok(true);
        };
        // Ads are invitations to inspect a WINDOW, never evidence of consent or payment.
        if objective[0].kind == q[1].kind
            && objective[1].kind == q[0].kind
            && q[1].quantity <= objective[0].quantity
            && q[0].quantity >= objective[1].quantity
            && self.wanted(v, c, future, q[0].kind)? >= q[0].quantity
            && self.spare(v, c, future, q[1].kind)? >= q[1].quantity
            && fair(&q, c)
            && let Some(account) = peer.account
        {
            self.quote = Some(objective);
            self.incoming = Some(Incoming::outbound(
                v,
                peer.id,
                account,
                name,
                [q[1].clone(), q[0].clone()],
            ));
        } else if related
            && self.outgoing.is_none()
            && v.elapsed_ms >= self.next_message_ms
            && let Some(account) = peer.account
            && v.character
                .items
                .iter()
                .skip(4)
                .filter(|k| **k == objective[0].kind)
                .count()
                >= usize::from(objective[0].quantity)
        {
            self.respond(
                v,
                (peer.id, account, name),
                ReplyKind::Counter(objective),
                c,
            )?;
        }
        let _ = channel;
        Ok(true)
    }
    pub(in crate::bots) fn recover(&mut self, receipts: Vec<Delivery>) -> Result<()> {
        let Some(b) = &mut self.bargain else {
            return Ok(());
        };
        for (ordinal, r) in receipts.iter().enumerate() {
            ensure!(
                r.terms == b.terms
                    && r.ordinal as usize == ordinal
                    && *r == b.terms.delivery(r.ordinal)?,
                "stale, duplicate or foreign settlement receipt"
            );
        }
        ensure!(
            receipts.len() >= b.confirmed as usize,
            "committed market receipts regressed"
        );
        if b.confirmed as usize != receipts.len() {
            self.trade = None;
        }
        b.confirmed = u8::try_from(receipts.len())?;
        if b.terms.delivery(b.confirmed).is_err()
            && b.resolution.as_deref() != Some("settled_all_installments_transaction_identified")
        {
            // A death before the mind save cannot undo a committed final installment.
            b.resolution = None;
            self.finish("settled_all_installments_transaction_identified");
        }
        Ok(())
    }
    fn end_session(&mut self, v: &PlayerView, reason: &str) {
        if reason != "left_nexus" {
            self.preparing = false;
        }
        if let Some(s) = self.session.take() {
            tracing::info!(account=v.session.account,elapsed_ms=v.elapsed_ms,anchor=?s.anchor,until=s.until,reason,"market intentional session ended");
        }
        if self
            .outgoing
            .as_ref()
            .is_some_and(|m| m.text.starts_with("wts "))
        {
            self.outgoing = None;
        }
    }
    pub(in crate::bots) fn decide(
        &mut self,
        v: &PlayerView,
        c: &Content,
        future: Option<u16>,
        grid: &Grid,
        rng: &mut impl Rng,
    ) -> Result<Option<Decision>> {
        self.observe(v);
        if let Some(q) = &self.quote
            && self.wanted(v, c, future, q[1].kind)? == 0
            && !(q[1].kind == SPEED
                && v.character.base_stats[4] < c.classes[&v.character.class].caps[4])
        {
            self.quote = None;
            self.preparing = false;
        }
        if !super::settled(v) {
            return Ok(None);
        }
        if let Some(b) = self.bargain.clone().filter(|b| b.resolution.is_none()) {
            let side = b
                .terms
                .accounts
                .iter()
                .position(|a| *a == v.session.account)
                .context("foreign bargain")?;
            let peer = 1 - side;
            let delivery = b.terms.delivery(b.confirmed)?;
            let remaining = b.terms.goods[side]
                .quantity
                .saturating_sub(b.confirmed * b.terms.batch);
            let failure = if now() >= b.terms.expires {
                Some("expired_remaining_balance_unenforced")
            } else if self.count(v, b.terms.goods[side].kind) < remaining as usize {
                Some("promised_stock_changed_remaining_balance_unenforced")
            } else {
                None
            };
            if let Some(reason) = failure {
                self.finish(reason);
                return Ok(None);
            }
            let mut d = Decision {
                goal: Goal::Wait,
                movement: None,
                aim: None,
                item: None,
                journey: None,
                chat: None,
            };
            if v.map.name != "Nexus" && v.map.name != "Vault" {
                d.journey = Some(Journey::Nexus);
                return Ok(Some(d));
            }
            if !b.agreed && b.confirmed == 0 {
                return Ok(Some(d));
            }
            let own = delivery.quantities[side] as usize;
            let needed = delivery.quantities[peer] as usize;
            let carried = v
                .character
                .items
                .iter()
                .skip(4)
                .filter(|k| **k == b.terms.goods[side].kind)
                .count();
            let free = v.character.items.iter().skip(4).filter(|k| **k < 0).count();
            let bank = carried < own || free + own < needed;
            if v.map.name == "Vault" {
                let Some(chest) = v.objects.iter().find(|o| o.kind == 0x504) else {
                    return Ok(Some(d));
                };
                let transfer = if carried < own
                    && let Some(from) = chest
                        .contents
                        .iter()
                        .position(|k| *k == b.terms.goods[side].kind)
                    && let Some(to) = v
                        .character
                        .items
                        .iter()
                        .enumerate()
                        .skip(4)
                        .find(|(_, k)| **k < 0)
                        .map(|(i, _)| i)
                {
                    Some(Action::Swap {
                        from: Slot {
                            object: chest.id,
                            slot: from,
                            kind: chest.contents[from],
                        },
                        to: super::own(v, to),
                    })
                } else if bank
                    && let Some(to) = chest.contents.iter().position(|k| *k < 0)
                    && let Some(from) = v
                        .character
                        .items
                        .iter()
                        .enumerate()
                        .skip(4)
                        .find(|(_, k)| **k >= 0 && **k != b.terms.goods[side].kind)
                        .map(|(i, _)| i)
                {
                    Some(Action::Swap {
                        from: super::own(v, from),
                        to: Slot {
                            object: chest.id,
                            slot: to,
                            kind: -1,
                        },
                    })
                } else {
                    None
                };
                if let Some(action) = transfer {
                    d.goal = Goal::Bank(chest.id);
                    if v.position.distance(chest.position) < 1.4 {
                        d.item = Some(action)
                    } else {
                        d.movement = grid.towards(v.position, chest.position)
                    }
                } else if bank {
                    self.finish("vault_capacity_or_stock_unavailable_remaining_balance_unenforced");
                } else {
                    d.journey = Some(Journey::Nexus)
                }
                return Ok(Some(d));
            }
            if bank {
                if let Some(p) = v.objects.iter().find(|p| p.kind == 0x720 && p.usable) {
                    d.goal = Goal::Portal(p.id);
                    if v.position.distance(p.position) < 1.4 {
                        d.journey = Some(Journey::Portal(p.id))
                    } else {
                        d.movement = grid.towards(v.position, p.position)
                    }
                }
                return Ok(Some(d));
            }
            let meeting = Pos {
                x: b.terms.meeting[1] as f32 + 0.5,
                y: b.terms.meeting[2] as f32 + 0.5,
            };
            if v.position.distance(meeting) >= 1. {
                d.movement = grid.towards(v.position, meeting);
                return Ok(Some(d));
            }
            return Ok(Some(d));
        }
        if let Some(g) = &self.incoming {
            let mut d = Decision {
                goal: Goal::Wait,
                movement: None,
                aim: None,
                item: None,
                journey: None,
                chat: None,
            };
            if let Some(p) = v
                .objects
                .iter()
                .find(|p| p.id == g.player && p.account == Some(g.account))
                && p.position.distance(v.position) >= 1.5
            {
                d.movement = grid.towards(v.position, p.position);
            }
            return Ok(Some(d));
        }
        if v.map.name == "Vault"
            && let Some(q) = self.quote.clone()
        {
            let mut d = Decision {
                goal: Goal::Wait,
                movement: None,
                aim: None,
                item: None,
                journey: None,
                chat: None,
            };
            let carried = v
                .character
                .items
                .iter()
                .skip(4)
                .filter(|k| **k == q[0].kind)
                .count();
            let free = v.character.items.iter().skip(4).filter(|k| **k < 0).count();
            let Some(chest) = v.objects.iter().find(|o| o.kind == 0x504) else {
                return Ok(Some(d));
            };
            let need_stock = carried < usize::from(q[0].quantity);
            let need_space = (need_stock && free == 0)
                || free + usize::from(q[0].quantity) < usize::from(q[1].quantity);
            let transfer = if need_space {
                chest
                    .contents
                    .iter()
                    .position(|k| *k < 0)
                    .zip(
                        v.character
                            .items
                            .iter()
                            .enumerate()
                            .skip(4)
                            .find(|(_, k)| {
                                **k >= 0
                                    && (**k != q[0].kind || carried > usize::from(q[0].quantity))
                            })
                            .map(|(i, _)| i),
                    )
                    .map(|(to, from)| Action::Swap {
                        from: super::own(v, from),
                        to: Slot {
                            object: chest.id,
                            slot: to,
                            kind: -1,
                        },
                    })
            } else if need_stock {
                chest
                    .contents
                    .iter()
                    .position(|k| *k == q[0].kind)
                    .zip(
                        v.character
                            .items
                            .iter()
                            .enumerate()
                            .skip(4)
                            .find(|(_, k)| **k < 0)
                            .map(|(i, _)| i),
                    )
                    .map(|(from, to)| Action::Swap {
                        from: Slot {
                            object: chest.id,
                            slot: from,
                            kind: q[0].kind,
                        },
                        to: super::own(v, to),
                    })
            } else {
                None
            };
            if let Some(action) = transfer {
                d.goal = Goal::Bank(chest.id);
                if v.position.distance(chest.position) < 1.4 {
                    d.item = Some(action);
                } else {
                    d.movement = grid.towards(v.position, chest.position);
                }
            } else if carried >= usize::from(q[0].quantity) && !need_space {
                d.journey = Some(Journey::Nexus);
            } else {
                self.quote = None;
                self.preparing = false;
                tracing::info!(
                    account = v.session.account,
                    "market preparation unavailable: own vault stock or capacity changed"
                );
                return Ok(None);
            }
            return Ok(Some(d));
        }
        if v.map.name != "Nexus" {
            return Ok(None);
        }
        let mut d = Decision {
            goal: Goal::Wait,
            movement: None,
            aim: None,
            item: None,
            journey: None,
            chat: None,
        };
        if self.trade.is_some() {
            return Ok(Some(d));
        }
        if self.session.is_none() && self.session_used {
            return Ok(None);
        }
        if self
            .session
            .as_ref()
            .is_some_and(|s| v.elapsed_ms >= s.until)
        {
            self.end_session(v, "elapsed");
            return Ok(None);
        }
        let Some(quote) = self.quote(v, c, future)? else {
            self.end_session(v, "no_real_exchange");
            return Ok(None);
        };
        self.quote = Some(quote.clone());
        if self.session.is_none() {
            self.session_used = true;
            let admitted = self.preparing || self.trader || rng.gen_ratio(1, 5);
            tracing::info!(
                account = v.session.account,
                admitted,
                probability = if self.trader { 1.0 } else { 0.2 },
                reason = if admitted {
                    "visit_admitted"
                } else {
                    "visit_declined"
                },
                "market autonomous session admission"
            );
            if !admitted {
                return Ok(None);
            }
            let duration_ms = rng.gen_range(60..=240) * 1000u64;
            self.session = Some(MarketSession {
                anchor: grid
                    .walkable
                    .terrain
                    .points()
                    .filter(|p| {
                        grid.walkable.contains(p)
                            && center(*p).distance(v.position) <= 7.
                            && grid.segment(v.position, center(*p))
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .choose(rng)
                    .context("market has no reachable Nexus trading position")?,
                until: v
                    .elapsed_ms
                    .checked_add(duration_ms)
                    .context("market session deadline overflow")?,
                next_move: v.elapsed_ms + 10_000,
                destination: None,
            });
            tracing::info!(account=v.session.account,elapsed_ms=v.elapsed_ms,duration_ms,session=?self.session,?quote,"market intentional session started");
        }
        let carried = v
            .character
            .items
            .iter()
            .skip(4)
            .filter(|k| **k == quote[0].kind)
            .count();
        let free = v.character.items.iter().skip(4).filter(|k| **k < 0).count();
        if carried < usize::from(quote[0].quantity)
            || free + usize::from(quote[0].quantity) < usize::from(quote[1].quantity)
        {
            self.preparing = true;
            if let Some(portal) = v.objects.iter().find(|p| p.kind == 0x720 && p.usable) {
                d.goal = Goal::Portal(portal.id);
                if v.position.distance(portal.position) < 1.4 {
                    d.journey = Some(Journey::Portal(portal.id));
                } else {
                    d.movement = grid.towards(v.position, portal.position);
                }
                return Ok(Some(d));
            }
            self.end_session(v, "vault_unobserved");
            tracing::info!(
                account = v.session.account,
                "market preparation unavailable: no observed Vault portal"
            );
            return Ok(None);
        }
        self.preparing = false;
        let s = self
            .session
            .as_mut()
            .expect("market session just initialized");
        if v.position.distance(center(s.anchor)) > 1.5 {
            d.movement = grid.towards(v.position, center(s.anchor));
            return Ok(Some(d));
        }
        if v.elapsed_ms >= s.next_move {
            s.next_move = v.elapsed_ms + 10_000;
            let (x, y) = tile(v.position);
            // The half-tile margin leaves room for ordinary controller reaction steps.
            s.destination = [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
                .into_iter()
                .filter(|p| {
                    center(*p).distance(center(s.anchor)) <= 1.5
                        && grid.segment(v.position, center(*p))
                })
                .choose(rng);
            tracing::info!(account=v.session.account,anchor=?s.anchor,target=?s.destination,"market intentional session local step");
        }
        d.movement = s
            .destination
            .map(center)
            .filter(|p| v.position.distance(*p) > 0.2 && grid.segment(v.position, *p));
        if v.elapsed_ms >= self.next_ad && self.outgoing.is_none() {
            let ad = format!(
                "wts {} for {}",
                goods_text(&quote[0], c)?,
                goods_text(&quote[1], c)?
            );
            ensure!(
                parse_quote(&ad, c).as_ref() == Some(&quote),
                "unresolvable market quote: {ad}"
            );
            self.queue(ad, Channel::Local);
            self.next_ad = v.elapsed_ms + 6_000 + u64::from(style(&self.name) % 7) * 1_000;
        }
        Ok(Some(d))
    }
    fn trade_packet(&mut self, p: &Packet, v: &PlayerView) -> Result<bool> {
        let mut r = Reader::new(&p.body);
        match p.id {
            61 => {
                let name = r.utf()?;
                r.done()?;
                self.incoming_request(name, v);
            }
            67 => {
                let read_items = |r: &mut Reader<'_>| -> Result<(Vec<i32>, [bool; 12])> {
                    ensure!(r.i16()? == 12, "market trade start shape");
                    let mut items = Vec::new();
                    let mut tradeable = [false; 12];
                    for t in &mut tradeable {
                        items.push(r.i32()?);
                        r.i32()?;
                        *t = r.u8()? == 1;
                        r.u8()?;
                    }
                    Ok((items, tradeable))
                };
                let (mine, tradeable) = read_items(&mut r)?;
                let peer = r.utf()?;
                let (theirs, _) = read_items(&mut r)?;
                r.done()?;
                self.pending_trade = None;
                self.trade = Some(SessionTrade {
                    binding: [v.session.world, v.session.player, v.session.character],
                    peer,
                    items: [mine, theirs],
                    tradeable,
                    offers: [[false; 12]; 2],
                    ready: v.time_ms + 3000,
                    ready_at: v.clock_at + Duration::from_millis(3000),
                    empty_offer_until: v.clock_at + Duration::from_secs(20),
                    changed: false,
                });
            }
            23 => {
                let values = read_offer(&mut r)?;
                r.done()?;
                if let Some(t) = &mut self.trade {
                    if t.offers[1] != values {
                        self.pending_trade = None;
                        t.empty_offer_until = v.clock_at + Duration::from_secs(20);
                    }
                    t.offers[1] = values;
                    t.ready = v.time_ms + 3000;
                    t.ready_at = v.clock_at + Duration::from_millis(3000);
                }
            }
            18 => {
                read_offer(&mut r)?;
                read_offer(&mut r)?;
                r.done()?;
            }
            12 => {
                let result = r.i32()?;
                let reason = r.utf()?;
                r.done()?;
                self.trade = None;
                if result != 0 {
                    self.pending_trade = None;
                }
                if self.incoming.is_some() {
                    self.end_incoming(v);
                }
                tracing::info!(
                    account = v.session.account,
                    result,
                    reason,
                    "market native trade outcome (not a settlement receipt)"
                );
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
pub(in crate::bots) const SPEED: i32 = 0xa21;
const DEFENSE: i32 = 0xa20;
const LIFE: i32 = 0xae9;
const LIFE_GEAR_TIER: u8 = 10;
fn style(name: &str) -> u32 {
    name.bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
}
fn fair(q: &[Goods; 2], c: &Content) -> bool {
    match (q[0].kind, q[1].kind) {
        (SPEED, DEFENSE) => (3..=5).contains(&q[0].quantity) && q[1].quantity == 1,
        (DEFENSE, SPEED) => q[0].quantity == 1 && (3..=5).contains(&q[1].quantity),
        (DEFENSE, LIFE) => (5..=7).contains(&q[0].quantity) && q[1].quantity == 1,
        (LIFE, DEFENSE) => q[0].quantity == 1 && (5..=7).contains(&q[1].quantity),
        _ => [0, 1].into_iter().any(|side| {
            let gear = &q[side];
            let pay = &q[1 - side];
            gear.quantity == 1
                && (1..=2).contains(&pay.quantity)
                && matches!(pay.kind, DEFENSE | LIFE)
                && c.objects
                    .get(&(gear.kind as u16))
                    .is_some_and(|o| !o.consumable && o.tier.is_some_and(|t| t >= 3))
        }),
    }
}
pub(in crate::bots) fn goods_text(g: &Goods, c: &Content) -> Result<String> {
    let name = match g.kind {
        SPEED => "spd",
        DEFENSE => "def",
        LIFE => "life",
        0xad3 => "dhide",
        0xaf9 => "hydra",
        _ => &c.get(u16::try_from(g.kind)?)?.name,
    };
    Ok(format!("{} {name}", g.quantity))
}
#[derive(Debug, PartialEq, Eq)]
pub(in crate::bots) enum Advertisement {
    Quote([Goods; 2]),
    Buying(Goods),
    Selling(Goods),
}

pub(in crate::bots) fn parse_goods(s: &str, c: &Content) -> Option<Goods> {
    let s = s.trim();
    let (quantity, name) = if let Some((n, rest)) = s.split_once(' ') {
        if let Ok(n) = n.parse::<u8>() {
            (n, rest)
        } else {
            (1, s)
        }
    } else {
        (1, s)
    };
    if !(1..=8).contains(&quantity) {
        return None;
    }
    let kind = match name {
        "spd" | "speed" => SPEED,
        "def" | "defense" => DEFENSE,
        "life" => LIFE,
        "dhide" => 0xad3,
        "hydra" => 0xaf9,
        _ => {
            let mut items = c
                .objects
                .values()
                .filter(|o| o.class == "Equipment" && o.name.eq_ignore_ascii_case(name));
            let kind = i32::from(items.next()?.kind);
            if items.next().is_some() {
                return None;
            }
            kind
        }
    };
    Some(Goods { kind, quantity })
}

pub(in crate::bots) fn counter_body(text: &str) -> Option<&str> {
    ["nty ", "nah ", "can do ", "no thx ", "sry "].into_iter().find_map(|p| text.strip_prefix(p))
}

// Templates and human chat share the same shapes; the caller supplies literal slots or real goods.
pub(in crate::bots) fn parse_advertisement(
    text: &str,
    parse: impl Fn(&str) -> Option<Goods>,
) -> Option<Advertisement> {
    let lower = text.trim().to_ascii_lowercase();
    let text = lower.strip_suffix('!').unwrap_or(&lower).trim_end();
    let text = if let Some((body, name)) = text.rsplit_once(" @") {
        if name != "{name}"
            && (name.is_empty()
                || name.len() > 16
                || !name.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return None;
        }
        body
    } else {
        text
    };
    if text.contains(['@', '!']) {
        return None;
    }
    let opening = [
        "wts ", "sell ", "selling ", "s>", "trading ", "trade ", "t>", "wtb ", "buy ", "buying ",
        "b>",
    ]
    .into_iter()
    .find_map(|prefix| {
        text.strip_prefix(prefix).map(|body| {
            (
                body.trim(),
                matches!(prefix, "wtb " | "buy " | "buying " | "b>"),
            )
        })
    });
    let (body, buying) = opening.unwrap_or((text, false));
    let explicit = body
        .split_once(" for ")
        .or_else(|| body.split_once('='))
        .or_else(|| body.strip_suffix(')').and_then(|s| s.split_once(" (")));
    let mut candidates = Vec::new();
    if let Some((first, second)) = explicit {
        candidates.push([parse(first.trim())?, parse(second.trim())?]);
    } else if opening.is_some() {
        for (i, _) in body.char_indices().filter(|(_, ch)| *ch == ' ') {
            if let (Some(first), Some(second)) =
                (parse(body[..i].trim()), parse(body[i + 1..].trim()))
            {
                candidates.push([first, second]);
            }
        }
        if candidates.is_empty() {
            let goods = parse(body)?;
            return Some(if buying {
                Advertisement::Buying(goods)
            } else {
                Advertisement::Selling(goods)
            });
        }
    }
    if candidates.len() != 1 {
        return None;
    }
    let mut q = candidates.pop()?;
    if q[0].kind == q[1].kind {
        return None;
    }
    if buying {
        q.swap(0, 1);
    }
    Some(Advertisement::Quote(q))
}

pub(in crate::bots) fn parse_quote(text: &str, c: &Content) -> Option<[Goods; 2]> {
    match parse_advertisement(text, |s| parse_goods(s, c))? {
        Advertisement::Quote(q) => Some(q),
        // Interest alone supplies neither a price nor consent to an exchange.
        Advertisement::Buying(_) | Advertisement::Selling(_) => None,
    }
}
pub(in crate::bots) fn read_offer(r: &mut Reader<'_>) -> Result<[bool; 12]> {
    ensure!(r.i16()? == 12, "market offer shape");
    let mut a = [false; 12];
    for b in &mut a {
        let n = r.u8()?;
        ensure!(n <= 1, "market offer boolean");
        *b = n == 1;
    }
    Ok(a)
}

#[cfg(test)]
mod interaction_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn market_current_message_cancels_after_stock_context_changes() {
        let (dir, app) = crate::server::test_app();
        app.world.clock.set_paused(true);
        let (owner, id, _packets, _brain) = crate::bots::runtime::tests::actor(
            &app,
            "market-current-747",
            "Trader",
            [SPEED, SPEED, SPEED, SPEED, SPEED, -1, -1, -1],
        )
        .await;
        let mut v = app.world.player_view(id).await.unwrap();
        let mut market = Market {
            name: "Trader".into(),
            ..Market::default()
        };
        market.session = Some(MarketSession {
            anchor: tile(v.position),
            until: v.elapsed_ms + 60000,
            next_move: 0,
            destination: None,
        });
        let q = market.quote(&v, &app.content, None).unwrap().unwrap();
        let message = Message {
            text: format!(
                "wts {} for {}",
                goods_text(&q[0], &app.content).unwrap(),
                goods_text(&q[1], &app.content).unwrap()
            ),
            channel: Channel::Local,
        };
        assert!(
            market
                .message_current(&message, &v, &app.content, None)
                .unwrap()
        );
        v.character.items[4..].fill(-1);
        assert!(
            !market
                .message_current(&message, &v, &app.content, None)
                .unwrap()
        );
        let refusal = Message {
            text: format!("nty need {}", goods_text(&q[1], &app.content).unwrap()),
            channel: Channel::Local,
        };
        assert!(
            !market
                .message_current(&refusal, &v, &app.content, None)
                .unwrap()
        );
        drop((owner, app));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
