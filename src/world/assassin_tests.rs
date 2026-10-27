use super::*;

fn fixture(poison: u16) -> (World, i32, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
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
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.clock.set_paused(true);
    let mut ch = Character::fresh(w.content.get(0x320).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.base[1] = 252;
    ch.mp = 252;
    ch.items[1] = poison.into();
    let player = w.enter(ch, None).unwrap();
    let enemy = w
        .spawn_entity(0x668, Pos { x: 10.5, y: 8.5 }, vec![])
        .unwrap();
    (w, player, enemy)
}

fn cast(w: &mut World, player: i32, position: Pos) -> Result<()> {
    let kind = w.players[&player].character.items[1];
    let item = w.content.get(kind as u16)?.clone();
    w.activate_ability(
        player,
        Slot {
            object: player,
            slot: 1,
            kind,
        },
        &item,
        position,
    )
}

#[test]
fn assassin_poison_tiers_delay_defense_duration_and_credit() {
    for (item, raw, duration, cost, radius) in [
        (0xaa3, 150, 5000, 30, 2.5),
        (0xaa4, 250, 5500, 45, 3.),
        (0xaa5, 400, 6000, 65, 3.5),
        (0xaa6, 550, 6500, 80, 4.),
        (0xaa7, 700, 7000, 95, 4.5),
        (0xaa8, 900, 7500, 110, 5.),
        (0xb2a, 900, 7500, 110, 5.),
    ] {
        let (mut w, player, enemy) = fixture(item);
        let hp = w.entities[&enemy].hp;
        let pos = w.entities[&enemy].pos;
        let edge = w
            .spawn_entity(
                0x668,
                Pos {
                    x: pos.x + radius - 0.01,
                    y: pos.y,
                },
                vec![],
            )
            .unwrap();
        let outside = w
            .spawn_entity(
                0x668,
                Pos {
                    x: pos.x + radius + 0.01,
                    y: pos.y,
                },
                vec![],
            )
            .unwrap();
        cast(&mut w, player, pos).unwrap();
        assert_eq!(w.players[&player].character.mp, 252 - cost);
        assert!(cast(&mut w, player, pos).is_err());
        assert_eq!(w.players[&player].character.mp, 252 - cost);
        w.now = 1499;
        w.ability_step();
        assert_eq!(w.entities[&enemy].hp, hp);
        w.now = 1500;
        w.ability_step();
        assert_eq!(w.entities[&enemy].hp, hp);
        w.now += duration / 2;
        w.ability_step();
        assert_eq!(hp - w.entities[&enemy].hp, (raw - 12) / 2);
        w.now = 1500 + duration;
        w.ability_step();
        assert_eq!(hp - w.entities[&enemy].hp, raw - 12);
        assert_eq!(w.entities[&enemy].damage[&1], raw - 12);
        assert_eq!(hp - w.entities[&edge].hp, raw - 12);
        assert_eq!(w.entities[&outside].hp, hp);
        w.now += 10000;
        w.ability_step();
        assert_eq!(hp - w.entities[&enemy].hp, raw - 12);
        let effective = w.players[&player].character.effective(&w.content).unwrap();
        assert_eq!(
            effective[6..],
            if item == 0xb2a { [17, 12] } else { [15, 10] }
        );
    }
}

#[test]
fn assassin_poison_snapshots_landing_targets_and_stacks_per_cast_and_owner() {
    let (mut w, player, enemy) = fixture(0xaa3);
    let pos = w.entities[&enemy].pos;
    let mut ch = w.players[&player].character.clone();
    ch.id = 2;
    ch.account = 2;
    let ally = w.enter(ch, None).unwrap();
    let missed = w
        .spawn_entity(0x668, Pos { x: 20., y: 20. }, vec![])
        .unwrap();
    w.entities.get_mut(&enemy).unwrap().pos = Pos { x: 25., y: 25. };
    cast(&mut w, player, pos).unwrap();
    w.players.get_mut(&player).unwrap().ability_ready_ms = 0;
    cast(&mut w, player, pos).unwrap();
    cast(&mut w, ally, pos).unwrap();
    w.now = 1499;
    w.ability_step();
    w.entities.get_mut(&enemy).unwrap().pos = pos;
    w.now = 1500;
    w.ability_step();
    w.entities.get_mut(&enemy).unwrap().pos = Pos { x: 25., y: 25. };
    w.entities.get_mut(&missed).unwrap().pos = pos;
    w.now = 6500;
    w.ability_step();
    assert_eq!(w.entities[&enemy].hp, 7000 - 3 * 138);
    assert_eq!(w.entities[&enemy].damage[&1], 276);
    assert_eq!(w.entities[&enemy].damage[&2], 138);
    assert_eq!(w.entities[&missed].hp, 7000);
    assert!(w.entities[&missed].damage.is_empty());
}

#[test]
fn assassin_poison_departure_immunity_and_failure_paths() {
    for landed in [false, true] {
        let (mut w, player, enemy) = fixture(0xaa3);
        let pos = w.entities[&enemy].pos;
        cast(&mut w, player, pos).unwrap();
        if landed {
            w.now = 1500;
            w.ability_step();
        }
        w.leave(player).unwrap();
        w.now = 10000;
        w.ability_step();
        assert_eq!(w.entities[&enemy].hp, 7000);
    }
    let (mut w, player, enemy) = fixture(0xaa3);
    let pos = w.entities[&enemy].pos;
    w.apply_condition(enemy, crate::condition::INVULNERABLE, 2000)
        .unwrap();
    cast(&mut w, player, pos).unwrap();
    w.now = 1500;
    w.ability_step();
    w.now = 6500;
    w.ability_step();
    assert_eq!(w.entities[&enemy].hp, 7000);
    for failure in [
        "mana",
        "quiet",
        "paused",
        "dead",
        "departing",
        "quarantined",
        "wrong class",
    ] {
        let (mut w, player, enemy) = fixture(0xaa3);
        let p = w.players.get_mut(&player).unwrap();
        match failure {
            "mana" => p.character.mp = 29,
            "quiet" => {
                p.statuses.insert(crate::condition::QUIET, (0, 10000));
                p.character.mp = 0;
            }
            "paused" => p.paused = true,
            "dead" => p.death_pending = true,
            "departing" => p.departing = true,
            "quarantined" => p.persistence_failure = Some("prepared failure".into()),
            "wrong class" => p.character.class = 0x30e,
            _ => unreachable!(),
        }
        let mp = p.character.mp;
        let pos = w.entities[&enemy].pos;
        assert!(cast(&mut w, player, pos).is_err(), "{failure}");
        assert_eq!(w.players[&player].character.mp, mp, "{failure}");
        w.now = 10000;
        w.ability_step();
        assert_eq!(w.entities[&enemy].hp, 7000, "{failure}");
    }
}

#[tokio::test]
async fn assassin_requires_both_level_twenty_records_after_permanent_death() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for (rogue, wizard) in [(19, 20), (20, 19), (20, 20)] {
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let guid = format!("assassin-{rogue}-{wizard}");
        let account = store.list(guid.clone()).await.unwrap();
        let assassin = content.get(0x320).unwrap();
        assert!(store.create(account.id, assassin).await.is_err());
        for (class, level) in [
            (0x30e, 5),
            (0x310, 5),
            (0x307, 5),
            (0x300, rogue),
            (0x30e, wizard),
        ] {
            let mut ch = store
                .create(account.id, content.get(class).unwrap())
                .await
                .unwrap();
            ch.gain_experience(
                &content,
                crate::store::level_start(level).unwrap(),
                &mut rand::thread_rng(),
            )
            .unwrap();
            assert_eq!(ch.level, level);
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
        assert_eq!(listed.best_levels[&0x300], rogue);
        assert_eq!(listed.best_levels[&0x30e], wizard);
        let result = store.create(account.id, assassin).await;
        if rogue == 20 && wizard == 20 {
            let ch = result.unwrap();
            assert_eq!(ch.level, 1);
            assert!(ch.alive);
            assert_eq!(ch.items, assassin.equipment);
            assert_eq!(ch.base, Character::fresh(assassin).unwrap().base);
            assert_eq!(assassin.slots[..4], [2, 18, 6, 9]);
            assert_eq!(content.classes[&0x320].caps[4], 75);
        } else {
            assert!(result.is_err());
        }
    }
}
