use super::*;

#[test]
fn off_axis_known_corner_admits_actual_safe_keys_and_makes_progress() {
    use crate::{
        content::{Cell, Map},
        world::{Controller, World},
    };
    let (content, mut view) = super::super::quest_tests::fixture();
    let mut cells = vec![
        Cell {
            ground: 1,
            object: None,
            config: String::new(),
            terrain: 0,
            region: 0
        };
        128 * 128
    ];
    cells[10 * 128 + 12].ground = 0xc9;
    let geometry = crate::navigation::Geometry::new(
        (128, 128),
        cells.iter().map(|c| c.ground).collect(),
        &content.grounds,
    );
    view.tiles = crate::navigation::Tiles::new(
        geometry,
        (0..128).flat_map(|x| (0..128).map(move |y| (x, y))),
    )
    .into();
    view.position = Pos { x: 10.5, y: 10.5 };
    let grid = Grid::terrain(&view);
    let target = Pos { x: 14.5, y: 12. };
    let waypoint = grid.next(view.position, target).unwrap();
    assert!(grid.segment(view.position, waypoint));
    let content = Arc::new(content);
    let mut w = World::new(
        content.clone(),
        Arc::new(Map {
            width: 128,
            height: 128,
            cells,
            spawns: vec![(10.5, 10.5)],
        }),
    )
    .unwrap();
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    ch.base[4] = 25;
    let id = w.enter(ch, None).unwrap();
    w.players.get_mut(&id).unwrap().controller = Controller::Autonomous;
    let movement = held_movement(
        &grid,
        view.position,
        waypoint,
        combat::speed(&view) * 7. * 0.05,
    );
    execute_inputs(&mut w, (id, 1, 1), movement, None, None, 7).unwrap();
    for _ in 0..7 {
        w.step();
    }
    let p = &w.players[&id];
    assert!(
        p.inputs.as_ref().unwrap().blocked.is_none(),
        "valid off-axis route must not become a blocked octant ray"
    );
    assert!(
        p.pos.y > 10.6 && p.pos.distance(target) < view.position.distance(target),
        "safe corner progress: {:?}",
        p.pos
    );
}

#[test]
fn tight_known_passage_admits_short_safe_hold_instead_of_waiting_forever() {
    let (_, view) = super::super::quest_tests::fixture();
    let mut grid = Grid::terrain(&view);
    let corridor = [
        (10, 10),
        (11, 10),
        (11, 11),
        (12, 11),
        (13, 11),
        (13, 12),
        (14, 11),
        (14, 12),
    ];
    for x in 6..19 {
        for y in 6..18 {
            if !corridor.contains(&(x, y)) {
                grid.walkable.remove(&(x, y));
            }
        }
    }
    let start = Pos { x: 10.5, y: 10.5 };
    let goal = Pos { x: 14.5, y: 12. };
    assert!(grid.segment(start, goal));
    for index in 0..8 {
        let heading = index as f32 * std::f32::consts::FRAC_PI_4;
        let full = Pos {
            x: start.x + heading.cos() * 4.,
            y: start.y + heading.sin() * 4.,
        };
        assert!(!grid.segment(start, full));
    }
    let step = held_movement(&grid, start, goal, 4.).expect("short known key path fits the corner");
    assert!(grid.segment(start, step) && start.distance(step) > 0.1 && start.distance(step) < 4.);
    assert!(step.distance(goal) < start.distance(goal));
}

#[test]
fn mouse_aimed_spell_uses_actual_cursor_and_tome_does_not_move_it() {
    use crate::{
        content::Map,
        world::{Controller, World},
    };
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let mut w = World::new(content.clone(), map).unwrap();
    for (n, class) in [crate::content::WIZARD, crate::content::PRIEST]
        .into_iter()
        .enumerate()
    {
        let mut ch = Character::fresh(content.get(class).unwrap()).unwrap();
        ch.id = n as i32 + 1;
        ch.account = ch.id;
        ch.mp = 1000;
        let character = ch.id;
        let id = w.enter(ch, None).unwrap();
        w.players.get_mut(&id).unwrap().controller = Controller::Autonomous;
        let pos = w.players[&id].pos;
        let desired = Pos {
            x: pos.x + 4.,
            y: pos.y,
        };
        let shots = w.bullets.len();
        execute_inputs(
            &mut w,
            (id, character, character),
            None,
            None,
            Some(Action::Use {
                slot: 1,
                x: desired.x,
                y: desired.y,
            }),
            4,
        )
        .unwrap();
        let input = w.players[&id].inputs.as_ref().unwrap();
        if class == crate::content::WIZARD {
            assert!(!input.fire);
            let cursor = input.cursor.unwrap();
            assert!(cursor.distance(desired) > 3.);
            assert_eq!(w.bullets.len() - shots, 20);
            assert!(
                w.bullets[shots..]
                    .iter()
                    .all(|b| b.start.distance(cursor) < 1e-5)
            );
        } else {
            assert!(
                input.cursor.is_none() && !input.fire,
                "self-centered tome cannot drag pointer"
            );
        }
    }
}

#[tokio::test]
async fn virtual_inputs_execute_held_diagonal_and_actual_cursor_between_decisions() {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, _output, _brain) =
        tests::actor(&app, "virtual-inputs", "Trader", [-1; 8]).await;
    let view = app.world.player_view(id).await.unwrap();
    let session = (id, view.session.account, view.session.character);
    let start = view.position;
    let step_speed = (4. + 5.6 * view.character.stats[4] as f32 / 75.) * 0.05;
    let target = Pos {
        x: start.x + 7.,
        y: start.y + 3.,
    };
    server::inventory(&app, id, move |w| {
        execute_inputs(
            w,
            session,
            Some(Pos {
                x: start.x + 5.,
                y: start.y + 5.,
            }),
            Some(target),
            None,
            6,
        )
    })
    .await
    .unwrap()
    .unwrap();
    app.world
        .for_player(id)
        .call(move |w| {
            let mut distance = 0.;
            let mut expected_distance = 0.;
            let mut shots = 0;
            for _ in 0..12 {
                let before = w.players[&id].pos;
                let bullet = w.players[&id].next_bullet;
                let ground = w.map.cell(before.x, before.y).unwrap().ground;
                expected_distance += step_speed * w.content.grounds[&ground];
                w.step();
                let p = &w.players[&id];
                let dx = p.pos.x - before.x;
                let dy = p.pos.y - before.y;
                assert!((dx - dy).abs() < 1e-4, "normalized held diagonal");
                distance += before.distance(p.pos);
                let input = p.inputs.as_ref().unwrap();
                assert_eq!(input.keys, [1, 1]);
                if p.next_bullet != bullet {
                    shots += 1;
                    let shot = w
                        .bullets
                        .iter()
                        .find(|b| b.owner == id && b.id == bullet)
                        .unwrap();
                    let cursor = input.cursor.unwrap();
                    if shots == 1 {
                        assert!(
                            cursor.distance(target) > 1.,
                            "first shot must not snap to observed lead"
                        );
                    }
                    assert!(
                        (shot.angle - angle(p.pos, cursor)).abs() < 1e-5,
                        "weapon uses actual cursor, not latest target"
                    );
                }
            }
            assert!(
                (distance - expected_distance).abs() < 1e-3,
                "ordinary speed, not doubled diagonal: {distance}"
            );
            assert!((2..12).contains(&shots), "held ordinary cooldown: {shots}");
            Ok(())
        })
        .await
        .unwrap();
    let state = app.world.player_view(id).await.unwrap();
    let cursor = state.inputs.as_ref().unwrap().cursor.unwrap();
    let new_target = Pos {
        x: start.x - 4.,
        y: start.y - 6.,
    };
    server::inventory(&app, id, move |w| {
        execute_inputs(w, session, None, Some(new_target), None, 6)
    })
    .await
    .unwrap()
    .unwrap();
    let changed = app.world.player_view(id).await.unwrap();
    assert!(
        changed.inputs.unwrap().cursor.unwrap().distance(cursor) < 1e-6,
        "target change cannot snap actual cursor"
    );
    app.world
        .for_player(id)
        .call(move |w| {
            w.step();
            Ok(())
        })
        .await
        .unwrap();
    let changed = app.world.player_view(id).await.unwrap();
    let input = changed.inputs.unwrap();
    assert_eq!(input.keys, [0, 0]);
    assert!(input.cursor.unwrap().distance(cursor) <= 1.001);
    let mut last_tick = None;
    let packet = app
        .world
        .spectator_cursor(-2, id, session.1, session.2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((packet.id, packet.body.len()), (250, 21));
    assert!(crate::world::spectator::advance_cursor(&mut last_tick, &packet).unwrap());
    let duplicate = app
        .world
        .spectator_cursor(-2, id, session.1, session.2)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !crate::world::spectator::advance_cursor(&mut last_tick, &duplicate).unwrap(),
        "paused world poll must not refresh native cursor"
    );
    app.world
        .for_player(id)
        .call(move |w| {
            w.step();
            Ok(())
        })
        .await
        .unwrap();
    let advanced = app
        .world
        .spectator_cursor(-2, id, session.1, session.2)
        .await
        .unwrap()
        .unwrap();
    assert!(crate::world::spectator::advance_cursor(&mut last_tick, &advanced).unwrap());
    let mut r = Reader::new(&packet.body);
    assert_eq!(r.i32().unwrap(), id);
    assert_eq!(r.i32().unwrap(), session.2);
    assert_eq!(r.i32().unwrap() as u32, changed.tick as u32);
    assert_eq!(r.f32().unwrap(), input.cursor.unwrap().x);
    assert_eq!(r.f32().unwrap(), input.cursor.unwrap().y);
    assert_eq!(r.u8().unwrap(), 1);
    r.done().unwrap();
    for (world, player, account, character) in [
        (-3, id, session.1, session.2),
        (-2, id, session.1 + 1, session.2),
        (-2, id, session.1, session.2 + 1),
        (-2, i32::MAX, session.1, session.2),
    ] {
        assert!(
            app.world
                .spectator_cursor(world, player, account, character)
                .await
                .unwrap()
                .is_none()
        );
    }
    app.world
        .for_player(id)
        .call(move |w| {
            assert!(
                execute_inputs(w, (id, session.1, session.2 + 1), None, None, None, 6).is_err()
            );
            assert!(
                execute_inputs(w, session, None, Some(Pos { x: f32::NAN, y: 1. }), None, 6)
                    .is_err()
            );
            w.players.get_mut(&id).unwrap().pending = true;
            w.step();
            let p = &w.players[&id];
            let state = p.inputs.as_ref().unwrap();
            assert!(!state.fire && state.cursor.is_none() && state.keys == [0, 0]);
            w.players.get_mut(&id).unwrap().pending = false;
            w.step();
            assert!(
                !w.players[&id].inputs.as_ref().unwrap().fire,
                "busy completion cannot revive held work"
            );
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        app.world
            .spectator_cursor(-2, id, session.1, session.2)
            .await
            .unwrap()
            .is_none()
    );
    stop_inputs(&app, id).await.unwrap();
    assert!(app.world.player_view(id).await.unwrap().inputs.is_none());
    lifecycle::finalize(&app, id, CloseReason::Controller)
        .unwrap()
        .wait()
        .await
        .unwrap();
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
