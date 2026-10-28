use super::*;
use crate::condition::{self, DAMAGING, HEALING, INVULNERABLE};

fn fixture(seal: u16) -> (World, [i32; 3]) {
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
    let ids = std::array::from_fn(|i| {
        let mut ch = Character::fresh(w.content.get(0x31f).unwrap()).unwrap();
        ch.account = i as i32 + 1;
        ch.id = 1;
        ch.hp = 100;
        ch.items[1] = seal.into();
        w.enter(ch, None).unwrap()
    });
    w.players.get_mut(&ids[1]).unwrap().pos.x += 2.;
    w.players.get_mut(&ids[2]).unwrap().pos.x += 9.;
    (w, ids)
}
fn cast(w: &mut World, id: i32) -> Result<()> {
    let kind = w.players[&id].character.items[1];
    let item = w.content.get(kind as u16)?.clone();
    w.activate_ability(
        id,
        Slot {
            object: id,
            slot: 1,
            kind,
        },
        &item,
        w.players[&id].pos,
    )
}

#[test]
fn paladin_seal_tiers_target_allies_once_without_max_hp_buff() {
    for (seal, cost, duration, range) in [
        (0xa52, 70, 5000, 5.5),
        (0xad9, 75, 6000, 5.75),
        (0xa53, 80, 7000, 6.),
        (0xada, 85, 8000, 6.25),
        (0xa54, 90, 9000, 6.5),
        (0xa55, 95, 10000, 7.),
        (0xb26, 95, 10000, 7.),
        (0xc06, 100, 5000, 5.),
    ] {
        let (mut w, [caster, ally, far]) = fixture(seal);
        w.players.get_mut(&ally).unwrap().pos.x = w.players[&caster].pos.x + range - 0.01;
        let before = w.players[&caster].effective(&w.content, w.now).unwrap();
        cast(&mut w, caster).unwrap();
        assert_eq!(w.players[&caster].character.mp, 100 - cost);
        for id in [caster, ally] {
            for effect in [HEALING, DAMAGING] {
                assert_eq!(w.players[&id].statuses[&effect], (0, duration));
            }
            assert_eq!(w.players[&id].character.hp, 100);
        }
        assert_eq!(
            w.players[&caster].effective(&w.content, w.now).unwrap(),
            before
        );
        assert!(w.players[&far].statuses.is_empty());
        assert_eq!(
            w.players[&caster].statuses.contains_key(&INVULNERABLE),
            seal == 0xc06
        );
        assert!(!w.players[&ally].statuses.contains_key(&INVULNERABLE));
        if seal == 0xc06 {
            assert_eq!(w.players[&caster].statuses[&INVULNERABLE], (0, 1400));
        }
        w.players.get_mut(&caster).unwrap().character.mp = 100;
        assert!(cast(&mut w, caster).is_err());
        assert_eq!(w.players[&caster].character.mp, 100);
    }
}

#[test]
fn paladin_overlap_is_nonstacking_and_preserves_longer_deadline() {
    let (mut w, [caster, ally, far]) = fixture(0xb26);
    cast(&mut w, caster).unwrap();
    w.now = 1000;
    w.players.get_mut(&ally).unwrap().character.items[1] = 0xa52;
    cast(&mut w, ally).unwrap();
    for id in [caster, ally] {
        for effect in [HEALING, DAMAGING] {
            assert_eq!(w.players[&id].statuses[&effect].1, 10000);
        }
        w.players.get_mut(&id).unwrap().character.base[6] = 0;
    }
    w.players.get_mut(&far).unwrap().character.base[6] = 0;
    for _ in 0..20 {
        w.step();
    }
    for id in [caster, ally] {
        assert!(
            (128..=129).contains(&w.players[&id].character.hp),
            "Healing must add 28 HP/s: player {id} has {} HP after one second from 100",
            w.players[&id].character.hp
        );
    }
    assert!((100..=101).contains(&w.players[&far].character.hp));
    w.apply_condition(caster, condition::SICK, 1000).unwrap();
    let hp = w.players[&caster].character.hp;
    w.step();
    assert_eq!(w.players[&caster].character.hp, hp);
    w.now = 10000;
    assert_eq!(
        w.players[&ally].conditions(w.now) & (condition::bit(HEALING) | condition::bit(DAMAGING)),
        0
    );
}

#[tokio::test]
async fn paladin_unlock_requires_both_twenty_and_retains_deleted_progress() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for levels in [[19, 20], [20, 19], [20, 20]] {
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let guid = format!("paladin-{}-{}", levels[0], levels[1]);
        let account = store.list(guid.clone()).await.unwrap();
        let paladin = content.get(0x31f).unwrap();
        assert!(store.create(account.id, paladin).await.is_err());
        for (class, level) in [0x31e, 0x310].into_iter().zip(levels) {
            let mut ch = store
                .create_prepared(account.id, content.get(class).unwrap(), None)
                .await
                .unwrap();
            ch.gain_experience(
                &content,
                crate::store::level_start(level).unwrap(),
                &mut rand::thread_rng(),
            )
            .unwrap();
            store.progress(ch.clone()).await.unwrap();
            store.delete_character(account.id, ch.id).await.unwrap();
            assert!(store.load(account.id, ch.id).await.is_err());
            if class == 0x31e || levels != [20, 20] {
                assert!(store.create(account.id, paladin).await.is_err());
            }
        }
        let listed = store.list(guid).await.unwrap();
        assert!(listed.characters.is_empty());
        let created = store.create(account.id, paladin).await;
        if levels == [20, 20] {
            let ch = created.unwrap();
            assert_eq!(ch.items, paladin.equipment);
            assert_eq!(ch.base, Character::fresh(paladin).unwrap().base);
            assert_eq!(ch.level, 1);
            assert!(ch.alive);
        } else {
            assert!(created.is_err());
        }
    }
}
