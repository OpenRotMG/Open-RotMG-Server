use super::*;
use crate::world::view::{Lifecycle, MapInfo, OwnCharacter, Session, VisibleObject};

pub(super) fn fixture() -> (Content, PlayerView) {
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
