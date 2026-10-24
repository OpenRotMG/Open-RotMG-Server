use super::tests::fixture;
use super::*;
use rand::{RngCore, rngs::StdRng};

const STOCK: [i32; 8] = [0xa1d, 0xa1d, 0xa22, 0xa22, -1, -1, -1, -1];
fn terrain(v: &mut PlayerView) -> Grid {
    let geometry =
        crate::navigation::Geometry::new((20, 20), vec![0; 400], &BTreeMap::from([(0, 1.)]));
    v.tiles = Arc::new(crate::navigation::Tiles::new(
        geometry,
        (0..20).flat_map(|x| (0..20).map(move |y| (x, y))),
    ));
    Grid::terrain(v)
}

#[test]
fn market_session_clock_is_sampled_once_and_expiry_releases_this_visit() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut durations = BTreeSet::new();
    for seed in 0..32 {
        let (mut m, mut v) = fixture(&c, STOCK);
        let grid = terrain(&mut v);
        let mut rng = StdRng::seed_from_u64(seed);
        let mut replay = rng.clone();
        let duration = replay.gen_range(60..=1800) * 1000u64;
        assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_some());
        let session = m.session.clone().unwrap();
        assert_eq!(session.until, v.elapsed_ms + duration);
        assert!((60_000..=1_800_000).contains(&duration));
        durations.insert(duration);
        assert_eq!(rng.clone().next_u64(), replay.next_u64());
        let next_random = rng.clone().next_u64();
        for elapsed in [1001, 2000, 9000] {
            v.elapsed_ms = elapsed;
            m.next_ad = 0;
            m.outgoing = None;
            assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_some());
            assert_eq!(m.session.as_ref().unwrap(), &session);
        }
        v.elapsed_ms = session.until;
        assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
        assert!(m.session.is_none() && m.session_used && m.outgoing.is_none());
        v.elapsed_ms += 30_000;
        assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
        assert_eq!(rng.clone().next_u64(), next_random);
        v.map.name = "Realm".into();
        m.observe(&v);
        v.map.name = "Nexus".into();
        v.position = Pos { x: 8.5, y: 8.5 };
        assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_some());
        assert_eq!(m.session.as_ref().unwrap().anchor, (8, 8));
        assert!(m.session.as_ref().unwrap().until > v.elapsed_ms);
    }
    assert!(durations.len() > 1);
}

#[test]
fn market_session_moves_only_on_cadence_with_local_clear_segments() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let (mut m, mut v) = fixture(&c, STOCK);
    let mut grid = terrain(&mut v);
    let mut rng = StdRng::seed_from_u64(17);
    m.decide(&v, &c, None, &grid, &mut rng).unwrap();
    v.elapsed_ms = m.session.as_ref().unwrap().next_move;
    let d = m.decide(&v, &c, None, &grid, &mut rng).unwrap().unwrap();
    let target = d.movement.unwrap();
    assert!(target.distance(center(m.session.as_ref().unwrap().anchor)) <= 1.5);
    assert!(grid.segment(v.position, target));
    let session = m.session.clone();
    let next_random = rng.clone().next_u64();
    v.elapsed_ms += 1;
    m.decide(&v, &c, None, &grid, &mut rng).unwrap();
    assert_eq!(m.session, session);
    assert_eq!(rng.clone().next_u64(), next_random);
    v.position = target;
    assert!(
        m.decide(&v, &c, None, &grid, &mut rng)
            .unwrap()
            .unwrap()
            .movement
            .is_none()
    );
    let (x, y) = tile(v.position);
    for p in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
        grid.walkable.remove(&p);
    }
    v.elapsed_ms = m.session.as_ref().unwrap().next_move;
    let d = m.decide(&v, &c, None, &grid, &mut rng).unwrap().unwrap();
    assert!(d.movement.is_none() && d.journey.is_none());
    assert_eq!(rng.clone().next_u64(), next_random);
    assert_eq!(m.session.as_ref().unwrap().until, session.unwrap().until);
}

#[test]
fn market_session_requires_real_stock_and_need_and_native_trade_preempts_expiry() {
    let mut c = Content::load(std::path::Path::new("data")).unwrap();
    let (mut m, mut v) = fixture(&c, [-1, -1, 0xa22, 0xa22, -1, -1, -1, -1]);
    let grid = terrain(&mut v);
    let mut rng = StdRng::seed_from_u64(9);
    let next_random = rng.clone().next_u64();
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
    assert!(!m.session_used);
    assert_eq!(rng.clone().next_u64(), next_random);
    v.character.items[4..12].copy_from_slice(&STOCK);
    m.decide(&v, &c, None, &grid, &mut rng).unwrap();
    v.elapsed_ms = m.session.as_ref().unwrap().until;
    m.trade = Some(SessionTrade {
        binding: [-2, 10, 1],
        peer: "Peer".into(),
        items: [vec![], vec![]],
        tradeable: [false; 12],
        offers: [[false; 12]; 2],
        ready: 0,
        ready_at: Instant::now(),
        changed: false,
    });
    let next_random = rng.clone().next_u64();
    let d = m.decide(&v, &c, None, &grid, &mut rng).unwrap().unwrap();
    assert!(d.movement.is_none() && d.journey.is_none());
    assert!(m.session.is_some());
    assert_eq!(rng.clone().next_u64(), next_random);
    m.trade = None;
    m.replace(Terms {
        version: 1,
        accounts: [1, 2],
        names: ["Trader".into(), "Peer".into()],
        goods: [
            Goods {
                kind: 0xa1d,
                quantity: 1,
            },
            Goods {
                kind: 0xa23,
                quantity: 1,
            },
        ],
        meeting: [-2, 5, 5],
        expires: now() + 120,
        batch: 1,
    });
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_some());
    assert_eq!(rng.clone().next_u64(), next_random);
    m.bargain = None;
    m.outgoing = None;
    v.elapsed_ms -= 1;
    v.character.items[4] = -1;
    v.character.items[5] = -1;
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
    assert!(m.session.is_none() && m.outgoing.is_none());
    v.character.items[4..12].copy_from_slice(&STOCK);
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());

    // A restricted equipment catalog isolates the fully-satisfied-needs boundary.
    c.objects.retain(|k, _| {
        [
            0x30e, 0xa97, 0xa2e, 0xa1d, 0xa22, 0xa23, 0xa1f, 0xa20, 0xa21, 0xa34, 0xa35, 0xa4c,
        ]
        .contains(k)
    });
    let (mut m, mut v) = fixture(&c, [0xa1d, 0xa1d, 0xa22, 0xa22, 0xa23, 0xa23, -1, -1]);
    v.character.base_stats = c.classes[&v.character.class].caps;
    let grid = terrain(&mut v);
    assert!(m.want(&v, &c, None).unwrap().is_none());
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
    assert!(!m.session_used);
}

#[test]
fn market_session_observation_drops_only_expired_ads_before_publication() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let (mut m, mut v) = fixture(&c, STOCK);
    let grid = terrain(&mut v);
    let mut rng = StdRng::seed_from_u64(9);
    m.decide(&v, &c, None, &grid, &mut rng).unwrap();
    let session = m.session.clone();
    let ad = m.outgoing.clone();
    assert!(ad.as_ref().unwrap().text.starts_with("sell "));
    m.trade = Some(SessionTrade {
        binding: [-2, 10, 1],
        peer: "Peer".into(),
        items: [vec![], vec![]],
        tradeable: [false; 12],
        offers: [[false; 12]; 2],
        ready: 0,
        ready_at: Instant::now(),
        changed: false,
    });
    let trade = m.trade.clone();
    let next_random = rng.clone().next_u64();
    v.elapsed_ms = session.as_ref().unwrap().until - 1;
    m.next_message_ms = v.elapsed_ms + 1;
    m.observe(&v);
    assert_eq!(m.outgoing, ad);
    v.elapsed_ms += 1;
    m.observe(&v);
    assert!(m.outgoing.is_none());
    assert_eq!(m.session, session);
    assert!(m.session_used);
    assert_eq!(m.trade, trade);
    for verb in ["offer", "counter", "agree", "no deal:"] {
        m.queue(
            format!("Peer, {verb} terms"),
            Channel::Tell("Peer".into()),
            verb == "agree",
        );
        let message = m.outgoing.clone();
        m.observe(&v);
        assert_eq!(m.outgoing, message);
        assert_eq!(m.session, session);
        assert!(m.session_used);
        assert_eq!(m.trade, trade);
    }
    let d = m.decide(&v, &c, None, &grid, &mut rng).unwrap().unwrap();
    assert!(d.movement.is_none() && d.journey.is_none());
    assert_eq!(m.session, session);
    assert_eq!(m.trade, trade);
    assert_eq!(rng.clone().next_u64(), next_random);
    m.trade = None;
    assert!(m.decide(&v, &c, None, &grid, &mut rng).unwrap().is_none());
    assert!(m.session.is_none() && m.session_used);
}

#[test]
fn market_session_idle_hook_preserves_item_and_releases_normal_portal_choice() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let (_, mut v) = fixture(&c, [0xa1d, 0xa1d, 0xa22, 0xa22, 0xa23, -1, -1, -1]);
    terrain(&mut v);
    v.objects[0].kind = 0x720;
    v.objects[0].account = None;
    v.objects[0].equipped.clear();
    v.objects[0].name = "Vault".into();
    let profile = *PROFILES.iter().find(|p| p.name == "Pixl").unwrap();
    let mut brain = Brain::new(profile, &v);
    v.elapsed_ms = 100_000;
    v.time_ms = 100_000;
    v.character.mp = 0;
    let d = brain.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Wait);
    assert!(d.journey.is_none());
    assert!(matches!(d.item, Some(Action::Use { slot: 8, .. })));
    v.character.mp = v.character.stats[1];
    v.elapsed_ms = brain.mind.market.session.as_ref().unwrap().until;
    v.time_ms = v.elapsed_ms;
    v.tick += 1;
    let d = brain.decide(&v, &c).unwrap();
    assert!(matches!(d.journey, Some(Journey::Portal(20))));
    assert!(brain.mind.market.session.is_none());
}
