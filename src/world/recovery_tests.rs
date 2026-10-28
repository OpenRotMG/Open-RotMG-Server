use super::*;
use crate::condition::{BLEEDING, HEALING, QUIET, SICK};

fn fixture() -> (World, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
        width: 16,
        height: 16,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0,
            };
            256
        ],
        spawns: vec![(8.5, 8.5)],
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.clock.set_paused(true);
    let mut ch = Character::fresh(w.content.get(0x30e).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.items.fill(-1);
    ch.base = [670, 385, 50, 0, 50, 50, 40, 60];
    ch.hp = 100;
    ch.mp = 100;
    let id = w.enter(ch, None).unwrap();
    (w, id)
}

fn steps(w: &mut World, count: usize) {
    for _ in 0..count {
        w.step();
    }
}

#[test]
fn recovery_rates_and_condition_matrix() {
    // This guards reconstruction, not fidelity: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/realm/StatsManager.cs#L87-L141 conflicts with Kithio's MP divisor.
    for (conditions, hp, mp) in [
        (vec![], 106, 104),
        (vec![SICK], 100, 104),
        (vec![QUIET], 106, 0),
        (vec![BLEEDING], 79, 104),
        (vec![HEALING], 135, 104),
        (vec![SICK, HEALING], 100, 104),
        (vec![BLEEDING, HEALING], 108, 104),
        (vec![SICK, BLEEDING, QUIET, HEALING], 79, 0),
    ] {
        let (mut w, id) = fixture();
        for condition in &conditions {
            w.apply_condition(id, *condition, 10000).unwrap();
        }
        steps(&mut w, 21);
        let p = &w.players[&id];
        assert_eq!((p.character.hp, p.character.mp), (hp, mp), "{conditions:?}");
    }
    let (mut w, id) = fixture();
    w.players.get_mut(&id).unwrap().character.base[6..].fill(0);
    steps(&mut w, 40);
    let p = &w.players[&id];
    assert_eq!((p.character.hp, p.character.mp), (102, 101));
    w.apply_condition(id, BLEEDING, 10000).unwrap();
    w.players.get_mut(&id).unwrap().character.hp = 2;
    steps(&mut w, 40);
    assert_eq!(w.players[&id].character.hp, 1);
    assert!(w.players[&id].character.alive);
}

#[test]
fn recovery_fraction_pause_block_and_cap_boundaries() {
    let (mut w, id) = fixture();
    let p = w.players.get_mut(&id).unwrap();
    p.hp_fraction = 0.75;
    p.mp_fraction = 0.75;
    p.paused = true;
    steps(&mut w, 20);
    let p = w.players.get_mut(&id).unwrap();
    assert_eq!((p.character.hp, p.character.mp), (100, 100));
    assert_eq!((p.hp_fraction, p.mp_fraction), (0.75, 0.75));
    p.paused = false;
    w.apply_condition(id, SICK, 100).unwrap();
    w.apply_condition(id, QUIET, 100).unwrap();
    steps(&mut w, 1);
    let p = &w.players[&id];
    assert_eq!((p.hp_fraction, p.mp_fraction), (0., 0.));
    steps(&mut w, 1);
    let p = &w.players[&id];
    assert_eq!((p.character.hp, p.character.mp), (100, 0));
    assert!(p.hp_fraction > 0. && p.mp_fraction > 0.);
    let p = w.players.get_mut(&id).unwrap();
    p.character.hp = 669;
    p.character.mp = 384;
    steps(&mut w, 40);
    let p = &w.players[&id];
    assert_eq!((p.character.hp, p.character.mp), (670, 385));
    assert_eq!((p.hp_fraction, p.mp_fraction), (0., 0.));
    let p = w.players.get_mut(&id).unwrap();
    p.character.hp = 100;
    p.character.mp = 100;
    w.prepare_stat_modifier(id, 6, 40, 1050).unwrap();
    steps(&mut w, 20);
    assert_eq!(w.players[&id].character.hp, 111);
    steps(&mut w, 20);
    assert_eq!(w.players[&id].character.hp, 117);
    assert_eq!(w.players[&id].character.base[6], 40);
}

#[tokio::test]
async fn recovery_persists_vitals_not_fractions_or_temporary_effects() {
    let (mut w, id) = fixture();
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
    let account = store.list("recovery-test".into()).await.unwrap();
    let mut ch = store
        .create(account.id, w.content.get(0x30e).unwrap())
        .await
        .unwrap();
    let p = w.players.get_mut(&id).unwrap();
    ch.base = p.character.base;
    ch.hp = 100;
    ch.mp = 100;
    p.character = ch;
    w.prepare_stat_modifier(id, 7, 40, 10000).unwrap();
    steps(&mut w, 19);
    let p = &w.players[&id];
    assert!(p.hp_fraction > 0. && p.mp_fraction > 0.);
    let saved = p.character.clone();
    store.progress(saved.clone()).await.unwrap();
    let loaded = store.load(account.id, saved.id).await.unwrap();
    assert_eq!((loaded.hp, loaded.mp), (saved.hp, saved.mp));
    assert_eq!(loaded.base, saved.base);
    w.players.remove(&id);
    let id = w.enter(loaded, None).unwrap();
    let p = &w.players[&id];
    assert_eq!((p.hp_fraction, p.mp_fraction), (0., 0.));
    assert_eq!(
        p.effective(&w.content, w.now).unwrap(),
        saved.effective(&w.content).unwrap()
    );
    assert_eq!(p.conditions(w.now), 0);
}

#[test]
fn dungeon_drinks_commit_once_and_gate_each_resource_independently() {
    for (item, heal, mana) in [
        (0x707, 150, 0),
        (0xc12, 100, 100),
        (0xa3f, 100, 0),
        (0xc1a, 0, 100),
        (0xb36, 150, 0),
        (0xb37, 0, 150),
    ] {
        for (sick, quiet, full) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
        ] {
            let (mut w, id) = fixture();
            let p = w.players.get_mut(&id).unwrap();
            p.character.items[4] = item;
            if full {
                p.character.hp = 670;
                p.character.mp = 385;
            }
            if sick {
                w.apply_condition(id, SICK, 1000).unwrap();
            }
            if quiet {
                w.apply_condition(id, QUIET, 1000).unwrap();
            }
            let before = w.players[&id].character.clone();
            let slot = Slot {
                object: id,
                slot: 4,
                kind: item,
            };
            let plan = w.use_item(id, slot, w.players[&id].pos).unwrap().unwrap();
            let p = &w.players[&id];
            assert_eq!((p.character.hp, p.character.mp), (before.hp, before.mp));
            assert_eq!(p.character.items, before.items);
            assert!(w.use_item(id, slot, w.players[&id].pos).is_err());
            let replay = plan.clone();
            w.finish_inventory(plan, Ok(before.revision + 1)).unwrap();
            let p = &w.players[&id];
            let expected = (
                (before.hp + if sick { 0 } else { heal }).min(670),
                (before.mp + if quiet { 0 } else { mana }).min(385),
            );
            assert_eq!(
                (p.character.hp, p.character.mp),
                expected,
                "{item:#x} {sick}/{quiet}/{full}"
            );
            assert_eq!(p.character.items[4], -1);
            assert_eq!(p.character.revision, before.revision + 1);
            assert!(w.finish_inventory(replay, Ok(before.revision + 1)).is_err());
            assert_eq!(
                (w.players[&id].character.hp, w.players[&id].character.mp),
                expected
            );
        }
    }
}

#[test]
fn dungeon_drink_refusals_and_failed_commit_publish_no_effects() {
    for state in 0..5 {
        let (mut w, id) = fixture();
        let p = w.players.get_mut(&id).unwrap();
        p.character.items[4] = 0xc3a;
        match state {
            0 => p.paused = true,
            1 => p.pending = true,
            2 => p.character.hp = 0,
            _ => {}
        }
        let before = p.character.clone();
        let slot = Slot {
            object: id,
            slot: 4,
            kind: if state == 3 { 0xc12 } else { 0xc3a },
        };
        let result = w.use_item(id, slot, w.players[&id].pos);
        if state < 4 {
            assert!(result.is_err());
        } else {
            let plan = result.unwrap().unwrap();
            assert_eq!(
                w.players[&id].effective(&w.content, w.now).unwrap(),
                before.base
            );
            assert!(
                w.finish_inventory(plan, Err(anyhow::anyhow!("injected commit failure")))
                    .is_err()
            );
            assert!(w.players[&id].persistence_failure.is_some());
        }
        let p = &w.players[&id];
        assert_eq!(
            (p.character.hp, p.character.mp, p.character.revision),
            (before.hp, before.mp, before.revision)
        );
        assert_eq!(p.character.items, before.items);
        assert_eq!(p.effective(&w.content, w.now).unwrap(), before.base);
        assert_eq!(p.conditions(w.now), 0);
    }
}
