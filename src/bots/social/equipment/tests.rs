use super::*;
use crate::world::{Pos, view::VisibleObject};

const LONG_SWORD: i32 = 0xa01;
const CRYSTAL_SWORD: i32 = 0xa03;
const WARRIOR: u16 = 0x31d;

fn fixture() -> (Content, PlayerView, Market, Policy) {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    let mut peer = v.objects.remove(0);
    peer.id = 3;
    peer.account = Some(2);
    peer.kind = WARRIOR;
    peer.name = "GuestVisible".into();
    peer.position = Pos { x: 11., y: 10.5 };
    peer.hp = 500;
    peer.max_hp = 500;
    peer.level = Some(20);
    peer.condition_mask = None;
    peer.equipped = vec![0xa00, -1, -1, -1];
    v.objects.push(peer);
    v.character.items[4] = LONG_SWORD;
    v.character.inventory_revision = 17;
    (
        c,
        v,
        Market::default(),
        Policy {
            future_class: None,
            current_goal: None,
            willingness: 6.,
            available: true,
        },
    )
}

#[test]
fn useful_public_upgrade_binds_real_backpack_slot_revision_and_identity_without_mutation() {
    let (c, v, market, policy) = fixture();
    assert!(
        c.item_values.quote_range(LONG_SWORD).is_err(),
        "unknown price is not zero or exclusion"
    );
    let before = serde_json::to_value(&v).unwrap();
    let market_before = market.clone();
    let candidate = select(&v, &c, &market, &policy).unwrap().unwrap();
    assert_eq!(
        (candidate.slot, candidate.kind, candidate.inventory_revision),
        (4, LONG_SWORD, 17)
    );
    assert_eq!(
        (
            candidate.world,
            candidate.own_account,
            candidate.own_character,
            candidate.own_player
        ),
        (v.session.world, 1, 1, 1)
    );
    assert_eq!(
        (
            candidate.recipient_player,
            candidate.recipient_account,
            candidate.recipient_name.as_str()
        ),
        (3, 2, "GuestVisible")
    );
    assert_eq!(
        candidate.reason,
        Reason::PublicUpgrade {
            equipped_slot: 0,
            previous: 0xa00
        }
    );
    assert_eq!(serde_json::to_value(&v).unwrap(), before);
    assert_eq!(market, market_before);
}

#[test]
fn willingness_availability_personal_future_and_current_goal_controls_keep_goods() {
    let (c, mut v, market, mut policy) = fixture();
    policy.willingness = 5.;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    policy.willingness = 6.;
    policy.available = false;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    policy.available = true;
    policy.current_goal = Some(LONG_SWORD);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    policy.current_goal = None;
    policy.future_class = Some(WARRIOR);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    policy.future_class = None;
    v.character.class = WARRIOR;
    v.character.items[..4].copy_from_slice(&[0xa00, -1, -1, -1]);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
}

#[test]
fn real_acquisition_sale_reservation_and_locked_native_trade_prevent_selection() {
    let (c, mut v, mut market, policy) = fixture();
    v.character.items[4] = CRYSTAL_SWORD;
    v.objects[0].equipped[0] = -1;
    assert!(select(&v, &c, &market, &policy).unwrap().is_some());
    market.observe_goal(&v, &c, None, Some(2794)).unwrap();
    assert_eq!(
        market
            .spendable_surplus(&v, &c, None, CRYSTAL_SWORD)
            .unwrap(),
        0
    );
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    market.observe_goal(&v, &c, None, None).unwrap();
    market.trade = Some(crate::bots::pursuits::market::SessionTrade {
        binding: [v.session.world, v.session.player, v.session.character],
        peer: "GuestVisible".into(),
        items: [v.character.items.clone(), vec![-1; 12]],
        tradeable: [true; 12],
        offers: [[false; 12]; 2],
        ready: 0,
        ready_at: v.clock_at,
        empty_offer_until: v.clock_at,
        changed: false,
    });
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
}

#[test]
fn only_real_carried_slots_not_equipped_cached_vault_or_observed_chest_are_candidates() {
    let (c, mut v, mut market, policy) = fixture();
    v.character.items[4] = -1;
    let mut saved = serde_json::to_value(&market).unwrap();
    saved["vault"] = serde_json::json!([LONG_SWORD, -1, -1, -1, -1, -1, -1, -1]);
    market = serde_json::from_value(saved).unwrap();
    let mut chest: VisibleObject = fixture().1.objects.remove(0);
    chest.id = 4;
    chest.account = None;
    chest.kind = 0x504;
    chest.name = c.get(chest.kind).unwrap().name.clone();
    chest.contents = vec![LONG_SWORD; 8];
    chest.equipped.clear();
    v.objects.push(chest);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.map.name = "Vault".into();
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.map.name = "Realm".into();
    v.character.class = WARRIOR;
    v.character.items[..4].copy_from_slice(&[LONG_SWORD, -1, -1, -1]);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.character.items[8] = LONG_SWORD;
    v.character.inventory_revision += 1;
    assert_eq!(
        market
            .spendable_surplus(&v, &c, policy.future_class, LONG_SWORD)
            .unwrap(),
        1
    );
    let candidate = select(&v, &c, &market, &policy).unwrap().unwrap();
    assert_eq!(
        (candidate.slot, candidate.kind, candidate.inventory_revision),
        (8, LONG_SWORD, 18)
    );
}

#[test]
fn unsupported_soulbound_resurrection_consumables_and_unknown_effects_are_not_gifts() {
    let (mut c, mut v, market, policy) = fixture();
    c.objects.get_mut(&(LONG_SWORD as u16)).unwrap().soulbound = true;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    c.objects.get_mut(&(LONG_SWORD as u16)).unwrap().soulbound = false;
    c.objects
        .get_mut(&(LONG_SWORD as u16))
        .unwrap()
        .unsupported_equipment = true;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    c.objects
        .get_mut(&(LONG_SWORD as u16))
        .unwrap()
        .unsupported_equipment = false;
    c.objects
        .get_mut(&(LONG_SWORD as u16))
        .unwrap()
        .projectiles
        .clear();
    assert!(
        select(&v, &c, &market, &policy).unwrap().is_none(),
        "tier alone is not quality"
    );
    v.character.items[4] = 0xb3e;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.character.items[4..8].fill(0xa22);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
}

#[test]
fn public_class_missing_gear_and_noncomparable_patterns_do_not_guess_need() {
    let (mut c, mut v, market, policy) = fixture();
    v.objects[0].kind = crate::content::WIZARD;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects[0].kind = WARRIOR;
    v.objects[0].equipped[0] = -1;
    let candidate = select(&v, &c, &market, &policy).unwrap().unwrap();
    assert_eq!(
        candidate.reason,
        Reason::MissingEquipment { equipped_slot: 0 }
    );
    v.objects[0].equipped[0] = 0xa00;
    c.objects.get_mut(&(LONG_SWORD as u16)).unwrap().shots += 1;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    c.objects.get_mut(&(LONG_SWORD as u16)).unwrap().shots -= 1;
    c.objects
        .get_mut(&(LONG_SWORD as u16))
        .unwrap()
        .projectiles
        .get_mut(&0)
        .unwrap()
        .speed += 1.;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects[0].equipped[0] = 65535;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
}

#[test]
fn stat_upgrades_are_componentwise_and_peer_hidden_contents_are_irrelevant() {
    let (c, mut v, market, policy) = fixture();
    v.character.items[4] = 0xa10;
    v.objects[0].equipped[2] = 0xa0f;
    assert_eq!(
        select(&v, &c, &market, &policy).unwrap().unwrap().reason,
        Reason::PublicUpgrade {
            equipped_slot: 2,
            previous: 0xa0f
        }
    );
    v.character.items[4] = 0xa4f;
    v.character.items[3] = 0xa25;
    v.objects[0].equipped[3] = -1;
    assert!(
        select(&v, &c, &market, &policy).unwrap().is_some(),
        "untiered actual bonus is known"
    );
    v.objects[0].contents = vec![0xa4f, 0xb3e];
    assert!(
        select(&v, &c, &market, &policy).unwrap().is_some(),
        "no hidden inventory inference"
    );
    v.objects[0].equipped[3] = 0xa24;
    assert!(
        select(&v, &c, &market, &policy).unwrap().is_none(),
        "defense gain does not erase attack loss"
    );
}

#[test]
fn unsafe_dead_self_ambiguous_and_unsettled_targets_produce_no_claim() {
    let (c, mut v, market, policy) = fixture();
    v.objects[0].hp = 0;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects[0].hp = 500;
    v.objects[0].account = Some(v.session.account);
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects[0].account = None;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects[0].account = Some(2);
    v.objects.push(fixture().1.objects.remove(0));
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects.pop();
    v.objects[0].position.x = 100.;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.objects[0].position.x = 11.;
    v.character.hp = 100;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
    v.character.hp = 500;
    v.lifecycle.busy = true;
    assert!(select(&v, &c, &market, &policy).unwrap().is_none());
}

#[test]
fn invalid_own_bindings_and_classes_are_errors_not_fallbacks() {
    let (c, mut v, market, mut policy) = fixture();
    policy.future_class = Some(65535);
    assert!(select(&v, &c, &market, &policy).is_err());
    policy.future_class = None;
    v.character.class = 65535;
    assert!(select(&v, &c, &market, &policy).is_err());
    v.character.class = crate::content::WIZARD;
    v.character.items[4] = 65536;
    assert!(select(&v, &c, &market, &policy).is_err());
    v.character.items[4] = LONG_SWORD;
    v.character.inventory_revision = -1;
    assert!(select(&v, &c, &market, &policy).is_err());
}
