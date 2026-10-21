use super::*;
use crate::bots::tests::{content, object, view};
use crate::content::WIZARD;
#[path = "durability_tests.rs"]
mod durability_tests;

#[test]
fn community_market_request_pacing_waits_for_native_request_lifetime_and_resets_only_for_new_binding_or_terms()
 {
    let mut m = Market::default();
    assert!(m.request_due(0));
    assert!(!m.request_due(50));
    assert!(!m.request_due(19999));
    assert!(m.request_due(20000));
    m.bind();
    assert!(m.request_due(20001));
    assert!(!m.request_due(20050));
    let (_, v, _) = context();
    m.replace(terms(&v));
    assert!(m.request_due(20051));
    assert!(!m.request_due(20052));
}
#[test]
fn community_market_opposite_unit_rendezvous_boundaries_close_into_strict_native_reach() {
    let mut arrivals = Vec::new();
    for offset in [-1., 1.] {
        let (c, mut v, mut m) = context();
        let t = terms(&v);
        let meeting = Pos { x: 11.5, y: 11.5 };
        v.position = Pos {
            x: meeting.x,
            y: meeting.y + offset,
        };
        let peer = v.objects.iter_mut().find(|p| p.account == Some(4)).unwrap();
        peer.position = Pos {
            x: meeting.x,
            y: meeting.y - offset,
        };
        m.replace(t);
        let b = m.bargain.as_mut().unwrap();
        b.agreed = true;
        b.own_consent = true;
        let d = m.decide(&v, &c, None, &Grid::new(&v)).unwrap().unwrap();
        let movement=d.movement.expect("opposite closed unit-radius arrivals are exactly two apart; rendezvous must close further without weakening native reach");
        assert!(movement.distance(meeting) < v.position.distance(meeting));
        arrivals.push(movement);
    }
    assert!(arrivals[0].distance(arrivals[1]) < 2.);
}

#[test]
fn community_market_rendezvous_does_not_move_inside_reach_or_bypass_blockage_expiry_or_refusal() {
    for case in ["inside", "blocked", "expired", "refused"] {
        let (c, mut v, mut m) = context();
        let mut t = terms(&v);
        v.position = Pos {
            x: 11.5,
            y: if case == "inside" { 12. } else { 12.5 },
        };
        if case == "expired" {
            t.expires = now();
        }
        m.replace(t);
        let b = m.bargain.as_mut().unwrap();
        b.agreed = true;
        b.own_consent = true;
        if case == "refused" {
            b.resolution = Some("refused_remaining_balance_unenforced".into());
        }
        let mut grid = Grid::new(&v);
        if case == "blocked" {
            grid.walkable = [tile(v.position)].into();
        }
        let d = m.decide(&v, &c, None, &grid).unwrap();
        assert!(
            d.is_none_or(|d| d.movement.is_none() && d.journey.is_none()),
            "{case}"
        );
        if case == "expired" {
            assert_eq!(
                m.bargain.as_ref().unwrap().resolution.as_deref(),
                Some("expired_remaining_balance_unenforced")
            );
        }
    }
}

fn context() -> (Content, PlayerView, Market) {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Nexus".into();
    v.session.world = -2;
    v.character.base_stats = c.classes[&WIZARD].caps;
    v.character.base_stats[4] -= 4;
    v.character.items[4..].copy_from_slice(&[0xa22, 0xa22, 0xa23, 0xa23, 0xa4c, 0xa4c, -1, -1]);
    let mut peer = object(4, WIZARD, 12., 11.5);
    peer.name = "Biceps".into();
    peer.equipped = vec![0xa97];
    v.objects.push(peer);
    let m = Market {
        name: "Triceps".into(),
        ..Market::default()
    };
    (c, v, m)
}
fn text(speaker: i32, name: &str, recipient: &str, text: &str) -> Packet {
    let mut w = wire::Writer::new();
    w.utf(name)
        .i32(speaker)
        .i32(0)
        .u8(0)
        .utf(recipient)
        .utf(text)
        .utf("");
    Packet {
        id: wire::TEXT,
        body: w.0,
    }
}
fn terms(v: &PlayerView) -> Terms {
    Terms {
        version: 42,
        accounts: [4, v.session.account],
        names: ["Biceps".into(), "Triceps".into()],
        goods: [
            Goods {
                kind: 0xa21,
                quantity: 2,
            },
            Goods {
                kind: 0xa4c,
                quantity: 2,
            },
        ],
        meeting: [-2, 11, 11],
        expires: now() + 180,
        batch: 1,
    }
}
#[test]
fn community_market_recipient_specific_private_guild_and_unknown_identity() {
    for channel in ["", "Triceps", "*Guild*"] {
        let (c, v, mut m) = context();
        let t = terms(&v);
        let speaker = if channel.is_empty() { 4 } else { -1 };
        assert!(
            m.receive(
                &text(speaker, "Biceps", channel, &t.text("Triceps", "offer")),
                &v,
                &c,
                None
            )
            .unwrap()
        );
        assert!(m.bargain.is_some());
        assert!(m.outgoing.as_ref().unwrap().consent);
        assert_eq!(
            m.outgoing.as_ref().unwrap().channel,
            match channel {
                "" => Channel::Local,
                "*Guild*" => Channel::Guild,
                _ => Channel::Tell("Biceps".into()),
            }
        );
    }
    let (c, mut v, mut m) = context();
    let t = terms(&v);
    for (speaker, name, recipient) in [
        (4, "Biceps", "Other"),
        (1, "Triceps", "Triceps"),
        (-1, "*System*", ""),
    ] {
        assert!(
            !m.receive(
                &text(speaker, name, recipient, &t.text("Triceps", "offer")),
                &v,
                &c,
                None
            )
            .unwrap()
        );
        assert!(m.bargain.is_none());
    }
    v.objects.clear();
    assert!(
        m.receive(
            &text(-1, "Unknown", "Triceps", &t.text("Triceps", "offer")),
            &v,
            &c,
            None
        )
        .unwrap()
    );
    assert!(m.bargain.is_none());
}
#[test]
fn community_market_explicit_identical_consent_only_changed_terms_invalidate() {
    let (c, v, mut m) = context();
    let t = terms(&v);
    m.receive(
        &text(4, "Biceps", "", &t.text("Triceps", "offer")),
        &v,
        &c,
        None,
    )
    .unwrap();
    let reply = m.outgoing.clone().unwrap();
    m.published(&reply);
    assert!(m.bargain.as_ref().unwrap().own_consent);
    for value in [
        "Triceps, yes".to_string(),
        format!("Triceps, agree {}", t.body().replace("2593x2", "2593x1")),
        t.text("Other", "agree"),
    ] {
        m.receive(&text(4, "Biceps", "", &value), &v, &c, None)
            .unwrap();
    }
    // Only an exact canonical agreement can bind the current observed sender.
    m.bargain.as_mut().unwrap().peer_consent = None;
    m.receive(
        &text(5, "Biceps", "", &t.text("Triceps", "agree")),
        &v,
        &c,
        None,
    )
    .unwrap();
    assert!(m.bargain.as_ref().unwrap().peer_consent.is_none());
    m.receive(
        &text(4, "Biceps", "", &t.text("Triceps", "agree")),
        &v,
        &c,
        None,
    )
    .unwrap();
    assert!(m.bargain.as_ref().unwrap().peer_consent.is_some());
    let mut changed = t.clone();
    changed.version += 1;
    changed.batch = 2;
    m.receive(
        &text(4, "Biceps", "", &changed.text("Triceps", "offer")),
        &v,
        &c,
        None,
    )
    .unwrap();
    let b = m.bargain.as_ref().unwrap();
    assert!(!b.own_consent);
    assert!(b.peer_consent.is_none());
    m.receive(
        &text(4, "Biceps", "", &t.text("Triceps", "agree")),
        &v,
        &c,
        None,
    )
    .unwrap();
    assert!(m.bargain.as_ref().unwrap().peer_consent.is_none());
    m.bind();
    assert!(!m.bargain.as_ref().unwrap().own_consent);
}
#[test]
fn community_market_partial_changed_terms_revoke_consent_without_rewriting_paid_balance() {
    let (c, v, mut m) = context();
    let original = terms(&v);
    m.receive(
        &text(4, "Biceps", "Triceps", &original.text("Triceps", "offer")),
        &v,
        &c,
        None,
    )
    .unwrap();
    let reply = m.outgoing.clone().unwrap();
    m.published(&reply);
    m.receive(
        &text(4, "Biceps", "Triceps", &original.text("Triceps", "agree")),
        &v,
        &c,
        None,
    )
    .unwrap();
    m.recover(vec![original.delivery(0).unwrap()]).unwrap();
    assert!(m.bargain.as_ref().unwrap().own_consent);
    assert!(m.bargain.as_ref().unwrap().peer_consent.is_some());
    let stale_remaining = m.bargain.as_ref().unwrap().text("Triceps", "agree");
    let mut changed = original.clone();
    changed.version += 1;
    changed.batch = 2;
    m.receive(
        &text(4, "Biceps", "Triceps", &changed.text("Triceps", "counter")),
        &v,
        &c,
        None,
    )
    .unwrap();
    let b = m.bargain.as_ref().unwrap();
    assert_eq!(b.terms, original);
    assert_eq!(b.confirmed, 1);
    assert!(
        !b.own_consent,
        "changed terms cannot retain old acceptance after payment"
    );
    assert!(b.peer_consent.is_none());
    m.receive(
        &text(4, "Biceps", "Triceps", &original.text("Triceps", "agree")),
        &v,
        &c,
        None,
    )
    .unwrap();
    assert!(
        m.bargain.as_ref().unwrap().peer_consent.is_none(),
        "a prior original assent cannot authorize a remaining delivery after revocation"
    );
    m.receive(
        &text(4, "Biceps", "Triceps", &stale_remaining),
        &v,
        &c,
        None,
    )
    .unwrap();
    assert!(
        m.bargain.as_ref().unwrap().peer_consent.is_none(),
        "the previously current remaining assent is revoked too"
    );
}
#[test]
fn community_market_changed_interests_refusal_stock_capacity_expiry_and_death() {
    let (c, mut v, mut m) = context();
    let t = terms(&v);
    assert!(m.suitable(&v, &c, None, &t).unwrap());
    v.character.base_stats[4] = c.classes[&WIZARD].caps[4];
    assert!(!m.suitable(&v, &c, None, &t).unwrap());
    m.receive(
        &text(4, "Biceps", "", &t.text("Triceps", "offer")),
        &v,
        &c,
        None,
    )
    .unwrap();
    assert!(!m.outgoing.as_ref().unwrap().consent);
    m.replace(t.clone());
    v.character.items[8] = -1;
    v.character.items[9] = -1;
    m.decide(&v, &c, None, &Grid::new(&v)).unwrap();
    assert!(
        m.bargain
            .as_ref()
            .unwrap()
            .resolution
            .as_ref()
            .unwrap()
            .contains("stock_changed")
    );
    m.replace(t.clone());
    m.bargain.as_mut().unwrap().terms.expires = now() - 1;
    m.decide(&v, &c, None, &Grid::new(&v)).unwrap();
    assert!(
        m.bargain
            .as_ref()
            .unwrap()
            .resolution
            .as_ref()
            .unwrap()
            .contains("expired")
    );
    m.replace(t);
    m.finish("character_died_remaining_balance_unenforced");
    let raw = serde_json::to_string(&m).unwrap();
    let mut restart: Market = serde_json::from_str(&raw).unwrap();
    restart.bind();
    assert!(
        restart
            .bargain
            .as_ref()
            .unwrap()
            .resolution
            .as_ref()
            .unwrap()
            .contains("died")
    );
}
#[test]
fn community_market_partial_final_and_duplicate_receipts_are_transaction_identified() {
    let (_, v, mut m) = context();
    let t = terms(&v);
    m.replace(t.clone());
    let first = t.delivery(0).unwrap();
    let last = t.delivery(1).unwrap();
    m.recover(vec![first.clone()]).unwrap();
    assert_eq!(m.bargain.as_ref().unwrap().confirmed, 1);
    assert!(m.bargain.as_ref().unwrap().resolution.is_none());
    let raw = serde_json::to_string(&m).unwrap();
    m = serde_json::from_str(&raw).unwrap();
    m.bind();
    assert!(m.recover(vec![first.clone(), first.clone()]).is_err());
    assert!(m.recover(vec![]).is_err());
    let mut stale = last.clone();
    stale.terms.version += 1;
    assert!(m.recover(vec![first.clone(), stale]).is_err());
    m.finish("character_died_remaining_balance_unenforced");
    m.recover(vec![first.clone(), last.clone()]).unwrap();
    assert!(
        m.bargain
            .as_ref()
            .unwrap()
            .resolution
            .as_ref()
            .unwrap()
            .starts_with("settled")
    );
    m.recover(vec![first, last]).unwrap();
}
