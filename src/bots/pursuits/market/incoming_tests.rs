use super::*;

#[tokio::test]
async fn incoming_rechecks_cached_quote_before_native_inspection() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (_peer_owner, peer_id, _peer_output, _peer_brain) = crate::bots::runtime::tests::actor(
        &app,
        "value-buyer",
        "Buyer",
        [SPEED, SPEED, SPEED, SPEED, SPEED, -1, -1, -1],
    )
    .await;
    let (_owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app,
        "value-seller",
        "Seller",
        [DEFENSE, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    let mut view = app.world.player_view(id).await.unwrap();
    let peer = view.objects.iter().find(|p| p.id == peer_id).unwrap();
    let account = peer.account.unwrap();
    view.position = peer.position;
    let quote = [
        Goods {
            kind: DEFENSE,
            quantity: 1,
        },
        Goods {
            kind: SPEED,
            quantity: 5,
        },
    ];
    let mut market = Market {
        incoming: Some(Incoming::outbound(
            &view,
            peer_id,
            account,
            "Buyer".into(),
            quote,
        )),
        ..Market::default()
    };
    assert!(matches!(
        market.incoming_action(&view, &app.content, None).unwrap(),
        Some(crate::world::TradeAction::Cancel)
    ));
    let quote = [
        Goods {
            kind: DEFENSE,
            quantity: 1,
        },
        Goods {
            kind: SPEED,
            quantity: 3,
        },
    ];
    market.incoming = Some(Incoming::outbound(
        &view,
        peer_id,
        account,
        "Buyer".into(),
        quote,
    ));
    assert!(matches!(
        market.incoming_action(&view, &app.content, None).unwrap(),
        Some(crate::world::TradeAction::Request { .. })
    ));
    assert_eq!(
        app.world.player_view(id).await.unwrap().character.items,
        view.character.items
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn issue1371_current_policy_requires_the_exact_quoted_basket() {
    use crate::world::TradeAction;
    const SHIELD: i32 = 0xa0b;
    const ATTACK: i32 = 0xa1f;
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app,
        "issue1371-seller",
        "Doc",
        [SHIELD, SHIELD, -1, -1, -1, -1, -1, -1],
    )
    .await;
    let (_peer_owner, peer_id, _peer_output, _peer_brain) = crate::bots::runtime::tests::actor(
        &app,
        "issue1371-buyer",
        "Buyer",
        [ATTACK, DEFENSE, ATTACK, SPEED, -1, -1, -1, -1],
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
    let mut view = app.world.player_view(id).await.unwrap();
    let peer = view.objects.iter().find(|p| p.id == peer_id).unwrap();
    let account = peer.account.unwrap();
    view.position = peer.position;
    let quote = parse_quote(
        "trading 2 Golden Shield for 1 Potion of Attack",
        &app.content,
    )
    .unwrap();
    assert_eq!(
        quote[0],
        Goods {
            kind: SHIELD,
            quantity: 2
        }
    );
    assert_eq!(
        quote[1],
        Goods {
            kind: ATTACK,
            quantity: 1
        }
    );
    assert!(fair(&quote, &app.content));
    assert_eq!(
        parse_quote(
            "buying 1 Potion of Attack for 2 Golden Shield",
            &app.content
        ),
        Some(quote.clone())
    );
    let their_items = app
        .world
        .player_view(peer_id)
        .await
        .unwrap()
        .character
        .items;
    assert!(!fair(
        &[
            quote[0].clone(),
            Goods {
                kind: DEFENSE,
                quantity: 1
            }
        ],
        &app.content
    ));
    println!(
        "per-item units: Golden Shield = {:?}, Attack = {:?}, Defense = {:?}",
        app.content.item_values.quote_range(SHIELD).unwrap(),
        app.content.item_values.quote_range(ATTACK).unwrap(),
        app.content.item_values.quote_range(DEFENSE).unwrap()
    );
    for (label, slots, accepts) in [
        ("exact attack", vec![4], true),
        ("attack plus defense", vec![4, 5], false),
        ("two attacks", vec![4, 6], false),
        ("defense instead", vec![5], false),
        ("speed instead", vec![7], false),
    ] {
        let mut offer = [false; 12];
        for slot in slots {
            offer[slot] = true;
        }
        let mut market = Market {
            name: "Doc".into(),
            quote: Some(quote.clone()),
            incoming: Some(Incoming::outbound(
                &view,
                peer_id,
                account,
                "Buyer".into(),
                quote.clone(),
            )),
            trade: Some(SessionTrade {
                binding: [view.session.world, id, view.session.character],
                peer: "Buyer".into(),
                items: [view.character.items.clone(), their_items.clone()],
                tradeable: [true; 12],
                offers: [[false; 12], offer],
                ready: 0,
                ready_at: view.clock_at,
                empty_offer_until: view.clock_at + Duration::from_secs(20),
                changed: false,
            }),
            ..Market::default()
        };
        assert!(
            matches!(market.incoming_action(&view, &app.content, None).unwrap(), Some(TradeAction::Change { offer }) if offer.iter().filter(|b| **b).count() == 2)
        );
        assert!(
            market
                .incoming_action(&view, &app.content, None)
                .unwrap()
                .is_none()
        );
        view.time_ms += 3001;
        assert!(
            market
                .incoming_action(&view, &app.content, None)
                .unwrap()
                .is_none()
        );
        view.clock_at += Duration::from_millis(3001);
        let action = market.incoming_action(&view, &app.content, None).unwrap();
        let outcome = match action {
            Some(TradeAction::Accept { .. }) => "Accept",
            Some(TradeAction::Cancel) => "Cancel",
            _ => "not settled",
        };
        println!("issue1371 {label}: {outcome}");
        if accepts {
            assert!(matches!(action, Some(TradeAction::Accept { mine, theirs })
                if mine[4] && mine[5] && mine.iter().filter(|b| **b).count() == 2 && theirs == offer));
        } else {
            assert!(
                matches!(action, Some(TradeAction::Cancel)),
                "{label}: {outcome}"
            );
        }
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
