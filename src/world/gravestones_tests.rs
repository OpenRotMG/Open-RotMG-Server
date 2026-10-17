use super::tests::{ch, world};
use super::*;

const GRAVES: [u16; 11] = [
    0x723, 0x724, 0x725, 0x726, 0x727, 0x728, 0x729, 0x72a, 0x72b, 0x734, 0x735,
];

fn stage(w: &mut World, id: i32) {
    let p = w.players.get_mut(&id).unwrap();
    p.character.hp = 0;
    p.death_pending = true;
    assert_eq!(w.pending_deaths().len(), 1);
}

fn grave(w: &World) -> &Entity {
    w.entities
        .values()
        .find(|e| GRAVES.contains(&e.kind))
        .unwrap()
}

#[test]
fn gravestones_use_each_class_base_caps_for_all_eleven_forms() {
    let mut w = world();
    let classes: Vec<_> = w.content.classes.keys().copied().collect();
    for class in classes {
        for (tier, expected) in GRAVES.into_iter().enumerate() {
            let mut c = ch(&w);
            c.class = class;
            c.items = w.content.objects[&class].equipment.clone();
            c.level = if tier == 0 {
                1
            } else if tier == 1 {
                19
            } else {
                20
            };
            c.exp = crate::store::level_start(c.level).unwrap();
            c.base = w.content.classes[&class].caps.map(|cap| cap - 1);
            for value in c.base.iter_mut().take(tier.saturating_sub(2)) {
                *value += 1;
            }
            let id = w.enter(c, None).unwrap();
            stage(&mut w, id);
            w.finish_death(id).unwrap();
            let latest = w
                .entities
                .values()
                .filter(|e| GRAVES.contains(&e.kind))
                .max_by_key(|e| e.id)
                .unwrap();
            assert_eq!(latest.kind, expected, "class={class:x}, tier={tier}");
            assert_eq!(
                latest.expires,
                match tier {
                    0 => Some(30_000),
                    1 => Some(60_000),
                    2 => Some(300_000),
                    _ => None,
                }
            );
            assert_eq!(w.content.objects[&expected].class, "GameObject");
            assert!(!w.content.objects[&expected].enemy && !w.content.objects[&expected].blocks);
            w.begin_departure(id).unwrap();
            w.finish_departure(id).unwrap();
        }
    }
}

#[test]
fn gravestone_publication_follows_successful_once_only_death_completion() {
    let mut w = world();
    let id = w.enter(ch(&w), None).unwrap();
    let position = w.players[&id].pos;
    let count = w.entities.len();
    assert!(w.finish_death(id).is_err());
    stage(&mut w, id);
    assert_eq!(w.entities.len(), count);
    w.committed(id, Ok(())).unwrap();
    w.finish_death(id).unwrap();
    assert_eq!(w.entities.len(), count + 1);
    let e = grave(&w);
    assert_eq!(e.name, "Test");
    assert_eq!((e.pos.x, e.pos.y), (position.x, position.y));
    assert!(e.items.is_empty() && e.parent.is_none() && e.camp.is_none());
    assert!(w.walkable(position));
    assert!(!w.players[&id].character.alive);
    assert!(
        w.players[&id]
            .character
            .items
            .iter()
            .all(|kind| *kind == -1)
    );
    assert!(w.finish_death(id).is_err());
    assert_eq!(w.entities.len(), count + 1);

    let mut failed = world();
    let id = failed.enter(ch(&failed), None).unwrap();
    stage(&mut failed, id);
    assert!(
        failed
            .committed::<()>(id, Err(anyhow::anyhow!("disk failure")))
            .is_err()
    );
    assert!(failed.finish_death(id).is_err());
    assert!(!failed.entities.values().any(|e| GRAVES.contains(&e.kind)));
}

#[test]
fn gravestones_ignore_equipment_bonuses_and_maxed_stats_take_priority_over_level() {
    let mut w = world();
    let mut c = ch(&w);
    c.level = 20;
    c.base = w.content.classes[&c.class].caps.map(|cap| cap - 1);
    let ring = w
        .content
        .objects
        .values()
        .find(|o| {
            o.slot_type == w.content.objects[&c.class].slots[3]
                && o.bonuses.iter().any(|(_, amount)| *amount > 0)
        })
        .unwrap();
    c.items[3] = i32::from(ring.kind);
    assert!(
        c.effective(&w.content)
            .unwrap()
            .iter()
            .zip(w.content.classes[&c.class].caps)
            .any(|(value, cap)| *value >= cap)
    );
    let id = w.enter(c, None).unwrap();
    stage(&mut w, id);
    w.finish_death(id).unwrap();
    assert_eq!(grave(&w).kind, 0x725);

    let mut w = world();
    let mut c = ch(&w);
    c.base[3] = w.content.classes[&c.class].caps[3];
    let id = w.enter(c, None).unwrap();
    stage(&mut w, id);
    w.finish_death(id).unwrap();
    assert_eq!(grave(&w).kind, 0x726);
    w.now = 3_600_000;
    w.mechanic_expiry();
    assert_eq!(grave(&w).kind, 0x726);
}

#[test]
fn gravestones_publish_native_names_and_exact_expiry_removals_without_loot() {
    for (level, lifetime) in [(1, 30_000), (19, 60_000), (20, 300_000)] {
        let mut w = world();
        let mut c = ch(&w);
        c.level = level;
        let dying = w.enter(c, None).unwrap();
        let mut observer = ch(&w);
        observer.id = 2;
        observer.account = 2;
        let (packets, mut rx) = mpsc::channel(128);
        let (lost, _) = watch::channel(false);
        let viewer = w.enter(observer, Some(Output { packets, lost })).unwrap();
        while rx.try_recv().is_ok() {}
        w.now = 1_000;
        stage(&mut w, dying);
        w.finish_death(dying).unwrap();
        let grave = grave(&w).id;
        w.updates(viewer, false).unwrap();
        let mut name = Writer::new();
        text_stat(&mut name, 31, "Test");
        let mut expected = Writer::new();
        expected.i16(0).i16(1).i16(w.entities[&grave].kind as i16);
        expected.0.extend(w.stats(grave, viewer).unwrap());
        expected.i16(0);
        assert!(
            expected
                .0
                .windows(name.0.len())
                .any(|bytes| bytes == name.0)
        );
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|p| p.id == wire::UPDATE && p.body == expected.0)
        );
        assert!(
            w.swap(
                viewer,
                Slot {
                    object: grave,
                    slot: 0,
                    kind: -1
                },
                Slot {
                    object: viewer,
                    slot: 4,
                    kind: -1
                }
            )
            .is_err()
        );
        w.now = 1_000 + lifetime - 1;
        w.mechanic_expiry();
        assert!(w.entities.contains_key(&grave));
        w.now += 1;
        w.mechanic_expiry();
        assert!(!w.entities.contains_key(&grave));
        assert!(w.cells.iter().all(|cell| !cell.contains(&grave)));
        w.updates(viewer, false).unwrap();
        let mut dropped = Writer::new();
        dropped.i16(0).i16(0).i16(1).i32(grave);
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|p| p.id == wire::UPDATE && p.body == dropped.0)
        );
    }
}

#[test]
fn gravestones_remain_at_death_position_even_if_a_gate_closed_on_it() {
    let mut w = world();
    let id = w.enter(ch(&w), None).unwrap();
    let pos = w.players[&id].pos;
    let wall = w
        .content
        .objects
        .values()
        .find(|o| o.blocks && !o.enemy)
        .unwrap()
        .kind;
    w.spawn_entity(wall, pos, vec![]).unwrap();
    assert!(!w.walkable(pos));
    stage(&mut w, id);
    w.finish_death(id).unwrap();
    assert_eq!((grave(&w).pos.x, grave(&w).pos.y), (pos.x, pos.y));
}
