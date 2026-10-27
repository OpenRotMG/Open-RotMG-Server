use super::*;
use crate::condition;

fn fixture(tome: u16) -> (World, [i32; 3]) {
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
                region: 0,
            };
            1024
        ],
        spawns: vec![(8.5, 8.5)],
    };
    let mut world = World::new(content, Arc::new(map)).unwrap();
    world.clock.set_paused(true);
    let ids = std::array::from_fn(|index| {
        let mut ch = Character::fresh(world.content.get(0x310).unwrap()).unwrap();
        ch.account = index as i32 + 1;
        ch.id = 1;
        ch.base[0] = 670;
        ch.base[1] = 385;
        ch.hp = 100;
        ch.mp = 300;
        ch.items[1] = tome.into();
        world.enter(ch, None).unwrap()
    });
    world.players.get_mut(&ids[1]).unwrap().pos.x += 4.;
    world.players.get_mut(&ids[2]).unwrap().pos.x += 9.;
    (world, ids)
}

fn cast(world: &mut World, id: i32) -> Result<()> {
    let kind = world.players[&id].character.items[1];
    let item = world.content.get(kind as u16)?.clone();
    world.activate_ability(
        id,
        Slot {
            object: id,
            slot: 1,
            kind,
        },
        &item,
        world.players[&id].pos,
    )
}

#[tokio::test]
async fn priest_unlock_requires_wizard_five_and_keeps_dead_characters_dead() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for level in [4, 5] {
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let guid = format!("priest-unlock-{level}");
        let account = store.list(guid.clone()).await.unwrap();
        let priest = content.get(0x310).unwrap();
        assert!(store.create(account.id, priest).await.is_err());
        let mut wizard = store
            .create(account.id, content.get(0x30e).unwrap())
            .await
            .unwrap();
        wizard
            .gain_experience(
                &content,
                crate::store::level_start(level).unwrap(),
                &mut rand::thread_rng(),
            )
            .unwrap();
        assert_eq!(wizard.level, level);
        store.progress(wizard.clone()).await.unwrap();
        wizard.hp = 0;
        wizard.alive = false;
        store
            .death(wizard.clone(), "prepared unlock test".into())
            .await
            .unwrap();
        let listed = store.list(guid).await.unwrap();
        assert_eq!(listed.best_levels[&0x30e], level);
        assert!(listed.characters.is_empty());
        let created = store.create(account.id, priest).await;
        if level == 4 {
            assert!(created.is_err());
        } else {
            let ch = created.unwrap();
            assert_ne!(ch.id, wizard.id);
            assert_eq!(ch.level, 1);
            assert_eq!(ch.items, priest.equipment);
            assert_eq!(ch.base, Character::fresh(priest).unwrap().base);
            assert!(ch.alive);
        }
    }
}

#[test]
fn priest_tiers_heal_only_in_range_and_spend_once() {
    for (tome, amount, cost) in [
        (0xa31, 100, 60),
        (0xad7, 130, 65),
        (0xa32, 165, 70),
        (0xad8, 205, 75),
        (0xa33, 250, 80),
        (0xa5b, 300, 85),
        (0xb25, 300, 85),
    ] {
        let (mut w, [caster, ally, far]) = fixture(tome);
        cast(&mut w, caster).unwrap();
        assert_eq!(w.players[&caster].character.mp, 300 - cost);
        for target in [caster, ally] {
            assert_eq!(w.players[&target].character.hp, 100 + amount);
        }
        assert_eq!(w.players[&far].character.hp, 100);
        let deadline = w.players[&caster].ability_ready_ms;
        assert!(deadline > 0);
        assert!(
            cast(&mut w, caster)
                .unwrap_err()
                .to_string()
                .contains("cooldown")
        );
        assert_eq!(w.players[&caster].character.mp, 300 - cost);
        w.players.get_mut(&caster).unwrap().ability_ready_ms = 0;
        w.players.get_mut(&ally).unwrap().character.hp = 669;
        cast(&mut w, caster).unwrap();
        assert_eq!(w.players[&ally].character.hp, 670);
    }
}

#[test]
fn priest_purification_heals_before_cleansing_and_preserves_positive_effects() {
    let (mut w, [caster, ally, far]) = fixture(0xc09);
    for target in [caster, ally, far] {
        for effect in (2..=11).chain([14, 16, 27, 28, 15, 19, 26]) {
            w.apply_condition(target, effect, 10000).unwrap();
        }
    }
    assert!(
        cast(&mut w, caster)
            .unwrap_err()
            .to_string()
            .contains("Quiet")
    );
    assert_eq!(w.players[&caster].character.mp, 0);
    w.players
        .get_mut(&caster)
        .unwrap()
        .statuses
        .remove(&condition::QUIET);
    w.players.get_mut(&caster).unwrap().character.mp = 300;
    cast(&mut w, caster).unwrap();
    for target in [caster, ally] {
        assert_eq!(w.players[&target].character.hp, 100);
        assert_eq!(
            w.players[&target].conditions(w.now),
            condition::bit(19) | condition::bit(26)
        );
    }
    assert_ne!(
        w.players[&far].conditions(w.now) & condition::bit(condition::SICK),
        0
    );
    assert_ne!(
        w.players[&far].conditions(w.now) & condition::bit(condition::SPEEDY),
        0
    );
    w.players.get_mut(&caster).unwrap().ability_ready_ms = 0;
    cast(&mut w, caster).unwrap();
    assert_eq!(w.players[&caster].character.hp, 350);
    assert_eq!(w.players[&ally].character.hp, 350);
    assert_eq!(w.players[&far].character.hp, 100);
    assert_eq!(w.players[&caster].character.mp, 100);
    let (mut w, [caster, ally, _]) = fixture(0xc09);
    w.apply_condition(ally, condition::SPEEDY, 10000).unwrap();
    cast(&mut w, caster).unwrap();
    assert_ne!(
        w.players[&ally].conditions(w.now) & condition::bit(condition::SPEEDY),
        0
    );
}

#[test]
fn priest_protection_is_self_armored_and_failures_leave_no_effects() {
    let (mut w, [caster, ally, _]) = fixture(0xc1e);
    w.apply_condition(ally, condition::SICK, 10000).unwrap();
    cast(&mut w, caster).unwrap();
    assert_eq!(w.players[&caster].character.hp, 350);
    assert_eq!(w.players[&caster].character.mp, 200);
    assert_eq!(w.players[&ally].character.hp, 100);
    assert_eq!(
        w.players[&caster].statuses[&condition::ARMORED],
        (w.now, w.now + 4000)
    );
    assert_eq!(
        w.players[&caster].conditions(w.now),
        condition::bit(condition::ARMORED)
    );
    assert!(!w.players[&ally].statuses.contains_key(&condition::ARMORED));
    w.now += 4000;
    assert_eq!(w.players[&caster].conditions(w.now), 0);
    for denied in [
        "mana",
        "paused",
        "dead",
        "departing",
        "quarantined",
        "wrong class",
    ] {
        let (mut w, [caster, ally, _]) = fixture(0xc1e);
        let p = w.players.get_mut(&caster).unwrap();
        match denied {
            "mana" => p.character.mp = 99,
            "paused" => p.paused = true,
            "dead" => p.death_pending = true,
            "departing" => p.departing = true,
            "quarantined" => p.persistence_failure = Some("injected failure".into()),
            "wrong class" => p.character.class = 0x30e,
            _ => unreachable!(),
        }
        let mana = p.character.mp;
        assert!(cast(&mut w, caster).is_err(), "{denied}");
        assert_eq!(w.players[&caster].character.mp, mana, "{denied}");
        for target in [caster, ally] {
            assert_eq!(w.players[&target].character.hp, 100, "{denied}");
            assert!(w.players[&target].statuses.is_empty(), "{denied}");
        }
    }
}
