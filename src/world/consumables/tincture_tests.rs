use super::*;

fn fixture() -> (World, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/nexus-limbo.wmap"), &content).unwrap());
    let mut w = World::new(content, map).unwrap();
    w.clock.set_paused(true);
    let mut ch = Character::fresh(w.content.get(0x30e).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.items.fill(-1);
    ch.base = [670, 385, 50, 25, 50, 60, 0, 0];
    ch.hp = 670;
    ch.mp = 385;
    let id = w.enter(ch, None).unwrap();
    (w, id)
}
fn slot(id: i32, kind: u16) -> Slot {
    Slot {
        object: id,
        slot: 4,
        kind: kind.into(),
    }
}
fn begin(w: &mut World, id: i32, kind: u16) -> InventoryPlan {
    w.players.get_mut(&id).unwrap().character.items[4] = kind.into();
    w.use_item(id, slot(id, kind), w.players[&id].pos)
        .unwrap()
        .unwrap()
}
fn drink(w: &mut World, id: i32, kind: u16) {
    let plan = begin(w, id, kind);
    let revision = plan.revision + 1;
    w.finish_inventory(plan, Ok(revision)).unwrap();
}
fn stats(w: &World, id: i32) -> [i32; 8] {
    w.players[&id].effective(&w.content, w.now).unwrap()
}

#[test]
fn boosts_diminish_per_stat_and_expire_independently() {
    let (mut w, id) = fixture();
    let base = stats(&w, id);
    for expected in [15, 22, 25, 26, 26] {
        drink(&mut w, id, 0xb0c);
        assert_eq!(stats(&w, id)[5], base[5] + expected);
        w.now += 1000;
    }
    drink(&mut w, id, 0xb0f);
    assert_eq!(stats(&w, id)[3], base[3] + 15);
    w.now = 30_000;
    assert_eq!(stats(&w, id)[5], base[5] + 26);
    w.now = 34_000;
    assert_eq!(stats(&w, id)[5], base[5]);
    assert_eq!(stats(&w, id)[3], base[3] + 15);
    w.now = 35_000;
    assert_eq!(stats(&w, id), base);
    for sequence in [[0xb0c, 0xb10], [0xb10, 0xb0c]] {
        for kind in sequence {
            drink(&mut w, id, kind);
        }
        assert_eq!(stats(&w, id)[5], base[5] + 37);
        w.now += 30_000;
    }
    for kind in [0xb0d, 0xb0d, 0xb0e, 0xb0e] {
        drink(&mut w, id, kind);
    }
    assert_eq!(&stats(&w, id)[..2], &[782, 497]);
    assert_eq!(
        (w.players[&id].character.hp, w.players[&id].character.mp),
        (670, 385)
    );
    let p = w.players.get_mut(&id).unwrap();
    p.character.hp = 782;
    p.character.mp = 497;
    w.prepare_stat_modifier(id, 5, -10, 30_000).unwrap();
    drink(&mut w, id, 0xb0c);
    drink(&mut w, id, 0xb0c);
    assert_eq!(stats(&w, id)[5], base[5] + 12);
    w.players.get_mut(&id).unwrap().character.items[3] = 0xa4d;
    assert_eq!(stats(&w, id)[5], base[5] + 15);
    w.players.get_mut(&id).unwrap().character.items[3] = -1;
    w.now += 30_000;
    w.expire_consumable_boosts();
    assert_eq!(stats(&w, id), base);
    assert_eq!(
        (w.players[&id].character.hp, w.players[&id].character.mp),
        (670, 385)
    );
    assert_eq!(w.players[&id].character.base, base);
    for _ in 0..128 {
        drink(&mut w, id, 0xb0c);
    }
    assert_eq!(stats(&w, id)[5], base[5] + 26);
    let revision = w.players[&id].character.revision;
    w.players.get_mut(&id).unwrap().character.items[4] = 0xb0c;
    assert!(w.use_item(id, slot(id, 0xb0c), w.players[&id].pos).is_err());
    assert_eq!(w.players[&id].character.revision, revision);
    assert_eq!(w.players[&id].character.items[4], 0xb0c);
    assert!(!w.players[&id].pending);
    assert_eq!(w.players[&id].boosts.len(), 128);
    w.now += 30_000;
    drink(&mut w, id, 0xb0c);
    assert_eq!(stats(&w, id)[5], base[5] + 15);
}

#[test]
fn aura_recipients_and_inventory_commit_remain_authoritative() {
    let (mut w, id) = fixture();
    let base = stats(&w, id);
    let mut ch = w.players[&id].character.clone();
    ch.account = 2;
    ch.id = 2;
    let ally = w.enter(ch, None).unwrap();
    let pos = w.players[&id].pos;
    for (distance, included) in [(5.999, true), (6., false), (6.001, false)] {
        w.players.get_mut(&ally).unwrap().pos = Pos {
            x: pos.x + distance,
            y: pos.y,
        };
        let plan = begin(&mut w, id, 0xb10);
        assert_eq!(stats(&w, id), base);
        assert_eq!(stats(&w, ally), base);
        let revision = plan.revision + 1;
        w.finish_inventory(plan, Ok(revision)).unwrap();
        assert_eq!(stats(&w, ally)[5], base[5] + if included { 30 } else { 0 });
        w.now += 30_000;
    }
    w.players.get_mut(&ally).unwrap().pos = pos;
    for state in 0..3 {
        let p = w.players.get_mut(&ally).unwrap();
        p.paused = state == 0;
        p.pending = state == 1;
        p.character.hp = if state == 2 { 0 } else { 670 };
        drink(&mut w, id, 0xb10);
        assert_eq!(stats(&w, ally), base);
        w.now += 30_000;
    }
    let plan = begin(&mut w, id, 0xb0c);
    assert!(w.use_item(id, slot(id, 0xb0c), pos).is_err());
    assert_eq!(stats(&w, id), base);
    assert!(
        w.finish_inventory(plan, Err(anyhow::anyhow!("injected commit failure")))
            .is_err()
    );
    assert_eq!(stats(&w, id), base);
    assert_eq!(w.players[&id].character.items[4], 0xb0c);
    assert!(
        w.players
            .values()
            .all(|p| p.boosts.iter().all(|b| b.until.is_some()))
    );
}
