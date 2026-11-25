use super::*;
use crate::content::{
    Content,
    game::{self, Field, Value as GameValue},
};
use std::{collections::VecDeque, path::Path, sync::LazyLock};

static CONTENT: LazyLock<Content> = LazyLock::new(|| Content::load(Path::new("data")).unwrap());
static GAME: LazyLock<GameData> =
    LazyLock::new(|| GameData::load(Path::new("data"), &CONTENT).unwrap());
const ACTOR: u16 = 0xfffe;
const ONE: u16 = 0xfff1;
const TWO: u16 = 0xfff2;
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
    for (id, name) in [(ONE, "Loot One"), (TWO, "Loot Two")] {
        let mut item = content.objects[&0xa22].clone();
        item.kind = id;
        item.name = name.into();
        item.tier = Some(250);
        item.slot_type = 1;
        content.objects.insert(id, item);
    }
    content
}
fn fixture_inputs(body: &str) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("game/123.5.1/game.xml".into(), br#"<Game schema="1"><Include file="rules.xml"/><Root object="Loot Fixture"/></Game>"#.to_vec()),
        ("game/123.5.1/rules.xml".into(), format!(r#"<Objects>
            <Object type="65534" id="Loot Fixture" disposition="active" provenance="authored" origin="isolated loot test" reason="explicit fixture properties, not historical actor"><Behavior>Wander</Behavior>{body}</Object>
            <Object type="65521" id="Loot One" disposition="native" nativeService="inventory" provenance="authored" origin="isolated loot test" reason="explicit test item"/>
            <Object type="65522" id="Loot Two" disposition="native" nativeService="inventory" provenance="authored" origin="isolated loot test" reason="explicit test item"/>
            </Objects>"#).into_bytes()),
        ("objects.xml".into(), std::fs::read("data/objects.xml").unwrap()),
        ("grounds.xml".into(), std::fs::read("data/grounds.xml").unwrap()),
    ])
}
fn fixture(body: &str) -> GameData {
    game::compile_fixture(&fixture_content(), fixture_inputs(body)).unwrap()
}
fn row(attrs: &str) -> String {
    format!("<SoulboundLoot {attrs}>Loot One</SoulboundLoot>")
}
fn pool(attrs: &str) -> String {
    format!("<SoulboundLoot tier=\"250\" {attrs}>Weapon</SoulboundLoot>")
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
fn owners(plan: &AllocationPlan) -> Vec<Owner> {
    plan.allocations().iter().map(Allocation::owner).collect()
}

#[test]
fn xml_rewards_phase1_independent_equality_minimum_and_maximum_vector() {
    let expectation: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/xml_contract/expectations.json"
    ))
    .unwrap();
    let ledger = [credit(11, 149), credit(12, 150), credit(13, 151)];
    let game = fixture(&row("prob=\".3\" threshold=\".15\" min=\"2\""));
    let draws = vec![Draw::Index(2, 1), Draw::Unit(0.3), Draw::Unit(0.299)];
    let plan = sample(&game, &ledger, draws.clone());
    assert_eq!(owners(&plan), [Owner::Character(13), Owner::Character(12)]);
    assert_eq!(expectation["loot"]["final_owners"], json!([13, 12]));
    let capped = fixture(&row("prob=\".3\" threshold=\".15\" max=\"1\""));
    assert_eq!(
        owners(&sample(&capped, &ledger, draws)),
        [Owner::Character(13)]
    );
    assert_eq!(expectation["loot"]["maximum_one_final_owners"], json!([13]));
}

#[test]
fn xml_rewards_positive_damage_threshold_zero_equality_and_next_below() {
    for (threshold, expected) in [
        ("0", vec![11, 12, 13]),
        (".15", vec![12, 13]),
        ("1", vec![]),
    ] {
        let game = fixture(&row(&format!("threshold=\"{threshold}\"")));
        let ledger = [
            credit(10, 0),
            credit(11, 149),
            credit(12, 150),
            credit(13, 151),
        ];
        let mut script = Script::new(match expected.len() {
            3 => vec![Draw::Index(3, 2), Draw::Index(2, 1)],
            2 => vec![Draw::Index(2, 1)],
            _ => vec![],
        });
        let plan =
            evaluate_draws(&game, context(ACTOR), &ledger, 0, limits(9), &mut script).unwrap();
        script.done();
        assert_eq!(
            owners(&plan),
            expected
                .into_iter()
                .map(Owner::Character)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn xml_rewards_durable_character_aggregation_living_scope_and_effective_not_overkill_credit() {
    let game = fixture(&row("threshold=\".15\""));
    let mut dead = credit(20, 1000);
    dead.alive = false;
    let mut leaving = credit(21, 1000);
    leaving.departing = true;
    let mut foreign_world = credit(22, 1000);
    foreign_world.world = 8;
    let mut foreign_encounter = credit(23, 1000);
    foreign_encounter.encounter = Some(20);
    let ledger = [
        credit(11, 100),
        credit(11, 50),
        credit(12, 149),
        credit(13, 150),
        credit(14, 0),
        dead,
        leaving,
        foreign_world,
        foreign_encounter,
    ];
    let before = serde_json::to_vec(&ledger).unwrap();
    let data = serde_json::to_vec(&game).unwrap();
    // Two durable characters remain distinct even if the coordinator knows the same account.
    assert_eq!(
        owners(&sample(&game, &ledger, vec![Draw::Index(2, 1)])),
        [Owner::Character(11), Owner::Character(13)]
    );
    assert_eq!(serde_json::to_vec(&ledger).unwrap(), before);
    assert_eq!(serde_json::to_vec(&game).unwrap(), data);
    let full = fixture(&row("threshold=\"1\""));
    // Final 900 attempted damage against 1 HP is supplied as 1 effective HP, not 900.
    let effective = [credit(30, 999), credit(31, 999), credit(31, 1)];
    assert_eq!(
        owners(&sample(&full, &effective, vec![])),
        [Owner::Character(31)]
    );
}

#[test]
fn xml_rewards_p0_p1_equality_and_uniform_sorted_pool_endpoints() {
    let game = fixture(&(pool("prob=\"0\"") + &pool("prob=\"1\"") + &pool("prob=\".3\"")));
    let plan = sample(
        &game,
        &[credit(11, 1)],
        vec![Draw::Index(2, 0), Draw::Unit(0.3)],
    );
    assert_eq!(
        plan.allocations()
            .iter()
            .map(Allocation::item)
            .collect::<Vec<_>>(),
        [ONE]
    );
    let plan = sample(
        &game,
        &[credit(11, 1)],
        vec![Draw::Index(2, 1), Draw::Unit(0.299), Draw::Index(2, 0)],
    );
    assert_eq!(
        plan.allocations()
            .iter()
            .map(Allocation::item)
            .collect::<Vec<_>>(),
        [TWO, ONE]
    );
    assert_eq!(plan.allocations()[1].row(), 2);
}

#[test]
fn xml_rewards_fair_order_max_stops_rolls_and_pool_choices_follow_recipient_phase() {
    let game = fixture(&pool("prob=\".5\" max=\"1\""));
    let plan = sample(
        &game,
        &[credit(13, 1), credit(11, 1), credit(12, 1)],
        vec![
            Draw::Index(3, 2),
            Draw::Index(2, 1),
            Draw::Unit(0.1),
            Draw::Index(2, 1),
        ],
    );
    assert_eq!(owners(&plan), [Owner::Character(11)]);
    assert_eq!(plan.allocations()[0].item(), TWO);
    let game = fixture(&pool("prob=\".5\""));
    let plan = sample(
        &game,
        &[credit(12, 1), credit(11, 1)],
        vec![
            Draw::Index(2, 1),
            Draw::Unit(0.1),
            Draw::Unit(0.2),
            Draw::Index(2, 0),
            Draw::Index(2, 1),
        ],
    );
    assert_eq!(owners(&plan), [Owner::Character(11), Owner::Character(12)]);
}

#[test]
fn xml_rewards_fair_topup_first_without_success_then_cycle_only_above_recipient_count() {
    let game = fixture(&row("prob=\".5\" min=\"4\""));
    let plan = sample(
        &game,
        &[credit(11, 1), credit(12, 1), credit(13, 1)],
        vec![
            Draw::Index(3, 0),
            Draw::Index(2, 0),
            Draw::Unit(0.1),
            Draw::Unit(0.6),
            Draw::Unit(0.6),
        ],
    );
    assert_eq!(
        owners(&plan),
        [
            Owner::Character(12),
            Owner::Character(13),
            Owner::Character(11),
            Owner::Character(12)
        ]
    );
    let solo = fixture(&(row("prob=\"0\" min=\"2\"") + &row("prob=\"0\" min=\"2\"")));
    let plan = sample(&solo, &[credit(11, 1)], vec![]);
    assert_eq!(owners(&plan), [Owner::Character(11); 4]);
    assert_eq!(
        plan.allocations()
            .iter()
            .map(Allocation::row)
            .collect::<Vec<_>>(),
        [0, 0, 1, 1]
    );
}

#[test]
fn xml_rewards_repeated_identical_rows_are_independent_and_no_eligible_means_zero_draws() {
    let game = fixture(&(row("prob=\".3\"") + &row("prob=\".3\"")));
    let plan = sample(
        &game,
        &[credit(11, 1)],
        vec![Draw::Unit(0.2), Draw::Unit(0.2)],
    );
    assert_eq!(plan.allocations().len(), 2);
    assert_eq!(plan.allocations()[0].row(), 0);
    assert_eq!(plan.allocations()[1].row(), 1);
    let min = fixture(&row("min=\"2\""));
    assert!(sample(&min, &[], vec![]).allocations().is_empty());
    let noops = fixture(&(row("prob=\"0\"") + &row("max=\"0\"")));
    assert!(
        sample(&noops, &[credit(11, 1), credit(12, 1)], vec![])
            .allocations()
            .is_empty()
    );
}

#[test]
fn xml_rewards_public_once_without_contributors_and_explicit_threshold_gate() {
    let game = fixture("<Loot prob=\".3\" min=\"2\">Loot One</Loot>");
    let empty = sample(&game, &[], vec![Draw::Unit(0.3)]);
    let many = sample(
        &game,
        &[credit(11, 1), credit(12, 1)],
        vec![Draw::Unit(0.3)],
    );
    assert_eq!(empty, many);
    assert_eq!(owners(&empty), [Owner::Public; 2]);
    let gated = fixture("<Loot threshold=\".15\" min=\"2\">Loot One</Loot>");
    assert!(
        sample(&gated, &[credit(11, 149)], vec![])
            .allocations()
            .is_empty()
    );
    assert_eq!(
        owners(&sample(&gated, &[credit(11, 150)], vec![])),
        [Owner::Public; 2]
    );
    let mixed = fixture(&("<Loot>Loot One</Loot>".to_string() + &row("")));
    assert_eq!(
        owners(&sample(&mixed, &[credit(11, 1)], vec![])),
        [Owner::Public, Owner::Character(11)]
    );
}

#[test]
fn xml_rewards_seeded_fair_recipient_permutation_has_independent_bounded_frequency_check() {
    let game = fixture(&row("max=\"1\""));
    let ledger = [credit(11, 1), credit(12, 1), credit(13, 1)];
    let mut counts = BTreeMap::<Owner, usize>::new();
    for seed in 0..6000 {
        let plan = evaluate(&game, context(ACTOR), &ledger, seed, limits(8)).unwrap();
        *counts.entry(plan.allocations()[0].owner()).or_default() += 1;
    }
    assert_eq!(counts.len(), 3);
    for count in counts.values() {
        assert!((1800..=2200).contains(count), "{counts:?}");
    }
}

#[test]
fn xml_rewards_independently_enumerated_two_character_minimum_changes_final_item_probability() {
    let game = fixture(&pool("prob=\".5\" min=\"2\" max=\"2\""));
    let mut observed = BTreeMap::<(Owner, u16), usize>::new();
    // 2 permutations × 4 Bernoulli outcomes × 4 uniform item outcomes = 32 equiprobable cases.
    for permutation in 0..2 {
        for first in [0.25, 0.75] {
            for second in [0.25, 0.75] {
                for a in 0..2 {
                    for b in 0..2 {
                        let plan = sample(
                            &game,
                            &[credit(11, 1), credit(12, 1)],
                            vec![
                                Draw::Index(2, permutation),
                                Draw::Unit(first),
                                Draw::Unit(second),
                                Draw::Index(2, a),
                                Draw::Index(2, b),
                            ],
                        );
                        assert_eq!(plan.allocations().len(), 2);
                        for allocation in plan.allocations() {
                            *observed
                                .entry((allocation.owner(), allocation.item()))
                                .or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    for owner in [Owner::Character(11), Owner::Character(12)] {
        assert_eq!(observed[&(owner, ONE)], 16); // Final item chance 1/2, not raw p/pool=1/4.
        assert_eq!(observed[&(owner, TWO)], 16);
    }
    let facts = definition_facts(&game, ACTOR).unwrap();
    assert_eq!(
        facts["rows"][0]["attempt_item_probability_before_min_max"],
        0.25
    );
    assert_eq!(
        facts["rows"][0]["final_probability"]["status"],
        "not_projected"
    );
}

#[test]
fn xml_rewards_all_rows_preflight_invalid_numbers_pools_identity_and_minmax_before_draws() {
    let original = fixture(&(row("prob=\".5\"") + &row("prob=\".5\"")));
    for mode in 0..10 {
        let mut game = original.clone();
        let row = &mut game.definitions.get_mut(&ACTOR).unwrap().loot[1];
        match mode {
            0 => row.probability = f64::NAN,
            1 => row.probability = 1.01,
            2 => row.threshold = f64::INFINITY,
            3 => row.threshold = -0.01,
            4 => row.pool.clear(),
            5 => row.pool = vec![TWO, ONE],
            6 => row.pool = vec![0xffff],
            7 => {
                row.min = 2;
                row.max = Some(1);
            }
            8 => row.source.object = "foreign".into(),
            9 => row.tier = Some(9),
            _ => unreachable!(),
        }
        let mut script = Script::new(vec![]);
        let error = format!(
            "{:#}",
            evaluate_draws(
                &game,
                context(ACTOR),
                &[credit(11, 1)],
                0,
                limits(8),
                &mut script
            )
            .unwrap_err()
        );
        assert!(
            error.contains("row=1") && error.contains("rules.xml"),
            "{error}"
        );
        script.done();
    }
}

#[test]
fn xml_rewards_checked_input_overflow_required_limits_and_giant_minimum_zero_draws() {
    let game = fixture(&row("prob=\".5\""));
    for ledger in [
        vec![credit(0, 1)],
        vec![credit(11, u64::MAX), credit(11, 1)],
        {
            let mut conflicting = credit(11, 1);
            conflicting.alive = false;
            vec![credit(11, 1), conflicting]
        },
    ] {
        let mut script = Script::new(vec![]);
        assert!(evaluate_draws(&game, context(ACTOR), &ledger, 0, limits(8), &mut script).is_err());
        script.done();
    }
    let mut script = Script::new(vec![]);
    let low = PlanningLimits {
        max_contributions: NonZeroUsize::new(1).unwrap(),
        ..limits(8)
    };
    assert!(
        evaluate_draws(
            &game,
            context(ACTOR),
            &[credit(11, 1), credit(12, 1)],
            0,
            low,
            &mut script
        )
        .is_err()
    );
    let giant = fixture(&row("min=\"4294967295\""));
    let error = format!(
        "{:#}",
        evaluate_draws(
            &giant,
            context(ACTOR),
            &[credit(11, 1)],
            0,
            limits(8),
            &mut script
        )
        .unwrap_err()
    );
    assert!(
        error.contains("minimum exceeds limit") && error.contains("needed=4294967295"),
        "{error}"
    );
    script.done();
}

#[test]
fn xml_rewards_sampled_work_limit_is_error_not_partial_success_and_retry_keeps_seed() {
    let game = fixture(&row(""));
    let ledger = [credit(11, 1), credit(12, 1)];
    let before = ledger;
    assert!(
        format!(
            "{:#}",
            evaluate(&game, context(ACTOR), &ledger, 42, limits(1)).unwrap_err()
        )
        .contains("sampled planning allocation limit")
    );
    let plan = evaluate(&game, context(ACTOR), &ledger, 42, limits(2)).unwrap();
    assert_eq!(
        plan,
        evaluate(&game, context(ACTOR), &ledger, 42, limits(2)).unwrap()
    );
    assert_eq!(before, ledger);
    assert_eq!(plan.seed(), 42);
    let rare = fixture(&row("prob=\".001\""));
    // Worst case is 2 allocations but the fixed sample is empty and fits limit1.
    let mut script = Script::new(vec![Draw::Index(2, 1), Draw::Unit(0.9), Draw::Unit(0.9)]);
    assert!(
        evaluate_draws(&rare, context(ACTOR), &ledger, 42, limits(1), &mut script)
            .unwrap()
            .allocations()
            .is_empty()
    );
    script.done();
}

#[test]
fn xml_rewards_exact_owner_demand_above_native_live_capacity_is_not_an_entitlement_clamp() {
    for n in [8, 9, 1025] {
        let game = fixture(&row(&format!("prob=\"0\" min=\"{n}\"")));
        let plan = evaluate(&game, context(ACTOR), &[credit(11, 1)], 7, limits(n)).unwrap();
        assert_eq!(plan.demand(), BTreeMap::from([(Owner::Character(11), n)]));
        assert_eq!(plan.allocations().len(), n);
        assert!(evaluate(&game, context(ACTOR), &[credit(11, 1)], 7, limits(n - 1)).is_err());
        // Native packing/128 live bags can backpressure this cached plan; no availability input changes RNG.
        plan.validate(&game, context(ACTOR), &[credit(11, 1)], limits(n))
            .unwrap();
    }
}

#[test]
fn xml_rewards_immutable_plan_restore_source_identity_and_cached_retry_without_rng() {
    let game = fixture(&row("min=\"2\""));
    let ledger = [credit(11, 1)];
    let plan = evaluate(&game, context(ACTOR), &ledger, 19, limits(8)).unwrap();
    let bytes = serde_json::to_vec(&plan).unwrap();
    let restored: AllocationPlan = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, plan);
    for _ in 0..3 {
        restored
            .validate(&game, context(ACTOR), &ledger, limits(8))
            .unwrap();
    }
    assert_eq!(
        game.definitions[&ACTOR].loot[plan.allocations()[0].row()]
            .source
            .file,
        "game/123.5.1/rules.xml"
    );
    for key in [
        "plan_version",
        "allocation_version",
        "content_fingerprint",
        "rules_fingerprint",
        "defeat",
        "allocations",
    ] {
        let mut changed = serde_json::to_value(&plan).unwrap();
        match key {
            "plan_version" => changed[key] = json!(9),
            "allocation_version" | "content_fingerprint" | "rules_fingerprint" => {
                changed[key] = json!("changed")
            }
            "defeat" => changed[key]["event"] = json!(6),
            "allocations" => changed[key][0]["item"] = json!(TWO),
            _ => unreachable!(),
        }
        let changed: AllocationPlan = serde_json::from_value(changed).unwrap();
        assert!(
            changed
                .validate(&game, context(ACTOR), &ledger, limits(8))
                .is_err(),
            "{key}"
        );
    }
    let reordered = fixture(&row("min=\"3\""));
    assert!(
        plan.validate(&reordered, context(ACTOR), &ledger, limits(8))
            .is_err()
    );
}

#[test]
fn xml_rewards_same_compiler_xml_mutations_change_allocations_and_export_invalid_source_refuses() {
    let content = fixture_content();
    let compile = |body: &str| game::compile_fixture(&content, fixture_inputs(body)).unwrap();
    for (before_xml, after_xml, before_count, count, item) in [
        (row("prob=\"0\""), row("prob=\"1\""), 0, 1, ONE),
        (
            row("threshold=\".15\""),
            row("threshold=\".151\""),
            1,
            0,
            ONE,
        ),
        (row("prob=\"0\""), row("prob=\"0\" min=\"2\""), 0, 2, ONE),
        (row("max=\"2\""), row("max=\"0\""), 1, 0, ONE),
        (
            row(""),
            "<SoulboundLoot>Loot Two</SoulboundLoot>".into(),
            1,
            1,
            TWO,
        ),
    ] {
        let base = compile(&before_xml);
        assert_eq!(
            sample(&base, &[credit(11, 150)], vec![])
                .allocations()
                .len(),
            before_count
        );
        let changed = compile(&after_xml);
        assert_ne!(base.fingerprint, changed.fingerprint);
        assert_ne!(
            definition_facts(&base, ACTOR).unwrap(),
            definition_facts(&changed, ACTOR).unwrap()
        );
        let plan = sample(&changed, &[credit(11, 150)], vec![]);
        assert_eq!(plan.allocations().len(), count);
        assert!(plan.allocations().iter().all(|a| a.item() == item));
    }
    for body in [
        row("prob=\"NaN\""),
        row("threshold=\"1.1\""),
        row("min=\"-1\""),
        row("min=\"1.5\""),
        row("min=\"2\" max=\"1\""),
        "<SoulboundLoot tier=\"249\">Weapon</SoulboundLoot>".into(),
        "<SoulboundLoot>Not selected</SoulboundLoot>".into(),
    ] {
        assert!(
            game::compile_fixture(&content, fixture_inputs(&body)).is_err(),
            "{body}"
        );
    }
}

#[test]
fn xml_rewards_catalogue_v2_conditions_source_fingerprint_repeats_and_no_legacy_probability_tables()
{
    let game = fixture(
        &(row("prob=\".3\"")
            + &row("prob=\".3\" threshold=\".15\" min=\"1\" max=\"2\"")
            + "<Loot prob=\".3\">Loot One</Loot>"
            + &row("")),
    );
    let snapshot = crate::loot::catalogue::snapshot(&game, Some("test-revision")).unwrap();
    assert_eq!(snapshot["schema_version"], 2);
    assert_eq!(snapshot["content_fingerprint"], game.fingerprint);
    assert_eq!(snapshot["allocation_version"], ALLOCATION_VERSION);
    assert_eq!(snapshot["revision"], "test-revision");
    assert!(snapshot.get("groups").is_none());
    let rows = snapshot["objects"][0]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0]["bernoulli_attempt_probability"],
        rows[1]["bernoulli_attempt_probability"]
    );
    assert_ne!(rows[0], rows[1]);
    assert_ne!(rows[0]["eligibility"], rows[2]["eligibility"]);
    assert_eq!(rows[3]["bernoulli_attempt_probability"], 1.); // Missing prob is compiled default, not ignored/no-op.
    assert_eq!(rows[1]["source_row"]["threshold"], 0.15);
    assert_eq!(rows[1]["source_row"]["min"], 1);
    assert_eq!(rows[1]["source_row"]["max"], 2);
    assert_eq!(rows[0]["source_row"]["source"]["object"], "Loot Fixture");
    assert_eq!(rows[1]["guarantee_is_per_player"], false);
    let code = include_str!("../catalogue.rs");
    for forbidden in [
        "rules(kind)",
        "table(kind)",
        "damage_threshold",
        "tenfold",
        "portal_drop(",
    ] {
        assert!(!code.contains(forbidden));
    }
}

struct Constant {
    unit: f64,
    calls: usize,
}
impl Draws for Constant {
    fn unit(&mut self) -> Result<f64> {
        self.calls += 1;
        Ok(self.unit)
    }
    fn index(&mut self, upper: usize) -> Result<usize> {
        self.calls += 1;
        Ok(upper - 1)
    }
}
#[test]
fn xml_rewards_actual_gamedata_stheno_thessal_literal_values_and_portal_declarations() {
    let game = &*GAME;
    assert_eq!(
        game.fingerprint,
        "c45004a8b7475507fc5b8935020a15d168da3b5a45e53db2b36835d534dea54d"
    );
    for (file, actor, tag, child, fields) in [
        (
            "snakePit",
            "Stheno the Snake Queen",
            "SoulboundLoot",
            "Wand of the Bulwark",
            &[("prob", ".001"), ("threshold", ".15")][..],
        ),
        (
            "snakePit",
            "Stheno the Snake Queen",
            "SoulboundLoot",
            "Potion of Speed",
            &[("prob", ".3"), ("min", "1")],
        ),
        (
            "ghostShip",
            "Beach Bum",
            "Behavior",
            "MetamorphoseAtDeath",
            &[("prob", "0.7"), ("childId", "Beachzone Portal")],
        ),
    ] {
        let raw = std::fs::read_to_string(format!(
            "data/reference/release-7.0/xml/dungeons/{file}.xml"
        ))
        .unwrap();
        let doc = roxmltree::Document::parse(&raw).unwrap();
        let row = doc
            .descendants()
            .find(|n| n.attribute("id") == Some(actor))
            .unwrap()
            .children()
            .find(|n| n.has_tag_name(tag) && n.text() == Some(child))
            .unwrap();
        for &(field, value) in fields {
            assert_eq!(row.attribute(field), Some(value), "{actor}/{child}");
        }
    }
    let stheno = &game.definitions[&0x917];
    let bulwark = stheno.loot.iter().position(|r| r.pool == [0xc04]).unwrap();
    assert_eq!(
        (
            stheno.loot[bulwark].probability,
            stheno.loot[bulwark].threshold
        ),
        (0.001, 0.15)
    );
    assert!(
        stheno
            .loot
            .iter()
            .any(|r| r.pool == [0xa21] && r.probability == 0.3 && r.min == 1)
    );
    assert!(
        stheno
            .loot
            .iter()
            .any(|r| r.probability == 0.10 && r.threshold == 0.10)
    );
    let hp = game.properties[&0x917].hp as u64;
    let eligible_damage = (hp * 15).div_ceil(100);
    let mut at = Constant {
        unit: 0.001,
        calls: 0,
    };
    let fail = evaluate_draws(
        game,
        context(0x917),
        &[credit(11, eligible_damage)],
        5,
        limits(512),
        &mut at,
    )
    .unwrap();
    assert!(!fail.allocations().iter().any(|a| a.row() == bulwark));
    let mut below = Constant {
        unit: 0.000999,
        calls: 0,
    };
    let success = evaluate_draws(
        game,
        context(0x917),
        &[credit(11, eligible_damage)],
        5,
        limits(512),
        &mut below,
    )
    .unwrap();
    assert!(
        success
            .allocations()
            .iter()
            .any(|a| a.row() == bulwark && a.item() == 0xc04 && a.owner() == Owner::Character(11))
    );
    let thessal = &game.definitions[&0x1706];
    let mana = thessal.loot.iter().position(|r| r.pool == [0xaea]).unwrap();
    assert_eq!(
        (thessal.loot[mana].probability, thessal.loot[mana].min),
        (0.18, 2)
    );
    let mut draw = Constant {
        unit: 0.99,
        calls: 0,
    };
    let plan = evaluate_draws(
        game,
        context(0x1706),
        &[credit(11, game.properties[&0x1706].hp as u64)],
        5,
        limits(512),
        &mut draw,
    )
    .unwrap();
    assert_eq!(
        plan.allocations()
            .iter()
            .filter(|a| a.row() == mana && a.owner() == Owner::Character(11))
            .count(),
        2
    );
    let snapshot = crate::loot::catalogue::snapshot(game, Some("unit-only")).unwrap();
    for (kind, prob, target) in [(0xe55, 0.7, 0x742), (0x657, 0.02, 0x890)] {
        let facts = snapshot["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == kind)
            .unwrap();
        assert!(facts["death_births"].as_array().unwrap().iter().any(
            |v| v["compiled_behavior"]["fields"]["Prob"]["Number"] == prob
                && v["target"]["id"] == target
                && v["target"]["class"] == "Portal"
        ));
        assert!(
            game.definitions[&(kind as u16)]
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
                .any(
                    |b| matches!(b.fields.get(&Field::Prob),Some(GameValue::Number(p)) if *p==prob)
                )
        );
    }
    let out = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap());
    std::fs::write(
        out.join("xml-rewards-drop-rates-v2.json"),
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("xml-rewards-actual-plan.json"),
        serde_json::to_vec_pretty(&plan).unwrap(),
    )
    .unwrap();
}

#[test]
fn xml_rewards_actual_xml_only_bulwark_mutation_same_evaluator_binary_and_compiler_refusal() {
    let original = &*GAME;
    let mut inputs: BTreeMap<_, _> = original
        .inputs
        .keys()
        .map(|path| {
            (
                path.clone(),
                std::fs::read(Path::new("data").join(path)).unwrap(),
            )
        })
        .collect();
    let key = "game/123.5.1/actors.xml";
    let text = String::from_utf8(inputs[key].clone()).unwrap();
    let from = "<SoulboundLoot prob=\".001\" threshold=\".15\">Wand of the Bulwark</SoulboundLoot>";
    assert_eq!(text.matches(from).count(), 2);
    inputs.insert(
        key.into(),
        text.replacen(from, &from.replace(".001", ".002"), 1)
            .into_bytes(),
    );
    let changed = game::compile_fixture_at(&CONTENT, inputs.clone(), Path::new("data")).unwrap();
    let ledger = [credit(11, original.properties[&0x917].hp as u64)];
    let mut old_draw = Constant {
        unit: 0.0015,
        calls: 0,
    };
    let mut new_draw = Constant {
        unit: 0.0015,
        calls: 0,
    };
    let before = evaluate_draws(
        original,
        context(0x917),
        &ledger,
        99,
        limits(512),
        &mut old_draw,
    )
    .unwrap();
    let after = evaluate_draws(
        &changed,
        context(0x917),
        &ledger,
        99,
        limits(512),
        &mut new_draw,
    )
    .unwrap();
    assert!(!before.allocations().iter().any(|a| a.item() == 0xc04));
    assert!(after.allocations().iter().any(|a| a.item() == 0xc04));
    assert_ne!(before.content_fingerprint(), after.content_fingerprint());
    assert_ne!(before.rules_fingerprint(), after.rules_fingerprint());
    let find = |facts: Value| {
        facts["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["source_row"]["pool"] == json!([0xc04]))
            .unwrap()["bernoulli_attempt_probability"]
            .as_f64()
            .unwrap()
    };
    assert_eq!(find(definition_facts(original, 0x917).unwrap()), 0.001);
    assert_eq!(find(definition_facts(&changed, 0x917).unwrap()), 0.002);
    inputs.insert(
        key.into(),
        text.replacen(from, &from.replace(".001", "NaN"), 1)
            .into_bytes(),
    );
    assert!(game::compile_fixture_at(&CONTENT, inputs, Path::new("data")).is_err());
}

#[test]
fn xml_rewards_zero_probability_minimum_permutation_and_failed_public_gate_spend_only_declared_draws()
 {
    let game = fixture(&row("prob=\"0\" min=\"1\""));
    let plan = sample(
        &game,
        &[credit(11, 1), credit(12, 1)],
        vec![Draw::Index(2, 0)],
    );
    assert_eq!(owners(&plan), [Owner::Character(12)]);
    let giant = fixture(&row("min=\"4294967295\""));
    assert!(sample(&giant, &[], vec![]).allocations().is_empty());
    let public = fixture("<Loot threshold=\".15\" min=\"1\">Loot One</Loot>");
    let mut dead = credit(11, 1000);
    dead.alive = false;
    let mut departed = credit(12, 1000);
    departed.departing = true;
    let mut foreign = credit(13, 1000);
    foreign.world = 8;
    assert!(
        sample(&public, &[dead, departed, foreign], vec![])
            .allocations()
            .is_empty()
    );
}

#[test]
fn xml_rewards_invalid_declared_identity_hp_and_scripted_draw_domains_are_contextual_errors() {
    let base = fixture(&row("prob=\".5\""));
    let mut missing = context(0xffff);
    let mut script = Script::new(vec![]);
    assert!(evaluate_draws(&base, missing, &[credit(11, 1)], 4, limits(8), &mut script).is_err());
    missing = context(ACTOR);
    missing.actor = 0;
    assert!(evaluate_draws(&base, missing, &[credit(11, 1)], 4, limits(8), &mut script).is_err());
    for hp in [0, -1] {
        let mut game = base.clone();
        game.properties.get_mut(&ACTOR).unwrap().hp = hp;
        assert!(
            evaluate_draws(
                &game,
                context(ACTOR),
                &[credit(11, 1)],
                4,
                limits(8),
                &mut script
            )
            .is_err()
        );
    }
    script.done();
    for value in [f64::NAN, -0.01, 1.0] {
        let mut script = Script::new(vec![Draw::Unit(value)]);
        let error = format!(
            "{:#}",
            evaluate_draws(
                &base,
                context(ACTOR),
                &[credit(11, 1)],
                4,
                limits(8),
                &mut script
            )
            .unwrap_err()
        );
        assert!(
            error.contains("unit draw outside") && error.contains("row=0"),
            "{error}"
        );
        script.done();
    }
    let mut script = Script::new(vec![Draw::Index(2, 2)]);
    assert!(
        format!(
            "{:#}",
            evaluate_draws(
                &base,
                context(ACTOR),
                &[credit(11, 1), credit(12, 1)],
                4,
                limits(8),
                &mut script
            )
            .unwrap_err()
        )
        .contains("index 2 outside")
    );
    script.done();
}

#[test]
fn xml_rewards_restore_rejects_duplicate_owner_without_cycling() {
    let ledger = [credit(11, 1), credit(12, 1)];
    let mut rejected = Vec::new();
    for attrs in ["prob=\"0\" min=\"2\" max=\"2\"", ""] {
        let game = fixture(&row(attrs));
        let plan = evaluate(&game, context(ACTOR), &ledger, 19, limits(8)).unwrap();
        plan.validate(&game, context(ACTOR), &ledger, limits(8))
            .unwrap();
        assert_eq!(plan.allocations().len(), 2);
        let mut value = serde_json::to_value(&plan).unwrap();
        for allocation in value["allocations"].as_array_mut().unwrap() {
            allocation["owner"] = json!({"Character": 11});
        }
        let restored: AllocationPlan = serde_json::from_value(value).unwrap();
        rejected.push(
            restored
                .validate(&game, context(ACTOR), &ledger, limits(8))
                .is_err(),
        );
    }
    assert_eq!(
        rejected,
        [true, true],
        "min=N and min<N duplicate-owner mutations"
    );
}

#[test]
fn xml_rewards_restore_rejects_missing_probability_one_allocations() {
    let ledger = [credit(11, 1), credit(12, 1), credit(13, 1)];
    let mut rejected = Vec::new();
    for (body, contributors, expected) in [
        (row(""), &ledger[..2], 2),
        ("<Loot>Loot One</Loot>".into(), &ledger[..0], 1),
        (row("max=\"1\""), &ledger[..], 1),
    ] {
        let game = fixture(&body);
        let plan = evaluate(&game, context(ACTOR), contributors, 19, limits(8)).unwrap();
        plan.validate(&game, context(ACTOR), contributors, limits(8))
            .unwrap();
        assert_eq!(plan.allocations().len(), expected);
        let mut value = serde_json::to_value(&plan).unwrap();
        value["allocations"] = json!([]);
        let restored: AllocationPlan = serde_json::from_value(value).unwrap();
        rejected.push(
            restored
                .validate(&game, context(ACTOR), contributors, limits(8))
                .is_err(),
        );
    }
    assert_eq!(
        rejected,
        [true, true, true],
        "private, public and capped p1 omissions"
    );
}

#[test]
fn xml_rewards_restore_rejects_impossible_cycle_multiplicity_including_absent_owner() {
    let game = fixture(&row("prob=\"0\" min=\"5\""));
    let ledger = [credit(11, 1), credit(12, 1), credit(13, 1)];
    let plan = evaluate(&game, context(ACTOR), &ledger, 19, limits(8)).unwrap();
    plan.validate(&game, context(ACTOR), &ledger, limits(8))
        .unwrap();
    let mut multiplicities: Vec<_> = plan.demand().into_values().collect();
    multiplicities.sort();
    assert_eq!(multiplicities, [1, 2, 2]);
    let mut rejected = Vec::new();
    for owners in [[11, 11, 11, 12, 13], [11, 11, 11, 12, 12]] {
        let mut value = serde_json::to_value(&plan).unwrap();
        for (allocation, owner) in value["allocations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(owners)
        {
            allocation["owner"] = json!({"Character": owner});
        }
        let restored: AllocationPlan = serde_json::from_value(value).unwrap();
        rejected.push(
            restored
                .validate(&game, context(ACTOR), &ledger, limits(8))
                .is_err(),
        );
    }
    assert_eq!(
        rejected,
        [true, true],
        "3/1/1 and 3/2/0 are impossible for N3/min5"
    );
}

#[test]
fn xml_rewards_restore_preserves_genuine_row_scoped_and_gated_boundary_plans() {
    let ledger = [credit(11, 1), credit(12, 1), credit(13, 1)];
    let repeated = row("") + &row("");
    let cases = [
        (row("max=\"1\""), &ledger[..], 1),
        (
            "<Loot threshold=\".15\">Loot One</Loot>".into(),
            &ledger[..],
            0,
        ),
        (row("min=\"2\""), &ledger[..0], 0),
        (row("max=\"0\""), &ledger[..], 0),
        (row("min=\"2\""), &ledger[..1], 2),
        (repeated, &ledger[..1], 2),
    ];
    for (body, contributors, expected) in cases {
        let game = fixture(&body);
        let plan = evaluate(&game, context(ACTOR), contributors, 19, limits(8)).unwrap();
        assert_eq!(plan.allocations().len(), expected);
        let bytes = serde_json::to_vec(&plan).unwrap();
        let restored: AllocationPlan = serde_json::from_slice(&bytes).unwrap();
        restored
            .validate(&game, context(ACTOR), contributors, limits(8))
            .unwrap();
        assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    }
    for attrs in [
        "prob=\"0\" min=\"5\"",
        "prob=\".5\" min=\"5\"",
        "min=\"5\"",
        "min=\"6\"",
    ] {
        let game = fixture(&row(attrs));
        for seed in 0..4 {
            let plan = evaluate(&game, context(ACTOR), &ledger, seed, limits(8)).unwrap();
            let restored: AllocationPlan =
                serde_json::from_value(serde_json::to_value(&plan).unwrap()).unwrap();
            restored
                .validate(&game, context(ACTOR), &ledger, limits(8))
                .unwrap();
            assert_eq!(restored, plan);
        }
    }
}

/// Test-only draws consuming the retired Rust table's RNG words (gen_bool u64, SliceRandom u32 index, usize ceiling).
struct LegacyWords(StdRng, bool);
impl Draws for LegacyWords {
    fn unit(&mut self) -> Result<f64> {
        Ok(rand::RngCore::next_u64(&mut self.0) as f64 / 18_446_744_073_709_551_616.)
    }
    fn index(&mut self, upper: usize) -> Result<usize> {
        Ok(if std::mem::take(&mut self.1) {
            self.0.gen_range(0..upper)
        } else {
            self.0.gen_range(0..upper as u32) as usize
        })
    }
}
const CHEST_SEEDS: u64 = 4096;
type ChestRow = (u16, Option<(u32, u32)>, Vec<(u64, Vec<u16>)>);
fn chest_digests(rows: &[ChestRow], outputs: &[(u16, Vec<Vec<u16>>)]) -> (String, String) {
    let hash = |value: serde_json::Value| format!("{:x}", Sha256::digest(value.to_string()));
    (hash(json!(rows)), hash(json!(outputs)))
}
fn compiled_chest_digests(game: &GameData) -> (String, String) {
    let rows: Vec<_> = game
        .landmark_chests
        .values()
        .map(|chest| {
            (
                chest.landmark,
                chest.ceiling,
                chest
                    .rows
                    .iter()
                    .map(|row| (row.probability.to_bits(), row.pool.clone()))
                    .collect(),
            )
        })
        .collect();
    let outputs: Vec<_> = game
        .landmark_chests
        .values()
        .map(|chest| {
            (
                chest.landmark,
                (0..CHEST_SEEDS)
                    .map(|seed| {
                        landmark_chest_draws(
                            game,
                            chest.landmark,
                            &mut LegacyWords(StdRng::seed_from_u64(seed), chest.ceiling.is_some()),
                        )
                        .unwrap()
                    })
                    .collect(),
            )
        })
        .collect();
    chest_digests(&rows, &outputs)
}

#[test]
fn xml_rewards_landmark_chests_are_authored_site_rows_with_pinned_legacy_draw_outputs() {
    let game = &*GAME;
    assert_eq!(
        game.landmark_chests.keys().copied().collect::<Vec<_>>(),
        [0x668, 0x675, 0x678, 0x67d, 0x925, 0xdc2]
    );
    for chest in game.landmark_chests.values() {
        assert_eq!(
            (chest.container, &chest.provenance),
            (0x501, &game::Provenance::Authored)
        );
        assert_eq!(
            chest.ceiling,
            matches!(chest.landmark, 0x925 | 0xdc2).then_some((3, 7))
        );
        assert!(
            chest.origin.contains("https://")
                && chest.rows.iter().all(|r| r.source.path == "LandmarkChest")
        );
    }
    // Site contents never become every Treasure Chest's or the landmark's own death rows.
    assert!(game.definitions[&0x501].loot.is_empty());
    assert!(game.landmark_chests.keys().all(|k| {
        game.definitions[k]
            .loot
            .iter()
            .all(|r| r.source.path != "LandmarkChest")
    }));
    let facts = crate::loot::catalogue::snapshot(game, None).unwrap()["landmark_chests"].clone();
    assert_eq!(facts.as_array().unwrap().len(), 6);
    assert_eq!(facts[4]["success_ceiling"]["max"], 7);
    // Captured from the live retired chest_table/landmark_chest before deletion (see slice C report).
    assert_eq!(
        compiled_chest_digests(game),
        (PINNED_CHEST_ROWS.into(), PINNED_CHEST_OUTPUTS.into())
    );
    let first = landmark_chest(game, 0xdc2, 7).unwrap();
    assert_eq!(first, landmark_chest(game, 0xdc2, 7).unwrap());
    assert!(first.len() <= 7);
    assert!(landmark_chest(game, 0x600, 7).is_err());
}
const PINNED_CHEST_ROWS: &str = "4f17014dbfbf06d6d95314979999d60983a9ada921ed07ab0b97f417337c331b";
const PINNED_CHEST_OUTPUTS: &str =
    "d617a293f0b1ef80c9022735fefa91726495dbd7d445b5b87fa700a818d89f6e";

#[test]
fn xml_rewards_public_rows_refuse_selected_soulbound_items_with_context() {
    let (landmark, chest) = GAME.landmark_chests.iter().next().unwrap();
    let item = chest.rows[0].pool[0];
    landmark_chest(&GAME, *landmark, 0).unwrap();
    let mut game = GAME.clone();
    game.properties.get_mut(&item).unwrap().soulbound = true;
    let error = format!("{:#}", landmark_chest(&game, *landmark, 0).unwrap_err());
    assert!(
        error.contains(&format!(
            "public row contains selected soulbound item {item:#x}"
        )) && error.contains(&format!("landmark chest {landmark:#x}")),
        "{error}"
    );
}
