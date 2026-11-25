//! Oryx's Castle actors are XML-owned (oryxCastle.xml); these cover the selected map and XML Guardian loot.
#[cfg(test)]
mod castle_contract {
    use super::super::*;
    use crate::{condition, endgame as finale};
    use finale::GUARDIANS;

    fn castle() -> (World, i32) {
        let content = Arc::new(crate::world::test_content());
        let map = Map::load(std::path::Path::new("data/oryx-castle.wmap"), &content).unwrap();
        let mut world = World::new(content.clone(), Arc::new(map)).unwrap();
        world.name = "Oryx's Castle".into();
        world.xml.template = Some(crate::endgame::CASTLE.into());
        world.encounter = true;
        world.prepare_endgame().unwrap();
        let mut character = Character::fresh(content.get(0x307).unwrap()).unwrap();
        character.name = "Castle".into();
        character.account = 1;
        character.id = 1;
        let player = world.enter(character, None).unwrap();
        world
            .apply_condition(player, condition::INVINCIBLE, 600_000)
            .unwrap();
        (world, player)
    }
    #[test]
    fn castle_map_matches_the_march_2012_wikiwiki_map() {
        // wikiwiki.jp rmd "Oryx's Castle" oryxcastlemap.png (CDN t=20120331234252); its cell = map cell + (3, 0).
        use sha2::{Digest, Sha256};
        let (world, _) = castle();
        let map = &world.map;
        let cells = |kind| -> String {
            (0..map.height)
                .flat_map(|y| (0..map.width).map(move |x| (x, y)))
                .filter(|&(x, y)| map.cells[(y * map.width + x) as usize].object == Some(kind))
                .map(|(x, y)| format!("{},{y};", x + 3))
                .collect()
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(cells(0xd70))),
            "117b62d0734bbe163568d188c1fa5ee10657c7fa47c480ffb747ee16e7516568"
        );
        assert_eq!(
            cells(0xd87),
            "108,22;114,22;111,33;111,45;108,53;114,53;70,91;152,91;26,92;196,92;23,96;29,96;193,96;199,96;77,98;145,98;65,127;157,127;62,131;68,131;154,131;160,131;"
        );
        // Hand-drawn start squares; each island's spawn cell lies on or beside its own square.
        let squares = [
            (25, 149, 27, 150),
            (195, 149, 197, 150),
            (65, 185, 66, 186),
            (157, 184, 158, 185),
        ];
        assert_eq!(map.spawns.len(), 4);
        for (x0, y0, x1, y1) in squares {
            assert_eq!(
                map.spawns
                    .iter()
                    .filter(|(x, y)| {
                        let (x, y) = (*x as i32 + 3, *y as i32);
                        (x0 - 1..=x1 + 1).contains(&x) && (y0 - 1..=y1 + 1).contains(&y)
                    })
                    .count(),
                1
            );
        }
    }

    #[test]
    fn guardians_drop_xml_defense_incantation_and_stone_sword_while_sword_and_armor_drop_nothing() {
        let content = crate::world::test_content();
        let game = content.game.as_ref().unwrap();
        for kind in GUARDIANS {
            let rows = &game.definitions[&kind].loot;
            let row = |item: u16| rows.iter().find(|r| r.pool == [item]).unwrap();
            assert_eq!(row(0xa20).min, 1, "guaranteed Defense for {kind:#x}");
            assert!(row(finale::INCANTATION).probability > 0. && row(0xc05).probability > 0.);
        }
        for kind in [0xd7c, 0xd7e] {
            assert!(game.definitions[&kind].loot.is_empty());
        }
    }
}
