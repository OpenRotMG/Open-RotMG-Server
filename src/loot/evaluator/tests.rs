use super::*;
use crate::content::{Content, game};
use std::{collections::VecDeque, path::Path, sync::LazyLock};

static CONTENT: LazyLock<Content> = LazyLock::new(|| Content::load(Path::new("data")).unwrap());
const ACTOR: u16 = 0xfffe;
fn limits(n: usize) -> PlanningLimits {
    PlanningLimits {
        max_allocations: NonZeroUsize::new(n).unwrap(),
        max_contributions: NonZeroUsize::new(128).unwrap(),
    }
}
fn context(kind: u16) -> DefeatContext {
    DefeatContext {
        world: 7,
        encounter: Some(19),
        actor: 23,
        event: 5,
        kind,
    }
}
fn credit(character: i32, effective_damage: u64) -> Contribution {
    Contribution {
        character,
        effective_damage,
        world: 7,
        encounter: Some(19),
        alive: true,
        departing: false,
    }
}
fn fixture_content() -> Content {
    let mut content = CONTENT.clone();
    let mut actor = content.objects[&0x600].clone();
    actor.kind = ACTOR;
    actor.name = "Loot Fixture".into();
    actor.hp = 1000;
    actor.projectiles.clear();
    content.objects.insert(ACTOR, actor);
    content
}
fn fixture_inputs(body: &str) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("game/123.5.1/game.xml".into(), br#"<Game schema="1"><Include file="rules.xml"/><Root object="Loot Fixture"/></Game>"#.to_vec()),
        ("game/123.5.1/rules.xml".into(), format!(r#"<Objects>
            <Object type="65534" id="Loot Fixture" disposition="active" provenance="authored" origin="isolated loot test" reason="explicit fixture properties, not historical actor"><Behavior>Wander</Behavior>{body}</Object>
            <Object type="0xa22" id="Health Potion" disposition="native" nativeService="inventory" provenance="authored" origin="isolated loot test" reason="explicit test item"/>
            </Objects>"#).into_bytes()),
        ("objects.xml".into(), std::fs::read("data/objects.xml").unwrap()),
        ("grounds.xml".into(), std::fs::read("data/grounds.xml").unwrap()),
    ])
}
fn fixture(body: &str) -> GameData {
    game::compile_fixture(&fixture_content(), fixture_inputs(body)).unwrap()
}
#[derive(Clone, Debug)]
enum Draw {
    Unit(f64),
    Index(usize, usize),
}
struct Script(VecDeque<Draw>);
impl Script {
    fn new(draws: Vec<Draw>) -> Self {
        Self(draws.into())
    }
    fn done(&self) {
        assert!(self.0.is_empty(), "unconsumed draws {:?}", self.0);
    }
}
impl Draws for Script {
    fn unit(&mut self) -> Result<f64> {
        match self.0.pop_front().expect("unexpected unit draw") {
            Draw::Unit(n) => Ok(n),
            other => panic!("unit instead of {other:?}"),
        }
    }
    fn index(&mut self, upper: usize) -> Result<usize> {
        match self.0.pop_front().expect("unexpected index draw") {
            Draw::Index(expected, n) => {
                assert_eq!(upper, expected);
                Ok(n)
            }
            other => panic!("index({upper}) instead of {other:?}"),
        }
    }
}
fn sample(game: &GameData, ledger: &[Contribution], draws: Vec<Draw>) -> AllocationPlan {
    let mut script = Script::new(draws);
    let plan = evaluate_draws(game, context(ACTOR), ledger, 71, limits(4096), &mut script).unwrap();
    script.done();
    plan
}
#[test]
fn xml_loot_allocates_to_eligible_owners_with_row_minimum_and_cap() {
    let game = fixture(
        r#"
        <SoulboundLoot prob=".3" threshold=".15" min="2">Health Potion</SoulboundLoot>
        <SoulboundLoot threshold=".15" max="1">Health Potion</SoulboundLoot>
        <Loot threshold=".15">Health Potion</Loot>"#,
    );
    let ledger = [credit(11, 149), credit(12, 150), credit(13, 151)];
    let draws = vec![
        Draw::Index(2, 1),
        Draw::Unit(0.3),
        Draw::Unit(0.299),
        Draw::Index(2, 1),
    ];
    let plan = sample(&game, &ledger, draws);
    let owners: Vec<_> = plan.allocations().iter().map(Allocation::owner).collect();
    assert_eq!(
        owners,
        [
            Owner::Character(13),
            Owner::Character(12),
            Owner::Character(12),
            Owner::Public
        ]
    );
}
