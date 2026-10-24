use super::*;
use crate::world::view::{Lifecycle, MapInfo, OwnCharacter, Session, VisibleObject};

fn fixture() -> (Content, PlayerView) {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let geometry = crate::navigation::Geometry::new(
        (128, 128),
        vec![1; 128 * 128],
        &BTreeMap::from([(1, 1.)]),
    );
    let tiles = crate::navigation::Tiles::new(
        geometry,
        (0..128).flat_map(|x| (0..128).map(move |y| (x, y))),
    );
    let stats = [500, 200, 35, 10, 25, 30, 30, 25];
    let view = PlayerView {
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
        position: Pos { x: 10.5, y: 10.5 },
        quest: Some(2),
        map: MapInfo {
            name: "Realm".into(),
            width: 128,
            height: 128,
            allow_teleport: true,
        },
        character: OwnCharacter {
            conditions: 0,
            class: crate::content::WIZARD,
            level: 14,
            experience: 20000,
            hp: stats[0],
            mp: stats[1],
            stats,
            base_stats: stats,
            items: vec![0xa9e, 0xa2e, 0xa80, -1, -1, -1, -1, -1, -1, -1, -1, -1],
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
        tiles: Arc::new(tiles),
        player_markers: vec![],
        projectiles: vec![],
        objects: vec![VisibleObject {
            id: 2,
            account: None,
            kind: 0x678,
            name: "Oasis Giant".into(),
            position: Pos { x: 10.5, y: 90.5 },
            hp: 650,
            max_hp: 650,
            level: None,
            equipped: vec![],
            contents: vec![],
            blocks: false,
            usable: false,
            condition_mask: Some(0),
        }],
    };
    (content, view)
}

#[test]
fn published_realm_quest_displaces_retained_roaming() {
    let (content, mut view) = fixture();
    for profile in ARCHETYPES {
        let mut brain = Brain::new(profile, &view);
        brain.goal = Goal::Venture;
        assert!(progression::ready(&view, &content, 0x718));
        let decision = brain.decide(&view, &content).unwrap();
        assert_eq!(decision.goal, Goal::Hunt(2), "{}", profile.name);
        let movement = decision.movement.expect("travel toward published quest");
        assert!(movement.y > view.position.y && (movement.x - view.position.x).abs() < 0.1);
    }
    view.objects[0].position.y = 30.5;
    let mut brain = Brain::new(ARCHETYPES[0], &view);
    brain.goal = Goal::Venture;
    assert_eq!(brain.decide(&view, &content).unwrap().goal, Goal::Hunt(2));
}

#[test]
fn quest_still_yields_to_unsuitability_recovery_and_supplies() {
    let (content, mut view) = fixture();
    view.objects[0].kind = 0x91f;
    view.objects[0].max_hp = 300000;
    assert!(!progression::can_fight(&view, &content, &view.objects[0]));
    assert_ne!(
        Brain::new(ARCHETYPES[0], &view)
            .decide(&view, &content)
            .unwrap()
            .goal,
        Goal::Hunt(2)
    );
    view.objects[0].kind = 0x678;
    view.objects[0].max_hp = 650;
    view.objects[0].condition_mask = Some(crate::condition::bit(crate::condition::INVULNERABLE));
    assert_ne!(
        Brain::new(ARCHETYPES[0], &view)
            .decide(&view, &content)
            .unwrap()
            .goal,
        Goal::Hunt(2)
    );
    view.objects[0].condition_mask = Some(0);
    let mut brain = Brain::new(ARCHETYPES[0], &view);
    brain.goal = Goal::Hunt(2);
    view.character.hp = 100;
    let recovery = brain.decide(&view, &content).unwrap();
    assert_eq!(recovery.goal, Goal::Recover);
    assert!(matches!(recovery.journey, Some(Journey::Nexus)));
    brain.goal = recovery.goal;
    view.character.hp = 500;
    view.tick += 61;
    view.time_ms += 3050;
    assert_eq!(brain.decide(&view, &content).unwrap().goal, Goal::Hunt(2));
    view.objects.push(VisibleObject {
        id: 3,
        account: None,
        kind: 0x500,
        name: String::new(),
        position: view.position,
        hp: 1,
        max_hp: 1,
        level: None,
        equipped: vec![],
        contents: vec![0xa22],
        blocks: false,
        usable: false,
        condition_mask: None,
    });
    assert_eq!(
        Brain::new(ARCHETYPES[0], &view)
            .decide(&view, &content)
            .unwrap()
            .goal,
        Goal::Loot(3)
    );
}

fn recovery_scene(hp: i32) -> (Content, PlayerView) {
    let (content, mut view) = fixture();
    view.character.hp = hp;
    view.character.items[1] = -1;
    view.objects[0].position = Pos { x: 16.5, y: 10.5 };
    (content, view)
}

#[test]
fn noncritical_recovery_steps_back_or_rests_locally() {
    let (content, mut view) = recovery_scene(250);
    let decision = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert_eq!(decision.goal, Goal::Recover);
    assert!(decision.journey.is_none());
    let step = decision.movement.expect("available backstep");
    assert!(
        step.distance(view.objects[0].position) > view.position.distance(view.objects[0].position)
    );
    view.objects.clear();
    view.quest = None;
    let quiet = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert_eq!(quiet.goal, Goal::Recover);
    assert!(quiet.journey.is_none() && quiet.movement.is_none());
}

#[test]
fn recovery_emergency_boundary_retains_personality() {
    let (content, mut view) = recovery_scene(125);
    let cautious = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert!(matches!(cautious.journey, Some(Journey::Nexus)));
    let tolerant = Brain::new(ARCHETYPES[1], &view)
        .decide(&view, &content)
        .unwrap();
    assert_eq!(tolerant.goal, Goal::Recover);
    assert!(tolerant.journey.is_none() && tolerant.movement.is_some());
    view.character.hp = 50;
    assert!(matches!(
        Brain::new(ARCHETYPES[1], &view)
            .decide(&view, &content)
            .unwrap()
            .journey,
        Some(Journey::Nexus)
    ));
}

#[test]
fn recovery_requires_executable_clear_backstep() {
    let (content, mut view) = recovery_scene(250);
    let exits = |view: &PlayerView| {
        matches!(
            Brain::new(ARCHETYPES[0], view)
                .decide(view, &content)
                .unwrap()
                .journey,
            Some(Journey::Nexus)
        )
    };
    view.character.conditions = crate::content::PARALYZED;
    assert!(exits(&view));
    view.character.conditions = 0;
    let known = view.tiles.clone();
    let geometry = crate::navigation::Geometry::new(
        (128, 128),
        vec![1; 128 * 128],
        &BTreeMap::from([(1, 1.)]),
    );
    view.tiles = Arc::new(crate::navigation::Tiles::new(
        geometry,
        [tile(view.position)],
    ));
    assert!(exits(&view));
    view.tiles = known;
    view.projectiles
        .push(crate::world::view::VisibleProjectile {
            owner: 2,
            id: 0,
            start: view.objects[0].position,
            position: view.position,
            angle: std::f32::consts::PI,
            speed: 60.,
            lifetime_ms: 2400.,
            amplitude: 0.,
            frequency: 0.,
            passes_cover: false,
        });
    assert!(exits(&view));
}

#[test]
fn recovery_preserves_healing_and_close_outmatched_precedence() {
    let (content, mut view) = recovery_scene(50);
    view.character.items[4] = 0xa22;
    let healing = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert!(healing.journey.is_none());
    assert!(matches!(healing.item, Some(Action::Use { slot: 4, .. })));
    view.objects[0].kind = 0x91f;
    view.objects[0].max_hp = 300000;
    view.objects[0].position.x = 13.5;
    let outmatched = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert!(matches!(outmatched.journey, Some(Journey::Nexus)));
    view.character.items[4] = -1;
    view.character.class = crate::content::PRIEST;
    view.character.items[0] = 0xa3d;
    view.character.items[1] = 0xa31;
    let pending = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert!(pending.journey.is_none());
    assert!(matches!(pending.item, Some(Action::Use { slot: 1, .. })));
}

#[test]
fn recovery_rejects_centerline_clear_but_blocked_endpoint_footprint() {
    let (content, mut view) = recovery_scene(250);
    let mut ground = vec![1; 128 * 128];
    ground[10 * 128 + 8] = 255;
    let geometry =
        crate::navigation::Geometry::new((128, 128), ground, &BTreeMap::from([(1, 1.), (255, 0.)]));
    view.tiles = Arc::new(crate::navigation::Tiles::new(
        geometry.clone(),
        (0..128).flat_map(|x| (0..128).map(move |y| (x, y))),
    ));
    let grid = Grid::terrain(&view);
    let step = Pos { x: 9.25, y: 10.5 };
    assert!(grid.segment(view.position, step));
    assert!(!recovery_step_valid(&view, &content, &grid, step));
    assert!(recovery_step_valid(
        &view,
        &content,
        &grid,
        Pos { x: 9.5, y: 10.5 }
    ));
    let decision = Brain::new(ARCHETYPES[0], &view)
        .decide(&view, &content)
        .unwrap();
    assert!(matches!(decision.journey, Some(Journey::Nexus)));
    assert!(decision.movement.is_some_and(|p| p.distance(step) < 0.001));
    view.tiles = Arc::new(crate::navigation::Tiles::new(
        geometry,
        (0..128)
            .flat_map(|x| (0..128).map(move |y| (x, y)))
            .filter(|p| *p != (8, 10)),
    ));
    assert!(!recovery_step_valid(
        &view,
        &content,
        &Grid::terrain(&view),
        step
    ));
    assert!(matches!(
        Brain::new(ARCHETYPES[0], &view)
            .decide(&view, &content)
            .unwrap()
            .journey,
        Some(Journey::Nexus)
    ));
}

#[test]
fn recovery_footprint_preserves_half_cell_and_object_boundaries() {
    let (content, mut view) = recovery_scene(250);
    let grid = Grid::terrain(&view);
    let step = Pos { x: 9.25, y: 10.5 };
    assert!(recovery_step_valid(&view, &content, &grid, step));
    assert!(!recovery_step_valid(
        &view,
        &content,
        &grid,
        Pos { x: 0.25, y: 10.5 }
    ));
    assert!(recovery_step_valid(
        &view,
        &content,
        &grid,
        Pos { x: 0.5, y: 10.5 }
    ));
    assert!(!recovery_step_valid(
        &view,
        &content,
        &grid,
        Pos {
            x: f32::NAN,
            y: 10.5
        }
    ));
    view.objects[0].kind = content
        .objects
        .values()
        .find(|o| o.full_occupy)
        .unwrap()
        .kind;
    view.objects[0].position = Pos { x: 8.5, y: 10.5 };
    assert!(!recovery_step_valid(&view, &content, &grid, step));
    view.objects[0].kind = content
        .objects
        .values()
        .find(|o| o.blocks && !o.full_occupy)
        .unwrap()
        .kind;
    assert!(recovery_step_valid(&view, &content, &grid, step));
    view.objects[0].position = step;
    assert!(!recovery_step_valid(&view, &content, &grid, step));
}
