use super::*;
use std::path::Path;

fn goods(kind: i32, quantity: u8) -> Goods {
    Goods { kind, quantity }
}

#[test]
fn quotes_and_fairness_share_ranges_and_native_whole_item_bounds() {
    let c = Content::load(Path::new("data")).unwrap();
    let mana = bounded_quote(&goods(2794, 1), &goods(DEFENSE, 8), &c).unwrap();
    assert_eq!(mana, [goods(2794, 1), goods(DEFENSE, 3)]);
    assert!(fair(&mana, &c));
    assert!(fair(&[goods(2794, 1), goods(DEFENSE, 4)], &c));
    assert!(!fair(&[goods(2794, 1), goods(DEFENSE, 5)], &c));
    assert!(bounded_quote(&goods(2794, 1), &goods(DEFENSE, 2), &c).is_none());
    assert_eq!(
        bounded_quote(&goods(SPEED, 8), &goods(DEFENSE, 1), &c),
        Some([goods(SPEED, 3), goods(DEFENSE, 1)])
    );
    assert!(!fair(&[goods(SPEED, 5), goods(DEFENSE, 1)], &c));
    assert!(fair(&[goods(2719, 1), goods(SPEED, 1)], &c));
    assert!(!fair(&[goods(2719, 3), goods(SPEED, 4)], &c));
    assert!(
        !fair(&[goods(2719, 4), goods(2585, 3)], &c),
        "shared SPD uncertainty cancels between two one-SPD items"
    );
    assert!(!fair(&[goods(DEFENSE, 5), goods(LIFE, 1)], &c));
    assert!(fair(&[goods(DEFENSE, 7), goods(LIFE, 1)], &c));
    // Crystal Sword's catalog value replaces the old one/two-potion tier override.
    assert_eq!(
        bounded_quote(&goods(2563, 1), &goods(DEFENSE, 8), &c),
        Some([goods(2563, 1), goods(DEFENSE, 3)])
    );
    assert!(!fair(&[goods(2563, 1), goods(DEFENSE, 1)], &c));
    assert!(bounded_quote(&goods(1826, 1), &goods(DEFENSE, 16), &c).is_none());
    assert_eq!(
        bounded_quote(&goods(1826, 1), &goods(LIFE, 8), &c),
        Some([goods(1826, 1), goods(LIFE, 2)])
    );
    for kind in [2612, 2721, 2727, 2806, 2784, 2878, 65535, -1] {
        assert!(bounded_quote(&goods(kind, 8), &goods(DEFENSE, 8), &c).is_none());
        assert!(!fair(&[goods(DEFENSE, 1), goods(kind, 1)], &c));
    }
    assert!(!fair(&[goods(DEFENSE, 0), goods(LIFE, 1)], &c));
    assert!(!fair(&[goods(DEFENSE, 14), goods(LIFE, 2)], &c));
    assert!(parse_quote("wts 1 Crystal Sword for 3 def", &c).is_some());
    assert!(parse_quote("wts 1 Crystal Sword for 1 def + 1 life", &c).is_none());
    assert!(
        parse_quote("wts 1 dia for 3 def", &c).is_none(),
        "research aliases are not universal decoders"
    );
}

#[tokio::test]
async fn live_quote_uses_only_actual_spare_inventory_and_shared_values() {
    let (dir, app) = crate::server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, _output, _brain) = crate::bots::runtime::tests::actor(
        &app,
        "researched-quote",
        "Trader",
        [2563, SPEED, SPEED, SPEED, -1, -1, -1, -1],
    )
    .await;
    let view = app.world.player_view(id).await.unwrap();
    let market = Market {
        name: "Trader".into(),
        ..Market::default()
    };
    let quote = market.quote(&view, &app.content, None).unwrap().unwrap();
    assert!(fair(&quote, &app.content));
    assert!(
        market
            .spare(&view, &app.content, None, quote[0].kind)
            .unwrap()
            >= quote[0].quantity
    );
    assert!(
        market
            .wanted(&view, &app.content, None, quote[1].kind)
            .unwrap()
            >= quote[1].quantity
    );
    assert_eq!(
        app.world.player_view(id).await.unwrap().character.items,
        view.character.items,
        "valuation never seeds or mutates inventory"
    );
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
