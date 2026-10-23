use super::*;
use crate::content::WIZARD;
use crate::world::view::{
    Lifecycle, MapInfo, OwnCharacter, Session, VisibleObject, VisibleProjectile,
};

pub(super) fn content() -> Content {
    Content::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data")).unwrap()
}
pub(super) fn tiles(
    width: i32,
    height: i32,
    points: impl IntoIterator<Item = TilePoint>,
) -> Arc<crate::navigation::Tiles> {
    let geometry = crate::navigation::Geometry::new(
        (width, height),
        vec![0; (width * height) as usize],
        &BTreeMap::from([(0, 1.)]),
    );
    Arc::new(crate::navigation::Tiles::new(geometry, points))
}
pub(super) fn view(content: &Content) -> PlayerView {
    PlayerView {
        session: Session {
            world: -3,
            player: 1,
            account: 1,
            character: 1,
        },
        tick: 100,
        time_ms: 5000,
        clock_at: std::time::Instant::now(),
        elapsed_ms: 5000,
        position: Pos { x: 11.5, y: 11.5 },
        quest: None,
        map: MapInfo {
            name: "Realm".into(),
            width: 40,
            height: 40,
            allow_teleport: true,
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
            items: content.get(WIZARD).unwrap().equipment.clone(),
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
        tiles: tiles(40, 40, (0..30).flat_map(|y| (0..30).map(move |x| (x, y)))),
        objects: vec![],
        player_markers: vec![],
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
pub(super) fn on_adventure(profile: Profile, v: &PlayerView) -> Brain {
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
fn community_crowd_names_do_not_create_leaders_or_an_uninvited_human_anchor() {
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
    for profile in [PROFILES[2], PROFILES[3]] {
        assert!(
            matches!(
                on_adventure(profile, &v).decide(&v, &content).unwrap().goal,
                Goal::Explore(..)
            ),
            "unpublished marker handles and familiar names cannot create leaders"
        );
    }
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
fn community_crowd_no_leader_waits_for_a_straggler_and_invitation_expires() {
    let content = content();
    let mut v = view(&content);
    let mut peer = object(2, WIZARD, 23.5, 11.5);
    peer.name = "BattleCrus".into();
    peer.equipped = v.character.items[..4].to_vec();
    v.objects.push(peer);
    let d = on_adventure(PROFILES[1], &v).decide(&v, &content).unwrap();
    assert!(matches!(d.goal, Goal::Explore(..)));
    assert!(d.movement.is_some());
    let mut brain = on_adventure(PROFILES[0], &v);
    brain.companion = Some(2);
    brain.invitation_until = v.time_ms;
    brain.decide(&v, &content).unwrap();
    assert!(brain.companion.is_none());
}
#[test]
fn community_crowd_regroup_requires_a_fresh_published_peer_and_is_paced() {
    let content = content();
    let mut v = view(&content);
    let mut leader = object(2, WIZARD, 48.5, 11.5);
    leader.name = "Biceps".into();
    leader.equipped = v.character.items[..4].to_vec();
    v.objects.push(leader);
    v.player_markers.push(crate::world::view::PlayerMarker {
        id: 2,
        account: 2,
        name: "Biceps".into(),
        position: v.objects[0].position,
    });
    let mut brain = on_adventure(PROFILES[2], &v);
    brain.crowd.knows_minimap = true;
    assert!(matches!(
        brain.decide(&v, &content).unwrap().journey,
        Some(Journey::Regroup(2))
    ));
    v.tick += 5;
    v.time_ms += 250;
    assert!(brain.decide(&v, &content).unwrap().journey.is_none());
    v.tick += 300;
    v.time_ms += 15000;
    v.character.hp = 20;
    let recovering = brain.decide(&v, &content).unwrap();
    assert_eq!(recovering.goal, Goal::Recover);
    assert!(!matches!(recovering.journey, Some(Journey::Regroup(_))));
    v.tick += 100;
    v.time_ms += 5000;
    v.character.hp = 100;
    v.objects.clear();
    v.player_markers.clear();
    assert!(brain.decide(&v, &content).unwrap().journey.is_none());
}
#[test]
fn community_crowd_an_own_quest_outweighs_a_visible_crews_destination() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 20;
    v.map.width = 200;
    v.map.height = 200;
    v.tiles = tiles(200, 200, v.tiles.points());
    let mut leader = object(2, WIZARD, 18.5, 11.5);
    leader.name = "Biceps".into();
    leader.equipped = v.character.items[..4].to_vec();
    let target = object(3, 0x609, 150.5, 150.5);
    v.objects.extend([leader, target]);
    v.quest = Some(3);
    let mut brain = on_adventure(PROFILES[0], &v);
    assert_eq!(brain.decide(&v, &c).unwrap().goal, Goal::Hunt(3));
    v.objects.remove(0);
    v.tick += 300;
    v.time_ms += 15000;
    assert_eq!(brain.decide(&v, &c).unwrap().goal, Goal::Hunt(3));
}
#[test]
fn community_crowd_an_observed_straggler_does_not_halt_an_own_quest() {
    let c = content();
    let mut v = view(&c);
    v.character.level = 20;
    v.map.width = 200;
    v.map.height = 200;
    v.tiles = tiles(200, 200, v.tiles.points());
    let mut peer = object(2, WIZARD, 24.5, 11.5);
    peer.name = "BattleCrus".into();
    peer.equipped = v.character.items[..4].to_vec();
    v.objects.extend([peer, object(3, 0x609, 150.5, 150.5)]);
    v.quest = Some(3);
    let d = on_adventure(PROFILES[1], &v).decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Hunt(3));
    assert!(d.movement.is_some());
}
#[test]
fn community_pursuit_unfamiliar_equipment_requires_assessment_before_swap() {
    let c = content();
    let mut v = view(&c);
    v.character.items[4] = 0xa9a;
    let mut brain = Brain::new(PROFILES[1], &v);
    let decision = brain.decide(&v, &c).unwrap();
    assert!(
        !matches!(decision.item, Some(Action::Swap { .. })),
        "unfamiliar equipment was equipped without assessment"
    );
}
#[test]
fn community_pursuit_supply_reconciles_holdings_and_invalidates_missing_dead_and_old_session_steps()
{
    let c = content();
    let mut v = view(&c);
    v.character.items[4..].fill(-1);
    let mut bag = object(2, 0x500, 12.5, 11.5);
    bag.contents = vec![0xa22, 0xa22];
    v.objects.push(bag);
    let mut b = Brain::new(PROFILES[1], &v);
    assert!(matches!(
        b.decide(&v, &c).unwrap().item,
        Some(Action::Swap { .. })
    ));
    assert_eq!(b.mind.history.back().unwrap().carried, 0);
    v.tick += 4;
    v.time_ms += 200;
    v.character.items[4] = 0xa22;
    v.character.inventory_revision += 1;
    b.decide(&v, &c).unwrap();
    assert!(b.mind.history.iter().any(|h| h.carried == 1));
    let prior = b.mind.clone();
    v.tick += 4;
    v.time_ms += 200;
    b.decide(&v, &c).unwrap();
    assert_eq!(b.mind, prior);
    v.objects.clear();
    v.tick += 4;
    v.time_ms += 200;
    b.decide(&v, &c).unwrap();
    assert_eq!(
        b.mind.history.back().unwrap().reason,
        "observed_opportunity_missing_or_expired"
    );
    v.objects.push({
        let mut bag = object(22, 0x500, 12.5, 11.5);
        bag.contents = vec![0xa22];
        bag
    });
    v.tick += 4;
    v.time_ms += 200;
    b.decide(&v, &c).unwrap();
    let next = b.mind.history.back().unwrap().activity;
    assert_eq!(next, 1);
    let continuing = serde_json::to_value(&b.mind).unwrap();
    assert_eq!(continuing["activity"]["id"], 1);
    assert_eq!(continuing["activity"]["carried"], 1);
    assert_eq!(continuing["activity"]["opportunity"]["observed_object"], 22);
    v.session.character += 1;
    v.character.items[4] = -1;
    v.character.inventory_revision = 0;
    b.mind.bind(&v, 0).unwrap();
    assert!(
        b.mind
            .history
            .iter()
            .any(|h| h.activity == next && h.reason == "character_changed_old_steps_invalidated")
    );
    assert_eq!(
        b.mind.history.back().unwrap().reason,
        "account_session_bound"
    );
    assert_eq!(
        b.mind.history.back().unwrap().binding[1],
        v.session.character
    );
    assert_eq!(b.mind.history.back().unwrap().carried, 0);
    v.session.player += 1;
    assert!(b.decide(&v, &c).is_err());
    let mut satisfied = view(&c);
    satisfied.character.items[4] = 0xa22;
    satisfied.character.items[5] = 0xa22;
    satisfied.objects = v.objects;
    let mut b = Brain::new(PROFILES[1], &satisfied);
    b.decide(&satisfied, &c).unwrap();
    assert!(b.mind.history.iter().all(|h| h.activity == 0));
}
#[test]
fn community_pursuit_knowledge_assessment_ends_and_reconsiders_supported_equal_tier_equipment() {
    let c = content();
    let mut v = view(&c);
    v.character.items[3] = 0xa28;
    v.character.items[4] = 0xa27;
    let mut b = Brain::new(PROFILES[1], &v);
    b.mind.facts.push(pursuits::ItemFact {
        kind: 0xa27,
        assessments: 2,
    });
    let swap = b.mind.assess(&v, &c).unwrap().unwrap();
    assert!(matches!(swap, Action::Swap { .. }));
    v.character.items.swap(3, 4);
    v.character.inventory_revision += 1;
    b.mind.facts.push(pursuits::ItemFact {
        kind: 0xa28,
        assessments: 2,
    });
    v.character.mp = 10;
    assert!(
        matches!(b.mind.assess(&v, &c).unwrap(), Some(Action::Swap { .. })),
        "mana depletion should support reconsidering the health-ring swap"
    );
    v.character.items.swap(3, 4);
    v.character.mp = 100;
    v.character.hp = 45;
    assert!(b.mind.assess(&v, &c).unwrap().is_some());
    let mut unknown = Brain::new(PROFILES[1], &v);
    assert!(unknown.mind.assess(&v, &c).unwrap().is_none());
    assert!(unknown.mind.assess(&v, &c).unwrap().is_none());
    assert!(unknown.mind.assess(&v, &c).unwrap().is_some());
}
#[test]
fn community_pursuit_attention_execution_and_risk_change_real_scoring_inputs_not_cadence() {
    let c = content();
    let mut v = view(&c);
    v.character.items[4] = 0xa9a;
    for (id, y) in [(0, 18.), (1, 11.5)] {
        v.projectiles.push(VisibleProjectile {
            owner: 99,
            id,
            start: Pos { x: 14., y },
            position: Pos { x: 14., y },
            angle: std::f32::consts::PI,
            speed: 50.,
            lifetime_ms: 2000.,
            amplitude: 0.,
            frequency: 0.,
            passes_cover: false,
        });
    }
    let broad = pursuits::Cognition::default();
    let mut narrow = broad.clone();
    narrow.attention = 2;
    let attended = combat::Perception::attend(&v, &broad, true);
    let occupied = combat::Perception::attend(&v, &narrow, true);
    assert!(combat::clearance(&v, v.position, &attended) < 0.2);
    assert!(combat::clearance(&v, v.position, &occupied) > 4.);
    let grid = Grid::terrain(&v);
    let broad_path = combat::movement(
        &v,
        &grid,
        &[],
        Some(Pos { x: 18.35, y: 11.5 }),
        7.,
        false,
        &attended,
    )
    .unwrap();
    let narrow_path = combat::movement(
        &v,
        &grid,
        &[],
        Some(Pos { x: 18.35, y: 11.5 }),
        7.,
        false,
        &occupied,
    )
    .unwrap();
    assert!(broad_path.distance(narrow_path) > 0.1);
    let alone = {
        v.projectiles.remove(0);
        combat::Perception::attend(&v, &narrow, false)
    };
    assert!(combat::clearance(&v, v.position, &alone) < 0.2);
    let mut coarse = broad.clone();
    coarse.velocity_step = 4.;
    coarse.directions = 4;
    let coarse = combat::Perception::attend(&v, &coarse, false);
    assert!(
        attended
            .velocity(Pos { x: 0., y: 1.6 })
            .distance(coarse.velocity(Pos { x: 0., y: 1.6 }))
            > 1.
    );
    assert_eq!(
        coarse
            .velocity(Pos { x: 0., y: 0. })
            .distance(Pos { x: 0., y: 0. }),
        0.
    );
    let mut bag = object(8, 0x500, 12.5, 11.5);
    bag.contents = vec![0xa22];
    v.objects.push(bag);
    v.projectiles[0].angle = 0.;
    v.projectiles[0].position.x = 9.;
    v.projectiles[0].start.x = 9.;
    v.projectiles[0].position.y = 12.;
    v.projectiles[0].start.y = 12.;
    let mut cautious = Brain::new(PROFILES[1], &v);
    cautious.mind.cognition.pickup_clearance = 1.2;
    let mut bold = Brain::new(PROFILES[1], &v);
    bold.mind.cognition.pickup_clearance = 0.1;
    assert!(matches!(
        cautious.decide(&v, &c).unwrap().journey,
        Some(Journey::Nexus)
    ));
    assert!(bold.decide(&v, &c).unwrap().journey.is_none());
    assert_eq!(cautious.next_decision, bold.next_decision);
}
#[test]
fn community_market_local_advertisement_enters_recipient_policy() {
    let c = content();
    let mut v = view(&c);
    let mut peer = object(4, WIZARD, 12.5, 11.5);
    peer.name = "Biceps".into();
    peer.account = Some(4);
    peer.equipped = vec![0xa97];
    v.objects.push(peer);
    let mut b = Brain::new(PROFILES[0], &v);
    let mut out = wire::Writer::new();
    out.utf("Biceps")
        .i32(4)
        .i32(0)
        .u8(0)
        .utf("")
        .utf("sell 2595x2; want 2594")
        .utf("");
    assert!(
        b.mind
            .market
            .receive(
                &Packet {
                    id: wire::TEXT,
                    body: out.0
                },
                &v,
                &c,
                b.mind.future_class
            )
            .unwrap(),
        "market messages must reach recipient policy even with dialogue disabled"
    );
}
