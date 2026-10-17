use super::*;

fn realm() -> World {
    let mut d = Directory::prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        12351,
    )
    .unwrap();
    d.worlds.remove(&-3).unwrap()
}
fn root(w: &World, kind: u16) -> i32 {
    w.entities
        .values()
        .find(|e| e.kind == kind && e.quest == Some(e.id))
        .unwrap()
        .id
}

#[test]
fn landmarks_have_selected_geometry_hazards_and_unchanged_population_reservations() {
    for seed in [1, 2, 42, 12351] {
        let d = Directory::prepared(
            std::path::Path::new("data"),
            tests::world().content,
            16,
            seed,
        )
        .unwrap();
        let w = &d.worlds[&-3];
        assert_eq!(w.hero_identities().len(), 13);
        assert_eq!(w.setpieces.len(), 13);
        assert!(w.population.iter().all(|s| w.outside_setpieces(s.pos)));
        assert_eq!(w.entities.values().filter(|e| e.kind == 0x501).count(), 6);
        assert!(w.map.cells.iter().any(|c| c.ground == 0xcb));
        assert!(w.map.cells.iter().any(|c| c.ground == 0xbc));
        for e in w
            .entities
            .values()
            .filter(|e| matches!(e.kind, 0x668 | 0x67d))
        {
            let c = w.map.cell(e.pos.x, e.pos.y).unwrap();
            assert_eq!(e.hp, w.content.objects[&e.kind].hp);
            if e.kind == 0x668 {
                assert_eq!(c.terrain, 1);
                assert_eq!(c.ground, 0xf4);
                assert_eq!(c.object, Some(0x153));
            } else {
                assert!([3, 4].contains(&c.terrain));
                assert_eq!(c.ground, 0x60);
            }
        }
        for kind in crate::quest::EVENTS {
            let id = root(w, kind);
            assert_eq!(w.entities[&id].hp, w.content.objects[&kind].hp);
        }
        let controller = root(w, 0xd5f);
        let towers = w.quest_members(controller, &[0xd5e]);
        assert_eq!(towers.len(), 5);
        for id in towers {
            let e = &w.entities[&id];
            assert_eq!(w.map.cell(e.pos.x, e.pos.y).unwrap().ground, 0xf2);
            assert!((14.0..16.1).contains(&e.pos.distance(w.entities[&controller].pos)));
        }
    }
}
#[test]
fn skull_and_cube_reproduction_respects_density_and_existing_encounter_caps() {
    let mut w = realm();
    for kind in [0xd56, 0xd59] {
        let id = root(&w, kind);
        for n in 0..100 {
            w.now = n * 500;
            let members: Vec<_> = w
                .entities
                .values()
                .filter(|e| e.quest == Some(id))
                .map(|e| e.id)
                .collect();
            for member in members {
                w.event_mechanics(member).unwrap();
            }
            assert!(w.entities.values().filter(|e| e.quest == Some(id)).count() <= 32);
        }
        let range = if kind == 0xd56 {
            0xd57..=0xd58
        } else {
            0xd5a..=0xd5c
        };
        for follower in range {
            let count = w.quest_members(id, &[follower]).len();
            assert!(count > 0);
            assert!(count <= if kind == 0xd56 { 20 } else { 10 });
        }
    }
}
#[test]
fn event_orbits_cannot_adopt_an_unrelated_encounter() {
    let mut w = realm();
    let shrine = root(&w, 0xd56);
    w.now = 500;
    w.event_mechanics(shrine).unwrap();
    let blue = w.quest_members(shrine, &[0xd58])[0];
    let pos = w.entities[&shrine].pos;
    let foreign = w
        .spawn_entity(
            0xd56,
            Pos {
                x: pos.x + 2.,
                y: pos.y,
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&foreign).unwrap().quest = Some(foreign);
    w.remove_entity(shrine);
    assert!(matches!(
        w.event_movement(blue, 0).0,
        crate::encounter::Motion::Wander
    ));
    assert!(
        w.orbit_anchor(blue, crate::encounter::Anchor::QuestMember(0xd56), pos)
            .is_err()
    );
}
#[test]
fn pentaract_revives_at_fifteen_seconds_then_completes_and_credits_only_once() {
    let mut w = realm();
    let controller = root(&w, 0xd5f);
    let pos = w.entities[&controller].pos;
    let player = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&player).unwrap().pos = pos;
    let towers = w.quest_members(controller, &[0xd5e]);
    let first = towers[0];
    let first_pos = w.entities[&first].pos;
    for id in &towers {
        w.event_mechanics(*id).unwrap();
    }
    assert_eq!(w.quest_members(controller, &[0xd5d]).len(), 25);
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.quest == Some(controller))
            .count(),
        31
    );
    w.damage_enemy(player, first, 6000, true, &[], None)
        .unwrap();
    assert!(w.critical_deaths.is_empty());
    assert_eq!(
        w.entities[&controller].damage[&w.players[&player].character.id],
        5000
    );
    w.players.get_mut(&player).unwrap().paused = true;
    w.now = 14950;
    w.event_step();
    assert_eq!(w.quest_members(controller, &[0xd60]).len(), 1);
    w.now = 15000;
    w.event_step();
    assert!(w.quest_members(controller, &[0xd60]).is_empty());
    let revived = w
        .quest_members(controller, &[0xd5e])
        .into_iter()
        .find(|id| w.entities[id].pos.distance(first_pos) < 0.1)
        .unwrap();
    assert!(w.entities[&revived].damage.is_empty());
    w.players.get_mut(&player).unwrap().paused = false;
    w.now = 16000;
    for id in w.quest_members(controller, &[0xd5e]) {
        w.damage_enemy(player, id, 6000, true, &[], None).unwrap();
    }
    w.event_step();
    assert!(!w.entities.contains_key(&controller));
    assert_eq!(w.critical_deaths, vec![controller]);
    assert!(w.entities.values().all(|e| e.quest != Some(controller)));
    assert_eq!(w.players[&player].character.quests, 1);
    let xp = w.players[&player].experience_queued;
    w.now = 60000;
    w.event_step();
    assert_eq!(w.critical_deaths, vec![controller]);
    assert_eq!(w.players[&player].experience_queued, xp);
}
#[test]
fn reproduce_uses_its_cooldown_wandering_protection_and_no_xp_without_suppressing_eye_xp() {
    let mut w = realm();
    let shrine = root(&w, 0xd56);
    w.now = 499;
    w.event_mechanics(shrine).unwrap();
    assert!(w.quest_members(shrine, &[0xd57, 0xd58]).is_empty());
    w.now = 500;
    w.event_mechanics(shrine).unwrap();
    let red = w.quest_members(shrine, &[0xd57])[0];
    let blue = w.quest_members(shrine, &[0xd58])[0];
    let orbit = w.entities[&blue].orbit.unwrap();
    assert!((5.0..25.0).contains(&orbit.radius));
    assert!(((5.55 * 0.9 + 0.74) * 1.5..(5.55 * 2.1 + 0.74) * 1.5).contains(&orbit.speed));
    assert!([-1., 1.].contains(&orbit.direction));
    let pos = w.entities[&red].pos;
    assert_eq!(pos.distance(w.entities[&shrine].pos), 0.);
    assert!(matches!(
        w.event_movement(red, 0).0,
        crate::encounter::Motion::Wander
    ));
    w.move_entity(
        red,
        Pos {
            x: pos.x + 20.,
            y: pos.y,
        },
        21.,
    )
    .unwrap();
    assert!(matches!(
        w.event_movement(red, 0).0,
        crate::encounter::Motion::Protect { .. }
    ));
    let p = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&p).unwrap().pos = w.entities[&red].pos;
    let xp = w.players[&p].experience_queued;
    w.defeat_entity(red).unwrap();
    assert_eq!(w.players[&p].experience_queued, xp);
    let eye = w.spawn_entity(0xd5d, w.players[&p].pos, vec![]).unwrap();
    w.entities.get_mut(&eye).unwrap().quest = Some(shrine);
    w.defeat_entity(eye).unwrap();
    assert!(w.players[&p].experience_queued > xp);
    let before = w.players[&p].experience_queued;
    let cube = root(&w, 0xd59);
    w.players.get_mut(&p).unwrap().pos = w.entities[&cube].pos;
    w.defeat_entity(cube).unwrap();
    assert!(w.players[&p].experience_queued > before);
}
#[test]
fn selected_event_shots_and_stasis_immunity_are_valid() {
    let mut w = realm();
    let player = w.enter(tests::ch(&w), None).unwrap();
    for kind in crate::quest::ROSTER
        .iter()
        .copied()
        .filter(|k| (0xd56..=0xd60).contains(k))
    {
        crate::encounter::validate(&w.content, kind).unwrap();
        crate::loot::validate(&w.content, kind).unwrap();
        let center = w.entities[&root(&w, 0xd5f)].pos;
        let id = w.spawn_entity(kind, center, vec![]).unwrap();
        w.entities.get_mut(&id).unwrap().quest = Some(id);
        let target = Pos {
            x: center.x + 4.,
            y: center.y + 4.,
        };
        w.players.get_mut(&player).unwrap().pos = target;
        for t in (0..=3000).step_by(50) {
            for (index, angle) in w.event_shots(id, target, t).unwrap() {
                assert!(angle.is_finite());
                assert!(w.content.objects[&kind].projectiles.contains_key(&index));
            }
        }
        if w.content.objects[&kind].stasis_immune {
            w.apply_condition(id, crate::condition::STASIS, 1000)
                .unwrap();
            assert_eq!(
                w.entities[&id].conditions(w.now) & crate::condition::bit(crate::condition::STASIS),
                0
            );
        }
        w.remove_entity(id);
    }
}
