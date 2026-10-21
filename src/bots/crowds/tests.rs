use super::*;
use crate::bots::tests::{content, object, view};
use crate::content::WIZARD;

fn publish(v: &mut PlayerView) {
    v.player_markers = v
        .objects
        .iter()
        .filter_map(|o| {
            o.account.map(|account| PlayerMarker {
                id: o.id,
                account,
                name: o.name.clone(),
                position: o.position,
            })
        })
        .collect();
}
fn crowd(v: &mut PlayerView) {
    v.map.width = 128;
    for (id, x) in [(2, 48.5), (3, 49.5), (4, 50.5)] {
        let mut p = object(id, WIZARD, x, 11.5);
        p.name = format!("Independent{id}");
        v.objects.push(p);
    }
    publish(v);
    v.objects.clear();
}
fn advance(v: &mut PlayerView) {
    v.tick += 10;
    v.time_ms += 500;
}

#[test]
fn community_crowd_knowledge_attention_and_travel_change_discovery_not_resources() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    b.crowd.knows_minimap = false;
    assert!(!matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Regroup(_))
    ));
    advance(&mut v);
    b.crowd.knows_minimap = true;
    b.mind.cognition.attention = 1;
    assert!(!matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Regroup(_))
    ));
    advance(&mut v);
    b.mind.cognition.attention = 8;
    v.map.allow_teleport = false;
    assert!(b.decide(&v, &c).unwrap().journey.is_none());
    advance(&mut v);
    v.map.allow_teleport = true;
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Regroup(2..=4))
    ));
    advance(&mut v);
    assert!(
        b.decide(&v, &c).unwrap().journey.is_none(),
        "own teleport attempts are paced"
    );
}
#[test]
fn community_crowd_unnoticed_and_stale_contacts_cannot_supply_a_destination() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    b.mind.cognition.attention = 1;
    let mut hazard = object(99, 0x205, 25.5, 11.5);
    hazard.hp = 100;
    v.objects.push(hazard);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert_eq!(b.crowd.len(), 0);
    assert!(b.crowd.destination(&v, None).is_none());
    v.objects.clear();
    b.mind.cognition.attention = 8;
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(b.crowd.destination(&v, None).is_some());
    advance(&mut v);
    assert!(
        b.crowd.destination(&v, None).is_none(),
        "unrefreshed positions are not live tracking"
    );
    v.player_markers.clear();
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert_eq!(b.crowd.len(), 0);
    assert!(b.decide(&v, &c).unwrap().journey.is_none());
    let binding = (v.session.world, v.session.player, v.session.character);
    for changed in [
        (binding.0 + 1, binding.1, binding.2),
        (binding.0, binding.1 + 1, binding.2),
        (binding.0, binding.1, binding.2 + 1),
    ] {
        (v.session.world, v.session.player, v.session.character) = changed;
        assert!(b.decide(&v, &c).is_err());
    }
    let fresh = Brain::new(PROFILES[1], &v);
    assert_eq!(
        fresh.crowd.len(),
        0,
        "world/session replacement never inherits action handles"
    );
}
#[test]
fn community_crowd_unsolicited_private_expired_and_departure_chat_are_not_orders() {
    let c = content();
    let mut v = view(&c);
    for (recipient, text) in [
        ("", "tp here"),
        ("Biceps", "nexus"),
        ("", "pc here"),
        ("", "Biceps follow me"),
    ] {
        let mut w = wire::Writer::new();
        w.utf("Unseen")
            .i32(999)
            .i32(0)
            .u8(5)
            .utf(recipient)
            .utf(text)
            .utf("");
        let mut b = Brain::new(PROFILES[1], &v);
        assert!(
            !b.receive(
                Packet {
                    id: wire::TEXT,
                    body: w.0
                },
                &v,
                true
            )
            .unwrap()
        );
        assert!(b.companion.is_none());
        assert!(b.decide(&v, &c).unwrap().journey.is_none());
    }
    let mut p = object(2, WIZARD, 20.5, 11.5);
    p.equipped = v.character.items[..4].to_vec();
    v.objects.push(p);
    let mut b = Brain::new(PROFILES[1], &v);
    b.companion = Some(2);
    b.invitation_until = v.time_ms;
    assert!(!matches!(b.decide(&v, &c).unwrap().goal, Goal::Follow(2)));
    assert!(b.companion.is_none());
}
#[test]
fn community_crowd_own_recovery_supply_and_fight_outrank_company() {
    let c = content();
    let mut v = view(&c);
    v.character.items[4..].fill(-1);
    crowd(&mut v);
    let mut bag = object(9, 0x500, 12.5, 11.5);
    bag.contents = vec![0xa22, 0xa22];
    v.objects.push(bag);
    let mut b = Brain::new(PROFILES[1], &v);
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Loot(9));
    assert!(d.journey.is_none());
    advance(&mut v);
    v.character.hp = 1;
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Nexus)
    ));
    let mut v = view(&c);
    crowd(&mut v);
    v.objects.push(object(8, 0x205, 15.5, 11.5));
    let mut b = Brain::new(PROFILES[1], &v);
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Fight(8));
}
#[test]
fn community_crowd_independent_edges_and_visitor_exit_do_not_choose_a_leader() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut targets = BTreeSet::new();
    for account in 10..16 {
        v.session.account = account;
        let mut b = Brain::new(PROFILES[1], &v);
        if let Some(Journey::Regroup(id)) = b.decide(&v, &c).unwrap().journey {
            targets.insert(id);
        }
    }
    assert!(targets.len() >= 2);
    let visitor = PlayerMarker {
        id: 10,
        account: 10,
        name: "Visitor".into(),
        position: Pos { x: 12.5, y: 11.5 },
    };
    v.player_markers = vec![visitor.clone()];
    let mut b = Brain::new(PROFILES[1], &v);
    assert!(
        b.decide(&v, &c).unwrap().journey.is_none(),
        "a visitor alone is not a crowd"
    );
    advance(&mut v);
    crowd(&mut v);
    v.player_markers.push(visitor);
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Regroup(2..=4))
    ));
    v.tick += 220;
    v.time_ms += 11000;
    v.player_markers.retain(|p| p.id != 10);
    assert!(
        matches!(
            b.decide(&v, &c).unwrap().journey,
            Some(Journey::Regroup(2..=4))
        ),
        "visitor exit cannot cancel an independently discovered outing"
    );
}

fn visible_contact(v: &mut PlayerView, id: i32) {
    let p = v.player_markers.iter().find(|p| p.id == id).unwrap();
    v.position = Pos {
        x: p.position.x - 6.,
        y: p.position.y,
    };
    v.objects
        .push(object(p.id, WIZARD, p.position.x, p.position.y));
    advance(v);
}

#[test]
fn community_crowd_approach_selection_without_reached_outcome_is_not_completion() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    let Goal::Follow(peer) = b.decide(&v, &c).unwrap().goal else {
        panic!("passive crowd approach not selected")
    };
    assert_eq!(b.crowd.approaching.unwrap().0, peer);
    advance(&mut v);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(
        b.crowd.completed.is_empty(),
        "selection/rejected movement cannot grant contact"
    );
    assert_eq!(b.crowd.destination(&v, None).unwrap().id, peer);
}

#[test]
fn community_crowd_approach_observed_live_contact_fulfills_company_not_shadowing() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    let Goal::Follow(peer) = b.decide(&v, &c).unwrap().goal else {
        panic!("passive crowd approach not selected")
    };
    visible_contact(&mut v, peer);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(b.crowd.completed.contains(&(peer, peer)));
    assert!(b.crowd.approaching.is_none());
    assert_ne!(
        b.crowd.destination(&v, None).unwrap().id,
        peer,
        "a different unfinished contact remains possible"
    );
    advance(&mut v);
    v.position.x -= 8.;
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert_ne!(
        b.crowd.destination(&v, None).unwrap().id,
        peer,
        "ordinary separation does not reopen fulfilled passive company"
    );
}

#[test]
fn community_crowd_approach_unselected_proximity_does_not_complete_activity() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    visible_contact(&mut v, 2);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(b.crowd.approaching.is_none());
    assert!(b.crowd.completed.is_empty());
}

#[test]
fn community_crowd_approach_hidden_dead_or_wrong_incarnation_is_not_completion() {
    let c = content();
    for invalid in ["hidden", "dead", "wrong-account", "departed", "unattended"] {
        let mut v = view(&c);
        crowd(&mut v);
        let mut b = Brain::new(PROFILES[1], &v);
        let Goal::Follow(peer) = b.decide(&v, &c).unwrap().goal else {
            panic!("passive crowd approach not selected")
        };
        visible_contact(&mut v, peer);
        match invalid {
            "hidden" => v.objects.clear(),
            "dead" => v.objects[0].hp = 0,
            "wrong-account" => v.objects[0].account = Some(peer + 100),
            "departed" => v.player_markers.retain(|p| p.id != peer),
            "unattended" => {
                b.mind.cognition.attention = 1;
                v.objects
                    .push(object(99, 0x205, v.position.x + 1., v.position.y));
            }
            _ => unreachable!(),
        }
        b.crowd.attend(&v, &c, &b.mind.cognition, false);
        assert!(b.crowd.completed.is_empty(), "{invalid}");
        if invalid == "departed" {
            assert!(b.crowd.approaching.is_none());
        }
    }
}

#[test]
fn community_crowd_approach_departure_new_player_and_own_session_reset_contact() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    let Goal::Follow(peer) = b.decide(&v, &c).unwrap().goal else {
        panic!("passive crowd approach not selected")
    };
    visible_contact(&mut v, peer);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(!b.crowd.completed.is_empty());
    v.player_markers
        .iter_mut()
        .find(|p| p.id == peer)
        .unwrap()
        .id += 100;
    v.objects[0].id += 100;
    advance(&mut v);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(
        b.crowd.completed.is_empty(),
        "departed action handles never suppress a new incarnation"
    );
    b.crowd.approach_selected(peer + 100);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(b.crowd.completed.contains(&(peer + 100, peer)));
    v.session.world -= 1;
    assert!(b.decide(&v, &c).is_err());
    assert!(Brain::new(PROFILES[1], &v).crowd.completed.is_empty());
}

#[test]
fn community_crowd_approach_completion_does_not_override_explicit_company() {
    let c = content();
    let mut v = view(&c);
    crowd(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    let Goal::Follow(peer) = b.decide(&v, &c).unwrap().goal else {
        panic!("passive crowd approach not selected")
    };
    visible_contact(&mut v, peer);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    b.companion = Some(peer);
    b.invitation_until = v.time_ms + 5000;
    v.position = Pos { x: 11.5, y: 11.5 };
    v.objects[0].position = Pos { x: 22.5, y: 11.5 };
    advance(&mut v);
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Follow(peer));
    assert!(
        b.crowd.approaching.is_none(),
        "explicit company is not a new passive approach"
    );
}
