use super::*;
use crate::dungeon::spider::{self, BOSS, EGG, HATCHLING};

fn arena() -> World {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(spider::generate(&content, 12351).unwrap());
    let mut w = World::new(content, map).unwrap();
    w.name = "Spider Den".into();
    w.encounter = true;
    w
}
fn fighter(w: &mut World, account: i32, pos: Pos) -> i32 {
    let mut ch = tests::ch(w);
    ch.id = account;
    ch.account = account;
    let id = w.enter(ch, None).unwrap();
    w.players.get_mut(&id).unwrap().pos = pos;
    id
}

#[test]
fn spider_layout_is_seeded_connected_and_uses_selected_roster_and_assets() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let first = serde_json::to_vec(&spider::generate(&content, 1).unwrap()).unwrap();
    assert_eq!(
        first,
        serde_json::to_vec(&spider::generate(&content, 1).unwrap()).unwrap()
    );
    assert_ne!(
        first,
        serde_json::to_vec(&spider::generate(&content, 2).unwrap()).unwrap()
    );
    for seed in 0..8 {
        let map = spider::generate(&content, seed).unwrap();
        let reached = crate::dungeon::tests::reachable(&map, &content);
        let enemies: Vec<_> = map
            .cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object.is_some_and(|k| content.objects[&k].enemy))
            .collect();
        assert_eq!(
            enemies
                .iter()
                .filter(|(_, c)| c.object == Some(BOSS))
                .count(),
            1
        );
        assert!(enemies.len() >= 26);
        assert!(enemies.iter().all(|(i, _)| reached.contains(i)));
        for kind in spider::ROSTER {
            crate::encounter::validate(&content, kind).unwrap();
            crate::loot::validate(&content, kind).unwrap();
        }
    }
    assert_eq!(
        (content.objects[&BOSS].hp, content.objects[&BOSS].defense),
        (2700, 4)
    );
    assert_eq!(crate::dungeon::portal_drop(0x62c), Some((0x719, 0.1)));
    assert_eq!(crate::dungeon::portal_drop(0x637), Some((0x719, 0.1)));
}

#[test]
fn arachna_shields_then_builds_one_owned_web_and_fires_selected_patterns() {
    let mut w = arena();
    let boss = w.entities.values().find(|e| e.kind == BOSS).unwrap().id;
    let home = w.entities[&boss].pos;
    fighter(
        &mut w,
        1,
        Pos {
            x: home.x - 8.,
            y: home.y,
        },
    );
    assert_ne!(
        w.entities[&boss].condition & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    w.now = 100;
    w.npc_step();
    assert!(!w.entities.values().any(|e| e.parent == Some(boss)));
    w.now = 1550;
    w.npc_step();
    assert!(!w.entities.values().any(|e| e.parent == Some(boss)));
    w.now = 1600;
    w.npc_step();
    let spokes: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.parent == Some(boss))
        .map(|e| e.id)
        .collect();
    assert_eq!(spokes.len(), 9);
    let protections = crate::condition::bit(crate::condition::INVULNERABLE)
        | crate::condition::bit(crate::condition::STASIS_IMMUNE);
    assert!(
        spokes
            .iter()
            .all(|id| w.entities[id].conditions(w.now) & protections == protections)
    );
    let mut source_timer = 0;
    let mut source_shots = Vec::new();
    for now in (0..1000).step_by(50) {
        if source_timer <= 0 {
            source_shots.push(now);
            source_timer = 5;
        } else {
            source_timer -= 50;
        }
    }
    assert!(
        source_shots
            .windows(2)
            .all(|t| t[1] - t[0] == spider::profile(0x937).interval)
    );
    for id in &spokes {
        let shots = crate::encounter::phased_attacks(w.entities[id].kind, 100000, 0., 0);
        let expected: [[f32; 3]; 9] = [
            [120., 180., 240.],
            [180., 240., 300.],
            [240., 300., 360.],
            [300., 360., 420.],
            [0., 60., 120.],
            [60., 120., 180.],
            [120., 180., 240.],
            [240., 300., 360.],
            [0., 60., 120.],
        ];
        for ((_, angle), degrees) in shots
            .iter()
            .zip(expected[(w.entities[id].kind - 0x937) as usize])
        {
            let radians = degrees.to_radians();
            assert!((angle.cos() - radians.cos()).abs() < 0.00001);
            assert!((angle.sin() - radians.sin()).abs() < 0.00001);
        }
        let notices: Vec<_> = shots
            .iter()
            .enumerate()
            .map(|(i, (index, angle))| enemy_shots::EnemyShot {
                bullet: i as u8,
                projectile: *index,
                start: w.entities[id].pos,
                angle: *angle,
                damage: 0,
            })
            .collect();
        let packets = enemy_shots::packets(*id, &notices);
        assert_eq!(packets.len(), 1, "web fan must use one native ACK");
        assert_eq!(packets[0][20], 3);
    }
    assert_eq!(w.entities[&boss].pos.distance(home), 0.);
    w.now = 2050;
    w.npc_step();
    assert!(!w.bullets.iter().any(|b| b.owner == boss));
    w.now = 2100;
    w.npc_step();
    assert_eq!(
        w.entities[&boss].condition & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    let shots: Vec<_> = w.bullets.iter().filter(|b| b.owner == boss).collect();
    assert_eq!(shots.len(), 14);
    assert_eq!(shots.iter().filter(|b| b.spec.min == 75).count(), 1);
    assert_eq!(
        w.entities
            .values()
            .filter(|e| e.parent == Some(boss))
            .count(),
        9
    );
    w.remove_entity(boss);
    w.mechanic_expiry();
    assert!(spokes.iter().all(|id| !w.entities.contains_key(id)));
}

#[test]
fn eggs_hatch_once_on_contact_and_hatchlings_survive_the_removed_egg() {
    let mut w = arena();
    let egg = w.entities.values().find(|e| e.kind == EGG).unwrap().id;
    let pos = w.entities[&egg].pos;
    fighter(&mut w, 1, pos);
    let before = w.entities.values().filter(|e| e.kind == HATCHLING).count();
    w.now = 100;
    w.spider_mechanics(egg, pos, 0).unwrap();
    assert!(!w.entities.contains_key(&egg));
    let after = w.entities.values().filter(|e| e.kind == HATCHLING).count();
    assert!((2..=7).contains(&(after - before)));
    w.mechanic_expiry();
    assert_eq!(
        w.entities.values().filter(|e| e.kind == HATCHLING).count(),
        after
    );
}

#[test]
fn charging_spiders_lock_the_destination_and_respect_paralysis_and_collision() {
    let mut w = arena();
    let pos = Pos { x: 12.5, y: 74.5 };
    let id = w.spawn_entity(0x21d, pos, vec![]).unwrap();
    let target = Pos { x: 17.5, y: 74.5 };
    let profile = spider::profile(0x21d);
    let motion = profile.motion;
    w.now = 100;
    assert_eq!(
        w.charge_goal(id, Some(target), motion, profile.speed)
            .unwrap()
            .distance(target),
        0.
    );
    let moved_target = Pos { x: 12.5, y: 79.5 };
    w.now = 150;
    assert_eq!(
        w.charge_goal(id, Some(moved_target), motion, profile.speed)
            .unwrap()
            .distance(target),
        0.
    );
    let player = fighter(&mut w, 1, target);
    w.apply_condition(id, crate::condition::PARALYZED, 1000)
        .unwrap();
    w.npc_step();
    assert_eq!(w.entities[&id].pos.distance(pos), 0.);
    for now in (200..=1150).step_by(50) {
        w.now = now;
        w.npc_step();
    }
    assert!(w.entities[&id].pos.x > pos.x);
    assert!(w.move_entity(id, Pos { x: 0.5, y: 74.5 }, 30.).is_err());
    assert!(w.players.contains_key(&player));
}

#[test]
fn native_web_hits_slow_without_damage_and_queens_blob_applies_weak() {
    for (kind, index, effect, damage) in [
        (0x937, 0, crate::condition::SLOWED, 0),
        (BOSS, 1, crate::condition::WEAK, 75),
    ] {
        let mut w = arena();
        let (packets, _rx) = mpsc::channel(4096);
        let (lost, _loss) = watch::channel(false);
        let id = w
            .enter(tests::ch(&w), Some(Output { packets, lost }))
            .unwrap();
        let pos = w.players[&id].pos;
        let owner = w.spawn_entity(kind, pos, vec![]).unwrap();
        w.players.get_mut(&id).unwrap().known.insert(owner);
        let spec = w.content.objects[&kind].projectiles[&index].clone();
        w.bullets.push(Bullet {
            owner,
            source_kind: kind,
            id: 7,
            friendly: false,
            start: pos,
            pos,
            angle: 0.,
            born: w.now,
            damage,
            spec,
            hit: BTreeSet::new(),
        });
        let mut body = Writer::new();
        body.u8(7)
            .i32(owner)
            .u8(index)
            .f32(pos.x)
            .f32(pos.y)
            .f32(0.)
            .i16(damage as i16);
        w.send_enemy_shot(owner, body.0);
        w.acknowledge_shoot(id, 100).unwrap();
        let hp = w.players[&id].character.hp;
        w.native_player_hit(id, owner, 7).unwrap();
        assert_eq!(w.players[&id].character.hp, hp - damage);
        assert_ne!(
            w.players[&id].conditions(w.now) & crate::condition::bit(effect),
            0
        );
        w.native_player_hit(id, owner, 7).unwrap();
        assert_eq!(w.players[&id].character.hp, hp - damage);
        w.now += if effect == crate::condition::SLOWED {
            3001
        } else {
            5001
        };
        assert_eq!(
            w.players[&id].conditions(w.now) & crate::condition::bit(effect),
            0
        );
    }
}

#[tokio::test]
async fn participating_players_get_private_ichor_and_clear_is_published_only_after_storage() {
    let mut w = arena();
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
    let account = store.list("spider-clear-fixture".into()).await.unwrap().id;
    let ch = store
        .create(account, w.content.get(0x030e).unwrap())
        .await
        .unwrap();
    let id = w.enter(ch, None).unwrap();
    let boss = w.entities.values().find(|e| e.kind == BOSS).unwrap().id;
    let pos = w.entities[&boss].pos;
    w.players.get_mut(&id).unwrap().pos = pos;
    let other = fighter(
        &mut w,
        2,
        Pos {
            x: pos.x - 1.,
            y: pos.y,
        },
    );
    for player in [id, other] {
        w.entities
            .get_mut(&boss)
            .unwrap()
            .damage
            .insert(w.players[&player].character.id, 1);
    }
    w.defeat_entity(boss).unwrap();
    assert!(w.cleared);
    let exit = w.clear_exit.unwrap();
    assert!(!w.entities[&exit].published);
    for player in [id, other] {
        let owner = w.players[&player].character.account;
        let bags: Vec<_> = w
            .entities
            .values()
            .filter(|e| e.items.contains(&0x707) && e.extra_stats.contains(&(54, owner)))
            .collect();
        assert_eq!(bags.len(), 1);
        assert!(!bags[0].accessible(if owner == 1 { 2 } else { 1 }));
    }
    let snapshots = w.pending_progress();
    let snapshot = snapshots.iter().find(|(p, _)| *p == id).unwrap().1.clone();
    assert_eq!(snapshot.spider_dens, 1);
    assert_eq!(w.players[&id].character.spider_dens, 0);
    store.progress(snapshot.clone()).await.unwrap();
    w.finish_progress(id, snapshot).unwrap();
    assert!(w.entities[&exit].published);
    assert_eq!(
        store
            .load(account, w.players[&id].character.id)
            .await
            .unwrap()
            .spider_dens,
        1
    );
    assert!(w.pending_progress().is_empty());
    let mut historical = serde_json::to_value(&w.players[&id].character).unwrap();
    historical.as_object_mut().unwrap().remove("spider_dens");
    let historical: Character = serde_json::from_value(historical).unwrap();
    assert_eq!(historical.spider_dens, 0);
}
