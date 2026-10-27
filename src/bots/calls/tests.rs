use super::*;
use crate::world::view::PlayerMarker;

fn fixture() -> (Content, PlayerView, Brain) {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    let mut peer = v.objects.remove(0);
    v.quest = None;
    v.character.items[4] = 0xa22;
    peer.id = 3;
    peer.kind = crate::content::WIZARD;
    peer.account = Some(200);
    peer.name = "GuestObserved".into();
    peer.position = Pos { x: 22.5, y: 10.5 };
    peer.hp = 500;
    peer.max_hp = 500;
    peer.equipped = v.character.items[..4].to_vec();
    v.player_markers.push(PlayerMarker {
        id: peer.id,
        account: peer.account.unwrap(),
        name: peer.name.clone(),
        position: peer.position,
    });
    v.objects.push(peer);
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.mind
        .intentions
        .as_mut()
        .unwrap()
        .observe(&v, &c, b.profile.identity)
        .unwrap();
    (c, v, b)
}
fn pending(b: &mut Brain, v: &PlayerView) {
    b.calls.opportunity = Some(Opportunity {
        world: v.session.world,
        actor: 3,
        account: 200,
        name: "GuestObserved".into(),
        activity: Activity::Snake,
        until: v.elapsed_ms + 30_000,
    });
}

#[test]
fn public_snake_words_share_the_existing_bot_emission_meaning() {
    for text in ["snake", " Snake Pit ", "snake here", "TP SNAKE"] {
        assert_eq!(Activity::parse(text), Some(Activity::Snake));
    }
    for text in ["tp", "tp gods", "tp demon", "tp sphynix", "tp sphinx"] {
        assert!(Activity::parse(text).is_some());
    }
    for text in [
        "snake?",
        "lets do snake",
        "tp tomb",
        "snake here now",
        "buy speed",
    ] {
        assert_eq!(Activity::parse(text), None);
    }
}

#[test]
fn interested_ready_receivers_make_a_partial_choice_without_claiming_entry() {
    let (c, mut v, _) = fixture();
    let mut dispatched = 0;
    for account in 1..=32 {
        v.session.account = account;
        let mut b = Brain::new(ARCHETYPES[0], &v);
        b.mind
            .intentions
            .as_mut()
            .unwrap()
            .observe(&v, &c, b.profile.identity)
            .unwrap();
        let main = serde_json::to_value(&b.mind.intentions).unwrap();
        assert_eq!(
            b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
            None
        );
        b.receive_call(&v, &c, 3, "GuestObserved", "", "snake here")
            .unwrap();
        if b.calls.opportunity.is_some() {
            assert_eq!(b.calls.opportunity.as_ref().unwrap().name, "GuestObserved");
            assert_eq!(b.call_target(&v, &c).unwrap(), Some(3));
            assert_eq!(b.calls.last.as_ref().unwrap()["motive"], "max_speed");
            assert_eq!(
                b.calls.last.as_ref().unwrap()["reason"],
                "ordinary_teleport_requested_not_joined"
            );
            dispatched += 1;
        }
        assert_eq!(serde_json::to_value(&b.mind.intentions).unwrap(), main);
        assert_eq!(v.map.name, "Realm");
        assert!(b.call_target(&v, &c).unwrap().is_none());
    }
    assert!(
        dispatched > 0 && dispatched < 32,
        "not a compulsory crowd: {dispatched}"
    );
}

#[test]
fn own_readiness_and_existing_commitments_decline_without_changing_goals() {
    let (c, mut v, mut b) = fixture();
    let main = serde_json::to_value(&b.mind.intentions).unwrap();
    v.lifecycle.busy = true;
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("lifecycle_or_map")
    );
    v.lifecycle.busy = false;
    v.character.hp = 100;
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("health_or_weapon")
    );
    pending(&mut b, &v);
    assert_eq!(b.call_target(&v, &c).unwrap(), None);
    assert_eq!(b.calls.last.as_ref().unwrap()["reason"], "health_or_weapon");
    v.character.hp = 500;
    v.character.items[4] = -1;
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("no_health_reserve")
    );
    v.character.items[4..].fill(0xa22);
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("no_reward_space")
    );
    v.character.items[5..].fill(-1);
    b.next_teleport = v.time_ms + 1;
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("cooldown_or_recovering")
    );
    b.next_teleport = 0;
    b.companion = Some(3);
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("ordinary_commitment")
    );
    b.companion = None;
    v.map.name = "Snake Pit".into();
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("ordinary_dungeon_or_world")
    );
    assert_eq!(serde_json::to_value(&b.mind.intentions).unwrap(), main);
}

#[test]
fn speed_currency_motive_requires_unfunded_missing_target_and_safe_shared_terms() {
    let (c, mut v, mut b) = fixture();
    v.character.base_stats[4] = c.classes[&v.character.class].caps[4];
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("no_speed_need")
    );
    b.mind.market.set_target(Some(2719));
    assert!(b.mind.market.needs_resource(&v, &c, None, 0xa21).unwrap());
    assert_eq!(
        b.call_motive(&v, &c, Activity::Snake).unwrap(),
        Some("speed_for_unfunded_item")
    );
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        None
    );
    v.character.items[5] = 0xa21;
    assert!(!b.mind.market.needs_resource(&v, &c, None, 0xa21).unwrap());
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("funded_item_purchase")
    );
    pending(&mut b, &v);
    assert_eq!(b.call_target(&v, &c).unwrap(), None);
    assert_eq!(
        b.calls.last.as_ref().unwrap()["reason"],
        "funded_item_purchase"
    );
    v.character.items[5] = 2719;
    assert!(!b.mind.market.needs_resource(&v, &c, None, 0xa21).unwrap());
    v.character.items[5] = -1;
    for target in [1826, 2727, 65535] {
        b.mind.market.set_target(Some(target));
        assert!(!b.mind.market.needs_resource(&v, &c, None, 0xa21).unwrap());
    }
}

#[test]
fn stale_world_caller_and_expiry_cannot_dispatch_or_reroll() {
    let (c, mut v, mut b) = fixture();
    pending(&mut b, &v);
    v.objects.clear();
    assert_eq!(
        b.call_target(&v, &c).unwrap(),
        None,
        "marker alone is not a live visible caller"
    );
    assert_eq!(
        b.calls.last.as_ref().unwrap()["reason"],
        "identity_safety_or_distance_changed"
    );
    pending(&mut b, &v);
    v.session.world += 1;
    assert_eq!(b.call_target(&v, &c).unwrap(), None);
    assert_eq!(b.calls.last.as_ref().unwrap()["status"], "expired");
    pending(&mut b, &v);
    v.elapsed_ms += 30_000;
    assert_eq!(b.call_target(&v, &c).unwrap(), None);
    assert_eq!(b.calls.last.as_ref().unwrap()["status"], "expired");
}

#[test]
fn speaker_binding_uses_observed_guest_name_not_logical_profile_and_keeps_dedup() {
    let (c, v, mut b) = fixture();
    b.receive_call(&v, &c, 3, b.profile.name, "", "snake")
        .unwrap();
    assert_eq!(b.calls.last.as_ref().unwrap()["reason"], "unbound_speaker");
    b.receive_call(&v, &c, 3, "GuestObserved", "someone", "snake")
        .unwrap();
    assert_eq!(
        b.calls.last.as_ref().unwrap()["reason"],
        "not_public_or_current_session"
    );
    b.receive_call(&v, &c, 3, "GuestObserved", "", "snake")
        .unwrap();
    let outcome = b.calls.last.clone();
    b.receive_call(&v, &c, 3, "GuestObserved", "", "snake here")
        .unwrap();
    assert_eq!(b.calls.seen.len(), 1);
    assert_eq!(
        b.calls.last, outcome,
        "same meaning cannot reroll via another phrase"
    );
}

#[test]
fn calls_cannot_replace_ordinary_portal_intent_or_an_entered_dungeon_budget() {
    let (c, mut v, mut b) = fixture();
    let mut portal = v.objects.remove(0);
    portal.id = 9;
    portal.kind = 0x718;
    portal.usable = true;
    v.objects.push(portal);
    b.goal = Goal::Portal(9);
    assert_eq!(
        b.call_rejection(&v, &c, Activity::Snake, 200).unwrap(),
        Some("ordinary_dungeon_or_world")
    );
    b.goal = Goal::Wait;
    let i = b.mind.intentions.as_mut().unwrap();
    i.selected(&v, &c, &Goal::Portal(9), false, None, None)
        .unwrap();
    v.map.name = "Snake Pit".into();
    i.observe(&v, &c, b.profile.identity).unwrap();
    let entered = serde_json::to_value(&b.mind.intentions).unwrap();
    assert!(!b.mind.intentions.as_ref().unwrap().call_available(&v));
    pending(&mut b, &v);
    assert_eq!(b.call_target(&v, &c).unwrap(), None);
    assert_eq!(serde_json::to_value(&b.mind.intentions).unwrap(), entered);
}
