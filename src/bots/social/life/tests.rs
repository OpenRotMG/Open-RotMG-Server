use super::*;
use crate::bots::tests::{content, object, view};
use crate::wire::Writer;

fn message(text: &str, audience: &str) -> Packet {
    let mut w = Writer::new();
    w.utf("Helper")
        .i32(2)
        .i32(0)
        .u8(0)
        .utf(audience)
        .utf(text)
        .utf("");
    Packet {
        id: wire::TEXT,
        body: w.0,
    }
}
fn scene() -> (Content, PlayerView, Life) {
    let c = content();
    let mut v = view(&c);
    let mut p = object(2, PRIEST, 12.5, 11.5);
    p.account = Some(2);
    p.name = "Helper".into();
    p.equipped = vec![2564, 2775, -1, -1];
    p.max_hp = 100;
    p.hp = 100;
    v.objects.push(p);
    (c, v, Life::default())
}
#[test]
fn community_social_requests_can_be_ignored_refused_or_offer_only_real_spares() {
    let (_, mut v, mut l) = scene();
    assert!(
        !l.receive(&message("someone else, spare health?", ""), &v, "Owner", 8.)
            .unwrap()
    );
    assert!(
        !l.receive(
            &message("Owner, spare health?", "PrivateOther"),
            &v,
            "Owner",
            8.
        )
        .unwrap()
    );
    assert!(
        l.receive(&message("Owner, spare health?", ""), &v, "Owner", 8.)
            .unwrap()
    );
    assert!(l.gift.is_none());
    assert_eq!(l.outgoing.as_deref(), Some("Helper, keeping my supplies"));
    v.character.items[4..7].fill(HEALTH);
    l.receive(&message("Owner, spare health?", "Owner"), &v, "Owner", 3.)
        .unwrap();
    assert!(l.gift.is_none(), "proximity alone is not trust");
    l.receive(&message("Owner, spare health?", "Owner"), &v, "Owner", 8.)
        .unwrap();
    assert!(l.gift.as_ref().unwrap().giving);
    l.receive(&message("Owner, no thanks", ""), &v, "Owner", 8.)
        .unwrap();
    assert!(
        l.gift.as_ref().unwrap().cancelled,
        "native revocation must precede forgetting consent"
    );
    let mut l = Life::default();
    v.character.hp = 1;
    l.receive(&message("Owner, spare health?", ""), &v, "Owner", 8.)
        .unwrap();
    assert!(l.gift.is_none(), "own need overrides generosity");
}
#[test]
fn community_social_gift_acceptance_requires_own_need_and_committed_effect() {
    let (_, mut v, mut l) = scene();
    l.receive(&message("Owner, take health", ""), &v, "Owner", 3.)
        .unwrap();
    assert!(l.gift.is_some());
    let mut done = Writer::new();
    done.i32(0).utf("Trade successful.");
    let packet = Packet {
        id: 12,
        body: done.0,
    };
    l.receive(&packet, &v, "Owner", 3.).unwrap();
    assert!(
        l.episodes.is_empty(),
        "success packet alone is not an inventory receipt"
    );
    l.receive(&message("Owner, take health", ""), &v, "Owner", 3.)
        .unwrap();
    let free = v
        .character
        .items
        .iter()
        .enumerate()
        .skip(4)
        .find(|(_, k)| **k < 0)
        .unwrap()
        .0;
    v.character.items[free] = HEALTH;
    v.character.inventory_revision += 1;
    l.receive(&packet, &v, "Owner", 3.).unwrap();
    assert!(
        !l.known(2),
        "changed holdings plus a legacy result cannot identify who helped"
    );
    assert!(l.episodes.is_empty());
    v.character.items[5] = HEALTH;
    l.receive(&message("Owner, take health", ""), &v, "Owner", 3.)
        .unwrap();
    assert!(l.gift.is_none());
    assert_eq!(l.outgoing.as_deref(), Some("Helper, no thanks"));
}
#[test]
fn community_social_healing_acknowledges_effect_not_requests_and_respects_mana() {
    let (c, mut v, mut l) = scene();
    l.receive(&message("Owner, heal please", ""), &v, "Owner", 3.)
        .unwrap();
    assert!(!l.known(2));
    let mut n = Writer::new();
    n.i32(v.session.player).utf("+35").u32(0xff00ff00);
    l.receive(
        &Packet {
            id: wire::NOTIFICATION,
            body: n.0,
        },
        &v,
        "Owner",
        3.,
    )
    .unwrap();
    assert!(!l.known(2));
    let effect = |kind, target, a: Pos| {
        let mut w = Writer::new();
        w.u8(kind)
            .i32(target)
            .f32(a.x)
            .f32(a.y)
            .f32(0.)
            .f32(0.)
            .u32(0xffffffff);
        Packet {
            id: wire::SHOW_EFFECT,
            body: w.0,
        }
    };
    l.receive(
        &effect(1, v.session.player, v.objects[0].position),
        &v,
        "Owner",
        3.,
    )
    .unwrap();
    l.receive(&effect(5, 2, Pos { x: 4., y: 0. }), &v, "Owner", 3.)
        .unwrap();
    assert!(l.known(2));
    v.character.class = PRIEST;
    v.character.items[0] = 2564;
    v.character.items[1] = 2775;
    v.objects[0].hp = 50;
    v.character.mp = c.get(2775).unwrap().mana_cost;
    assert!(
        l.healing(&v, &c, 3.).unwrap().is_none(),
        "reserve for own danger"
    );
    v.character.mp *= 2;
    assert!(l.healing(&v, &c, 3.).unwrap().is_some());
    v.character.ability_ready_ms = v.time_ms + 1;
    assert!(l.healing(&v, &c, 3.).unwrap().is_none());
    v.character.ability_ready_ms = 0;
    v.character.mp = 0;
    assert!(l.healing(&v, &c, 8.).unwrap().is_none());
}
#[test]
fn community_social_death_restart_preserves_fact_not_live_contact_or_actions() {
    let (c, mut v, mut l) = scene();
    l.remember(&v, 2, "Helper".into(), Interaction::HealedBy, 35);
    l.receive(&message("Owner, take health", ""), &v, "Owner", 3.)
        .unwrap();
    let raw = serde_json::to_string(&l).unwrap();
    let mut restored: Life = serde_json::from_str(&raw).unwrap();
    restored.bind();
    assert!(restored.known(2));
    assert!(
        restored.gift.is_none() && restored.heal_request.is_none() && restored.waiting.is_none()
    );
    v.session.character += 1;
    v.objects.clear();
    let g = Grid::terrain(&v);
    assert!(
        restored.decide(&v, &g, 3., false).is_none(),
        "unavailable acquaintance is not hidden coordinates or an indefinite wait"
    );
    let mut p = object(99, PRIEST, 20.5, 11.5);
    p.account = Some(2);
    p.name = "Helper".into();
    p.equipped = vec![2564];
    v.objects.push(p);
    let d = restored.decide(&v, &g, 3., false).unwrap();
    assert_eq!(d.goal, Goal::Follow(99));
    let mut mind = pursuits::Mind::new(v.session.account);
    mind.social = restored;
    mind.validate().unwrap();
    v.character.class = crate::content::WIZARD;
    assert!(c.classes.contains_key(&v.character.class));
}
#[test]
fn community_social_full_entrance_wait_and_reconsider_use_public_cues() {
    let (_, mut v, mut waiter) = scene();
    v.objects.clear();
    v.map.name = "Nexus".into();
    v.elapsed_ms = 100;
    let mut portal = object(9, 0x712, 12.5, 11.5);
    portal.name = "Djinn (4/4)".into();
    portal.usable = false;
    v.objects.push(portal);
    let mut vault = object(10, 0x720, 13.5, 11.5);
    vault.usable = true;
    v.objects.push(vault);
    let grid = Grid::terrain(&v);
    assert_eq!(
        waiter.decide(&v, &grid, 3., false).unwrap().goal,
        Goal::Wait
    );
    let mut sorter = waiter.clone();
    v.elapsed_ms += 6000;
    assert_eq!(
        waiter.decide(&v, &grid, 3., false).unwrap().goal,
        Goal::Wait
    );
    assert_eq!(
        sorter.decide(&v, &grid, 8., false).unwrap().goal,
        Goal::Portal(10)
    );
    v.objects[0].usable = true;
    v.objects[0].name = "Djinn (3/4)".into();
    assert!(waiter.decide(&v, &grid, 3., false).is_none());
    assert!(waiter.waiting.is_none());
}
