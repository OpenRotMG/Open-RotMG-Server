use super::*;
use crate::wire::Writer;

#[tokio::test]
async fn incoming_refusal_counter_is_funded_and_expires_with_offer_contact_or_own_state() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app,
        "counter-748",
        "Trader",
        [SPEED, SPEED, SPEED, SPEED, SPEED, -1, -1, -1],
    )
    .await;
    let (peer_owner, peer_id, _peer_output, _peer_brain) =
        crate::bots::runtime::tests::actor(&app, "counter-peer-748", "Buyer", [-1; 8]).await;
    app.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&peer_id).unwrap().pos = w.players[&id].pos;
            w.updates(id, true)?;
            Ok(())
        })
        .await
        .unwrap();
    let mut v = app.world.player_view(id).await.unwrap();
    let mut market = Market {
        name: "Trader".into(),
        ..Market::default()
    };
    let q = market.quote(&v, &app.content, None).unwrap().unwrap();
    market.quote = Some(q.clone());
    let mut text = Writer::new();
    text.utf("Buyer")
        .i32(peer_id)
        .i32(0)
        .u8(5)
        .utf("")
        .utf("wtb spd")
        .utf("");
    assert!(
        market
            .receive(
                &Packet {
                    id: wire::TEXT,
                    body: text.0
                },
                &v,
                &app.content,
                None
            )
            .unwrap()
    );
    assert!(
        market.incoming.is_none(),
        "bare interest is not a firm quote or native consent"
    );
    let interest_reply = market
        .outgoing
        .take()
        .expect("own funded counter may answer interest without assuming peer stock");
    assert_eq!(
        interest_reply.text,
        format!(
            "nty wts {} for {}",
            goods_text(&q[0], &app.content).unwrap(),
            goods_text(&q[1], &app.content).unwrap()
        )
    );
    assert!(
        market
            .message_current(&interest_reply, &v, &app.content, None)
            .unwrap()
    );
    let counter_text = |text: &str| {
        let mut w = Writer::new();
        w.utf("Buyer")
            .i32(peer_id)
            .i32(0)
            .u8(5)
            .utf("Trader")
            .utf(text)
            .utf("");
        Packet {
            id: wire::TEXT,
            body: w.0,
        }
    };
    for refusal in [
        "nty",
        "nah",
        "nty need 1 def",
        "nty selling def",
        "can do free life",
    ] {
        assert!(
            !market
                .receive(&counter_text(refusal), &v, &app.content, None)
                .unwrap()
        );
        assert!(market.incoming.is_none());
    }
    for prefix in ["nty", "nah", "can do", "no thx", "sry"] {
        let text = format!(
            "{prefix} {} for {}",
            goods_text(&q[1], &app.content).unwrap(),
            goods_text(&q[0], &app.content).unwrap()
        );
        assert!(
            market
                .receive(&counter_text(&text), &v, &app.content, None)
                .unwrap()
        );
        assert!(
            matches!(market.incoming_action(&v,&app.content,None).unwrap(),Some(crate::world::TradeAction::Request { name }) if name=="Buyer")
        );
        assert!(
            market.trade.is_none() && market.pending_trade.is_none(),
            "a complete counter only invites native inspection, even when peer stock is empty"
        );
        market.incoming = None;
    }
    market.incoming_request("Buyer".into(), &v);
    let mut offer = [false; 12];
    offer[4] = true;
    let mut their_items = vec![-1; 12];
    their_items[4] = 0xa22;
    market.trade = Some(SessionTrade {
        binding: [v.session.world, id, v.session.character],
        peer: "Buyer".into(),
        items: [v.character.items.clone(), their_items],
        tradeable: [true; 12],
        offers: [[false; 12], offer],
        ready: 0,
        ready_at: v.clock_at,
        empty_offer_until: v.clock_at,
        changed: false,
    });
    assert!(matches!(
        market.incoming_action(&v, &app.content, None).unwrap(),
        Some(crate::world::TradeAction::Change { .. })
    ));
    v.time_ms += 3001;
    v.clock_at += Duration::from_millis(3001);
    let mut silent = Market {
        name: "Buyer".into(),
        ..market.clone()
    };
    assert!(matches!(
        silent.incoming_action(&v, &app.content, None).unwrap(),
        Some(crate::world::TradeAction::Cancel)
    ));
    assert!(
        silent.outgoing.is_none(),
        "most sellers close a bad window without a word"
    );
    assert!(matches!(
        market.incoming_action(&v, &app.content, None).unwrap(),
        Some(crate::world::TradeAction::Cancel)
    ));
    let refusal = market.outgoing.take().expect("this seller says nty");
    assert_eq!(
        (refusal.text.as_str(), &refusal.channel),
        (
            format!("nty need {}", goods_text(&q[1], &app.content).unwrap()).as_str(),
            &Channel::Local
        )
    );
    market.end_incoming(&v);
    market.trade = None;
    assert!(
        market
            .message_current(&refusal, &v, &app.content, None)
            .unwrap()
    );
    let reply = interest_reply;
    assert!(
        market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.character.inventory_revision += 1;
    assert!(
        !market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.character.inventory_revision -= 1;
    v.objects.retain(|p| p.id != peer_id);
    assert!(
        !market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        v.position.x = value;
        assert!(
            market
                .message_current(&reply, &v, &app.content, None)
                .is_err(),
            "own geometry failure must precede missing peer cancellation"
        );
    }
    drop((owner, peer_owner, app));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn incoming_full_inventory_retains_quote_for_existing_vault_preparation_not_a_promise() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app,
        "capacity-748",
        "Trader",
        [DEFENSE, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22],
    )
    .await;
    let (peer_owner, peer_id, _peer_output, _peer_brain) =
        crate::bots::runtime::tests::actor(&app, "capacity-peer-748", "Seller", [-1; 8]).await;
    app.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&peer_id).unwrap().pos = w.players[&id].pos;
            let speed = w.content.classes[&w.players[&id].character.class].base[4];
            w.players.get_mut(&id).unwrap().character.base[4] = speed;
            w.updates(id, true)?;
            Ok(())
        })
        .await
        .unwrap();
    let mut v = app.world.player_view(id).await.unwrap();
    assert!(
        v.objects.iter().any(|p| p.kind == 0x720 && p.usable),
        "fixture observes an actual Vault portal"
    );
    let mut market = Market {
        name: "Trader".into(),
        ..Market::default()
    };
    market.quote = Some(
        bounded_quote(
            &Goods {
                kind: DEFENSE,
                quantity: 1,
            },
            &Goods {
                kind: SPEED,
                quantity: 8,
            },
            &app.content,
        )
        .expect("researched whole DEF/SPD terms for the capacity fixture"),
    );
    let q = market.quote(&v, &app.content, None).unwrap().unwrap();
    assert_eq!(q[0].kind, DEFENSE);
    assert_eq!(q[1].kind, SPEED);
    let grid = Grid::terrain(&v);
    let mut rng = rand::rngs::mock::StepRng::new(u64::MAX, 0);
    assert!(
        market
            .decide(&v, &app.content, None, &grid, &mut rng)
            .unwrap()
            .is_none()
    );
    assert!(
        market.session.is_none() && market.session_used && !market.preparing,
        "ordinary nontrader actually declined this visit"
    );
    assert_eq!(market.quote, Some(q.clone()));
    market.incoming_request("Seller".into(), &v);
    market.trade = Some(SessionTrade {
        binding: [v.session.world, id, v.session.character],
        peer: "Seller".into(),
        items: [v.character.items.clone(), vec![-1; 12]],
        tradeable: [true; 12],
        offers: [[false; 12]; 2],
        ready: 0,
        ready_at: v.clock_at,
        empty_offer_until: v.clock_at + Duration::from_secs(20),
        changed: false,
    });
    assert!(matches!(
        market.incoming_action(&v, &app.content, None).unwrap(),
        Some(crate::world::TradeAction::Cancel)
    ));
    assert!(market.preparing, "existing market journey owns banking");
    assert_eq!(market.quote, Some(q));
    let reply = market.outgoing.clone().expect("truthful capacity reply");
    assert_eq!(reply.text, "full inv");
    assert!(
        market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.character.inventory_revision += 1;
    assert!(
        !market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.character.inventory_revision -= 1;
    market.end_incoming(&v);
    market.trade = None;
    let portal = v
        .objects
        .iter()
        .find(|p| p.kind == 0x720 && p.usable)
        .unwrap()
        .id;
    let decision=market.decide(&v,&app.content,None,&grid,&mut rng).unwrap().expect("selected capacity preparation must reach the existing Vault route after declined admission");
    assert_eq!(decision.goal, Goal::Portal(portal));
    assert!(
        decision.movement.is_some()
            || matches!(decision.journey,Some(Journey::Portal(id)) if id==portal)
    );
    assert!(
        market.session.is_none() && market.session_used,
        "preparation must not reopen advertising"
    );
    let mut expired = Market {
        name: "Trader".into(),
        trader: true,
        ..Market::default()
    };
    let mut admitted_rng = rand::rngs::StdRng::seed_from_u64(748);
    assert!(
        expired
            .decide(&v, &app.content, None, &grid, &mut admitted_rng)
            .unwrap()
            .is_some()
    );
    let until = expired.session.as_ref().unwrap().until;
    let elapsed = v.elapsed_ms;
    v.elapsed_ms = until;
    let decision = expired
        .decide(&v, &app.content, None, &grid, &mut admitted_rng)
        .unwrap()
        .expect("selected preparation precedes ad-session expiry");
    assert_eq!(decision.goal, Goal::Portal(portal));
    assert!(
        decision.movement.is_some()
            || matches!(decision.journey,Some(Journey::Portal(id)) if id==portal)
    );
    assert_eq!(
        expired.session.as_ref().unwrap().until,
        until,
        "banking never renews the ad-session deadline"
    );
    v.elapsed_ms = elapsed;
    let items = v.character.items.clone();
    v.character.items[5..].fill(-1);
    assert!(
        market
            .decide(&v, &app.content, None, &grid, &mut rng)
            .unwrap()
            .is_none()
    );
    assert!(
        !market.preparing && market.session.is_none() && market.session_used,
        "available room releases preparation without restarting ads"
    );
    v.elapsed_ms = until;
    assert!(
        expired
            .decide(&v, &app.content, None, &grid, &mut admitted_rng)
            .unwrap()
            .is_none()
    );
    assert!(
        !expired.preparing && expired.session.is_none(),
        "completed preparation must not revive an expired ad session"
    );
    v.elapsed_ms = elapsed;
    v.character.items = items;
    for remove in [false, true] {
        if remove {
            v.objects.retain(|p| p.kind != 0x720);
        } else {
            for p in &mut v.objects {
                if p.kind == 0x720 {
                    p.usable = false;
                }
            }
        }
        market.preparing = true;
        assert!(
            market
                .decide(&v, &app.content, None, &grid, &mut rng)
                .unwrap()
                .is_none()
        );
        assert!(
            !market.preparing && !market.reserves_stock(),
            "no usable observed Vault must release preparation and reserve"
        );
        assert!(
            !market
                .message_current(&reply, &v, &app.content, None)
                .unwrap()
        );
    }
    v.character.items[5..].fill(-1);
    assert!(
        !market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    drop((owner, peer_owner, app));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn thanks_requires_exact_receipt_but_survives_ordinary_revision_advance_until_original_deadline()
 {
    use crate::store::{InventoryWrite, TradeWitness};
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app,
        "thanks-748",
        "Trader",
        [SPEED, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    let (peer_owner, peer_id, _peer_output, _peer_brain) = crate::bots::runtime::tests::actor(
        &app,
        "thanks-peer-748",
        "Buyer",
        [DEFENSE, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    app.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&peer_id).unwrap().pos = w.players[&id].pos;
            w.updates(id, true)?;
            Ok(())
        })
        .await
        .unwrap();
    let mut v = app.world.player_view(id).await.unwrap();
    let peer = app.world.player_view(peer_id).await.unwrap();
    let mut mine = v.character.items.clone();
    mine[4] = DEFENSE;
    let mut theirs = peer.character.items.clone();
    theirs[4] = SPEED;
    let witness = TradeWitness {
        transaction: "c".repeat(32),
        writes: vec![
            InventoryWrite {
                id: v.session.character,
                account: v.session.account,
                revision: v.character.inventory_revision,
                items: mine.clone(),
            },
            InventoryWrite {
                id: peer.session.character,
                account: peer.session.account,
                revision: peer.character.inventory_revision,
                items: theirs,
            },
        ],
    };
    let mut market = Market {
        name: "Trader".into(),
        ..Market::default()
    };
    market.incoming_request("Buyer".into(), &v);
    let pending = replies::PendingTrade {
        contact: market.incoming.take().unwrap(),
        witness: witness.clone(),
    };
    market.pending_trade = Some(pending.clone());
    v.character.items = mine;
    v.character.inventory_revision += 1;
    market
        .recover_trade_reply(&app.store, &v, &app.content)
        .await
        .unwrap();
    assert!(
        market.outgoing.is_none(),
        "even matching holdings/revision do not prove settlement"
    );
    app.store
        .exchange_trade(witness.transaction.clone(), witness.writes.clone(), None)
        .await
        .unwrap();
    // Ordinary consumption can supersede the receipt snapshot before the first recovery.
    v.character.items[4] = -1;
    v.character.inventory_revision += 1;
    v.character.base_stats[3] += 1;
    let objects = std::mem::take(&mut v.objects);
    let mut wrong = pending.clone();
    wrong.witness.writes[0].items[4] = 0xa22;
    market.pending_trade = Some(wrong);
    assert!(
        market
            .recover_trade_reply(&app.store, &v, &app.content)
            .await
            .is_err(),
        "wrong receipt stays an error even after peer departure"
    );
    v.objects = objects;
    market.pending_trade = Some(pending);
    market
        .recover_trade_reply(&app.store, &v, &app.content)
        .await
        .unwrap();
    assert!(market.pending_trade.is_none());
    let reply = market
        .outgoing
        .clone()
        .expect("verified past exchange may be acknowledged");
    assert_eq!(reply.text, "ty");
    assert!(
        market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.character.inventory_revision += 1;
    assert!(
        market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    for changed in 0..3 {
        match changed {
            0 => v.session.character += 1,
            1 => v.session.player += 1,
            _ => v.session.world -= 1,
        }
        assert!(
            !market
                .message_current(&reply, &v, &app.content, None)
                .unwrap()
        );
        match changed {
            0 => v.session.character -= 1,
            1 => v.session.player -= 1,
            _ => v.session.world += 1,
        }
    }
    let objects = std::mem::take(&mut v.objects);
    assert!(
        !market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.objects = objects;
    v.elapsed_ms += 19_999;
    assert!(
        market
            .message_current(&reply, &v, &app.content, None)
            .unwrap()
    );
    v.character.inventory_revision += 1;
    v.elapsed_ms += 1;
    market
        .recover_trade_reply(&app.store, &v, &app.content)
        .await
        .unwrap();
    assert!(
        !market
            .message_current(&reply, &v, &app.content, None)
            .unwrap(),
        "later revisions never renew the original deadline"
    );
    let mut malformed = witness;
    malformed.transaction = "invalid".into();
    market
        .respond(
            &v,
            (peer_id, peer.session.account, "Buyer".into()),
            replies::ReplyKind::Thanks(malformed),
            &app.content,
        )
        .unwrap();
    let malformed_reply = market.outgoing.clone().unwrap();
    v.objects.clear();
    assert!(
        market
            .message_current(&malformed_reply, &v, &app.content, None)
            .is_err(),
        "malformed receipt precedes contextual cancellation"
    );
    drop((owner, peer_owner, app));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vault_trip_keeps_the_nexus_market_visit_until_an_outing() {
    let (_, mut v) = crate::bots::quest_tests::fixture();
    let mut market = Market {
        session: Some(MarketSession {
            anchor: (1, 1),
            until: u64::MAX,
            next_move: 0,
            destination: None,
        }),
        session_used: true,
        ..Market::default()
    };
    v.map.name = "Vault".into();
    market.observe(&v);
    // Every world entry reloads the durable mind, which drops the process-local visit.
    let mut reloaded = Market::default();
    reloaded.bind();
    reloaded.continue_visit(&market);
    v.map.name = "Nexus".into();
    reloaded.observe(&v);
    assert!(reloaded.session.is_some() && reloaded.session_used);
    v.map.name = "Realm".into();
    reloaded.observe(&v);
    assert!(reloaded.session.is_none() && !reloaded.session_used);
}

#[tokio::test]
async fn funded_bots_settle_through_native_exchange() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (ring, staff) = (2749, 2719);
    let (seller_owner, seller, mut seller_packets, mut seller_brain) =
        crate::bots::runtime::tests::actor(
            &app,
            "native-market-seller",
            "Seller",
            [ring, ring, staff, staff, -1, -1, -1, -1],
        )
        .await;
    let (buyer_owner, buyer, mut buyer_packets, mut buyer_brain) =
        crate::bots::runtime::tests::actor(
            &app,
            "native-market-buyer",
            "Buyer",
            [SPEED, SPEED, -1, -1, -1, -1, -1, -1],
        )
        .await;
    let goods = |kind| Goods { kind, quantity: 1 };
    seller_brain.mind.market.quote = Some([goods(ring), goods(SPEED)]);
    let clock_at = app.world.clock.now();
    app.world
        .directory(move |d| {
            let w = d.worlds.get_mut(&-2).unwrap();
            w.players.get_mut(&buyer).unwrap().pos = w.players[&seller].pos;
            w.updates(seller, true)?;
            w.updates(buyer, true)?;
            d.chat(
                seller,
                "wts Ring of Superior Health for spd".into(),
                clock_at,
            )
        })
        .await
        .unwrap();

    let mut packets = [0; 4];
    let actors = [seller, buyer];
    let before = [
        app.world.player_view(seller).await.unwrap(),
        app.world.player_view(buyer).await.unwrap(),
    ];
    app.world.clock.set_paused(false);
    let mut offers_settled = false;
    for _ in 0..12 {
        for (id, output, brain) in [
            (buyer, &mut buyer_packets, &mut buyer_brain),
            (seller, &mut seller_packets, &mut seller_brain),
        ] {
            let view = app.world.player_view(id).await.unwrap();
            while let Ok(packet) = output.try_recv() {
                match packet.id {
                    61 => packets[0] += 1,
                    67 => packets[1] += 1,
                    23 => packets[2] += 1,
                    12 => {
                        assert_eq!(Reader::new(&packet.body).i32().unwrap(), 0);
                        packets[3] += 1;
                    }
                    _ => {}
                }
                crate::bots::social::receive_market(&app, brain, &packet, &view)
                    .await
                    .unwrap();
            }
            crate::bots::social::market(&app, brain, &view)
                .await
                .unwrap();
        }
        if packets[3] == 2 {
            break;
        }
        // Both native endpoints must actually publish their offers before the settling delay.
        if packets[2] == 2 && !offers_settled {
            tokio::time::sleep(Duration::from_millis(3150)).await;
            offers_settled = true;
        }
    }
    assert_eq!(
        packets,
        [1, 2, 2, 2],
        "requests, window endpoints, offer updates, successful endpoints"
    );
    for (i, id) in actors.into_iter().enumerate() {
        let after = app.world.player_view(id).await.unwrap();
        let saved = app
            .store
            .load(after.session.account, after.session.character)
            .await
            .unwrap();
        assert_eq!(
            after.character.inventory_revision,
            before[i].character.inventory_revision + 1
        );
        assert_eq!(saved.revision, after.character.inventory_revision);
        assert_eq!(saved.items, after.character.items);
        assert_eq!(saved.items[4], [SPEED, ring][i]);
    }
    let view = app.world.player_view(buyer).await.unwrap();
    assert!(
        buyer_brain
            .mind
            .market
            .accepts(
                &view,
                &app.content,
                None,
                &[goods(staff), goods(SPEED)],
                None
            )
            .unwrap(),
        "buyer still qualifies for a different fair sale"
    );
    let clock_at = app.world.clock.now();
    app.world
        .directory(move |d| d.chat(seller, "wts Staff of Horror for spd".into(), clock_at))
        .await
        .unwrap();
    while let Ok(packet) = buyer_packets.try_recv() {
        crate::bots::social::receive_market(&app, &mut buyer_brain, &packet, &view)
            .await
            .unwrap();
    }
    assert!(
        buyer_brain.mind.market.incoming.is_none(),
        "recent peers are excluded before requesting"
    );
    let session = [view.session.world, buyer, view.session.character];
    let seller_account = before[0].session.account;
    let repeated = app
        .world
        .for_player(buyer)
        .call(move |w| {
            w.autonomous_incoming_trade(
                buyer,
                session,
                seller,
                (seller_account, "Seller".into()),
                crate::world::TradeAction::Request {
                    name: "Seller".into(),
                },
                Some([goods(SPEED), goods(staff)]),
            )
            .map(|_| ())
        })
        .await;
    assert_eq!(
        repeated.unwrap_err().to_string(),
        "autonomous market pair has a recent committed exchange"
    );
    drop((seller_owner, buyer_owner, app));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn vault_funds_preserve_the_sellers_fair_exact_price() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.map.name = "Nexus".into();
    v.session.world = -2;
    let mut seller = v.objects.remove(0);
    seller.account = Some(2);
    seller.name = "Seller".into();
    seller.equipped = vec![0xa97, -1, -1, -1];
    seller.position = v.position;
    let seller_id = seller.id;
    let portal = crate::world::view::VisibleObject {
        id: seller_id + 1,
        account: None,
        kind: 0x720,
        name: "Vault Portal".into(),
        position: v.position,
        hp: 0,
        max_hp: 0,
        level: None,
        equipped: vec![],
        contents: vec![],
        blocks: false,
        usable: true,
        condition_mask: None,
    };
    v.objects = vec![seller, portal];
    let mut vault = vec![DEFENSE; 4];
    vault.resize(8, -1);
    let mut market = Market {
        vault: Some(vault.clone()),
        ..Default::default()
    };
    let mut packet = Writer::new();
    packet
        .utf("Seller")
        .i32(seller_id)
        .i32(0)
        .u8(5)
        .utf("")
        .utf("wts Staff of Astral Knowledge for 4 def")
        .utf("");
    let ad = Packet {
        id: wire::TEXT,
        body: packet.0,
    };
    assert!(market.receive(&ad, &v, &c, None).unwrap());
    assert!(
        market.incoming.is_none(),
        "Vault memory is not a funded match"
    );
    let mut rng = rand::rngs::StdRng::seed_from_u64(744);
    let grid = Grid::terrain(&v);
    let to_vault = market
        .decide(&v, &c, None, &grid, &mut rng)
        .unwrap()
        .unwrap();
    assert!(matches!(to_vault.journey, Some(Journey::Portal(_))));
    v.map.name = "Vault".into();
    let seller = v.objects.remove(0);
    let mut chest = v.objects.pop().unwrap();
    chest.kind = 0x504;
    chest.contents = vault;
    v.objects = vec![chest];
    for expected in 0..4 {
        let withdraw = market
            .decide(&v, &c, None, &grid, &mut rng)
            .unwrap()
            .unwrap();
        let Some(Action::Swap { from, to }) = withdraw.item else {
            panic!("must withdraw all four at the advertised price; only withdrew {expected}");
        };
        assert_eq!(from.kind, DEFENSE);
        v.character.items[to.slot] = from.kind;
        v.objects[0].contents[from.slot] = -1;
    }
    assert!(matches!(
        market
            .decide(&v, &c, None, &grid, &mut rng)
            .unwrap()
            .unwrap()
            .journey,
        Some(Journey::Nexus)
    ));
    v.map.name = "Nexus".into();
    v.objects = vec![seller];
    assert!(market.receive(&ad, &v, &c, None).unwrap());
    assert!(
        matches!(market.incoming_action(&v, &c, None).unwrap(), Some(crate::world::TradeAction::Request { name }) if name == "Seller")
    );
}
