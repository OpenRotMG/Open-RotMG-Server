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
    let mut market = Market::default();
    market.incoming = Some(Incoming::outbound(
        &view,
        peer_id,
        account,
        "Buyer".into(),
        quote,
    ));
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
