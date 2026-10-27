use super::*;
use crate::wire::Writer;

#[tokio::test]
async fn incoming_refusal_counter_is_funded_and_expires_with_offer_contact_or_own_state() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app, "counter-748", "Trader", [SPEED, SPEED, SPEED, SPEED, SPEED, -1, -1, -1],
    ).await;
    let (peer_owner, peer_id, _peer_output, _peer_brain) = crate::bots::runtime::tests::actor(
        &app, "counter-peer-748", "Buyer", [-1; 8],
    ).await;
    app.world.for_player(id).call(move |w| {
        w.players.get_mut(&peer_id).unwrap().pos = w.players[&id].pos;
        w.updates(id, true)?;
        Ok(())
    }).await.unwrap();
    let mut v = app.world.player_view(id).await.unwrap();
    let mut market = Market { name: "Trader".into(), ..Market::default() };
    let q = market.quote(&v, &app.content, None).unwrap().unwrap();
    market.quote = Some(q.clone());
    let mut text = Writer::new();
    text.utf("Buyer").i32(peer_id).i32(0).u8(5).utf("").utf("wtb spd").utf("");
    assert!(market.receive(&Packet { id: wire::TEXT, body: text.0 }, &v, &app.content, None).unwrap());
    assert!(market.incoming.is_none(), "bare interest is not a firm quote or native consent");
    let interest_reply = market.outgoing.take().expect("own funded counter may answer interest without assuming peer stock");
    assert_eq!(interest_reply.text, format!("nty wts {} for {}", goods_text(&q[0], &app.content).unwrap(), goods_text(&q[1], &app.content).unwrap()));
    assert!(market.message_current(&interest_reply, &v, &app.content, None).unwrap());
    market.incoming_request("Buyer".into(), &v);
    let mut offer = [false; 12];
    offer[4] = true;
    let mut their_items = vec![-1; 12];
    their_items[4] = 0xa22;
    market.trade = Some(SessionTrade {
        binding: [v.session.world, id, v.session.character], peer: "Buyer".into(),
        items: [v.character.items.clone(), their_items], tradeable: [true; 12],
        offers: [[false; 12], offer], ready: 0, ready_at: v.clock_at,
        empty_offer_until: v.clock_at, changed: false,
    });
    assert!(matches!(market.incoming_action(&v, &app.content, None).unwrap(), Some(crate::world::TradeAction::Change { .. })));
    v.time_ms += 3001;
    v.clock_at += Duration::from_millis(3001);
    assert!(matches!(market.incoming_action(&v, &app.content, None).unwrap(), Some(crate::world::TradeAction::Cancel)));
    let reply = market.outgoing.clone().expect("refusal with own funded counteroffer");
    assert_eq!(reply.text, format!("nty wts {} for {}", goods_text(&q[0], &app.content).unwrap(), goods_text(&q[1], &app.content).unwrap()));
    market.end_incoming(&v);
    market.trade = None;
    assert!(market.message_current(&reply, &v, &app.content, None).unwrap());
    v.character.inventory_revision += 1;
    assert!(!market.message_current(&reply, &v, &app.content, None).unwrap());
    v.character.inventory_revision -= 1;
    v.objects.retain(|p| p.id != peer_id);
    assert!(!market.message_current(&reply, &v, &app.content, None).unwrap());
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        v.position.x = value;
        assert!(market.message_current(&reply, &v, &app.content, None).is_err(), "own geometry failure must precede missing peer cancellation");
    }
    drop((owner, peer_owner, app));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn incoming_full_inventory_retains_quote_for_existing_vault_preparation_not_a_promise() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app, "capacity-748", "Trader", [DEFENSE, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22, 0xa22],
    ).await;
    let (peer_owner, peer_id, _peer_output, _peer_brain) = crate::bots::runtime::tests::actor(
        &app, "capacity-peer-748", "Seller", [-1; 8],
    ).await;
    app.world.for_player(id).call(move |w| {
        w.players.get_mut(&peer_id).unwrap().pos = w.players[&id].pos;
        w.updates(id, true)?;
        Ok(())
    }).await.unwrap();
    let mut v = app.world.player_view(id).await.unwrap();
    v.character.base_stats[4] = 0;
    assert!(v.objects.iter().any(|p| p.kind == 0x720 && p.usable), "fixture observes an actual Vault portal");
    let mut market = Market { name: "Trader".into(), ..Market::default() };
    let q = market.quote(&v, &app.content, None).unwrap().unwrap();
    assert_eq!(q[0].kind, DEFENSE);
    assert_eq!(q[1].kind, SPEED);
    market.quote = Some(q.clone());
    market.incoming_request("Seller".into(), &v);
    market.trade = Some(SessionTrade {
        binding: [v.session.world, id, v.session.character], peer: "Seller".into(),
        items: [v.character.items.clone(), vec![-1; 12]], tradeable: [true; 12],
        offers: [[false; 12]; 2], ready: 0, ready_at: v.clock_at,
        empty_offer_until: v.clock_at + Duration::from_secs(20), changed: false,
    });
    assert!(matches!(market.incoming_action(&v, &app.content, None).unwrap(), Some(crate::world::TradeAction::Cancel)));
    assert!(market.preparing, "existing market journey owns banking");
    assert_eq!(market.quote, Some(q));
    let reply = market.outgoing.clone().expect("truthful capacity reply");
    assert_eq!(reply.text, "full inv");
    assert!(market.message_current(&reply, &v, &app.content, None).unwrap());
    v.character.items[5..].fill(-1);
    assert!(!market.message_current(&reply, &v, &app.content, None).unwrap());
    drop((owner, peer_owner, app));
    std::fs::remove_dir_all(dir).unwrap();
}
