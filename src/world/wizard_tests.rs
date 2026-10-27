use super::*;

#[test]
fn selected_wizard_novas_preserve_layout_costs_and_rejection_state() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let class = content.get(0x30e).unwrap();
    assert_eq!(class.slots[..4], [17, 11, 14, 9]);
    assert_eq!(content.classes[&0x30e].caps[7], 60);
    assert!(content.unlocked(0x30e, &BTreeMap::new()).unwrap());
    let map = Arc::new(Map {
        width: 32,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            32 * 32
        ],
        spawns: vec![(8.5, 8.5)],
    });
    for (kind, min, max, cost, lifetime, multi) in [
        (0xa2e, 25, 40, 30, 1000, false),
        (0xad4, 30, 70, 40, 1000, false),
        (0xa2f, 50, 90, 50, 1000, false),
        (0xad5, 65, 110, 60, 1000, false),
        (0xad6, 80, 130, 70, 1000, false),
        (0xa30, 90, 155, 80, 1000, false),
        (0xb24, 80, 165, 80, 1000, false),
        (0xc0c, 60, 90, 75, 700, true),
    ] {
        let mut world = World::new(content.clone(), map.clone()).unwrap();
        let character: Character = serde_json::from_value(serde_json::json!({
            "schema":1,"lifetime":crate::store::FameLifetime::complete(),
            "class":0x30e,"base":content.classes[&0x30e].base,
            "id":1,"account":1,"name":"Wizard","level":1,"exp":0,
            "fame":0,"hp":100,"mp":100,"revision":0,"alive":true,
            "items":[0xa97,kind,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
        }))
        .unwrap();
        let id = world.enter(character, None).unwrap();
        let slot = Slot {
            object: id,
            slot: 1,
            kind,
        };
        let item = content.get(kind as u16).unwrap();
        let target = Pos { x: 12.5, y: 8.5 };
        world.activate_ability(id, slot, item, target).unwrap();
        assert_eq!(world.players[&id].character.mp, 100 - cost);
        assert_eq!(world.players[&id].character.shots, 20);
        assert_eq!(world.bullets.len(), 20);
        for (i, bullet) in world.bullets.iter().enumerate() {
            assert_eq!(bullet.owner, id);
            assert!(bullet.friendly);
            assert_eq!((bullet.start.x, bullet.start.y), (target.x, target.y));
            assert!((bullet.angle - i as f32 * std::f32::consts::TAU / 20.).abs() < 0.00001);
            assert!((min..=max).contains(&bullet.damage));
            assert_eq!(bullet.spec.lifetime, lifetime as f32);
            assert_eq!(bullet.spec.multi_hit, multi);
            assert!(!bullet.spec.armor_piercing);
        }
        for (mana, quiet, ready, target) in [
            (100, false, u64::MAX, target),
            (cost - 1, false, 0, target),
            (100, true, 0, target),
            (100, false, 0, Pos { x: 31., y: 31. }),
        ] {
            let p = world.players.get_mut(&id).unwrap();
            p.character.mp = mana;
            p.ability_ready_ms = ready;
            p.lab_conditions = if quiet {
                crate::condition::bit(crate::condition::QUIET)
            } else {
                0
            };
            assert!(world.activate_ability(id, slot, item, target).is_err());
            assert_eq!(world.players[&id].character.mp, mana);
            assert_eq!(world.players[&id].character.shots, 20);
            assert_eq!(world.bullets.len(), 20);
        }
        world.players.get_mut(&id).unwrap().lab_conditions = 0;
        world.bullets.truncate(1);
        let damage = world.bullets[0].damage;
        let targets: Vec<_> = [13.1, 14.1]
            .into_iter()
            .map(|x| {
                world
                    .spawn_entity(0x181f, Pos { x, y: 8.5 }, vec![])
                    .unwrap()
            })
            .collect();
        for _ in 0..4 {
            world.step();
        }
        assert_eq!(world.entities[&targets[0]].hp, 1000 - damage);
        assert_eq!(
            world.entities[&targets[1]].hp,
            1000 - if multi { damage } else { 0 }
        );
    }
}
