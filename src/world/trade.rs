use super::*;

pub(super) struct Trade {
    players: [i32; 2],
    items: [Vec<i32>; 2],
    offers: [[bool; 12]; 2],
    accepted: [bool; 2],
    ready: u64,
    pending: bool,
}
pub struct TradePlan {
    key: i32,
    pub plans: [InventoryPlan; 2],
}
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum TradeAction {
    Request {
        name: String,
    },
    Change {
        offer: [bool; 12],
    },
    Accept {
        mine: [bool; 12],
        theirs: [bool; 12],
    },
    Cancel,
}
fn offer(out: &mut Writer, values: &[bool; 12]) {
    out.i16(12);
    for value in values {
        out.u8(u8::from(*value));
    }
}
impl World {
    pub fn script_trade(&mut self, id: i32, action: TradeAction) -> Result<Option<TradePlan>> {
        ensure!(
            self.players
                .get(&id)
                .context("script trader missing")?
                .controller
                == Controller::Qa,
            "trade control requires a script-owned player"
        );
        if let Some(key) = self.trade_key(id) {
            ensure!(
                self.trades[&key]
                    .players
                    .iter()
                    .all(|id| self.players[id].controller == Controller::Qa),
                "trade control cannot change a browser player's trade"
            );
        }
        match action {
            TradeAction::Request { name } => {
                ensure!(
                    self.players
                        .values()
                        .any(|p| p.character.name == name && p.controller == Controller::Qa),
                    "script trade target must be script-owned"
                );
                self.request_trade(id, name)?;
            }
            TradeAction::Change { offer } => self.change_trade(id, offer)?,
            TradeAction::Accept { mine, theirs } => return self.accept_trade(id, mine, theirs),
            TradeAction::Cancel => {
                self.cancel_trade(id, "Trade cancelled.");
            }
        }
        Ok(None)
    }
    fn trade_key(&self, id: i32) -> Option<i32> {
        self.trades
            .iter()
            .find(|(_, trade)| trade.players.contains(&id))
            .map(|(key, _)| *key)
    }
    fn trade_ready(&self, id: i32) -> Result<()> {
        let p = self.players.get(&id).context("trade player missing")?;
        ensure!(
            p.active() && !p.pending,
            "trade player busy, dead, departing or quarantined"
        );
        Ok(())
    }
    fn tradeable(&self, id: i32, slot: usize) -> Result<bool> {
        let item = self.players[&id].character.items[slot];
        Ok(slot >= 4 && item >= 0 && !self.content.get(u16::try_from(item)?)?.soulbound)
    }
    fn trade_items(&self, out: &mut Writer, id: i32) -> Result<()> {
        let p = &self.players[&id];
        let slots = &self.content.get(p.character.class)?.slots;
        ensure!(
            slots.len() == 12 && p.character.items.len() == 12,
            "invalid trade inventory shape"
        );
        out.i16(12);
        for (slot, item) in p.character.items.iter().enumerate() {
            out.i32(*item)
                .i32(slots[slot])
                .u8(u8::from(self.tradeable(id, slot)?))
                .u8(0);
        }
        Ok(())
    }
    pub fn request_trade(&mut self, id: i32, name: String) -> Result<()> {
        ensure!(
            !name.is_empty() && name.len() <= 16,
            "invalid trade target name"
        );
        self.trade_ready(id)?;
        ensure!(self.trade_key(id).is_none(), "already trading");
        let targets: Vec<_> = self
            .players
            .iter()
            .filter(|(_, p)| p.character.name == name)
            .map(|(id, _)| *id)
            .collect();
        ensure!(
            targets.len() == 1 && targets[0] != id,
            "trade target missing, ambiguous or self"
        );
        let target = targets[0];
        self.trade_ready(target)?;
        ensure!(self.trade_key(target).is_none(), "target already trading");
        ensure!(
            self.players[&id].pos.distance(self.players[&target].pos) < 2.,
            "trade target out of range"
        );
        if self
            .trade_requests
            .get(&target)
            .is_some_and(|(to, expires)| *to == id && *expires > self.now)
        {
            let players = [id, target];
            let mut starts = Vec::new();
            for (mine, theirs) in [(id, target), (target, id)] {
                let mut out = Writer::new();
                self.trade_items(&mut out, mine)?;
                out.utf(&self.players[&theirs].character.name);
                self.trade_items(&mut out, theirs)?;
                starts.push((mine, out.0));
            }
            self.trade_requests
                .retain(|from, (to, _)| !players.contains(from) && !players.contains(to));
            self.trades.insert(
                id,
                Trade {
                    players,
                    items: players.map(|id| self.players[&id].character.items.clone()),
                    offers: [[false; 12]; 2],
                    accepted: [false; 2],
                    ready: self.now + 3000,
                    pending: false,
                },
            );
            for (mine, body) in starts {
                send(&self.players[&mine], 67, body);
            }
            tracing::info!(player = id, partner = target, "native trade started");
        } else {
            self.trade_requests.insert(id, (target, self.now + 20_000));
            let mut out = Writer::new();
            out.utf(&self.players[&id].character.name);
            send(&self.players[&target], 61, out.0);
            tracing::info!(
                player = id,
                partner = target,
                "reciprocal trade request sent"
            );
        }
        Ok(())
    }
    pub fn cancel_trade(&mut self, id: i32, reason: &str) -> bool {
        self.trade_requests
            .retain(|from, (to, _)| *from != id && *to != id);
        let Some(key) = self.trade_key(id) else {
            return false;
        };
        if self.trades[&key].pending {
            tracing::info!(player=id,%reason,"trade cancellation deferred until atomic commit finishes");
            return false;
        }
        let trade = self.trades.remove(&key).unwrap();
        for player in trade.players {
            let mut out = Writer::new();
            out.i32(1).utf(reason);
            send(&self.players[&player], 12, out.0);
        }
        tracing::info!(players=?trade.players,%reason,"trade cancelled without inventory mutation");
        true
    }
    pub fn change_trade(&mut self, id: i32, values: [bool; 12]) -> Result<()> {
        let key = self.trade_key(id).context("no active trade")?;
        ensure!(!self.trades[&key].pending, "trade commit pending");
        self.trade_ready(id)?;
        let side = self.trades[&key]
            .players
            .iter()
            .position(|p| *p == id)
            .unwrap();
        for (slot, selected) in values.iter().enumerate() {
            ensure!(
                !selected || self.tradeable(id, slot)?,
                "empty, equipped or Soulbound trade offer"
            );
        }
        let trade = self.trades.get_mut(&key).unwrap();
        ensure!(
            self.players[&id].character.items == trade.items[side],
            "trade inventory changed"
        );
        trade.offers[side] = values;
        trade.accepted = [false; 2];
        trade.ready = self.now + 3000;
        let other = trade.players[1 - side];
        let mut out = Writer::new();
        offer(&mut out, &values);
        send(&self.players[&other], 23, out.0);
        Ok(())
    }
    pub fn accept_trade(
        &mut self,
        id: i32,
        mine: [bool; 12],
        theirs: [bool; 12],
    ) -> Result<Option<TradePlan>> {
        let key = self.trade_key(id).context("no active trade")?;
        let trade = &self.trades[&key];
        ensure!(
            !trade.pending && self.now >= trade.ready,
            "trade commit pending or offer settling"
        );
        let side = trade.players.iter().position(|p| *p == id).unwrap();
        ensure!(
            mine == trade.offers[side] && theirs == trade.offers[1 - side],
            "stale trade acceptance"
        );
        for (index, player) in trade.players.iter().enumerate() {
            self.trade_ready(*player)?;
            ensure!(
                self.players[player].character.items == trade.items[index],
                "trade inventory changed"
            );
        }
        ensure!(
            self.players[&trade.players[0]]
                .pos
                .distance(self.players[&trade.players[1]].pos)
                < 2.,
            "trade participants out of range"
        );
        let trade = self.trades.get_mut(&key).unwrap();
        trade.accepted[side] = true;
        let other = trade.players[1 - side];
        let mut out = Writer::new();
        offer(&mut out, &trade.offers[1 - side]);
        offer(&mut out, &trade.offers[side]);
        send(&self.players[&other], 18, out.0);
        if !trade.accepted.iter().all(|accepted| *accepted) {
            return Ok(None);
        }
        let players = trade.players;
        let offers = trade.offers;
        let originals = trade.items.clone();
        let mut inventories = originals.clone();
        for side in 0..2 {
            for slot in 4..12 {
                if offers[side][slot] {
                    inventories[side][slot] = -1;
                }
            }
        }
        for side in 0..2 {
            for slot in 4..12 {
                if offers[1 - side][slot] {
                    let destination = inventories[side][4..]
                        .iter()
                        .position(|item| *item == -1)
                        .context("trade recipient inventory is full")?
                        + 4;
                    inventories[side][destination] = originals[1 - side][slot];
                }
            }
        }
        let plans = [
            self.plan(players[0], inventories[0].clone(), None),
            self.plan(players[1], inventories[1].clone(), None),
        ];
        for player in players {
            self.players.get_mut(&player).unwrap().pending = true;
        }
        self.trades.get_mut(&key).unwrap().pending = true;
        Ok(Some(TradePlan { key, plans }))
    }
    pub fn finish_trade(&mut self, plan: TradePlan, result: Result<Vec<i32>>) -> Result<()> {
        let result = result.and_then(|revisions| {
            ensure!(
                revisions.len() == 2
                    && self
                        .trades
                        .get(&plan.key)
                        .context("reserved trade missing")?
                        .pending,
                "trade completion shape or state mismatch"
            );
            for (index, write) in plan.plans.iter().enumerate() {
                let p = self
                    .players
                    .get(&write.player)
                    .context("reserved trade participant missing")?;
                ensure!(
                    p.pending
                        && p.character.revision == write.revision
                        && p.character.id == write.character
                        && p.character.account == write.account
                        && write.revision.checked_add(1) == Some(revisions[index]),
                    "reserved trade owner or revision changed"
                );
            }
            Ok(revisions)
        });
        let revisions = match result {
            Ok(revisions) => revisions,
            Err(error) => {
                for write in &plan.plans {
                    if let Some(p) = self.players.get_mut(&write.player) {
                        p.quarantine(write.player, format!("{error:#}"));
                    }
                }
                return Err(error);
            }
        };
        for (index, write) in plan.plans.into_iter().enumerate() {
            let p = self.players.get_mut(&write.player).unwrap();
            p.character.items = write.items;
            p.character.revision = revisions[index];
            p.pending = false;
        }
        let trade = self.trades.remove(&plan.key).unwrap();
        for player in trade.players {
            let mut out = Writer::new();
            out.i32(0).utf("Trade successful.");
            send(&self.players[&player], 12, out.0);
        }
        tracing::info!(players=?trade.players,revisions=?revisions,"two-owner inventory exchange committed");
        Ok(())
    }
    pub(super) fn trade_step(&mut self) {
        self.trade_requests
            .retain(|_, (_, expires)| *expires > self.now);
        let interrupted: Vec<_> = self
            .trades
            .values()
            .filter(|t| {
                !t.pending
                    && (t.players.iter().any(|id| !self.players[id].active())
                        || self.players[&t.players[0]]
                            .pos
                            .distance(self.players[&t.players[1]].pos)
                            >= 2.)
            })
            .map(|t| t.players[0])
            .collect();
        for id in interrupted {
            self.cancel_trade(id, "Trade interrupted by departure, death or distance.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InventoryWrite, Store};
    use crate::wire::Reader;
    async fn fixture() -> (World, Store, [i32; 2], [mpsc::Receiver<Packet>; 2]) {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let store = Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let map =
            Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
        let mut w = World::new(content, map).unwrap();
        let mut players = Vec::new();
        let mut receivers = Vec::new();
        for guid in ["trade-a", "trade-b"] {
            let account = store.list(guid.into()).await.unwrap();
            let character = store
                .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            let (packets, receiver) = mpsc::channel(64);
            let (lost, _) = watch::channel(false);
            players.push(w.enter(character, Some(Output { packets, lost })).unwrap());
            receivers.push(receiver);
        }
        let ids = [players[0], players[1]];
        let plan = w
            .swap(
                ids[1],
                Slot {
                    object: ids[1],
                    slot: 0,
                    kind: 0xa97,
                },
                Slot {
                    object: ids[1],
                    slot: 5,
                    kind: -1,
                },
            )
            .unwrap();
        let revision = store
            .exchange(
                vec![InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                }],
                None,
            )
            .await
            .unwrap()[0];
        w.finish_inventory(plan, Ok(revision)).unwrap();
        let mut receivers: [mpsc::Receiver<Packet>; 2] = receivers.try_into().ok().unwrap();
        for r in &mut receivers {
            while r.try_recv().is_ok() {}
        }
        (w, store, ids, receivers)
    }
    fn start(w: &mut World, players: [i32; 2]) {
        w.request_trade(players[0], w.players[&players[1]].character.name.clone())
            .unwrap();
        w.request_trade(players[1], w.players[&players[0]].character.name.clone())
            .unwrap();
    }
    fn offers(w: &mut World, players: [i32; 2]) -> [[bool; 12]; 2] {
        let mut offers = [[false; 12]; 2];
        offers[0][4] = true;
        offers[1][5] = true;
        w.change_trade(players[0], offers[0]).unwrap();
        w.change_trade(players[1], offers[1]).unwrap();
        w.now += 3000;
        offers
    }
    fn writes(plan: &TradePlan) -> Vec<InventoryWrite> {
        plan.plans
            .iter()
            .map(|p| InventoryWrite {
                id: p.character,
                account: p.account,
                revision: p.revision,
                items: p.items.clone(),
            })
            .collect()
    }
    fn read_offer(r: &mut Reader<'_>) -> [bool; 12] {
        assert_eq!(r.i16().unwrap(), 12);
        std::array::from_fn(|_| r.u8().unwrap() == 1)
    }
    #[tokio::test]
    async fn trade_request_expiry_and_script_scope_do_not_control_browser_players() {
        let (mut w, _, ids, _) = fixture().await;
        let name = w.players[&ids[1]].character.name.clone();
        assert!(
            w.script_trade(ids[0], TradeAction::Request { name: name.clone() })
                .is_err()
        );
        w.players.get_mut(&ids[0]).unwrap().controller = Controller::Qa;
        assert!(
            w.script_trade(ids[0], TradeAction::Request { name: name.clone() })
                .is_err()
        );
        w.request_trade(ids[0], name.clone()).unwrap();
        w.now += 20_000;
        w.trade_step();
        w.request_trade(ids[1], w.players[&ids[0]].character.name.clone())
            .unwrap();
        assert!(w.trades.is_empty());
        w.request_trade(ids[0], name).unwrap();
        assert!(
            w.script_trade(ids[0], TradeAction::Change { offer: [false; 12] })
                .is_err()
        );
        w.cancel_trade(ids[0], "test cancellation");
        assert!(w.trade_requests.is_empty());
    }
    #[tokio::test]
    async fn original_trade_frames_orientation_reset_and_durable_exchange() {
        let (mut w, store, ids, mut receivers) = fixture().await;
        start(&mut w, ids);
        let requested = receivers[1].try_recv().unwrap();
        assert_eq!(requested.id, 61);
        let mut r = Reader::new(&requested.body);
        assert_eq!(r.utf().unwrap(), "Guest1");
        r.done().unwrap();
        for (index, receiver) in receivers.iter_mut().enumerate() {
            let start = receiver.try_recv().unwrap();
            assert_eq!(start.id, 67);
            let mut r = Reader::new(&start.body);
            for side in [index, 1 - index] {
                assert_eq!(r.i16().unwrap(), 12);
                for slot in 0..12 {
                    assert_eq!(
                        r.i32().unwrap(),
                        w.players[&ids[side]].character.items[slot]
                    );
                    assert_eq!(
                        r.i32().unwrap(),
                        w.content.get(crate::content::WIZARD).unwrap().slots[slot]
                    );
                    assert_eq!(r.u8().unwrap() == 1, w.tradeable(ids[side], slot).unwrap());
                    assert_eq!(r.u8().unwrap(), 0);
                }
                if side == index {
                    assert_eq!(r.utf().unwrap(), w.players[&ids[1 - index]].character.name);
                }
            }
            r.done().unwrap();
        }
        let offers = offers(&mut w, ids);
        for receiver in &mut receivers {
            assert_eq!(receiver.try_recv().unwrap().id, 23);
        }
        assert!(
            w.accept_trade(ids[0], offers[0], offers[1])
                .unwrap()
                .is_none()
        );
        assert!(receivers[0].try_recv().is_err());
        let accepted = receivers[1].try_recv().unwrap();
        assert_eq!(accepted.id, 18);
        let mut r = Reader::new(&accepted.body);
        assert_eq!(read_offer(&mut r), offers[1]);
        assert_eq!(read_offer(&mut r), offers[0]);
        r.done().unwrap();
        w.change_trade(ids[1], [false; 12]).unwrap();
        let key = w.trade_key(ids[0]).unwrap();
        assert_eq!(w.trades[&key].accepted, [false; 2]);
        assert!(w.accept_trade(ids[0], offers[0], offers[1]).is_err());
        w.change_trade(ids[1], offers[1]).unwrap();
        w.now += 3000;
        assert!(
            w.accept_trade(ids[0], offers[0], offers[1])
                .unwrap()
                .is_none()
        );
        let plan = w
            .accept_trade(ids[1], offers[1], offers[0])
            .unwrap()
            .unwrap();
        assert!(ids.iter().all(|id| w.players[id].pending));
        assert_eq!(w.players[&ids[0]].character.items[4], 0xa22);
        let revisions = store.exchange(writes(&plan), None).await.unwrap();
        w.finish_trade(plan, Ok(revisions)).unwrap();
        assert!(w.trade_key(ids[0]).is_none());
        assert_eq!(w.players[&ids[0]].character.items[4], 0xa97);
        assert_eq!(w.players[&ids[1]].character.items[5], 0xa22);
        for id in ids {
            let ch = &w.players[&id].character;
            assert_eq!(store.load(ch.account, ch.id).await.unwrap().items, ch.items);
        }
    }
    #[tokio::test]
    async fn pending_exchange_waits_through_death_and_disconnect_without_losing_items() {
        let (mut w, store, ids, _) = fixture().await;
        start(&mut w, ids);
        let offers = offers(&mut w, ids);
        w.accept_trade(ids[0], offers[0], offers[1]).unwrap();
        let plan = w
            .accept_trade(ids[1], offers[1], offers[0])
            .unwrap()
            .unwrap();
        assert!(w.begin_departure(ids[0]).unwrap().is_none());
        let p = w.players.get_mut(&ids[1]).unwrap();
        p.character.hp = 0;
        p.death_pending = true;
        assert!(w.pending_deaths().is_empty());
        let revisions = store.exchange(writes(&plan), None).await.unwrap();
        w.finish_trade(plan, Ok(revisions)).unwrap();
        let departed = w.begin_departure(ids[0]).unwrap().unwrap();
        assert_eq!(departed.items[4], 0xa97);
        let deaths = w.pending_deaths();
        assert_eq!(deaths.len(), 1);
        assert_eq!(deaths[0].1.items[5], 0xa22);
        store
            .death(deaths[0].1.clone(), "test exchange death".into())
            .await
            .unwrap();
        w.finish_death(ids[1]).unwrap();
        assert!(
            store
                .load(deaths[0].1.account, deaths[0].1.id)
                .await
                .is_err()
        );
        assert_eq!(
            store
                .load(departed.account, departed.id)
                .await
                .unwrap()
                .items[4],
            0xa97
        );
    }
    #[tokio::test]
    async fn stale_two_owner_commit_rolls_back_and_quarantines_both_participants() {
        let (mut w, store, ids, _) = fixture().await;
        start(&mut w, ids);
        let offers = offers(&mut w, ids);
        w.accept_trade(ids[0], offers[0], offers[1]).unwrap();
        let plan = w
            .accept_trade(ids[1], offers[1], offers[0])
            .unwrap()
            .unwrap();
        let second = &plan.plans[1];
        store
            .exchange(
                vec![InventoryWrite {
                    id: second.character,
                    account: second.account,
                    revision: second.revision,
                    items: w.players[&second.player].character.items.clone(),
                }],
                None,
            )
            .await
            .unwrap();
        let result = store.exchange(writes(&plan), None).await;
        assert!(result.is_err());
        assert!(w.finish_trade(plan, result).is_err());
        for id in ids {
            assert!(w.players[&id].pending);
            assert!(w.begin_departure(id).is_err());
            let ch = &w.players[&id].character;
            assert_eq!(store.load(ch.account, ch.id).await.unwrap().items, ch.items);
        }
    }
    #[tokio::test]
    async fn lost_trade_participant_does_not_prevent_quarantining_the_survivor() {
        let (mut w, store, ids, _) = fixture().await;
        start(&mut w, ids);
        let offers = offers(&mut w, ids);
        w.accept_trade(ids[0], offers[0], offers[1]).unwrap();
        let plan = w
            .accept_trade(ids[1], offers[1], offers[0])
            .unwrap()
            .unwrap();
        let revisions = store.exchange(writes(&plan), None).await.unwrap();
        let missing = plan.plans[0].player;
        let survivor = plan.plans[1].player;
        w.players.remove(&missing);
        assert!(w.finish_trade(plan, Ok(revisions)).is_err());
        assert!(w.players[&survivor].persistence_failure.is_some());
        assert!(w.begin_departure(survivor).is_err());
    }
    #[tokio::test]
    async fn empty_equipped_soulbound_full_and_lifecycle_offers_cannot_mutate_inventory() {
        let (mut w, _, ids, _) = fixture().await;
        let soulbound = w
            .content
            .objects
            .values()
            .find(|o| o.class == "Equipment" && o.soulbound)
            .unwrap()
            .kind;
        w.players.get_mut(&ids[0]).unwrap().character.items[6] = i32::from(soulbound);
        start(&mut w, ids);
        for slot in [0, 7, 6] {
            let mut offer = [false; 12];
            offer[slot] = true;
            assert!(w.change_trade(ids[0], offer).is_err());
        }
        w.cancel_trade(ids[0], "test reset");
        for slot in 4..12 {
            w.players.get_mut(&ids[1]).unwrap().character.items[slot] = 0xa4f;
        }
        start(&mut w, ids);
        let mut offered = [false; 12];
        offered[4] = true;
        w.change_trade(ids[0], offered).unwrap();
        w.now += 3000;
        let before = ids.map(|id| w.players[&id].character.items.clone());
        w.accept_trade(ids[0], offered, [false; 12]).unwrap();
        assert!(w.accept_trade(ids[1], [false; 12], offered).is_err());
        w.cancel_trade(ids[1], "full inventory");
        assert_eq!(ids.map(|id| w.players[&id].character.items.clone()), before);
        start(&mut w, ids);
        w.players.get_mut(&ids[1]).unwrap().pos.x += 3.;
        w.trade_step();
        assert!(w.trades.is_empty());
        w.players.get_mut(&ids[1]).unwrap().pos = w.players[&ids[0]].pos;
        start(&mut w, ids);
        w.players.get_mut(&ids[1]).unwrap().death_pending = true;
        w.pending_deaths();
        assert!(w.trades.is_empty());
    }
}
