use super::tests::ch;
use super::*;
use std::{path::Path, time::Duration};

#[test]
fn solo_reentries_sample_spawn_points_and_preserve_prepared_positions() {
    let mut w = open_world();
    Arc::make_mut(&mut w.map).spawns = vec![(1.5, 2.5), (3.5, 2.5)];
    w.rng = rand::rngs::StdRng::seed_from_u64(76);
    let fixture = w.enter(ch(&w), None).unwrap();
    let presentation = w.players[&fixture].presentation.clone();
    w.leave(fixture).unwrap();
    let mut arrivals = BTreeSet::new();
    for _ in 0..32 {
        let id = w
            .enter_at(ch(&w), None, None, presentation.clone())
            .unwrap();
        let pos = w.players[&id].pos;
        assert!(w.map.spawns.contains(&(pos.x, pos.y)));
        arrivals.insert((pos.x as i32, pos.y as i32));
        w.leave(id).unwrap();
    }
    assert_eq!(arrivals.len(), 2);
    let pos = Pos { x: 2.5, y: 2.5 };
    let id = w
        .enter_at(ch(&w), None, Some(pos), presentation.clone())
        .unwrap();
    assert_eq!(w.players[&id].pos.x, pos.x);
    assert_eq!(w.players[&id].pos.y, pos.y);
    w.leave(id).unwrap();
    Arc::make_mut(&mut w.map).spawns.clear();
    assert!(
        w.enter_at(ch(&w), None, None, presentation)
            .unwrap_err()
            .to_string()
            .contains("no spawn cells")
    );
}

#[test]
fn snake_pit_native_move_rejects_full_occupy_floor_margin() {
    let content = Arc::new(Content::load(Path::new("data")).unwrap());
    let map = Arc::new(crate::dungeon::snake_pit(&content, 1).unwrap());
    let mut w = World::new(content, map).unwrap();
    let id = w.enter(ch(&w), None).unwrap();
    let gap = Pos { x: 10.25, y: 75.5 };
    assert_eq!(w.map.cell(9., 75.).unwrap().object, Some(0x1ed));
    assert!(w.walkable(gap));
    let received = w.attack_epoch + Duration::from_millis(100);
    assert!(w.native_move_received(id, gap, (100, received)).is_err());
    assert_eq!(w.players[&id].pos.x, 15.5);
}

fn open_world() -> World {
    let content = Arc::new(Content::load(Path::new("data")).unwrap());
    let map = Map {
        width: 5,
        height: 5,
        cells: vec![
            crate::content::Cell {
                ground: 0x16,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            25
        ],
        spawns: vec![(2.5, 2.5)],
    };
    World::new(content, Arc::new(map)).unwrap()
}

#[test]
fn full_occupy_sides_diagonals_and_exact_half_cells() {
    let mut w = open_world();
    assert!(w.content.objects[&0x1ed].full_occupy);
    let wall = w
        .spawn_entity(0x1ed, Pos { x: 1.5, y: 1.5 }, vec![])
        .unwrap();
    for (dx, dy) in [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ] {
        w.entities.get_mut(&wall).unwrap().pos = Pos {
            x: 2.5 + dx as f32,
            y: 2.5 + dy as f32,
        };
        let near = Pos {
            x: 2.5 + dx as f32 * 0.25,
            y: 2.5 + dy as f32 * 0.25,
        };
        assert!(w.walkable(near));
        assert!(!w.player_position_valid(None, near), "neighbor {dx},{dy}");
        assert!(w.player_position_valid(None, Pos { x: 2.5, y: 2.5 }));
        if dx != 0 && dy != 0 {
            assert!(w.player_position_valid(None, Pos { x: 2.5, y: near.y }));
            assert!(w.player_position_valid(None, Pos { x: near.x, y: 2.5 }));
        }
        assert!(w.player_position_valid(
            None,
            Pos {
                x: 5. - near.x,
                y: 5. - near.y
            }
        ));
    }
}

#[test]
fn void_and_map_borders_are_full_but_nowalk_neighbors_are_not() {
    let mut w = open_world();
    for pos in [
        Pos { x: 0.25, y: 2.5 },
        Pos { x: 4.75, y: 2.5 },
        Pos { x: 2.5, y: 0.25 },
        Pos { x: 2.5, y: 4.75 },
        Pos { x: 0.25, y: 0.25 },
    ] {
        assert!(!w.player_position_valid(None, pos));
    }
    assert!(w.player_position_valid(None, Pos { x: 0.5, y: 0.5 }));
    let near = Pos { x: 2.25, y: 2.5 };
    Arc::make_mut(&mut w.map).cells[11].ground = 255;
    assert!(!w.player_position_valid(None, near));
    let no_walk = *w
        .content
        .grounds
        .iter()
        .find(|(id, speed)| **id != 255 && **speed <= 0.)
        .unwrap()
        .0;
    Arc::make_mut(&mut w.map).cells[11].ground = no_walk;
    assert!(w.player_position_valid(None, near));
    assert!(!w.player_position_valid(None, Pos { x: 1.5, y: 2.5 }));
}

#[test]
fn quarter_cell_bypass_and_half_boundary_departure_match_modify_step() {
    let mut w = open_world();
    w.spawn_entity(0x1ed, Pos { x: 1.5, y: 2.5 }, vec![])
        .unwrap();
    let inside = Pos { x: 2.25, y: 2.5 };
    let same_quarter = Pos { x: 2.3, y: 2.5 };
    assert!(!w.player_position_valid(Some(inside), same_quarter));
    assert!(w.player_step_valid(inside, same_quarter));
    assert!(!w.player_step_valid(Pos { x: 2.5, y: 2.5 }, inside));
    assert!(w.player_step_valid(inside, Pos { x: 2.5, y: 2.5 }));
    assert!(w.player_step_valid(Pos { x: 2.5, y: 2.5 }, Pos { x: 2.6, y: 2.5 }));
    // Departure along the other axis still forces validation of the invalid left margin.
    assert!(!w.player_step_valid(inside, Pos { x: 2.3, y: 2.6 }));
    w.spawn_entity(0x1ed, Pos { x: 2.5, y: 2.5 }, vec![])
        .unwrap();
    Arc::make_mut(&mut w.map).cells[12].ground = 255;
    assert!(w.player_step_valid(Pos { x: 2.5, y: 2.5 }, Pos { x: 2.6, y: 2.5 }));
    assert!(!w.player_position_valid(None, Pos { x: 2.6, y: 2.5 }));
}

#[test]
fn ordinary_occupy_has_no_margin_and_blocked_start_can_escape() {
    let mut w = open_world();
    let id = w.enter(ch(&w), None).unwrap();
    let kind = w
        .content
        .objects
        .values()
        .find(|o| o.blocks && !o.full_occupy)
        .unwrap()
        .kind;
    let obstacle = w
        .spawn_entity(kind, Pos { x: 2.5, y: 2.5 }, vec![])
        .unwrap();
    assert!(!w.player_position_valid(None, w.players[&id].pos));
    for (time, pos) in [(100, Pos { x: 2.6, y: 2.5 }), (200, Pos { x: 3.1, y: 2.5 })] {
        w.native_move_received(
            id,
            pos,
            (time, w.attack_epoch + Duration::from_millis(time as u64)),
        )
        .unwrap();
    }
    assert!(w.player_position_valid(None, Pos { x: 3.1, y: 2.5 }));
    assert!(!w.player_step_valid(w.players[&id].pos, Pos { x: 2.9, y: 2.5 }));
    w.remove_entity(obstacle);
    w.players.get_mut(&id).unwrap().pos = Pos { x: 2.5, y: 2.5 };
    w.spawn_entity(kind, Pos { x: 2.5, y: 2.5 }, vec![])
        .unwrap();
    w.now = 300;
    w.move_to(id, Pos { x: 2.6, y: 2.5 }, 300).unwrap();
    w.now = 400;
    w.move_to(id, Pos { x: 2.6, y: 2.5 }, 400).unwrap();
}

#[test]
fn native_predicted_shot_origin_rejects_full_occupy_margin() {
    let mut w = open_world();
    let id = w.enter(ch(&w), None).unwrap();
    w.spawn_entity(0x1ed, Pos { x: 1.5, y: 2.5 }, vec![])
        .unwrap();
    let err = w
        .native_shoot_received(
            id,
            (100, w.attack_epoch + Duration::from_millis(100)),
            1,
            0xa97,
            Pos { x: 2.55, y: 2.5 },
            0.,
        )
        .unwrap_err();
    assert!(err.to_string().contains("invalid shot origin"));
    assert!(w.bullets.is_empty());
    w.players.get_mut(&id).unwrap().controller = Controller::Qa;
    assert!(w.action(id, Action::Move { x: 2.25, y: 2.5 }).is_err());
    w.now = 100;
    assert!(w.move_to(id, Pos { x: 2.25, y: 2.5 }, 100).is_err());
    assert_eq!(w.players[&id].last_move, 0);
    w.move_to(id, Pos { x: 2.5, y: 2.5 }, 100).unwrap();
}

#[test]
fn teleport_is_strict_even_with_same_square_or_quarter() {
    let mut d = super::tests::directory();
    let mut w = open_world();
    w.name = "Realm".into();
    let a = w.enter(ch(&w), None).unwrap();
    let mut c = ch(&w);
    c.id += 1;
    c.account += 1;
    let b = w.enter(c, None).unwrap();
    w.players.get_mut(&a).unwrap().known.insert(b);
    let kind = w
        .content
        .objects
        .values()
        .find(|o| o.blocks && !o.full_occupy)
        .unwrap()
        .kind;
    let obstacle = w
        .spawn_entity(kind, Pos { x: 2.5, y: 2.5 }, vec![])
        .unwrap();
    d.worlds.insert(-2, w);
    assert!(
        d.teleport(a, b, Instant::now())
            .unwrap_err()
            .to_string()
            .contains("position is blocked")
    );
    let w = d.worlds.get_mut(&-2).unwrap();
    w.remove_entity(obstacle);
    w.spawn_entity(0x1ed, Pos { x: 1.5, y: 2.5 }, vec![])
        .unwrap();
    for id in [a, b] {
        w.players.get_mut(&id).unwrap().pos.x = 2.25;
    }
    assert!(
        d.teleport(a, b, Instant::now())
            .unwrap_err()
            .to_string()
            .contains("position is blocked")
    );
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&b)
        .unwrap()
        .pos
        .x = 2.5;
    d.teleport(a, b, Instant::now()).unwrap();
}
