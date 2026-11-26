pub mod abyss;
pub mod beach;
pub mod davy;
pub mod jungle;
pub mod lab;
pub mod manor;
pub mod pirate;
pub mod snake;
pub mod spider;
pub mod sprite;
pub mod tomb;
pub mod trench;
pub mod undead;

// Exit identities are historical rule inputs, not registrations of playable worlds.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalDungeon {
    PirateCave,
    SpiderDen,
    ForbiddenJungle,
    SnakePit,
    SpriteWorld,
    UndeadLair,
    AbyssOfDemons,
    TombOfTheAncients,
    OceanTrench,
    ManorOfTheImmortals,
}
impl HistoricalDungeon {
    pub const ALL: [Self; 10] = [
        Self::PirateCave,
        Self::SpiderDen,
        Self::ForbiddenJungle,
        Self::SnakePit,
        Self::SpriteWorld,
        Self::UndeadLair,
        Self::AbyssOfDemons,
        Self::TombOfTheAncients,
        Self::OceanTrench,
        Self::ManorOfTheImmortals,
    ];
}

pub const DUNGEONS: [(u16, &str, &str, u16); 13] = [
    (0x717, "pirate-cave", "Pirate Cave", 0x927),
    (0x718, "snake-pit", "Snake Pit", 0x917),
    (0x70c, "sprite-world", "Sprite World", 0xd06),
    (0x71a, "undead-lair", "Undead Lair", 0xd90),
    (0x71b, "abyss-of-demons", "Abyss of Demons", abyss::MALPHAS),
    (0x730, "ocean-trench", "Ocean Trench", trench::THESSAL),
    (0x719, "spider-den", "Spider Den", spider::BOSS),
    (0x890, "mad-lab", lab::NAME, lab::BOSS),
    (0x733, "forbidden-jungle", "Forbidden Jungle", jungle::BOSS),
    (
        0x739,
        "manor-of-the-immortals",
        "Manor of the Immortals",
        manor::SUMMONER,
    ),
    (0x742, "beachzone", "Beachzone", beach::BOSS),
    (0x741, davy::TEMPLATE, davy::NAME, davy::BOSS),
    (
        tomb::PORTAL,
        "tomb-of-the-ancients",
        "Tomb of the Ancients",
        tomb::ANCHOR,
    ),
];

pub(crate) fn generate(
    content: &crate::content::Content,
    world: &crate::content::game::World,
    generator: &crate::content::game::geometry::Generator,
    seed: impl FnOnce() -> u64,
) -> anyhow::Result<crate::content::Map> {
    use crate::content::game::geometry::GeneratorKind::*;
    match generator.kind {
        Pirate => pirate::generate_for(content, generator, seed()),
        Snake => snake::generate_for(content, generator, seed()),
        Sprite => sprite::generate_for(content, generator, seed()),
        Undead => undead::generate_for(content, generator, seed()),
        Abyss => abyss::generate_for(content, generator, seed()),
        Trench => trench::generate_for(content, generator, seed()),
        Spider => spider::generate_for(content, generator, seed()),
        Lab => lab::generate_for(content, world, generator, seed()),
        Jungle => jungle::generate_for(content, generator, seed()),
        Manor => manor::generate_for(content, generator, seed()),
        Beach => beach::generate_for(content, generator),
        Davy => davy::generate_for(content, generator, seed()),
        Tomb => tomb::generate_for(content, generator, seed()),
    }
}

#[cfg(test)]
pub(crate) fn test_generator<'a>(
    content: &'a crate::content::Content,
    id: &str,
) -> &'a crate::content::game::geometry::Generator {
    let game = content.game.as_ref().unwrap();
    let world = game.world_template(id).unwrap();
    let crate::content::game::geometry::Geometry::Generator(g) =
        &game.maps[world.map as usize].geometry
    else {
        panic!("test expected linked generator for {id}")
    };
    g
}

#[cfg(test)]
pub(crate) fn generate_selected(
    content: &crate::content::Content,
    id: &str,
    seed: u64,
) -> anyhow::Result<crate::content::Map> {
    let world = content.game.as_ref().unwrap().world_template(id).unwrap();
    generate(content, world, test_generator(content, id), || seed)
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
impl Rect {
    /// The cell `along` this rect's `side` edge, `out` cells outside it.
    fn at(self, side: char, along: i32, out: i32) -> (i32, i32) {
        match side {
            'N' => (self.x + along, self.y - out),
            'S' => (self.x + along, self.y + self.h - 1 + out),
            'W' => (self.x - out, self.y + along),
            _ => (self.x + self.w - 1 + out, self.y + along),
        }
    }
    fn span(a: (i32, i32), b: (i32, i32)) -> Rect {
        Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs() + 1,
            h: (a.1 - b.1).abs() + 1,
        }
    }
    fn grow(self, g: i32) -> Rect {
        Rect {
            x: self.x - g,
            y: self.y - g,
            w: self.w + 2 * g,
            h: self.h + 2 * g,
        }
    }
    fn overlaps(self, o: Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}
fn opposite(side: char) -> char {
    match side {
        'N' => 'S',
        'S' => 'N',
        'W' => 'E',
        _ => 'W',
    }
}

fn rect_bounds(rects: impl Iterator<Item = Rect> + Clone) -> (i32, i32, i32, i32) {
    (
        rects.clone().map(|r| r.x).min().unwrap_or(0),
        rects.clone().map(|r| r.y).min().unwrap_or(0),
        rects.clone().map(|r| r.x + r.w).max().unwrap_or(0),
        rects.map(|r| r.y + r.h).max().unwrap_or(0),
    )
}
