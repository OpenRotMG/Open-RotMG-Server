use super::*;
use std::time::Duration;

#[test]
fn spider_den_teleport_requires_an_eligible_observed_target() {
    let (mut world, sender) = movement_tests::fixture(0x18, false);
    world.name = "Spider Den".into();
    let mut character = world.players[&sender].character.clone();
    character.id = 2;
    character.account = 2;
    let target = world.enter(character, None).unwrap();
    world.players.get_mut(&sender).unwrap().known.insert(target);
    let destination = Pos { x: 12.25, y: 9.75 };
    world.players.get_mut(&target).unwrap().pos = destination;
    let mut directory = Directory::test_with_seed(
        std::path::Path::new("data"),
        world.content.clone(),
        16,
        1051,
    )
    .unwrap();
    directory.worlds.insert(-2, world);
    let received = Instant::now();
    let receipt = directory.teleport(sender, target, received).unwrap();
    assert_eq!(receipt.position.distance(destination), 0.);
    let world = directory.worlds.get_mut(&-2).unwrap();
    world.players.get_mut(&target).unwrap().paused = true;
    assert!(
        directory
            .teleport(sender, target, received + Duration::from_secs(10))
            .unwrap_err()
            .to_string()
            .contains("paused player")
    );
    let world = directory.worlds.get_mut(&-2).unwrap();
    world.players.get_mut(&target).unwrap().paused = false;
    world
        .players
        .get_mut(&sender)
        .unwrap()
        .known
        .remove(&target);
    assert!(
        directory
            .teleport(sender, target, received + Duration::from_secs(10))
            .unwrap_err()
            .to_string()
            .contains("not been observed")
    );
}

#[test]
fn teleport_to_progress_save_target_preserves_its_reservation_and_commit() {
    for controller in [Controller::Human, Controller::Autonomous] {
        let (mut world, sender) = movement_tests::fixture(0x18, false);
        world.name = "Nexus".into();
        let mut ch = world.players[&sender].character.clone();
        ch.id = 2;
        ch.account = 2;
        let (packets, _rx) = tokio::sync::mpsc::channel(128);
        let (lost, _) = tokio::sync::watch::channel(None);
        let target = world.enter(ch, Some(Output { packets, lost })).unwrap();
        world.players.get_mut(&sender).unwrap().known.insert(target);
        let q = world.players.get_mut(&target).unwrap();
        q.controller = controller;
        q.pos = Pos { x: 12.25, y: 9.75 };
        q.queue_experience(10, None, false).unwrap();
        let plans = world.pending_progress_where(|id, _| id == target);
        assert_eq!(plans.len(), 1);
        let snapshot = plans[0].1.clone();
        let q = &world.players[&target];
        q.validate_progress_reservation(&snapshot).unwrap();
        let before = q.character.clone();
        let destination = q.pos;
        let content = world.content.clone();
        let mut d =
            Directory::test_with_seed(std::path::Path::new("data"), content, 16, 910).unwrap();
        d.worlds.insert(-2, world);
        let received = Instant::now();
        let receipt = d.teleport(sender, target, received).unwrap();
        assert_eq!(
            (receipt.player, receipt.character, receipt.target),
            (sender, 1, target)
        );
        assert_eq!(receipt.position.distance(destination), 0.);
        let w = d.worlds.get_mut(&-2).unwrap();
        let q = &w.players[&target];
        assert_eq!(q.character, before);
        assert_eq!(q.pos.distance(destination), 0.);
        q.validate_progress_reservation(&snapshot).unwrap();
        assert!(!cooperative::ready(q));
        w.finish_progress(target, snapshot.clone()).unwrap();
        assert!(cooperative::ready(&w.players[&target]));
        assert_eq!(w.players[&target].character.exp, snapshot.exp);
        assert!(w.finish_progress(target, snapshot).is_err());
    }
}

#[test]
fn progress_save_target_teleport_keeps_all_other_refusals_and_quarantine() {
    let (template, _) = movement_tests::fixture(0x18, false);
    let mut d = Directory::test_with_seed(
        std::path::Path::new("data"),
        template.content.clone(),
        16,
        910,
    )
    .unwrap();
    for state in [
        "sender_pending",
        "sender_progress",
        "inventory_pending",
        "hp",
        "dead",
        "death_pending",
        "departing",
        "quarantined",
        "output_lost",
        "output_closed",
        "paused",
        "invisible",
        "unknown",
        "blocked",
        "correcting",
    ] {
        let (mut w, sender) = movement_tests::fixture(0x18, false);
        w.name = "Nexus".into();
        let mut ch = w.players[&sender].character.clone();
        ch.id = 2;
        ch.account = 2;
        let target = w.enter(ch, None).unwrap();
        w.players.get_mut(&sender).unwrap().known.insert(target);
        w.players
            .get_mut(&target)
            .unwrap()
            .queue_experience(10, None, false)
            .unwrap();
        let snapshot = w
            .pending_progress_where(|id, _| id == target)
            .pop()
            .unwrap()
            .1;
        let mut output_rx = None;
        let q = w.players.get_mut(&target).unwrap();
        match state {
            "sender_pending" => w.players.get_mut(&sender).unwrap().pending = true,
            "sender_progress" => {
                w.players
                    .get_mut(&sender)
                    .unwrap()
                    .queue_experience(10, None, false)
                    .unwrap();
                assert_eq!(w.pending_progress_where(|id, _| id == sender).len(), 1);
            }
            "inventory_pending" => {
                w.finish_progress(target, snapshot.clone()).unwrap();
                let kind = w.players[&target].character.items[0];
                w.drop_item(
                    target,
                    Slot {
                        object: target,
                        slot: 0,
                        kind,
                    },
                )
                .unwrap();
                assert!(w.players[&target].pending && !w.players[&target].progress_started);
            }
            "hp" => q.character.hp = 0,
            "dead" => q.character.alive = false,
            "death_pending" => q.death_pending = true,
            "departing" => q.departing = true,
            "quarantined" => {
                assert!(
                    w.committed::<()>(
                        target,
                        Err(anyhow::anyhow!("injected progress commit failure"))
                    )
                    .is_err()
                );
            }
            "output_lost" | "output_closed" => {
                let (packets, rx) = tokio::sync::mpsc::channel(16);
                let (lost, _) = tokio::sync::watch::channel(None);
                if state == "output_lost" {
                    lose(&lost, "injected_output_loss");
                }
                q.tx = Some(Output { packets, lost });
                if state == "output_lost" {
                    output_rx = Some(rx);
                }
            }
            "paused" => q.paused = true,
            "invisible" => q.lab_conditions = crate::condition::bit(crate::condition::INVISIBLE),
            "unknown" => {
                w.players.get_mut(&sender).unwrap().known.remove(&target);
            }
            "blocked" => q.pos.x = -1.,
            "correcting" => w
                .players
                .get_mut(&sender)
                .unwrap()
                .goto_acks
                .push_back((true, Instant::now())),
            _ => unreachable!(),
        }
        let start = w.players[&sender].pos;
        d.worlds.insert(-2, w);
        assert!(
            d.teleport(sender, target, Instant::now()).is_err(),
            "{state}"
        );
        assert!(d.teleports.is_empty(), "{state}");
        let w = &d.worlds[&-2];
        assert_eq!(w.players[&sender].pos.distance(start), 0., "{state}");
        if state != "inventory_pending" {
            w.players[&target]
                .validate_progress_reservation(&snapshot)
                .unwrap();
        }
        if state == "quarantined" {
            assert!(w.players[&target].persistence_failure.is_some());
        }
        drop(output_rx);
    }
}

#[test]
fn teleport_refusals_preserve_cooldown_and_success_uses_current_target_position() {
    let (mut world, sender) = movement_tests::fixture(0x18, false);
    world.name = "Nexus".into();
    let mut ch = world.players[&sender].character.clone();
    ch.id = 2;
    ch.account = 2;
    let target = world.enter(ch, None).unwrap();
    world.players.get_mut(&sender).unwrap().known.insert(target);
    let start = world.players[&sender].pos;
    let destination = Pos { x: 12.25, y: 9.75 };
    world.players.get_mut(&target).unwrap().pos = destination;
    let content = world.content.clone();
    let mut d = Directory::test_with_seed(std::path::Path::new("data"), content, 16, 910).unwrap();
    d.worlds.insert(-2, world);
    let received = Instant::now();
    for prior in [false, true] {
        d.teleports.clear();
        if prior {
            d.teleports.insert(1, received - Duration::from_secs(20));
        }
        d.teleports.insert(99, received - Duration::from_secs(20));
        let cooldowns = d.teleports.clone();
        for (refusal, expected) in [
            (0, "paused player"),
            (1, "invisible player"),
            (2, "busy, dead"),
            (3, "not been observed"),
            (4, "not a live player"),
            (5, "Cannot teleport in this world"),
            (6, "while paused"),
        ] {
            let w = d.worlds.get_mut(&-2).unwrap();
            match refusal {
                0 => w.players.get_mut(&target).unwrap().paused = true,
                1 => {
                    w.players.get_mut(&target).unwrap().lab_conditions =
                        crate::condition::bit(crate::condition::INVISIBLE)
                }
                2 => w.players.get_mut(&target).unwrap().pending = true,
                3 => {
                    w.players.get_mut(&sender).unwrap().known.remove(&target);
                }
                5 => w.name = "Vault".into(),
                6 => w.players.get_mut(&sender).unwrap().paused = true,
                _ => {}
            }
            let requested = if refusal == 4 { i32::MAX } else { target };
            let error = d.teleport(sender, requested, received).unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert_eq!(d.teleports, cooldowns);
            let w = d.worlds.get_mut(&-2).unwrap();
            assert_eq!(w.players[&sender].pos.distance(start), 0.);
            assert!(!w.players[&sender].character.lifetime.teleported);
            let q = w.players.get_mut(&target).unwrap();
            q.paused = false;
            q.lab_conditions = 0;
            q.pending = false;
            let p = w.players.get_mut(&sender).unwrap();
            p.paused = false;
            p.known.insert(target);
            w.name = "Nexus".into();
        }
    }
    let receipt = d.teleport(sender, target, received).unwrap();
    assert_eq!(receipt.position.distance(destination), 0.);
    assert_eq!(receipt.world, -2);
    assert_eq!(d.teleports[&1], received);
    assert!(d.worlds[&-2].players[&sender].character.lifetime.teleported);
    let new_destination = Pos { x: 14.5, y: 10.5 };
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&target)
        .unwrap()
        .pos = new_destination;
    let error = d
        .teleport(sender, target, received + Duration::from_millis(9999))
        .unwrap_err();
    assert!(error.to_string().contains("Too soon"));
    assert_eq!(d.worlds[&-2].players[&sender].pos.distance(destination), 0.);
    assert_eq!(d.teleports[&1], received);
    assert_eq!(
        d.teleport(sender, target, received + Duration::from_secs(10))
            .unwrap()
            .position
            .distance(new_destination),
        0.
    );
}
