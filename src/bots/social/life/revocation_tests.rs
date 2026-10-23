use super::*;
use crate::{server::App, store::InventoryWrite, world::Output};
use tokio::sync::{mpsc, watch};

async fn uptake_packet(app: &App, brain: &mut Brain, packet: &Packet) {
    let id = brain.session.1;
    let v = app
        .world
        .directory(move |d| d.player_view(id))
        .await
        .unwrap();
    if !receive_life(app, brain, packet, &v).await.unwrap() {
        brain
            .mind
            .market
            .receive(packet, &v, &app.content, None)
            .unwrap();
    }
}
async fn uptake(app: &App, brain: &mut Brain, rx: &mut mpsc::Receiver<Packet>) {
    while let Ok(packet) = rx.try_recv() {
        uptake_packet(app, brain, &packet).await;
    }
}
async fn tick(app: &App, brain: &mut Brain) -> bool {
    let id = brain.session.1;
    let v = app
        .world
        .directory(move |d| d.player_view(id))
        .await
        .unwrap();
    life(app, brain, &v).await.unwrap()
}
async fn settle_trade_clock(app: &App) {
    // Holding this fixture's worker avoids inventing ticks or filling synthetic clients' output queues.
    let clock = app.world.clock.clone();
    app.world
        .in_world(-2)
        .call(move |_| {
            assert!(clock.paused());
            clock.set_paused(false);
            std::thread::sleep(std::time::Duration::from_millis(3050));
            clock.set_paused(true);
            Ok(())
        })
        .await
        .unwrap();
}
enum Ending {
    WithdrawBeforeCommit,
    CommitBeforeWithdrawal,
    RejectNativeAccept,
}
async fn withdrawal(ending: Ending) {
    let commit_first = matches!(ending, Ending::CommitBeforeWithdrawal);
    let (dir, app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    let mut ids = vec![];
    let mut rx = vec![];
    let mut owners = vec![];
    for (i, name) in ["Kadabu", "Recipient"].into_iter().enumerate() {
        let a = app
            .store
            .list(format!("gift-revocation-{i}"))
            .await
            .unwrap()
            .id;
        let mut ch = app
            .store
            .create_prepared(
                a,
                app.content.get(crate::content::WIZARD).unwrap(),
                Some(name.into()),
            )
            .await
            .unwrap();
        ch.items[4..].fill(-1);
        if i == 0 {
            ch.items[4..8].fill(HEALTH);
        }
        ch.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account: a,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        app.store.progress(ch.clone()).await.unwrap();
        owners.push(app.claim(a).unwrap());
        let (packets, receiver) = mpsc::channel(512);
        let (lost, _) = watch::channel(false);
        rx.push(receiver);
        let presentation = app.store.presentation(a).await.unwrap();
        let bind = crate::server::lifecycle::binding(&app, a).unwrap();
        let id = app
            .world
            .in_world(-2)
            .call(move |w| {
                let id = w.enter_at(
                    ch,
                    Some(Output { packets, lost }),
                    Some(Pos {
                        x: 76.5 + i as f32 * 0.5,
                        y: 73.5,
                    }),
                    presentation,
                )?;
                bind(id)?;
                if i == 0 {
                    w.players.get_mut(&id).unwrap().controller =
                        crate::world::Controller::Autonomous;
                }
                Ok(id)
            })
            .await
            .unwrap();
        ids.push(id);
    }
    let own = ids[0];
    let peer = ids[1];
    app.world
        .in_world(-2)
        .call(move |w| {
            w.updates(own, true)?;
            w.updates(peer, true)
        })
        .await
        .unwrap();
    let v = app
        .world
        .directory(move |d| d.player_view(own))
        .await
        .unwrap();
    let character = v.session.character;
    let mut brain = Brain::new(PROFILES[3], &v);
    brain.mind.market.name = "Kadabu".into();
    let now = app.world.clock.now();
    app.world
        .directory(move |d| d.chat(peer, "Kadabu, spare health?".into(), now))
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut rx[0]).await;
    assert!(tick(&app, &mut brain).await);
    app.world
        .directory(move |d| d.chat(peer, "Kadabu, yes health".into(), now))
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut rx[0]).await;
    assert!(tick(&app, &mut brain).await);
    app.world
        .in_world(-2)
        .call(move |w| w.request_trade(peer, "Kadabu".into()))
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut rx[0]).await;
    let gift = brain.mind.social.gift.take();
    assert!(!cancel_gift(&app, &mut brain).await.unwrap());
    brain.mind.social.gift = gift;
    app.world
        .in_world(-2)
        .call(move |w| w.change_trade(peer, [false; 12]))
        .await
        .expect("a non-gift publication failure must not revoke an unrelated native trade");
    uptake(&app, &mut brain, &mut rx[0]).await;
    tick(&app, &mut brain).await;
    if matches!(ending, Ending::RejectNativeAccept) {
        let trade = brain.mind.market.trade.as_mut().unwrap();
        trade.ready = 0;
        trade.ready_at = app.world.clock.now();
        assert!(!tick(&app, &mut brain).await);
        assert!(
            brain.mind.social.gift.is_none(),
            "failed World Accept must leave no acceptance witness"
        );
        assert!(
            app.world
                .in_world(-2)
                .call(move |w| w.accept_trade(peer, [false; 12], [false; 12]))
                .await
                .is_err()
        );
        assert!(!brain.mind.social.known(2));
        assert_eq!(
            app.store
                .load(1, character)
                .await
                .unwrap()
                .items
                .iter()
                .filter(|k| **k == HEALTH)
                .count(),
            4
        );
        let db = rusqlite::Connection::open_with_flags(
            dir.join("game.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM trade_transactions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        for id in ids.iter().copied() {
            crate::server::depart(&app, id).await.unwrap();
        }
        app.shutdown.send_replace(true);
        drop(owners);
        std::fs::write(dir.join("gift-accept-rejection-result.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"focused premature native Accept guard; no outcome repaired","mind":brain.mind,"owners":app.owners.lock().unwrap().len()})).unwrap()).unwrap();
        println!("gift failed-accept evidence: {}", dir.display());
        return;
    }
    app.world
        .in_world(-2)
        .call(move |w| {
            w.change_trade(peer, [false; 12])?;
            w.now += 3000;
            Ok(())
        })
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut rx[0]).await;
    app.world
        .in_world(-2)
        .call(|w| {
            w.now += 3000;
            Ok(())
        })
        .await
        .unwrap();
    settle_trade_clock(&app).await;
    tick(&app, &mut brain).await;
    let original = brain
        .mind
        .social
        .gift
        .as_ref()
        .unwrap()
        .acceptance
        .clone()
        .unwrap();
    assert!(!app.store.gift_committed(original.clone()).await.unwrap());
    let serialized = serde_json::to_string(&brain.mind.social).unwrap();
    assert!(!serialized.contains(&original.transaction));
    assert!(
        serde_json::from_str::<Life>(&serialized)
            .unwrap()
            .gift
            .is_none()
    );
    app.world
        .in_world(-2)
        .call(move |w| w.change_trade(peer, [false; 12]))
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut rx[0]).await;
    assert!(
        brain
            .mind
            .social
            .gift
            .as_ref()
            .unwrap()
            .acceptance
            .is_none(),
        "native offer change revokes the old acceptance"
    );
    assert!(
        !app.world
            .in_world(-2)
            .call(move |w| w.gift_acceptance_current(&original))
            .await
            .unwrap()
    );
    app.world
        .in_world(-2)
        .call(|w| {
            w.now += 3000;
            Ok(())
        })
        .await
        .unwrap();
    settle_trade_clock(&app).await;
    tick(&app, &mut brain).await;
    let acceptance = brain
        .mind
        .social
        .gift
        .as_ref()
        .unwrap()
        .acceptance
        .clone()
        .unwrap();
    let mut giving = [false; 12];
    giving[4] = true;
    for peer_binding in [
        (acceptance.peer.account + 100, "Recipient".into()),
        (acceptance.peer.account, "Other".into()),
    ] {
        let session = acceptance.session;
        let error = app
            .world
            .in_world(-2)
            .call(move |w| {
                w.autonomous_gift(
                    own,
                    session,
                    peer,
                    peer_binding,
                    true,
                    crate::world::TradeAction::Accept {
                        mine: giving,
                        theirs: [false; 12],
                    },
                )
            })
            .await
            .err()
            .expect("foreign observed peer must be rejected before native acceptance");
        assert!(error.to_string().contains("counterparty binding changed"));
    }
    let plan = if commit_first {
        Some(
            app.world
                .in_world(-2)
                .call(move |w| w.accept_trade(peer, [false; 12], giving))
                .await
                .unwrap()
                .unwrap(),
        )
    } else {
        None
    };
    app.world
        .directory(move |d| d.chat(peer, "Kadabu, no thanks".into(), now))
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut rx[0]).await;
    let retained = tick(&app, &mut brain).await;
    if let Some(plan) = plan {
        assert!(retained, "a real pending commit must not be wished away");
        crate::server::persist_trade(&app, plan).await.unwrap();
        assert!(app.store.gift_committed(acceptance.clone()).await.unwrap());
        let mut missing = acceptance.clone();
        missing.transaction = "0".repeat(32);
        assert!(!app.store.gift_committed(missing).await.unwrap());
        let mut bad = Vec::new();
        let mut a = acceptance.clone();
        a.transaction.clear();
        bad.push(a);
        let mut a = acceptance.clone();
        a.own.revision += 1;
        bad.push(a);
        let mut a = acceptance.clone();
        a.peer.character += 100;
        bad.push(a);
        let mut a = acceptance.clone();
        a.peer.revision += 1;
        bad.push(a);
        let mut a = acceptance.clone();
        a.own.character += 100;
        bad.push(a);
        let mut a = acceptance.clone();
        a.peer.account += 100;
        bad.push(a);
        let mut a = acceptance.clone();
        a.writes_hash[0] ^= 1;
        bad.push(a);
        let mut a = acceptance.clone();
        a.giving = !a.giving;
        bad.push(a);
        for a in bad {
            assert!(
                app.store.gift_committed(a).await.is_err(),
                "foreign/stale/corrupt receipt must be explicit rejection"
            );
        }
        let packets = std::iter::from_fn(|| rx[0].try_recv().ok()).collect::<Vec<_>>();
        let completion = packets.iter().find(|p| p.id == 12).unwrap();
        let saved = brain.mind.social.gift.clone();
        let current = app
            .world
            .directory(move |d| d.player_view(own))
            .await
            .unwrap();
        let mut malformed = Vec::new();
        let mut a = acceptance.clone();
        a.session[1] += 100;
        malformed.push(a);
        let mut a = acceptance.clone();
        a.peer_player += 100;
        malformed.push(a);
        let mut a = acceptance.clone();
        a.peer_name = "Other".into();
        malformed.push(a);
        for a in malformed {
            brain.mind.social.gift.as_mut().unwrap().acceptance = Some(a);
            assert!(
                receive_life(&app, &mut brain, completion, &current)
                    .await
                    .is_err()
            );
            assert!(!brain.mind.social.known(2));
        }
        brain.mind.social.gift = saved;
        for packet in &packets {
            uptake_packet(&app, &mut brain, packet).await;
        }
        uptake_packet(&app, &mut brain, completion).await;
        assert_eq!(
            brain
                .mind
                .social
                .episodes
                .iter()
                .filter(|e| e.interaction == Interaction::GaveHealth)
                .count(),
            1,
            "a replayed native result cannot credit the same gift twice"
        );
        assert!(
            brain
                .mind
                .social
                .episodes
                .iter()
                .any(|e| e.interaction == Interaction::GaveHealth)
        );
        assert_eq!(
            app.store
                .load(1, character)
                .await
                .unwrap()
                .items
                .iter()
                .filter(|k| **k == HEALTH)
                .count(),
            3
        );
    } else {
        assert!(!retained);
        assert!(
            app.world
                .in_world(-2)
                .call(move |w| w.accept_trade(peer, [false; 12], giving))
                .await
                .is_err(),
            "withdrawal must revoke the prior native accept, not just forget the gift"
        );
        assert!(
            brain
                .mind
                .social
                .episodes
                .iter()
                .all(|e| e.interaction != Interaction::GaveHealth)
        );
        assert_eq!(
            app.store
                .load(1, character)
                .await
                .unwrap()
                .items
                .iter()
                .filter(|k| **k == HEALTH)
                .count(),
            4
        );
    }
    for id in ids.iter().copied() {
        crate::server::depart(&app, id).await.unwrap();
    }
    app.shutdown.send_replace(true);
    drop(owners);
    std::fs::write(dir.join("gift-revocation-result.json"),serde_json::to_vec_pretty(&serde_json::json!({"fixture":"focused native-acceptance authority test, not actual controller behavior evidence","commit_first":commit_first,"mind":brain.mind,"owners":app.owners.lock().unwrap().len()})).unwrap()).unwrap();
}
#[tokio::test]
async fn community_social_gift_withdrawal_revokes_latched_native_acceptance_before_late_accept() {
    withdrawal(Ending::WithdrawBeforeCommit).await;
}
#[tokio::test]
async fn community_social_gift_commit_winning_withdrawal_fence_is_remembered_not_undone() {
    withdrawal(Ending::CommitBeforeWithdrawal).await;
}
#[tokio::test]
async fn community_social_gift_failed_native_accept_cannot_create_a_receipt() {
    withdrawal(Ending::RejectNativeAccept).await;
}

#[tokio::test]
async fn community_social_gift_unrelated_pending_commit_cannot_credit_a_new_offer() {
    let (dir, app) = crate::server::fixture();
    app.world.clock.set_paused(true);
    let mut actors = Vec::new();
    let mut receivers = Vec::new();
    let mut owners = Vec::new();
    for (i, name) in ["Kadabu", "ActualDonor", "Offerer"].into_iter().enumerate() {
        let account = app
            .store
            .list(format!("receipt-binding-{i}"))
            .await
            .unwrap()
            .id;
        let mut ch = app
            .store
            .create_prepared(
                account,
                app.content.get(crate::content::WIZARD).unwrap(),
                Some(name.into()),
            )
            .await
            .unwrap();
        ch.items[4..].fill(-1);
        if i > 0 {
            ch.items[4..8].fill(HEALTH);
        }
        ch.revision = app
            .store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
        app.store.progress(ch.clone()).await.unwrap();
        owners.push(app.claim(account).unwrap());
        let (packets, rx) = mpsc::channel(512);
        let (lost, _) = watch::channel(false);
        receivers.push(rx);
        let presentation = app.store.presentation(account).await.unwrap();
        let bind = crate::server::lifecycle::binding(&app, account).unwrap();
        let character = ch.id;
        let player = app
            .world
            .in_world(-2)
            .call(move |w| {
                let id = w.enter_at(
                    ch,
                    Some(Output { packets, lost }),
                    Some(Pos {
                        x: 76.5 + i as f32 * 0.5,
                        y: 73.5,
                    }),
                    presentation,
                )?;
                bind(id)?;
                if i == 0 {
                    w.players.get_mut(&id).unwrap().controller =
                        crate::world::Controller::Autonomous;
                }
                Ok(id)
            })
            .await
            .unwrap();
        actors.push((player, account, character));
    }
    let (own, account, character) = actors[0];
    let (donor, donor_account, donor_character) = actors[1];
    let (offerer, offerer_account, offerer_character) = actors[2];
    let ids = [own, donor, offerer];
    app.world
        .in_world(-2)
        .call(move |w| {
            for id in ids {
                w.updates(id, true)?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let view = app
        .world
        .directory(move |d| d.player_view(own))
        .await
        .unwrap();
    let mut brain = Brain::new(PROFILES[3], &view);
    brain.mind.market.name = "Kadabu".into();
    let mut health_offer = [false; 12];
    health_offer[4] = true;
    app.world
        .in_world(-2)
        .call(move |w| {
            w.request_trade(own, "ActualDonor".into())?;
            w.request_trade(donor, "Kadabu".into())?;
            w.change_trade(own, [false; 12])?;
            w.change_trade(donor, health_offer)
        })
        .await
        .unwrap();
    settle_trade_clock(&app).await;
    let plan = app
        .world
        .in_world(-2)
        .call(move |w| {
            assert!(w.accept_trade(own, [false; 12], health_offer)?.is_none());
            w.accept_trade(donor, health_offer, [false; 12])?
                .context("unrelated native commit was not reserved")
        })
        .await
        .unwrap();
    let transaction = plan.transaction.clone();
    let received = app.world.clock.now();
    app.world
        .directory(move |d| d.chat(offerer, "Kadabu, take health".into(), received))
        .await
        .unwrap();
    uptake(&app, &mut brain, &mut receivers[0]).await;
    assert_eq!(
        brain.mind.social.gift.as_ref().unwrap().account,
        offerer_account
    );
    crate::server::persist_trade(&app, plan).await.unwrap();
    uptake(&app, &mut brain, &mut receivers[0]).await;
    let own_after = app.store.load(account, character).await.unwrap();
    let donor_after = app
        .store
        .load(donor_account, donor_character)
        .await
        .unwrap();
    let offerer_after = app
        .store
        .load(offerer_account, offerer_character)
        .await
        .unwrap();
    let count = |ch: &crate::store::Character| ch.items.iter().filter(|k| **k == HEALTH).count();
    assert_eq!(
        (
            count(&own_after),
            count(&donor_after),
            count(&offerer_after)
        ),
        (1, 3, 4)
    );
    std::fs::write(dir.join("unrelated-gift-receipt-result.json"), serde_json::to_vec_pretty(&serde_json::json!({"scope":"focused real Directory text, native pending trade and Store completion; not ordinary controller/native scene proof","actual_transaction":transaction,"actual_donor":donor_account,"offerer":offerer_account,"own":own_after,"donor":donor_after,"offerer_after":offerer_after,"mind":brain.mind})).unwrap()).unwrap();
    println!("gift unrelated-commit evidence: {}", dir.display());
    assert!(
        !brain.mind.social.known(offerer_account),
        "a different account's committed health must not credit the new offerer"
    );
    assert!(
        !brain
            .mind
            .social
            .outgoing
            .as_ref()
            .is_some_and(|s| s.contains("thanks"))
    );
    for id in ids {
        crate::server::depart(&app, id).await.unwrap();
    }
    app.shutdown.send_replace(true);
    drop(owners);
}
