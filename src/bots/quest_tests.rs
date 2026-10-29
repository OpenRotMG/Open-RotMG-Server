use super::*;
use crate::world::view::{Lifecycle, MapInfo, OwnCharacter, PlayerMarker, Session, VisibleObject};

fn adventure(brain: &mut Brain, v: &PlayerView, c: &Content) -> Decision {
    brain
        .mind
        .intentions
        .as_mut()
        .unwrap()
        .observe(v, c, brain.profile.identity)
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
    v.objects.push(VisibleObject {
        id: 3,
        account: None,
        kind: 0x225,
        name: String::new(),
        position: Pos { x: 12.5, y: 10.5 },
        hp: c.objects[&0x225].hp,
        max_hp: c.objects[&0x225].hp,
        level: None,
        equipped: vec![],
        contents: vec![],
        blocks: false,
        usable: false,
        condition_mask: Some(0),
    });
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
        let ready = world.players[&player].ability_ready_ms();
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
        assert_eq!(world.players[&player].ability_ready_ms(), ready);
        assert!(world.players[&player].character.mp >= mana - if casts { 30 } else { 0 });
    }
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

#[test]
fn wavelength_vault_errand_banks_before_leaving() {
    let (c, mut v) = fixture();
    v.map.name = "Vault".into();
    v.quest = None;
    let potion = 0xa1f;
    let stat = progression::potion_index(&c, potion).unwrap().unwrap();
    v.character.base_stats[stat] = c.classes[&v.character.class].caps[stat];
    v.character.items[4] = potion;
    let chest = &mut v.objects[0];
    chest.kind = 0x504;
    chest.hp = 0;
    chest.position = Pos { x: 11.5, y: 10.5 };
    chest.contents = vec![-1; 8];
    let mut brain = Brain::new(ARCHETYPES[1], &v);
    brain
        .plan
        .start(plan::now(), plan::Owner::Bank, "Vault".into());
    let deposit = brain.decide(&v, &c).unwrap();
    assert_eq!(deposit.goal, Goal::Bank(2));
    assert!(
        deposit.journey.is_none(),
        "the Vault is not left before banking"
    );
    v.character.items[4] = -1;
    v.objects[0].contents[0] = potion;
    v.character.inventory_revision += 1;
    v.tick += 5;
    brain.decide(&v, &c).unwrap();
    assert!(brain.plan.idle());
    assert!(
        brain
            .plan
            .events
            .iter()
            .any(|e| e.kind == plan::Kind::Completed && e.owner == Some(plan::Owner::Bank))
    );
    v.tick += 5;
    v.elapsed_ms += 2000;
    let leave = brain.decide(&v, &c).unwrap();
    assert!(matches!(leave.journey, Some(Journey::Nexus)));
}

#[test]
fn threatened_bot_beside_health_bag_handles_the_threat_instead_of_leaving() {
    let (c, mut v) = fixture();
    v.quest = None;
    let mut bag = v.objects.remove(0);
    bag.id = 9;
    bag.kind = 0x500;
    bag.hp = 0;
    bag.contents = vec![0xa22];
    bag.position = Pos { x: 11.5, y: 10.5 };
    v.objects.push(bag);
    v.projectiles.push(crate::world::view::VisibleProjectile {
        owner: 2,
        id: 1,
        start: Pos { x: 13.5, y: 10.5 },
        position: Pos { x: 12.5, y: 10.5 },
        angle: std::f32::consts::PI,
        speed: 100.,
        lifetime_ms: 2000.,
        amplitude: 0.,
        frequency: 0.,
        passes_cover: false,
    });
    let mut brain = Brain::new(ARCHETYPES[1], &v);
    let d = brain.decide(&v, &c).unwrap();
    assert_eq!(brain.activity, Activity::Survive);
    assert!(
        d.journey.is_none(),
        "a visible supply is not a reason to leave"
    );
}
