use super::*;
use crate::condition::{DAMAGING, SPEEDY, STASIS, STASIS_IMMUNE, bit};

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
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.clock.set_paused(true);
    let mut ch = Character::fresh(w.content.get(0x323).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.base[1] = 385;
    ch.mp = 385;
    ch.items[1] = kind.into();
    let owner = w.enter(ch, None).unwrap();
    (w, owner, Pos { x: 10.5, y: 10.5 })
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
fn ready(w: &mut World, owner: i32, now: u64) {
    w.now = now;
    w.players.get_mut(&owner).unwrap().ability_ready_ms = 0;
}

#[test]
fn mystic_orbs_bound_radius_stasis_and_temporary_immunity() {
    // Post-expiry immunity prevents chain Stasis: https://web.archive.org/web/20120528163147id_/http://forums.wildshadow.com/wiki/realm-mad-god/class-guide-mystic
    for (kind, duration, cost) in [
        (0xae4, 3000, 60),
        (0xa42, 4000, 65),
        (0xa43, 5000, 70),
        (0xa44, 6000, 75),
        (0xa45, 7000, 80),
        (0xa46, 8000, 85),
        (0xb2d, 8000, 85),
        (0xc0b, 5000, 100),
    ] {
        let (mut w, owner, pos) = fixture(kind);
        let inner = w.spawn_entity(0x943, pos, vec![]).unwrap();
        let edge = w
            .spawn_entity(
                0x943,
                Pos {
                    x: pos.x + 3.,
                    ..pos
                },
                vec![],
            )
            .unwrap();
        let outside = w
            .spawn_entity(
                0x943,
                Pos {
                    x: pos.x + 3.01,
                    ..pos
                },
                vec![],
            )
            .unwrap();
        let immune = w.spawn_entity(0x917, pos, vec![]).unwrap();
        cast(&mut w, owner, pos).unwrap();
        assert_eq!(w.players[&owner].character.mp, 385 - cost);
        assert!(
            cast(&mut w, owner, pos)
                .unwrap_err()
                .to_string()
                .contains("cooldown")
        );
        for target in [inner, edge] {
            assert_eq!(w.entities[&target].statuses[&STASIS], (0, duration));
            assert_eq!(
                w.entities[&target].statuses[&STASIS_IMMUNE],
                (duration, duration + 3000)
            );
        }
        assert!(w.entities[&outside].statuses.is_empty());
        assert!(w.entities[&immune].statuses.is_empty());
        assert_eq!(
            w.damage_enemy(owner, inner, 1000, false, &[], None)
                .unwrap(),
            0
        );
        ready(&mut w, owner, duration - 1);
        w.players.get_mut(&owner).unwrap().character.mp = 385;
        cast(&mut w, owner, pos).unwrap();
        assert_eq!(w.entities[&inner].statuses[&STASIS], (0, duration));
        ready(&mut w, owner, duration);
        w.players.get_mut(&owner).unwrap().character.mp = 385;
        assert_eq!(w.entities[&inner].conditions(w.now) & bit(STASIS), 0);
        assert_ne!(w.entities[&inner].conditions(w.now) & bit(STASIS_IMMUNE), 0);
        cast(&mut w, owner, pos).unwrap();
        assert_eq!(w.entities[&inner].statuses[&STASIS], (0, duration));
        assert_eq!(
            w.entities[&inner].statuses[&STASIS_IMMUNE],
            (duration, duration + 3000)
        );
        assert!(w.damage_enemy(owner, inner, 100, false, &[], None).unwrap() > 0);
        ready(&mut w, owner, duration + 3000);
        w.players.get_mut(&owner).unwrap().character.mp = 385;
        assert_eq!(w.entities[&inner].conditions(w.now) & bit(STASIS_IMMUNE), 0);
        cast(&mut w, owner, pos).unwrap();
        assert_eq!(
            w.entities[&inner].statuses[&STASIS],
            (duration + 3000, 2 * duration + 3000)
        );
    }
}

#[test]
fn mystic_conflict_buffs_expire_and_rejections_do_not_spend() {
    let (mut w, owner, pos) = fixture(0xc0b);
    cast(&mut w, owner, pos).unwrap();
    for index in [SPEEDY, DAMAGING] {
        assert_eq!(w.players[&owner].statuses[&index], (0, 2500));
        assert_ne!(w.players[&owner].conditions(2499) & bit(index), 0);
        assert_eq!(w.players[&owner].conditions(2500) & bit(index), 0);
    }
    assert_eq!(w.content.get(0xc0b).unwrap().cooldown_ms, 5000);
    for state in [
        "mana",
        "Quiet",
        "wrong class",
        "paused",
        "dead",
        "departing",
        "quarantined",
        "range",
    ] {
        let (mut w, owner, mut pos) = fixture(0xc0b);
        let p = w.players.get_mut(&owner).unwrap();
        match state {
            "mana" => p.character.mp = 99,
            "Quiet" => {
                p.statuses.insert(crate::condition::QUIET, (0, 10000));
            }
            "wrong class" => p.character.class = 0x30e,
            "paused" => p.paused = true,
            "dead" => p.death_pending = true,
            "departing" => p.departing = true,
            "quarantined" => p.persistence_failure = Some("injected failure".into()),
            "range" => pos.x = 31.,
            _ => unreachable!(),
        }
        let mana = p.character.mp;
        assert!(cast(&mut w, owner, pos).is_err(), "{state}");
        assert_eq!(w.players[&owner].character.mp, mana);
        assert!(!w.players[&owner].statuses.contains_key(&SPEEDY));
        assert!(!w.players[&owner].statuses.contains_key(&DAMAGING));
    }
}

#[test]
fn mystic_123_5_1_minions_are_not_stasis_immune() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for kind in [
        0xd55, 0x943, 0x942, 0x1749, 0xd1c, 0xd1d, 0xd1e, 0xd1f, 0xd20, 0xd21, 0xd22, 0xd23, 0xd24,
    ] {
        assert!(!content.get(kind).unwrap().stasis_immune, "{kind:#x}");
    }
    let mystic = content.get(0x323).unwrap();
    assert_eq!(&mystic.slots[..4], &[17, 21, 14, 9]);
    assert_eq!(&mystic.equipment[..4], &[0xa97, 0xae4, -1, -1]);
}
