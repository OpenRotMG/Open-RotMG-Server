use super::*;
use crate::{
    bots::runtime::tests::actor_in,
    server::{App, Owner},
    world::{Slot, TradeAction},
};
use tokio::sync::mpsc;
const RING: i32 = 0xa4f;

struct Scene {
    dir: std::path::PathBuf,
    app: App,
    _owners: [Owner; 2],
    output: mpsc::Receiver<Packet>,
    _peer_output: mpsc::Receiver<Packet>,
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
            _peer_output: peer_output,
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
            receive_life(&self.app, &mut self.brain, &packet, &v)
                .await
                .unwrap();
            if packet.id != wire::TEXT {
                self.brain
                    .mind
                    .market
                    .receive(&packet, &v, &self.app.content, None)
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
    s.publish().await;
    let mut heard = false;
    while let Ok(packet) = s.output.try_recv() {
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
    let v = s.view().await;
    let consent = s.text(&format!("Donor, yes {item_name}"), s.peer);
    assert!(
        receive_life(&s.app, &mut s.brain, &consent, &v)
            .await
            .unwrap()
    );
    life(&s.app, &mut s.brain, &v).await.unwrap();
    let id = v.session.player;
    let peer = s.peer;
    s.app
        .world
        .for_player(peer)
        .call(move |w| w.request_trade(peer, "Donor".into()))
        .await
        .unwrap();
    s.pump().await;
    let v = s.view().await;
    life(&s.app, &mut s.brain, &v).await.unwrap();
    s.pump().await;
    s.app
        .world
        .for_player(id)
        .call(move |w| {
            for _ in 0..61 {
                w.step();
            }
            Ok(())
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(3050)).await;
    let v = s.view().await;
    life(&s.app, &mut s.brain, &v).await.unwrap();
    assert!(
        s.brain
            .mind
            .social
            .gift
            .as_ref()
            .unwrap()
            .acceptance
            .is_some()
    );
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
async fn equipment_drop_guard_runs_after_queue_mutation_and_retains_stock_on_rejection() {
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
