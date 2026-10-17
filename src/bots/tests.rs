use super::*;
fn on_adventure(profile: Profile, v: &PlayerView) -> Brain {
    let names = if ["Biceps", "Triceps", "BattleCrus"].contains(&profile.name) {
        ["Biceps", "Triceps", "BattleCrus"]
    } else {
        ["ExElite", "Kadabu", "Pixl"]
    };
    let members = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            let account = if name == profile.name {
                v.session.account
            } else {
                v.objects
                    .iter()
                    .find(|p| p.name == name)
                    .and_then(|p| p.account)
                    .unwrap_or(i as i32 + 2)
            };
            adventures::Member {
                account,
                name: name.into(),
            }
        })
        .collect();
    let mut b = Brain::new(profile, v);
    let purpose = if v.objects.iter().any(|o| o.kind == 0x718) {
        adventures::Purpose::Speed
    } else {
        adventures::Purpose::Equipment
    };
    b.group = Some(adventures::Adventure {
        members,
        purpose,
        until: v.elapsed_ms + 600000,
        finished: false,
    });
    b
}
#[test]
fn exploration_remains_available_when_a_higher_ranked_route_is_blocked() {
    let c = content();
    let mut v = view(&c);
    let mut ally = object(4, WIZARD, 22.5, 11.5);
    ally.name = "Biceps".into();
    ally.equipped = vec![0xa97];
    v.objects.push(ally);
    assert_eq!(
        on_adventure(PROFILES[2], &v).decide(&v, &c).unwrap().goal,
        Goal::Follow(4)
    );
    Arc::make_mut(&mut v.tiles).retain(|t| t.x != 15);
    let d = on_adventure(PROFILES[2], &v).decide(&v, &c).unwrap();
    assert!(matches!(d.goal, Goal::Explore(x, _) if x < 15));
}

#[test]
fn retained_exploration_can_outscore_a_clear_distant_enemy() {
    let c = content();
    let mut v = view(&c);
    let enemy = c.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 26.5, 11.5));
    let mut b = Brain::new(PROFILES[1], &v);
    b.goal = Goal::Explore(8, 11);
    assert_eq!(b.decide(&v, &c).unwrap().goal, Goal::Explore(8, 11));
}

#[test]
fn strong_tomes_still_protect_low_level_companions() {
    let c = content();
    let mut v = view(&c);
    v.character.class = crate::content::PRIEST;
    v.character.items[0] = 2564;
    v.character.items[1] = 2775;
    v.character.mp = c.get(2775).unwrap().mana_cost;
    assert!(c.get(2775).unwrap().heal > 100);
    let mut peer = object(2, WIZARD, 12.5, 11.5);
    peer.equipped = vec![2711, 2606, -1, -1];
    peer.max_hp = 100;
    peer.hp = 60;
    v.objects.push(peer);
    assert!(social::healing(&v, &c).unwrap().is_some());
    v.objects[0].hp = 80;
    assert!(social::healing(&v, &c).unwrap().is_none());
    v.objects.clear();
    v.character.hp = 60;
    assert!(social::healing(&v, &c).unwrap().is_some());
    v.character.hp = 80;
    assert!(social::healing(&v, &c).unwrap().is_none());
}

#[test]
fn bounded_waypoint_checks_its_actual_endpoint_not_a_distant_sampling_grid() {
    let mut v = view(&content());
    v.position = Pos {
        x: 12.1716,
        y: 12.1715,
    };
    Arc::make_mut(&mut v.tiles).retain(|t| (t.x, t.y) != (15, 14));
    let grid = Grid::new(&v);
    let target = Pos { x: 25.5, y: 25.5 };
    assert!(grid.segment(v.position, target));
    let next = grid.next(v.position, target).unwrap();
    assert!(grid.walkable.contains(&tile(next)));
    assert!(grid.segment(v.position, next));
}

#[test]
fn observed_static_cover_survives_leaving_view_but_not_a_verified_clear_tile() {
    let c = content();
    let mut v = view(&c);
    v.map.width = 60;
    v.tiles = (0..30)
        .flat_map(|y| {
            (0..60).map(move |x| Tile {
                x,
                y,
                ground: 0,
                ground_speed: 1.,
            })
        })
        .collect::<Vec<_>>()
        .into();
    let mut wall = object(2, 0x1ce, 2.5, 11.5);
    wall.blocks = true;
    assert!(!c.objects[&wall.kind].enemy);
    v.objects.push(wall);
    let mut brain = Brain::new(PROFILES[1], &v);
    assert!(!brain.grid(&v, &c).walkable.contains(&(2, 11)));
    v.objects.clear();
    v.position.x = 35.5;
    let remembered = brain.grid(&v, &c);
    assert!(!remembered.walkable.contains(&(2, 11)));
    assert!(remembered.walkable.contains(&(2, 12)));
    assert!(
        Brain::new(PROFILES[1], &v)
            .grid(&v, &c)
            .walkable
            .contains(&(2, 11))
    );
    v.position.x = 11.5;
    assert!(brain.grid(&v, &c).walkable.contains(&(2, 11)));
}

#[test]
fn remembered_routes_advance_nearby_before_revalidating_distant_cover() {
    let c = content();
    let mut v = view(&c);
    v.position = Pos { x: 9.5, y: 70.5 };
    v.tiles = (0..98)
        .flat_map(|y| {
            (0..94).map(move |x| Tile {
                x,
                y,
                ground: 253,
                ground_speed: 1.,
            })
        })
        .collect::<Vec<_>>()
        .into();
    let destination = Pos { x: 8.5, y: 93.5 };
    let grid = Grid::new(&v);
    let next = grid.next(v.position, destination).unwrap();
    assert!(v.position.distance(next) <= 4.001);
    assert!(next.distance(destination) < v.position.distance(destination));
    let mut wall = object(2, 0x1ce, destination.x, destination.y);
    wall.blocks = true;
    v.objects.push(wall);
    assert!(Grid::new(&v).next(next, destination).is_none());
    let nearby = Pos { x: 10.5, y: 70.5 };
    assert_eq!(Grid::new(&v).next(v.position, nearby).unwrap().x, nearby.x);
}

#[test]
fn dodge_predictions_use_actual_movement_conditions_and_ground() {
    use crate::content::{PARALYZED, SLOWED, SPEEDY};
    let c = content();
    let mut v = view(&c);
    v.character.stats[4] = 75;
    for t in Arc::make_mut(&mut v.tiles) {
        t.ground_speed = 0.6;
    }
    for (flags, expected) in [
        (0, 5.76),
        (SLOWED, 2.4),
        (SPEEDY, 8.64),
        (SLOWED | SPEEDY, 2.4),
        (PARALYZED | SPEEDY, 0.),
    ] {
        v.character.conditions = flags;
        assert!((combat::speed(&v) - expected).abs() < 0.001);
    }
    let grid = Grid::new(&v);
    assert!(combat::movement(&v, &grid, &[], Some(Pos { x: 15.5, y: 11.5 }), 7., false).is_none());
}

#[test]
fn stunned_wizard_keeps_target_but_does_not_request_normal_fire() {
    let c = content();
    let mut v = view(&c);
    let enemy = c.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 15.5, 11.5));
    let ready = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(matches!(ready.goal, Goal::Fight(2)));
    assert!(ready.aim.is_some());
    v.character.conditions = crate::content::STUNNED;
    let stunned = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(matches!(stunned.goal, Goal::Fight(2)));
    assert!(stunned.aim.is_none());
}

use crate::content::WIZARD;
use crate::world::view::{
    Lifecycle, MapInfo, OwnCharacter, Session, Tile, VisibleObject, VisibleProjectile,
};
pub(super) fn content() -> Content {
    Content::load(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .as_path(),
    )
    .unwrap()
}
pub(super) fn view(content: &Content) -> PlayerView {
    let class = content.get(WIZARD).unwrap();
    PlayerView {
        session: Session {
            world: -3,
            player: 1,
            account: 1,
            character: 1,
        },
        tick: 100,
        time_ms: 5000,
        elapsed_ms: 5000,
        position: Pos { x: 11.5, y: 11.5 },
        quest: None,
        map: MapInfo {
            name: "Realm".into(),
            width: 40,
            height: 40,
        },
        character: OwnCharacter {
            conditions: 0,
            class: WIZARD,
            level: 1,
            experience: 0,
            hp: 100,
            mp: 100,
            stats: [100, 100, 12, 0, 10, 15, 12, 12],
            base_stats: [100, 100, 12, 0, 10, 15, 12, 12],
            items: class.equipment.clone(),
            inventory_revision: 0,
            pirate_caves: 0,
            snake_pits: 0,
            ability_ready_ms: 0,
        },
        lifecycle: Lifecycle {
            alive: true,
            death_pending: false,
            departing: false,
            busy: false,
            quarantined: false,
            output: OutputState::Live,
        },
        tiles: (0..30)
            .flat_map(|y| {
                (0..30).map(move |x| Tile {
                    x,
                    y,
                    ground: 0,
                    ground_speed: 1.,
                })
            })
            .collect::<Vec<_>>()
            .into(),
        objects: vec![],
        projectiles: vec![],
    }
}
pub(super) fn object(id: i32, kind: u16, x: f32, y: f32) -> VisibleObject {
    VisibleObject {
        id,
        account: (kind == WIZARD || kind == crate::content::PRIEST).then_some(id),
        kind,
        name: String::new(),
        position: Pos { x, y },
        hp: 100,
        max_hp: 100,
        level: None,
        equipped: vec![],
        contents: vec![],
        blocks: false,
        usable: true,
        condition_mask: None,
    }
}
#[test]
fn same_observation_produces_resource_recovery_and_companion_choices() {
    let content = content();
    let mut v = view(&content);
    v.character.hp = 50;
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 21.5, 11.5));
    let bag = content
        .objects
        .values()
        .find(|o| o.class == "Container")
        .unwrap()
        .kind;
    let mut loot = object(3, bag, 13., 11.5);
    loot.contents = vec![0xa22];
    v.objects.push(loot);
    let mut ally = object(4, WIZARD, 22.5, 11.5);
    ally.name = "Biceps".into();
    ally.equipped = vec![0xa97];
    v.objects.push(ally);
    let goals: Vec<_> = PROFILES[..3]
        .iter()
        .map(|p| on_adventure(*p, &v).decide(&v, &content).unwrap().goal)
        .collect();
    assert_eq!(goals, vec![Goal::Recover, Goal::Loot(3), Goal::Follow(4)]);
}
#[test]
fn explores_known_frontiers_toward_an_observed_distant_encounter() {
    let content = content();
    let mut v = view(&content);
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 39.5, 11.5));
    let mut brain = Brain::new(PROFILES[1], &v);
    brain.heading = std::f32::consts::PI;
    let decision = brain.decide(&v, &content).unwrap();
    assert_eq!(decision.goal, Goal::Explore(29, 11));
    let grid = Grid::new(&v);
    assert!(!grid.known.contains(&(39, 11)));
    assert!(grid.segment(v.position, decision.movement.unwrap()));
    assert!(decision.aim.is_none());
}

#[test]
fn exploration_does_not_wait_for_disconnected_known_frontiers() {
    let content = content();
    let mut v = view(&content);
    for y in 0..30 {
        let mut wall = object(100 + y, WIZARD, 16.5, y as f32 + 0.5);
        wall.blocks = true;
        v.objects.push(wall);
    }
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 39.5, 11.5));
    let mut brain = Brain::new(PROFILES[1], &v);
    brain.goal = Goal::Explore(29, 11);
    let decision = brain.decide(&v, &content).unwrap();
    let Goal::Explore(x, y) = decision.goal else {
        panic!("reachable exploration expected")
    };
    assert!(x < 16);
    let grid = Grid::new(&v);
    assert!(grid.reachable(v.position).contains(&(x, y)));
    assert!(grid.segment(v.position, decision.movement.unwrap()));
}

#[test]
fn busy_clock_lifecycle_and_session_do_not_fabricate_actions() {
    let content = content();
    let mut v = view(&content);
    let mut b = Brain::new(PROFILES[0], &v);
    v.lifecycle.busy = true;
    let d = b.decide(&v, &content).unwrap();
    assert!(d.movement.is_none() && d.aim.is_none() && d.item.is_none());
    assert!(b.decide(&v, &content).is_err());
    v.tick += 1;
    v.session.character = 2;
    assert!(b.decide(&v, &content).is_err());
    v.session.character = 1;
    for state in [OutputState::Detached, OutputState::Lost] {
        v.lifecycle.output = state;
        assert!(b.decide(&v, &content).is_err());
    }
    v.lifecycle.output = OutputState::Live;
    v.lifecycle.death_pending = true;
    assert!(b.decide(&v, &content).is_err());
    v.lifecycle.death_pending = false;
    v.lifecycle.quarantined = true;
    assert!(b.decide(&v, &content).is_err());
}
#[test]
fn projectile_dodge_uses_observed_trajectory_and_only_known_routes() {
    let content = content();
    let mut v = view(&content);
    v.projectiles.push(VisibleProjectile {
        owner: 9,
        id: 1,
        start: Pos { x: 9.5, y: 11.5 },
        position: Pos { x: 9.5, y: 11.5 },
        angle: 0.,
        speed: 50.,
        lifetime_ms: 1000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
    });
    let d = Brain::new(PROFILES[1], &v).decide(&v, &content).unwrap();
    assert_eq!(d.goal, Goal::Dodge);
    let movement = d.movement.unwrap();
    assert!(Grid::new(&v).segment(v.position, movement));
    assert!(movement.distance(v.position) > 1.);
    let grid = Grid {
        walkable: BTreeSet::from([(0, 0), (0, 1), (1, 1), (2, 1)]),
        known: BTreeSet::from([(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)]),
        dimensions: (3, 2),
        ..Grid::default()
    };
    assert!(grid.next(center((0, 0)), center((3, 1))).is_none());
    let next = grid.next(center((0, 0)), center((2, 1))).unwrap();
    assert_eq!(tile(next), (0, 1));
    assert!(!grid.frontier((1, 1), 3, 2));
    assert!(!grid.frontier((2, 1), 3, 2));
    let mut straight = Grid {
        walkable: (0..=24).map(|x| (x, 0)).collect(),
        known: (0..=24).map(|x| (x, 0)).collect(),
        dimensions: (25, 1),
        ..Grid::default()
    };
    let mut position = center((0, 0));
    for _ in 0..6 {
        let next = straight.next(position, center((24, 0))).unwrap();
        assert!(position.distance(next) <= 4.001);
        position = next;
    }
    assert_eq!(tile(position), (24, 0));
    straight.walkable.remove(&(13, 0));
    assert!(!straight.segment(center((0, 0)), center((24, 0))));
    assert!(straight.next(center((0, 0)), center((24, 0))).is_none());
}
#[test]
fn held_resources_compatible_upgrades_and_margin_survive_seeded_replay() {
    let content = content();
    let mut v = view(&content);
    v.character.hp = 1;
    v.character.items[4..].fill(-1);
    let mut brain = Brain::new(PROFILES[0], &v);
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
    v.tick += 1;
    v.character.items[4] = 0xa22;
    v.character.stats[0] = 200;
    assert!(matches!(
        brain.decide(&v, &content).unwrap().item,
        Some(Action::Use { slot: 4, .. })
    ));
    assert!(!useful(&content, &v, 0xa01).unwrap());
    let mut left = Brain::new(PROFILES[2], &v);
    let mut right = Brain::new(PROFILES[2], &v);
    v.character.hp = 200;
    for tick in 110..140 {
        v.tick = tick;
        let a = left.decide(&v, &content).unwrap();
        let b = right.decide(&v, &content).unwrap();
        assert_eq!(a.goal, b.goal);
        assert_eq!(format!("{:?}", a.movement), format!("{:?}", b.movement));
        left.goal = a.goal;
        right.goal = b.goal;
    }
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects = vec![object(2, enemy, 18.5, 11.5), object(3, enemy, 17.5, 11.5)];
    v.tick = 200;
    v.time_ms = 10000;
    left.goal = Goal::Fight(2);
    assert_eq!(left.decide(&v, &content).unwrap().goal, Goal::Fight(2));
}
#[test]
fn only_addressed_native_chat_changes_companion_memory() {
    let content = content();
    let mut v = view(&content);
    let mut brain = on_adventure(PROFILES[2], &v);
    let packet = |text: &str| {
        let mut w = crate::wire::Writer::new();
        w.utf("Human").i32(9).i32(0).u8(5).utf("").utf(text).utf("");
        Packet {
            id: wire::TEXT,
            body: w.0,
        }
    };
    assert!(
        !brain
            .receive(packet("BattleCrus follow me"), &v, true)
            .unwrap()
    );
    assert!(brain.companion.is_none());
    let mut sender = object(9, WIZARD, 12.5, 11.5);
    sender.equipped = v.character.items[..4].to_vec();
    v.objects.push(sender);
    assert!(
        !brain
            .receive(packet("Triceps follow me"), &v, true)
            .unwrap()
    );
    assert!(brain.companion.is_none());
    assert!(
        brain
            .receive(packet("BattleCrus follow me"), &v, true)
            .unwrap()
    );
    assert_eq!(brain.companion, Some(9));
    v.objects[0].position = Pos { x: 20.5, y: 11.5 };
    let mut leader = object(2, WIZARD, 11.5, 22.5);
    leader.name = "Biceps".into();
    leader.equipped = v.character.items[..4].to_vec();
    v.objects.push(leader);
    brain.goal = Goal::Follow(2);
    let accepted = brain.decide(&v, &content).unwrap();
    assert_eq!(accepted.goal, Goal::Follow(9));
    brain.goal = accepted.goal;
    v.character.hp = 20;
    v.tick += 5;
    v.time_ms += 250;
    assert_eq!(brain.decide(&v, &content).unwrap().goal, Goal::Recover);
    v.character.hp = v.character.stats[0];
    v.time_ms += 60000;
    v.tick += 1200;
    assert_eq!(brain.decide(&v, &content).unwrap().goal, Goal::Follow(2));
    assert!(brain.companion.is_none());
}

#[test]
fn fresh_crewmate_meets_unseen_leader_in_nexus_through_expiring_public_chat() {
    let content = content();
    let mut v = view(&content);
    let packet = |name: &str, actor, recipient: &str, text: &str| {
        let mut w = crate::wire::Writer::new();
        w.utf(name)
            .i32(actor)
            .i32(0)
            .u8(5)
            .utf(recipient)
            .utf(text)
            .utf("");
        Packet {
            id: wire::TEXT,
            body: w.0,
        }
    };
    let mut follower = on_adventure(PROFILES[2], &v);
    assert!(follower.decide(&v, &content).unwrap().chat.is_none());
    v.time_ms += 10000;
    v.tick += 200;
    assert_eq!(follower.decide(&v, &content).unwrap().chat, Some("tp?"));
    assert_eq!(follower.next_call, v.time_ms + 30000);
    assert_eq!(follower.requested_until, v.time_ms + 5000);
    v.tick += 5;
    assert!(follower.decide(&v, &content).unwrap().chat.is_none());
    for (name, actor, recipient) in [
        ("ExElite", 9, ""),
        ("Biceps", -1, ""),
        ("Biceps", 2, "Human"),
    ] {
        follower
            .receive(packet(name, actor, recipient, "nexus"), &v, true)
            .unwrap();
        assert!(follower.regroup_call.is_none());
    }
    let mut leader_view = view(&content);
    leader_view.session.player = 2;
    let mut leader = on_adventure(PROFILES[1], &leader_view);
    leader_view.time_ms += 10000;
    leader_view.tick += 200;
    leader
        .receive(packet("Kadabu", 9, "", "tp?"), &leader_view, true)
        .unwrap();
    assert!(leader.regroup_call.is_none());
    leader
        .receive(packet("BattleCrus", 1, "", "tp?"), &leader_view, true)
        .unwrap();
    let reply = leader.decide(&leader_view, &content).unwrap();
    assert_eq!(reply.chat, Some("nexus"));
    assert!(matches!(reply.journey, Some(Journey::Nexus)));
    follower
        .receive(packet("Biceps", 2, "", "tp here"), &v, true)
        .unwrap();
    assert!(follower.regroup_call.is_none());
    follower
        .receive(packet("Biceps", 2, "", "nexus"), &v, true)
        .unwrap();
    v.tick += 5;
    let decision = follower.decide(&v, &content).unwrap();
    assert!(matches!(decision.journey, Some(Journey::Nexus)));
    assert!(decision.aim.is_none() && decision.item.is_none() && decision.movement.is_none());
    assert!(v.objects.is_empty());
    follower
        .receive(packet("Biceps", 2, "", "tp here"), &v, true)
        .unwrap();
    v.tick += 5;
    assert!(follower.decide(&v, &content).unwrap().journey.is_none());
    let mut unsolicited = on_adventure(PROFILES[2], &v);
    unsolicited
        .receive(packet("Biceps", 2, "", "tp here"), &v, true)
        .unwrap();
    assert!(unsolicited.regroup_call.is_none());
    let mut occupied = on_adventure(PROFILES[2], &v);
    occupied.requested_until = v.time_ms + 5000;
    occupied.companion = Some(9);
    occupied
        .receive(packet("Biceps", 2, "", "nexus"), &v, true)
        .unwrap();
    assert!(occupied.regroup_call.is_none());
    occupied.companion = None;
    occupied.leader_seen = Some((2, v.position, v.time_ms));
    occupied
        .receive(packet("Biceps", 2, "", "nexus"), &v, true)
        .unwrap();
    assert!(matches!(
        occupied.decide(&v, &content).unwrap().journey,
        Some(Journey::Nexus)
    ));
    let mut expired = on_adventure(PROFILES[2], &v);
    expired.requested_until = v.time_ms + 5000;
    expired
        .receive(packet("Biceps", 2, "", "nexus"), &v, true)
        .unwrap();
    v.time_ms += 5001;
    assert!(expired.decide(&v, &content).unwrap().journey.is_none());
    let mut injured = on_adventure(PROFILES[2], &v);
    injured.requested_until = v.time_ms + 5000;
    injured
        .receive(packet("Biceps", 2, "", "nexus"), &v, true)
        .unwrap();
    v.character.hp = 30;
    assert!(matches!(
        injured.decide(&v, &content).unwrap().journey,
        Some(Journey::Nexus)
    ));
    v.character.hp = 100;
    let mut endangered = on_adventure(PROFILES[1], &leader_view);
    endangered.next_call = leader_view.time_ms;
    endangered
        .receive(packet("BattleCrus", 1, "", "tp?"), &leader_view, true)
        .unwrap();
    let kind = content.objects.values().find(|o| o.enemy).unwrap().kind;
    leader_view.objects.push(object(9, kind, 14.5, 11.5));
    assert!(
        endangered
            .decide(&leader_view, &content)
            .unwrap()
            .chat
            .is_none()
    );
}

#[test]
fn nexus_call_reunites_the_whole_crew_not_only_the_requester() {
    let content = content();
    let v = view(&content);
    for profile in [PROFILES[0], PROFILES[2]] {
        let mut follower = on_adventure(profile, &v);
        let mut w = crate::wire::Writer::new();
        w.utf("Biceps")
            .i32(2)
            .i32(0)
            .u8(5)
            .utf("")
            .utf("nexus")
            .utf("");
        follower
            .receive(
                Packet {
                    id: wire::TEXT,
                    body: w.0,
                },
                &v,
                true,
            )
            .unwrap();
        assert!(matches!(
            follower.decide(&v, &content).unwrap().journey,
            Some(Journey::Nexus)
        ));
    }
}

#[test]
fn observed_crew_can_teleport_and_nexus_arrivals_wait_for_peers() {
    let content = content();
    let mut v = view(&content);
    let mut follower = on_adventure(PROFILES[2], &v);
    follower.requested_until = v.time_ms + 5000;
    let mut peer = object(2, WIZARD, 23.5, 11.5);
    peer.name = "Biceps".into();
    v.objects.push(peer);
    let mut w = crate::wire::Writer::new();
    w.utf("Biceps")
        .i32(2)
        .i32(0)
        .u8(5)
        .utf("")
        .utf("tp here")
        .utf("");
    follower
        .receive(
            Packet {
                id: wire::TEXT,
                body: w.0,
            },
            &v,
            true,
        )
        .unwrap();
    assert!(matches!(
        follower.decide(&v, &content).unwrap().journey,
        Some(Journey::Regroup(2))
    ));
    v.map.name = "Nexus".into();
    v.objects.clear();
    let mut arrived = on_adventure(PROFILES[2], &v);
    let decision = arrived.decide(&v, &content).unwrap();
    assert_eq!(decision.goal, Goal::Wait);
    assert!(decision.journey.is_none() && decision.movement.is_none());
    v.time_ms += 10001;
    v.tick += 201;
    assert_eq!(arrived.decide(&v, &content).unwrap().goal, Goal::Wait);
    v.time_ms += 30000;
    v.tick += 600;
    assert_ne!(arrived.decide(&v, &content).unwrap().goal, Goal::Wait);
}

#[test]
fn selected_target_controls_aim_and_equipped_spell_cooldown() {
    let content = content();
    let mut v = view(&content);
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects = vec![object(2, enemy, 15.5, 11.5), object(3, enemy, 11.5, 14.5)];
    v.objects[0].hp = 1000;
    let mut brain = Brain::new(PROFILES[1], &v);
    brain.goal = Goal::Fight(2);
    let first = brain.decide(&v, &content).unwrap();
    assert_eq!(first.goal, Goal::Fight(2));
    assert!(first.aim.unwrap().abs() < 0.01);
    assert!(matches!(first.item, Some(Action::Use { slot: 1, x, y }) if x == 15.5 && y == 11.5));
    v.character.ability_ready_ms = v.time_ms
        + content
            .get(v.character.items[1] as u16)
            .unwrap()
            .cooldown_ms;
    v.tick += 4;
    v.time_ms += 200;
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
    v.tick += 20;
    v.time_ms += 1000;
    v.character.mp = 0;
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
}

#[test]
fn kites_from_close_targets_and_does_not_fire_through_known_cover() {
    let content = content();
    let mut v = view(&content);
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 13.5, 11.5));
    let mut brain = Brain::new(PROFILES[1], &v);
    let decision = brain.decide(&v, &content).unwrap();
    assert_eq!(decision.goal, Goal::Fight(2));
    assert!(decision.movement.unwrap().distance(v.objects[0].position) > 2.);
    let mut wall = object(9, WIZARD, 12.5, 11.5);
    wall.blocks = true;
    v.objects.push(wall);
    v.tick += 4;
    v.time_ms += 200;
    let blocked = brain.decide(&v, &content).unwrap();
    assert!(blocked.aim.is_none());
    assert!(blocked.item.is_none());
    assert!(blocked.movement.is_some());
    let mut path = blocked;
    for _ in 0..8 {
        brain.goal = path.goal.clone();
        v.position = path
            .movement
            .expect("blocked target needs an approach route");
        v.tick += 4;
        v.time_ms += 200;
        path = brain.decide(&v, &content).unwrap();
        if path.aim.is_some() {
            break;
        }
    }
    assert!(
        path.aim.is_some(),
        "must reach a firing lane instead of orbiting behind cover"
    );
}

#[test]
fn committed_disengagement_uses_nexus_before_the_last_hit_without_held_healing() {
    let content = content();
    let mut v = view(&content);
    v.character.hp = 30;
    v.character
        .items
        .iter_mut()
        .skip(4)
        .for_each(|item| *item = -1);
    let mut brain = Brain::new(PROFILES[0], &v);
    let escaped = brain.decide(&v, &content).unwrap();
    assert_eq!(escaped.goal, Goal::Recover);
    assert!(matches!(escaped.journey, Some(Journey::Nexus)));
    assert!(escaped.aim.is_none());
    v.character.items[4] = 2594;
    let mut brain = Brain::new(PROFILES[0], &v);
    let stocked = brain.decide(&v, &content).unwrap();
    assert!(stocked.journey.is_none());
    assert!(matches!(stocked.item, Some(Action::Use { slot: 4, .. })));
}

#[test]
fn observed_motion_leads_aim_and_recovery_has_a_release_threshold() {
    let content = content();
    let mut v = view(&content);
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 15.5, 11.5));
    let mut brain = Brain::new(PROFILES[0], &v);
    brain.decide(&v, &content).unwrap();
    v.tick += 6;
    v.time_ms += 300;
    v.objects[0].position.y += 0.6;
    assert!(
        brain.decide(&v, &content).unwrap().aim.unwrap() > angle(v.position, v.objects[0].position)
    );
    v.tick += 6;
    v.time_ms += 300;
    v.character.hp = 50;
    let recovery = brain.decide(&v, &content).unwrap();
    assert_eq!(recovery.goal, Goal::Recover);
    assert!(recovery.aim.is_none());
    brain.goal = recovery.goal;
    v.tick += 100;
    v.time_ms += 5000;
    v.character.hp = 65;
    assert_eq!(brain.decide(&v, &content).unwrap().goal, Goal::Recover);
    v.tick += 6;
    v.time_ms += 300;
    v.character.hp = 90;
    assert_eq!(brain.decide(&v, &content).unwrap().goal, Goal::Fight(2));
}

#[test]
fn dodge_scores_the_swept_player_path_not_only_the_endpoint() {
    let content = content();
    let mut v = view(&content);
    v.projectiles.push(VisibleProjectile {
        owner: 9,
        id: 0,
        start: Pos { x: 12.125, y: 10. },
        position: Pos { x: 12.125, y: 10. },
        angle: std::f32::consts::FRAC_PI_2,
        speed: 100.,
        lifetime_ms: 1000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
    });
    let crossing = Pos { x: 12.75, y: 11.5 };
    let away = Pos { x: 10.25, y: 11.5 };
    assert!(combat::clearance(&v, crossing) < 0.2);
    assert!(combat::clearance(&v, away) > 0.6);
}

#[test]
fn cohorts_choose_distinct_observed_leaders_not_an_uninvited_human() {
    let content = content();
    let mut v = view(&content);
    for (id, name, x, y) in [
        (2, "Biceps", 21.5, 11.5),
        (3, "ExElite", 11.5, 21.5),
        (4, "Human", 10.5, 11.5),
    ] {
        let mut peer = object(id, WIZARD, x, y);
        peer.name = name.into();
        peer.equipped = v.character.items[..4].to_vec();
        v.objects.push(peer);
    }
    assert_eq!(
        on_adventure(PROFILES[2], &v)
            .decide(&v, &content)
            .unwrap()
            .goal,
        Goal::Follow(2)
    );
    assert_eq!(
        on_adventure(PROFILES[3], &v)
            .decide(&v, &content)
            .unwrap()
            .goal,
        Goal::Follow(3)
    );
    v.objects.retain(|o| o.name != "Biceps");
    assert!(matches!(
        on_adventure(PROFILES[1], &v)
            .decide(&v, &content)
            .unwrap()
            .goal,
        Goal::Explore(..)
    ));
}

#[test]
fn priest_heals_actual_public_injury_and_respects_range_mana_and_cooldown() {
    let content = content();
    let mut v = view(&content);
    v.character.class = crate::content::PRIEST;
    v.character.items = content.get(v.character.class).unwrap().equipment.clone();
    let mut peer = object(2, WIZARD, 12.5, 11.5);
    peer.equipped = vec![0xa97];
    peer.name = "Biceps".into();
    v.objects.push(peer);
    let mut brain = Brain::new(PROFILES[0], &v);
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
    v.tick += 6;
    v.time_ms += 300;
    v.objects[0].hp = 40;
    assert!(matches!(
        brain.decide(&v, &content).unwrap().item,
        Some(Action::Use { slot: 1, .. })
    ));
    v.character.ability_ready_ms = v.time_ms
        + content
            .get(v.character.items[1] as u16)
            .unwrap()
            .cooldown_ms;
    v.tick += 6;
    v.time_ms += 300;
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
    v.tick += 100;
    v.time_ms += 5000;
    v.character.mp = 0;
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
    v.tick += 6;
    v.time_ms += 300;
    v.character.mp = 100;
    v.objects[0].position.x = 25.5;
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
}

#[test]
fn leader_waits_for_a_real_straggler_and_invitation_expires() {
    let content = content();
    let mut v = view(&content);
    let mut peer = object(2, WIZARD, 23.5, 11.5);
    peer.name = "BattleCrus".into();
    peer.equipped = v.character.items[..4].to_vec();
    v.objects.push(peer);
    let d = on_adventure(PROFILES[1], &v).decide(&v, &content).unwrap();
    assert_eq!(d.goal, Goal::Gather(2));
    assert!(d.movement.is_none());
    let mut brain = on_adventure(PROFILES[0], &v);
    brain.companion = Some(2);
    brain.invitation_until = v.time_ms;
    brain.decide(&v, &content).unwrap();
    assert!(brain.companion.is_none());
}

#[test]
fn regroup_requests_only_a_visible_healthy_peer_and_is_paced() {
    let content = content();
    let mut v = view(&content);
    let mut leader = object(2, WIZARD, 21.5, 11.5);
    leader.name = "Biceps".into();
    leader.equipped = v.character.items[..4].to_vec();
    v.objects.push(leader);
    let mut brain = on_adventure(PROFILES[2], &v);
    assert!(matches!(
        brain.decide(&v, &content).unwrap().journey,
        Some(Journey::Regroup(2))
    ));
    v.tick += 5;
    v.time_ms += 250;
    assert!(brain.decide(&v, &content).unwrap().journey.is_none());
    v.tick += 300;
    v.time_ms += 15000;
    v.objects[0].hp = 40;
    assert!(brain.decide(&v, &content).unwrap().journey.is_none());
    v.tick += 5;
    v.time_ms += 250;
    v.objects.clear();
    assert!(brain.decide(&v, &content).unwrap().journey.is_none());
}

#[test]
fn earned_portal_call_and_observed_cave_exit_drive_normal_transfers() {
    let content = content();
    let mut v = view(&content);
    v.character.level = 2;
    for (id, name) in [(2, "Triceps"), (3, "BattleCrus")] {
        let mut member = object(id, WIZARD, 12.5, 11.5);
        member.name = name.into();
        member.equipped = v.character.items[..4].to_vec();
        v.objects.push(member);
    }
    v.objects.push(object(4, 0x717, 12.5, 11.5));
    let leader = on_adventure(PROFILES[1], &v).decide(&v, &content).unwrap();
    assert_eq!(leader.goal, Goal::Portal(4));
    assert_eq!(leader.chat, Some("pc here"));
    assert!(matches!(leader.journey, Some(Journey::Portal(4))));
    v.objects[0].name = "Biceps".into();
    v.objects.remove(1);
    let mut follower = on_adventure(PROFILES[2], &v);
    follower.decide(&v, &content).unwrap();
    let mut out = crate::wire::Writer::new();
    out.utf("Biceps")
        .i32(2)
        .i32(0)
        .u8(5)
        .utf("")
        .utf("pc here")
        .utf("");
    follower
        .receive(
            Packet {
                id: wire::TEXT,
                body: out.0,
            },
            &v,
            true,
        )
        .unwrap();
    v.objects.remove(0);
    v.tick += 5;
    v.time_ms += 250;
    assert!(matches!(
        follower.decide(&v, &content).unwrap().journey,
        Some(Journey::Portal(4))
    ));
    v.map.name = "Pirate Cave".into();
    v.objects.clear();
    let mut cave = on_adventure(PROFILES[2], &v);
    assert!(cave.decide(&v, &content).unwrap().journey.is_none());
    v.tick += 5;
    v.time_ms += 250;
    v.objects.push(object(4, 0x71d, 12.5, 11.5));
    assert_eq!(v.character.pirate_caves, 0);
    assert!(matches!(
        cave.decide(&v, &content).unwrap().journey,
        Some(Journey::Portal(4))
    ));
}

#[test]
fn quiet_exploration_revisits_only_an_actually_observed_hunting_ground() {
    let content = content();
    let mut v = view(&content);
    let enemy = content.objects.values().find(|o| o.enemy).unwrap().kind;
    v.objects.push(object(2, enemy, 15.5, 11.5));
    let mut brain = Brain::new(PROFILES[1], &v);
    brain.decide(&v, &content).unwrap();
    v.objects.clear();
    v.position.x = 25.5;
    v.tick += 700;
    v.time_ms += 35000;
    assert_eq!(
        brain.decide(&v, &content).unwrap().goal,
        Goal::Revisit(15, 11)
    );
    v.tick += 4;
    v.time_ms += 200;
    let fresh = Brain::new(PROFILES[1], &v).decide(&v, &content).unwrap();
    assert!(matches!(fresh.goal, Goal::Explore(..)));
    v.position.x = 15.5;
    brain.decide(&v, &content).unwrap();
    assert!(brain.hunting.is_none());
    v.position.x = 25.5;
    v.tick += 700;
    v.time_ms += 35000;
    assert!(!matches!(
        brain.decide(&v, &content).unwrap().goal,
        Goal::Revisit(..)
    ));
    v.map.name = "Pirate Cave".into();
    v.tick += 4;
    v.time_ms += 200;
    brain.goal = Goal::Revisit(15, 11);
    assert!(matches!(
        brain.decide(&v, &content).unwrap().goal,
        Goal::Explore(..)
    ));
}

#[test]
fn crews_search_recent_observed_contacts_without_teleporting_to_hidden_peers() {
    let content = content();
    let mut v = view(&content);
    let mut peer = object(2, WIZARD, 19.5, 11.5);
    peer.name = "Biceps".into();
    peer.equipped = v.character.items[..4].to_vec();
    v.objects.push(peer);
    let mut follower = on_adventure(PROFILES[2], &v);
    follower.decide(&v, &content).unwrap();
    v.objects.clear();
    v.tick += 5;
    v.time_ms += 250;
    let search = follower.decide(&v, &content).unwrap();
    assert_eq!(search.goal, Goal::Follow(2));
    assert!(search.movement.is_some());
    assert!(search.journey.is_none());
    v.tick += 300;
    v.time_ms += 15000;
    assert!(matches!(
        follower.decide(&v, &content).unwrap().journey,
        Some(Journey::Nexus)
    ));

    let mut peer = object(2, WIZARD, 19.5, 11.5);
    peer.name = "BattleCrus".into();
    peer.equipped = v.character.items[..4].to_vec();
    v.objects.push(peer);
    let mut leader = on_adventure(PROFILES[1], &v);
    leader.decide(&v, &content).unwrap();
    v.objects.clear();
    v.position.x = 25.5;
    v.tick += 4;
    v.time_ms += 200;
    let search = leader.decide(&v, &content).unwrap();
    assert_eq!(search.goal, Goal::Gather(2));
    assert!(search.movement.is_some());
}

#[test]
fn earned_stat_potions_are_worth_collecting() {
    let c = content();
    let v = view(&c);
    assert!(useful(&c, &v, 0xa21).unwrap());
    assert!(useful(&c, &v, 0xa35).unwrap());
    assert!(useful(&c, &v, 0xa4c).unwrap());
}

#[test]
fn equipped_crews_accept_snake_pit_but_starters_do_not() {
    let c = content();
    let mut v = view(&c);
    v.objects.push(object(2, 0x718, 12.5, 11.5));
    for (id, name) in [(3, "Triceps"), (4, "BattleCrus")] {
        let mut peer = object(id, WIZARD, 12.5, 11.5);
        peer.name = name.into();
        peer.equipped = c.get(WIZARD).unwrap().equipment[..4].to_vec();
        v.objects.push(peer);
    }
    assert!(!matches!(
        on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2)
    ));
    v.character.level = 20;
    v.character.stats[0] = 650;
    v.character.hp = 650;
    v.character.stats[3] = 15;
    v.character.items[0] = c
        .objects
        .values()
        .find(|o| o.slot_type == 17 && o.tier == Some(6))
        .unwrap()
        .kind as i32;
    v.character.items[2] = c
        .objects
        .values()
        .find(|o| o.slot_type == 14 && o.tier == Some(6))
        .unwrap()
        .kind as i32;
    for peer in v.objects.iter_mut().filter(|p| !p.equipped.is_empty()) {
        peer.equipped[0] = v.character.items[0];
        peer.hp = 500;
        peer.max_hp = 500;
        peer.level = Some(20);
    }
    assert_eq!(
        on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2)
    );
    assert_eq!(c.objects[&2714].tier, Some(3));
    v.objects[1].equipped[0] = 2714;
    assert!(!matches!(
        on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2)
    ));
}

#[test]
fn dungeon_rewards_are_collected_before_the_committed_exit() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.objects.push(object(2, 0x71d, 12.5, 11.5));
    let mut bag = object(3, 0x506, 12.5, 11.5);
    bag.contents = vec![0xa21, -1, -1, -1, -1, -1, -1, -1];
    v.objects.push(bag);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Loot(3));
    assert!(matches!(d.item, Some(Action::Swap { .. })));
    assert!(d.journey.is_none());
    v.character.items[0] = 2717;
    v.character.items[4..].fill(2717);
    v.character.items[6] = 2711;
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(matches!(
        d.item,
        Some(Action::Swap {
            from: Slot {
                object: 3,
                kind: 0xa21,
                ..
            },
            to: Slot {
                slot: 6,
                kind: 2711,
                ..
            }
        })
    ));
    assert!(d.journey.is_none());
    v.character.items[6] = 0xa21;
    assert!(matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().item,
        Some(Action::Use { slot: 6, .. })
    ));
    v.character.items[4..].fill(2594);
    assert!(
        progression::pickup(&v, &c, &v.objects[1])
            .unwrap()
            .is_none()
    );
    v.objects.remove(1);
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2)
    );
}

#[test]
fn permanent_potion_uses_own_base_not_equipment_bonuses_and_respects_cap() {
    let c = content();
    let mut v = view(&c);
    v.character.items[4] = 0xa21;
    let cap = c.classes[&WIZARD].caps[4];
    v.character.stats[4] = cap + 10;
    v.character.base_stats[4] = cap - 1;
    assert!(matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().item,
        Some(Action::Use { slot: 4, .. })
    ));
    v.character.base_stats[4] = cap;
    assert!(progression::consume(&v, &c).unwrap().is_none());
    v.lifecycle.busy = true;
    assert!(
        Brain::new(PROFILES[1], &v)
            .decide(&v, &c)
            .unwrap()
            .item
            .is_none()
    );
}

#[test]
fn banking_uses_only_observed_chest_slots_and_full_vault_does_not_discard_items() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Vault".into();
    let mut chest = object(2, 0x504, 12.5, 11.5);
    chest.contents = vec![-1; 8];
    v.objects.push(chest);
    v.character.items[4] = 0xa21;
    v.character.base_stats[4] = c.classes[&WIZARD].caps[4];
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(matches!(
        d.item,
        Some(Action::Swap {
            from: Slot {
                object: 1,
                slot: 4,
                kind: 0xa21
            },
            to: Slot {
                object: 2,
                slot: 0,
                kind: -1
            }
        })
    ));
    v.objects[0].contents = vec![0xa21; 8];
    let mut brain = Brain::new(PROFILES[1], &v);
    let d = brain.decide(&v, &c).unwrap();
    assert!(d.item.is_none());
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    assert!(brain.next_bank > v.time_ms);
    v.objects.clear();
    assert!(progression::bank(&v, &c, &Grid::new(&v)).unwrap().is_none());
}

#[test]
fn native_quest_drives_only_known_routes_and_does_not_reveal_terrain() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 3;
    v.map.width = 512;
    v.map.height = 512;
    let mut target = object(2, 0x609, 150.5, 150.5);
    target.max_hp = c.objects[&target.kind].hp;
    v.objects.push(target);
    v.quest = Some(2);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(2));
    let destination = d.movement.unwrap();
    assert!(Grid::new(&v).segment(v.position, destination));
    assert!(destination.distance(v.position) <= 4.001);
    assert!(v.tiles.iter().any(|t| (t.x, t.y) == tile(destination)));
    v.objects.clear();
    assert!(!matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Hunt(_)
    ));
}

#[test]
fn beginners_escape_outmatched_encounters_and_shields_do_not_waste_spells() {
    let c = content();
    let mut v = view(&c);
    let mut god = object(2, 0x654, 13.5, 11.5);
    god.max_hp = c.objects[&god.kind].hp;
    v.objects.push(god);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Recover);
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    v.character.stats[0] = 650;
    v.character.hp = 650;
    v.objects[0].condition_mask = Some(crate::condition::bit(crate::condition::INVULNERABLE));
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(d.aim.is_none());
    assert!(d.item.is_none());
}

#[tokio::test]
async fn prepared_bot_banks_through_normal_durable_inventory_and_death_rules() {
    use crate::store::{InventoryWrite, Store};
    let c = std::sync::Arc::new(content());
    let store = Store::open(std::path::Path::new(":memory:"), &c).unwrap();
    let account = store.list("prepared-bot-banking".into()).await.unwrap();
    let mut character = store
        .create(account.id, c.get(WIZARD).unwrap())
        .await
        .unwrap();
    character.base[4] = c.classes[&WIZARD].caps[4];
    character.items[4] = 0xa21;
    character.revision = store
        .exchange(
            vec![InventoryWrite {
                id: character.id,
                account: account.id,
                revision: character.revision,
                items: character.items.clone(),
            }],
            None,
        )
        .await
        .unwrap()[0];
    store.progress(character.clone()).await.unwrap();
    let mut directory = crate::world::Directory::test_with_seed(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data"),
        c.clone(),
        16,
        42,
    )
    .unwrap();
    let scope = directory.instance("vault", Some(account.id)).unwrap();
    let w = directory.worlds.get_mut(&scope).unwrap();
    w.load_vault(store.vault(account.id).await.unwrap())
        .unwrap();
    let chest = w.entities.values().find(|e| e.kind == 0x504).unwrap();
    let position = chest.pos;
    let (packets, _rx) = tokio::sync::mpsc::channel(OUTPUT_CAPACITY);
    let (lost, _) = tokio::sync::watch::channel(false);
    let id = w
        .enter_autonomous(character.clone(), crate::world::Output { packets, lost })
        .unwrap();
    w.players.get_mut(&id).unwrap().pos = position;
    w.updates(id, false).unwrap();
    let view = directory.player_view(id).unwrap();
    let decision = Brain::new(PROFILES[1], &view).decide(&view, &c).unwrap();
    let plan = directory
        .worlds
        .get_mut(&scope)
        .unwrap()
        .autonomous_action(id, decision.item.unwrap())
        .unwrap()
        .unwrap();
    assert!(
        store
            .vault(account.id)
            .await
            .unwrap()
            .items
            .iter()
            .all(|i| *i == -1)
    );
    assert_eq!(
        store.load(account.id, character.id).await.unwrap().items[4],
        0xa21
    );
    let revision = store
        .exchange(
            vec![InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision,
                items: plan.items.clone(),
            }],
            plan.vault.clone(),
        )
        .await
        .unwrap()[0];
    directory
        .worlds
        .get_mut(&scope)
        .unwrap()
        .finish_inventory(plan, Ok(revision))
        .unwrap();
    assert_eq!(store.vault(account.id).await.unwrap().items[0], 0xa21);
    assert_eq!(
        store.load(account.id, character.id).await.unwrap().items[4],
        -1
    );
    let mut dead = directory.worlds[&scope].players[&id].character.clone();
    dead.hp = 0;
    dead.alive = false;
    store
        .death(dead, "prepared banking death".into())
        .await
        .unwrap();
    assert!(store.load(account.id, character.id).await.is_err());
    let fresh = store
        .create(account.id, c.get(WIZARD).unwrap())
        .await
        .unwrap();
    assert_ne!(fresh.id, character.id);
    assert_eq!(fresh.items, c.get(WIZARD).unwrap().equipment);
    assert_eq!(store.vault(account.id).await.unwrap().items[0], 0xa21);
}

#[test]
fn unknown_quest_route_targets_a_frontier_not_a_known_dead_end() {
    let mut v = view(&content());
    v.map.width = 100;
    v.map.height = 100;
    let mut grid = Grid::new(&v);
    // The closest known square is a sealed alcove, not a route into undiscovered rooms.
    for y in 0..29 {
        grid.walkable.remove(&(20, y));
    }
    let mut from = Pos { x: 18.5, y: 10.5 };
    for _ in 0..30 {
        let point = grid.towards(from, Pos { x: 80.5, y: 10.5 }).unwrap();
        assert!(grid.segment(from, point));
        from = point;
        if from.y > 25. {
            break;
        }
    }
    assert!(from.y > 25.);
}

#[test]
fn observed_destructible_cover_is_shot_without_fabricating_a_route_or_wasting_a_spell() {
    let c = content();
    let mut v = view(&c);
    let mut wall = object(2, 0x172, 15.5, 11.5);
    wall.hp = 5;
    wall.blocks = true;
    assert!(c.objects[&wall.kind].destructible);
    v.objects.push(wall);
    let grid = Grid::new(&v);
    let d = progression::clear_cover(&v, &c, &grid, Pos { x: 25.5, y: 11.5 }, 8., false).unwrap();
    assert_eq!(d.goal, Goal::Break(2));
    assert!(d.aim.is_some() && d.movement.is_none() && d.item.is_none());
    v.objects[0].kind = 0x1ed;
    assert!(
        progression::clear_cover(&v, &c, &Grid::new(&v), Pos { x: 25.5, y: 11.5 }, 8., false)
            .is_none()
    );
}

#[test]
fn nearby_durable_threat_outscores_an_endless_stream_of_small_minions() {
    let c = content();
    let mut v = view(&c);
    v.character.hp = 600;
    v.character.stats[0] = 600;
    let mut minion = object(2, 0x223, 13.5, 11.5);
    minion.hp = 5;
    minion.max_hp = 5;
    let mut python = object(3, 0x228, 17.5, 11.5);
    python.hp = 500;
    python.max_hp = 500;
    v.objects.extend([minion, python]);
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Fight(3)
    );
}

#[test]
fn exhausted_healing_reserves_trigger_retreat_before_the_next_large_volley() {
    let c = content();
    let mut v = view(&c);
    v.character.hp = 34;
    v.character.items[4..].fill(-1);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    assert!(d.item.is_none());
    v.character.items[4] = 0xa22;
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(d.journey.is_none());
    assert!(matches!(d.item, Some(Action::Use { slot: 4, .. })));
}

#[test]
fn dungeon_push_fires_at_visible_threats_without_camping_endless_reinforcements() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.map.width = 200;
    v.map.height = 200;
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    let mut boss = object(2, 0x917, 150.5, 150.5);
    boss.max_hp = 7500;
    boss.hp = 7500;
    let mut snake = object(3, 0x223, 14.5, 11.5);
    snake.max_hp = 5;
    snake.hp = 5;
    v.objects.extend([boss, snake]);
    v.quest = Some(2);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(2));
    assert!(d.movement.is_some());
    assert!(d.aim.unwrap().abs() < 0.001);
    assert!(d.item.is_none());
}

#[test]
fn a_visible_crews_destination_outweighs_the_followers_different_quest_marker() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 20;
    v.map.width = 200;
    v.map.height = 200;
    let mut leader = object(2, WIZARD, 18.5, 11.5);
    leader.name = "Biceps".into();
    leader.equipped = v.character.items[..4].to_vec();
    let target = object(3, 0x609, 150.5, 150.5);
    v.objects.extend([leader, target]);
    v.quest = Some(3);
    let mut brain = on_adventure(PROFILES[0], &v);
    assert_eq!(brain.decide(&v, &c).unwrap().goal, Goal::Follow(2));
    v.objects.remove(0);
    v.tick += 300;
    v.time_ms += 15000;
    assert_eq!(brain.decide(&v, &c).unwrap().goal, Goal::Hunt(3));
}

#[test]
fn a_leader_gathers_an_observed_straggler_before_pursuing_another_distant_quest() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 20;
    v.map.width = 200;
    v.map.height = 200;
    let mut peer = object(2, WIZARD, 24.5, 11.5);
    peer.name = "BattleCrus".into();
    peer.equipped = v.character.items[..4].to_vec();
    v.objects.extend([peer, object(3, 0x609, 150.5, 150.5)]);
    v.quest = Some(3);
    let d = on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Gather(2));
    assert!(d.movement.is_none());
}

#[test]
fn healthy_dodging_keeps_pressure_and_spell_use_on_the_visible_boss() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    v.character.mp = c.objects[&(v.character.items[1] as u16)].mana_cost;
    let mut boss = object(2, 0x917, 17.5, 11.5);
    boss.hp = 7500;
    boss.max_hp = 7500;
    let mut minion = object(3, 0x223, 14.5, 11.5);
    minion.hp = 5;
    minion.max_hp = 5;
    v.objects.extend([boss, minion]);
    v.quest = Some(2);
    v.projectiles.push(VisibleProjectile {
        owner: 9,
        id: 0,
        start: Pos { x: 9.5, y: 11.5 },
        position: Pos { x: 9.5, y: 11.5 },
        angle: 0.,
        speed: 50.,
        lifetime_ms: 1000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
    });
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Dodge);
    assert!(d.movement.is_some() && d.aim.is_some());
    assert!(matches!(d.item, Some(Action::Use {slot: 1,x,y}) if x==17.5 && y==11.5));
}

#[test]
fn dungeon_room_fighting_preserves_the_last_spell_for_the_published_boss() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.map.width = 200;
    v.map.height = 200;
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    v.character.mp = c.objects[&(v.character.items[1] as u16)].mana_cost;
    let mut boss = object(2, 0x917, 150.5, 150.5);
    boss.hp = 7500;
    boss.max_hp = 7500;
    let mut parent = object(3, 0x226, 15.5, 11.5);
    parent.hp = 200;
    parent.max_hp = 200;
    v.objects.extend([boss, parent]);
    v.quest = Some(2);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(d.aim.is_some() && d.item.is_none());
    v.objects.pop();
    v.objects[0].position = Pos { x: 15.5, y: 11.5 };
    assert!(matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().item,
        Some(Action::Use { slot: 1, .. })
    ));
}

#[test]
fn equipped_adventurers_venture_inland_over_known_terrain_not_hidden_maps() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    v.character.stats[3] = 10;
    v.character.items[0] = 2715;
    v.map.width = 512;
    v.map.height = 512;
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Venture);
    assert!(Grid::new(&v).segment(v.position, d.movement.unwrap()));
    v.character.level = 1;
    assert!(!matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Venture
    ));
}

#[test]
fn carried_mana_is_used_without_fabricating_resources_or_bypassing_quiet() {
    let c = content();
    let mut v = view(&c);
    v.character.mp = 0;
    assert!(progression::mana(&v, &c).unwrap().is_none());
    v.character.items[5] = 0xa23;
    assert!(useful(&c, &v, 0xa23).unwrap());
    assert!(matches!(
        progression::mana(&v, &c).unwrap(),
        Some(Action::Use { slot: 5, .. })
    ));
    v.character.conditions = crate::condition::bit(crate::condition::QUIET);
    assert!(progression::mana(&v, &c).unwrap().is_none());
    v.character.conditions = 0;
    v.character.mp = v.character.stats[1];
    assert!(progression::mana(&v, &c).unwrap().is_none());
}

#[test]
fn a_distant_dungeon_objective_does_not_outrank_observed_needed_mana() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.map.width = 200;
    v.map.height = 200;
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    v.character.stats[1] = 200;
    v.character.mp = 0;
    let mut boss = object(2, 0x917, 150.5, 150.5);
    boss.hp = 7500;
    boss.max_hp = 7500;
    let mut bag = object(3, 0x500, 16.5, 11.5);
    bag.contents = vec![0xa23, -1, -1, -1, -1, -1, -1, -1];
    v.objects.extend([boss, bag]);
    v.quest = Some(2);
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Loot(3)
    );
    v.character.mp = 200;
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Hunt(2)
    );
}

#[test]
fn cleared_dungeons_keep_stat_rewards_but_do_not_trap_scavengers_in_reinforcements() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.character.level = 20;
    v.character.stats[0] = 600;
    v.character.hp = 350;
    let exit = object(2, 0x71d, 18.5, 11.5);
    let mut minion = object(3, 0x223, 15.5, 11.5);
    minion.hp = 5;
    minion.max_hp = 5;
    let mut bag = object(4, 0x500, 13., 11.5);
    bag.contents = vec![2621, -1, -1, -1, -1, -1, -1, -1];
    v.objects.extend([exit, minion, bag]);
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2)
    );
    v.objects[2].contents[0] = 0xa21;
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Loot(4)
    );
}

#[test]
fn moving_enemies_need_weapon_lead_but_nova_originates_at_the_observed_target() {
    let c = content();
    let v = view(&c);
    let grid = Grid::new(&v);
    let mut enemy = object(2, 0x226, 17.5, 11.5);
    enemy.hp = 500;
    enemy.max_hp = 500;
    let mut d = Decision {
        goal: Goal::Fight(2),
        movement: None,
        aim: None,
        item: None,
        journey: None,
        chat: None,
    };
    combat::attack(&v, &c, &grid, &enemy, Pos { x: 0., y: 2. }, &mut d).unwrap();
    assert!(d.aim.is_some_and(|aim| aim > 0.));
    assert!(
        matches!(d.item, Some(Action::Use {slot:1,x,y}) if x==enemy.position.x && y==enemy.position.y)
    );
}

#[test]
fn admitted_dungeon_intentions_beat_optional_nearby_fighting() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    v.character.stats[3] = 10;
    v.character.items[0] = 2715;
    v.objects.push(object(2, 0x718, 18.5, 11.5));
    let mut enemy = object(3, 0x226, 17.5, 11.5);
    enemy.hp = 500;
    enemy.max_hp = 500;
    v.objects.push(enemy);
    for (id, name) in [(4, "Triceps"), (5, "BattleCrus")] {
        let mut peer = object(id, WIZARD, 18.5, 12.5);
        peer.name = name.into();
        peer.hp = 500;
        peer.max_hp = 500;
        peer.level = Some(20);
        peer.equipped = vec![2715, 2773, 2654, -1];
        v.objects.push(peer);
    }
    assert_eq!(
        on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2)
    );
}

#[test]
fn dungeon_ready_crews_search_a_central_region_instead_of_chasing_distant_quests() {
    let c = content();
    let mut v = view(&c);
    v.map.width = 512;
    v.map.height = 512;
    v.position = Pos { x: 256.5, y: 267.5 };
    v.tiles = (260..301)
        .flat_map(|y| {
            (240..281).map(move |x| Tile {
                x,
                y,
                ground: 0,
                ground_speed: 1.,
            })
        })
        .collect::<Vec<_>>()
        .into();
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    v.character.stats[3] = 10;
    v.character.items[0] = 2715;
    let mut quest = object(2, 0x668, 100.5, 400.5);
    quest.hp = 7000;
    quest.max_hp = 7000;
    v.objects.push(quest);
    v.quest = Some(2);
    let mut brain = Brain::new(PROFILES[1], &v);
    let d = brain.decide(&v, &c).unwrap();
    assert!(matches!(d.goal, Goal::Explore(..)));
    assert!(Grid::new(&v).segment(v.position, d.movement.unwrap()));
    v.position = Pos { x: 256.5, y: 322.5 };
    v.tick += 200;
    v.time_ms += 10000;
    v.tiles = (302..343)
        .flat_map(|y| {
            (240..281).map(move |x| Tile {
                x,
                y,
                ground: 0,
                ground_speed: 1.,
            })
        })
        .collect::<Vec<_>>()
        .into();
    assert!(matches!(
        brain.decide(&v, &c).unwrap().goal,
        Goal::Explore(..)
    ));
    v.character.items[0] = 2711;
    assert_eq!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Hunt(2)
    );
}

#[test]
fn a_dungeon_objective_advances_through_reinforcements_while_returning_fire() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Snake Pit".into();
    v.map.width = 200;
    v.map.height = 200;
    v.character.level = 20;
    v.character.hp = 600;
    v.character.stats[0] = 600;
    let mut boss = object(2, 0x917, 150.5, 150.5);
    boss.hp = 7500;
    boss.max_hp = 7500;
    boss.condition_mask = Some(crate::condition::bit(crate::condition::STASIS_IMMUNE));
    let mut guard = object(3, 0x226, 13.5, 11.5);
    guard.hp = 500;
    guard.max_hp = 500;
    v.objects.extend([boss, guard]);
    v.quest = Some(2);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(2));
    assert!(d.movement.is_some() && d.aim.is_some());
}
