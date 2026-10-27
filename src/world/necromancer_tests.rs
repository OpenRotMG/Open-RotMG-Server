use super::*;

#[tokio::test]
async fn necromancer_requires_both_level_twenty_records() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let class = content.get(0x321).unwrap();
    assert_eq!(class.slots[..4], [17, 19, 14, 9]);
    assert_eq!(
        content.classes[&0x321].caps,
        [670, 385, 60, 25, 50, 60, 30, 75]
    );
    for (wizard, priest) in [(19, 20), (20, 19), (20, 20)] {
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let guid = format!("necromancer-{wizard}-{priest}");
        let account = store.list(guid.clone()).await.unwrap();
        assert!(store.create(account.id, class).await.is_err());
        for (kind, level) in [(0x30e, wizard), (0x310, priest)] {
            let mut ch = store
                .create(account.id, content.get(kind).unwrap())
                .await
                .unwrap();
            ch.gain_experience(
                &content,
                crate::store::level_start(level).unwrap(),
                &mut rand::thread_rng(),
            )
            .unwrap();
            store.progress(ch.clone()).await.unwrap();
            ch.hp = 0;
            ch.alive = false;
            store
                .death(ch, "prepared unlock test".into())
                .await
                .unwrap();
        }
        let listed = store.list(guid).await.unwrap();
        assert!(listed.characters.is_empty());
        assert_eq!(listed.best_levels[&0x30e], wizard);
        assert_eq!(listed.best_levels[&0x310], priest);
        let created = store.create(account.id, class).await;
        if wizard == 20 && priest == 20 {
            let ch = created.unwrap();
            assert_eq!(ch.level, 1);
            assert_eq!(ch.items, class.equipment);
            assert_eq!(ch.base, Character::fresh(class).unwrap().base);
            assert!(ch.alive);
        } else {
            assert!(created.is_err());
        }
    }
}

#[test]
fn necromancer_skulls_drain_actual_damage_and_heal_around_caster() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map {
        width: 32,
        height: 32,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            1024
        ],
        spawns: vec![(8.5, 8.5)],
    });
    for (kind, damage, cost, radius) in [
        (0xaaa, 25, 40, 2.5),
        (0xaab, 45, 60, 2.75),
        (0xaac, 70, 80, 3.),
        (0xaad, 100, 100, 3.25),
        (0xaae, 125, 120, 3.5),
        (0xaaf, 150, 140, 3.75),
        (0xb2b, 150, 140, 3.75),
        (0xc16, 80, 140, 4.25),
        (0xc0e, 200, 135, 2.),
    ] {
        let mut w = World::new(content.clone(), map.clone()).unwrap();
        w.clock.set_paused(true);
        let players: [i32; 4] = std::array::from_fn(|i| {
            let mut ch = Character::fresh(content.get(0x321).unwrap()).unwrap();
            ch.account = i as i32 + 1;
            ch.id = 1;
            ch.base[0] = 670;
            ch.base[1] = 385;
            ch.hp = 100;
            ch.mp = 350;
            ch.items[1] = kind;
            w.enter(ch, None).unwrap()
        });
        let [caster, ally, cursor_ally, sick] = players;
        w.players.get_mut(&ally).unwrap().pos.x += radius - 0.1;
        let target = Pos { x: 18.5, y: 8.5 };
        w.players.get_mut(&cursor_ally).unwrap().pos = target;
        w.apply_condition(sick, crate::condition::SICK, 10000)
            .unwrap();
        let enemy = w.spawn_entity(0x655, target, vec![]).unwrap();
        let weak = w.spawn_entity(0x655, target, vec![]).unwrap();
        w.entities.get_mut(&weak).unwrap().hp = 1;
        let immune: Vec<_> = [
            crate::condition::INVINCIBLE,
            crate::condition::INVULNERABLE,
            crate::condition::STASIS,
        ]
        .into_iter()
        .map(|effect| {
            let id = w.spawn_entity(0x655, target, vec![]).unwrap();
            w.apply_condition(id, effect, 10000).unwrap();
            id
        })
        .collect();
        assert_eq!(content.objects[&0x655].defense, 18);
        let dealt = (damage - 18).max(damage * 15 / 100);
        let item = content.get(kind as u16).unwrap();
        let slot = Slot {
            object: caster,
            slot: 1,
            kind,
        };
        w.activate_ability(caster, slot, item, target).unwrap();
        assert_eq!(w.entities[&enemy].hp, 1000 - dealt, "{kind:#x}");
        assert!(!w.entities.contains_key(&weak));
        for id in immune {
            assert_eq!(w.entities[&id].hp, 1000);
        }
        for id in [caster, ally] {
            assert_eq!(w.players[&id].character.hp, 101 + dealt, "{kind:#x}");
        }
        for id in [cursor_ally, sick] {
            assert_eq!(w.players[&id].character.hp, 100);
        }
        assert_eq!(w.players[&caster].character.mp, 350 - cost);
        assert!(w.activate_ability(caster, slot, item, target).is_err());
        assert_eq!(w.players[&caster].character.mp, 350 - cost);
        w.players.get_mut(&caster).unwrap().ability_ready_ms = 0;
        w.players.get_mut(&caster).unwrap().character.mp = cost - 1;
        assert!(w.activate_ability(caster, slot, item, target).is_err());
        assert_eq!(w.entities[&enemy].hp, 1000 - dealt);
        w.players.get_mut(&caster).unwrap().character.mp = 350;
        w.players.get_mut(&ally).unwrap().character.hp = 669;
        w.activate_ability(caster, slot, item, target).unwrap();
        assert_eq!(w.players[&ally].character.hp, 670);
        w.players.get_mut(&caster).unwrap().ability_ready_ms = 0;
        let hp = w.players[&caster].character.hp;
        w.activate_ability(caster, slot, item, Pos { x: 8.5, y: 18.5 })
            .unwrap();
        assert_eq!(w.players[&caster].character.hp, hp);
        assert_eq!(w.players[&caster].character.mp, 350 - 2 * cost);
    }
}
