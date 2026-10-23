use super::{Bullet, Pos, tests};
use std::{collections::BTreeSet, sync::Arc};

#[test]
fn example_cover() {
    let mut world = tests::world();
    for id in world.entities.keys().copied().collect::<Vec<_>>() {
        world.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut world.map).cells {
        cell.ground = 0x61;
    }
    let player = world.enter(tests::ch(&world), None).unwrap();
    world.players.get_mut(&player).unwrap().pos = Pos { x: 12.5, y: 100.5 };
    world
        .spawn_entity(0x120, Pos { x: 11.5, y: 100.5 }, vec![])
        .unwrap();
    let start = Pos { x: 10.5, y: 100.5 };
    world.bullets.push(Bullet {
        owner: 500,
        source_kind: 0x683,
        id: 1,
        friendly: false,
        start,
        pos: start,
        angle: 0.,
        born: 0,
        damage: 8,
        spec: world.content.objects[&0x683].projectiles[&0].clone(),
        hit: BTreeSet::new(),
        hit_enemy: false,
    });
    for _ in 0..5 {
        world.step();
    }
    assert_eq!(world.players[&player].character.hp, 100);
}
