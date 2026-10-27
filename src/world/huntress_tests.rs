use super::*;
use crate::condition::{PARALYZED, SLOWED, bit};

fn fixture(kind: u16) -> (World, i32, Pos) {
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
    let mut world = World::new(content, Arc::new(map)).unwrap();
    world.clock.set_paused(true);
    let mut ch = Character::fresh(world.content.get(0x322).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.base[1] = 252;
    ch.mp = 252;
    ch.items[1] = kind.into();
    let owner = world.enter(ch, None).unwrap();
    (world, owner, Pos { x: 10.5, y: 10.5 })
}
fn cast(w: &mut World, owner: i32, target: Pos) -> Result<()> {
    let kind = w.players[&owner].character.items[1];
    let item = w.content.get(kind as u16)?.clone();
    w.activate_ability(
        owner,
        Slot {
            object: owner,
            slot: 1,
            kind,
        },
        &item,
        target,
    )
}
fn enemy(w: &mut World, center: Pos, distance: f32) -> i32 {
    let id = w
        .spawn_entity(
            0x917,
            Pos {
                x: center.x + distance,
                y: center.y,
            },
            vec![],
        )
        .unwrap();
    w.entities.get_mut(&id).unwrap().condition &= !bit(crate::condition::INVULNERABLE);
    id
}

#[test]
fn huntress_tiers_arm_then_blast_multiple_enemies_once() {
    for (kind, radius, damage, effect, duration, cost) in [
        (0xab1, 4., 50, SLOWED, 3000, 40),
        (0xab2, 4.5, 90, SLOWED, 3500, 60),
        (0xab3, 5., 140, SLOWED, 4000, 80),
        (0xab4, 5.5, 200, SLOWED, 4500, 100),
        (0xab5, 6., 250, SLOWED, 5000, 120),
        (0xab6, 6.5, 300, SLOWED, 5500, 140),
        (0xb2c, 6.5, 300, SLOWED, 6000, 140),
        (0xc1c, 4., 350, PARALYZED, 3500, 130),
    ] {
        let (mut w, owner, pos) = fixture(kind);
        let inner = enemy(&mut w, pos, radius / 2. - 0.1);
        let outer = enemy(&mut w, pos, radius - 0.1);
        let far = enemy(&mut w, pos, radius + 0.1);
        cast(&mut w, owner, pos).unwrap();
        assert_eq!(w.players[&owner].character.mp, 252 - cost);
        assert!(
            cast(&mut w, owner, pos)
                .unwrap_err()
                .to_string()
                .contains("cooldown")
        );
        w.now = 1499;
        w.ability_step();
        assert_eq!(w.entities[&inner].hp, 7500);
        assert_eq!(w.ability_state.thrown.len(), 1);
        w.now = 1500;
        w.ability_step();
        for target in [inner, outer] {
            assert_eq!(w.entities[&target].hp, 7500 - (damage - 19));
            assert_eq!(
                w.entities[&target].statuses[&effect],
                (1500, 1500 + duration)
            );
            assert_eq!(w.entities[&target].damage[&1], damage - 19);
            w.now = 1500 + duration;
            assert_eq!(w.entities[&target].conditions(w.now) & bit(effect), 0);
        }
        assert_eq!(w.entities[&far].hp, 7500);
        assert!(w.ability_state.thrown.is_empty() && w.ability_state.objects.is_empty());
        w.ability_step();
        assert_eq!(w.entities[&inner].hp, 7500 - (damage - 19));
    }
}

#[test]
fn huntress_outer_ring_does_not_trigger_but_ten_second_expiry_explodes() {
    let (mut w, owner, pos) = fixture(0xab1);
    let target = enemy(&mut w, pos, 3.9);
    cast(&mut w, owner, pos).unwrap();
    for now in [1500, 11499] {
        w.now = now;
        w.ability_step();
        assert_eq!(w.entities[&target].hp, 7500);
        assert_eq!(w.ability_state.objects.len(), 1);
    }
    w.now = 11500;
    w.ability_step();
    assert_eq!(w.entities[&target].hp, 7469);
    assert!(w.ability_state.objects.is_empty());
}

#[test]
fn huntress_rejections_and_owner_departure_leave_no_delayed_damage() {
    for state in [
        "mana",
        "Quiet",
        "paused",
        "dead",
        "departing",
        "quarantined",
        "wrong class",
        "range",
        "blocked",
    ] {
        let (mut w, owner, mut pos) = fixture(0xab1);
        let p = w.players.get_mut(&owner).unwrap();
        match state {
            "mana" => p.character.mp = 39,
            "Quiet" => {
                p.statuses.insert(crate::condition::QUIET, (0, 10000));
            }
            "paused" => p.paused = true,
            "dead" => p.death_pending = true,
            "departing" => p.departing = true,
            "quarantined" => p.persistence_failure = Some("injected failure".into()),
            "wrong class" => p.character.class = 0x30e,
            "range" => pos.x = 31.,
            "blocked" => Arc::make_mut(&mut w.map).cells[10 * 32 + 10].ground = 255,
            _ => unreachable!(),
        }
        let mana = w.players[&owner].character.mp;
        assert!(cast(&mut w, owner, pos).is_err(), "{state}");
        assert_eq!(w.players[&owner].character.mp, mana);
        assert!(w.ability_state.thrown.is_empty() && w.ability_state.objects.is_empty());
    }
    for landed in [false, true] {
        let (mut w, owner, pos) = fixture(0xab1);
        let target = enemy(&mut w, pos, 3.9);
        cast(&mut w, owner, pos).unwrap();
        if landed {
            w.now = 1500;
            w.ability_step();
        }
        let saved = w.leave(owner).unwrap();
        assert_eq!(saved.mp, 212);
        let next = w.enter(saved, None).unwrap();
        assert_ne!(next, owner);
        w.now = 12000;
        w.ability_step();
        assert_eq!(w.entities[&target].hp, 7500);
        assert!(w.ability_state.thrown.is_empty() && w.ability_state.objects.is_empty());
        assert!(!w.entities.values().any(|e| e.kind == 0x711));
    }
}

#[tokio::test]
async fn huntress_requires_both_twenty_records_and_preserves_death() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for (rogue, archer) in [(19, 20), (20, 19), (20, 20)] {
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let guid = format!("huntress-{rogue}-{archer}");
        let account = store.list(guid.clone()).await.unwrap().id;
        let huntress = content.get(0x322).unwrap();
        assert!(store.create(account, huntress).await.is_err());
        for (class, level) in [(0x30e, 5), (0x310, 5), (0x307, archer), (0x300, rogue)] {
            let mut ch = store
                .create(account, content.get(class).unwrap())
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
                .death(ch.clone(), "prepared unlock test".into())
                .await
                .unwrap();
            assert!(store.load(account, ch.id).await.is_err());
        }
        let listed = store.list(guid).await.unwrap();
        assert!(listed.characters.is_empty());
        assert_eq!(listed.best_levels[&0x300], rogue);
        assert_eq!(listed.best_levels[&0x307], archer);
        let created = store.create(account, huntress).await;
        if rogue == 20 && archer == 20 {
            let ch = created.unwrap();
            assert_eq!(ch.level, 1);
            assert_eq!(ch.items, huntress.equipment);
            assert_eq!(ch.base, Character::fresh(huntress).unwrap().base);
            assert!(ch.alive);
        } else {
            assert!(created.is_err());
        }
    }
}
