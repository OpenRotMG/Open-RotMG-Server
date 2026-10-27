use super::*;
use crate::world::view::{Lifecycle, MapInfo, OwnCharacter, Session, VisibleObject};

fn adventure(brain: &mut Brain, v: &PlayerView, c: &Content) -> Decision {
    brain
        .mind
        .intentions
        .as_mut()
        .unwrap()
        .observe(v, c)
        .unwrap();
    let enemies: Vec<_> = v
        .objects
        .iter()
        .filter(|o| c.objects[&o.kind].enemy)
        .collect();
    let s = Scene {
        nearest: enemies.last().copied(),
        quarry: enemies.last().copied(),
        enemies,
        perception: combat::Perception::attend(v, &brain.mind.cognition, false),
        inspecting: false,
        range: 6.,
        passes_cover: false,
        health: v.character.hp as f32 / v.character.stats[0] as f32,
        danger: 10.,
        threatened: false,
        retreat: false,
        outmatched: false,
        grid: Arc::new(Grid::terrain(v)),
        remedy: None,
    };
    brain
        .adventure(
            v,
            c,
            &s,
            Decision {
                goal: Goal::Wait,
                movement: None,
                aim: None,
                item: None,
                journey: None,
                chat: None,
            },
            false,
        )
        .unwrap()
}

#[test]
fn geared_rushers_pass_trash_but_inexperienced_or_unprepared_bots_clear() {
    let (c, mut v) = fixture();
    v.map.name = "Snake Pit".into();
    v.character.level = 20;
    v.character.stats[3] = 25;
    v.objects[0].kind = 0x917;
    v.objects[0].condition_mask = Some(crate::condition::bit(crate::condition::INVULNERABLE));
    let mut snake = v.objects[0].clone();
    snake.id = 3;
    snake.kind = 0x225;
    snake.position = Pos { x: 12.5, y: 10.5 };
    snake.hp = c.objects[&snake.kind].hp;
    snake.max_hp = snake.hp;
    snake.condition_mask = Some(0);
    v.objects.push(snake);
    let mut brain = Brain::new(ARCHETYPES[1], &v);
    brain.goal = Goal::Fight(3);
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Hunt(2));
    brain.mind.cognition.directions = 4;
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Fight(3));
    brain.mind.cognition.directions = 16;
    v.character.stats[3] = 10;
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Fight(3));
    v.character.stats[3] = 25;
    v.character.level = 14;
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Fight(3));
    v.character.level = 20;
    v.character.hp = 300;
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Fight(3));
    v.character.hp = 500;
    v.map.name = "Realm".into();
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Fight(3));
    v.map.name = "Undead Lair".into();
    v.objects[0].kind = 0x678;
    v.objects[0].condition_mask = Some(0);
    let rushing = adventure(&mut brain, &v, &c);
    assert_eq!(rushing.goal, Goal::Hunt(2));
    assert!(
        rushing.aim.is_some(),
        "shoot nearby trash while moving to the quest"
    );
    v.objects[0].position = Pos { x: 10.5, y: 14.5 };
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Fight(2));
    v.map.name = "Snake Pit".into();
    v.objects[0].position = Pos { x: 10.5, y: 90.5 };
    v.objects[1].kind = 0x223;
    v.character.level = 14;
    v.character.stats[3] = 10;
    brain.mind.cognition.directions = 4;
    assert_eq!(adventure(&mut brain, &v, &c).goal, Goal::Hunt(2));
    v.character.hp = 100;
    let mut hurt = Brain::new(ARCHETYPES[1], &v);
    assert_eq!(hurt.decide(&v, &c).unwrap().goal, Goal::Recover);
}

#[test]
fn portal_callers_wait_instead_of_entering_on_the_calling_decision() {
    let (c, mut v) = fixture();
    v.objects[0].kind = 0x718;
    v.objects[0].usable = true;
    v.objects[0].position = v.position;
    v.objects[0].hp = 0;
    v.quest = None;
    let mut callers = 0;
    let mut silent = 0;
    for seed in 1..=32 {
        v.session.account = seed;
        let mut brain = Brain::new(ARCHETYPES[1], &v);
        let first = adventure(&mut brain, &v, &c);
        assert_eq!(first.goal, Goal::Portal(2));
        if first.chat.is_some() {
            callers += 1;
            assert!(first.journey.is_none());
            assert_eq!(brain.portal, Some((2, u64::MAX)));
            brain.goal = first.goal;
            assert!(adventure(&mut brain, &v, &c).journey.is_none());
            brain.portal = Some((2, v.time_ms + 4000));
            v.time_ms += 3999;
            assert!(adventure(&mut brain, &v, &c).journey.is_none());
            v.time_ms += 1;
            assert!(matches!(
                adventure(&mut brain, &v, &c).journey,
                Some(Journey::Portal(2))
            ));
        } else {
            silent += 1;
            assert!(matches!(first.journey, Some(Journey::Portal(2))));
        }
    }
    assert!(callers > 0 && silent > 0);
}

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
