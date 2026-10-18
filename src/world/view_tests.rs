use super::tests::receive;
use super::*;
use crate::content::WIZARD;
use crate::wire::Reader;

#[test]
fn incremental_discovery_preserves_sorted_snapshots_and_invalidates_weak_cache_keys() {
    let mut d = super::tests::directory();
    let (id, _rx) = receive(&mut d, -2, 1, WIZARD);
    let first = d.player_view(id).unwrap().tiles;
    let first_json = serde_json::to_value(&first).unwrap();
    {
        let w = d.worlds.get_mut(&-2).unwrap();
        w.players.get_mut(&id).unwrap().pos.x += 3.;
        w.updates(id, false).unwrap();
    }
    let updated = d.player_view(id).unwrap().tiles;
    assert!(!Arc::ptr_eq(&first, &updated));
    assert_eq!(serde_json::to_value(&first).unwrap(), first_json);
    assert!(
        updated
            .windows(2)
            .all(|p| (p[0].x, p[0].y) < (p[1].x, p[1].y))
    );
    let w = &d.worlds[&-2];
    let p = &w.players[&id];
    let cache = p.view_tiles.borrow();
    assert_eq!(cache.rebuilds, 1);
    assert!(cache.incremental_tiles > 0);
    assert_eq!(cache.cloned_snapshot_tiles, first.len() as u64);
    assert_eq!(updated.len(), p.tiles.len());
    for t in updated.iter() {
        let ground = w.map.cells[t.y as usize * w.map.width as usize + t.x as usize].ground;
        assert_eq!(t.ground, ground);
        assert_eq!(t.ground_speed, w.content.grounds[&ground]);
    }
    drop(cache);
    let weak = Arc::downgrade(&updated);
    drop(updated);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.players.get_mut(&id).unwrap().pos.x += 3.;
    w.updates(id, false).unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(d.worlds[&-2].players[&id].view_tiles.borrow().rebuilds, 1);
}

#[test]
fn terrain_views_reuse_storage_but_invalidate_discovery_map_and_content() {
    let mut d = super::tests::directory();
    let (id, _rx) = receive(&mut d, -2, 1, WIZARD);
    let first = d.player_view(id).unwrap().tiles;
    assert!(Arc::ptr_eq(&first, &d.player_view(id).unwrap().tiles));
    let w = d.worlds.get_mut(&-2).unwrap();
    let point = w
        .map
        .cells
        .iter()
        .enumerate()
        .find_map(|(i, c)| {
            let p = (
                (i % w.map.width as usize) as i16,
                (i / w.map.width as usize) as i16,
            );
            (w.content.grounds.contains_key(&c.ground) && !w.players[&id].tiles.contains(&p))
                .then_some(p)
        })
        .unwrap();
    w.players.get_mut(&id).unwrap().tiles.insert(point);
    let discovered = d.player_view(id).unwrap().tiles;
    assert!(!Arc::ptr_eq(&first, &discovered));
    assert_eq!(discovered.len(), first.len() + 1);
    assert!(Arc::ptr_eq(&discovered, &d.player_view(id).unwrap().tiles));
    let w = d.worlds.get_mut(&-2).unwrap();
    let tile = &discovered[0];
    let index = tile.y as usize * w.map.width as usize + tile.x as usize;
    let ground = *w
        .content
        .grounds
        .keys()
        .find(|g| **g != tile.ground)
        .unwrap();
    Arc::make_mut(&mut w.map).cells[index].ground = ground;
    let changed = d.player_view(id).unwrap().tiles;
    assert!(!Arc::ptr_eq(&changed, &discovered));
    assert_eq!(changed[0].ground, ground);
    Arc::make_mut(&mut d.worlds.get_mut(&-2).unwrap().content)
        .grounds
        .insert(ground, 0.123);
    let latest = d.player_view(id).unwrap().tiles;
    assert!(!Arc::ptr_eq(&latest, &changed));
    assert_eq!(latest[0].ground_speed, 0.123);
    assert_eq!(
        serde_json::to_value(d.player_view(id).unwrap()).unwrap()["tiles"]
            .as_array()
            .unwrap()
            .len(),
        latest.len()
    );
}

#[test]
fn own_condition_mask_matches_timed_native_state_without_private_deadlines() {
    let mut d = super::tests::directory();
    let (id, _rx) = receive(&mut d, -2, 1, WIZARD);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.players
        .get_mut(&id)
        .unwrap()
        .statuses
        .insert(crate::condition::SLOWED, (0, 150));
    w.now = 149;
    let v = d.player_view(id).unwrap();
    assert_eq!(v.character.conditions, crate::content::SLOWED);
    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("until") && !json.contains("since"));
    let w = d.worlds.get_mut(&-2).unwrap();
    w.now = 150;
    w.players.get_mut(&id).unwrap().paused = true;
    assert_eq!(d.player_view(id).unwrap().character.conditions, 1 << 20);
}

#[test]
fn restricted_view_excludes_private_inventory_ai_and_undiscovered_collision() {
    let mut d = super::tests::directory();
    let (a, _ar) = receive(&mut d, -2, 1, WIZARD);
    let (b, _br) = receive(&mut d, -2, 2, WIZARD);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.players.get_mut(&b).unwrap().character.items[11] = 88888;
    let pos = w.players[&a].pos;
    let bag = w.spawn_entity(0x508, pos, vec![0xa22]).unwrap();
    w.entities.get_mut(&bag).unwrap().extra_stats.push((54, 1));
    let hidden = w.spawn_entity(0x508, pos, vec![0xa23]).unwrap();
    w.entities
        .get_mut(&hidden)
        .unwrap()
        .extra_stats
        .push((54, 2));
    let staged = w.spawn_entity(0x71d, pos, vec![]).unwrap();
    w.entities.get_mut(&staged).unwrap().published = false;
    w.updates(a, false).unwrap();
    let view = d.player_view(a).unwrap();
    assert_eq!(view.session.world, -2);
    assert_eq!(
        view.character.base_stats,
        d.worlds[&-2].players[&a].character.base
    );
    let peer = view.objects.iter().find(|o| o.id == b).unwrap();
    assert_eq!(peer.equipped.len(), 4);
    assert_eq!(peer.level, Some(d.worlds[&-2].players[&b].character.level));
    assert!(peer.contents.is_empty());
    assert_eq!(
        view.objects.iter().find(|o| o.id == bag).unwrap().contents,
        vec![0xa22]
    );
    assert!(
        !view
            .objects
            .iter()
            .any(|o| o.id == hidden || o.id == staged)
    );
    assert!(view.tiles.len() < d.worlds[&-2].map.cells.len());
    assert!(!view.tiles.iter().any(|t| t.x == 0 && t.y == 0));
    let json = serde_json::to_string(&view).unwrap();
    for private in [
        "88888",
        "next_shot",
        "damage",
        "hit",
        "home",
        "goal",
        "config",
        "terrain",
        "region",
    ] {
        assert!(!json.contains(private), "private field leaked: {private}");
    }
}
#[test]
fn own_condition_view_matches_native_stat29_without_exposing_other_statuses() {
    let mut d = super::tests::directory();
    let (a, _ar) = receive(&mut d, -2, 1, WIZARD);
    let (b, _br) = receive(&mut d, -2, 2, WIZARD);
    let w = d.worlds.get_mut(&-2).unwrap();
    for effect in [
        crate::condition::SLOWED,
        crate::condition::SICK,
        crate::condition::QUIET,
    ] {
        w.apply_condition(a, effect, 2000).unwrap();
    }
    w.apply_condition(b, crate::condition::STUNNED, 5000)
        .unwrap();
    w.updates(a, false).unwrap();
    for (now, paused) in [(0, false), (1000, true), (2000, false), (2000, true)] {
        let w = d.worlds.get_mut(&-2).unwrap();
        w.now = now;
        w.players.get_mut(&a).unwrap().paused = paused;
        let before = w.players[&a].statuses.clone();
        let bytes = w.stats(a, a).unwrap();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.i32().unwrap(), a);
        r.f32().unwrap();
        r.f32().unwrap();
        let count = r.i16().unwrap();
        let mut mask = None;
        for _ in 0..count {
            let kind = r.u8().unwrap();
            if matches!(kind, 31 | 62) {
                r.utf().unwrap();
            } else {
                let value = r.i32().unwrap();
                if kind == 29 {
                    mask = Some(value);
                }
            }
        }
        r.done().unwrap();
        let view = d.player_view(a).unwrap();
        assert_eq!(Some(view.character.conditions), mask);
        assert_eq!(view.character.conditions & (1 << 20) != 0, paused);
        if now == 2000 {
            assert_eq!(view.character.conditions, if paused { 1 << 20 } else { 0 });
        }
        let json = serde_json::to_value(view).unwrap();
        assert!(
            json["objects"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|o| o["kind"] == WIZARD)
                .all(|o| o.get("condition_mask").is_none())
        );
        assert_eq!(d.worlds[&-2].players[&a].statuses, before);
    }
}
#[test]
fn known_visible_npc_condition_view_matches_native29_without_hidden_entities() {
    let mut d = super::tests::directory();
    let (a, _ar) = receive(&mut d, -2, 1, WIZARD);
    let w = d.worlds.get_mut(&-2).unwrap();
    let pos = w.players[&a].pos;
    let npc = w.spawn_entity(0x600, pos, vec![]).unwrap();
    let hidden = w.spawn_entity(0x600, pos, vec![]).unwrap();
    w.entities.get_mut(&hidden).unwrap().published = false;
    w.entities.get_mut(&npc).unwrap().condition =
        crate::condition::bit(crate::condition::INVULNERABLE);
    w.apply_condition(npc, crate::condition::STUNNED, 2000)
        .unwrap();
    w.updates(a, false).unwrap();
    for now in [0, 2000] {
        let w = d.worlds.get_mut(&-2).unwrap();
        w.now = now;
        let before = w.entities[&npc].statuses.clone();
        let bytes = w.stats(npc, a).unwrap();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.i32().unwrap(), npc);
        r.f32().unwrap();
        r.f32().unwrap();
        let count = r.i16().unwrap();
        let mut native_mask = None;
        for _ in 0..count {
            let kind = r.u8().unwrap();
            if kind == 31 {
                r.utf().unwrap();
            } else {
                let value = r.i32().unwrap();
                if kind == 29 {
                    native_mask = Some(value);
                }
            }
        }
        r.done().unwrap();
        let view = d.player_view(a).unwrap();
        assert_eq!(
            view.objects
                .iter()
                .find(|o| o.id == npc)
                .unwrap()
                .condition_mask,
            native_mask
        );
        assert!(native_mask.unwrap() & crate::condition::bit(crate::condition::INVULNERABLE) != 0);
        assert_eq!(
            native_mask.unwrap() & crate::condition::bit(crate::condition::STUNNED) != 0,
            now == 0
        );
        assert!(!view.objects.iter().any(|o| o.id == hidden));
        assert_eq!(d.worlds[&-2].entities[&npc].statuses, before);
    }
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&a)
        .unwrap()
        .known
        .remove(&npc);
    assert!(
        !d.player_view(a)
            .unwrap()
            .objects
            .iter()
            .any(|o| o.id == npc)
    );
}
#[test]
fn quest_view_requires_the_same_published_native_target() {
    let mut d = super::tests::directory();
    let (id, _rx) = receive(&mut d, -3, 1, WIZARD);
    let w = d.worlds.get_mut(&-3).unwrap();
    w.players.get_mut(&id).unwrap().character.level = 5;
    w.updates(id, false).unwrap();
    let quest = w.players[&id].quest.unwrap();
    assert_eq!(d.player_view(id).unwrap().quest, Some(quest));
    assert!(
        d.player_view(id)
            .unwrap()
            .objects
            .iter()
            .any(|o| o.id == quest)
    );
    d.worlds
        .get_mut(&-3)
        .unwrap()
        .players
        .get_mut(&id)
        .unwrap()
        .known
        .remove(&quest);
    assert!(d.player_view(id).unwrap().quest.is_none());
}

#[test]
fn projectile_view_is_observed_trajectory_not_rng_hit_ledger_or_old_shots() {
    let mut d = super::tests::directory();
    let (a, _ar) = receive(&mut d, -2, 1, WIZARD);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.now = 1000;
    let pos = w.players[&a].pos;
    w.shoot(a, 1000, 1, 0xa97, pos, 0.).unwrap();
    assert_eq!(d.player_view(a).unwrap().projectiles.len(), 1);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.bullets[0].damage = 99999;
    w.bullets[0].hit.insert(54321);
    let json = serde_json::to_string(&d.player_view(a).unwrap()).unwrap();
    assert!(!json.contains("99999") && !json.contains("54321"));
    let w = d.worlds.get_mut(&-2).unwrap();
    w.players.get_mut(&a).unwrap().known_since.insert(a, 1000);
    assert!(d.player_view(a).unwrap().projectiles.is_empty());
}
#[test]
fn actual_scoped_chat_stream_and_old_session_remain_distinct_from_replacement_view() {
    let mut d = super::tests::directory();
    let (a, mut ar) = receive(&mut d, -2, 1, WIZARD);
    let (_b, mut br) = receive(&mut d, -2, 2, WIZARD);
    let (_c, mut cr) = receive(&mut d, -3, 3, WIZARD);
    let old = d.player_view(a).unwrap().session;
    d.chat(a, "Ready.".into(), Instant::now()).unwrap();
    assert_eq!(br.try_recv().unwrap().id, wire::TEXT);
    assert!(cr.try_recv().is_err());
    let ch = d.worlds.get_mut(&-2).unwrap().leave(a).unwrap();
    let (packets, mut new_rx) = mpsc::channel(128);
    let (lost, _) = watch::channel(false);
    let new = d
        .worlds
        .get_mut(&-3)
        .unwrap()
        .enter(ch, Some(Output { packets, lost }))
        .unwrap();
    while new_rx.try_recv().is_ok() {}
    let view = d.player_view(new).unwrap();
    assert_ne!(
        (old.world, old.player),
        (view.session.world, view.session.player)
    );
    assert_eq!(old.character, view.session.character);
    assert!(d.player_view(old.player).is_err());
    assert!(new_rx.try_recv().is_err());
    let packet = ar.try_recv().unwrap();
    let mut r = Reader::new(&packet.body);
    r.utf().unwrap();
    assert_eq!(r.i32().unwrap(), old.player);
    r.i32().unwrap();
    r.u8().unwrap();
    r.utf().unwrap();
    assert_eq!(r.utf().unwrap(), "Ready.");
    assert_eq!(r.utf().unwrap(), "");
    r.done().unwrap();
}
#[test]
fn view_reports_busy_quarantine_lost_closed_and_detached_without_fabricating_success() {
    let mut d = super::tests::directory();
    let (a, _ar) = receive(&mut d, -2, 1, WIZARD);
    let p = d.worlds.get_mut(&-2).unwrap().players.get_mut(&a).unwrap();
    p.pending = true;
    p.persistence_failure = Some("database details stay in logs".into());
    for _ in 0..129 {
        send(p, wire::TEXT, vec![]);
    }
    let view = d.player_view(a).unwrap();
    assert!(view.lifecycle.busy && view.lifecycle.quarantined);
    assert_eq!(view.character.inventory_revision, 0);
    let json = serde_json::to_value(view).unwrap();
    assert_eq!(json["lifecycle"]["output"], "lost");
    assert!(!json.to_string().contains("database details"));
    let p = d.worlds.get_mut(&-2).unwrap().players.get_mut(&a).unwrap();
    p.tx = None;
    assert_eq!(
        serde_json::to_value(d.player_view(a).unwrap()).unwrap()["lifecycle"]["output"],
        "detached"
    );
    d.worlds.get_mut(&-2).unwrap().players.remove(&a);
    assert!(d.player_view(a).is_err());
}
