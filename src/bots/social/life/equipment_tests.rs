use super::*;
use crate::{
    bots::runtime::tests::actor_in,
    server::{App, Owner},
    world::Slot,
};
use tokio::sync::mpsc;
const RING: i32 = 0xa4f;

struct Scene {
    dir: std::path::PathBuf,
    app: App,
    _owners: [Owner; 2],
    output: mpsc::Receiver<Packet>,
    peer_output: mpsc::Receiver<Packet>,
    brain: Brain,
    peer: i32,
}
impl Scene {
    async fn new(backpack: [i32; 8]) -> Self {
        let (dir, app) = crate::server::test_app();
        app.world.clock.set_paused(true);
        let (owner, id, output, mut brain) = actor_in(
            &app,
            "equipment-donor",
            "Donor",
            [RING, RING, -1, -1, -1, -1, -1, -1],
            -3,
        )
        .await;
        let world = brain.session.0;
        let (peer_owner, peer, peer_output, _) =
            actor_in(&app, "equipment-peer", "Recipient", backpack, world).await;
        app.world
            .directory(move |d| {
                let w = d.worlds.get_mut(&world).unwrap();
                let pos = w.players[&id].pos;
                w.players.get_mut(&peer).unwrap().pos = Pos {
                    x: pos.x + 0.5,
                    y: pos.y,
                };
                w.updates(id, true)?;
                w.updates(peer, true)?;
                Ok(())
            })
            .await
            .unwrap();
        let v = app.world.player_view(id).await.unwrap();
        brain
            .mind
            .intentions
            .as_mut()
            .unwrap()
            .observe(&v, &app.content, brain.profile.identity)
            .unwrap();
        Self {
            dir,
            app,
            _owners: [owner, peer_owner],
            output,
            peer_output,
            brain,
            peer,
        }
    }
    async fn view(&self) -> PlayerView {
        self.app
            .world
            .player_view(self.brain.session.1)
            .await
            .unwrap()
    }
    async fn offer(&mut self, drop: bool) -> equipment::Candidate {
        let v = self.view().await;
        let policy = equipment::Policy {
            future_class: None,
            current_goal: None,
            willingness: 6.,
            available: true,
        };
        let candidate = equipment::select(&v, &self.app.content, &self.brain.mind.market, &policy)
            .unwrap()
            .expect("real spare ring for public missing ring");
        assert!(
            self.brain
                .equipment_check(&v, None)
                .current(&candidate, &v, &self.app.content)
                .unwrap()
        );
        self.brain
            .mind
            .social
            .offer_equipment(&v, &self.app.content, candidate.clone(), drop)
            .unwrap();
        candidate
    }
    fn text(&self, body: &str, actor: i32) -> Packet {
        let mut w = wire::Writer::default();
        w.utf("Recipient")
            .i32(actor)
            .i32(0)
            .u8(0)
            .utf("")
            .utf(body)
            .utf("");
        Packet {
            id: wire::TEXT,
            body: w.0,
        }
    }
    async fn pump(&mut self) {
        let v = self.view().await;
        while let Ok(packet) = self.output.try_recv() {
            if !receive_life(&self.app, &mut self.brain, &packet, &v)
                .await
                .unwrap()
            {
                crate::bots::social::receive_market(&self.app, &mut self.brain, &packet, &v)
                    .await
                    .unwrap();
            }
        }
    }
    async fn publish(&mut self) {
        for _ in 0..128 {
            let v = self.view().await;
            self.brain.advance_composition(&self.app, &v).await.unwrap();
            life(&self.app, &mut self.brain, &v).await.unwrap();
            if self.brain.mind.social.outgoing.is_none() {
                return;
            }
            self.app
                .world
                .for_player(v.session.player)
                .call(|w| {
                    w.step();
                    Ok(())
                })
                .await
                .unwrap();
        }
        panic!("typed equipment offer did not publish");
    }
    async fn native_countdown(&mut self) {
        let before = self.view().await;
        self.app.world.clock.set_paused(false);
        let elapsed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let current = self.view().await;
                if current.time_ms >= before.time_ms + 3050
                    && current.clock_at >= before.clock_at + std::time::Duration::from_millis(3050)
                {
                    break;
                }
                self.pump().await;
                while self.peer_output.try_recv().is_ok() {}
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await;
        self.app.world.clock.set_paused(true);
        elapsed
            .expect("native countdown must elapse on both actual game and pause-adjusted clocks");
    }
    async fn native_acceptance(&mut self) -> bool {
        let item_name = self.app.content.get(RING as u16).unwrap().name.clone();
        self.publish().await;
        let mut heard = false;
        while let Ok(packet) = self.output.try_recv() {
            if packet.id == wire::TEXT {
                let mut r = Reader::new(&packet.body);
                r.utf().unwrap();
                r.i32().unwrap();
                r.i32().unwrap();
                r.u8().unwrap();
                r.utf().unwrap();
                heard |= r.utf().unwrap().contains(&item_name);
            }
        }
        assert!(heard, "normal typed offer must actually publish");
        let v = self.view().await;
        let consent = self.text(&format!("Donor, yes {item_name}"), self.peer);
        assert!(
            receive_life(&self.app, &mut self.brain, &consent, &v)
                .await
                .unwrap()
        );
        life(&self.app, &mut self.brain, &v).await.unwrap();
        let peer = self.peer;
        self.app
            .world
            .for_player(peer)
            .call(move |w| w.request_trade(peer, "Donor".into()))
            .await
            .unwrap();
        self.pump().await;
        assert!(
            self.brain.mind.market.incoming.is_none(),
            "gift native request must not become a market negotiation"
        );
        assert!(
            self.brain.mind.market.trade.is_some(),
            "reciprocal native request must open the window"
        );
        let v = self.view().await;
        life(&self.app, &mut self.brain, &v).await.unwrap();
        assert!(
            self.brain.mind.social.gift.is_some(),
            "gift must remain after ordinary native Change"
        );
        self.pump().await;
        self.native_countdown().await;
        let v = self.view().await;
        life(&self.app, &mut self.brain, &v).await.unwrap();
        self.brain
            .mind
            .social
            .gift
            .as_ref()
            .is_some_and(|g| g.acceptance.is_some())
    }
    async fn close(self) {
        let dir = self.dir.clone();
        drop(self);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn equipment_consumer_types_actual_offer_and_requires_receipt_even_after_eligibility_changes()
{
    let mut s = Scene::new([-1; 8]).await;
    let candidate = s.offer(false).await;
    let item_name = s.app.content.get(RING as u16).unwrap().name.clone();
    assert!(
        s.brain
            .mind
            .social
            .outgoing
            .as_ref()
            .unwrap()
            .contains(&item_name)
    );
    assert!(s.brain.mind.social.episodes.is_empty());
    assert!(
        s.native_acceptance().await,
        "normal native gift must reach its acceptance witness"
    );
    let peer = s.peer;
    assert!(s.brain.mind.social.episodes.is_empty());
    s.brain.profile.companions = 0.;
    let v = s.view().await;
    assert!(
        life(&s.app, &mut s.brain, &v).await.unwrap(),
        "native witness protects pending settlement before eligibility"
    );
    let mut mine = [false; 12];
    mine[candidate.slot] = true;
    let plan = s
        .app
        .world
        .for_player(peer)
        .call(move |w| Ok(w.accept_trade(peer, [false; 12], mine)?.unwrap()))
        .await
        .unwrap();
    crate::server::persist_trade(&s.app, plan).await.unwrap();
    let v = s.view().await;
    life(&s.app, &mut s.brain, &v).await.unwrap();
    assert!(s.brain.mind.social.gift.is_none());
    assert_eq!(v.character.items[candidate.slot], -1);
    let recipient = s.app.world.player_view(peer).await.unwrap();
    assert!(recipient.character.items[4..].contains(&RING));
    assert_eq!(s.brain.mind.social.episodes.len(), 1);
    assert_eq!(
        s.brain.mind.social.episodes[0].interaction,
        Interaction::GaveEquipment
    );
    life(&s.app, &mut s.brain, &v).await.unwrap();
    assert_eq!(s.brain.mind.social.episodes.len(), 1);
    let saved = s
        .app
        .store
        .load(v.session.account, v.session.character)
        .await
        .unwrap();
    assert_eq!(saved.items, v.character.items);
    s.close().await;
}

#[tokio::test]
async fn equipment_consent_is_addressed_kind_actor_bound_and_refusal_never_drops() {
    let mut s = Scene::new([-1; 8]).await;
    let candidate = s.offer(false).await;
    let v = s.view().await;
    let name = s.app.content.get(RING as u16).unwrap().name.clone();
    let early = s.text(&format!("Donor, yes {name}"), s.peer);
    assert!(
        !receive_life(&s.app, &mut s.brain, &early, &v)
            .await
            .unwrap()
    );
    assert!(!s.brain.mind.social.gift.as_ref().unwrap().consent);
    s.publish().await;
    for (body, actor) in [
        (format!("Other, yes {name}"), s.peer),
        ("Donor, yes health".into(), s.peer),
        (format!("Donor, yes {name}"), s.peer + 100),
    ] {
        let packet = s.text(&body, actor);
        receive_life(&s.app, &mut s.brain, &packet, &v)
            .await
            .unwrap();
        assert!(!s.brain.mind.social.gift.as_ref().unwrap().consent);
    }
    let packet = s.text("Donor, no thanks", s.peer);
    receive_life(&s.app, &mut s.brain, &packet, &v)
        .await
        .unwrap();
    assert!(!life(&s.app, &mut s.brain, &v).await.unwrap());
    assert!(!s.brain.mind.social.equipment_due(&v));
    assert!(s.brain.mind.social.drop.is_none());
    assert!(s.brain.mind.social.episodes.is_empty());
    assert_eq!(s.view().await.character.items[candidate.slot], RING);
    s.close().await;
}

#[tokio::test]
async fn equipment_candidate_fence_rejects_stale_stock_peer_world_death_and_unwillingness() {
    let mut s = Scene::new([-1; 8]).await;
    let candidate = s.offer(false).await;
    let mut v = s.view().await;
    let check = s.brain.equipment_check(&v, Some(&candidate));
    v.character.inventory_revision += 1;
    assert!(!check.current(&candidate, &v, &s.app.content).unwrap());
    v.character.inventory_revision -= 1;
    v.character.items[candidate.slot] = HEALTH;
    assert!(!check.current(&candidate, &v, &s.app.content).unwrap());
    v.character.items[candidate.slot] = RING;
    v.session.world += 1;
    assert!(!check.current(&candidate, &v, &s.app.content).unwrap());
    v.session.world -= 1;
    let peer = v.objects.iter_mut().find(|p| p.id == s.peer).unwrap();
    peer.name = "Replaced".into();
    assert!(!check.current(&candidate, &v, &s.app.content).unwrap());
    let peer = v.objects.iter_mut().find(|p| p.id == s.peer).unwrap();
    peer.name = "Recipient".into();
    peer.id += 100;
    assert!(!check.current(&candidate, &v, &s.app.content).unwrap());
    let peer = v
        .objects
        .iter_mut()
        .find(|p| p.name == "Recipient")
        .unwrap();
    peer.id -= 100;
    v.lifecycle.death_pending = true;
    assert!(!check.current(&candidate, &v, &s.app.content).unwrap());
    v.lifecycle.death_pending = false;
    s.brain.profile.companions = 5.;
    assert!(
        !s.brain
            .equipment_check(&v, Some(&candidate))
            .current(&candidate, &v, &s.app.content)
            .unwrap()
    );
    assert!(s.brain.mind.social.episodes.is_empty());
    s.close().await;
}

#[tokio::test]
async fn equipment_drop_is_initial_choice_and_commits_ordinary_removal_and_pickup_without_history()
{
    let mut s = Scene::new([-1; 8]).await;
    let candidate = s.offer(true).await;
    assert!(s.brain.mind.social.gift.is_none());
    assert!(s.brain.mind.social.outgoing.is_none());
    let v = s.view().await;
    assert!(life(&s.app, &mut s.brain, &v).await.unwrap());
    let after = s.view().await;
    assert_eq!(after.character.items[candidate.slot], -1);
    assert_eq!(
        after.character.inventory_revision,
        v.character.inventory_revision + 1
    );
    assert!(s.brain.mind.social.episodes.is_empty());
    let peer = s.peer;
    s.app
        .world
        .for_player(peer)
        .call(move |w| {
            w.updates(peer, true)?;
            Ok(())
        })
        .await
        .unwrap();
    let recipient = s.app.world.player_view(peer).await.unwrap();
    let bag = recipient
        .objects
        .iter()
        .find(|o| o.contents.contains(&RING))
        .expect("ordinary visible ground bag");
    let from = Slot {
        object: bag.id,
        slot: bag.contents.iter().position(|k| *k == RING).unwrap(),
        kind: RING,
    };
    let to = Slot {
        object: peer,
        slot: 4,
        kind: -1,
    };
    assert!(
        crate::server::inventory(&s.app, peer, move |w| w
            .autonomous_action(peer, Action::Swap { from, to }))
        .await
        .unwrap()
        .unwrap()
    );
    let recipient = s.app.world.player_view(peer).await.unwrap();
    assert_eq!(recipient.character.items[4], RING);
    let saved = s
        .app
        .store
        .load(recipient.session.account, recipient.session.character)
        .await
        .unwrap();
    assert_eq!(saved.items, recipient.character.items);
    assert!(s.brain.mind.social.episodes.is_empty());
    s.close().await;
}

#[tokio::test]
async fn equipment_drop_guard_rechecks_stale_revision_and_retains_stock_on_rejection() {
    let mut s = Scene::new([-1; 8]).await;
    let candidate = s.offer(true).await;
    let id = s.brain.session.1;
    s.app
        .world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().character.revision += 1;
            Ok(())
        })
        .await
        .unwrap();
    let v = s.view().await;
    life(&s.app, &mut s.brain, &v).await.unwrap();
    assert_eq!(s.view().await.character.items[candidate.slot], RING);
    assert!(s.brain.mind.social.episodes.is_empty());
    assert!(s.brain.mind.social.drop.is_none());
    s.close().await;
}

#[tokio::test]
async fn equipment_no_space_native_refusal_keeps_goods_without_drop_or_history() {
    let mut s = Scene::new([HEALTH; 8]).await;
    let candidate = s.offer(false).await;
    assert!(!s.native_acceptance().await);
    let v = s.view().await;
    assert_eq!(v.character.items[candidate.slot], RING);
    assert!(s.brain.mind.social.episodes.is_empty());
    assert!(s.brain.mind.social.drop.is_none());
    assert!(!s.brain.mind.social.equipment_due(&v));
    let peer = s.app.world.player_view(s.peer).await.unwrap();
    assert!(!peer.character.items.contains(&RING));
    s.close().await;
}

#[tokio::test]
async fn equipment_drop_rejects_native_window_opened_after_check_capture_without_cancelling_it() {
    let mut s = Scene::new([-1; 8]).await;
    s.offer(true).await;
    let candidate = s.brain.mind.social.drop.take().unwrap();
    assert!(s.brain.mind.market.trade.is_none());
    let before = s.view().await;
    let id = before.session.player;
    let peer = s.peer;
    let peer_before = s.app.world.player_view(peer).await.unwrap();
    let bags_before = s
        .app
        .world
        .for_player(id)
        .call(|w| {
            Ok(w.entities
                .iter()
                .filter(|(_, e)| w.content.objects[&e.kind].class == "Container")
                .map(|(id, e)| (*id, e.items.clone()))
                .collect::<BTreeMap<_, _>>())
        })
        .await
        .unwrap();
    let diagnostic = s.dir.join("drop-rejection.log");
    let _diagnostics = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(std::sync::Mutex::new(
                std::fs::File::create(&diagnostic).unwrap(),
            ))
            .finish(),
    );
    let mut mine = [false; 12];
    mine[candidate.slot] = true;
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let actor = s.app.world.for_player(id);
    let opening = tokio::spawn(async move {
        actor
            .call(move |w| {
                entered.send(()).unwrap();
                wait.recv().context("native-window gate abandoned")?;
                w.request_trade(peer, "Donor".into())?;
                w.request_trade(id, "Recipient".into())?;
                w.change_trade(id, mine)?;
                Ok(())
            })
            .await
    });
    ready.await.unwrap();
    let app = s.app.clone();
    let mut dropping = Box::pin(generosity::drop_equipment(
        &app,
        &mut s.brain,
        &before,
        candidate,
    ));
    // Check is captured before the first await, while this World still cannot run its queued fence.
    assert!(futures_util::poll!(dropping.as_mut()).is_pending());
    release.send(()).unwrap();
    opening.await.unwrap().unwrap();
    dropping.await.unwrap();
    let rejection = std::fs::read_to_string(diagnostic).unwrap();
    assert!(rejection.contains("equipment drop blocked by active native trade"));
    assert!(rejection.contains("equipment drop rejected; no delivery inferred"));
    assert_eq!(
        crate::server::lifecycle::state(&s.app, before.session.account),
        Some((false, Some(id), false))
    );
    let after = s.view().await;
    let peer_after = s.app.world.player_view(peer).await.unwrap();
    for (before, after) in [(&before, &after), (&peer_before, &peer_after)] {
        assert_eq!(after.character.items, before.character.items);
        assert_eq!(
            after.character.inventory_revision,
            before.character.inventory_revision
        );
        let saved = s
            .app
            .store
            .load(after.session.account, after.session.character)
            .await
            .unwrap();
        assert_eq!(saved.items, before.character.items);
        assert_eq!(saved.revision, before.character.inventory_revision);
    }
    assert!(s.brain.mind.social.episodes.is_empty());
    let bags_after = s
        .app
        .world
        .for_player(id)
        .call(move |w| {
            assert!(w.has_active_trade(id));
            assert!(w.has_active_trade(peer));
            Ok(w.entities
                .iter()
                .filter(|(_, e)| w.content.objects[&e.kind].class == "Container")
                .map(|(id, e)| (*id, e.items.clone()))
                .collect::<BTreeMap<_, _>>())
        })
        .await
        .unwrap();
    assert_eq!(bags_after, bags_before);
    s.native_countdown().await;
    s.app
        .world
        .for_player(peer)
        .call(move |w| {
            assert!(
                w.accept_trade(peer, [false; 12], mine)?.is_none(),
                "original donor mask remains unchanged, with no donor acceptance"
            );
            assert!(w.has_active_trade(id));
            w.cancel_trade(id, "Test complete");
            Ok(())
        })
        .await
        .unwrap();
    s.close().await;
}

#[tokio::test]
async fn health_native_window_without_chat_consent_uses_life_and_verified_receipt() {
    let mut s = Scene::new([HEALTH, -1, -1, -1, -1, -1, -1, -1]).await;
    let before = s.view().await;
    assert_eq!(health(&before), 0);
    let id = before.session.player;
    let peer = s.peer;
    let recipient = s.app.world.player_view(peer).await.unwrap();
    s.brain.mind.social.gift = Some(Gift {
        account: recipient.session.account,
        player: peer,
        name: "Recipient".into(),
        giving: false,
        kind: HEALTH,
        item_name: "health".into(),
        equipment: None,
        until: before.elapsed_ms + 30000,
        session: [before.session.world, id, before.session.character],
        requested: true,
        consent: false,
        acceptance: None,
        cancelled: false,
    });
    let mine = [false; 12];
    let mut theirs = [false; 12];
    theirs[4] = true;
    s.app
        .world
        .for_player(id)
        .call(move |w| {
            w.request_trade(id, "Recipient".into())?;
            w.request_trade(peer, "Donor".into())?;
            w.change_trade(peer, theirs)?;
            Ok(())
        })
        .await
        .unwrap();
    s.pump().await;
    let v = s.view().await;
    life(&s.app, &mut s.brain, &v).await.unwrap();
    assert!(
        s.brain.mind.social.gift.is_some(),
        "legacy health native consent does not require chat assent"
    );
    s.native_countdown().await;
    let v = s.view().await;
    life(&s.app, &mut s.brain, &v).await.unwrap();
    let gift = s.brain.mind.social.gift.as_ref().unwrap();
    assert!(!gift.consent);
    assert_eq!(gift.acceptance.as_ref().unwrap().kind, HEALTH);
    assert!(s.brain.mind.social.episodes.is_empty());
    let plan = s
        .app
        .world
        .for_player(peer)
        .call(move |w| Ok(w.accept_trade(peer, theirs, mine)?.unwrap()))
        .await
        .unwrap();
    crate::server::persist_trade(&s.app, plan).await.unwrap();
    let settled = s.view().await;
    life(&s.app, &mut s.brain, &settled).await.unwrap();
    assert_eq!(health(&settled), 1);
    assert!(s.brain.mind.social.gift.is_none());
    assert_eq!(s.brain.mind.social.episodes.len(), 1);
    assert_eq!(
        s.brain.mind.social.episodes[0].interaction,
        Interaction::ReceivedHealth
    );
    s.close().await;
}

#[tokio::test]
async fn equipment_native_window_without_chat_consent_is_rejected_without_drop_or_history() {
    let mut s = Scene::new([-1; 8]).await;
    s.offer(false).await;
    s.publish().await;
    assert!(!s.brain.mind.social.gift.as_ref().unwrap().consent);
    let before = s.view().await;
    let id = before.session.player;
    let peer = s.peer;
    s.app
        .world
        .for_player(id)
        .call(move |w| {
            w.request_trade(id, "Recipient".into())?;
            w.request_trade(peer, "Donor".into())?;
            Ok(())
        })
        .await
        .unwrap();
    s.pump().await;
    let v = s.view().await;
    assert!(!life(&s.app, &mut s.brain, &v).await.unwrap());
    let after = s.view().await;
    assert_eq!(after.character.items, before.character.items);
    assert_eq!(
        after.character.inventory_revision,
        before.character.inventory_revision
    );
    assert!(s.brain.mind.social.gift.is_none());
    assert!(s.brain.mind.social.drop.is_none());
    assert!(s.brain.mind.social.episodes.is_empty());
    assert!(!s.brain.mind.social.equipment_due(&after));
    s.app
        .world
        .for_player(id)
        .call(move |w| {
            assert!(!w.has_active_trade(id));
            Ok(())
        })
        .await
        .unwrap();
    s.close().await;
}
