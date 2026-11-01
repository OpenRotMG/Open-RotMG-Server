use super::*;
use crate::world::view::{VisibleObject, VisibleProjectile};

fn object(id: i32, kind: u16) -> VisibleObject {
    VisibleObject {
        id,
        kind,
        account: None,
        name: String::new(),
        position: Pos { x: 10.5, y: 10.5 },
        hp: 0,
        max_hp: 0,
        level: None,
        equipped: vec![],
        contents: vec![],
        blocks: false,
        usable: false,
        condition_mask: Some(0),
    }
}
fn fixture() -> (Content, PlayerView, Events, Profile) {
    let (c, mut v) = quest_tests::fixture();
    v.map.name = "Wine Cellar".into();
    v.character.level = 20;
    v.objects.clear();
    v.quest = None;
    let mut e = Events::default();
    let mut boss = object(2, crate::endgame::ORYX2);
    boss.hp = 100;
    v.objects.push(boss);
    e.observe(&v);
    v.objects.clear();
    e.observe(&v);
    assert!(
        !e.postclear_observed(&v),
        "disappearance alone is not a kill"
    );
    let mut exit = object(3, 0x71d);
    exit.usable = true;
    v.objects.push(exit);
    e.observe(&v);
    assert!(e.postclear_observed(&v));
    let mut peer = object(4, crate::content::WIZARD);
    peer.account = Some(2);
    peer.hp = 500;
    v.objects.push(peer);
    v.time_ms += 1800;
    let mut p = ARCHETYPES[0];
    p.identity = "Cedar";
    p.companions = 8.;
    v.character.items[4] = 0xa9e;
    (c, v, e, p)
}
fn decide(e: &mut Events, v: &PlayerView, c: &Content, p: Profile) -> Option<Decision> {
    e.decide(v, c, &Grid::terrain(v), p, true).unwrap()
}

#[test]
fn aftermath_clear_requires_new_exit_and_no_living_boss() {
    let (_, mut v, mut e, _) = fixture();
    e.bind();
    let mut boss = object(2, crate::endgame::ORYX2);
    boss.hp = 100;
    v.objects.push(boss);
    e.observe(&v);
    assert!(!e.postclear_observed(&v));
    v.objects.retain(|p| p.id != 2);
    e.observe(&v);
    assert!(!e.postclear_observed(&v), "pre-existing exit cannot clear");
    let mut exit = object(5, 0x712);
    exit.usable = true;
    v.objects.push(exit);
    e.observe(&v);
    assert!(e.postclear_observed(&v));
}

#[test]
fn aftermath_reports_only_committed_own_pickups_not_prepared_or_failed_swaps() {
    let (c, mut v, mut e, p) = fixture();
    v.character.items[5] = 0xa35;
    assert_eq!(
        decide(&mut e, &v, &c, p).unwrap().chat,
        Some("no drop here for me")
    );
    let swap = Action::Swap {
        from: Slot {
            object: 9,
            slot: 0,
            kind: 0xa35,
        },
        to: Slot {
            object: v.session.player,
            slot: 5,
            kind: -1,
        },
    };
    e.collecting(&v, &swap);
    assert!(
        e.committed(&v).is_err(),
        "unchanged revision is not receipt"
    );
    assert!(e.outing.as_ref().unwrap().loot.is_empty());
    e.collecting(&v, &swap);
    e.rejected();
    v.character.inventory_revision += 1;
    e.committed(&v).unwrap();
    assert!(e.outing.as_ref().unwrap().loot.is_empty());
    e.collecting(&v, &swap);
    v.character.inventory_revision += 1;
    e.committed(&v).unwrap();
    e.outing.as_mut().unwrap().reported = false;
    assert_eq!(decide(&mut e, &v, &c, p).unwrap().chat, Some("got wis"));
    assert_eq!(e.pickups.len(), 1);
}

#[test]
fn aftermath_quiet_no_audience_hazards_and_useful_loot_precede_idle_reports() {
    let (c, mut v, mut e, p) = fixture();
    let mut quiet = p;
    quiet.identity = "Reed";
    assert!(decide(&mut e, &v, &c, quiet).is_none_or(|d| d.chat.is_none()));
    v.objects.retain(|p| p.account.is_none());
    assert!(decide(&mut e, &v, &c, p).unwrap().chat.is_none());
    let mut peer = object(4, crate::content::WIZARD);
    peer.account = Some(2);
    peer.hp = 500;
    v.objects.push(peer);
    let mut boss = object(2, crate::endgame::ORYX2);
    boss.hp = 100;
    v.objects.push(boss);
    assert!(decide(&mut e, &v, &c, p).is_none());
    v.objects.pop();
    v.projectiles.push(VisibleProjectile {
        owner: 2,
        id: 1,
        start: v.position,
        position: v.position,
        angle: 0.,
        speed: 10.,
        lifetime_ms: 1000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
        damage: 0,
        armor_piercing: false,
    });
    assert!(decide(&mut e, &v, &c, p).is_none());
    v.projectiles.clear();
    let mut bag = object(9, 0x500);
    bag.contents = vec![0xa35];
    v.objects.push(bag);
    assert!(decide(&mut e, &v, &c, p).is_none());
    e.outing.as_mut().unwrap().pending = Some((5, 0xa35, 0));
    let speech = e
        .speech(&v, &c, p, Some((1, 0xa1f)), None)
        .unwrap()
        .unwrap();
    assert!(!speech.valid(&e, &v, &c, p, Some((1, 0xa1f))).unwrap());
}

#[test]
fn aftermath_independent_departures_are_bounded_without_attention_coupling() {
    let (c, mut v, mut e, p) = fixture();
    let times: BTreeSet<_> = ARCHETYPES.iter().map(|p| linger(&v, &c, *p)).collect();
    assert!(times.len() >= 4);
    assert!(times.iter().all(|t| (1200..=6199).contains(t)));
    let mut changed = p;
    changed.attention = 1;
    assert_eq!(linger(&v, &c, p), linger(&v, &c, changed));
    v.time_ms += 7000;
    e.outing.as_mut().unwrap().reported = true;
    assert!(
        decide(&mut e, &v, &c, p).is_none(),
        "ordinary exit selection resumes"
    );
    v.objects.retain(|o| !o.usable);
    assert!(matches!(
        decide(&mut e, &v, &c, p).unwrap().journey,
        Some(Journey::Nexus)
    ));
}

#[test]
fn aftermath_request_is_bound_to_goal_session_ownership_audience_and_linger() {
    let (c, mut v, mut e, p) = fixture();
    let goal = Some((7, 0xa1f));
    let s = e.speech(&v, &c, p, goal, None).unwrap().unwrap();
    assert!(s.valid(&e, &v, &c, p, goal).unwrap());
    assert!(!s.valid(&e, &v, &c, p, Some((8, 0xa1f))).unwrap());
    assert!(!s.valid(&e, &v, &c, p, None).unwrap());
    v.character.items[5] = 0xa1f;
    assert!(!s.valid(&e, &v, &c, p, goal).unwrap());
    v.character.items[5] = -1;
    v.session.world += 1;
    assert!(!s.valid(&e, &v, &c, p, goal).unwrap());
    v.session.world -= 1;
    v.time_ms += 7000;
    assert!(!s.valid(&e, &v, &c, p, goal).unwrap());
    assert!(e.speech(&v, &c, p, goal, None).unwrap().is_none());
}

#[test]
fn aftermath_held_incantation_uses_existing_opening_and_readiness_path() {
    let (c, mut v, mut e, p) = fixture();
    v.map.name = "Oryx's Chamber".into();
    v.objects.clear();
    v.objects.push(object(10, crate::endgame::LOCKED_CELLAR));
    v.character.items[5] = i32::from(crate::endgame::INCANTATION);
    e.bind();
    e.observe(&v);
    assert!(matches!(
        decide(&mut e, &v, &c, p).unwrap().item,
        Some(Action::Use { slot: 5, .. })
    ));
    v.character.items[5] = -1;
    assert!(
        decide(&mut e, &v, &c, p).is_none(),
        "no held key, no opening"
    );
    v.character.items[5] = i32::from(crate::endgame::INCANTATION);
    v.character.hp = 100;
    assert!(
        decide(&mut e, &v, &c, p).is_none(),
        "unsafe idle opening is deferred"
    );
}

#[test]
fn aftermath_commitment_preserves_held_incantation_and_remedy_not_idle_reports() {
    let (c, mut v, e, p) = fixture();
    let mut b = Brain::new(p, &v);
    b.mind.events = e;
    b.companion = Some(4);
    b.invitation_until = v.time_ms + 60000;
    assert!(b.committed(&v));
    assert!(
        b.mind
            .events
            .decide(&v, &c, &Grid::terrain(&v), p, false)
            .unwrap()
            .is_none()
    );
    let d = b.decide(&v, &c).unwrap();
    assert!(d.chat.is_none());
    assert_ne!(b.activity, Activity::Event);

    v.tick += 1;
    v.time_ms += 50;
    v.elapsed_ms += 50;
    v.map.name = "Oryx's Chamber".into();
    v.objects.clear();
    v.objects.push(object(10, crate::endgame::LOCKED_CELLAR));
    v.character.items[5] = i32::from(crate::endgame::INCANTATION);
    b.mind.events.bind();
    let d = b.decide(&v, &c).unwrap();
    assert!(b.committed(&v));
    assert!(matches!(d.item, Some(Action::Use { slot: 5, .. })));

    v.tick += 1;
    v.time_ms += 50;
    v.elapsed_ms += 50;
    v.map.name = "Mad Lab".into();
    v.character.conditions = crate::condition::bit(crate::condition::HEXED);
    v.objects.clear();
    let mut blue = object(11, crate::dungeon::lab::DEHEXXER);
    blue.position.x += 1.;
    v.objects.push(blue);
    b.mind.events.bind();
    b.mind.events.knows_blue = true;
    let d = b.decide(&v, &c).unwrap();
    assert!(b.committed(&v));
    assert_eq!(d.goal, Goal::Remedy(11));
}
