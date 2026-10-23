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
fn community_crowd_departure_does_not_dissolve_other_accounts() {
    let (_, v, _, mut s) = fixture();
    s.register(3, Default::default());
    s.trips.push(Adventure {
        members: vec![member(1, "A"), member(2, "B"), member(3, "C")],
        purpose: Purpose::Experience,
        until: v.elapsed_ms + 600000,
        finished: false,
    });
    s.leave(1);
    let remaining = s
        .group(2, v.elapsed_ms)
        .expect("independent peers retain their outing");
    assert_eq!(
        remaining
            .members
            .iter()
            .map(|m| m.account)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
}
#[test]
fn community_crowd_existing_outing_can_agree_with_a_third_account() {
    let (c, mut v, b, mut s) = fixture();
    let offer = invite(&mut s, &b, &v, &c);
    s.apply(
        member(2, "ExElite"),
        Plan::Answer(offer),
        true,
        "yes".into(),
        v.elapsed_ms,
    );
    v.elapsed_ms += 100000;
    v.objects[0].account = Some(3);
    v.objects[0].id = 3;
    v.objects[0].name = "Third".into();
    s.register(3, Default::default());
    let mut b = Brain::new(PROFILES[1], &v);
    b.group = s.group(1, v.elapsed_ms);
    let plan = s
        .next(&b, &v, &c, false)
        .expect("an outing is not an exclusive pair");
    s.validate(1, &plan, v.elapsed_ms).unwrap();
    s.apply(
        member(1, "Biceps"),
        plan,
        false,
        "Third, join?".into(),
        v.elapsed_ms,
    );
    let offer = s.outgoing(1, v.elapsed_ms).unwrap();
    s.validate(3, &Plan::Answer(offer.clone()), v.elapsed_ms)
        .unwrap();
    s.apply(
        member(3, "Third"),
        Plan::Answer(offer),
        true,
        "yes".into(),
        v.elapsed_ms,
    );
    assert_eq!(s.group(3, v.elapsed_ms).unwrap().members.len(), 3);
}
