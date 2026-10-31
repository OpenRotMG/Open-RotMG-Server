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
        b.crowd.observe(v, c);
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
