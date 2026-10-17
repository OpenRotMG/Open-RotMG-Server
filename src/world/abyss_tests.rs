use super::{tests, *};
use crate::{
    condition,
    dungeon::abyss::{self, BRIDGE, IDOL, LAVA, MALPHAS},
};

fn fixture() -> (World, i32) {
    let mut w = tests::world();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for c in &mut Arc::make_mut(&mut w.map).cells {
        c.ground = 0x61;
        c.object = None;
    }
    let mut ch = tests::ch(&w);
    ch.base = w.content.classes[&ch.class].caps;
    ch.hp = ch.base[0];
    ch.mp = ch.base[1];
    let id = w
        .enter_at(
            ch,
            None,
            Some(Pos { x: 12.5, y: 12.5 }),
            AccountPresentation {
                guild: None,
                fame: 0,
                best_fame: BTreeMap::new(),
                credits: 0,
            },
        )
        .unwrap();
    (w, id)
}
#[test]
fn reference_wisdom_regeneration_uses_equipment_and_respects_quiet_and_maximum_mana() {
    let (mut w, id) = fixture();
    let caps = w.content.classes[&0x310].caps;
    let p = w.players.get_mut(&id).unwrap();
    p.character.class = 0x310;
    p.character.base = caps;
    p.character.items = vec![0xaf6, 0xb25, 0xb05, 0xac5, -1, -1, -1, -1, -1, -1, -1, -1];
    p.character.mp = 0;
    let values = w.players[&id].character.effective(&w.content).unwrap();
    assert_eq!(values[7], 83);
    for _ in 0..200 {
        w.step();
    }
    assert_eq!(w.players[&id].character.mp, 55);
    w.players.get_mut(&id).unwrap().character.mp = values[1] - 1;
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(w.players[&id].character.mp, values[1]);
    w.apply_condition(id, condition::QUIET, 1000).unwrap();
    for _ in 0..10 {
        w.step();
    }
    assert_eq!(w.players[&id].character.mp, 0);
    assert_eq!(w.players[&id].mp_fraction, 0.);
    for _ in 0..20 {
        w.step();
    }
    assert!(w.players[&id].character.mp > 0);
}
#[test]
fn lava_uses_native_damage_rng_ignores_armor_and_preserves_the_next_weapon_roll() {
    let (mut w, id) = fixture();
    w.players.get_mut(&id).unwrap().controller = Controller::Human;
    let pos = w.players[&id].pos;
    let i = pos.y as usize * w.map.width as usize + pos.x as usize;
    Arc::make_mut(&mut w.map).cells[i].ground = LAVA;
    let hp = w.players[&id].character.hp;
    let seed = w.players[&id].seed;
    let expected = ((seed as u64 * 16807) % 2147483647) as u32;
    w.ground_damage(id, 1000, pos).unwrap();
    assert_eq!(
        w.players[&id].character.hp,
        hp - 30 - (expected % 30) as i32
    );
    assert_eq!(w.players[&id].seed, expected);
    let weapon = w.content.objects[&0xa97].projectiles[&0].clone();
    let following = ((expected as u64 * 16807) % 2147483647) as u32;
    assert_eq!(
        w.players.get_mut(&id).unwrap().roll_damage(&weapon),
        weapon.min + (following % (weapon.max - weapon.min) as u32) as i32
    );
    let bridge = w.spawn_entity(BRIDGE, pos, vec![]).unwrap();
    let hp = w.players[&id].character.hp;
    w.ground_damage(id, 1600, pos).unwrap();
    assert_eq!(w.players[&id].character.hp, hp);
    w.players.get_mut(&id).unwrap().controller = Controller::Qa;
    let seed = w.players[&id].seed;
    w.ground_damage(id, 2200, pos).unwrap();
    assert_eq!(w.players[&id].seed, seed);
    w.remove_entity(bridge);
    w.players.get_mut(&id).unwrap().character.hp = 1;
    w.ground_damage(id, 2800, pos).unwrap();
    assert!(w.players[&id].death_pending);
    let deaths = w.pending_deaths();
    assert_eq!(deaths.len(), 1);
    assert_eq!(deaths[0].2, "Lava");
}
#[test]
fn automatic_actor_lava_obeys_the_same_per_tile_cooldown_and_invulnerability() {
    let (mut w, id) = fixture();
    let pos = w.players[&id].pos;
    let i = pos.y as usize * w.map.width as usize + pos.x as usize;
    Arc::make_mut(&mut w.map).cells[i].ground = LAVA;
    w.step();
    let first = w.players[&id].character.hp;
    let seed = w.players[&id].seed;
    for _ in 0..9 {
        w.step();
    }
    assert_eq!(w.players[&id].seed, seed);
    w.step();
    assert!(w.players[&id].character.hp < first);
    w.apply_condition(id, condition::INVULNERABLE, 2000)
        .unwrap();
    let hp = w.players[&id].character.hp;
    w.ground_damage(id, 2000, pos).unwrap();
    assert_eq!(w.players[&id].character.hp, hp);
}
#[test]
fn malphas_has_four_combat_forms_selected_projectiles_bounded_children_and_repeating_shields() {
    let (mut w, player) = fixture();
    w.encounter = true;
    let home = Pos { x: 17.5, y: 12.5 };
    let boss = w.spawn_entity(MALPHAS, home, vec![]).unwrap();
    let target = w.players[&player].pos;
    assert_ne!(
        w.entities[&boss].condition & condition::bit(condition::INVULNERABLE),
        0
    );
    for elapsed in [
        0, 2000, 9000, 11500, 23500, 25300, 26700, 36500, 39000, 47000, 50200, 52200,
    ] {
        w.now = elapsed;
        w.abyss_mechanics(boss, target, elapsed).unwrap();
        let shield = matches!(abyss::phase(elapsed), 1 | 4 | 5) || elapsed % 50200 < 2000;
        if shield {
            assert_ne!(
                w.entities[&boss].condition & condition::bit(condition::INVULNERABLE),
                0
            );
        }
        for child in w.entities.values().filter(|e| e.parent == Some(boss)) {
            crate::encounter::validate(&w.content, child.kind).unwrap();
        }
        assert!(
            w.entities
                .values()
                .filter(|e| e.parent == Some(boss))
                .count()
                <= 16
        );
    }
    assert!(
        w.entities
            .values()
            .all(|e| e.parent != Some(boss) || e.kind != 0x90b)
    );
    for elapsed in (0..100400).step_by(100) {
        for (index, _) in abyss::attacks(MALPHAS, 0., elapsed) {
            assert!(w.content.objects[&MALPHAS].projectiles.contains_key(&index));
        }
    }
    assert_eq!(abyss::phase(0), 0);
    assert_eq!(abyss::phase(11500), 2);
    assert_eq!(abyss::phase(23500), 3);
    assert_eq!(abyss::phase(39000), 5);
    w.now = 2000;
    w.abyss_mechanics(boss, target, 2000).unwrap();
    assert_eq!(
        w.damage_enemy(player, boss, 100, true, &[], None).unwrap(),
        100
    );
    w.now = 9000;
    w.abyss_mechanics(boss, target, 9000).unwrap();
    assert_eq!(
        w.damage_enemy(player, boss, 100, true, &[], None).unwrap(),
        0
    );
}
#[test]
fn a_damaged_missile_flashes_then_explodes_once_even_after_a_lethal_hit() {
    let (mut w, player) = fixture();
    w.encounter = true;
    let missile = w
        .spawn_entity(0x909, Pos { x: 17.5, y: 12.5 }, vec![])
        .unwrap();
    w.now = 100;
    w.npc_step();
    w.damage_enemy(player, missile, 5000, true, &[], None)
        .unwrap();
    assert_eq!(w.entities[&missile].hp, 1);
    w.now = 150;
    assert!(w.npc_step().iter().all(|(id, _)| *id != missile));
    assert_eq!(w.entities[&missile].stage, Some(1));
    w.now = 750;
    w.npc_step();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == missile).count(), 8);
    w.now = 800;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&missile));
    assert_eq!(w.players[&player].character.kills, 0);
}
#[test]
fn an_idol_activates_on_damage_closes_only_its_bridge_and_restores_lava_without_exploration_fame() {
    let content = tests::world().content;
    let map = (0..16)
        .map(|seed| abyss::generate(&content, seed).unwrap())
        .find(|map| map.cells.iter().any(|c| c.object == Some(IDOL)))
        .unwrap();
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.encounter = true;
    w.name = "Abyss of Demons".into();
    let idol = w.entities.values().find(|e| e.kind == IDOL).unwrap().id;
    let home = w.entities[&idol].home;
    let region = w.map.cell(home.x, home.y).unwrap().region;
    let bridges = |w: &World| {
        w.entities
            .values()
            .filter(|e| e.kind == BRIDGE && w.map.cell(e.pos.x, e.pos.y).unwrap().region == region)
            .count()
    };
    let others = |w: &World| {
        w.entities
            .values()
            .filter(|e| e.kind == BRIDGE && w.map.cell(e.pos.x, e.pos.y).unwrap().region != region)
            .count()
    };
    let count = bridges(&w);
    assert!(count > 0);
    let other_count = others(&w);
    let mut ch = tests::ch(&w);
    ch.base = w.content.classes[&ch.class].caps;
    ch.hp = ch.base[0];
    let player = w
        .enter_at(
            ch,
            None,
            Some(home),
            AccountPresentation {
                guild: None,
                fame: 0,
                best_fame: BTreeMap::new(),
                credits: 0,
            },
        )
        .unwrap();
    w.npc_step();
    assert_eq!(bridges(&w), count);
    w.updates(player, true).unwrap();
    let tiles = w.players[&player].character.tiles;
    w.damage_enemy(player, idol, 1, true, &[], None).unwrap();
    w.npc_step();
    assert_eq!(bridges(&w), 0);
    assert_eq!(others(&w), other_count);
    assert!(!w.lava_patches.is_empty());
    let patches = w.lava_patches.clone();
    let patch = *patches.first_key_value().unwrap().0;
    w.now = 2100;
    w.abyss_step();
    for (i, (_, ground)) in patches {
        assert_eq!(w.map.cells[i].ground, ground);
    }
    w.updates(player, false).unwrap();
    assert_eq!(w.players[&player].character.tiles, tiles);
    w.now = 10000;
    w.abyss_step();
    let pos = Pos {
        x: (patch % w.map.width as usize) as f32 + 0.5,
        y: (patch / w.map.width as usize) as f32 + 0.5,
    };
    let p = w.players.get_mut(&player).unwrap();
    p.controller = Controller::Human;
    p.pos = pos;
    let hp = p.character.hp;
    w.ground_damage(player, 6000, pos).unwrap();
    assert!(w.players[&player].character.hp < hp);
    w.damage_enemy(player, idol, 30000, true, &[], None)
        .unwrap();
    assert_eq!(bridges(&w), count);
    assert_eq!(others(&w), other_count);
    assert!(
        w.entities
            .values()
            .any(|e| e.items.contains(&0xa34) && e.extra_stats.contains(&(54, 1)))
    );
}
