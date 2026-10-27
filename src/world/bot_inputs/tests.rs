use super::*;

#[test]
fn native_cursor_tick_gate_rejects_duplicates_stale_and_ambiguous_wrap() {
    use crate::world::spectator::advance_cursor;
    let packet = |tick: u32| {
        let mut out = Writer::new();
        out.i32(1).i32(2).i32(tick as i32).f32(3.).f32(4.).u8(1);
        Packet {
            id: 250,
            body: out.0,
        }
    };
    let mut last = None;
    assert!(advance_cursor(&mut last, &packet(u32::MAX)).unwrap());
    assert!(!advance_cursor(&mut last, &packet(u32::MAX)).unwrap());
    assert!(advance_cursor(&mut last, &packet(0)).unwrap());
    assert!(!advance_cursor(&mut last, &packet(u32::MAX)).unwrap());
    assert!(!advance_cursor(&mut last, &packet(1 << 31)).unwrap());
    assert_eq!(last, Some(0));
    assert!(
        advance_cursor(
            &mut last,
            &Packet {
                id: 250,
                body: vec![]
            }
        )
        .is_err()
    );
}

#[test]
fn skill_tracking_is_finite_continuous_and_observations_are_not_queued_twice() {
    let start = Pos { x: 10., y: 10. };
    let target = Pos { x: 20., y: 10. };
    let mut novice = Inputs::default();
    let mut veteran = Inputs::default();
    novice.observe(start, None, Some(target), 7);
    veteran.observe(start, None, Some(target), 3);
    assert!(
        novice.fire && veteran.fire,
        "observation cadence already supplies reaction delay"
    );
    assert_eq!(novice.cursor.unwrap().x, start.x);
    novice.track();
    veteran.track();
    assert!(veteran.cursor.unwrap().x > novice.cursor.unwrap().x);
    assert!(veteran.cursor.unwrap().x < target.x);
    let prior = novice.cursor.unwrap();
    novice.observe(start, None, Some(Pos { x: 0., y: 30. }), 7);
    assert_eq!(novice.cursor.unwrap().x, prior.x);
    novice.track();
    assert!(novice.cursor.unwrap().distance(prior) <= 120. / 7. * 0.05 + 1e-5);
    for _ in 0..500 {
        novice.track();
    }
    let point = novice.cursor.unwrap();
    assert!(
        point.x.is_finite()
            && point.y.is_finite()
            && point.distance(Inputs::bounded(start, Pos { x: 0., y: 30. })) < 1e-4
    );
    novice.observe(start, None, None, 7);
    assert!(!novice.fire && novice.keys == [0, 0]);
}

#[test]
fn idle_cursor_is_available_and_aim_stays_inside_native_scene() {
    let start = Pos { x: 10., y: 10. };
    let mut input = Inputs::default();
    input.observe(start, None, None, 4);
    assert!(
        input.cursor.is_some(),
        "idle live controller needs a cursor"
    );
    input.observe(start, None, Some(Pos { x: 90., y: 50. }), 4);
    for _ in 0..200 {
        input.track();
    }
    let cursor = input.cursor.unwrap();
    assert!(start.distance(cursor) <= 5.00001);
    assert!(((cursor.y - start.y) / (cursor.x - start.x) - 0.5).abs() < 1e-5);
}

#[test]
fn lifecycle_clears_inputs_without_replaying_on_ready_or_replacement() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let mut w = World::new(content.clone(), map).unwrap();
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 2;
    let id = w.enter(ch.clone(), None).unwrap();
    w.players.get_mut(&id).unwrap().controller = Controller::Autonomous;
    let pos = w.players[&id].pos;
    let target = Pos {
        x: pos.x + 5.,
        y: pos.y + 5.,
    };
    for reason in 0..5 {
        w.bot_inputs((id, 2, 1), Some(target), Some(target), 4)
            .unwrap();
        let p = w.players.get_mut(&id).unwrap();
        match reason {
            0 => p.pending = true,
            1 => p.death_pending = true,
            2 => p.departing = true,
            3 => p.character.alive = false,
            _ => p.persistence_failure = Some("test quarantine".into()),
        }
        w.step_bot_inputs(id);
        let p = w.players.get_mut(&id).unwrap();
        let input = p.inputs.as_ref().unwrap();
        assert_eq!(input.keys, [0, 0]);
        assert_eq!(
            input.cursor.is_some(),
            reason == 0,
            "busy retains cursor; real lifecycle invalidates it"
        );
        assert!(!input.fire && p.bot_aim.is_none());
        p.pending = false;
        p.death_pending = false;
        p.departing = false;
        p.character.alive = true;
        p.persistence_failure = None;
        w.step_bot_inputs(id);
        assert!(!w.players[&id].inputs.as_ref().unwrap().fire);
    }
    w.players.remove(&id);
    ch.id = 3;
    let replacement = w.enter(ch, None).unwrap();
    assert!(w.players[&replacement].inputs.is_none());
    assert!(
        w.bot_inputs((id, 2, 1), Some(target), Some(target), 4)
            .is_err()
    );
    assert!(
        w.bot_inputs((replacement, 2, 1), Some(target), Some(target), 4)
            .is_err()
    );
}

#[test]
fn held_pointer_translates_with_the_virtual_view_without_involuntary_rotation() {
    let start = Pos { x: 10., y: 10. };
    let mut input = Inputs::default();
    input.observe(start, None, Some(Pos { x: 15., y: 12. }), 4);
    for _ in 0..100 {
        input.track();
    }
    let before = input.cursor.unwrap();
    let moved = Pos { x: 12., y: 7. };
    input.translate(moved);
    input.track();
    let after = input.cursor.unwrap();
    assert!((after.x - moved.x - (before.x - start.x)).abs() < 1e-5);
    assert!((after.y - moved.y - (before.y - start.y)).abs() < 1e-5);
}

#[test]
fn blocked_held_step_publishes_waypoint_feedback_and_releases_keys() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let mut w = World::new(content.clone(), map).unwrap();
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 2;
    let id = w.enter(ch, None).unwrap();
    w.players.get_mut(&id).unwrap().controller = Controller::Autonomous;
    let (start, target) = (1..w.map.height - 1)
        .flat_map(|y| (1..w.map.width - 1).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let start = Pos {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            let target = Pos {
                x: start.x + 1.,
                y: start.y,
            };
            (w.player_position_valid(None, start) && !w.player_position_valid(None, target))
                .then_some((start, target))
        })
        .expect("tutorial walkable/blocked boundary");
    w.players.get_mut(&id).unwrap().pos = start;
    w.bot_inputs((id, 2, 1), Some(target), None, 4).unwrap();
    for _ in 0..10 {
        w.now += 50;
        w.step_bot_inputs(id);
    }
    let input = w.players[&id].inputs.as_ref().unwrap();
    assert_eq!(input.keys, [0, 0]);
    let attempted = input.blocked.unwrap();
    assert!(attempted.x > start.x && attempted.x <= target.x);
    assert_eq!(attempted.y, start.y);
    assert!(w.player_position_valid(None, w.players[&id].pos));
}

#[test]
fn eight_keys_normalize_speed_and_release_at_waypoints() {
    let start = Pos { x: 10., y: 10. };
    for (x, y) in [
        (1., 0.),
        (1., 1.),
        (0., 1.),
        (-1., 1.),
        (-1., 0.),
        (-1., -1.),
        (0., -1.),
        (1., -1.),
    ] {
        let mut input = Inputs::default();
        let waypoint = Pos {
            x: 10. + x * 3.,
            y: 10. + y * 3.,
        };
        input.observe(start, Some(waypoint), None, 4);
        let step = input.movement(start, 0.3).unwrap();
        assert!((start.distance(step) - 0.3).abs() < 1e-5);
        assert!(input.movement(waypoint, 0.3).is_none());
        assert_eq!(input.keys, [0, 0]);
    }
}
