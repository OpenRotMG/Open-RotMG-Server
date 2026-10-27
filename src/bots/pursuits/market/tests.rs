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

#[test]
fn item_funds_ignore_cached_vault_and_prepare_only_live_owned_stock() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    let mut chest = v.objects.remove(0);
    v.objects.clear();
    v.character.items[4..6].fill(DEFENSE);
    let mut m = Market {
        vault: Some(vec![DEFENSE; 8]),
        ..Market::default()
    };
    m.observe_goal(&v, &c, None, Some(2794)).unwrap();
    assert!(m.quote(&v, &c, None).unwrap().is_none());
    let grid = Grid::terrain(&v);
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
    assert_eq!(m.target, Some(2794));
    assert_eq!(
        m.resource_payload().unwrap()["reason"],
        "saving_continue_hunt_loot"
    );
    v.map.name = "Vault".into();
    chest.id = 42;
    chest.kind = 0x504;
    chest.position = v.position;
    chest.contents = vec![DEFENSE, -1, -1, -1, -1, -1, -1, -1];
    v.objects.push(chest);
    let buy = [goods(DEFENSE, 3), goods(2794, 1)];
    assert_eq!(m.quote(&v, &c, None).unwrap(), Some(buy.clone()));
    let d = m.decide(&v, &c, None, &grid, &mut rng).unwrap().unwrap();
    assert!(
        matches!(&d.item, Some(Action::Swap { from, to }) if from.object == 42 && from.kind == DEFENSE && to.kind == -1)
    );
    assert!(cctv::reason(crate::bots::Activity::Trade, &d, &v, &c).contains("withdrawing"));
    assert_eq!(
        v.character.items[6], -1,
        "selection is not a committed withdrawal"
    );
    v.character.items[6] = DEFENSE;
    v.character.inventory_revision += 1;
    v.objects[0].contents[0] = -1;
    let d = m.decide(&v, &c, None, &grid, &mut rng).unwrap().unwrap();
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    v.map.name = "Nexus".into();
    v.objects.clear();
    assert_eq!(m.quote(&v, &c, None).unwrap(), Some(buy));
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_some());
    assert!(
        m.session.is_some() && !m.trader,
        "funded purpose bypasses random persona admission"
    );
    v.character.items[6] = -1;
    assert!(
        m.quote(&v, &c, None).unwrap().is_none(),
        "no optimistic wallet after preparation"
    );
}

#[test]
fn goal_reserves_surplus_currency_not_personal_potion_keeps() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.objects.clear();
    v.character.items[4..8].copy_from_slice(&[DEFENSE, DEFENSE, 2613, SPEED]);
    let mut m = Market::default();
    m.observe_goal(&v, &c, None, Some(2794)).unwrap();
    assert_eq!(m.spendable_surplus(&v, &c, None, DEFENSE).unwrap(), 0);
    assert_eq!(Market::verified_surplus(&v, &c, None, 2613).unwrap(), 0);
    assert!(
        matches!(
            progression::consume(&v, &c, &m, None).unwrap(),
            Some(Action::Use { slot: 6, .. })
        ),
        "personal Wisdom remains usable while actual DEF/SPD savings are protected"
    );
    v.character.items[8] = DEFENSE;
    assert!(matches!(
        m.resources(&v, &c, None).unwrap().unwrap().state,
        AcquisitionState::Funded { .. }
    ));
    assert_eq!(m.spendable_surplus(&v, &c, None, SPEED).unwrap(), 1);
    assert!(
        m.can_use(&v, &c, None, SPEED).unwrap(),
        "funded reservation is not a blanket cash lock"
    );
    assert!(!m.can_use(&v, &c, None, DEFENSE).unwrap());
}

#[test]
fn maxed_liquid_loot_can_replace_junk_but_not_reserved_cash_or_locked_quotes() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    let mut bag = v.objects.remove(0);
    v.objects.clear();
    v.character.base_stats[4] = c.classes[&v.character.class].caps[4];
    v.character.items[4..].copy_from_slice(&[DEFENSE, DEFENSE, 2613, 2692, 2692, 2692, 2692, 2692]);
    bag.kind = 0x500;
    bag.contents = vec![SPEED];
    let mut m = Market::default();
    m.observe_goal(&v, &c, None, Some(2720)).unwrap();
    assert!(
        progression::pickup(&v, &c, &bag, None, None)
            .unwrap()
            .is_none()
    );
    let action = progression::pickup(&v, &c, &bag, None, Some(&m)).unwrap();
    assert!(
        matches!(action, Some(Action::Swap { from, to }) if from.kind == SPEED && to.kind == 2692)
    );
    let quote = [goods(DEFENSE, 1), goods(SPEED, 3)];
    m.quote = Some(quote.clone());
    m.incoming = Some(Incoming::outbound(&v, 9, 2, "Peer".into(), quote.clone()));
    m.observe_goal(&v, &c, None, Some(2794)).unwrap();
    assert_eq!(m.quote, Some(quote.clone()));
    assert_eq!(m.incoming.as_ref().unwrap().quote, Some(quote));
    assert_eq!(m.spendable_surplus(&v, &c, None, DEFENSE).unwrap(), 0);
    assert!(progression::consume(&v, &c, &m, None).unwrap().is_none());
    assert!(
        progression::pickup(&v, &c, &bag, None, Some(&m))
            .unwrap()
            .is_none()
    );
}
