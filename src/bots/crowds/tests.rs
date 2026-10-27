use super::*;
use crate::world::view::VisibleObject;

fn fixture() -> (Content, PlayerView, Brain) {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.session.account = 201;
    v.session.world = -4;
    v.quest = None;
    let mut peer = v.objects[0].clone();
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
        position: peer.position,
        name: peer.name.clone(),
    });
    v.objects.push(peer);
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.crowd.observe(&v, &c);
    b.mind
        .intentions
        .as_mut()
        .unwrap()
        .observe(&v, &c, b.profile.identity)
        .unwrap();
    (c, v, b)
}
fn hit(shooter: i32, bullet: u8) -> Packet {
    let mut w = wire::Writer::default();
    w.i32(2).u8(0).i16(10).u8(0).u8(bullet).i32(shooter);
    Packet { id: 47, body: w.0 }
}
fn shared(c: &Content, v: &PlayerView, b: &mut Brain) {
    b.crowd.receive(&hit(v.session.player, 1), v, c).unwrap();
    b.crowd.receive(&hit(3, 1), v, c).unwrap();
    assert!(b.crowd.cooperator(v, |_| true).is_some());
}
fn advance(v: &mut PlayerView, ms: u64) {
    v.elapsed_ms += ms;
    v.time_ms += ms;
    v.tick += 1;
}
fn adventure(c: &Content, v: &PlayerView, b: &mut Brain) -> Decision {
    b.crowd.observe(v, c);
    b.mind
        .intentions
        .as_mut()
        .unwrap()
        .observe(v, c, b.profile.identity)
        .unwrap();
    let enemies: Vec<_> = v
        .objects
        .iter()
        .filter(|o| c.objects[&o.kind].enemy && o.hp > 0)
        .collect();
    let projectile = &c.get(v.character.items[0] as u16).unwrap().projectiles[&0];
    let s = Scene {
        nearest: None,
        quarry: None,
        enemies,
        range: projectile.speed * projectile.lifetime / 10000.,
        passes_cover: projectile.passes_cover,
        perception: combat::Perception::attend(v, &b.mind.cognition, false),
        inspecting: false,
        health: 1.,
        danger: f32::INFINITY,
        threatened: false,
        retreat: false,
        outmatched: false,
        grid: b.navigation_grid(v, c, &[]),
        remedy: None,
    };
    b.adventure(
        v,
        c,
        &s,
        Decision {
            goal: Goal::Wait,
            movement: None,
            aim: None,
            item: None,
            journey: None,
            chat: None,
        },
        false,
    )
    .unwrap()
}

#[test]
fn shared_combat_beats_optional_follow_without_changing_solo_combat_scores() {
    let (c, mut v, mut b) = fixture();
    v.objects[0].position = Pos { x: 14.5, y: 10.5 };
    shared(&c, &v, &mut b);
    let d = adventure(&c, &v, &mut b);
    assert_eq!(d.goal, Goal::Fight(2));
    assert!(d.aim.is_some());
    assert!(d.journey.is_none());
    assert_eq!(
        b.crowd.cooperator(&v, |_| true).unwrap().name,
        "GuestObserved"
    );
}

#[test]
fn useful_loot_pause_then_visible_catchup_respects_reached_company_progress_guard() {
    let (c, mut v, mut b) = fixture();
    shared(&c, &v, &mut b);
    v.objects.remove(0);
    v.objects.push(VisibleObject {
        id: 4,
        account: None,
        kind: 0x500,
        name: String::new(),
        position: Pos { x: 11.5, y: 10.5 },
        hp: 0,
        max_hp: 0,
        level: None,
        equipped: vec![],
        contents: vec![0xa22],
        blocks: false,
        usable: false,
        condition_mask: None,
    });
    let loot = adventure(&c, &v, &mut b);
    assert_eq!(loot.goal, Goal::Loot(4));
    assert!(matches!(loot.item, Some(Action::Swap { .. })));
    assert!(b.crowd.cooperator(&v, |_| true).is_some());
    v.objects.pop();
    v.character.items[4] = 0xa22;
    v.character.inventory_revision += 1;
    advance(&mut v, 1000);
    let catchup = adventure(&c, &v, &mut b);
    assert_eq!(catchup.goal, Goal::Follow(3));
    assert!(catchup.movement.is_some());
    assert!(
        catchup.journey.is_none(),
        "observed co-combat catchup walks rather than repeated teleport"
    );
    v.position = Pos { x: 20.5, y: 10.5 };
    advance(&mut v, 1000);
    b.mind
        .intentions
        .as_mut()
        .unwrap()
        .observe(&v, &c, b.profile.identity)
        .unwrap();
    assert!(
        !b.mind
            .intentions
            .as_ref()
            .unwrap()
            .eligible(&v, &Goal::Follow(3), false)
    );
    v.objects[0].position.x = 30.5;
    v.player_markers[0].position.x = 30.5;
    let own = adventure(&c, &v, &mut b);
    assert_ne!(
        own.goal,
        Goal::Follow(3),
        "a higher follow score cannot erase required own progress"
    );
}

#[test]
fn brief_public_visibility_loss_recovers_but_unseen_or_unsafe_targets_are_not_chased() {
    let (c, mut v, mut b) = fixture();
    shared(&c, &v, &mut b);
    let mut peer = v.objects.pop().unwrap();
    advance(&mut v, 1000);
    b.crowd.observe(&v, &c);
    assert!(
        b.crowd.cooperator(&v, |_| true).is_none(),
        "a minimap marker is not visible shared company"
    );
    advance(&mut v, 5000);
    v.objects.push(peer.clone());
    assert!(b.crowd.cooperator(&v, |_| true).is_some());
    assert!(
        b.crowd.cooperator(&v, |_| false).is_none(),
        "blocked route ends company"
    );
    assert!(b.crowd.cooperator(&v, |_| true).is_none());

    let (c, mut v, mut b) = fixture();
    shared(&c, &v, &mut b);
    v.objects.pop();
    advance(&mut v, 1000);
    assert_eq!(v.elapsed_ms, 6000);
    b.crowd.observe(&v, &c);
    advance(&mut v, 5950);
    assert_eq!(v.elapsed_ms, 11950);
    b.crowd.observe(&v, &c);
    assert!(b.crowd.cooperator(&v, |_| true).is_none());
    advance(&mut v, 50);
    assert_eq!(v.elapsed_ms, 12000);
    v.objects.push(peer.clone());
    assert!(
        b.crowd.cooperator(&v, |_| true).is_none(),
        "reappearance at the six-second loss deadline cannot restore company"
    );
    assert!(!b.crowd.cooperation.passive_available(41999));
    assert!(b.crowd.cooperation.passive_available(42000));
    let (c, mut v, mut b) = fixture();
    shared(&c, &v, &mut b);
    peer.position = Pos { x: 64.5, y: 64.5 };
    v.objects[1] = peer;
    assert!(b.crowd.cooperator(&v, |_| true).is_none());
}

#[test]
fn ongoing_shared_hits_cannot_renew_an_outing_and_session_changes_erase_it() {
    let (c, mut v, mut b) = fixture();
    shared(&c, &v, &mut b);
    for bullet in 2..=7 {
        advance(&mut v, 10_000);
        b.crowd.observe(&v, &c);
        b.crowd
            .receive(&hit(v.session.player, bullet), &v, &c)
            .unwrap();
        b.crowd.receive(&hit(3, bullet), &v, &c).unwrap();
    }
    assert!(
        b.crowd.cooperator(&v, |_| true).is_none(),
        "the fixed sixty-second outing expires despite new hits"
    );
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(
        b.crowd.destination(&v, None, |_| true).is_none(),
        "cooperative expiry cannot immediately become a fresh passive outing"
    );
    advance(&mut v, 30_000);
    shared(&c, &v, &mut b);
    v.session.world -= 1;
    b.crowd.observe(&v, &c);
    assert!(b.crowd.cooperator(&v, |_| true).is_none());
    assert_eq!(b.crowd.len(), 0);
}

#[test]
fn newer_public_peer_can_attract_passive_company_and_shared_damage_catchup() {
    let (c, mut v, _) = fixture();
    v.session.account = 199;
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.crowd.observe(&v, &c);
    assert!(can_approach(v.session.world, 199, 200));
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert_eq!(
        b.crowd.destination(&v, None, |_| true).unwrap().account,
        200
    );
    shared(&c, &v, &mut b);
    v.objects.remove(0);
    let catchup = adventure(&c, &v, &mut b);
    assert_eq!(catchup.goal, Goal::Follow(3));
    assert!(catchup.movement.is_some());
    assert!(catchup.journey.is_none());
}

#[test]
fn public_world_rank_is_strict_acyclic_and_not_account_age_ordered() {
    assert!(can_approach(-4, 199, 200));
    assert!(can_approach(-3, 200, 199));
    for world in [-5, -4, -3, 1, 2] {
        for account in [199, 200, 201, 202] {
            assert!(!can_approach(world, account, account));
            for peer in [199, 200, 201, 202] {
                if account != peer {
                    assert_ne!(
                        can_approach(world, account, peer),
                        can_approach(world, peer, account)
                    );
                }
                for third in [199, 200, 201, 202] {
                    if can_approach(world, account, peer) && can_approach(world, peer, third) {
                        assert!(can_approach(world, account, third));
                        assert!(!can_approach(world, third, account));
                    }
                }
            }
        }
    }
}

#[test]
fn passive_outings_are_directional_bounded_and_yield_to_safety_and_commitments() {
    let (c, mut v, mut b) = fixture();
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    b.crowd.approach_selected(3);
    assert!(b.crowd.destination(&v, None, |_| true).is_some());
    assert!(can_approach(v.session.world, 201, 200));
    assert!(!can_approach(v.session.world, 200, 201));
    assert!(!can_approach(v.session.world, 200, 200));
    let mut reverse = Crowd::new(true);
    v.session.account = 200;
    v.player_markers[0].account = 201;
    v.objects[1].account = Some(201);
    reverse.attend(&v, &c, &b.mind.cognition, false);
    assert!(reverse.destination(&v, None, |_| true).is_none());
    v.session.account = 201;
    v.player_markers[0].account = 200;
    v.objects[1].account = Some(200);
    let marker = v.player_markers.pop().unwrap();
    advance(&mut v, 1000);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(b.crowd.keeping_company(3));
    assert!(b.crowd.destination(&v, None, |_| true).is_none());
    v.player_markers.push(marker);
    advance(&mut v, 1000);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(b.crowd.destination(&v, None, |_| true).is_some());
    advance(&mut v, 90_000);
    b.crowd.attend(&v, &c, &b.mind.cognition, false);
    assert!(!b.crowd.keeping_company(3));
    assert!(b.crowd.destination(&v, None, |_| true).is_none());

    let (c, mut v, mut b) = fixture();
    shared(&c, &v, &mut b);
    v.character.hp = 1;
    b.decide(&v, &c).unwrap();
    assert!(b.crowd.cooperator(&v, |_| true).is_none());
    let (c, v, mut b) = fixture();
    shared(&c, &v, &mut b);
    b.companion = Some(3);
    b.invitation_until = v.time_ms + 10_000;
    b.decide(&v, &c).unwrap();
    assert!(b.crowd.cooperator(&v, |_| true).is_none());
}
