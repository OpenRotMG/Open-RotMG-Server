use super::*;
use rand::RngCore;

fn motion_step(w: &World, id: i32, b: &Behavior, at: u64) -> Step {
    Step {
        delta_ms: 50,
        active_ms: at,
        ..step(w, id, b.local)
    }
}
fn shadow(range: f64, stop: f64, speed: f64) -> (World, i32, i32, Behavior) {
    let (w, p, id) = fixture(&format!(
        r#"<Behavior range="{range}" reshadowRange="{stop}" speed="{speed}">Shadow</Behavior>"#
    ));
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    (w, p, id, b)
}

#[test]
fn xml_runtime_world_shadow_scaled_hysteresis_zero_speed_and_xml_range_mutation() {
    let mut outcomes = vec![];
    for range in [3., 6.] {
        let (mut w, p, id, b) = shadow(range, 0.5, 3.);
        w.players.get_mut(&p).unwrap().pos.x = 70.5;
        let st = motion_step(&w, id, &b, 0);
        let mut rng = rand::rngs::StdRng::seed_from_u64(21);
        if range == 6. {
            assert!(!w.ready(Service::Movement, &b, st).unwrap());
        }
        w.players.get_mut(&p).unwrap().pos.x = 70.501;
        assert!(w.ready(Service::Movement, &b, st).unwrap());
        assert_eq!(
            w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
            ActionOutcome::Applied
        );
        let moved = w.entities[&id].pos.x - 64.5;
        assert!((f64::from(moved) - 3. * 6.001 / range * 0.05).abs() < 0.00002);
        w.players.get_mut(&p).unwrap().pos.x = w.entities[&id].pos.x + 2.;
        assert!(w.advance(&b, motion_step(&w, id, &b, 50)).unwrap());
        let before = w.entities[&id].pos;
        w.players.get_mut(&p).unwrap().pos.x = before.x + 0.5;
        assert!(!w.advance(&b, motion_step(&w, id, &b, 100)).unwrap());
        assert_eq!(w.entities[&id].pos.distance(before), 0.);
        assert!(
            !w.ready(Service::Movement, &b, motion_step(&w, id, &b, 150))
                .unwrap()
        );
        outcomes.push(moved);
    }
    assert!(outcomes[0] > outcomes[1] * 1.99);
    let (mut w, p, id, b) = shadow(3., 0., 0.);
    w.players.get_mut(&p).unwrap().pos.x = 70.5;
    let mut rng = rand::rngs::StdRng::seed_from_u64(3);
    let draw = rng.clone().next_u64();
    assert_eq!(
        w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    assert_eq!(rng.next_u64(), draw);
    assert_eq!(w.entities[&id].pos.x, 64.5);
}

#[test]
fn xml_runtime_world_shadow_probe_is_effect_free_and_explicit_teleport_has_strict_boundary() {
    for distance in [30., 30.001] {
        let (mut w, p, id, b) = shadow(3., 0.5, 3.);
        w.players.get_mut(&p).unwrap().pos.x = 64.5 + distance;
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let probe = Step {
            delta_ms: 0,
            ..step(&w, id, b.local)
        };
        w.xml_start_motion(&b, probe, &mut rng).unwrap();
        let before = w.entities[&id].pos;
        let last = w.entities[&id].last_step;
        assert!(w.xml_move(&b, probe).unwrap());
        assert_eq!(w.entities[&id].pos.distance(before), 0.);
        assert_eq!(w.entities[&id].last_step.distance(last), 0.);
        assert!(w.advance(&b, motion_step(&w, id, &b, 50)).unwrap());
        if distance == 30. {
            assert!((w.entities[&id].pos.x - 66.).abs() < 0.0001);
        } else {
            assert_eq!(w.entities[&id].pos.distance(w.players[&p].pos), 0.);
            assert!(w.xml.motions.is_empty());
        }
    }
}

#[test]
fn xml_runtime_world_shadow_teleport_skips_segment_not_endpoint_environment_or_scope() {
    let (mut w, p, id, b) = shadow(40., 0., 0.);
    w.players.get_mut(&p).unwrap().pos.x = 100.5;
    let st = step(&w, id, b.local);
    let mut rng = rand::rngs::StdRng::seed_from_u64(17);
    Arc::make_mut(&mut w.map).cells[64 * 128 + 80].ground = 255;
    Arc::make_mut(&mut w.map).cells[64 * 128 + 100].ground = 255;
    let draw = rng.clone().next_u64();
    assert!(w.ready(Service::Movement, &b, st).unwrap());
    assert_eq!(
        w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
        ActionOutcome::Blocked
    );
    assert_eq!(rng.clone().next_u64(), draw);
    assert_eq!(w.entities[&id].pos.x, 64.5);
    assert!(w.xml.motions.is_empty());
    Arc::make_mut(&mut w.map).cells[64 * 128 + 100].ground = 0x18;
    assert_eq!(
        w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
        ActionOutcome::Applied
    );
    assert_eq!(w.entities[&id].pos.x, 100.5);
    assert_eq!(w.entities[&id].last_step.x, 36.);
    let mut alien = st;
    alien.token.scope.identity.world += 1;
    assert!(w.apply(Service::Movement, &b, alien, &mut rng).is_err());
    let (mut w, p, id) = fixture(
        r#"<Behavior minAltitude="120">StayAbove</Behavior><Behavior range="3" reshadowRange="0.5" speed="3">Shadow</Behavior>"#,
    );
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[1].clone();
    w.players.get_mut(&p).unwrap().pos.x = 100.5;
    Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap())[64 * 128 + 100] = 119;
    assert_eq!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    assert_eq!(w.entities[&id].pos.x, 64.5);
}

#[test]
fn xml_runtime_world_shadow_dormancy_stasis_paralysis_and_target_loss_do_not_jump() {
    let (mut w, p, id, b) = shadow(3., 0.5, 3.);
    w.players.get_mut(&p).unwrap().pos.x = 100.5;
    frame(&mut w, 0);
    frame(&mut w, 100);
    assert_eq!(w.entities[&id].pos.x, 64.5);
    assert_eq!(w.xml.actors[&id].active_ms(), 0);
    w.players.get_mut(&p).unwrap().pos.x = 70.5;
    w.apply_condition(id, crate::condition::STASIS, 1000)
        .unwrap();
    frame(&mut w, 150);
    assert_eq!(w.entities[&id].pos.x, 64.5);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    w.apply_condition(id, crate::condition::PARALYZED, 1000)
        .unwrap();
    frame(&mut w, 200);
    assert_eq!(w.entities[&id].pos.x, 64.5);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    w.players.get_mut(&p).unwrap().departing = true;
    assert!(
        !w.ready(Service::Movement, &b, motion_step(&w, id, &b, 0))
            .unwrap()
    );
}

#[test]
fn xml_runtime_world_shadow_missing_compiler_defaults_and_invalid_ranges_refuse_contextually() {
    let (mut w, p, id) = fixture("<Behavior>Shadow</Behavior>");
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    assert_eq!(
        (n(&b, F::Range), n(&b, F::Speed), n(&b, F::ReshadowRange)),
        (3., 3., 0.5)
    );
    w.players.get_mut(&p).unwrap().pos.x = 70.5;
    let mut rng = rand::rngs::StdRng::seed_from_u64(21);
    assert_eq!(
        w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
    assert!((w.entities[&id].pos.x - 64.8).abs() < 0.00001);
    for field in [F::Range, F::Speed, F::ReshadowRange] {
        let mut incomplete = b.clone();
        incomplete.fields.remove(&field);
        let error = format!("{:#}", native_validate(&incomplete).unwrap_err());
        assert!(
            error.contains("Shadow missing compiled") && error.contains(&incomplete.source.file),
            "{error}"
        );
    }
    for body in [
        r#"<Behavior range="0" reshadowRange="0" speed="1">Shadow</Behavior>"#,
        r#"<Behavior range="1" reshadowRange="2" speed="1">Shadow</Behavior>"#,
        r#"<Behavior protecteeId="Piratess" protectionRange="3">Orbit</Behavior>"#,
        r#"<Behavior protecteeId="Piratess" protectionRange="1" reprotectRange="2">Orbit</Behavior>"#,
    ] {
        assert!(
            std::panic::catch_unwind(|| fixture(body)).is_err(),
            "{body}"
        );
    }
}

#[test]
fn xml_runtime_world_actual_seven_orbit_nodes_hysteresis_latch_and_preserved_sample() {
    let content = Arc::new(crate::world::test_content());
    let mut sources = vec![];
    for d in content.game.as_ref().unwrap().definitions.values() {
        for b in d
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .filter(|b| b.kind == K::Orbit && b.fields.contains_key(&F::ProtectionRange))
        {
            sources.push((d.kind, b.clone()));
        }
    }
    assert_eq!(sources.len(), 7);
    for (kind, b) in sources {
        let mut map = flat_map(128);
        map.elevation = Some(vec![200; 128 * 128].into());
        let mut w = World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
        let id = spawn(&mut w, kind, 64.5, 64.5);
        let anchor_kind = objects(&b, F::ProtecteeId)[0];
        let anchor = spawn(&mut w, anchor_kind, 68.5, 64.5);
        let mut rng = rand::rngs::StdRng::seed_from_u64(4);
        let st = motion_step(&w, id, &b, 0);
        assert_eq!(
            w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
            ActionOutcome::Applied
        );
        let sample = w.xml.motions[&st.token].clone();
        assert!(sample.closing);
        assert!((w.entities[&id].pos.x - 64.5 - (sample.speed * 0.05) as f32).abs() < 0.00001);
        for (distance, closing) in [
            (3., true),
            (2.5, true),
            (1.801, true),
            (1.799, false),
            (2.5, false),
            (3., false),
            (3.001, true),
        ] {
            let pos = w.entities[&id].pos;
            w.position_entity(
                anchor,
                Pos {
                    x: pos.x + distance,
                    y: pos.y,
                },
            )
            .unwrap();
            w.advance(&b, motion_step(&w, id, &b, 50)).unwrap();
            let current = &w.xml.motions[&st.token];
            assert_eq!(
                current.closing, closing,
                "{} distance={distance}",
                b.source.path
            );
            assert_eq!(
                (current.radius, current.speed, current.born),
                (sample.radius, sample.speed, sample.born)
            );
        }
        w.remove_entity(anchor);
        assert!(!w.advance(&b, motion_step(&w, id, &b, 100)).unwrap());
        assert!(!w.xml.motions.contains_key(&st.token));
    }
}

#[test]
fn xml_runtime_world_orbit_nonnegative_uniform_support_and_seeded_draw_continuity() {
    for (radius, variability) in [
        (12., 0.),
        (0., 1.5),
        (12., 0.5),
        (12., 1.),
        (12., 1.5),
        (13., 1.5),
        (2., 4.),
    ] {
        for seed in 0..32 {
            let mut actual = rand::rngs::StdRng::seed_from_u64(seed);
            let mut expected = actual.clone();
            let sample =
                super::super::movement::orbit_radius(radius, variability, &mut actual).unwrap();
            let independent = if radius == 0. || variability == 0. {
                radius
            } else {
                let lower = if variability <= 1. { -variability } else { -1. };
                radius * (1. + expected.gen_range(lower..=variability))
            };
            assert_eq!(sample, independent);
            assert_eq!(actual.next_u64(), expected.next_u64());
            assert!(sample >= 0. && sample <= radius * (1. + variability));
            if radius > 0. && variability > 0. {
                assert!(sample > 0.);
            }
        }
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(9);
    let before = rng.clone().next_u64();
    assert!(super::super::movement::orbit_radius(f64::MAX, 1.5, &mut rng).is_err());
    assert_eq!(rng.next_u64(), before);
    let full = crate::world::test_content();
    let actual: Vec<_> = full.game.as_ref().unwrap().definitions[&0x1741]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .filter(|b| b.kind == K::Orbit)
        .collect();
    assert_eq!(actual.len(), 2);
    assert_eq!(
        actual
            .iter()
            .map(|b| n(b, F::Radius) * (1. + n(b, F::RadiusVariability)))
            .collect::<Vec<_>>(),
        [30., 32.5]
    );
}

#[test]
fn xml_runtime_world_orbit_wobble_analytic_reflection_zero_radius_and_threshold_equality() {
    use super::super::movement::{closing_phase, orbit_wobble};
    assert!(!closing_phase(false, 3., 3., 1.8));
    assert!(closing_phase(true, 3., 3., 1.8));
    assert!(!closing_phase(true, 1.8, 3., 1.8));
    for (base, expected) in [
        (6., [6., 14., 6., 2.]),
        (8., [8., 16., 8., 0.]),
        (10., [10., 18., 10., 2.]),
    ] {
        for (at, expected) in [0, 1000, 2000, 3000].into_iter().zip(expected) {
            assert!((orbit_wobble(base, 8., 4000, at).unwrap() - expected).abs() < 1e-12);
        }
    }
    assert_eq!(orbit_wobble(6., 0., 0, u64::MAX).unwrap(), 6.);
    assert!(orbit_wobble(6., 8., 0, 0).is_err());
    assert!(orbit_wobble(f64::MAX, 8., 4000, 0).is_err());
    let (mut w, _, id) = fixture(
        r#"<Behavior protecteeId="Piratess" radius="8" speed="1" speedVariability="0" radiusVariability="0" radiusWobbleDistance="8" radiusWobblePeriod="4">Orbit</Behavior>"#,
    );
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let anchor = spawn(&mut w, 0x601, 66.5, 64.5);
    let mut rng = rand::rngs::StdRng::seed_from_u64(8);
    w.xml_start_motion(&b, motion_step(&w, id, &b, 0), &mut rng)
        .unwrap();
    assert!(w.advance(&b, motion_step(&w, id, &b, 3000)).unwrap());
    assert_eq!(w.entities[&id].pos.y, w.entities[&anchor].pos.y);
    assert!((w.entities[&id].pos.x - 64.55).abs() < 1e-5);
}

#[test]
fn xml_runtime_world_movement_variants_produce_finite_bounded_observed_motion() {
    for body in [
        "<Behavior>BackAndForth</Behavior>",
        "<Behavior>Buzz</Behavior>",
        "<Behavior>Charge</Behavior>",
        "<Behavior>Circle</Behavior>",
        r#"<Behavior range="0.1">Follow</Behavior>"#,
        "<Behavior>KeepDistance</Behavior>",
        "<Behavior>Move</Behavior>",
        "<Behavior>StayBack</Behavior>",
        r#"<Behavior range="2" speed="1">StayCloseToSpawn</Behavior>"#,
        "<Behavior>Swirl</Behavior>",
        "<Behavior>Wander</Behavior>",
        r#"<Behavior groundTypes="Castle Stone Floor Tile" speed="1">Walk</Behavior>"#,
        r#"<Behavior minAltitude="120">StayAbove</Behavior>"#,
        r#"<Behavior terrain="Grass">StayInTerrain</Behavior>"#,
    ] {
        let (mut w, _, id) = fixture(body);
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        if b.kind == K::StayAbove {
            Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap())[64 * 128 + 64] =
                119;
        }
        if b.kind == K::StayInTerrain {
            Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = 0x48;
        }
        if b.kind == K::StayCloseToSpawn {
            w.position_entity(id, Pos { x: 70.5, y: 64.5 }).unwrap();
        }
        let mut rng = rand::rngs::StdRng::seed_from_u64(8);
        let probe = Step {
            delta_ms: 0,
            ..step(&w, id, b.local)
        };
        let before = w.entities[&id].pos;
        assert_eq!(
            w.apply(Service::Movement, &b, probe, &mut rng).unwrap(),
            ActionOutcome::Applied,
            "{body}"
        );
        assert_eq!(
            w.entities[&id].pos.distance(before),
            0.,
            "{body} probe moved"
        );
        assert!(
            w.advance(&b, motion_step(&w, id, &b, 50)).unwrap(),
            "{body}"
        );
        let moved = w.entities[&id].pos.distance(before);
        assert!(
            moved.is_finite() && moved > 0. && moved < 1.,
            "{body}: {moved}"
        );
    }
}

#[test]
fn xml_runtime_world_selected_native_metadata_dependencies_refuse_instead_of_ignoring_fields() {
    let content = crate::world::test_content();
    let game = content.game.as_ref().unwrap();
    for (kind, operation, field) in [
        (0x606, K::Buzz, F::Range),
        (0x904, K::Protect, F::Range),
        (0xd5a, K::Orbit, F::RadiusWobbleDistance),
    ] {
        let b = game.definitions[&kind]
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .find(|b| b.kind == operation && b.fields.contains_key(&field))
            .unwrap();
        native_validate(b).unwrap();
        let mut tampered = b.clone();
        match operation {
            K::Buzz => {
                assert_eq!(n(b, F::AcquireRange), 1.);
                tampered.fields.insert(F::AcquireRange, Value::Number(20.));
            }
            K::Protect => {
                assert_eq!(
                    (
                        n(b, F::AcquireRange),
                        n(b, F::ProtectionRange),
                        n(b, F::ReprotectRange)
                    ),
                    (15., 3., 0.1)
                );
                tampered.fields.remove(&F::ReprotectRange);
            }
            K::Orbit => {
                assert_eq!(
                    (
                        n(b, F::Radius),
                        n(b, F::RadiusWobbleDistance),
                        ms(b, F::RadiusWobblePeriod)
                    ),
                    (12., 8., 15000)
                );
                tampered.fields.remove(&F::RadiusWobblePeriod);
            }
            _ => unreachable!(),
        }
        let error = format!("{:#}", native_validate(&tampered).unwrap_err());
        assert!(
            error.contains(&b.source.object) && error.contains(&b.source.file),
            "{error}"
        );
    }
}

#[test]
fn xml_runtime_world_canonical_protect_ranges_consume_independent_activation_and_stop() {
    let (mut w, _, id) = fixture(
        r#"<Behavior protecteeId="Piratess" acquireRange="15" protectionRange="3" reprotectRange="0.1" speed="1">Protect</Behavior>"#,
    );
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let anchor = spawn(&mut w, 0x601, 68.5, 64.5);
    let mut rng = rand::rngs::StdRng::seed_from_u64(9);
    let st = motion_step(&w, id, &b, 0);
    assert!(w.ready(Service::Movement, &b, st).unwrap());
    w.apply(Service::Movement, &b, st, &mut rng).unwrap();
    w.position_entity(
        anchor,
        Pos {
            x: w.entities[&id].pos.x + 0.2,
            y: 64.5,
        },
    )
    .unwrap();
    assert!(w.advance(&b, motion_step(&w, id, &b, 50)).unwrap());
    w.position_entity(
        anchor,
        Pos {
            x: w.entities[&id].pos.x + 0.09,
            y: 64.5,
        },
    )
    .unwrap();
    assert!(!w.advance(&b, motion_step(&w, id, &b, 100)).unwrap());
    w.position_entity(
        anchor,
        Pos {
            x: w.entities[&id].pos.x + 2.,
            y: 64.5,
        },
    )
    .unwrap();
    assert!(
        !w.ready(Service::Movement, &b, motion_step(&w, id, &b, 150))
            .unwrap()
    );
}

#[test]
fn xml_runtime_world_actual_canonical_motion_fields_change_observed_services_without_legacy_priority()
 {
    let content = Arc::new(crate::world::test_content());
    for kind in [0x606, 0x904, 0xd5a] {
        let operation = match kind {
            0x606 => K::Buzz,
            0x904 => K::Protect,
            _ => K::Orbit,
        };
        let source = content.game.as_ref().unwrap().definitions[&kind]
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .find(|b| b.kind == operation)
            .unwrap()
            .clone();
        let mut results = vec![];
        for changed in [false, true] {
            let mut map = flat_map(128);
            map.elevation = Some(vec![200; 128 * 128].into());
            let mut w =
                World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
            let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
            ch.id = 1;
            ch.account = 1;
            let p = w.enter(ch, None).unwrap();
            w.players.get_mut(&p).unwrap().pos = Pos { x: 66., y: 64.5 };
            let id = spawn(&mut w, kind, 64.5, 64.5);
            let mut b = source.clone();
            if changed {
                match operation {
                    K::Buzz => {
                        for f in [F::AcquireRange, F::Range] {
                            b.fields.insert(f, Value::Number(2.));
                        }
                    }
                    K::Protect => {
                        for f in [F::ReprotectRange, F::Range] {
                            b.fields.insert(f, Value::Number(0.5));
                        }
                    }
                    K::Orbit => {
                        b.fields.insert(F::Radius, Value::Number(0.));
                        b.fields.insert(F::RadiusWobbleDistance, Value::Number(0.));
                    }
                    _ => unreachable!(),
                }
            }
            let mut rng = rand::rngs::StdRng::seed_from_u64(9);
            if operation == K::Buzz {
                let ready = w
                    .ready(Service::Movement, &b, motion_step(&w, id, &b, 0))
                    .unwrap();
                assert_eq!(ready, changed);
                if ready {
                    w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
                        .unwrap();
                    assert!(w.advance(&b, motion_step(&w, id, &b, 50)).unwrap());
                    assert!(w.entities[&id].pos.x > 64.5);
                }
                results.push(ready);
            } else {
                let anchor = w
                    .spawn_entity(
                        objects(&b, F::ProtecteeId)[0],
                        Pos { x: 68.5, y: 64.5 },
                        vec![],
                    )
                    .unwrap();
                assert_eq!(
                    w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
                        .unwrap(),
                    ActionOutcome::Applied
                );
                if operation == K::Protect {
                    let pos = w.entities[&id].pos;
                    w.position_entity(
                        anchor,
                        Pos {
                            x: pos.x + 0.3,
                            y: pos.y,
                        },
                    )
                    .unwrap();
                    let moving = w.advance(&b, motion_step(&w, id, &b, 50)).unwrap();
                    assert_eq!(moving, !changed);
                    results.push(moving);
                } else {
                    let right = w.entities[&id].pos.x > 64.5;
                    assert_eq!(right, changed);
                    results.push(right);
                }
            }
        }
        assert_ne!(results[0], results[1]);
    }
    for acquire in [0.5, 2.] {
        let (mut w, _, id) = fixture(&format!(
            r#"<Behavior acquireRange="{acquire}" range="0.1">Follow</Behavior>"#
        ));
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        assert_eq!(
            w.ready(Service::Movement, &b, step(&w, id, b.local))
                .unwrap(),
            acquire == 2.
        );
        let mut unresolved = b.clone();
        unresolved.fields.insert(F::Distance, Value::Number(0.1));
        assert!(native_validate(&unresolved).is_err());
        if acquire == 2. {
            let mut rng = rand::rngs::StdRng::seed_from_u64(1);
            w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
                .unwrap();
            assert!((w.entities[&id].pos.x - 64.55).abs() < 0.00001);
        }
    }
}

#[test]
fn xml_runtime_world_stayback_distance_controls_observed_separation_not_hidden_twenty_acquisition()
{
    for distance in [20., 30.] {
        let (mut w, p, id) = fixture(&format!(
            r#"<Behavior distance="{distance}" speed="1">StayBack</Behavior>"#
        ));
        w.players.get_mut(&p).unwrap().pos.x = 89.5;
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        assert_eq!(
            w.ready(Service::Movement, &b, step(&w, id, b.local))
                .unwrap(),
            distance == 30.
        );
        if distance == 30. {
            assert_eq!(
                w.apply(
                    Service::Movement,
                    &b,
                    motion_step(&w, id, &b, 0),
                    &mut rand::rngs::StdRng::seed_from_u64(3)
                )
                .unwrap(),
                ActionOutcome::Applied
            );
            assert!(w.entities[&id].pos.x < 64.5 && w.entities[&id].pos.x > 64.4);
        }
    }
}

#[test]
fn xml_runtime_world_environment_search_has_no_twenty_tile_cutoff_and_preserves_nearest_ties() {
    let (mut w, _, id) = fixture(r#"<Behavior minAltitude="120">StayAbove</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap()).fill(119);
    let set = |w: &mut World, x: usize, y: usize, height| {
        Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap())[y * 128 + x] = height;
    };
    set(&mut w, 90, 64, 120);
    let goal = |w: &World| {
        w.xml_environment_goal(&b, w.entities[&id].pos, 0x600, scope(w, id))
            .unwrap()
    };
    let mut costs = vec![];
    let started = std::time::Instant::now();
    assert_eq!(goal(&w).unwrap().distance(Pos { x: 90.5, y: 64.5 }), 0.);
    costs.push(serde_json::json!({"case":"far26", "elapsed_ns":started.elapsed().as_nanos(),"geometric_perimeter_visits_upper_bound":2809}));
    set(&mut w, 64, 38, 121);
    assert_eq!(
        goal(&w).unwrap().distance(Pos { x: 64.5, y: 38.5 }),
        0.,
        "row wins equal distance"
    );
    set(&mut w, 38, 64, 120);
    set(&mut w, 64, 38, 119);
    assert_eq!(goal(&w).unwrap().x, 38.5, "column wins equal distance");
    w.position_entity(id, Pos { x: 64.75, y: 64.5 }).unwrap();
    assert_eq!(
        goal(&w).unwrap().x,
        90.5,
        "fractional origin changes exact nearest"
    );
    set(&mut w, 65, 64, 120);
    let started = std::time::Instant::now();
    assert_eq!(goal(&w).unwrap().x, 65.5);
    costs.push(serde_json::json!({"case":"near", "elapsed_ns":started.elapsed().as_nanos(),"geometric_perimeter_visits_upper_bound":9}));
    set(&mut w, 64, 64, 120);
    assert!(
        goal(&w).is_none(),
        "threshold equality is already satisfied"
    );
    set(&mut w, 64, 64, 119);
    Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap()).fill(119);
    let started = std::time::Instant::now();
    assert!(goal(&w).is_none());
    costs.push(serde_json::json!({"case":"no_candidate_128x128", "elapsed_ns":started.elapsed().as_nanos(),"geometric_perimeter_visits_upper_bound":16384}));
    std::fs::write(Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("world-environment-search-cost.json"),serde_json::to_vec_pretty(&serde_json::json!({"qualification":"unit wall-time samples under concurrent tests; geometric bounds are inferred, not instrumentation; not performance-oracle/64MiB acceptance", "cases":costs})).unwrap()).unwrap();
    let rng = w.rng.clone().next_u64();
    let ids = w.ids.load(Ordering::Relaxed);
    for _ in 0..2 {
        assert!(
            !w.ready(Service::Movement, &b, step(&w, id, b.local))
                .unwrap()
        );
        let mut draw = w.rng.clone();
        assert_eq!(
            w.apply(Service::Movement, &b, step(&w, id, b.local), &mut draw)
                .unwrap(),
            ActionOutcome::Blocked
        );
        assert_eq!(draw.next_u64(), rng);
        assert!(w.xml.motions.is_empty());
    }
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(w.rng.clone().next_u64(), rng);
    Arc::make_mut(&mut w.map).elevation = None;
    assert!(
        format!(
            "{:#}",
            w.ready(Service::Movement, &b, step(&w, id, b.local))
                .unwrap_err()
        )
        .contains("missing source elevation capability")
    );
}

#[test]
fn xml_runtime_world_environment_search_rectangular_edges_and_blocked_segment_do_not_snap_or_retarget()
 {
    let (mut w, _, id) = fixture(r#"<Behavior groundTypes="Grass" speed="1">Walk</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    w.position_entity(id, Pos { x: 0.5, y: 0.5 }).unwrap();
    let map = Arc::make_mut(&mut w.map);
    map.width = 96;
    map.height = 70;
    map.cells.truncate(96 * 70);
    map.elevation = None;
    map.cells[69 * 96 + 95].ground = 0x48;
    let origin = w.entities[&id].pos;
    let goal = w
        .xml_environment_goal(&b, origin, 0x600, scope(&w, id))
        .unwrap()
        .unwrap();
    assert_eq!(goal.distance(Pos { x: 95.5, y: 69.5 }), 0.);
    Arc::make_mut(&mut w.map).cells[1].ground = 0x48;
    let wall = spawn(&mut w, 0x120, 1.5, 0.5);
    assert_eq!(
        w.xml_environment_goal(&b, origin, 0x600, scope(&w, id))
            .unwrap()
            .unwrap()
            .distance(goal),
        0.,
        "closer blocked endpoint is ineligible"
    );
    w.remove_entity(wall);
    Arc::make_mut(&mut w.map).cells[1].ground = 0x18;
    Arc::make_mut(&mut w.map).cells[3].ground = 0x48;
    let wall = spawn(&mut w, 0x120, 1.5, 0.5);
    let mut rng = rand::rngs::StdRng::seed_from_u64(8);
    let before = rng.clone().next_u64();
    let st = Step {
        delta_ms: 2000,
        ..step(&w, id, b.local)
    };
    for _ in 0..2 {
        assert_eq!(
            w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
            ActionOutcome::Blocked
        );
        assert_eq!(w.entities[&id].pos.distance(origin), 0.);
        assert_eq!(rng.clone().next_u64(), before);
        assert!(w.xml.motions.is_empty());
    }
    w.remove_entity(wall);
    assert_eq!(
        w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
        ActionOutcome::Applied
    );
    assert_eq!(
        w.entities[&id].pos.distance(Pos { x: 2.5, y: 0.5 }),
        0.,
        "bounded step, not snap to cell3.5"
    );
}

#[test]
fn xml_runtime_world_environment_xml_threshold_speed_ground_and_current_ground_mutations_change_motion()
 {
    let mut outcomes = vec![];
    for speed in [0., 0.5, 1.] {
        let (mut w, _, id) = fixture(&format!(
            r#"<Behavior minAltitude="120" speed="{speed}">StayAbove</Behavior>"#
        ));
        Arc::make_mut(Arc::make_mut(&mut w.map).elevation.as_mut().unwrap())[64 * 128 + 64] = 119;
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        let mut rng = rand::rngs::StdRng::seed_from_u64(8);
        let result = w
            .apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
            .unwrap();
        assert_eq!(
            result,
            if speed == 0. {
                ActionOutcome::Blocked
            } else {
                ActionOutcome::Applied
            }
        );
        let moved = w.entities[&id].pos.distance(Pos { x: 64.5, y: 64.5 });
        assert!((f64::from(moved) - speed * 0.05).abs() < 0.00001);
        outcomes.push(moved);
    }
    assert!(outcomes[0] == 0. && outcomes[1] < outcomes[2]);
    let (mut w, _, id) = fixture(
        r#"<Behavior terrain="Grass">StayInTerrain</Behavior><Behavior groundId="Grass">ChangeGround</Behavior>"#,
    );
    let bs = w.xml_game().definitions[&0x600].states[0].behaviors.clone();
    Arc::make_mut(&mut w.map).cells[64 * 128 + 90].ground = 0x48;
    assert!(
        w.ready(Service::Movement, &bs[0], step(&w, id, bs[0].local))
            .unwrap()
    );
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    w.apply(Service::Ground, &bs[1], step(&w, id, bs[1].local), &mut rng)
        .unwrap();
    assert!(
        !w.ready(Service::Movement, &bs[0], step(&w, id, bs[0].local))
            .unwrap(),
        "current ChangeGround is authoritative"
    );
    for ground in ["Grass", "Castle Stone Floor Tile"] {
        let (w, _, id) = fixture(&format!(
            r#"<Behavior groundTypes="{ground}">Walk</Behavior>"#
        ));
        let b = &w.xml_game().definitions[&0x600].states[0].behaviors[0];
        assert_eq!(
            w.ready(Service::Movement, b, step(&w, id, b.local))
                .unwrap(),
            ground == "Castle Stone Floor Tile",
            "Walk keeps moving even when current tile is allowed"
        );
    }
}

#[test]
fn xml_runtime_world_wander_optional_duration_has_no_invented_one_second_expiry() {
    for duration in [None, Some("1")] {
        let body = duration.map_or("<Behavior>Wander</Behavior>".to_owned(), |d| {
            format!(r#"<Behavior duration="{d}">Wander</Behavior>"#)
        });
        let (mut w, _, _) = fixture(&body);
        frame(&mut w, 0);
        let token = *w.xml.motions.keys().next().unwrap();
        let goal = w.xml.motions[&token].goal;
        frame(&mut w, 999);
        assert_eq!(w.xml.motions[&token].born, 0);
        frame(&mut w, 1000);
        frame(&mut w, 1001);
        let motion = &w.xml.motions[&token];
        if duration.is_none() {
            assert_eq!(motion.born, 0);
            assert!(motion.until.is_none());
            assert_eq!(motion.goal.distance(goal), 0.);
        } else {
            assert_eq!(motion.born, 1000);
            assert_eq!(motion.until, Some(2000));
            assert!(motion.goal.distance(goal) > 0.1);
        }
    }
    let (mut w, _, id) = fixture(r#"<Behavior duration="0">Wander</Behavior>"#);
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let mut rng = rand::rngs::StdRng::seed_from_u64(8);
    let before = rng.clone().next_u64();
    assert_eq!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    assert!(w.xml.motions.is_empty());
    assert_eq!(rng.clone().next_u64(), before);
    b.fields.insert(F::Duration, Value::Number(1.));
    let error = format!(
        "{:#}",
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap_err()
    );
    assert!(
        error.contains("Duration type") && error.contains("rules.xml:1"),
        "{error}"
    );
    assert_eq!(rng.clone().next_u64(), before);
    let (w, _, _) = fixture("<Behavior>Follow</Behavior>");
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    b.fields.remove(&F::Duration);
    assert!(
        format!("{:#}", native_validate(&b).unwrap_err())
            .contains("Follow missing compiled Duration")
    );
}

#[test]
fn xml_runtime_world_xml_move_backandforth_swirl_parameters_change_actual_native_steps() {
    for (body, dx, dy) in [
        (
            r#"<Behavior angle="0" distance="0.02" speed="1">Move</Behavior>"#,
            0.02,
            0.,
        ),
        (
            r#"<Behavior angle="90" distance="3" speed="1">Move</Behavior>"#,
            0.,
            0.05,
        ),
        (
            r#"<Behavior radius="0.02" speed="1">BackAndForth</Behavior>"#,
            0.02,
            0.,
        ),
        (
            r#"<Behavior angle="1.5707963267948966" radius="4" speed="1">BackAndForth</Behavior>"#,
            0.,
            0.05,
        ),
    ] {
        let (mut w, _, id) = fixture(body);
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        let mut rng = rand::rngs::StdRng::seed_from_u64(8);
        assert_eq!(
            w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
                .unwrap(),
            ActionOutcome::Applied
        );
        let pos = w.entities[&id].pos;
        assert!(
            (pos.x - 64.5 - dx).abs() < 0.00001 && (pos.y - 64.5 - dy).abs() < 0.00001,
            "{body}: {pos:?}"
        );
    }
    for (acceleration, turn) in [(1., 1.), (2., -1.)] {
        let (mut w, _, id) = fixture(&format!(
            r#"<Behavior speed="3" acceleration="{acceleration}" turnRate="{turn}" distance="4">Swirl</Behavior>"#
        ));
        let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        let mut rng = rand::rngs::StdRng::seed_from_u64(8);
        w.apply(Service::Movement, &b, motion_step(&w, id, &b, 0), &mut rng)
            .unwrap();
        assert_eq!(w.entities[&id].pos.distance(Pos { x: 64.5, y: 64.5 }), 0.);
        assert!(w.advance(&b, motion_step(&w, id, &b, 50)).unwrap());
        let moved = w.entities[&id].pos;
        assert!(
            (f64::from(moved.distance(Pos { x: 64.5, y: 64.5 })) - acceleration * 0.0025).abs()
                < 0.00001
        );
        assert_eq!(moved.y > 64.5, turn > 0.);
    }
}

#[test]
fn xml_runtime_world_back_and_forth_reverses_at_wall_without_recentering() {
    let (mut w, _, id) = fixture(r#"<Behavior radius="3" speed="10">BackAndForth</Behavior>"#);
    Arc::make_mut(&mut w.map).cells[64 * 128 + 66].ground = 255;
    frame(&mut w, 0);
    frame(&mut w, 50);
    assert!(w.entities[&id].pos.x > 64.5);
    for at in (100..=750).step_by(50) {
        frame(&mut w, at);
    }
    let pos = w.entities[&id].pos;
    assert!((61.49..64.0).contains(&pos.x) && pos.y == 64.5, "{pos:?}");
}

#[test]
fn xml_runtime_world_canonical_motion_field_types_refuse_before_native_rng_or_effects() {
    let (mut w, p, id) = fixture("<Behavior>RemainVisible</Behavior>");
    let mut rx = output(&mut w, p);
    let content = crate::world::test_content();
    let game = content.game.as_ref().unwrap();
    let mut seen = BTreeSet::new();
    let (mut count, mut stay_above) = (0, BTreeSet::new());
    for b in game
        .definitions
        .values()
        .flat_map(|d| &d.states)
        .flat_map(|s| &s.behaviors)
        .filter(|b| behavior::service(b.kind) == Some(Service::Movement))
    {
        if b.kind == K::StayAbove {
            count += 1;
            stay_above.insert(&b.source.object);
            let mut incomplete = b.clone();
            incomplete.fields.remove(&F::Speed);
            let error = format!("{:#}", native_validate(&incomplete).unwrap_err());
            assert!(
                error.contains("StayAbove missing compiled Speed")
                    && error.contains(&b.source.file)
                    && error.contains(&b.source.object),
                "{error}"
            );
            for speed in [0., 0.5] {
                let mut canonical = b.clone();
                canonical.fields.insert(F::Speed, Value::Number(speed));
                native_validate(&canonical).unwrap();
                canonical.fields.remove(&F::MinAltitude);
                assert!(
                    format!("{:#}", native_validate(&canonical).unwrap_err())
                        .contains("StayAbove missing compiled MinAltitude")
                );
            }
        }
        if !seen.insert(format!("{:?}", b.kind)) {
            continue;
        }
        let mut b = b.clone();
        b.fields.insert(F::Speed, Value::Bool(true));
        let mut rng = rand::rngs::StdRng::seed_from_u64(8);
        let before = rng.clone().next_u64();
        let error = format!(
            "{:#}",
            w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
                .unwrap_err()
        );
        assert!(
            error.contains("invalid compiled Speed type") && error.contains(&b.source.file),
            "{error}"
        );
        assert_eq!(rng.clone().next_u64(), before);
        assert!(w.xml.motions.is_empty());
        assert!(drain(&mut rx).is_empty());
    }
    assert_eq!((count, stay_above.len()), (133, 85));
    assert_eq!(seen.len(), 16);
    assert!(
        !seen.contains("Shadow"),
        "Shadow is separately covered by explicit generic XML fixtures"
    );
}
