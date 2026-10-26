use super::*;
use crate::world::view::{PlayerMarker, VisibleObject};

fn scene(level: i32) -> (Content, PlayerView) {
    let (c, mut v) = quest_tests::fixture();
    v.character.level = level;
    v.character.base_stats[4] = c.classes[&v.character.class].caps[4];
    v.character.items[4..6].fill(0xa22);
    v.quest = None;
    v.objects.clear();
    (c, v)
}

fn peer(v: &mut PlayerView, id: i32, position: Pos) {
    v.player_markers.push(PlayerMarker {
        id,
        account: id,
        name: format!("peer{id}"),
        position,
    });
    v.objects.push(VisibleObject {
        id,
        account: Some(id),
        kind: crate::content::WIZARD,
        name: format!("peer{id}"),
        position,
        hp: 500,
        max_hp: 500,
        level: Some(20),
        equipped: vec![],
        contents: vec![],
        blocks: false,
        usable: false,
        condition_mask: Some(0),
    });
}

#[test]
fn arrived_three_brain_company_chain_resumes_independent_routes() {
    let separated = [
        Pos { x: 10.5, y: 10.5 },
        Pos { x: 19., y: 10.5 },
        Pos { x: 15., y: 18. },
    ];
    let arrived = [
        Pos { x: 10.5, y: 10.5 },
        Pos { x: 14., y: 10.5 },
        Pos { x: 12., y: 13.5 },
    ];
    for account in 1..=3 {
        let (c, mut v) = scene(5);
        v.session.account = account;
        v.session.player = account;
        let target = account % 3 + 1;
        v.position = separated[(account - 1) as usize];
        peer(&mut v, target, separated[(target - 1) as usize]);
        let mut b = Brain::new(ARCHETYPES[0], &v);
        let d = b.decide(&v, &c).unwrap();
        assert_eq!(d.goal, Goal::Follow(target));
        assert!(d.movement.is_some());
        b.goal = d.goal;
        v.position = arrived[(account - 1) as usize];
        v.player_markers[0].position = arrived[(target - 1) as usize];
        v.objects[0].position = arrived[(target - 1) as usize];
        v.time_ms += 500;
        v.elapsed_ms += 500;
        v.tick += 10;
        let d = b.decide(&v, &c).unwrap();
        assert!(
            !matches!(d.goal, Goal::Follow(_)),
            "idle chain member {account}"
        );
        assert!(d.movement.is_some(), "an ordinary independent route exists");
    }
}

#[test]
fn passive_company_yields_to_suitable_distant_quest() {
    let (c, mut v) = scene(20);
    peer(&mut v, 3, Pos { x: 19., y: 10.5 });
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Follow(3));
    b.goal = d.goal;
    let enemy = quest_tests::fixture().1.objects.remove(0);
    v.quest = Some(enemy.id);
    v.objects.push(enemy);
    v.tick += 10;
    v.time_ms += 500;
    v.elapsed_ms += 500;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(2));
    assert!(d.movement.unwrap().y > v.position.y);
}

#[test]
fn uncommitted_company_is_bounded_for_leveling_and_fame_not_only_speed() {
    for level in [5, 20] {
        let (c, mut v) = scene(level);
        peer(&mut v, 3, Pos { x: 19., y: 10.5 });
        let mut b = Brain::new(ARCHETYPES[0], &v);
        b.inland_reached = true;
        assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Follow(3));
        assert_eq!(
            b.intent["mode"], "optional",
            "L{level} company must not count as progress"
        );
        v.time_ms += 90_001;
        v.elapsed_ms += 90_001;
        v.tick += 1800;
        assert!(
            !matches!(b.decide(&v, &c).unwrap().goal, Goal::Follow(_)),
            "L{level}"
        );
    }
}

#[test]
fn explicit_follow_and_useful_loot_keep_their_priorities() {
    let (c, mut v) = scene(5);
    peer(&mut v, 3, Pos { x: 19., y: 10.5 });
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.companion = Some(3);
    b.invitation_until = v.time_ms + 120_000;
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Follow(3));
    let mut bag = quest_tests::fixture().1.objects.remove(0);
    bag.id = 4;
    bag.kind = 0x500;
    bag.account = None;
    bag.contents = vec![0xa22];
    bag.position = v.position;
    v.character.items[4..6].fill(-1);
    v.objects.push(bag);
    assert_eq!(
        Brain::new(ARCHETYPES[0], &v).decide(&v, &c).unwrap().goal,
        Goal::Loot(4)
    );
}

#[test]
fn equipped_twenty_leaves_passive_company_for_published_realm_event() {
    let (c, mut v) = scene(20);
    peer(&mut v, 3, Pos { x: 19., y: 10.5 });
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Follow(3));
    b.goal = d.goal;
    let mut event = quest_tests::fixture().1.objects.remove(0);
    event.id = 4;
    event.kind = 0xd56;
    event.name = "Skull Shrine".into();
    event.max_hp = c.objects[&event.kind].hp;
    event.hp = event.max_hp;
    assert!(events::ready(&v, &c));
    assert!(progression::can_fight(&v, &c, &event));
    v.quest = Some(event.id);
    v.objects.push(event);
    v.tick += 10;
    v.time_ms += 500;
    v.elapsed_ms += 500;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(4));
    assert!(d.movement.unwrap().y > v.position.y);
    assert!(d.journey.is_none(), "no blind event teleport");
    v.character.hp = 50;
    v.character.items[4..6].fill(-1);
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Recover);
}

fn hunting_scene(level: i32, known: bool) -> (Content, PlayerView) {
    let (c, mut v) = quest_tests::fixture();
    v.character.level = level;
    v.character.items[4..6].fill(0xa22);
    v.quest = None;
    v.objects.clear();
    let mut ground = vec![1; 128 * 128];
    for y in 18..30 {
        for x in 18..30 {
            ground[y * 128 + x] = 0x6a;
        }
    }
    let geometry = crate::navigation::Geometry::new(
        (128, 128),
        ground,
        &BTreeMap::from([(1, 1.), (0x6a, 1.)]),
    );
    v.tiles = Arc::new(crate::navigation::Tiles::new(
        geometry,
        (0..128)
            .flat_map(|x| (0..128).map(move |y| (x, y)))
            .filter(|(x, y)| known || !(18..30).contains(x) || !(18..30).contains(y)),
    ));
    (c, v)
}

#[test]
fn equipped_twenty_returns_to_known_godlands_after_first_inland_trip() {
    let (c, v) = hunting_scene(20, true);
    for profile in ARCHETYPES {
        let mut b = Brain::new(profile, &v);
        b.inland_reached = true;
        let d = b.decide(&v, &c).unwrap();
        assert_eq!(d.goal, Goal::Venture, "{}", profile.name);
        let p = d.movement.expect("ordinary known-terrain route");
        assert!(p.x > v.position.x || p.y > v.position.y);
        assert!(d.journey.is_none());
    }
}

#[test]
fn godlands_preference_requires_own_readiness_and_discovery() {
    for variant in 0..5 {
        let (c, mut v) = hunting_scene(if variant == 0 { 10 } else { 20 }, variant != 4);
        match variant {
            1 => v.character.items[0] = 0xa97,
            2 => {
                v.character.hp = 100;
                v.character.items[4..6].fill(-1);
            }
            3 => v.character.stats[3] = 0,
            _ => {}
        }
        let mut b = Brain::new(ARCHETYPES[0], &v);
        b.inland_reached = true;
        let d = b.decide(&v, &c).unwrap();
        assert_ne!(d.goal, Goal::Venture, "variant{variant}");
        if variant == 2 {
            assert_eq!(d.goal, Goal::Recover);
        }
    }
}

#[test]
fn known_godlands_does_not_override_close_combat_or_supplies() {
    let (c, mut v) = hunting_scene(20, true);
    let mut enemy = quest_tests::fixture().1.objects.remove(0);
    enemy.position = Pos { x: 14.5, y: 10.5 };
    let enemy_id = enemy.id;
    v.objects.push(enemy);
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Fight(enemy_id));
    v.objects[0].kind = 0x500;
    v.objects[0].contents = vec![0xa22];
    v.objects[0].position = v.position;
    v.character.items[4..6].fill(-1);
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Loot(enemy_id));
}

#[test]
fn prepared_hunter_prefers_observed_suitable_god_over_empty_ground() {
    let (c, mut v) = hunting_scene(20, true);
    let mut god = quest_tests::fixture().1.objects.remove(0);
    god.kind = 0x651;
    god.max_hp = c.objects[&god.kind].hp;
    god.hp = god.max_hp;
    god.position = Pos { x: 35.5, y: 10.5 };
    assert!(progression::can_fight(&v, &c, &god));
    v.objects.push(god);
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(2));
    assert!(d.movement.unwrap().x > v.position.x);
    assert!(d.journey.is_none());
}

#[test]
fn godlands_route_keeps_progressing_into_discovered_hunting_terrain() {
    let (c, mut v) = hunting_scene(20, true);
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    for _ in 0..80 {
        let d = b.decide(&v, &c).unwrap();
        assert_eq!(d.goal, Goal::Venture);
        let p = d.movement.expect("known hunting route stays executable");
        let scale = 0.5 / p.distance(v.position).max(0.5);
        v.position.x += (p.x - v.position.x) * scale;
        v.position.y += (p.y - v.position.y) * scale;
        b.goal = d.goal;
        b.destination = d.movement;
        v.time_ms += 500;
        v.elapsed_ms += 500;
        v.tick += 10;
    }
    assert!((18.0..30.0).contains(&v.position.x));
    assert!((18.0..30.0).contains(&v.position.y));
}

#[test]
fn genuine_shared_damage_company_can_travel_then_yield_on_arrival() {
    let (c, mut v) = hunting_scene(20, true);
    peer(&mut v, 3, Pos { x: 19., y: 10.5 });
    v.objects.push(quest_tests::fixture().1.objects.remove(0));
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    b.crowd.observe(&v, &c);
    for shooter in [v.session.player, 3] {
        let mut w = wire::Writer::new();
        w.i32(2)
            .u8(0)
            .i16(12)
            .u8(u8::from(shooter == 3))
            .u8(0)
            .i32(shooter);
        b.crowd
            .receive(&Packet { id: 47, body: w.0 }, &v, &c)
            .unwrap();
    }
    v.objects.retain(|p| p.id != 2);
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Follow(3));
    assert!(d.movement.is_some());
    assert!(d.journey.is_none());
    b.goal = d.goal;
    v.objects[0].position.x = 14.;
    v.player_markers[0].position.x = 14.;
    v.tick += 10;
    v.time_ms += 500;
    v.elapsed_ms += 500;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Venture);
    assert!(d.movement.is_some());
}

#[test]
fn isolated_known_godlands_does_not_suppress_reachable_company() {
    let (c, mut v) = hunting_scene(20, true);
    let mut ground = vec![1; 128 * 128];
    for t in v.tiles.iter() {
        ground[t.y as usize * 128 + t.x as usize] = t.ground;
    }
    for i in 17..31 {
        for (x, y) in [(i, 17), (i, 30), (17, i), (30, i)] {
            ground[y * 128 + x] = 255;
        }
    }
    let geometry = crate::navigation::Geometry::new(
        (128, 128),
        ground,
        &BTreeMap::from([(1, 1.), (0x6a, 1.), (255, 0.)]),
    );
    v.tiles = Arc::new(crate::navigation::Tiles::new(
        geometry,
        (0..128).flat_map(|x| (0..128).map(move |y| (x, y))),
    ));
    peer(&mut v, 3, Pos { x: 2.5, y: 10.5 });
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.inland_reached = true;
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Follow(3));
    assert!(d.movement.unwrap().x < v.position.x);
}
