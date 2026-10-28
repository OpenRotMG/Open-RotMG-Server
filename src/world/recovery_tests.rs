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
fn potion_publication_is_silent_atomic_and_reports_only_actual_restoration() {
    for (index, kind) in [
        0xae9, 0xaea, 0xa1f, 0xa20, 0xa21, 0xa4c, 0xa34, 0xa35, 0xa22, 0xa23,
    ]
    .into_iter()
    .enumerate()
    {
        for succeeds in [false, true] {
            let (mut w, id) = fixture();
            let caps = w.content.classes[&0x30e].caps;
            let p = w.players.get_mut(&id).unwrap();
            p.character.base = caps;
            if index < 8 {
                p.character.base[index] -= if index < 2 { 2 } else { 1 };
            }
            p.character.hp = caps[0] - 7;
            p.character.mp = caps[1] - 9;
            p.character.items[4] = kind;
            p.known.insert(id);
            let before = p.character.clone();
            let (packets, mut inbox) = mpsc::channel(64);
            let (lost, _) = watch::channel(None);
            p.tx = Some(Output { packets, lost });
            let slot = Slot {
                object: id,
                slot: 4,
                kind,
            };
            let plan = w.use_item(id, slot, Pos { x: 0., y: 0. }).unwrap().unwrap();
            assert!(inbox.try_recv().is_err());
            assert_eq!(w.players[&id].character, before);
            assert!(w.use_item(id, slot, Pos { x: 0., y: 0. }).is_err());
            let result = if succeeds {
                Ok(before.revision + 1)
            } else {
                Err(anyhow::anyhow!("injected potion commit failure"))
            };
            assert_eq!(w.finish_inventory(plan, result).is_ok(), succeeds);
            let after = &w.players[&id].character;
            let packets: Vec<_> = std::iter::from_fn(|| inbox.try_recv().ok()).collect();
            assert!(!packets.iter().any(|p| p.id == wire::TEXT));
            if !succeeds {
                assert_eq!(after, &before);
                assert!(
                    !packets
                        .iter()
                        .any(|p| matches!(p.id, wire::NOTIFICATION | wire::SHOW_EFFECT | 4))
                );
                continue;
            }
            assert_eq!((after.items[4], after.revision), (-1, before.revision + 1));
            assert_eq!(after.base, caps);
            let notifications: Vec<_> = packets
                .iter()
                .filter(|p| p.id == wire::NOTIFICATION)
                .collect();
            if index < 8 {
                assert!(notifications.is_empty());
                w.players.get_mut(&id).unwrap().character.items[4] = kind;
                assert!(
                    w.use_item(id, slot, Pos { x: 0., y: 0. })
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("class cap")
                );
                assert!(inbox.try_recv().is_err());
                assert_eq!(w.players[&id].character.revision, before.revision + 1);
                assert_eq!(w.players[&id].character.items[4], kind);
            } else {
                // Kithio clips notifications to actual gain: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.UseItem.cs#L105-L151
                let mut r = wire::Reader::new(&notifications[0].body);
                assert_eq!(notifications.len(), 1);
                assert_eq!(r.i32().unwrap(), id);
                assert_eq!(r.utf().unwrap(), if index == 8 { "+7" } else { "+9" });
                assert_eq!(
                    if index == 8 { after.hp } else { after.mp },
                    caps[index - 8]
                );
            }
        }
    }
}

#[test]
fn fountain_cadence_recipients_range_sickness_and_caps() {
    let (mut w, near) = fixture();
    w.players.get_mut(&near).unwrap().pos = Pos { x: 8.5, y: 8.5 };
    w.fountains = vec![(999, Pos { x: 8.5, y: 8.5 })];
    let mut ids = vec![near];
    for account in 2..=5 {
        let mut ch = w.players[&near].character.clone();
        ch.account = account;
        ch.id = account;
        let id = w.enter(ch, None).unwrap();
        w.players.get_mut(&id).unwrap().pos = Pos { x: 8.5, y: 8.5 };
        ids.push(id);
    }
    let [_, ally, boundary, sick, pending] = ids[..] else {
        unreachable!()
    };
    w.players.get_mut(&ally).unwrap().character.hp = 669;
    w.players.get_mut(&boundary).unwrap().pos.x += 5.;
    w.apply_condition(sick, SICK, 10000).unwrap();
    w.players.get_mut(&pending).unwrap().pending = true;
    w.fountain_heal();
    let hp = |w: &World| {
        ids.iter()
            .map(|id| w.players[id].character.hp)
            .collect::<Vec<_>>()
    };
    assert_eq!(hp(&w), [200, 670, 100, 100, 100]);
    assert!(ids.iter().all(|id| w.players[id].character.mp == 100));
    w.now = 999;
    w.fountain_heal();
    assert_eq!(hp(&w), [200, 670, 100, 100, 100]);
    w.now = 1000;
    w.fountain_heal();
    assert_eq!(hp(&w), [300, 670, 100, 100, 100]);
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
