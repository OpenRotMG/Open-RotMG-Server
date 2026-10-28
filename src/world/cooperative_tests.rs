use super::*;
use std::time::Duration;

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
            assert_eq!(w.players[&sender].pos, start);
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
    assert_eq!(receipt.position, destination);
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
    assert_eq!(d.worlds[&-2].players[&sender].pos, destination);
    assert_eq!(d.teleports[&1], received);
    assert_eq!(
        d.teleport(sender, target, received + Duration::from_secs(10))
            .unwrap()
            .position,
        new_destination
    );
}
