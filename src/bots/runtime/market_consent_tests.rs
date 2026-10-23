use super::*;
use crate::bots::pursuits::market::{Goods, Terms};
use crate::store::InventoryWrite;
use crate::world::TradePlan;
use std::{fs, io::Write, path::PathBuf};

struct Context {
    dir: PathBuf,
    app: App,
    brain: Brain,
    output: mpsc::Receiver<Packet>,
    _human_output: mpsc::Receiver<Packet>,
    _owner: crate::server::Owner,
    _human_owner: crate::server::Owner,
    ids: [i32; 2],
    terms: Terms,
}
impl Context {
    async fn new() -> Self {
        let (dir, app) = crate::server::fixture();
        app.world.clock.set_paused(true);
        let profile = PROFILES[0];
        let auto = app
            .store
            .shore_character(
                profile.identity.into(),
                profile.name.into(),
                &app.content,
                false,
            )
            .await
            .unwrap();
        let human = app.store.list("consent-human".into()).await.unwrap();
        app.store.rename(human.id, "MarketQA".into()).await.unwrap();
        let human = app
            .store
            .create(human.id, app.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let mut chars = Vec::new();
        for (mut ch, kind) in [(auto, 0xa4c), (human, 0xa21)] {
            ch.level = 20;
            ch.exp = crate::store::level_start(20).unwrap();
            ch.base = app.content.classes[&ch.class].caps;
            if kind == 0xa4c {
                ch.base[4] -= 4;
            }
            ch.hp = ch.base[0];
            ch.mp = ch.base[1];
            ch.items[4..].copy_from_slice(&[kind, kind, -1, -1, -1, -1, -1, -1]);
            ch.revision = app
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account: ch.account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    vec![],
                )
                .await
                .unwrap()[0];
            app.store.progress(ch.clone()).await.unwrap();
            chars.push(ch);
        }
        let pos = app
            .world
            .directory(|d| {
                let p = d.worlds[&-2].map.spawns[0];
                Ok(Pos { x: p.0, y: p.1 })
            })
            .await
            .unwrap();
        let owner = app.claim(chars[0].account).unwrap();
        let (id, output, _, brain) = enter(&app, profile, chars[0].clone(), -2, Some(pos))
            .await
            .unwrap();
        let (packets, human_output) = mpsc::channel(512);
        let (lost, _) = watch::channel(false);
        let presentation = app.store.presentation(chars[1].account).await.unwrap();
        let human_owner = app.claim(chars[1].account).unwrap();
        let bind = server::lifecycle::binding(&app, chars[1].account).unwrap();
        let ch = chars[1].clone();
        let human = app
            .world
            .in_world(-2)
            .call(move |w| {
                let human =
                    w.enter_at(ch, Some(Output { packets, lost }), Some(pos), presentation)?;
                bind(human)?;
                w.updates(id, true)?;
                w.updates(human, true)?;
                Ok(human)
            })
            .await
            .unwrap();
        let terms = Terms {
            version: 771,
            accounts: [chars[1].account, chars[0].account],
            names: [chars[1].name.clone(), chars[0].name.clone()],
            goods: [
                Goods {
                    kind: 0xa21,
                    quantity: 2,
                },
                Goods {
                    kind: 0xa4c,
                    quantity: 2,
                },
            ],
            meeting: [-2, pos.x.floor() as i32, pos.y.floor() as i32],
            expires: pursuits::market::now() + 180,
            batch: 1,
        };
        fs::write(dir.join("consent-declaration.json"),serde_json::to_vec_pretty(&serde_json::json!({"characters":chars,"terms":terms,"humans":1,"autonomous":1,"qa":0,"scope":"production packet/controller adapter ordering regression; Human commands are normal authority, not autonomous culture proof"})).unwrap()).unwrap();
        println!("market consent initial context: {}", dir.display());
        Self {
            dir,
            app,
            brain,
            output,
            _human_output: human_output,
            _owner: owner,
            _human_owner: human_owner,
            ids: [id, human],
            terms,
        }
    }
    fn record(&self, event: serde_json::Value) {
        let mut out = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("consent-sequence.jsonl"))
            .unwrap();
        writeln!(out, "{event}").unwrap();
    }
    async fn view(&self) -> PlayerView {
        let id = self.ids[0];
        self.app
            .world
            .directory(move |d| d.player_view(id))
            .await
            .unwrap()
    }
    async fn observe_latch(&self, phase: &str) -> serde_json::Value {
        let id = self.ids[0];
        let world = self
            .app
            .world
            .for_player(id)
            .call(move |w| Ok(w.market_consent_test_state(id)))
            .await
            .unwrap();
        self.record(serde_json::json!({"event":"latch_observation","phase":phase,"world":world,
            "controller_trade":self.brain.mind.market.trade.as_ref().map(|t| serde_json::json!({"binding":t.binding,"ready":t.ready,"ready_at":format!("{:?}",t.ready_at),"changed":t.changed,"offers":t.offers})),
            "bargain":self.brain.mind.market.bargain}));
        world
    }
    async fn drain(&mut self) {
        while let Ok(packet) = self.output.try_recv() {
            let v = self.view().await;
            let result = social::receive_market(&self.app, &mut self.brain, &packet, &v)
                .await
                .unwrap();
            self.record(serde_json::json!({"event":"production_controller_packet_uptake","wire_id":packet.id,"wire_body":if [wire::TEXT,12,18,23,61,67].contains(&packet.id){Some(packet.body)}else{None},"view_time_ms":v.time_ms,"result":format!("{result:?}"),"controller_trade":self.brain.mind.market.trade.as_ref().map(|t|serde_json::json!({"ready":t.ready,"ready_at":format!("{:?}",t.ready_at),"changed":t.changed,"offers":t.offers})),"bargain":self.brain.mind.market.bargain}));
            assert_ne!(result, social::MarketUptake::PendingCommit);
        }
    }
    async fn say(&self, text: String) {
        let id = self.ids[1];
        let at = self.app.world.clock.now();
        self.record(serde_json::json!({"event":"human_normal_private_chat","text":text}));
        self.app
            .world
            .directory(move |d| d.chat(id, format!("/tell Triceps {text}"), at))
            .await
            .unwrap();
    }
    async fn driver(&mut self) -> u64 {
        let v = self.view().await;
        social::market(&self.app, &mut self.brain, &v)
            .await
            .unwrap();
        v.time_ms
    }
    fn receive_native_latch(&mut self, masks: [[bool; 12]; 2]) -> bool {
        let mut latched = false;
        while let Ok(p) = self._human_output.try_recv() {
            if [12, 18, 23, 67].contains(&p.id) {
                self.record(serde_json::json!({"event":"human_received_trade_packet","wire_id":p.id,"wire_body":p.body}));
            }
            if p.id == 18 {
                let mut r = Reader::new(&p.body);
                assert_eq!(pursuits::market::read_offer(&mut r).unwrap(), masks[1]);
                assert_eq!(pursuits::market::read_offer(&mut r).unwrap(), masks[0]);
                r.done().unwrap();
                latched = true;
                self.record(serde_json::json!({"event":"actual_native_acceptance_latched","wire_id":18,"wire_body":p.body,"bargain":self.brain.mind.market.bargain}));
            }
        }
        latched
    }
    async fn agree(&mut self) {
        self.say(self.terms.text("Triceps", "offer")).await;
        self.drain().await;
        self.driver().await;
        self.say(self.terms.text("Triceps", "agree")).await;
        self.drain().await;
        assert!(self.brain.mind.market.bargain.as_ref().unwrap().agreed);
    }
    async fn latch(&mut self) -> [[bool; 12]; 2] {
        self.latch_after_scheduler_stall(false).await
    }
    async fn latch_after_scheduler_stall(&mut self, stall_executor: bool) -> [[bool; 12]; 2] {
        let start = std::time::Instant::now();
        self.observe_latch("before_request").await;
        while self._human_output.try_recv().is_ok() {}
        let [auto, human] = self.ids;
        self.app
            .world
            .for_player(human)
            .call(move |w| w.request_trade(human, w.players[&auto].character.name.clone()))
            .await
            .unwrap();
        self.app.world.clock.set_paused(false);
        loop {
            self.drain().await;
            self.driver().await;
            self.drain().await;
            if self.brain.mind.market.trade.is_some() {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(28),
                "fresh consent/request did not establish native trade: {:?}",
                self.brain.mind.market
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.app.world.clock.set_paused(true);
        self.observe_latch("trade_start_received").await;
        self.driver().await;
        self.observe_latch("own_change_done").await;
        let masks = self
            .app
            .world
            .in_world(-2)
            .call(move |w| {
                let mut m = [[false; 12]; 2];
                for (i, kind) in [0xa4c, 0xa21].into_iter().enumerate() {
                    let slot = w.players[&[auto, human][i]]
                        .character
                        .items
                        .iter()
                        .enumerate()
                        .skip(4)
                        .find(|(_, k)| **k == kind)
                        .unwrap()
                        .0;
                    m[i][slot] = true;
                }
                w.change_trade(human, m[1])?;
                Ok(m)
            })
            .await
            .unwrap();
        self.observe_latch("human_change_done").await;
        self.drain().await;
        self.observe_latch("human_change_received_before_sleep")
            .await;
        let original_ready = self.brain.mind.market.trade.as_ref().unwrap().ready;
        self.brain.mind.market.trade.as_mut().unwrap().ready = 0;
        self.driver().await;
        let unready = self
            .observe_latch("tick_predicate_forced_but_elapsed_still_unready")
            .await;
        assert!(unready["clock_paused"].as_bool().unwrap());
        assert!(!unready["trade"]["settled"].as_bool().unwrap());
        assert_eq!(
            unready["trade"]["accepted"],
            serde_json::json!([false, false])
        );
        assert!(!self.receive_native_latch(masks));
        self.brain
            .mind
            .market
            .trade
            .as_mut()
            .expect("elapsed guard must prevent a rejected premature native Accept")
            .ready = original_ready;
        let binding = self.brain.mind.market.trade.as_ref().unwrap().binding;
        if stall_executor {
            self.app.world.clock.set_paused(false);
            let original_window = tokio::time::Instant::now() + Duration::from_millis(3250);
            self.record(serde_json::json!({"event":"declared_scheduler_counterfactual","stall_wall_ms":400,"original_total_wall_window_ms":3250,"scope":"current-thread test executor only; no clock/tick/authority mutation"}));
            std::thread::sleep(Duration::from_millis(400));
            tokio::time::sleep_until(original_window).await;
            self.app.world.clock.set_paused(true);
            let observed = self
                .observe_latch("fixed_stall_original_window_unready")
                .await;
            let now = observed["world_now"].as_u64().unwrap();
            assert!(observed["trade"]["settled"].as_bool().unwrap());
            assert!(now < self.brain.mind.market.trade.as_ref().unwrap().ready);
            assert_eq!(
                observed["trade"]["accepted"],
                serde_json::json!([false, false])
            );
            assert_eq!(observed["trade"]["offers"], serde_json::json!(masks));
            self.driver().await;
            assert!(!self.receive_native_latch(masks));
            self.observe_latch("no_early_native_acceptance").await;
        }
        // World ticks can lag wall time; only a real native acceptance proves the settling guard passed.
        self.app.world.clock.set_paused(false);
        loop {
            assert!(
                start.elapsed() < Duration::from_secs(28),
                "native acceptance deadline: {}",
                self.dir.display()
            );
            self.drain().await;
            let controller_ready = self.brain.mind.market.trade.as_ref().unwrap().ready;
            let decision_time = self.driver().await;
            let observed = self.observe_latch("settling_poll").await;
            let latched = self.receive_native_latch(masks);
            if decision_time < controller_ready {
                assert!(!latched);
                assert_eq!(
                    observed["trade"]["accepted"],
                    serde_json::json!([false, false])
                );
            }
            if latched {
                let trade = self.brain.mind.market.trade.as_ref().unwrap();
                assert_eq!(trade.binding, binding);
                assert_eq!(trade.peer, self.terms.names[0]);
                assert!(decision_time >= controller_ready);
                assert!(self.app.world.clock.now() >= trade.ready_at);
                assert!(observed["trade"]["settled"].as_bool().unwrap());
                assert_eq!(
                    observed["trade"]["accepted"],
                    serde_json::json!([true, false])
                );
                assert_eq!(observed["trade"]["offers"], serde_json::json!(masks));
                assert!(start.elapsed() < Duration::from_secs(28));
                self.app.world.clock.set_paused(true);
                self.record(serde_json::json!({"event":"actual_latch_predicate","latched":true,"elapsed_wall_ms":start.elapsed().as_millis(),"fixed_scheduler_stall":stall_executor,"decision_time":decision_time,"controller_ready":controller_ready}));
                return masks;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    async fn accept_human(&self, m: [[bool; 12]; 2]) -> TradePlan {
        let id = self.ids[1];
        let plan = self
            .app
            .world
            .for_player(id)
            .call(move |w| w.accept_trade(id, m[1], m[0]))
            .await
            .unwrap()
            .expect("bot must have latched its real native acceptance");
        self.record(serde_json::json!({"event":"both_native_accepts_reserved_atomic_exchange","transaction":plan.transaction,"delivery":plan.delivery}));
        plan
    }
    async fn changed(&self) {
        let mut t = self.terms.clone();
        t.version += 1;
        t.batch = 2;
        self.say(t.text("Triceps", "counter")).await;
    }
    fn writes(p: &TradePlan) -> Vec<InventoryWrite> {
        p.plans
            .iter()
            .map(|p| InventoryWrite {
                id: p.character,
                account: p.account,
                revision: p.revision,
                items: p.items.clone(),
            })
            .collect()
    }
    async fn receipts(&self) -> Vec<pursuits::market::Delivery> {
        self.app
            .store
            .market_receipts(self.terms.accounts[1], self.terms.version)
            .await
            .unwrap()
    }
    async fn finish(self, label: &str) {
        fs::write(self.dir.join("consent-result.json"),serde_json::to_vec_pretty(&serde_json::json!({"label":label,"mind":self.brain.mind,"receipts":self.receipts().await,"case":"real Directory TEXT -> production controller uptake -> guarded World native acceptance -> Store receipts"})).unwrap()).unwrap();
        self.app.shutdown.send_replace(true);
        for id in self.ids {
            server::depart(&self.app, id).await.unwrap();
        }
        println!("market consent ordering evidence: {}", self.dir.display());
    }
}
#[tokio::test]
async fn community_market_controller_changed_terms_and_refusal_revoke_latched_native_acceptance_before_payment()
 {
    for refusal in [false, true] {
        let mut c = Context::new().await;
        c.agree().await;
        let masks = c.latch().await;
        if refusal {
            c.say("Triceps, no deal: I withdraw.".into()).await;
        } else {
            c.changed().await;
        }
        c.drain().await;
        let human = c.ids[1];
        assert!(
            c.app
                .world
                .for_player(human)
                .call(move |w| w.accept_trade(human, masks[1], masks[0]))
                .await
                .is_err()
        );
        assert!(c.receipts().await.is_empty());
        let b = c.brain.mind.market.bargain.as_ref().unwrap();
        assert!(!b.own_consent && b.peer_consent.is_none());
        if refusal {
            assert!(b.resolution.is_some());
        } else {
            assert_ne!(b.terms.version, c.terms.version);
        }
        c.finish(if refusal {
            "refusal cancels old latch"
        } else {
            "new version cancels old latch"
        })
        .await;
    }
}
#[tokio::test]
async fn community_market_controller_partial_rewrite_keeps_original_balance_and_requires_fresh_remaining_consent()
 {
    partial_rewrite(false).await;
}

#[tokio::test]
async fn community_market_controller_partial_rewrite_waits_for_real_settling_after_executor_stall()
{
    partial_rewrite(true).await;
}

async fn partial_rewrite(stall_executor: bool) {
    let mut c = Context::new().await;
    c.agree().await;
    let masks = c.latch().await;
    let plan = c.accept_human(masks).await;
    server::persist_trade(&c.app, plan).await.unwrap();
    c.driver().await;
    let old_remaining_masks = c.latch().await;
    let stale_remaining = c
        .brain
        .mind
        .market
        .bargain
        .as_ref()
        .unwrap()
        .text("Triceps", "agree");
    c.changed().await;
    c.drain().await;
    let human = c.ids[1];
    assert!(
        c.app
            .world
            .for_player(human)
            .call(move |w| w.accept_trade(human, old_remaining_masks[1], old_remaining_masks[0]))
            .await
            .is_err()
    );
    let b = c.brain.mind.market.bargain.as_ref().unwrap();
    assert_eq!(b.terms, c.terms);
    assert_eq!(b.confirmed, 1);
    assert_eq!(b.consent_revision, 1);
    assert!(!b.own_consent && b.peer_consent.is_none());
    c.say(stale_remaining).await;
    c.drain().await;
    c.driver().await;
    assert!(
        c.brain
            .mind
            .market
            .bargain
            .as_ref()
            .unwrap()
            .peer_consent
            .is_none()
    );
    assert_eq!(c.receipts().await.len(), 1);
    let fresh = c
        .brain
        .mind
        .market
        .bargain
        .as_ref()
        .unwrap()
        .text("Triceps", "agree");
    c.say(fresh).await;
    c.drain().await;
    let masks = c.latch_after_scheduler_stall(stall_executor).await;
    let plan = c.accept_human(masks).await;
    server::persist_trade(&c.app, plan).await.unwrap();
    c.driver().await;
    assert_eq!(c.receipts().await.len(), 2);
    assert_eq!(c.brain.mind.market.bargain.as_ref().unwrap().confirmed, 2);
    c.finish("original remaining installment requires fresh paid-ordinal/revision agreement")
        .await;
}
#[tokio::test]
async fn community_market_controller_pending_exchange_defers_changed_terms_receipt_wins_once_and_replay_cannot_repay()
 {
    let mut c = Context::new().await;
    c.agree().await;
    let masks = c.latch().await;
    let plan = c.accept_human(masks).await;
    c.changed().await;
    let mut deferred = None;
    while let Ok(p) = c.output.try_recv() {
        let v = c.view().await;
        if social::receive_market(&c.app, &mut c.brain, &p, &v)
            .await
            .unwrap()
            == social::MarketUptake::PendingCommit
        {
            deferred = Some(p);
            break;
        }
    }
    let packet = deferred.expect("changed terms must wait for the pending atomic exchange");
    assert_eq!(c.brain.mind.market.bargain.as_ref().unwrap().terms, c.terms);
    assert_eq!(c.brain.mind.market.bargain.as_ref().unwrap().confirmed, 0);
    let writes = Context::writes(&plan);
    let tx = plan.transaction.clone();
    let delivery = plan.delivery.clone();
    let revisions = c
        .app
        .store
        .exchange_trade(tx.clone(), writes.clone(), delivery.clone())
        .await
        .unwrap();
    let v = c.view().await;
    assert_eq!(
        social::receive_market(&c.app, &mut c.brain, &packet, &v)
            .await
            .unwrap(),
        social::MarketUptake::PendingCommit
    );
    let auto = c.ids[0];
    let r = revisions.clone();
    c.app
        .world
        .for_player(auto)
        .call(move |w| w.finish_trade(plan, Ok(r)))
        .await
        .unwrap();
    let v = c.view().await;
    assert_eq!(
        social::receive_market(&c.app, &mut c.brain, &packet, &v)
            .await
            .unwrap(),
        social::MarketUptake::Applied
    );
    let b = c.brain.mind.market.bargain.as_ref().unwrap();
    assert_eq!(b.terms, c.terms);
    assert_eq!(b.confirmed, 1);
    assert_eq!(b.consent_revision, 1);
    assert!(!b.own_consent && b.peer_consent.is_none());
    assert_eq!(
        c.app
            .store
            .exchange_trade(tx, writes, delivery)
            .await
            .unwrap(),
        revisions
    );
    assert_eq!(c.receipts().await.len(), 1);
    c.finish("pending commit is not undone; original receipt reconciled before changed terms; replay idempotent").await;
}
