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

#[test]
fn recovering_wizard_nova_respects_target_protection_and_mana() {
    use crate::{
        condition::{INVINCIBLE, INVULNERABLE, PARALYZED, STASIS, STUNNED, bit},
        content::{Cell, Map},
        store::Character,
        world::{Controller, World},
    };
    for (protection, mana, casts) in [
        (0, 200, true),
        (bit(INVULNERABLE), 200, false),
        (bit(INVINCIBLE), 200, false),
        (bit(STASIS), 200, false),
        (0, 29, false),
    ] {
        let (content, mut view) = fixture();
        view.character.hp = 225;
        view.character.mp = mana;
        // Hold the sole target still so only the spell can change its HP.
        let conditions = protection | bit(PARALYZED) | bit(STUNNED);
        view.objects[0].position = Pos { x: 14.5, y: 10.5 };
        view.objects[0].condition_mask = Some(conditions);
        let content = Arc::new(content);
        let mut world = World::new(
            content.clone(),
            Arc::new(Map {
                width: 128,
                height: 128,
                cells: vec![
                    Cell {
                        ground: 1,
                        object: None,
                        config: String::new(),
                        terrain: 0,
                        region: 0,
                    };
                    128 * 128
                ],
                spawns: vec![(view.position.x, view.position.y)],
            }),
        )
        .unwrap();
        let mut character = Character::fresh(content.get(view.character.class).unwrap()).unwrap();
        character.id = view.session.character;
        character.account = view.session.account;
        character.base = view.character.base_stats;
        character.items = view.character.items.clone();
        character.hp = view.character.hp;
        character.mp = mana;
        let player = world.enter(character, None).unwrap();
        world.players.get_mut(&player).unwrap().controller = Controller::Autonomous;
        let target = world
            .spawn_entity(view.objects[0].kind, view.objects[0].position, vec![])
            .unwrap();
        world.entities.get_mut(&target).unwrap().condition = conditions;
        view.session.player = player;
        view.objects[0].id = target;
        view.quest = Some(target);
        let mut brain = Brain::new(ARCHETYPES[0], &view);
        let decision = brain.decide(&view, &content).unwrap();
        assert!(brain.recover_until > view.tick);
        assert_eq!(brain.activity, Activity::Survive);
        assert_eq!(decision.goal, Goal::Recover);
        assert!(
            decision.journey.is_none(),
            "Nexus must not mask the retreat cast"
        );
        assert!(decision.aim.is_none(), "retreat must not fire the staff");
        let backstep = decision.movement.expect("viable retreat step");
        assert!(
            backstep.distance(view.objects[0].position)
                > view.position.distance(view.objects[0].position)
        );
        world
            .autonomous_action(
                player,
                Action::Move {
                    x: backstep.x,
                    y: backstep.y,
                },
            )
            .unwrap();
        assert_eq!(
            decision.item.is_some(),
            casts,
            "protection={protection}, mana={mana}"
        );
        if let Some(action) = decision.item {
            assert!(matches!(action, Action::Use { slot: 1, .. }));
            world.autonomous_action(player, action).unwrap();
        }
        let spell = content.get(0xa2e).unwrap();
        assert_eq!(spell.mana_cost, 30);
        assert_eq!(world.bullets.len(), if casts { 20 } else { 0 });
        assert_eq!(
            world.players[&player].character.mp,
            mana - if casts { 30 } else { 0 }
        );
        let ready = world.players[&player].ability_ready_ms;
        assert_eq!(ready > 0, casts);
        let hp = world.entities[&target].hp;
        for _ in 0..=(spell.projectiles[&0].lifetime as u64 / 50) {
            world.step();
            assert_eq!(world.entities[&target].condition & protection, protection);
        }
        assert!(world.players[&player].pos.distance(backstep) < 0.1);
        assert!(world.bullets.is_empty());
        if casts {
            assert!(world.entities[&target].hp < hp);
        } else {
            assert_eq!(world.entities[&target].hp, hp);
        }
        assert_eq!(
            world.players[&player].character.shots,
            if casts { 20 } else { 0 }
        );
        assert_eq!(world.players[&player].ability_ready_ms, ready);
        assert!(world.players[&player].character.mp >= mana - if casts { 30 } else { 0 });
    }
}
