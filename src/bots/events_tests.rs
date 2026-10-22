use super::tests::{content, object, view};
use super::*;

fn equipped(v: &mut PlayerView) {
    v.character.level = 20;
    v.character.hp = 670;
    v.character.mp = 385;
    v.character.stats = [670, 385, 75, 25, 50, 75, 40, 60];
    v.character.base_stats = v.character.stats;
    v.character.items = vec![2824, 2852, 2821, 2765, 0xa22, 0xa22, -1, -1, -1, -1, -1, -1];
}
#[test]
fn community_events_published_lab_and_chamber_openings_require_own_readiness() {
    let c = content();
    for (map, portal) in [
        ("Realm", 0x890),
        ("Oryx's Castle", 0xd7b),
        ("Oryx's Chamber", 0x700),
    ] {
        let mut v = view(&c);
        equipped(&mut v);
        v.map.name = map.into();
        v.objects.push(object(7, portal, 12.5, 11.5));
        let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
        assert!(
            matches!(d.journey, Some(Journey::Portal(7))),
            "ready actor ignored published {map} opening: {:?}",
            d.goal
        );
        v.character.level = 1;
        let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
        assert!(!matches!(d.journey, Some(Journey::Portal(7))));
        v.character.level = 20;
        v.objects[0].usable = false;
        let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
        assert!(!matches!(d.journey, Some(Journey::Portal(7))));
    }
}
#[test]
fn community_events_unready_castle_actor_exits_instead_of_following_frontperson() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Oryx's Castle".into();
    let mut peer = object(5, crate::content::WIZARD, 22.5, 11.5);
    peer.equipped = c.objects[&crate::content::WIZARD].equipment.clone();
    v.objects.push(peer);
    let d = Brain::new(PROFILES[2], &v).decide(&v, &c).unwrap();
    assert!(
        matches!(d.journey, Some(Journey::Nexus)),
        "unready actor blindly advances: {:?}",
        d.goal
    );
}
#[test]
fn community_events_ready_actor_can_fight_actual_oryx_not_harmless_pool() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Chamber".into();
    let mut boss = object(6, crate::endgame::ORYX, 20.5, 11.5);
    boss.hp = c.objects[&boss.kind].hp;
    boss.max_hp = boss.hp;
    assert!(boss.max_hp > v.character.stats[0] * 50);
    v.objects.push(boss);
    assert!(progression::can_fight(&v, &c, &v.objects[0]));
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(
        matches!(d.goal, Goal::Fight(6) | Goal::Hunt(6)),
        "{:?}",
        d.goal
    );
}

#[test]
fn community_events_remedy_knowledge_changes_route_and_unknown_status_seeks_help() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Mad Lab".into();
    v.character.conditions =
        crate::condition::bit(crate::condition::HEXED) | crate::content::STUNNED;
    v.objects
        .push(object(12, crate::dungeon::lab::DEHEXXER, 16.5, 11.5));
    let mut experienced = Brain::new(PROFILES[0], &v);
    let mut unfamiliar = Brain::new(PROFILES[0], &v);
    unfamiliar.mind.events.knows_blue = false;
    let d = experienced.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Remedy(12));
    assert!(d.movement.is_some());
    assert_eq!(d.chat, Some("hexed; trying blue water"));
    let d = unfamiliar.decide(&v, &c).unwrap();
    assert_ne!(d.goal, Goal::Remedy(12));
    assert_eq!(d.chat, Some("hexed; need help"));
    v.character.conditions = 0;
    v.tick += 1;
    let d = experienced.decide(&v, &c).unwrap();
    assert_ne!(d.goal, Goal::Remedy(12));
    assert_ne!(d.chat, Some("hexed; trying blue water"));
}
#[test]
fn community_events_known_green_pool_avoided_without_shared_hidden_navigation() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Mad Lab".into();
    v.objects
        .push(object(9, crate::dungeon::lab::HEXXER, 15.5, 11.5));
    let mut b = Brain::new(PROFILES[0], &v);
    assert!(!b.navigation_grid(&v, &c, &[]).walkable.contains(&(15, 11)));
    b.mind.events.knows_blue = false;
    assert!(b.navigation_grid(&v, &c, &[]).walkable.contains(&(15, 11)));
    v.objects.clear();
    b.mind.events.knows_blue = true;
    assert!(b.navigation_grid(&v, &c, &[]).walkable.contains(&(15, 11)));
}
fn clear_scene(c: &Content) -> PlayerView {
    let mut v = view(c);
    equipped(&mut v);
    v.map.name = "Mad Lab".into();
    v.objects
        .push(object(6, crate::dungeon::lab::BOSS, 12.5, 11.5));
    let mut peer = object(8, crate::content::WIZARD, 13.5, 11.5);
    peer.equipped = c.objects[&crate::content::WIZARD].equipment.clone();
    v.objects.push(peer);
    v
}
#[test]
fn community_events_no_loot_speech_requires_witnessed_clear_reward_site_and_audience() {
    let c = content();
    let mut v = clear_scene(&c);
    let mut e = events::Events::default();
    e.observe(&v);
    v.objects.remove(0);
    e.observe(&v);
    v.time_ms += 2000;
    let grid = Grid::terrain(&v);
    assert!(
        e.decide(&v, &c, &grid, 4).unwrap().is_none(),
        "boss disappearance fabricated a clear"
    );
    v.objects.push(object(7, 0x71d, 14.5, 11.5));
    e.observe(&v);
    v.time_ms += 1000;
    let before = e.clone();
    v.objects.retain(|p| p.account.is_none());
    assert_ne!(
        e.decide(&v, &c, &grid, 4).unwrap().and_then(|d| d.chat),
        Some("no drop here for me")
    );
    e = before;
    let mut peer = object(8, crate::content::WIZARD, 13.5, 11.5);
    peer.equipped = vec![2824];
    v.objects.push(peer);
    assert_eq!(
        e.decide(&v, &c, &grid, 4).unwrap().unwrap().chat,
        Some("no drop here for me")
    );
    assert_ne!(
        e.decide(&v, &c, &grid, 4).unwrap().and_then(|d| d.chat),
        Some("no drop here for me")
    );
    let mut late = events::Events::default();
    late.observe(&v);
    assert_ne!(
        late.decide(&v, &c, &grid, 4).unwrap().and_then(|d| d.chat),
        Some("no drop here for me")
    );
}
#[test]
fn community_events_loot_report_requires_committed_own_pickup_not_an_observed_bag() {
    let c = content();
    let mut v = clear_scene(&c);
    let mut e = events::Events::default();
    e.observe(&v);
    v.objects.remove(0);
    v.objects.push(object(7, 0x71d, 14.5, 11.5));
    e.observe(&v);
    v.time_ms += 1000;
    let a = Action::Swap {
        from: Slot {
            object: 99,
            slot: 0,
            kind: 0xa35,
        },
        to: own(&v, 6),
    };
    let mut bag = object(99, 0x500, 12.5, 11.5);
    bag.contents = vec![0xa35];
    v.objects.push(bag);
    e.collecting(&v, &a);
    assert!(e.committed(&v).is_err());
    e.rejected();
    e.observe(&v);
    assert_ne!(
        e.decide(&v, &c, &Grid::terrain(&v), 4)
            .unwrap()
            .and_then(|d| d.chat),
        Some("got wis")
    );
    e.collecting(&v, &a);
    v.character.items[6] = 0xa35;
    v.objects.retain(|p| p.id != 99);
    v.character.inventory_revision += 1;
    e.committed(&v).unwrap();
    e.observe(&v);
    assert_eq!(
        e.decide(&v, &c, &Grid::terrain(&v), 4)
            .unwrap()
            .unwrap()
            .chat,
        Some("got wis")
    );
    assert_eq!(
        serde_json::to_value(&e).unwrap()["pickups"][0]["kind"],
        0xa35
    );
    e.validate().unwrap();
}
#[test]
fn community_events_next_entrance_hp_readiness_uses_owned_health_before_declining() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Castle".into();
    v.character.hp = 543;
    v.character.stats[0] = 810;
    v.character.mp = 68;
    v.character.items[4..8].fill(0xa22);
    v.objects.push(object(7, 0xd7b, 12.5, 11.5));
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(
        matches!(d.item, Some(Action::Use { slot: 4, .. })),
        "usable own HP ignored: {:?}",
        d.goal
    );
    assert!(d.journey.is_none());
    assert_eq!(v.character.hp, 543);
    v.character.items[4..8].fill(-1);
    assert!(matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().journey,
        Some(Journey::Nexus)
    ));
}
#[test]
fn community_events_known_blue_remains_a_dodge_objective_without_forcing_status_clear() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Mad Lab".into();
    v.character.conditions = crate::condition::bit(crate::condition::HEXED);
    v.objects
        .push(object(7, crate::dungeon::lab::DEHEXXER, 16.5, 11.5));
    v.projectiles.push(crate::world::view::VisibleProjectile {
        owner: 99,
        id: 0,
        start: Pos { x: 11.5, y: 9.5 },
        position: Pos { x: 11.5, y: 9.5 },
        angle: std::f32::consts::FRAC_PI_2,
        speed: 100.,
        lifetime_ms: 2000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
    });
    let d = Brain::new(PROFILES[0], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Remedy(7));
    assert!(d.movement.is_some());
    assert_ne!(v.character.conditions, 0);
    let mut unknown = Brain::new(PROFILES[0], &v);
    unknown.mind.events.knows_blue = false;
    assert_ne!(unknown.decide(&v, &c).unwrap().goal, Goal::Remedy(7));
    v.tiles = super::tests::tiles(
        v.map.width,
        v.map.height,
        v.tiles.points().filter(|(x, _)| *x != 13),
    );
    assert_ne!(
        Brain::new(PROFILES[0], &v).decide(&v, &c).unwrap().goal,
        Goal::Remedy(7)
    );
    v.character.hp = 10;
    v.character.items[4] = -1;
    v.character.items[5] = -1;
    assert!(matches!(
        Brain::new(PROFILES[0], &v).decide(&v, &c).unwrap().journey,
        Some(Journey::Nexus)
    ));
}
#[test]
fn community_events_opener_readiness_wait_is_bounded_and_never_grants_entry_or_resources() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Chamber".into();
    v.character.items[4] = -1;
    v.character.items[5] = -1;
    v.character.items[10] = i32::from(crate::endgame::INCANTATION);
    v.character.mp = 0;
    v.objects
        .push(object(7, crate::endgame::LOCKED_CELLAR, 12.5, 11.5));
    let mut b = Brain::new(PROFILES[1], &v);
    let d = b.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Recover);
    assert!(d.item.is_none());
    assert!(d.journey.is_none());
    assert_eq!(v.character.mp, 0);
    v.time_ms += 30000;
    v.tick += 1;
    let d = b.decide(&v, &c).unwrap();
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    assert!(d.item.is_none());
    assert_eq!(v.character.mp, 0);
}
#[test]
fn community_events_held_opener_approaches_only_observed_reachable_locked_cellar() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Chamber".into();
    v.character.items[10] = i32::from(crate::endgame::INCANTATION);
    v.objects
        .push(object(7, crate::endgame::LOCKED_CELLAR, 20.5, 11.5));
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Portal(7));
    assert!(d.movement.is_some_and(|p| p.x > v.position.x));
    assert!(d.item.is_none());
    assert!(d.journey.is_none());
    v.objects.clear();
    assert_ne!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(7)
    );
    v.objects
        .push(object(7, crate::endgame::LOCKED_CELLAR, 20.5, 11.5));
    v.character.items[10] = -1;
    assert_ne!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().goal,
        Goal::Portal(7)
    );
}
#[test]
fn community_events_bounded_attention_prioritizes_observed_incoming_shot_not_oldest_departing_shot()
{
    let c = content();
    let mut v = view(&c);
    v.map.name = "Oryx's Chamber".into();
    let shot = |owner, id, x, y, angle| crate::world::view::VisibleProjectile {
        owner,
        id,
        start: Pos { x, y },
        position: Pos { x, y },
        angle,
        speed: 100.,
        lifetime_ms: 2000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
    };
    v.projectiles = vec![
        shot(9, 0, 2.5, 11.5, std::f32::consts::PI),
        shot(10, 0, 11.5, 9.5, std::f32::consts::FRAC_PI_2),
    ];
    let cognition = pursuits::Cognition {
        attention: 1,
        ..Default::default()
    };
    let attended = combat::Perception::attend(&v, &cognition, false);
    assert!(
        combat::clearance(&v, v.position, &attended) < 0.2,
        "event attention spent on departing shot"
    );
    v.map.name = "Realm".into();
    let attended = combat::Perception::attend(&v, &cognition, false);
    assert!(
        combat::clearance(&v, v.position, &attended) > 4.,
        "non-event attention semantics changed"
    );
}
#[test]
fn community_events_earned_chamber_route_uses_own_supply_readiness_not_incidental_minion_clear() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Castle".into();
    v.character.mp = 0;
    v.objects.push(object(7, 0xd7b, 12.5, 11.5));
    v.objects.push(object(8, 0x205, 14.5, 11.5));
    assert!(matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().journey,
        Some(Journey::Portal(7))
    ));
    v.character.items[4] = -1;
    v.character.items[5] = -1;
    assert!(!matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().journey,
        Some(Journey::Portal(7))
    ));
}
#[test]
fn community_events_purifying_tome_responds_to_own_status_not_full_health() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Castle".into();
    v.character.class = crate::content::PRIEST;
    v.character.items[0] = 0xaf6;
    v.character.items[1] = 0xc09;
    v.character.conditions = crate::content::PARALYZED;
    assert!(matches!(
        social::healing(&v, &c).unwrap(),
        Some(Action::Use { slot: 1, .. })
    ));
    v.character.conditions = 0;
    assert!(social::healing(&v, &c).unwrap().is_none());
    v.character.conditions = crate::content::PARALYZED;
    v.character.mp = 0;
    assert!(social::healing(&v, &c).unwrap().is_none());
}
#[test]
fn community_events_observed_guardian_interrupts_incidental_pursuit_then_expires() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Castle".into();
    let mut guardian = object(6, crate::endgame::GUARDIANS[0], 12.5, 11.5);
    guardian.condition_mask = Some(crate::condition::bit(crate::condition::INVULNERABLE));
    v.objects.push(guardian);
    v.objects.push(object(10, 0x205, 26.5, 11.5));
    let mut b = Brain::new(PROFILES[1], &v);
    let d = b.decide(&v, &c).unwrap();
    assert!(
        !matches!(d.goal, Goal::Fight(10) | Goal::Hunt(10)),
        "incidental pursuit abandons observed shielded objective: {:?}",
        d.goal
    );
    v.objects.clear();
    v.tiles = super::tests::tiles(
        v.map.width,
        v.map.height,
        v.tiles
            .points()
            .chain((0..30).flat_map(|y| (30..40).map(move |x| (x, y)))),
    );
    v.position.x = 35.5;
    v.tick += 1;
    v.time_ms += 300;
    let d = b.decide(&v, &c).unwrap();
    assert!(
        d.movement.is_some_and(|p| p.x < v.position.x),
        "lost-sight objective area not revisited: {:?}",
        d.goal
    );
    v.tick += 1;
    v.time_ms += 20000;
    let d = b.decide(&v, &c).unwrap();
    assert!(!matches!(d.goal, Goal::Survey(12, 11)));
    v.character.level = 1;
    v.tick += 1;
    assert!(matches!(
        b.decide(&v, &c).unwrap().journey,
        Some(Journey::Nexus)
    ));
}
#[test]
fn community_events_cellar_opener_uses_only_held_incantation_at_published_locked_portal() {
    let c = content();
    let mut v = view(&c);
    equipped(&mut v);
    v.map.name = "Oryx's Chamber".into();
    v.objects
        .push(object(7, crate::endgame::LOCKED_CELLAR, 12.5, 11.5));
    v.character.items[6] = i32::from(crate::endgame::INCANTATION);
    let d = Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert!(matches!(d.item, Some(Action::Use { slot: 6, .. })));
    assert!(d.chat.is_none());
    v.character.items[6] = -1;
    assert!(!matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().item,
        Some(Action::Use { slot: 6, .. })
    ));
    v.character.items[6] = i32::from(crate::endgame::INCANTATION);
    v.objects[0].position.x = 25.5;
    assert!(!matches!(
        Brain::new(PROFILES[1], &v).decide(&v, &c).unwrap().item,
        Some(Action::Use { slot: 6, .. })
    ));
}
