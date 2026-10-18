use super::{tests, *};
use crate::wire::Reader;

fn arena() -> World {
    let mut w = tests::world();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut w.map).cells {
        cell.ground = 0x61;
    }
    w
}
fn numbers(w: &World, entity: i32, recipient: i32) -> BTreeMap<u8, i32> {
    let bytes = w.stats(entity, recipient).unwrap();
    let mut r = Reader::new(&bytes);
    assert_eq!(r.i32().unwrap(), entity);
    r.f32().unwrap();
    r.f32().unwrap();
    let count = r.i16().unwrap();
    let mut out = BTreeMap::new();
    for _ in 0..count {
        let id = r.u8().unwrap();
        if matches!(id, 31 | 62) {
            r.utf().unwrap();
        } else {
            assert!(out.insert(id, r.i32().unwrap()).is_none());
        }
    }
    r.done().unwrap();
    out
}
#[test]
fn player_status_distinguishes_name_boosts_wallet_character_fame_and_history() {
    let mut w = arena();
    let mut ch = tests::ch(&w);
    ch.name = "Guest1".into();
    ch.fame = 19;
    ch.items[3] = 0xa4f;
    let boosts = [10, 11, 2, 3, 4, 5, 6, 7];
    Arc::make_mut(&mut w.content)
        .objects
        .get_mut(&0xa4f)
        .unwrap()
        .bonuses = boosts
        .into_iter()
        .enumerate()
        .map(|(i, n)| (crate::content::STATS[i].0, n))
        .collect();
    let presentation = AccountPresentation {
        guild: None,
        credits: 73,
        fame: 27,
        best_fame: BTreeMap::from([(crate::content::WIZARD, 40), (crate::content::PRIEST, 150)]),
    };
    let (packets, mut rx) = mpsc::channel(64);
    let (lost, _) = watch::channel(false);
    let id = w
        .enter_at(ch, Some(Output { packets, lost }), None, presentation)
        .unwrap();
    let owner = numbers(&w, id, id);
    let peer = numbers(&w, id, 999);
    assert_eq!(owner[&65], 0);
    assert!(!peer.contains_key(&65));
    assert_eq!(owner[&35], 73);
    assert!(!peer.contains_key(&35));
    assert_eq!(owner[&39], 27);
    assert!(!peer.contains_key(&39));
    assert_eq!(
        (owner[&56], owner[&57], owner[&30], owner[&58]),
        (0, 19, 3, 150)
    );
    for (i, k) in [46, 47, 48, 49, 50, 53, 51, 52].into_iter().enumerate() {
        assert_eq!(owner[&k], boosts[i]);
    }
    let initial = w.stats(id, id).unwrap();
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|p| p.id == wire::UPDATE)
            .any(|p| p.body.windows(initial.len()).any(|bytes| bytes == initial))
    );
    let p = w.players.get_mut(&id).unwrap();
    p.character.name = "Named".into();
    p.character.items.swap(3, 7);
    p.character.xp_boost.active = true;
    let removed = numbers(&w, id, id);
    assert_eq!(removed[&56], 1);
    assert_eq!(removed[&65], 1);
    for k in [46, 47, 48, 49, 50, 53, 51, 52] {
        assert_eq!(removed[&k], 0);
    }
    assert_eq!(removed[&39], 27);
    assert!(!numbers(&w, id, 999).contains_key(&39));
}
#[test]
fn weapon_damage_matches_number_precision_order_and_rng_draw_contract() {
    for draw in [25, 125] {
        let mut w = arena();
        let mut ch = tests::ch(&w);
        ch.base[2] = 21;
        let kind = ch.items[0] as u16;
        let spec = Arc::make_mut(&mut w.content)
            .objects
            .get_mut(&kind)
            .unwrap()
            .projectiles
            .get_mut(&0)
            .unwrap();
        spec.min = draw;
        spec.max = draw;
        let id = w.enter(ch, None).unwrap();
        let pos = w.players[&id].pos;
        let seed = w.players[&id].seed;
        w.shoot(
            id,
            0,
            1,
            kind,
            Pos {
                x: pos.x + 0.3,
                y: pos.y,
            },
            0.,
        )
        .unwrap();
        assert_eq!(
            w.bullets.last().unwrap().damage,
            if draw == 25 { 23 } else { 115 }
        );
        assert_eq!(
            w.players[&id].seed, seed,
            "fixed damage must not advance RNG"
        );
    }
    let mut w = arena();
    let id = w.enter(tests::ch(&w), None).unwrap();
    let mut spec = w.content.objects[&0xa97].projectiles[&0].clone();
    spec.min = 25;
    spec.max = 30;
    let p = w.players.get_mut(&id).unwrap();
    p.seed = 12351;
    let mut seed = 12351u32;
    for _ in 0..20 {
        seed = ((u64::from(seed) * 16807) % 2147483647) as u32;
        assert_eq!(p.roll_damage(&spec), 25 + (seed % 5) as i32);
    }
}
#[test]
fn projectile_ground_rules_ignore_no_walk_but_stop_void_bounds_and_cover() {
    for friendly in [true, false] {
        for ground in [0x61, 0xbc, 255] {
            let mut w = arena();
            let id = w.enter(tests::ch(&w), None).unwrap();
            let start = Pos { x: 10.5, y: 100.5 };
            let target = Pos { x: 12.5, y: 100.5 };
            w.players.get_mut(&id).unwrap().pos = if friendly { start } else { target };
            let enemy = w.spawn_entity(0x600, target, vec![]).unwrap();
            let before = w.entities[&enemy].hp;
            let width = w.map.width as usize;
            Arc::make_mut(&mut w.map).cells[100 * width + 11].ground = ground;
            let mut spec = w.content.objects[&0x683].projectiles[&0].clone();
            spec.speed = 100.;
            spec.amplitude = 0.;
            spec.lifetime = 1000.;
            w.bullets.push(Bullet {
                owner: if friendly { id } else { enemy },
                source_kind: if friendly {
                    crate::content::WIZARD
                } else {
                    0x600
                },
                id: 1,
                friendly,
                start,
                pos: start,
                angle: 0.,
                born: 0,
                damage: 1,
                spec,
                hit: BTreeSet::new(),
            });
            for _ in 0..5 {
                w.step();
            }
            let expected = if ground == 255 { 0 } else { 1 };
            if friendly {
                assert_eq!(
                    w.entities[&enemy].hp,
                    before - expected,
                    "ground={ground} friendly"
                );
            } else {
                assert_eq!(
                    w.players[&id].character.hp,
                    100 - expected,
                    "ground={ground} hostile"
                );
            }
            assert!(w.bullets.is_empty());
            assert_eq!(w.walkable(Pos { x: 11.5, y: 100.5 }), ground == 0x61);
        }
    }
    for friendly in [true, false] {
        let mut w = arena();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let start = Pos { x: 0.1, y: 100.5 };
        let spec = w.content.objects[&0x683].projectiles[&0].clone();
        w.bullets.push(Bullet {
            owner: id,
            source_kind: crate::content::WIZARD,
            id: 1,
            friendly,
            start,
            pos: start,
            angle: std::f32::consts::PI,
            born: 0,
            damage: 10,
            spec,
            hit: BTreeSet::new(),
        });
        w.step();
        assert!(w.bullets.is_empty(), "bounds must consume either faction");
    }
}
#[test]
fn parametric_lobes_match_both_parities_before_rotation() {
    let w = arena();
    let spec = &w.content.objects[&0xc04].projectiles[&0];
    for id in 0..4 {
        for fraction in [0.125, 0.375] {
            for angle in [0., std::f32::consts::FRAC_PI_2, 0.73] {
                let phase = fraction * std::f64::consts::TAU;
                let a = phase.sin() * if id % 2 == 0 { -1. } else { 1. };
                let b = (2. * phase).sin() * if id % 4 < 2 { 1. } else { -1. };
                let theta = f64::from(angle);
                let m = f64::from(spec.magnitude);
                let expected = (
                    (a * theta.cos() - b * theta.sin()) * m,
                    (a * theta.sin() + b * theta.cos()) * m,
                );
                let actual = spec.offset(id, angle, spec.lifetime * fraction as f32);
                assert!(
                    (f64::from(actual.0) - expected.0).abs() < 0.00001
                        && (f64::from(actual.1) - expected.1).abs() < 0.00001,
                    "id={id}, fraction={fraction}, angle={angle}"
                );
            }
        }
    }
}
#[test]
fn fixed_sizes_reach_map_dynamic_and_wire_but_explicit_overrides_win() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let mut map = Map::load(std::path::Path::new("data/pirate-cave.wmap"), &content).unwrap();
    let boss_cell = map
        .cells
        .iter()
        .position(|c| c.object == Some(0x927))
        .unwrap();
    assert!(map.cells[boss_cell].config.is_empty());
    let mut w = World::new(content.clone(), Arc::new(map.clone())).unwrap();
    let boss = w.entities.values().find(|e| e.kind == 0x927).unwrap().id;
    assert_eq!(numbers(&w, boss, 0)[&2], 80);
    let spawn = w
        .map
        .spawns
        .iter()
        .map(|&(x, y)| Pos { x, y })
        .find(|p| w.walkable(*p))
        .unwrap();
    for (kind, size) in [
        (0x927, 80),
        (0x683, 105),
        (0x684, 110),
        (0x686, 120),
        (0x600, 100),
    ] {
        let id = w.spawn_entity(kind, spawn, vec![]).unwrap();
        assert_eq!(numbers(&w, id, 0)[&2], size, "kind={kind:#x}");
    }
    map.cells[boss_cell].config = "size:91".into();
    let w = World::new(content, Arc::new(map)).unwrap();
    assert_eq!(
        w.entities.values().find(|e| e.kind == 0x927).unwrap().size,
        91
    );
}
