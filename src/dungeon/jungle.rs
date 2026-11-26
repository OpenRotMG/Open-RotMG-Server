use crate::content::{Cell, Content, Map};
use anyhow::{Result, ensure};
use rand::{Rng, SeedableRng, seq::SliceRandom};

pub const COIL: u16 = 0xdc2;
pub const BOSS: u16 = 0xdc3;

// The period guide establishes branching circular clearings; no original server generator is preserved.
// https://web.archive.org/web/20120420071116/http://forums.wildshadow.com/wiki/realm-mad-god/forbidden-jungle
pub(super) fn generate_for(
    content: &Content,
    generator: &crate::content::game::geometry::Generator,
    seed: u64,
) -> Result<Map> {
    let [boss_kind, boss_totem, retreat] = generator.placements("boss-and-exit")?;
    let foliage = generator.placements::<6>("forest-edge")?;
    let [fire, shaman, hunter, warrior, totem, basilisk, baby] =
        generator.placements("room-actors")?;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let size = 120;
    let mut map = Map {
        elevation: None,
        width: size,
        height: size,
        cells: vec![
            Cell {
                ground: 0xff,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            (size * size) as usize
        ],
        spawns: vec![(60.5, 100.5)],
    };
    let mut rooms = vec![(2i32, 4i32, 0usize, 0usize)];
    let count = rng.gen_range(13..=17);
    while rooms.len() < count {
        let mut frontier = vec![];
        for (i, &(x, y, _, depth)) in rooms.iter().enumerate() {
            for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                let (x, y) = (x + dx, y + dy);
                if (0..5).contains(&x)
                    && (0..5).contains(&y)
                    && !rooms.iter().any(|r| r.0 == x && r.1 == y)
                {
                    frontier.push((x, y, i, depth + 1));
                }
            }
        }
        rooms.push(*frontier.choose(&mut rng).expect("bounded Jungle frontier"));
    }
    let boss = rooms.iter().enumerate().max_by_key(|(_, r)| r.3).unwrap().0;
    let center = |r: &(i32, i32, usize, usize)| (20 + r.0 * 20, 20 + r.1 * 20);
    let mut clear = |x: i32, y: i32, region: u8, rng: &mut rand::rngs::StdRng| {
        let c = &mut map.cells[(y * size + x) as usize];
        c.ground = if rng.gen_ratio(1, 5) { 0x56 } else { 0x1c };
        c.region = region;
    };
    for (i, r) in rooms.iter().enumerate() {
        let (cx, cy) = center(r);
        let radius = if i == boss { 11 } else { rng.gen_range(6..=8) };
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                if (x - cx).pow(2) + (y - cy).pow(2) <= radius * radius {
                    clear(x, y, (i + 2) as u8, &mut rng);
                }
            }
        }
        if i > 0 {
            let (px, py) = center(&rooms[r.2]);
            for y in cy.min(py) - 1..=cy.max(py) + 1 {
                for x in cx.min(px) - 1..=cx.max(px) + 1 {
                    clear(x, y, 0, &mut rng);
                }
            }
        }
    }
    let floors: Vec<_> = map.cells.iter().map(|c| c.ground != 0xff).collect();
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let i = (y * size + x) as usize;
            if !floors[i]
                && (-3..=3).any(|dy| {
                    (-3..=3).any(|dx| {
                        let (nx, ny) = (x + dx, y + dy);
                        (0..size).contains(&nx)
                            && (0..size).contains(&ny)
                            && floors[(ny * size + nx) as usize]
                    })
                })
            {
                map.cells[i].ground = 0x56;
                map.cells[i].object = Some(*foliage.choose(&mut rng).unwrap());
            } else if floors[i] && rng.gen_ratio(1, 35) {
                map.cells[i].object = Some(rng.gen_range(0xdd4..=0xdd7));
            }
        }
    }
    for (i, r) in rooms
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 0 && *i != boss)
    {
        let (cx, cy) = center(r);
        let placements = match i % 3 {
            0 => vec![
                (0, 0, fire),
                (-3, 0, shaman),
                (3, 0, hunter),
                (0, -3, warrior),
                (0, 3, warrior),
                (-2, 2, shaman),
            ],
            1 => vec![(0, 0, totem)],
            _ => vec![(0, 0, basilisk), (-2, 0, baby), (2, 0, baby)],
        };
        for (dx, dy, k) in placements {
            map.cells[((cy + dy) * size + cx + dx) as usize].object = Some(k);
        }
    }
    let (cx, cy) = center(&rooms[boss]);
    for dy in -5i32..=5 {
        for dx in -5i32..=5 {
            if dx.abs() + dy.abs() <= 8 {
                let c = &mut map.cells[((cy + dy) * size + cx + dx) as usize];
                c.ground = 0x02;
                c.object = None;
            }
        }
    }
    for (dx, dy) in [(-5, -5), (5, -5), (-5, 5), (5, 5)] {
        map.cells[((cy + dy) * size + cx + dx) as usize].object = Some(boss_totem);
    }
    for (dx, dy) in [
        (-7, -3),
        (-7, 3),
        (7, -3),
        (7, 3),
        (-3, -7),
        (3, -7),
        (-3, 7),
        (3, 7),
    ] {
        map.cells[((cy + dy) * size + cx + dx) as usize].object = Some(0xdc5);
    }
    map.cells[(cy * size + cx) as usize].object = Some(boss_kind);
    let c = &mut map.cells[(100 * size + 60) as usize];
    c.object = Some(retreat);
    c.region = 1;
    for c in &map.cells {
        ensure!(
            content.grounds.contains_key(&c.ground),
            "unknown Jungle ground"
        );
        if let Some(k) = c.object {
            content.get(k)?;
        }
    }
    tracing::info!(
        seed,
        rooms = rooms.len(),
        boss_room = boss,
        layout = ?rooms,
        "generated branching Forbidden Jungle"
    );
    Ok(map)
}
