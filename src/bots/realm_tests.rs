use super::*;

fn nexus(content: &Content, account: i32) -> (Brain, PlayerView) {
    let mut v = tests::view(content);
    v.session.account = account;
    v.session.world = -2;
    v.map.name = "Nexus".into();
    let mut quiet = tests::object(10, 0x712, 19.5, 11.5);
    quiet.name = "Beholder (0/85)".into();
    let mut busy = tests::object(11, 0x712, 19.5, 15.5);
    busy.name = "Medusa (3/85)".into();
    v.objects = vec![quiet, busy];
    let b = Brain::new(PROFILES[1], &v);
    v.time_ms += 10001;
    v.elapsed_ms += 10001;
    v.tick += 201;
    (b, v)
}

#[test]
fn nexus_realm_entry_prefers_the_public_crowd_instead_of_the_first_portal() {
    let content = tests::content();
    let mut busy = 0;
    for account in 1..=512 {
        let (mut brain, v) = nexus(&content, account);
        match brain.decide(&v, &content).unwrap().goal {
            Goal::Portal(11) => busy += 1,
            Goal::Portal(10) => {}
            goal => panic!("expected Realm entry, got {goal:?}, account={account}"),
        }
    }
    assert!(
        (400..480).contains(&busy),
        "populated Realm selected {busy}/512 times; expected approximately 6/7"
    );
}

#[test]
fn nexus_realm_choice_stays_committed_until_the_entrance_is_unavailable() {
    let content = tests::content();
    let (mut brain, mut v) = nexus(&content, 365);
    let first = brain.decide(&v, &content).unwrap();
    let Goal::Portal(selected) = first.goal else {
        panic!("expected Realm entry")
    };
    brain.goal = first.goal;
    brain.destination = first.movement;
    v.position = first.movement.unwrap();
    v.time_ms += 500;
    v.elapsed_ms += 500;
    v.tick += 10;
    for portal in &mut v.objects {
        portal.name = format!("Realm ({}/85)", if portal.id == selected { 0 } else { 20 });
    }
    let random = rand::RngCore::next_u64(&mut brain.rng.clone());
    assert_eq!(
        brain.decide(&v, &content).unwrap().goal,
        Goal::Portal(selected)
    );
    assert_eq!(
        rand::RngCore::next_u64(&mut brain.rng.clone()),
        random,
        "a committed outing must not draw again"
    );
    v.objects
        .iter_mut()
        .find(|p| p.id == selected)
        .unwrap()
        .usable = false;
    v.time_ms += 500;
    v.elapsed_ms += 500;
    v.tick += 10;
    let next = brain.decide(&v, &content).unwrap();
    let Goal::Portal(replacement) = next.goal else {
        panic!("another usable Realm is available")
    };
    assert_ne!(replacement, selected);
}

#[test]
fn nexus_uses_an_open_alternative_and_reports_malformed_public_counts() {
    let content = tests::content();
    for second_open in [true, false] {
        let (mut brain, mut v) = nexus(&content, 365);
        v.objects[0].usable = false;
        v.objects[0].name = "Unavailable".into();
        v.objects[1].usable = second_open;
        brain.goal = Goal::Portal(10);
        assert_eq!(
            brain.decide(&v, &content).unwrap().goal,
            if second_open {
                Goal::Portal(11)
            } else {
                Goal::Wait
            }
        );
    }
    let (mut brain, mut v) = nexus(&content, 365);
    v.objects[1].name = "Medusa (unknown/85)".into();
    let error = brain
        .decide(&v, &content)
        .err()
        .expect("do not silently guess a missing population");
    assert!(format!("{error:#}").contains("Medusa (unknown/85)"));
}

#[test]
fn realm_crowd_weights_preserve_quiet_choices_and_symmetric_ties() {
    let mut rng = rand::rngs::StdRng::seed_from_u64(365);
    for (choices, expected) in [
        (vec![(-3, 0), (1, 3)], vec![1000, 6000]),
        (vec![(1, 3), (-3, 0)], vec![6000, 1000]),
        (vec![(-3, 0), (1, 0)], vec![3500, 3500]),
        (vec![(-3, 3), (1, 3), (2, 0)], vec![6000, 6000, 1000]),
    ] {
        let mut counts = BTreeMap::<i32, usize>::new();
        for _ in 0..expected.iter().sum::<usize>() {
            *counts
                .entry(realms::choose(365, &choices, &mut rng).unwrap())
                .or_default() += 1;
        }
        for ((id, _), expected) in choices.iter().zip(expected) {
            assert!(
                counts[id].abs_diff(expected) < 250,
                "choices={choices:?}, counts={counts:?}"
            );
        }
    }
    assert!(realms::choose(365, &[], &mut rng).is_err());
    assert_eq!(realms::choose(365, &[(4, 0)], &mut rng).unwrap(), 4);
}
