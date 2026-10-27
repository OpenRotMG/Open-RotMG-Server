use super::*;
use crate::world::view::{Lifecycle, MapInfo, OwnCharacter, PlayerMarker, Session, VisibleObject};

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
        inputs: None,
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
            fame: 20,
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
        dungeon_entries: vec![],
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

fn rusher_fixture() -> (Content, PlayerView) {
    let (content, mut view) = fixture();
    view.map.name = "Abyss of Demons".into();
    view.objects[0].kind = 0x927;
    view.objects[0].position = Pos { x: 110.5, y: 10.5 };
    view.player_markers.push(PlayerMarker {
        id: 3,
        account: 3,
        name: "Rusher".into(),
        position: Pos { x: 100.5, y: 10.5 },
    });
    (content, view)
}

#[test]
fn dungeon_quest_uses_a_public_rusher_without_abandoning_its_goal() {
    let (content, mut view) = rusher_fixture();
    for world in [
        "Pirate Cave",
        "Snake Pit",
        "Sprite World",
        "Undead Lair",
        "Abyss of Demons",
        "Ocean Trench",
    ] {
        view.map.name = world.into();
        let mut brain = Brain::new(ARCHETYPES[0], &view);
        let decision = brain.decide(&view, &content).unwrap();
        assert_eq!(decision.goal, Goal::Hunt(2), "{world}");
        assert!(
            matches!(decision.journey, Some(Journey::Regroup(3))),
            "{world}: {:?}",
            decision.journey
        );
        view.tick += 10;
        view.time_ms += 100;
        view.elapsed_ms += 100;
        assert!(
            brain.decide(&view, &content).unwrap().journey.is_none(),
            "{world}: cooldown"
        );
    }
}

#[test]
fn dungeon_rusher_does_not_override_safety_or_teleport_policy() {
    for case in 0..8 {
        let (content, mut v) = rusher_fixture();
        let mut profile = ARCHETYPES[0];
        match case {
            0 => v.map.allow_teleport = false,
            1 => v.player_markers.clear(),
            2 => v.player_markers[0].position = Pos { x: 5.5, y: 10.5 },
            3 => v.player_markers[0].position = Pos { x: 20.5, y: 10.5 },
            4 => v.character.hp = 100,
            5 => profile.knows_minimap = false,
            6 => {
                let mut enemy = fixture().1.objects.pop().unwrap();
                enemy.id = 4;
                enemy.kind = 0x927;
                enemy.position = Pos { x: 12.5, y: 10.5 };
                v.objects.push(enemy);
            }
            7 => v.player_markers[0].position = v.objects[0].position,
            _ => unreachable!(),
        }
        let decision = Brain::new(profile, &v).decide(&v, &content).unwrap();
        assert!(
            !matches!(decision.journey, Some(Journey::Regroup(_))),
            "case {case}: {:?}",
            decision.journey
        );
    }
}
