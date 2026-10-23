use super::tests::{content, object, on_adventure, tiles, view};
use super::*;
use crate::content::WIZARD;
use crate::world::view::VisibleProjectile;

#[test]
fn exploration_remains_available_when_a_higher_ranked_route_is_blocked() {
    let c = content();
    let mut v = view(&c);
    let mut ally = object(4, WIZARD, 22.5, 11.5);
    ally.name = "Biceps".into();
    ally.equipped = vec![0xa97];
    v.objects.push(ally);
    v.player_markers.push(crate::world::view::PlayerMarker {
        id: 4,
        account: 4,
        name: "Biceps".into(),
        position: v.objects[0].position,
    });
    assert_eq!(
        on_adventure(PROFILES[2], &v).decide(&v, &c).unwrap().goal,
        Goal::Follow(4)
    );
    v.tiles = tiles(40, 40, v.tiles.points().filter(|(x, _)| *x != 15));
    let d = on_adventure(PROFILES[2], &v).decide(&v, &c).unwrap();
    assert!(matches!(d.goal, Goal::Explore(x, _) if x < 15));
}
#[test]
fn dodge_predictions_use_actual_movement_conditions_and_ground() {
    use crate::content::{PARALYZED, SLOWED, SPEEDY};
    let c = content();
    let mut v = view(&c);
    v.character.stats[4] = 75;
    let geometry =
        crate::navigation::Geometry::new((40, 40), vec![0; 1600], &BTreeMap::from([(0, 0.6)]));
    v.tiles = Arc::new(crate::navigation::Tiles::new(geometry, v.tiles.points()));
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
    let grid = Grid::terrain(&v);
    assert!(
        combat::movement(
            &v,
            &grid,
            &[],
            Some(Pos { x: 15.5, y: 11.5 }),
            7.,
            false,
            &combat::Perception::attend(&v, &pursuits::Cognition::default(), false)
        )
        .is_none()
    );
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
    loot.contents = vec![0xa23];
    v.objects.push(loot);
    let mut ally = object(4, WIZARD, 22.5, 11.5);
    ally.name = "Biceps".into();
    ally.equipped = vec![0xa97];
    v.objects.push(ally);
    let goals: Vec<_> = PROFILES[..3]
        .iter()
        .map(|p| on_adventure(*p, &v).decide(&v, &content).unwrap().goal)
        .collect();
    assert_eq!(goals, vec![Goal::Recover, Goal::Loot(3), Goal::Loot(3)]);
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
    assert!(progression::priority(&v, &content, 0xa01, false, None).unwrap() == 0);
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
    assert!(matches!(
        brain.decide(&v, &content).unwrap().goal,
        Goal::Explore(..)
    ));
    assert!(brain.companion.is_none());
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
    assert!(
        combat::clearance(
            &v,
            crossing,
            &combat::Perception::attend(&v, &pursuits::Cognition::default(), false)
        ) < 0.2
    );
    assert!(
        combat::clearance(
            &v,
            away,
            &combat::Perception::attend(&v, &pursuits::Cognition::default(), false)
        ) > 0.6
    );
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
    v.character.mp = 180;
    v.character.stats[1] = 180;
    v.character.base_stats[1] = 180;
    let mut brain = Brain::new(PROFILES[3], &v);
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
    v.tick += 6;
    v.time_ms += 300;
    v.objects[0].hp = 40;
    assert!(
        Brain::new(PROFILES[0], &v)
            .decide(&v, &content)
            .unwrap()
            .item
            .is_none(),
        "stranger injury alone does not compel an unwilling priest"
    );
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
    v.character.mp = 180;
    v.objects[0].position.x = 25.5;
    assert!(brain.decide(&v, &content).unwrap().item.is_none());
}
#[test]
fn earned_stat_potions_are_worth_collecting() {
    let c = content();
    let v = view(&c);
    assert!(progression::priority(&v, &c, 0xa21, false, None).unwrap() > 0);
    assert!(progression::priority(&v, &c, 0xa35, false, None).unwrap() > 0);
    assert!(progression::priority(&v, &c, 0xa4c, false, None).unwrap() > 0);
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
    assert_eq!(
        on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(2),
        "a less-equipped peer cannot veto another account's eligible travel"
    );
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
        progression::pickup(&v, &c, &v.objects[1], None)
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
    assert!(d.journey.is_none());
    assert!(
        d.aim.is_some(),
        "private equipment trial precedes renewed purpose"
    );
    let retained_items = v.character.items.clone();
    v.elapsed_ms += 1500;
    v.time_ms += 1500;
    v.tick += 30;
    let d = brain.decide(&v, &c).unwrap();
    assert!(d.item.is_none());
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    assert_eq!(v.character.items, retained_items);
    assert!(brain.next_bank > v.time_ms);
    v.objects.clear();
    assert!(
        progression::bank(&v, &c, &Grid::terrain(&v), None)
            .unwrap()
            .is_none()
    );
}
#[test]
fn carried_mana_is_used_without_fabricating_resources_or_bypassing_quiet() {
    let c = content();
    let mut v = view(&c);
    v.character.mp = 0;
    assert!(progression::mana(&v, &c).unwrap().is_none());
    v.character.items[5] = 0xa23;
    assert!(progression::priority(&v, &c, 0xa23, false, None).unwrap() > 0);
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
fn moving_enemies_need_weapon_lead_but_nova_originates_at_the_observed_target() {
    let c = content();
    let v = view(&c);
    let grid = Grid::terrain(&v);
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
    combat::attack(
        &v,
        &c,
        &grid,
        &enemy,
        Pos { x: 0., y: 2. },
        &combat::Perception::attend(&v, &pursuits::Cognition::default(), false),
        &mut d,
    )
    .unwrap();
    assert!(d.aim.is_some_and(|aim| aim > 0.));
    assert!(
        matches!(d.item, Some(Action::Use {slot:1,x,y}) if x==enemy.position.x && y==enemy.position.y)
    );
}
