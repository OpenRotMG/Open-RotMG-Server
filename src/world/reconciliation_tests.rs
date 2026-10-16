use super::reconciliation::{NativeInput, goto};
use super::tests::{ch, world};
use super::*;
use std::time::Duration;

fn native() -> (World, i32, mpsc::Receiver<Packet>, watch::Receiver<bool>) {
    let mut w = world();
    let (packets, rx) = mpsc::channel(4096);
    let (lost, loss) = watch::channel(false);
    let id = w.enter(ch(&w), Some(Output { packets, lost })).unwrap();
    w.attack_epoch = Instant::now() - Duration::from_secs(2);
    let p = w.players.get_mut(&id).unwrap();
    p.last_move = 1000;
    p.last_move_at = 0;
    p.movement_credit = 0.;
    (w, id, rx, loss)
}

#[test]
fn weapon_swap_accepts_only_equipped_or_unacknowledged_native_weapons() {
    for correcting in [false, true] {
        let (mut w, id, _rx, _loss) = native();
        w.safe = true;
        let p = w.players.get_mut(&id).unwrap();
        p.character.base[5] = 75;
        p.character.items[4] = 0xa98;
        let start = Pos {
            x: p.pos.x + 0.3,
            y: p.pos.y,
        };
        let angle = -w.content.get(0xa97).unwrap().arc.to_radians() / 2.;
        w.step();
        w.step();
        w.acknowledge_tick(id, 0).unwrap();
        let plan = w
            .swap(
                id,
                Slot {
                    object: id,
                    slot: 4,
                    kind: 0xa98,
                },
                Slot {
                    object: id,
                    slot: 0,
                    kind: 0xa97,
                },
            )
            .unwrap();
        w.finish_inventory(plan, Ok(1)).unwrap();
        if correcting {
            w.correct_position(id, 1000, "fixture correction").unwrap();
        }
        let received = w.attack_epoch + Duration::from_millis(2500);
        for time in [2000, 2200] {
            let bullet = w.players[&id].next_bullet;
            assert_eq!(
                w.native_shoot_received(id, (time, received), bullet, 0xa97, start, angle)
                    .unwrap(),
                if correcting {
                    NativeInput::DiscardedUntilGotoAck
                } else {
                    NativeInput::Applied
                }
            );
            w.step();
            w.step();
        }
        let bullet = w.players[&id].next_bullet;
        for kind in [0xa99, 0xa2e] {
            assert!(
                w.native_shoot_received(id, (2400, received), bullet, kind, start, angle)
                    .is_err()
            );
        }
        assert!(w.shoot(id, 2400, bullet, 0xa97, start, angle).is_err());
        w.acknowledge_tick(id, 0).unwrap();
        w.native_shoot_received(id, (2400, received), bullet, 0xa97, start, angle)
            .unwrap();
        w.acknowledge_tick(id, 1).unwrap();
        let p = &w.players[&id];
        let (bullet, seed) = (p.next_bullet, p.seed);
        assert!(
            w.native_shoot_received(id, (2600, received), bullet, 0xa97, start, angle)
                .unwrap_err()
                .to_string()
                .contains("weapon does not match")
        );
        assert_eq!(
            (w.players[&id].next_bullet, w.players[&id].seed),
            (bullet, seed)
        );
        if correcting {
            w.acknowledge_goto(id, 2500).unwrap();
        }
        let angle = -w.content.get(0xa98).unwrap().arc.to_radians() / 2.;
        assert_eq!(
            w.native_shoot_received(id, (2600, received), bullet, 0xa98, start, angle)
                .unwrap(),
            NativeInput::Applied
        );
        assert!(w.acknowledge_tick(id, w.players[&id].tick).is_err());
    }
}

#[test]
fn unacknowledged_weapon_ticks_are_bounded_and_do_not_disconnect_bots() {
    for controller in [Controller::Human, Controller::Autonomous] {
        let (mut w, id, _rx, loss) = native();
        let p = w.players.get_mut(&id).unwrap();
        p.controller = controller;
        for _ in 0..100 {
            p.remember_weapon_tick();
        }
        assert_eq!(*loss.borrow(), controller == Controller::Human);
        assert!(p.weapon_ticks.len() <= 64);
    }
}

#[test]
fn restrictive_native_condition_queues_real_stat29_before_one_owned_goto() {
    let (mut w, id, mut rx, loss) = native();
    w.safe = true;
    while rx.try_recv().is_ok() {}
    w.apply_condition(id, crate::condition::SLOWED, 2000)
        .unwrap();
    let authority = {
        let p = &w.players[&id];
        (
            p.seed,
            p.next_bullet,
            p.attack_credit,
            p.movement_credit,
            p.character.shots,
            p.last_move,
        )
    };
    w.step();
    w.step();
    let mut packets = vec![];
    while let Ok(packet) = rx.try_recv() {
        packets.push(packet);
    }
    assert_eq!(packets.iter().filter(|p| p.id == wire::GOTO).count(), 1);
    let tick = packets.iter().position(|p| p.id == wire::NEW_TICK).unwrap();
    let go = packets.iter().position(|p| p.id == wire::GOTO).unwrap();
    assert!(tick < go);
    let mut r = crate::wire::Reader::new(&packets[tick].body);
    r.i32().unwrap();
    assert_eq!(r.i32().unwrap(), 100);
    let count = r.i16().unwrap();
    let mut mask = None;
    for _ in 0..count {
        let entity = r.i32().unwrap();
        r.f32().unwrap();
        r.f32().unwrap();
        let stats = r.i16().unwrap();
        for _ in 0..stats {
            let kind = r.u8().unwrap();
            if matches!(kind, 31 | 62) {
                r.utf().unwrap();
            } else {
                let value = r.i32().unwrap();
                if entity == id && kind == 29 {
                    mask = Some(value);
                }
            }
        }
    }
    r.done().unwrap();
    assert_eq!(
        mask.unwrap() & crate::condition::bit(crate::condition::SLOWED),
        crate::condition::bit(crate::condition::SLOWED)
    );
    let p = &w.players[&id];
    assert_eq!(
        (
            p.seed,
            p.next_bullet,
            p.attack_credit,
            p.movement_credit,
            p.character.shots,
            p.last_move
        ),
        authority
    );
    assert!(p.correcting().unwrap());
    assert!(!*loss.borrow());
    w.acknowledge_goto(id, 1100).unwrap();
    w.apply_condition(id, crate::condition::SLOWED, 2000)
        .unwrap();
    w.step();
    w.step();
    while let Ok(packet) = rx.try_recv() {
        assert_ne!(packet.id, wire::GOTO, "refresh must not reanchor again");
    }
    w.now = 2500;
    w.step();
    w.step();
    while let Ok(packet) = rx.try_recv() {
        assert_ne!(
            packet.id,
            wire::GOTO,
            "expiry only raises the admitted movement coefficient"
        );
    }
    w.players
        .get_mut(&id)
        .unwrap()
        .statuses
        .insert(crate::condition::PARALYZED, (w.now, w.now + 1000));
    w.step();
    w.step();
    let mut gotos = 0;
    while let Ok(packet) = rx.try_recv() {
        gotos += usize::from(packet.id == wire::GOTO);
    }
    assert_eq!(gotos, 1);
    w.acknowledge_goto(id, 1200).unwrap();
    w.apply_condition(id, crate::condition::SLOWED, 2000)
        .unwrap();
    w.step();
    w.step();
    while let Ok(packet) = rx.try_recv() {
        assert_ne!(
            packet.id,
            wire::GOTO,
            "Slowed cannot lower an already Paralyzed motor"
        );
    }
}
#[test]
fn speedy_expiry_synchronizes_the_lower_native_coefficient_after_real_stat29() {
    let (mut w, id, mut rx, _loss) = native();
    w.safe = true;
    while rx.try_recv().is_ok() {}
    w.apply_condition(id, 15, 200).unwrap();
    w.step();
    w.step();
    while let Ok(p) = rx.try_recv() {
        assert_ne!(p.id, wire::GOTO, "Speedy only raises the coefficient");
    }
    w.step();
    w.step();
    let mut ids = vec![];
    while let Ok(p) = rx.try_recv() {
        ids.push(p.id);
    }
    let tick = ids.iter().position(|id| *id == wire::NEW_TICK).unwrap();
    let go = ids.iter().position(|id| *id == wire::GOTO).unwrap();
    assert!(tick < go);
    assert_eq!(ids.iter().filter(|id| **id == wire::GOTO).count(), 1);
    assert!(w.players[&id].correcting().unwrap());
}
#[test]
fn native_dazed_and_goto_discards_conserve_rng_without_authorizing_bullets() {
    let (mut w, id, _rx, _loss) = native();
    let weapon = w.players[&id].character.items[0] as u16;
    let item = w.content.get(weapon).unwrap().clone();
    assert!(item.projectiles[&0].min < item.projectiles[&0].max);
    let rate =
        0.0015 + 0.0065 * w.players[&id].character.effective(&w.content).unwrap()[5] as f64 / 75.;
    let normal = (1. / rate / item.rate as f64).floor() as i32;
    let start = Pos {
        x: w.players[&id].pos.x + 0.3,
        y: w.players[&id].pos.y,
    };
    let seed = w.players[&id].seed;
    let mut expected = seed;
    let mut next = 1;
    w.apply_condition(id, crate::condition::DAZED, 2000)
        .unwrap();
    for (time, discard) in [
        (2100, false),
        (2100 + normal, false),
        (2100 + 2 * normal, true),
    ] {
        if discard {
            w.correct_position(id, time, "prepared RNG conservation")
                .unwrap();
        }
        let before = w.players[&id].character.shots;
        for _ in 0..item.shots {
            w.native_shoot_received(
                id,
                (time, w.attack_epoch + Duration::from_millis(time as u64)),
                next,
                weapon,
                start,
                0.,
            )
            .unwrap();
            expected = ((expected as u64 * 16807) % 2147483647) as u32;
            next = (next + 1) % 128;
        }
        assert_eq!(w.players[&id].seed, expected);
        if time != 2100 {
            assert_eq!(w.players[&id].character.shots, before);
        }
    }
    assert_eq!(w.bullets.len(), item.shots as usize);
    w.acknowledge_goto(id, 3000).unwrap();
    w.now = 3000;
    w.native_shoot_received(
        id,
        (4000, w.attack_epoch + Duration::from_millis(4000)),
        next,
        weapon,
        start,
        0.,
    )
    .unwrap();
    expected = ((expected as u64 * 16807) % 2147483647) as u32;
    assert_eq!(w.players[&id].seed, expected);
    assert_eq!(w.bullets.len(), item.shots as usize + 1);
}
#[test]
fn batched_native_volleys_preserve_ids_rng_peer_shots_and_position() {
    let (mut w, id, mut rx, _loss) = native();
    let mut peer_character = ch(&w);
    peer_character.account = 2;
    peer_character.id = 2;
    let peer = w.enter(peer_character, None).unwrap();
    let (packets, mut peer_rx) = mpsc::channel(64);
    let (lost, _peer_loss) = watch::channel(false);
    w.players.get_mut(&peer).unwrap().tx = Some(Output { packets, lost });
    w.players.get_mut(&peer).unwrap().known.insert(id);
    while rx.try_recv().is_ok() {}
    let p = w.players.get_mut(&id).unwrap();
    p.character.base[5] = 75;
    p.last_attack = 1000;
    p.last_attack_at = 1000;
    p.attack_credit = 0.;
    let pos = p.pos;
    let mut seed = p.seed;
    let start = Pos {
        x: pos.x + 0.3,
        y: pos.y,
    };
    let item = w.content.get(0xa97).unwrap().clone();
    assert_eq!(item.shots, 2);
    let received = w.attack_epoch + Duration::from_millis(1069);
    for time in [1125, 1250, 1375] {
        for index in 0..item.shots {
            let bullet = w.players[&id].next_bullet;
            let angle = (index as f32 - 0.5) * item.arc.to_radians();
            assert_eq!(
                w.native_shoot_received(id, (time, received), bullet, 0xa97, start, angle)
                    .unwrap(),
                NativeInput::Applied
            );
            seed = ((seed as u64 * 16807) % 2147483647) as u32;
            assert_eq!(w.players[&id].seed, seed);
            assert_eq!(peer_rx.try_recv().unwrap().id, 74);
        }
    }
    let p = &w.players[&id];
    assert_eq!((p.character.shots, p.next_bullet), (6, 7));
    assert_eq!(p.pos.distance(pos), 0.);
    assert_eq!(p.movement_credit, 0.);
    assert_eq!(p.last_attack_at, 1069);
    assert!(p.goto_acks.is_empty());
    assert!(rx.try_recv().is_err());
    assert_eq!(w.bullets.len(), 6);
}

#[test]
fn predicted_origin_failure_is_typed_and_correction_never_authorizes_the_shot() {
    let (mut w, id, _rx, _loss) = native();
    let pos = w.players[&id].pos;
    let received = w.attack_epoch + Duration::from_millis(100);
    let error = w
        .native_shoot_received(id, (1100, received), 1, 0xa97, Pos { x: -2., y: pos.y }, 0.)
        .unwrap_err();
    assert!(error.downcast_ref::<RejectedNativeShot>().is_some());
    assert_eq!(w.players[&id].next_bullet, 1);
    assert_eq!(w.players[&id].attack_credit, 1000.);
    assert!(
        w.correct_native_shot(id, 1100, 99, 0xa97, "bad counter")
            .is_err()
    );
    assert!(w.players[&id].goto_acks.is_empty());
    w.correct_native_shot(id, 1100, 1, 0xa97, &format!("{error:#}"))
        .unwrap();
    assert_eq!(w.players[&id].next_bullet, 2);
    assert_eq!(w.players[&id].character.shots, 0);
    assert_eq!(w.players[&id].attack_credit, 1000.);
    assert_eq!(w.players[&id].movement_credit, 0.);
    assert_eq!(w.players[&id].pos.distance(pos), 0.);
    assert!(w.bullets.is_empty());
    w.acknowledge_goto(id, 1100).unwrap();
    assert!(
        w.correct_native_shot(id, 1200, 2, 0xa97, "too fast")
            .is_err()
    );
    assert!(w.players[&id].goto_acks.is_empty());
    assert_eq!(w.players[&id].next_bullet, 2);
    assert!(
        w.correct_native_shot(id, 1000, 2, 0xa97, "backwards")
            .is_err()
    );
    assert_eq!(
        w.native_shoot_received(id, (1100, received), 2, 0xa97, pos, 0.)
            .unwrap(),
        NativeInput::DiscardedUntilGotoAck
    );
    assert_eq!(
        w.native_shoot_received(
            id,
            (2100, w.attack_epoch + Duration::from_millis(500)),
            3,
            0xa97,
            Pos {
                x: pos.x + 0.3,
                y: pos.y
            },
            0.
        )
        .unwrap(),
        NativeInput::Applied
    );
    assert_eq!(w.players[&id].character.shots, 1);
    assert_eq!(w.bullets.len(), 1);
}

#[test]
fn receipt_burst_accepts_valid_client_cadence_without_borrowing_position_or_clock() {
    let (mut w, id, _rx, _loss) = native();
    w.players.get_mut(&id).unwrap().character.base[5] = 75;
    let item = w.content.get(0xa97).unwrap().clone();
    let pos = w.players[&id].pos;
    let start = Pos {
        x: pos.x + 0.3,
        y: pos.y,
    };
    let received = w.attack_epoch + Duration::from_millis(1000);
    for time in (1000..2500).step_by(125) {
        for index in 0..item.shots {
            let bullet = w.players[&id].next_bullet;
            let angle = item.arc.to_radians() * (index as f32 - 0.5);
            assert_eq!(
                w.native_shoot_received(id, (time, received), bullet, 0xa97, start, angle)
                    .unwrap(),
                NativeInput::Applied
            );
        }
    }
    let p = &w.players[&id];
    assert_eq!(p.character.shots, 24);
    assert_eq!(p.last_attack_at, 1000);
    assert_eq!(p.pos.distance(pos), 0.);
    assert_eq!(p.movement_credit, 0.);
    assert!(p.goto_acks.is_empty());
    assert_eq!(w.bullets.len(), 24);
}

#[test]
fn malformed_bullet_counter_cannot_spend_attack_credit_or_trigger_reconciliation() {
    let (mut w, id, _rx, _loss) = native();
    let p = &w.players[&id];
    let pos = p.pos;
    let (credit, seed) = (p.attack_credit, p.seed);
    let error = w
        .native_shoot_received(
            id,
            (1100, w.attack_epoch + Duration::from_millis(1000)),
            99,
            0xa97,
            Pos {
                x: pos.x + 0.3,
                y: pos.y,
            },
            0.,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unexpected player bullet counter")
    );
    assert!(error.downcast_ref::<RejectedNativeShot>().is_none());
    let p = &w.players[&id];
    assert_eq!(p.attack_credit, credit);
    assert_eq!(p.seed, seed);
    assert_eq!(p.last_attack, -1000);
    assert_eq!(p.last_attack_at, 0);
    assert_eq!(p.next_bullet, 1);
    assert_eq!(p.character.shots, 0);
    assert!(p.goto_acks.is_empty());
    assert!(w.bullets.is_empty());
}

#[test]
fn correction_discards_buffered_input_without_granting_authority() {
    let (mut w, id, _rx, _loss) = native();
    let pos = w.players[&id].pos;
    let received = w.attack_epoch + Duration::from_millis(100);
    let forged = Pos { x: -2., y: pos.y };
    let error = w
        .native_move_received(id, forged, (1100, received))
        .unwrap_err();
    assert!(error.to_string().contains("outside walkable terrain"));
    w.correct_position(id, 1100, &error.to_string()).unwrap();
    assert_eq!(w.players[&id].last_move, 1000);
    assert_eq!(
        w.native_move_received(id, forged, (1200, received))
            .unwrap(),
        NativeInput::DiscardedUntilGotoAck
    );
    assert_eq!(
        w.native_shoot_received(
            id,
            (1100, received),
            1,
            0xa97,
            Pos {
                x: pos.x + 100.,
                y: pos.y
            },
            0.
        )
        .unwrap(),
        NativeInput::DiscardedUntilGotoAck
    );
    for (time, bullet, kind, angle) in [
        (1000, 2, 0xa97, 0.),
        (1200, 99, 0xa97, 0.),
        (1200, 2, 0xa97, f32::NAN),
        (1200, 2, 0xa98, 0.),
    ] {
        assert!(
            w.native_shoot_received(id, (time, received), bullet, kind, pos, angle)
                .is_err()
        );
        assert_eq!(w.players[&id].next_bullet, 2);
    }
    let p = &w.players[&id];
    assert_eq!(p.pos.distance(pos), 0.);
    assert_eq!(p.movement_credit, 0.);
    assert_eq!(p.last_move_at, 0);
    assert_eq!(p.last_attack, -1000);
    assert_eq!(p.last_attack_at, 0);
    assert_eq!(p.attack_credit, 1000.);
    assert_eq!(p.attack_count, 0);
    assert_eq!(p.character.shots, 0);
    assert!(w.bullets.is_empty());
    assert_eq!(p.next_bullet, 2);
    w.acknowledge_goto(id, 1100).unwrap();
    assert_eq!(w.players[&id].movement_credit, 0.);
    assert_eq!(w.players[&id].last_move_at, 0);
    // An ACK cannot grant future processing-time funding.
    assert!(
        w.native_move_received(id, forged, (1200, received))
            .is_err()
    );
    w.native_move_received(
        id,
        Pos {
            x: pos.x + 0.4,
            y: pos.y,
        },
        (1200, received),
    )
    .unwrap();
    assert_eq!(w.players[&id].last_move_at, 100);
}

#[test]
fn a_discarded_volley_stays_discarded_across_ack_and_sequence_recovers() {
    let (mut w, id, _rx, _loss) = native();
    let pos = w.players[&id].pos;
    let received = w.attack_epoch + Duration::from_millis(500);
    w.correct_position(id, 1100, "fixture rejected movement")
        .unwrap();
    for bullet in 1..=2 {
        if bullet == 2 {
            w.acknowledge_goto(id, 1100).unwrap();
        }
        assert_eq!(
            w.native_shoot_received(id, (1100, received), bullet, 0xa97, pos, 0.)
                .unwrap(),
            NativeInput::DiscardedUntilGotoAck
        );
    }
    assert!(w.bullets.is_empty());
    assert!(
        w.native_shoot_received(id, (1100, received), 3, 0xa97, pos, 0.)
            .is_err()
    );
    assert_eq!(w.players[&id].next_bullet, 3);
    assert!(
        w.native_shoot_received(id, (2100, received), 3, 0xa97, Pos { x: -2., y: pos.y }, 0.)
            .is_err()
    );
    assert_eq!(w.players[&id].next_bullet, 3);
    assert_eq!(w.players[&id].character.shots, 0);
    assert_eq!(
        w.native_shoot_received(
            id,
            (2100, received),
            3,
            0xa97,
            Pos {
                x: pos.x + 0.3,
                y: pos.y
            },
            0.
        )
        .unwrap(),
        NativeInput::Applied
    );
    assert_eq!(w.players[&id].character.shots, 1);
    assert_eq!(w.players[&id].next_bullet, 4);
    assert_eq!(w.bullets.len(), 1);
}

#[test]
fn goto_ack_fifo_includes_other_players_and_rejects_unsolicited_or_bad_clocks() {
    let (mut w, id, _rx, _loss) = native();
    goto(w.players.get_mut(&id).unwrap(), false, vec![]);
    w.correct_position(id, 1100, "fixture correction").unwrap();
    assert!(w.acknowledge_goto(id, -1).is_err());
    assert_eq!(w.players[&id].goto_acks.len(), 2);
    w.acknowledge_goto(id, 1500).unwrap();
    assert_eq!(w.players[&id].last_move, 1000);
    assert!(w.players[&id].correcting().unwrap());
    assert!(w.acknowledge_goto(id, 999).is_err());
    assert_eq!(w.players[&id].goto_acks.len(), 1);
    w.acknowledge_goto(id, 1100).unwrap();
    assert!(!w.players[&id].correcting().unwrap());
    assert!(w.acknowledge_goto(id, 1100).is_err());
}

#[test]
fn goto_ack_backlog_and_real_time_timeout_fail_closed() {
    let (mut w, id, _rx, loss) = native();
    for _ in 0..64 {
        goto(w.players.get_mut(&id).unwrap(), false, vec![]);
    }
    assert!(!*loss.borrow());
    goto(w.players.get_mut(&id).unwrap(), false, vec![]);
    assert!(*loss.borrow());
    assert_eq!(w.players[&id].goto_acks.len(), 64);
    let (mut w, id, _rx, loss) = native();
    w.correct_position(id, 1100, "fixture correction").unwrap();
    w.players
        .get_mut(&id)
        .unwrap()
        .goto_acks
        .front_mut()
        .unwrap()
        .1 = Instant::now() - Duration::from_secs(6);
    assert!(w.acknowledge_goto(id, 1100).is_err());
    w.step();
    assert!(w.players[&id].departing);
    assert!(*loss.borrow());
    assert_eq!(w.players[&id].movement_credit, 0.);
}
