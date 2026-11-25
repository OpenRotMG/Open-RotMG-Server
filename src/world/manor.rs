#[cfg(test)]
mod tests {
    use crate::dungeon::manor::*;
    use crate::world::*;

    fn arena() -> (World, i32) {
        let content = Arc::new(crate::world::test_content());
        let map =
            crate::dungeon::generate_selected(&content, "manor-of-the-immortals", 12351).unwrap();
        let mut world = World::new(content, Arc::new(map)).unwrap();
        world.name = "Manor of the Immortals".into();
        world.xml.template = Some("manor-of-the-immortals".into());
        let boss = world.actors().find(|e| e.kind == BOSS).unwrap().id;
        (world, boss)
    }

    fn secret_bookshelf_view() -> (World, i32, i32, Pos) {
        let (mut world, _) = arena();
        // XML Gothic Bookshelves are passive walls, not per-tick actors (realm-applicability decision).
        let (wall, outside, inside) = world
            .entities
            .values()
            .filter(|e| {
                e.kind == BOOKCASE && world.map.cell(e.pos.x, e.pos.y).unwrap().region == 250
            })
            .find_map(|e| {
                [(0., 1.), (1., 0.)].into_iter().find_map(|(dx, dy)| {
                    let outside = Pos {
                        x: e.pos.x + dx,
                        y: e.pos.y + dy,
                    };
                    let inside = Pos {
                        x: e.pos.x - dx,
                        y: e.pos.y - dy,
                    };
                    (world.walkable(outside) && world.walkable(inside))
                        .then_some((e.id, outside, inside))
                })
            })
            .expect("generated Manor has a secret bookshelf between walkable tiles");
        assert!(world.content.objects[&BOOKCASE].blocks_sight);
        assert_eq!(world.entities[&wall].hp, 3000);
        assert_eq!(world.entities[&wall].condition, 0);
        let (fixture, player) = super::super::movement_tests::fixture(0x18, false);
        let player = world
            .enter(fixture.players[&player].character.clone(), None)
            .unwrap();
        world.players.get_mut(&player).unwrap().pos = outside;
        eprintln!(
            "Manor seed 12351: wall={:?}, outside={outside:?}, inside={inside:?}",
            world.entities[&wall].pos
        );
        (world, player, wall, inside)
    }

    #[test]
    fn manor_secret_bookshelf_hides_unexplored_tiles_and_actors_until_opened() {
        let (mut world, player, wall, inside) = secret_bookshelf_view();
        let tile = (inside.x as i16, inside.y as i16);
        let wall_pos = world.entities[&wall].pos;
        let wall_tile = (wall_pos.x as i16, wall_pos.y as i16);
        let marker = world.spawn_entity(ROCK, inside, vec![]).unwrap();
        let outside = world.players[&player].pos;
        assert!(
            !world
                .room_sight((outside.x as i32, outside.y as i32))
                .contains(&tile)
        );
        assert_ne!(world.quest_target(player), Some(marker));
        world.updates(player, true).unwrap();
        let p = &world.players[&player];
        assert!(
            p.known.contains(&wall),
            "the unopened bookshelf face must be visible"
        );
        assert!(p.tiles.contains(&wall_tile));
        assert!(
            !p.tiles.contains(&tile),
            "unopened Manor bookshelf leaked terrain behind it"
        );
        assert!(
            !p.known.contains(&marker),
            "unopened Manor bookshelf leaked an actor behind it"
        );
        world.remove_entity(wall);
        world.updates(player, false).unwrap();
        let p = &world.players[&player];
        assert_eq!((p.pos.x, p.pos.y), (outside.x, outside.y));
        assert!(
            p.tiles.contains(&tile),
            "opened bookshelf did not reveal terrain to a stationary player"
        );
        assert!(p.known.contains(&marker));
        assert!(!p.known.contains(&wall));
    }

    #[test]
    fn manor_secret_room_refuses_a_blink_until_its_bookshelf_opens() {
        use super::super::sight::tests::{blink, refused};
        const CLOAK: u16 = 0xa5a;
        let (mut world, player, wall, inside) = secret_bookshelf_view();
        refused(&mut world, player, CLOAK, inside);
        world.remove_entity(wall);
        blink(&mut world, player, CLOAK, inside).unwrap();
        let p = &world.players[&player];
        assert_eq!((p.pos.x, p.pos.y), (inside.x, inside.y));
    }

    #[test]
    fn manor_secret_bookshelf_removal_invalidates_stationary_tile_scan() {
        let (mut world, player, wall, _) = secret_bookshelf_view();
        world.updates(player, true).unwrap();
        assert!(world.players[&player].tile_scan.is_some());
        world.remove_entity(wall);
        assert_eq!(
            world.players[&player].tile_scan, None,
            "Manor bookshelf removal retained the closed-room scan"
        );
    }

    #[test]
    fn manor_admitted_staff_projectiles_defeat_original_bookshelf_and_open_sight() {
        let (fixture, _) = super::super::movement_tests::fixture(0x18, false);
        let mut map = (*fixture.map).clone();
        for y in 0..map.height {
            let cell = &mut map.cells[(y * map.width + 10) as usize];
            cell.object = Some(if y == 8 { BOOKCASE } else { 0x180a });
            cell.region = if y == 8 { 250 } else { 0 };
        }
        let mut world = World::new(fixture.content.clone(), Arc::new(map)).unwrap();
        world.name = "Manor of the Immortals".into();
        world.xml.template = Some("manor-of-the-immortals".into());
        world.encounter = true;
        let outside = Pos { x: 8.5, y: 8.5 };
        let wall_pos = Pos { x: 10.5, y: 8.5 };
        let inside = (11, 8);
        let wall = world
            .entities
            .values()
            .find(|e| e.kind == BOOKCASE && e.pos.distance(wall_pos) == 0.)
            .unwrap()
            .id;
        assert_eq!(world.map.cell(wall_pos.x, wall_pos.y).unwrap().region, 250);
        assert_eq!(world.entities[&wall].hp, 3000);
        assert_eq!(world.entities[&wall].condition, 0);
        let mut character =
            Character::fresh(world.content.get(crate::content::WIZARD).unwrap()).unwrap();
        character.id = 1384;
        character.account = 1;
        character.name = "ManorStaff".into();
        character.items.fill(-1);
        character.items[0] = 0xb08;
        character
            .gain_experience(
                &world.content,
                crate::store::level_start(20).unwrap(),
                &mut rand::rngs::StdRng::seed_from_u64(1384),
            )
            .unwrap();
        let player = world
            .enter_at(
                character,
                None,
                Some(outside),
                AccountPresentation {
                    guild: None,
                    credits: 0,
                    fame: 0,
                    best_fame: BTreeMap::new(),
                },
                false,
            )
            .unwrap();
        assert_eq!(world.players[&player].character.level, 20);
        assert_eq!(world.players[&player].character.shots, 0);
        assert_eq!(world.players[&player].character.hits, 0);
        assert_eq!(world.players[&player].conditions(world.now), 0);
        world.updates(player, true).unwrap();
        assert!(world.players[&player].known.contains(&wall));
        assert!(!world.players[&player].tiles.contains(&inside));
        let start = Pos {
            x: outside.x + 0.3,
            y: outside.y,
        };
        let mut damaged = false;
        for _ in 0..40 {
            if !world.entities.contains_key(&wall) {
                break;
            }
            for _ in 0..2 {
                let bullet = world.players[&player].next_bullet;
                world
                    .shoot(player, world.now as i32, bullet, 0xb08, start, 0.)
                    .unwrap();
            }
            for _ in 0..10 {
                world.step();
                // A lethal hit waits for the journal; draining publishes the defeat before the next check.
                crate::world::xml::tests::drain_events(&mut world).unwrap();
                let p = &world.players[&player];
                assert!(
                    p.character.alive && p.character.hp > 0 && !p.death_pending,
                    "engine player became terminal: time={} hp={} alive={} pending={}",
                    world.now,
                    p.character.hp,
                    p.character.alive,
                    p.death_pending
                );
                if let Some(e) = world.entities.get(&wall) {
                    if e.hp < 3000 {
                        assert!(e.hp > 0);
                        assert_eq!(e.damage.get(&1384), Some(&(3000 - e.hp)));
                        assert!(p.character.hits > 0 && p.character.lifetime.projectile_hits > 0);
                        damaged = true;
                    }
                } else {
                    break;
                }
            }
        }
        assert!(
            damaged,
            "no surviving original-wall projectile damage observed"
        );
        assert!(
            !world.entities.contains_key(&wall),
            "staff did not defeat original bookshelf"
        );
        world.updates(player, false).unwrap();
        let p = &world.players[&player];
        assert_eq!((p.pos.x, p.pos.y), (outside.x, outside.y));
        assert!(p.tiles.contains(&inside));
        assert!(!p.known.contains(&wall));
        assert!(p.character.shots > 0 && p.character.lifetime.projectile_hits > 0);
        assert!(p.character.kills > 0 && p.character.lifetime.kills > 0);
        assert_eq!(p.character.manors, 0);
        eprintln!(
            "engine staff bookshelf: time={}ms shots={} hits={} projectile_hits={} kills={}",
            world.now,
            p.character.shots,
            p.character.hits,
            p.character.lifetime.projectile_hits,
            p.character.kills
        );
    }
}
