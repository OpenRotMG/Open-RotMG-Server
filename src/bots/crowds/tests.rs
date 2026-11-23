use super::*;

fn marker(id: i32, x: f32, y: f32) -> PlayerMarker {
    PlayerMarker {
        id,
        account: 100 + id,
        name: format!("P{id}"),
        position: Pos { x, y },
    }
}
/// Remembers the band of each listed spot as if a monster of that band had been seen there.
fn observe(b: &mut Brain, v: &mut PlayerView, c: &Content, terrain: u8, spots: &[Pos]) {
    let kind = c
        .objects
        .values()
        .find(|o| o.enemy && o.terrain == terrain)
        .unwrap()
        .kind;
    for (i, spot) in spots.iter().enumerate() {
        let mut enemy = crate::bots::quest_tests::fixture().1.objects.remove(0);
        enemy.kind = kind;
        enemy.id = 50 + i as i32;
        enemy.position = *spot;
        v.objects = vec![enemy];
        b.crowd
            .bands
            .observe(v, c, &mut b.mind.godlands, false, b.profile.identity);
    }
    v.objects.clear();
}
fn decide(
    profile: Profile,
    level: i32,
    markers: Vec<PlayerMarker>,
    terrain: Option<u8>,
) -> Decision {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.quest = None;
    v.objects.clear();
    v.character.level = level;
    v.player_markers = markers;
    let mut b = Brain::new(profile, &v);
    if let Some(terrain) = terrain {
        let spots: Vec<_> = std::iter::once(v.position)
            .chain(v.player_markers.iter().map(|p| p.position))
            .collect();
        observe(&mut b, &mut v, &c, terrain, &spots);
    }
    b.decide(&v, &c).unwrap()
}

#[test]
fn midlands_player_teleports_to_a_crowd_in_its_own_area_instead_of_exploring() {
    // Live: 3 of 41 Realm bots below level 20 had another bot within 10 tiles; most explored alone.
    let pair = || vec![marker(3, 70.5, 10.5), marker(4, 72.5, 10.5)];
    let d = decide(ARCHETYPES[0], 11, pair(), Some(6));
    assert!(
        matches!(d.journey, Some(Journey::Regroup(3 | 4))),
        "{:?} {:?}",
        d.goal,
        d.journey
    );
    // A nearby crowd counts as the player's own area even on ground it has not seen.
    let d = decide(ARCHETYPES[0], 11, pair(), None);
    assert!(
        matches!(d.journey, Some(Journey::Regroup(3 | 4))),
        "{:?} {:?}",
        d.goal,
        d.journey
    );
    // Live: 233 of 400 gathering teleports went to a lone player, who moved on.
    let d = decide(ARCHETYPES[0], 11, vec![marker(3, 70.5, 10.5)], Some(6));
    assert!(
        !matches!(d.journey, Some(Journey::Regroup(_))),
        "{:?} {:?}",
        d.goal,
        d.journey
    );
}

#[test]
fn nearby_solo_gathering_finishes_the_approach_without_reacquiring() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.quest = None;
    v.character.level = 11;
    v.player_markers = vec![marker(3, 18.5, 10.5)];
    let mut profile = ARCHETYPES[0];
    profile.knows_minimap = true;
    let mut brain = Brain::new(profile, &v);
    let spots = [v.position, v.player_markers[0].position];
    observe(&mut brain, &mut v, &c, 6, &spots);
    for (x, peer, following) in [
        (10.5, 18.5, true),
        (12.5, 18.5, true),
        (14.5, 18.5, false),
        (10.5, 18.5, false),
        (10.5, 60.5, false),
        (10.5, 18.5, true),
    ] {
        v.tick += 5;
        v.time_ms += 250;
        v.position.x = x;
        v.player_markers[0].position.x = peer;
        let decision = brain.decide(&v, &c).unwrap();
        if following {
            assert_eq!(decision.goal, Goal::Follow(3), "x={x}, peer={peer}");
            assert!(decision.movement.unwrap().x > x);
        } else {
            assert!(
                matches!(decision.goal, Goal::Explore(..)),
                "{:?} at x={x}, peer={peer}",
                decision.goal
            );
        }
        brain.goal = decision.goal;
        brain.destination = decision.movement;
    }
}

#[test]
fn reaching_a_crowd_does_not_start_an_approach_to_its_other_side() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.quest = None;
    v.character.level = 11;
    v.position.x = 0.5;
    v.player_markers = vec![
        marker(3, 20.5, 10.5),
        marker(4, 6.5, 10.5),
        marker(5, 13.5, 10.5),
    ];
    let mut brain = Brain::new(ARCHETYPES[0], &v);
    assert!(brain.crowd.knows_minimap);
    let spots: Vec<_> = v.player_markers.iter().map(|p| p.position).collect();
    observe(&mut brain, &mut v, &c, 6, &spots);
    let approach = brain.decide(&v, &c).unwrap();
    assert_eq!(approach.goal, Goal::Follow(5));
    assert!(approach.movement.is_some_and(|p| p.x > v.position.x));
    brain.goal = approach.goal;
    brain.destination = approach.movement;

    v.position.x = 5.5;
    v.player_markers.push(marker(6, 6.5, 12.5));
    v.tick += 10;
    v.time_ms += 500;
    let kept = brain.decide(&v, &c).unwrap();
    assert_eq!(kept.goal, Goal::Follow(5));
    assert!(kept.movement.is_some_and(|p| p.x > v.position.x));
    brain.goal = kept.goal;
    brain.destination = kept.movement;

    v.position.x = 10.5;
    v.tick += 10;
    v.time_ms += 500;
    let arrived = brain.decide(&v, &c).unwrap();
    assert!(
        matches!(arrived.goal, Goal::Explore(..)),
        "{:?}",
        arrived.goal
    );
}

#[test]
fn godlands_farmer_teleports_to_one_interior_marker_on_the_ordinary_cooldown() {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    let destination = marker(4, 70.5, 70.5);
    let edge = marker(3, 67.5, 67.5);
    v.quest = None;
    v.objects.clear();
    v.character.level = 20;
    v.map.layout = Some("layout".into());
    v.map.godlands = Arc::new(vec![destination.position]);
    v.map.godland_tiles = Arc::new(
        (67..=73)
            .flat_map(|x| (67..=73).map(move |y| (x, y)))
            .collect(),
    );
    v.player_markers = vec![edge, destination];
    let mut b = Brain::new(ARCHETYPES[0], &v);
    b.crowd
        .bands
        .observe(&v, &c, &mut b.mind.godlands, true, b.profile.identity);
    v.character.hp = v.character.stats[0] * 4 / 5;
    assert!(b.decide(&v, &c).unwrap().journey.is_none());
    v.character.hp = v.character.stats[0];
    v.tick += 10;
    v.time_ms += 500;
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Regroup(4))
    ));
    v.tick += 10;
    v.time_ms += 500;
    assert!(b.decide(&v, &c).unwrap().journey.is_none());
}

#[test]
fn ambitious_low_level_joins_a_fame_train_but_a_cautious_one_does_not() {
    let train = || {
        (3..6)
            .map(|id| marker(id, 70.5 + id as f32, 60.5))
            .collect::<Vec<_>>()
    };
    let mut ambitious = ARCHETYPES[0];
    ambitious.recovery = 0.36;
    let d = decide(ambitious, 5, train(), None);
    assert!(
        matches!(d.journey, Some(Journey::Regroup(_))),
        "{:?} {:?}",
        d.goal,
        d.journey
    );
    let mut cautious = ARCHETYPES[0];
    cautious.recovery = 0.62;
    let d = decide(cautious, 5, train(), None);
    assert!(
        !matches!(d.journey, Some(Journey::Regroup(_))),
        "{:?} {:?}",
        d.goal,
        d.journey
    );
}

/// A level-20 farmer in a pile on wide Godlands, with spots to walk between every 24 tiles east and west.
fn farmer(reaction: u64, mut markers: Vec<PlayerMarker>) -> (Content, PlayerView, Brain) {
    let (c, mut v) = crate::bots::quest_tests::fixture();
    v.quest = None;
    v.objects.clear();
    v.character.level = 20;
    v.character.class = 0x307;
    v.character.items[..3].copy_from_slice(&[0xb02, -1, 0xaf9]);
    v.position = Pos { x: 60.5, y: 60.5 };
    v.map.layout = Some("layout".into());
    v.map.godland_tiles = Arc::new(
        (10..=120)
            .flat_map(|x| (40..=80).map(move |y| (x, y)))
            .collect(),
    );
    v.map.godland_route = Arc::new(
        (0..5)
            .map(|i| Pos {
                x: 12.5 + 24. * i as f32,
                y: 60.5,
            })
            .collect(),
    );
    markers.extend([
        marker(3, 62.5, 62.5),
        marker(4, 58.5, 62.5),
        marker(5, 68.5, 58.5),
        marker(6, 56.5, 58.5),
        marker(7, 64.5, 63.5),
        marker(8, 60.5, 56.5),
    ]);
    v.player_markers = markers;
    let mut profile = ARCHETYPES[0];
    profile.reaction = reaction;
    let mut b = Brain::new(profile, &v);
    b.crowd
        .bands
        .observe(&v, &c, &mut b.mind.godlands, true, b.profile.identity);
    (c, v, b)
}
fn step(b: &mut Brain, v: &mut PlayerView, c: &Content) -> Decision {
    v.tick += 5;
    v.time_ms += 250;
    let decision = b.decide(v, c).unwrap();
    b.goal = decision.goal.clone();
    b.destination = decision.movement;
    decision
}

#[test]
fn the_experienced_walk_on_from_an_emptied_spot_while_the_rest_stand() {
    // Live: a pile of level 20s chased each other for minutes on Godlands they had emptied; nobody led anywhere.
    for (reaction, leaves) in [(3, true), (7, false)] {
        let (c, mut v, mut b) = farmer(reaction, vec![]);
        b.course = 0.;
        for _ in 0..3 {
            let waiting = step(&mut b, &mut v, &c);
            assert!(waiting.movement.is_none(), "{:?}", waiting.goal);
        }
        let restless = step(&mut b, &mut v, &c);
        if leaves {
            assert_eq!(restless.goal, Goal::Venture);
            assert!(restless.movement.is_some_and(|p| p.x > v.position.x));
            assert!(matches!(b.travel, Some((Goal::Venture, _, to)) if tile(to) == (108, 60)));
        } else {
            assert!(restless.movement.is_none(), "{:?}", restless.goal);
        }
    }
}

#[test]
fn a_pile_follows_whoever_walks_on_and_keeps_to_them() {
    let (c, mut v, mut b) = farmer(7, vec![marker(9, 61.5, 59.5), marker(10, 59.5, 61.5)]);
    b.course = std::f32::consts::PI;
    let mut followed = 0;
    for i in 0..16 {
        // One walks east at 5 tiles a second; a second sets off after it a little later.
        v.player_markers[0].position.x += 1.25;
        if i >= 4 {
            v.player_markers[1].position.x += 1.25;
        }
        let d = step(&mut b, &mut v, &c);
        match d.goal {
            Goal::Follow(9) => {
                followed += 1;
                assert!(d.movement.is_some_and(|p| p.x > v.position.x));
                v.position.x += 1.25;
            }
            goal => assert_eq!(followed, 0, "left its walker for {goal:?} at step {i}"),
        }
    }
    assert!(followed >= 6, "{followed}");
    assert!(
        b.course.abs() < 0.1,
        "a follower takes its course from the walker: {}",
        b.course
    );
}

#[test]
fn a_god_in_sight_outranks_standing_with_the_pile() {
    let (c, mut v, mut b) = farmer(7, vec![]);
    let mut god = crate::bots::quest_tests::fixture().1.objects.remove(0);
    (god.id, god.kind) = (40, 0x654);
    (god.hp, god.max_hp) = (2000, 2000);
    god.position = Pos { x: 60.5, y: 73.5 };
    v.objects.push(god);
    assert_eq!(step(&mut b, &mut v, &c).goal, Goal::Hunt(40));
}
