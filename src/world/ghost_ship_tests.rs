use super::*;

fn ship_world() -> (World, i32, i32, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map {
        width: 128,
        height: 128,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            128 * 128
        ],
        spawns: vec![(64.5, 64.5)],
    });
    let mut w = World::new(content, map).unwrap();
    w.name = "Realm".into();
    w.encounter = true;
    let mut ch = Character::fresh(w.content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let player = w.enter(ch, None).unwrap();
    let ship = w
        .spawn_entity(0xe37, Pos { x: 64.5, y: 64.5 }, vec![])
        .unwrap();
    let anchor = w
        .spawn_entity(0xe38, Pos { x: 64.5, y: 64.5 }, vec![])
        .unwrap();
    w.landmark_bind(ship).unwrap();
    w.landmark_bind(anchor).unwrap();
    (w, ship, anchor, player)
}

#[test]
fn ghost_ship_damage_phases_remain_reachable_and_summons_are_owned() {
    let (mut w, ship, _, _) = ship_world();
    for (time, hp, stage, shield) in [
        (0, 50000, 0, false),
        (50, 48999, 1, true),
        (3050, 48999, 2, true),
        (3350, 48999, 3, false),
        (3400, 44999, 4, true),
        (5400, 44999, 5, false),
        (7400, 44999, 5, true),
        (9900, 44999, 5, false),
        (9950, 34999, 6, true),
        (11950, 34999, 6, false),
        (12000, 24999, 7, true),
        (14000, 24999, 7, false),
        (14050, 19999, 8, false),
        (14100, 19999, 8, false),
        (15550, 19999, 9, true),
        (17550, 19999, 9, false),
        (17600, 9999, 10, false),
        (17700, 9999, 11, true),
        (19700, 9999, 11, false),
    ] {
        w.now = time;
        w.entities.get_mut(&ship).unwrap().hp = hp;
        w.named_birth_step();
        w.ghost_ship_mechanics(ship, Some(Pos { x: 65., y: 65. }), 50)
            .unwrap();
        let e = &w.entities[&ship];
        assert_eq!(e.stage, Some(stage), "time={time}");
        assert_eq!(
            e.condition & condition::bit(condition::INVULNERABLE) != 0,
            shield,
            "stage={stage}, time={time}"
        );
    }
    assert_eq!(w.landmark_members(ship, 0xe3b).len(), 10);
    for kind in [0xe39, 0xe4f] {
        assert!(!w.landmark_members(ship, kind).is_empty());
        assert!(
            w.landmark_members(ship, kind)
                .into_iter()
                .all(|id| *w.entities[&id].quest == Some(ship))
        );
    }
}

#[test]
fn ghost_ship_first_and_replacement_summoned_spirits_do_not_award_xp() {
    let (mut w, ship, _, player) = ship_world();
    w.landmark_enter(ship, 3);
    let xp = w.players[&player].character.exp;
    let mut previous = None;
    for time in [4000, 8000] {
        w.now = time;
        w.ghost_ship_mechanics(ship, None, 50).unwrap();
        let children = w.landmark_members(ship, 0xe39);
        assert_eq!(children.len(), 1);
        let child = children[0];
        assert_ne!(previous, Some(child));
        assert!(w.entities[&child].script.as_ref().unwrap().no_experience);
        w.entities.get_mut(&child).unwrap().damage.insert(1, 2000);
        w.defeat_entity(child).unwrap();
        assert_eq!(w.players[&player].character.exp, xp);
        previous = Some(child);
    }
}

#[test]
fn ghost_ship_portal_roll_and_anchor_timers_complete_without_a_nearby_player() {
    let mut outcomes = BTreeSet::new();
    for seed in 0..16 {
        let (mut w, ship, anchor, player) = ship_world();
        w.players.get_mut(&player).unwrap().pos = Pos { x: 1.5, y: 1.5 };
        w.rng = rand::rngs::StdRng::seed_from_u64(seed);
        w.defeat_entity(ship).unwrap();
        let earned = w.entities[&anchor].stage == Some(2);
        outcomes.insert(earned);
        if earned {
            for (dx, dy) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                assert_eq!(
                    w.map.cells[((64 + dy) * 128 + 64 + dx) as usize].ground,
                    0xed
                );
            }
            w.now = 1000;
            w.ghost_ship_mechanics(anchor, None, 0).unwrap();
            assert_eq!(w.portal_requests.len(), 1);
            assert_eq!(w.portal_requests[0].0, 0x741);
            assert_eq!(
                w.portal_requests[0].1.distance(Pos { x: 64.5, y: 64.5 }),
                0.
            );
            w.now = 1499;
            w.ghost_ship_mechanics(anchor, None, 0).unwrap();
            assert_eq!(w.entities.values().filter(|e| e.kind == 0x18e).count(), 0);
            w.now = 1500;
            w.ghost_ship_mechanics(anchor, None, 0).unwrap();
            assert!(!w.entities.contains_key(&anchor));
            assert_eq!(w.entities.values().filter(|e| e.kind == 0x18e).count(), 2);
            assert_eq!(w.portal_requests.len(), 1);
        } else {
            assert_eq!(w.entities[&anchor].stage, Some(3));
            assert!(w.portal_requests.is_empty());
        }
    }
    assert_eq!(outcomes, BTreeSet::from([false, true]));
}
