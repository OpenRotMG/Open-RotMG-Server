use super::tests::{ch, world};
use super::*;

fn native() -> (World, i32, mpsc::Receiver<Packet>, watch::Receiver<bool>) {
    let mut w = world();
    let (packets, rx) = mpsc::channel(4096);
    let (lost, loss) = watch::channel(false);
    let id = w.enter(ch(&w), Some(Output { packets, lost })).unwrap();
    (w, id, rx, loss)
}
fn offer(w: &mut World, id: i32, owner: Option<i32>, bullet: u8, damage: i32) -> i32 {
    let pos = w.players[&id].pos;
    let owner = owner.unwrap_or_else(|| w.spawn_entity(0x804, pos, vec![]).unwrap());
    w.players.get_mut(&id).unwrap().known.insert(owner);
    let spec = w.content.get(0x804).unwrap().projectiles[&0].clone();
    w.bullets.push(Bullet {
        owner,
        source_kind: 0x804,
        id: bullet,
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
    body.u8(bullet)
        .i32(owner)
        .u8(0)
        .f32(pos.x)
        .f32(pos.y)
        .f32(0.)
        .i16(damage as i16);
    w.send_enemy_shot(owner, body.0);
    owner
}

#[test]
fn native_miss_takes_no_independent_damage_and_report_commits_exactly_once_without_echo() {
    let (mut w, id, mut rx, _) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    w.acknowledge_shoot(id, 100).unwrap();
    let hp = w.players[&id].character.hp;
    for _ in 0..2 {
        w.step();
    }
    assert_eq!(w.players[&id].character.hp, hp);
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 20);
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 20);
    while let Ok(packet) = rx.try_recv() {
        assert_ne!(packet.id, 47);
    }
}

#[test]
fn report_requires_actual_offer_and_ack_but_not_a_living_owner_or_server_bullet() {
    let (mut w, id, _rx, _loss) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    let hp = w.players[&id].character.hp;
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp);
    w.acknowledge_shoot(id, 100).unwrap();
    w.bullets.clear();
    w.remove_entity(owner);
    w.now = 4000;
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 20);
}

#[test]
fn expired_and_unknown_reports_are_observable_denials_not_disconnects_or_damage() {
    let (mut w, id, _rx, loss) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    w.acknowledge_shoot(id, 100).unwrap();
    let hp = w.players[&id].character.hp;
    w.now = 10000;
    w.expire_native_projectiles();
    w.native_player_hit(id, owner, 3).unwrap();
    w.native_player_hit(id, owner, 99).unwrap();
    assert_eq!(w.players[&id].character.hp, hp);
    assert!(!*loss.borrow());
}

#[test]
fn fifo_ack_separates_reused_bullet_ids_and_owner_nova_acknowledgements() {
    let (mut w, id, _rx, _loss) = native();
    assert!(native_hits::expect_shoot_ack(
        w.players.get_mut(&id).unwrap(),
        vec![],
        &w.clock
    ));
    let owner = offer(&mut w, id, None, 3, 20);
    let hp = w.players[&id].character.hp;
    w.acknowledge_shoot(id, 90).unwrap();
    assert!(w.players[&id].visible_projectiles.is_empty());
    w.acknowledge_shoot(id, 100).unwrap();
    w.native_player_hit(id, owner, 3).unwrap();
    w.bullets.clear();
    w.now += 50;
    offer(&mut w, id, Some(owner), 3, 25);
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 20);
    w.acknowledge_shoot(id, 200).unwrap();
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 45);
    assert!(w.acknowledge_shoot(id, 300).is_err());
}

#[test]
fn rejected_creation_ack_never_authorizes_a_hit() {
    let (mut w, id, _rx, _loss) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    w.acknowledge_shoot(id, -1).unwrap();
    let hp = w.players[&id].character.hp;
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp);
}

#[test]
fn reported_fatal_hit_uses_existing_permanent_death_state_and_conditions_engine() {
    let (mut w, id, _rx, _loss) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    w.acknowledge_shoot(id, 100).unwrap();
    w.players.get_mut(&id).unwrap().character.hp = 20;
    w.native_player_hit(id, owner, 3).unwrap();
    assert!(w.players[&id].death_pending);
    assert_eq!(w.players[&id].character.hp, 0);
    assert_eq!(w.players[&id].killer, "East Tutorial Gun");
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, 0);
}

#[test]
fn native_shoot_ack_bounds_both_empty_entries_and_wide_snapshot_vectors() {
    let limit = native_hits::MAX_PENDING_SHOOT_UNITS;
    let (mut w, id, _rx, loss) = native();
    let p = w.players.get_mut(&id).unwrap();
    for _ in 0..limit {
        assert!(native_hits::expect_shoot_ack(p, vec![], &w.clock));
    }
    assert!(!native_hits::expect_shoot_ack(p, vec![], &w.clock));
    assert!(*loss.borrow());
    assert_eq!(p.shoot_acks.len(), limit);

    let (mut w, id, _rx, loss) = native();
    offer(&mut w, id, None, 3, 20);
    let shot = w.bullets.last().unwrap().clone();
    let p = w.players.get_mut(&id).unwrap();
    p.shoot_acks.clear();
    assert!(native_hits::expect_shoot_ack(
        p,
        vec![shot; limit - 1],
        &w.clock
    ));
    assert!(!*loss.borrow());
    assert!(!native_hits::expect_shoot_ack(p, vec![], &w.clock));
    assert!(*loss.borrow());
    assert_eq!(p.shoot_acks.len(), 1);
}

#[test]
fn dense_native_backlog_drains_and_retains_exactly_once_hit_authority() {
    let (mut w, id, _rx, loss) = native();
    offer(&mut w, id, None, 3, 20);
    let shot = w.bullets.last().unwrap().clone();
    w.players.get_mut(&id).unwrap().shoot_acks.clear();
    for _ in 0..450 {
        assert!(native_hits::expect_shoot_ack(
            w.players.get_mut(&id).unwrap(),
            vec![shot.clone(); 3],
            &w.clock
        ));
    }
    assert!(!*loss.borrow());
    let hp = w.players[&id].character.hp;
    w.native_player_hit(id, shot.owner, shot.id).unwrap();
    assert_eq!(w.players[&id].character.hp, hp);
    for _ in 0..450 {
        w.acknowledge_shoot(id, 100).unwrap();
    }
    assert!(w.players[&id].shoot_acks.is_empty());
    assert!(w.acknowledge_shoot(id, 100).is_err());
    w.native_player_hit(id, shot.owner, shot.id).unwrap();
    w.native_player_hit(id, shot.owner, shot.id).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 20);
    assert!(!*loss.borrow());
}

#[test]
fn native_shoot_ack_survives_repeated_global_pauses_without_world_ticks() {
    let (mut w, id, _rx, loss) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    let sent = w.players[&id].shoot_acks.front().unwrap().0;
    let simulation = (w.now, w.tick);
    for bullet in [4, 5] {
        w.clock.set_paused(true);
        std::thread::sleep(std::time::Duration::from_secs(3));
        w.expire_native_projectiles();
        assert!(!*loss.borrow(), "a global pause consumed the SHOOT budget");
        assert!(!w.players[&id].departing);
        w.clock.set_paused(false);
        assert_eq!(w.players[&id].shoot_acks.front().unwrap().0, sent);
        offer(&mut w, id, Some(owner), bullet, 20);
    }
    w.expire_native_projectiles();
    assert!(!*loss.borrow());
    assert_eq!(w.players[&id].shoot_acks.len(), 3);
    assert_eq!((w.now, w.tick), simulation);
    for _ in 0..3 {
        w.acknowledge_shoot(id, 100).unwrap();
    }
    assert!(w.players[&id].shoot_acks.is_empty());
    let hp = w.players[&id].character.hp;
    w.native_player_hit(id, owner, 3).unwrap();
    w.native_player_hit(id, owner, 3).unwrap();
    assert_eq!(w.players[&id].character.hp, hp - 20);
}

#[test]
fn native_shoot_ack_counts_unpaused_stalls_across_pauses_without_world_ticks() {
    let (mut w, id, _rx, loss) = native();
    let owner = offer(&mut w, id, None, 3, 20);
    offer(&mut w, id, Some(owner), 4, 20);
    let sent = w.players[&id].shoot_acks.front().unwrap().0;
    let simulation = (w.now, w.tick);
    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_secs(3));
        w.clock.set_paused(true);
        std::thread::sleep(std::time::Duration::from_secs(3));
        w.clock.set_paused(false);
        assert_eq!(w.players[&id].shoot_acks.front().unwrap().0, sent);
        assert_eq!(w.players[&id].shoot_acks.len(), 2);
    }
    assert_eq!((w.now, w.tick), simulation);
    let error = w.acknowledge_shoot(id, 100).unwrap_err();
    assert_eq!(error.to_string(), "SHOOT acknowledgement timed out");
    assert!(!*loss.borrow());
    w.expire_native_projectiles();
    assert!(*loss.borrow());
    assert!(w.players[&id].departing);
    assert_eq!(w.players[&id].shoot_acks.len(), 1);
}

#[test]
fn native_shoot_ack_bug_context_labels_and_freezes_pause_excluding_age() {
    let mut d = super::tests::directory();
    d.clock.set_paused(true);
    std::thread::sleep(std::time::Duration::from_millis(25));
    d.clock.set_paused(false);
    let w = d.worlds.get_mut(&-1).unwrap();
    let (packets, _rx) = mpsc::channel(4096);
    let (lost, _loss) = watch::channel(false);
    let id = w.enter(ch(w), Some(Output { packets, lost })).unwrap();
    offer(w, id, None, 3, 20);
    d.clock.set_paused(true);
    let before = d.bug_context(id).unwrap()["native_acknowledgements"].clone();
    std::thread::sleep(std::time::Duration::from_millis(25));
    let after = d.bug_context(id).unwrap()["native_acknowledgements"].clone();
    assert!(before["oldest_shoot_pause_excluding_ms"].is_u64());
    assert!(before.get("oldest_shoot_ms").is_none());
    assert_eq!(
        before["oldest_shoot_pause_excluding_ms"],
        after["oldest_shoot_pause_excluding_ms"]
    );
    d.clock.set_paused(false);
}

#[test]
fn native_shoot_ack_five_second_timeout_remains_fail_closed() {
    let (mut w, id, _rx, loss) = native();
    offer(&mut w, id, None, 3, 20);
    w.players
        .get_mut(&id)
        .unwrap()
        .shoot_acks
        .front_mut()
        .unwrap()
        .0 = w.clock.now() - std::time::Duration::from_secs(6);
    assert!(!*loss.borrow());
    w.expire_native_projectiles();
    assert!(*loss.borrow());
    assert!(w.players[&id].departing);
}
