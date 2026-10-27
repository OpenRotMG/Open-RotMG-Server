use super::*;

fn scene(kind: i32) -> (World, i32, Slot) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let mut map = Map {
        width: 96,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            96 * 32
        ],
        spawns: vec![(4.5, 8.5)],
    };
    for y in 0..32 {
        map.cells[y * 96 + 6].ground = 255;
    }
    let mut world = World::new(content, Arc::new(map)).unwrap();
    let character: Character = serde_json::from_value(serde_json::json!({
        "schema":1,"lifetime":crate::store::FameLifetime::complete(),
        "class":0x325,"base":[670,385,60,25,60,60,75,60],
        "id":1,"account":1,"name":"Sorcerer","level":20,"exp":0,
        "fame":0,"hp":670,"mp":385,"revision":0,"alive":true,
        "items":[-1,kind,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
    }))
    .unwrap();
    let id = world.enter(character, None).unwrap();
    (
        world,
        id,
        Slot {
            object: id,
            slot: 1,
            kind,
        },
    )
}

#[test]
fn sorcerer_selected_scepters_chain_once_without_falloff_through_walls() {
    for (kind, damage, maximum, cost) in [
        (0xae5, 50, 3, 60),
        (0xb2e, 80, 4, 65),
        (0xb2f, 110, 5, 70),
        (0xb30, 140, 6, 75),
        (0xb31, 170, 7, 80),
        (0xb32, 200, 8, 85),
        (0xb33, 200, 8, 85),
        (0xc30, 180, 7, 65),
    ] {
        let (mut w, owner, slot) = scene(kind);
        assert_eq!(w.content.get(0x325).unwrap().slots[..4], [8, 23, 14, 9]);
        assert_eq!(w.content.get(kind as u16).unwrap().cooldown_ms, 500);
        let targets: Vec<_> = (0..=maximum)
            .map(|i| {
                w.spawn_entity(
                    0x181f,
                    Pos {
                        x: 8.5 + i as f32 * 8.,
                        y: 8.5,
                    },
                    vec![],
                )
                .unwrap()
            })
            .collect();
        assert!(!w.walkable(Pos { x: 6.5, y: 8.5 }));
        // Falloff is a later rework, not selected-era behavior: https://wikiwiki.jp/rmd/version/build_X15.0.0
        w.use_item(owner, slot, Pos { x: 12.5, y: 8.5 }).unwrap();
        assert_eq!(w.players[&owner].character.mp, 385 - cost);
        for (i, target) in targets.iter().enumerate() {
            let e = &w.entities[target];
            assert_eq!(e.hp, 1000 - if i < maximum { damage } else { 0 });
            assert_eq!(
                e.damage.get(&1).copied().unwrap_or(0),
                if i < maximum { damage } else { 0 }
            );
            assert_eq!(
                e.statuses.get(&crate::condition::SLOWED).copied(),
                (kind == 0xc30 && i < maximum).then_some((w.now, w.now + 3000))
            );
        }
        let hp: Vec<_> = targets.iter().map(|id| w.entities[id].hp).collect();
        for (mana, quiet, ready, pos) in [
            (385, false, u64::MAX, Pos { x: 12.5, y: 8.5 }),
            (cost - 1, false, 0, Pos { x: 12.5, y: 8.5 }),
            (385, true, 0, Pos { x: 12.5, y: 8.5 }),
            (385, false, 0, Pos { x: 40.5, y: 8.5 }),
        ] {
            let p = w.players.get_mut(&owner).unwrap();
            p.character.mp = mana;
            p.ability_ready_ms = ready;
            p.lab_conditions = if quiet {
                crate::condition::bit(crate::condition::QUIET)
            } else {
                0
            };
            assert!(w.use_item(owner, slot, pos).is_err());
            assert_eq!(w.players[&owner].character.mp, mana);
            assert_eq!(
                targets
                    .iter()
                    .map(|id| w.entities[id].hp)
                    .collect::<Vec<_>>(),
                hp
            );
        }
    }
}

#[test]
fn sorcerer_targeting_reconstruction_uses_nearest_cone_then_nearest_jump() {
    let (mut w, owner, _) = scene(0xae5);
    let targets: Vec<_> = [
        (7.5, 9.5),
        (10.5, 8.5),
        (10.5, 12.5),
        (2.5, 8.5),
        (4.5, 12.5),
    ]
    .into_iter()
    .map(|(x, y)| w.spawn_entity(0x181f, Pos { x, y }, vec![]).unwrap())
    .collect();
    w.ability_lightning(owner, Pos { x: 14.5, y: 8.5 }, 50, 2, None, 0xffff0088)
        .unwrap();
    assert_eq!(
        targets
            .iter()
            .map(|id| w.entities[id].hp)
            .collect::<Vec<_>>(),
        [950, 950, 1000, 1000, 1000]
    );
    for target in targets {
        w.remove_entity(target);
    }
    let boundary = w
        .spawn_entity(0x181f, Pos { x: 18.5, y: 8.5 }, vec![])
        .unwrap();
    let outside = w
        .spawn_entity(0x181f, Pos { x: 28.75, y: 8.5 }, vec![])
        .unwrap();
    w.ability_lightning(owner, Pos { x: 14.5, y: 8.5 }, 50, 3, None, 0xffff0088)
        .unwrap();
    assert_eq!(w.entities[&boundary].hp, 950);
    assert_eq!(w.entities[&outside].hp, 1000);
    w.remove_entity(boundary);
    w.remove_entity(outside);
    let behind = w
        .spawn_entity(0x181f, Pos { x: 2.5, y: 8.5 }, vec![])
        .unwrap();
    let outside_cone = w
        .spawn_entity(0x181f, Pos { x: 5.5, y: 10.5 }, vec![])
        .unwrap();
    let (mana, ready) = (
        w.players[&owner].character.mp,
        w.players[&owner].ability_ready_ms,
    );
    w.use_item(
        owner,
        Slot {
            object: owner,
            slot: 1,
            kind: 0xae5,
        },
        Pos { x: 14.5, y: 8.5 },
    )
    .unwrap();
    assert_eq!(w.players[&owner].character.mp, mana - 60);
    assert!(w.players[&owner].ability_ready_ms > ready);
    assert_eq!(
        [w.entities[&behind].hp, w.entities[&outside_cone].hp],
        [1000, 1000]
    );
}
