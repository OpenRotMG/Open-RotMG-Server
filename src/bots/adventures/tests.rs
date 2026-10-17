use super::*;
use crate::bots::tests::{content, object, view};
use crate::content::WIZARD;
fn member(account: i32, name: &str) -> Member {
    Member {
        account,
        name: name.into(),
    }
}
fn fixture() -> (Content, PlayerView, Brain, Society) {
    let c = content();
    let mut v = view(&c);
    v.character.level = 8;
    v.character.hp = 400;
    v.character.stats[0] = 400;
    v.character.items[..3].copy_from_slice(&[2717, 2773, 2654]);
    let mut peer = object(2, WIZARD, 13.5, 11.5);
    peer.name = "ExElite".into();
    peer.equipped = v.character.items[..4].to_vec();
    peer.level = Some(8);
    peer.hp = 400;
    peer.max_hp = 400;
    v.objects.push(peer);
    let b = Brain::new(PROFILES[1], &v);
    let mut s = Society::default();
    s.register(1, Default::default());
    s.register(2, Default::default());
    (c, v, b, s)
}
fn invite(s: &mut Society, b: &Brain, v: &PlayerView, c: &Content) -> Offer {
    let p = s.next(b, v, c, true).unwrap();
    assert!(matches!(
        p,
        Plan::Invite {
            purpose: Purpose::Experience,
            ..
        }
    ));
    s.validate(1, &p, v.elapsed_ms).unwrap();
    s.apply(
        member(1, "Biceps"),
        p,
        false,
        "ExElite, want to gain levels?".into(),
        v.elapsed_ms,
    );
    s.outgoing(1, v.elapsed_ms).unwrap()
}
#[test]
fn beginners_can_seek_equipment_companions_without_bypassing_dungeon_rules() {
    let (c, mut v, _, mut s) = fixture();
    v.character.level = 1;
    v.character.items = c.get(WIZARD).unwrap().equipment.clone();
    let mut b = Brain::new(PROFILES[1], &v);
    let plan = s.next(&b, &v, &c, true).unwrap();
    assert!(matches!(
        plan,
        Plan::Invite {
            purpose: Purpose::Equipment,
            ..
        }
    ));
    assert!(Purpose::Equipment.ready(&v, &c));
    assert!(!progression::ready(&v, &c, 0x717));
    b.proposing = true;
    let d = b.decide(&v, &c).unwrap();
    assert!(matches!(d.goal, Goal::Wait) && d.movement.is_none() && d.journey.is_none());
    b.proposing = false;
    v.tick += 5;
    v.time_ms += 250;
    v.objects.push(object(7, 0x717, 12.5, 11.5));
    assert!(!matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Portal(7))
    ));
    b.proposing = true;
    v.tick += 5;
    v.time_ms += 250;
    v.character.hp = 1;
    let d = b.decide(&v, &c).unwrap();
    assert!(
        matches!(d.goal, Goal::Recover | Goal::Dodge) || matches!(d.journey, Some(Journey::Nexus))
    );
}
#[test]
fn old_crew_names_do_not_create_a_group_and_invitations_are_not_acceptance() {
    let (c, mut v, mut b, mut s) = fixture();
    v.objects[0].name = "BattleCrus".into();
    v.objects[0].position.x = 24.5;
    assert!(b.group.is_none());
    assert!(!matches!(b.decide(&v, &c).unwrap().goal, Goal::Follow(_)));
    v.objects[0].position.x = 13.5;
    let o = invite(&mut s, &b, &v, &c);
    assert!(s.group(1, v.elapsed_ms).is_none());
    assert!(s.group(2, v.elapsed_ms).is_none());
    s.validate(2, &Plan::Answer(o.clone()), v.elapsed_ms)
        .unwrap();
    s.apply(
        member(2, "ExElite"),
        Plan::Answer(o),
        true,
        "yes".into(),
        v.elapsed_ms,
    );
    assert_eq!(s.group(1, v.elapsed_ms).unwrap().members[1].account, 2);
}
#[test]
fn a_published_invitation_waits_for_an_answer_not_forever() {
    let (c, mut v, mut b, mut s) = fixture();
    b.offered = Some(invite(&mut s, &b, &v, &c));
    v.map.name = "Nexus".into();
    v.time_ms += 10001;
    v.tick += 201;
    v.objects.push(object(7, 0x712, 12.5, 11.5));
    b.stationary = v.tick;
    b.destination = Some(Pos { x: 12.5, y: 11.5 });
    let d = b.decide(&v, &c).unwrap();
    assert!(matches!(d.goal, Goal::Wait) && d.movement.is_none() && d.journey.is_none());
    assert!(b.destination.is_none());
    v.elapsed_ms = b.offered.as_ref().unwrap().until;
    v.tick += 5;
    v.time_ms += 250;
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Portal(7))
    ));
}
#[test]
fn one_offer_has_one_response_and_never_starts_a_free_chat_loop() {
    let (c, v, b, mut s) = fixture();
    let o = invite(&mut s, &b, &v, &c);
    assert!(s.next(&b, &v, &c, true).is_none());
    let mut peer = fixture().1;
    peer.session.account = 2;
    peer.session.player = 2;
    peer.objects[0].account = Some(1);
    peer.objects[0].name = "Biceps".into();
    peer.objects[0].id = 1;
    let respondent = Brain::new(PROFILES[4], &peer);
    assert!(matches!(
        s.next(&respondent, &peer, &c, true),
        Some(Plan::Answer(_))
    ));
    assert!(s.next(&respondent, &peer, &c, true).is_none());
    s.apply(
        member(2, "ExElite"),
        Plan::Answer(o.clone()),
        true,
        "yes".into(),
        v.elapsed_ms,
    );
    assert!(s.validate(2, &Plan::Answer(o), v.elapsed_ms).is_err());
    assert!(s.next(&respondent, &peer, &c, true).is_none());
}
#[test]
fn crossed_slow_proposals_cannot_publish_two_offers_and_can_answer_once() {
    let (c, v, b, mut s) = fixture();
    let first = s.next(&b, &v, &c, true).unwrap();
    let mut other = fixture().1;
    other.session.account = 2;
    other.session.player = 2;
    other.elapsed_ms = v.elapsed_ms + 5000;
    other.objects[0].id = 3;
    other.objects[0].account = Some(3);
    other.objects[0].name = "Aster".into();
    s.register(3, Default::default());
    let recipient = Brain::new(PROFILES[4], &other);
    let crossed = s.next(&recipient, &other, &c, true).unwrap();
    s.apply(
        member(1, "Biceps"),
        first,
        false,
        "ExElite, want to gain levels?".into(),
        v.elapsed_ms + 8000,
    );
    assert!(s.validate(2, &crossed, v.elapsed_ms + 8000).is_err());
    other.elapsed_ms = v.elapsed_ms + 8000;
    other.objects[0].id = 1;
    other.objects[0].account = Some(1);
    other.objects[0].name = "Biceps".into();
    assert!(matches!(
        s.next(&recipient, &other, &c, true),
        Some(Plan::Answer(_))
    ));
    assert!(s.next(&recipient, &other, &c, true).is_none());
}
#[test]
fn injured_or_unready_adventurers_decline_instead_of_promising_a_trip() {
    let (c, mut v, b, mut s) = fixture();
    let o = invite(&mut s, &b, &v, &c);
    v.session.account = 2;
    v.session.player = 2;
    v.objects[0].account = Some(1);
    v.objects[0].name = "Biceps".into();
    v.objects[0].id = 1;
    v.character.hp = 20;
    let b = Brain::new(PROFILES[4], &v);
    let r = b
        .adventure_request(Plan::Answer(o.clone()), &v, &c)
        .unwrap();
    assert_eq!(r.payload["allowed_intents"], serde_json::json!(["decline"]));
    s.apply(
        member(2, "ExElite"),
        Plan::Answer(o),
        false,
        "need to recover".into(),
        v.elapsed_ms,
    );
    assert!(s.group(1, v.elapsed_ms).is_none());
}
#[test]
fn membership_uses_accounts_across_new_entities_and_world_clocks_but_expires() {
    let (c, mut v, b, mut s) = fixture();
    let o = invite(&mut s, &b, &v, &c);
    s.apply(
        member(2, "ExElite"),
        Plan::Answer(o),
        true,
        "yes".into(),
        v.elapsed_ms,
    );
    let trip = s.group(1, v.elapsed_ms).unwrap();
    assert_eq!(trip.phase(&v), Phase::Adventuring);
    v.session.world = 5;
    v.time_ms = 0;
    v.elapsed_ms += 1000;
    v.map.name = "Vault".into();
    v.objects[0].id = 991;
    v.objects[0].name = "Renamed".into();
    assert_eq!(trip.phase(&v), Phase::Preparing);
    assert!(s.group(1, v.elapsed_ms).is_some());
    v.map.name = "Realm".into();
    v.objects.clear();
    assert_eq!(trip.phase(&v), Phase::Regrouping);
    assert!(s.group(1, trip.until).is_none());
}
#[test]
fn no_unseen_recruitment_no_ignored_players_and_no_human_offer_without_language() {
    let (c, mut v, b, mut s) = fixture();
    s.unregister(2);
    assert!(s.next(&b, &v, &c, false).is_none());
    v.objects.clear();
    assert!(s.next(&b, &v, &c, true).is_none());
    let (_, v, b, mut s) = fixture();
    s.ignored.insert(1, BTreeSet::from([2]));
    assert!(s.next(&b, &v, &c, true).is_none());
}
#[test]
fn invitation_expiry_and_other_commitments_reject_delayed_acceptance() {
    let (c, v, b, mut s) = fixture();
    let o = invite(&mut s, &b, &v, &c);
    assert!(s.validate(2, &Plan::Answer(o.clone()), o.until).is_err());
    s.leave(1);
    assert!(s.validate(2, &Plan::Answer(o), v.elapsed_ms).is_err());
    assert!(s.group(2, v.elapsed_ms).is_none());
}
#[test]
fn plain_human_yes_is_only_routed_to_a_real_live_invitation() {
    let (c, v, mut b, mut s) = fixture();
    let mut o = invite(&mut s, &b, &v, &c);
    o.to.name = "Human".into();
    b.offered = Some(o);
    let mut v = v;
    v.objects[0].name = "Human".into();
    let mut w = crate::wire::Writer::new();
    w.utf("Human")
        .i32(2)
        .i32(0)
        .u8(5)
        .utf("")
        .utf("yes")
        .utf("");
    let p = Packet {
        id: wire::TEXT,
        body: w.0,
    };
    let r = b.dialogue_request(&p, &v, &c).unwrap().unwrap();
    assert!(matches!(r.plan, Some(Plan::Answer(_))));
    assert!(
        r.payload["allowed_intents"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("join"))
    );
    b.offered = None;
    assert!(b.dialogue_request(&p, &v, &c).unwrap().is_none());
}
#[test]
fn the_live_named_realm_portal_is_used_but_full_portals_are_not() {
    let (c, mut v, _, _) = fixture();
    v.map.name = "Nexus".into();
    v.objects.clear();
    let mut portal = object(7, 0x712, 12.5, 11.5);
    portal.name = "Djinn (0/16)".into();
    v.objects.push(portal);
    let mut b = Brain::new(PROFILES[1], &v);
    v.time_ms += 10001;
    v.tick += 201;
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Portal(7))
    ));
    v.objects[0].usable = false;
    v.tick += 5;
    v.time_ms += 250;
    assert!(!matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Portal(7))
    ));
}
#[test]
fn familiarity_requires_the_own_objective_clear_and_a_live_observed_companion() {
    let (c, mut v, b, mut s) = fixture();
    let o = invite(&mut s, &b, &v, &c);
    s.apply(
        member(2, "ExElite"),
        Plan::Answer(o),
        true,
        "yes".into(),
        v.elapsed_ms,
    );
    assert!(s.completed(1, &v, [0, 0]).is_empty());
    v.character.snake_pits = 1;
    assert!(s.completed(1, &v, [0, 0]).is_empty());
    v.character.pirate_caves = 1;
    v.lifecycle.death_pending = true;
    assert!(s.completed(1, &v, [0, 0]).is_empty());
    v.lifecycle.death_pending = false;
    assert_eq!(s.completed(1, &v, [0, 0]), vec![2]);
    assert!(s.group(1, v.elapsed_ms).unwrap().finished);
    assert!(s.group(1, v.elapsed_ms + 10000).is_none());
}
#[tokio::test]
async fn bounded_familiarity_is_account_data_not_dead_character_state() {
    let path = std::env::temp_dir().join(format!("bot-social-memory-{}.db", rand::random::<u64>()));
    let c = content();
    let store = crate::store::Store::open(&path, &c).unwrap();
    let a = store.list("memory-a".into()).await.unwrap().id;
    let peer = store.list("memory-b".into()).await.unwrap().id;
    let mut character = store.create(a, c.get(WIZARD).unwrap()).await.unwrap();
    let old = character.id;
    let mut s = Society::default();
    s.register(a, store.account_lists(a).await.unwrap());
    assert!(s.remember(a, peer));
    assert!(!s.remember(a, peer));
    for id in 100..103 {
        assert!(s.remember(a, id));
    }
    assert!(!s.remember(a, 104));
    store.edit_account_list(a, 0, true, peer).await.unwrap();
    character.hp = 0;
    character.alive = false;
    store.death(character, "memory test".into()).await.unwrap();
    drop(store);
    let store = crate::store::Store::open(&path, &c).unwrap();
    assert!(store.account_lists(a).await.unwrap()[0].contains(&peer));
    let replacement = store.create(a, c.get(WIZARD).unwrap()).await.unwrap();
    assert_ne!(old, replacement.id);
    assert_eq!(replacement.items, c.get(WIZARD).unwrap().equipment);
    s.leave(a);
    assert!(s.group(a, 0).is_none());
}
