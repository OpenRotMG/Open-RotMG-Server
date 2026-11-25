use super::compile::compile;
use super::*;
use crate::assert_matches;
use serde_json::json;
use std::sync::LazyLock;

static CONTENT: LazyLock<Content> = LazyLock::new(|| Content::load(Path::new("data")).unwrap());
const MANIFEST: &str = "game/123.5.1/game.xml";
const RULES: &str = "game/123.5.1/rules.xml";
fn object(name: &str, id: u16, body: &str) -> String {
    format!(
        r#"<Object type="{id}" id="{name}" disposition="active" provenance="authored" reason="compiler regression" origin="synthetic test">{body}</Object>"#
    )
}
// Admission-negative fixtures use raw_inputs so these fixed dependencies cannot hide omissions.
const BASIC_FIXTURE_NATIVES: &[(u16, &str)] =
    &[(6, "projectile"), (0x120, "scenery"), (0x160, "scenery")];
const LOOT_FIXTURE_NATIVES: &[(u16, &str)] = &[
    (0xa6c, "inventory"),
    (0xa6f, "inventory"),
    (0xa09, "inventory"),
    (0xa15, "inventory"),
    (0xa75, "inventory"),
    (0xad4, "inventory"),
    (0xad7, "inventory"),
    (0xad9, "inventory"),
    (0xa57, "inventory"),
    (0xa62, "inventory"),
    (0xa67, "inventory"),
    (0xa98, "inventory"),
    (0xaa4, "inventory"),
    (0xaab, "inventory"),
    (0xab2, "inventory"),
    (0xa42, "inventory"),
    (0xb1c, "inventory"),
    (0xb2e, "inventory"),
    (0xb08, "inventory"),
    (0xa22, "inventory"),
    (4, "projectile"),
    (9, "projectile"),
    (10, "projectile"),
    (16, "projectile"),
    (27, "projectile"),
    (32, "projectile"),
    (0x711, "trap"),
    (0x715, "decoy"),
];
fn fixture_natives(declarations: &[(u16, &str)]) -> String {
    declarations
        .iter()
        .map(|(kind, service)| native_declaration(*kind, service))
        .collect()
}
fn fixture_rules(xml: &str) -> Vec<u8> {
    format!(
        "<Objects>{xml}{}</Objects>",
        fixture_natives(BASIC_FIXTURE_NATIVES)
    )
    .into_bytes()
}
fn inputs(xml: String) -> BTreeMap<String, Vec<u8>> {
    raw_inputs(xml + &fixture_natives(BASIC_FIXTURE_NATIVES))
}
fn raw_inputs(xml: String) -> BTreeMap<String, Vec<u8>> {
    crate::content::game::test_inputs::selected([
        (
            MANIFEST,
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/></Game>"#
                .to_vec(),
        ),
        (RULES, format!("<Objects>{xml}</Objects>").into_bytes()),
    ])
}
fn simple(body: &str) -> BTreeMap<String, Vec<u8>> {
    inputs(object("Pirate", 0x600, body))
}
fn error(bytes: BTreeMap<String, Vec<u8>>, expected: &str) {
    let err = format!("{:#}", compile(&CONTENT, bytes).unwrap_err());
    assert!(err.contains(expected), "expected {expected:?} in {err}");
}
fn load(inputs: BTreeMap<String, Vec<u8>>) -> GameData {
    compile(&CONTENT, inputs).unwrap()
}
fn pirate(body: &str) -> GameData {
    load(simple(body))
}
#[test]
fn xml_compile_selected_client_properties_and_documented_defaults() {
    let game = pirate(
        "<Behavior>Shoot</Behavior><Behavior>Decay</Behavior><Behavior>ChangeSize</Behavior>",
    );
    assert_eq!(
        serde_json::to_value(&game.properties).unwrap(),
        serde_json::to_value(&CONTENT.objects).unwrap()
    );
    let b = &game.definitions[&0x600].states[0].behaviors;
    assert_matches!(b[0].fields[&Field::Cooldown], Value::Millis(1000));
    assert_matches!(b[0].fields[&Field::ProjectileId], Value::Projectile(0));
    assert_matches!(b[0].fields[&Field::Type], Value::Mode(Mode::Targeted));
    assert_matches!(b[1].fields[&Field::Time], Value::Millis(60000));
    assert_matches!(b[1].fields[&Field::DropLoot], Value::Bool(false));
    assert_matches!(b[2].fields[&Field::Rate], Value::Number(n) if n == 0.2);
    assert!(game.reachable.contains(&0x600));
    assert!(!game.property_dependencies[&0x600].is_empty());
}
#[test]
fn xml_compile_unknown_fields_numbers_and_source_context() {
    for (body, expected) in [
        ("<Behavior>Imaginary</Behavior>", "unknown server behavior"),
        (
            "<Behavior coodown=\"1\">Shoot</Behavior>",
            "unknown server attribute coodown",
        ),
        (
            "<Behavior rate=\"1\">Shoot</Behavior>",
            "not valid for Shoot",
        ),
        ("<Behavior cooldown=\"NaN\">Shoot</Behavior>", "nonfinite"),
        ("<Behavior cooldown=\"-1\">Shoot</Behavior>", "negative"),
        (
            "<Behavior cooldown=\"1e99\">Shoot</Behavior>",
            "integral milliseconds",
        ),
        (
            "<Behavior cooldown=\"0.0001\">Shoot</Behavior>",
            "integral milliseconds",
        ),
        ("<Behavior numShots=\"1.5\">Shoot</Behavior>", "integer"),
        (
            "<Behavior cooldownJitter=\"yes\">Shoot</Behavior>",
            "boolean",
        ),
        (
            "<Behavior projectileId=\"17\">Shoot</Behavior>",
            "unresolved selected projectile",
        ),
        (
            "<Behavior>MakeMinions</Behavior>",
            "missing required ChildIds",
        ),
        (
            "<Behavior childIds=\"Pirate\">MakeMinions</Behavior>",
            "missing required MaxChildren",
        ),
        (
            "<Behavior childIds=\"Pirate\" maxChildren=\"0\">MakeMinions</Behavior>",
            "invalid bounded child population",
        ),
        (
            "<MaxHitPoints>99</MaxHitPoints>",
            "selected-client properties may not be overlaid",
        ),
        (
            "<Behavior extra=\"1\">Wander</Behavior>",
            "unknown server attribute",
        ),
        ("<Behavior type=\"Flash\">Shoot</Behavior>", "incompatible"),
        (
            "<State id=\"Bad\"><Behavior cooldown=\"invalid\">Shoot</Behavior></State>",
            "malformed number",
        ),
    ] {
        error(simple(body), expected);
    }
    let err = format!(
        "{:#}",
        compile(
            &CONTENT,
            simple("<State id=\"Bad\"><Behavior cooldown=\"invalid\">Shoot</Behavior></State>")
        )
        .unwrap_err()
    );
    for context in ["rules.xml:1", "object=Pirate", "state=@root/Bad"] {
        assert!(err.contains(context), "{err}");
    }
}
#[test]
fn xml_compile_nested_embeds_recovered_four_quarters_keep_transitive_parameters() {
    let original =
        std::fs::read_to_string("data/reference/release-7.0/xml/encounters.xml").unwrap();
    let original = roxmltree::Document::parse(&original).unwrap();
    let source = original
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("id") == Some("Lord of the Lost Lands"))
        .unwrap();
    let runtime = std::fs::read_to_string("data/game/123.5.1/actors.xml").unwrap();
    let doc = roxmltree::Document::parse(&runtime).unwrap();
    let actor = doc
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("id") == source.attribute("id"))
        .unwrap();
    assert!(
        actor
            .attribute("reason")
            .unwrap()
            .contains("Shoot predictive=true normalized")
    );
    let mut body = actor
        .children()
        .filter(|n| n.has_tag_name("DefineEmbed"))
        .map(|n| &runtime[n.range()])
        .collect::<String>();
    assert_eq!(
        actor
            .children()
            .filter(|n| n.has_tag_name("DefineEmbed"))
            .count(),
        5
    );
    // Isolate the real quarter calls; production tests cover surrounding states and full child rules.
    fn call<'a, 'input>(o: roxmltree::Node<'a, 'input>, q: &str) -> roxmltree::Node<'a, 'input> {
        o.children()
            .find(|n| n.has_tag_name("State") && n.attribute("id") == Some(q))
            .unwrap()
            .children()
            .find(|n| n.has_tag_name("Embed"))
            .unwrap()
    }
    for q in ["Q1", "Q2", "Q3", "Q4"] {
        let supplied = |n: roxmltree::Node<'_, '_>| {
            n.children()
                .filter(|n| n.has_tag_name("Param"))
                .map(|n| {
                    (
                        n.attribute("name").unwrap().to_owned(),
                        n.text().unwrap().to_owned(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(supplied(call(source, q)), supplied(call(actor, q)));
        assert_eq!(supplied(call(actor, q)).len(), 7);
        body += &format!(
            "<State id=\"{q}\">{}</State>",
            &runtime[call(actor, q).range()]
        );
    }
    let guardian = CONTENT
        .objects
        .values()
        .find(|o| o.name == "Guardian of the Lost Lands")
        .unwrap()
        .kind;
    let dependency = object(
        "Guardian of the Lost Lands",
        guardian,
        "<Behavior>Wander</Behavior>",
    );
    let load = |body: &str| {
        production_syntax_load(emission_inputs(
            "Lord of the Lost Lands",
            0xd50,
            body,
            &dependency,
        ))
        .unwrap()
    };
    let game = load(&body);
    same_selected_properties(&game);
    let def = &game.definitions[&0xd50];
    let child = |parent, name: &str| {
        def.states
            .iter()
            .find(|s| s.parent == Some(parent) && s.name == name)
            .unwrap()
    };
    let after = |state: &State| {
        state
            .transitions
            .iter()
            .flat_map(|t| &t.predicates)
            .filter_map(|p| {
                if let Predicate::After(ms) = p {
                    Some(*ms)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    for (q, gap, length, shots, charge, attack, rest, guardians) in [
        ("Q1", 1200, 12000, 7, 2000, 6000, 3000, 5000),
        ("Q2", 800, 9000, 8, 1500, 7500, 2000, 6500),
        ("Q3", 600, 7000, 9, 800, 9000, 1000, 7500),
        ("Q4", 400, 5000, 10, 330, 9000, 330, 8000),
    ] {
        let q = child(def.root, q);
        assert_eq!(
            q.children
                .iter()
                .map(|i| def.states[*i as usize].name.as_str())
                .collect::<Vec<_>>(),
            [
                "Attack1",
                "PrepareForAttack2",
                "Attack2",
                "Rest",
                "Create Guardians"
            ]
        );
        let first = child(q.id, "Attack1");
        assert_eq!(after(first), [length]);
        assert_eq!(first.children.len(), 8);
        for part in &first.children {
            assert_eq!(after(&def.states[*part as usize]), [gap]);
        }
        let b = &child(first.id, "Part1").behaviors;
        assert_eq!(b.len(), 2);
        assert!(b.iter().all(|b| b.source.embeds.len() == 2
            && b.source.embeds[0].starts_with("quarter@")
            && b.source.embeds[1].starts_with("AttackOne@")));
        assert_eq!(after(child(q.id, "Attack2")), [attack]);
        assert_eq!(after(child(child(q.id, "Attack2").id, "Part0")), [charge]);
        assert_eq!(after(child(q.id, "Rest")), [rest]);
        assert_eq!(after(child(q.id, "Create Guardians")), [guardians]);
        assert_matches!(child(q.id,"PrepareForAttack2").behaviors[0].fields[&Field::NumShots], Value::Integer(n) if n==shots);
    }
    let locals = def
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .map(|b| b.local)
        .collect::<Vec<_>>();
    assert_eq!(
        locals.iter().copied().collect::<BTreeSet<_>>().len(),
        locals.len()
    );
    assert!(game.reachable.contains(&guardian));
    let changed = load(&body.replacen(
        "<Param name=\"timeBetweenShots\">1.2</Param>",
        "<Param name=\"timeBetweenShots\">1.3</Param>",
        1,
    ));
    assert_ne!(game.fingerprint, changed.fingerprint);
    assert_eq!(def.states.len(), changed.definitions[&0xd50].states.len());
    for (old, new) in def.states.iter().zip(&changed.definitions[&0xd50].states) {
        assert_eq!(
            old.behaviors.iter().map(|b| b.local).collect::<Vec<_>>(),
            new.behaviors.iter().map(|b| b.local).collect::<Vec<_>>()
        );
        if old.source.path.starts_with("@root/Q1/Attack1/") {
            assert_eq!(after(new), [1300]);
        } else {
            assert_eq!(after(old), after(new));
        }
    }
}
#[test]
fn xml_compile_nested_embeds_forward_bindings_without_caller_scope_capture() {
    for (body, speed) in [
        (
            r#"<DefineEmbed name="Leaf"><Behavior speed="{inner}">Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"><Param name="inner">{outer}</Param></Embed></DefineEmbed><Embed name="Outer"><Param name="outer">1.25</Param></Embed>"#,
            1.25,
        ),
        (
            r#"<DefineEmbed name="Leaf"><Behavior speed="{speed}">Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"><Param name="speed">{speed}</Param></Embed></DefineEmbed><Embed name="Outer"><Param name="speed">2</Param></Embed>"#,
            2.,
        ),
        (
            r#"<DefineEmbed name="Leaf"><Behavior speed="{speed}">Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"/></DefineEmbed><State id="Caller"><DefineEmbed name="Leaf"><Behavior speed="9">Wander</Behavior></DefineEmbed><Embed name="Outer"><Param name="speed">3</Param></Embed></State>"#,
            3.,
        ),
    ] {
        let game = production_syntax_load(simple(body)).unwrap();
        let b = game.definitions[&0x600]
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .collect::<Vec<_>>();
        assert_eq!(b.len(), 1);
        assert_matches!(b[0].fields[&Field::Speed], Value::Number(n) if n==speed);
    }
}
#[test]
fn xml_compile_nested_embeds_refuse_shadowed_dead_unused_and_invalid_dependencies() {
    for (body, expected) in [
        (
            r#"<DefineEmbed name="Leaf"><Behavior speed="{speed}">Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"><Param name="speed">2</Param></Embed></DefineEmbed><Embed name="Outer"><Param name="speed">1</Param></Embed>"#,
            "unused embed parameter speed",
        ),
        (
            r#"<DefineEmbed name="Leaf"><Behavior>Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><DefineEmbed name="Dead"><Behavior speed="{unused}">Wander</Behavior></DefineEmbed><Embed name="Leaf"/></DefineEmbed><Embed name="Outer"><Param name="unused">1</Param></Embed>"#,
            "unused embed parameter unused",
        ),
        (
            r#"<DefineEmbed name="Leaf"><Behavior>Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"/></DefineEmbed><State id="Caller"><DefineEmbed name="Leaf"><Behavior speed="{unused}">Wander</Behavior></DefineEmbed><Embed name="Outer"><Param name="unused">1</Param></Embed></State>"#,
            "unused embed parameter unused",
        ),
        (
            r#"<DefineEmbed name="Leaf"><Behavior speed="{missing}">Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"/></DefineEmbed><Embed name="Outer"/>"#,
            "unbound parameter",
        ),
        (
            r#"<DefineEmbed name="Outer"><Embed name="Missing"/></DefineEmbed><Embed name="Outer"/>"#,
            "unresolved embed",
        ),
        (
            r#"<DefineEmbed name="A"><Embed name="B"/></DefineEmbed><DefineEmbed name="B"><Embed name="A"/></DefineEmbed><Embed name="A"><Param name="unused">1</Param></Embed>"#,
            "recursive embed",
        ),
        (
            r#"<DefineEmbed name="Dead"><Embed name="Missing"/></DefineEmbed><Behavior>Wander</Behavior>"#,
            "unresolved embed",
        ),
        (
            r#"<DefineEmbed name="Dead"><Embed name="Dead"/></DefineEmbed><Behavior>Wander</Behavior>"#,
            "recursive embed",
        ),
        (
            r#"<DefineEmbed name="Dead"><Behavior typo="1">Wander</Behavior></DefineEmbed><Behavior>Wander</Behavior>"#,
            "unknown server attribute",
        ),
        (
            r#"<DefineEmbed name="Leaf"><Behavior speed="{inner}">Wander</Behavior></DefineEmbed><Embed name="Leaf"><Param name="inner">{missing}</Param></Embed>"#,
            "unbound parameter",
        ),
    ] {
        production_syntax_error(simple(body), expected);
    }
    let mut body = String::new();
    for i in 0..66 {
        body += &format!(
            "<DefineEmbed name=\"D{i}\"><Embed name=\"D{}\"/></DefineEmbed>",
            i + 1
        );
    }
    body +=
        "<DefineEmbed name=\"D66\"><Behavior>Wander</Behavior></DefineEmbed><Embed name=\"D0\"/>";
    let err = format!("{:#}", production_syntax_load(simple(&body)).unwrap_err());
    assert!(
        err.contains("rules.xml")
            && (err.contains("depth exceeds 64") || err.contains("recursive embed dependency")),
        "{err}"
    );
}
#[test]
fn xml_compile_template_negatives_and_context() {
    for (body, expected) in [
        ("<Embed name=\"Missing\"/>", "unresolved embed"),
        (
            "<DefineEmbed name=\"A\"><Embed name=\"A\"/></DefineEmbed><Embed name=\"A\"/>",
            "recursive embed",
        ),
        (
            "<DefineEmbed name=\"A\"><Embed name=\"B\"/></DefineEmbed><DefineEmbed name=\"B\"><Embed name=\"A\"/></DefineEmbed><Embed name=\"A\"/>",
            "recursive embed",
        ),
        (
            "<DefineEmbed name=\"A\"><Behavior cooldown=\"{delay}\">Shoot</Behavior></DefineEmbed><Embed name=\"A\"/>",
            "unbound parameter",
        ),
        (
            "<DefineEmbed name=\"A\"><Behavior>Wander</Behavior></DefineEmbed><Embed name=\"A\"><Param name=\"typo\">1</Param></Embed>",
            "unused embed parameter",
        ),
        (
            "<DefineEmbed name=\"A\"/><DefineEmbed name=\"A\"/><Behavior>Wander</Behavior>",
            "duplicate embed",
        ),
        (
            "<DefineEmbed name=\"Unused\"><Behavior mystery=\"1\">Shoot</Behavior></DefineEmbed><Behavior>Wander</Behavior>",
            "unknown server attribute",
        ),
    ] {
        error(simple(body), expected);
    }
    let err = format!("{:#}",compile(&CONTENT, simple("<DefineEmbed name=\"Pulse\"><Behavior cooldown=\"bad\">Shoot</Behavior></DefineEmbed><State id=\"Wait\"><Embed name=\"Pulse\"/></State>")).unwrap_err());
    assert!(err.contains("Pulse@"));
    assert!(err.contains("state=@root/Wait"));
    error(
        simple(&format!(
            "{}<Behavior>Wander</Behavior>{}",
            "<State id=\"deep\">".repeat(130),
            "</State>".repeat(130)
        )),
        "XML nesting exceeds",
    );
}
#[test]
fn xml_compile_embeds_use_definition_lexical_scope() {
    let body = r#"<DefineEmbed name="Leaf"><Behavior speed="1">Wander</Behavior></DefineEmbed>
        <DefineEmbed name="Outer"><Embed name="Leaf"/></DefineEmbed>
        <State id="Child"><DefineEmbed name="Leaf"><Behavior speed="2">Wander</Behavior></DefineEmbed><Embed name="Outer"/><Embed name="Leaf"/></State>"#;
    let game = pirate(body);
    let b = &game.definitions[&0x600].states[1].behaviors;
    assert_matches!(b[0].fields[&Field::Speed], Value::Number(n) if n == 1.);
    assert_matches!(b[1].fields[&Field::Speed], Value::Number(n) if n == 2.);
}
#[test]
fn xml_compile_sibling_resolution_and_no_uncle_fallback() {
    let body = r#"<State id="A"><State id="Wait"><Behavior>Wander</Behavior><Transition afterTime="0">Done</Transition></State><State id="Done"/></State>
        <State id="B"><State id="Wait"><Behavior>Wander</Behavior><Transition afterTime="0">Done</Transition></State><State id="Done"/></State>"#;
    let game = pirate(body);
    let def = &game.definitions[&0x600];
    let waits: Vec<_> = def.states.iter().filter(|s| s.name == "Wait").collect();
    assert_ne!(
        waits[0].transitions[0].targets,
        waits[1].transitions[0].targets
    );
    error(
        simple("<State id=\"A\"><Behavior>Wander</Behavior></State><State id=\"A\"/>"),
        "duplicate sibling state",
    );
    error(
        simple(
            "<State id=\"Done\"/><State id=\"A\"><State id=\"Inner\"><Behavior>Wander</Behavior><Transition afterTime=\"1\">Done</Transition></State></State>",
        ),
        "unresolved sibling state",
    );
    error(
        simple("<Behavior>Wander</Behavior><Transition afterTime=\"1\">Root</Transition>"),
        "no sibling scope",
    );
}
#[test]
fn xml_compile_exact_names_groups_and_reachable_actor_closure() {
    let pirate = object(
        "Pirate",
        0x600,
        "<Behavior childIds=\"Piratess\" maxChildren=\"2\">MakeMinions</Behavior>",
    );
    error(
        inputs(pirate.clone()),
        "Piratess has no explicit admission definition/disposition",
    );
    let follower = object(
        "Piratess",
        0x601,
        "<Group>Contract Group</Group><Behavior>Wander</Behavior>",
    );
    let game = load(inputs(pirate + &follower));
    assert!(game.reachable.contains(&0x601));
    let group = object(
        "Pirate",
        0x600,
        "<Behavior targetIds=\"Contract Group\">Heal</Behavior>",
    );
    let game = load(inputs(group + &follower));
    assert!(game.reachable.contains(&0x601));
    error(
        simple("<Behavior childIds=\"Piratesss\">MakeMinions</Behavior>"),
        "unresolved selected object",
    );
    error(
        simple("<Behavior targetIds=\"Unknown Group\">Heal</Behavior>"),
        "unresolved object/group",
    );
}
#[test]
fn xml_compile_duplicates_identity_and_disposition_negatives() {
    let pirate = object("Pirate", 0x600, "<Behavior>Wander</Behavior>");
    error(
        inputs(pirate.clone() + &pirate),
        "duplicate selected definition",
    );
    error(
        inputs(pirate.replace("id=\"Pirate\"", "id=\"Pirate Enemy\"")),
        "selected identity mismatch",
    );
    error(
        inputs(
            object("Pirate", 0x600, "")
                .replace("disposition=\"active\"", "disposition=\"fallback\""),
        ),
        "unknown disposition",
    );
    error(
        inputs(object("Pirate", 0x600, "").replace(
            "provenance=\"authored\"",
            "provenance=\"verified-historical\"",
        )),
        "unknown provenance",
    );
    error(simple(""), "active definition has no executable rules");
    error(
        simple("<State id=\"Empty\"/>"),
        "active definition has no effects",
    );
    for (disposition, name, id) in [
        ("passive", "Wine Barrel", 0xde5),
        ("native", "Realm Portal", 0x704),
    ] {
        let metadata = if disposition == "native" {
            "disposition=\"native\" nativeService=\"portal\""
        } else {
            "disposition=\"passive\""
        };
        let make = |body| {
            inputs(
                object("Pirate", 0x600, "<Behavior>Wander</Behavior>")
                    + &object(name, id, body).replace("disposition=\"active\"", metadata),
            )
        };
        assert!(compile(&CONTENT, make("")).is_ok());
        error(
            make("<Behavior>Wander</Behavior>"),
            if disposition == "native" {
                "native definition cannot contain server payload"
            } else {
                "contains executable rules"
            },
        );
        error(
            inputs(object("Pirate", 0x600, "").replace("disposition=\"active\"", metadata)),
            if disposition == "passive" {
                "passive disposition requires"
            } else {
                "nativeService portal incompatible with selected"
            },
        );
    }
}
#[test]
fn xml_compile_loot_rows_defaults_tier_pools_and_negatives() {
    let game = load(inputs(
        object(
            "Pirate",
            0x600,
            r#"<Behavior>Wander</Behavior>
        <SoulboundLoot prob=".001" threshold=".15">Staff of the Cosmic Whole</SoulboundLoot>
        <SoulboundLoot tier="1" min="2" max="3">Weapon</SoulboundLoot>
        <SoulboundLoot tier="1" min="2" max="3">Weapon</SoulboundLoot>
        <Loot>Health Potion</Loot>"#,
        ) + &fixture_natives(LOOT_FIXTURE_NATIVES),
    ));
    let rows = &game.definitions[&0x600].loot;
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].probability, 0.001);
    assert_eq!(rows[0].threshold, 0.15);
    assert_eq!(rows[1].min, 2);
    assert_eq!(rows[1].max, Some(3));
    assert_eq!(rows[1].pool, rows[2].pool);
    assert!(rows[1].pool.windows(2).all(|w| w[0] < w[1]));
    assert!(
        rows[1]
            .pool
            .iter()
            .all(|id| CONTENT.objects[id].tier == Some(1))
    );
    assert_eq!(rows[3].probability, 1.);
    assert_eq!(rows[3].threshold, 0.);
    assert_matches!(rows[3].ownership, LootOwnership::Public);
    for (row, expected) in [
        (
            "<Loot prob=\"1.1\">Health Potion</Loot>",
            "probability outside",
        ),
        ("<Loot threshold=\"NaN\">Health Potion</Loot>", "nonfinite"),
        (
            "<Loot min=\"2\" max=\"1\">Health Potion</Loot>",
            "min exceeds max",
        ),
        ("<Loot min=\"1.5\">Health Potion</Loot>", "integer"),
        (
            "<Loot tier=\"250\">Weapon</Loot>",
            "empty selected tier pool",
        ),
        (
            "<Loot>Imaginary Potion</Loot>",
            "unresolved selected object",
        ),
        (
            "<Loot tier=\"1\">Imaginary Pool</Loot>",
            "unknown tier pool",
        ),
    ] {
        error(
            simple(&format!("<Behavior>Wander</Behavior>{row}")),
            expected,
        );
    }
}
const GEOMETRY: &str = "game/123.5.1/geometry.xml";
const QUALIFIED: &str = r#"provenance="authored" origin="synthetic compiler fixture" reason="typed geometry contract test""#;
fn template_xml(id: &str) -> String {
    format!(
        r#"<Template id="{id}" {QUALIFIED} width="2" height="2" rotation="fixed" rooms="1"><CharMap char="." ground="Grass"/><Pattern><Row value=".."/><Row value=".."/></Pattern><Spawn x="0.5" y="0.5"/></Template>"#
    )
}
fn geometry_inputs(resources: &str) -> BTreeMap<String, Vec<u8>> {
    let mut bytes = simple("<Behavior map=\"Contract Map\">PlaceMap</Behavior>");
    bytes.insert(MANIFEST.into(),br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/><Geometry file="geometry.xml"/><World id="contract" map="Contract Map"><Population object="Pirate" min="1" max="2"/></World></Game>"#.to_vec());
    bytes.insert(
        GEOMETRY.into(),
        format!("<Geometry schema=\"1\">{resources}</Geometry>").into_bytes(),
    );
    bytes
}
fn map_inputs() -> BTreeMap<String, Vec<u8>> {
    geometry_inputs(&template_xml("Contract Map"))
}
#[test]
fn xml_compile_worlds_maps_and_asset_closure() {
    use super::geometry::Geometry;
    use std::io::Write;
    let mut wmap = Vec::new();
    let ground = CONTENT
        .ground_names
        .iter()
        .find(|(_, n)| n.as_str() == "Grass")
        .unwrap()
        .0;
    wmap.extend(1_i16.to_le_bytes());
    wmap.extend(i16::from(*ground).to_le_bytes());
    wmap.extend([0, 0, 0, 1]);
    wmap.extend(1_i32.to_le_bytes());
    wmap.extend(1_i32.to_le_bytes());
    wmap.extend(0_i16.to_le_bytes());
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&wmap).unwrap();
    let encoded = encoder.finish().unwrap();
    let hash = super::compile::hash(&encoded);
    let mut bytes = geometry_inputs(&format!(
        r#"<Map name="Contract Map" {QUALIFIED} file="fixture.wmap" sha256="{hash}" format="wmap" width="1" height="1"/>"#
    ));
    bytes.insert("fixture.wmap".into(), encoded);
    let game = load(bytes.clone());
    let Geometry::Encoded(asset) = &game.maps[0].geometry else {
        panic!("expected encoded map")
    };
    assert_eq!(asset.decode(&CONTENT).unwrap().spawns, vec![(0.5, 0.5)]);
    assert_eq!(asset.dimensions, [1, 1]);
    bytes.insert("fixture.wmap".into(), b"invalid map".to_vec());
    error(bytes, "map input SHA-256 mismatch for fixture.wmap");
    let game = load(map_inputs());
    let Geometry::Template(t) = &game.maps[0].geometry else {
        panic!("expected template")
    };
    assert_eq!(t.cells.len(), 4);
    assert_eq!(t.spawns, [[0.5, 0.5]]);
    assert_eq!(game.worlds[0].populations[0].max, 2);
    assert_matches!(
        game.definitions[&0x600].states[0].behaviors[0].fields[&Field::Map],
        Value::Map(0)
    );
    error(
        simple("<Behavior map=\"Absent\">PlaceMap</Behavior>"),
        "unresolved map",
    );
    let mut bytes = map_inputs();
    bytes.remove(GEOMETRY);
    error(bytes, "missing required input game/123.5.1/geometry.xml");
    for (from, to, expected) in [
        (
            "ground=\"Grass\"",
            "ground=\"Missing\"",
            "unresolved/ambiguous ground",
        ),
        ("value=\"..\"", "value=\".\"", "Pattern width mismatch"),
        ("value=\"..\"", "value=\".?\"", "unbound Pattern symbol"),
        (
            "ground=\"Grass\"",
            "ground=\"Grass\" object=\"Piratess\"",
            "Piratess has no explicit admission definition/disposition",
        ),
        ("x=\"0.5\"", "x=\"8\"", "template spawn outside bounds"),
    ] {
        let xml = template_xml("Contract Map").replace(from, to);
        error(geometry_inputs(&xml), expected);
    }
    let mut bytes = map_inputs();
    let m = String::from_utf8(bytes[MANIFEST].clone())
        .unwrap()
        .replace("max=\"2\"", "max=\"0\"");
    bytes.insert(MANIFEST.into(), m.into_bytes());
    error(bytes, "invalid bounded world population");
}
#[test]
fn xml_compile_clear_key_requires_an_intent_and_its_own_world() {
    let (world, field, key) = ("snake-pit", "dungeons[0]", ProgressKey::SnakePit);
    let mut bytes = map_inputs();
    let m = String::from_utf8(bytes[MANIFEST].clone()).unwrap().replace(
        "id=\"contract\"",
        &format!("id=\"{world}\" clearKey=\"{field}\""),
    );
    bytes.insert(MANIFEST.into(), m.as_bytes().to_vec());
    error(bytes.clone(), "clearKey requires an explicit Clear intent");
    bytes.insert(
        MANIFEST.into(),
        m.replace(
            "</World>",
            &format!(r#"<Clear boss="Pirate" route="none" {QUALIFIED} /></World>"#),
        )
        .into_bytes(),
    );
    let game = load(bytes);
    assert_eq!(game.worlds[0].clear_key, Some(key));
    assert_eq!(
        game.worlds[0].clear.as_ref().unwrap().route,
        ClearRoute::None
    );
    let mut bytes = map_inputs();
    let m = String::from_utf8(bytes[MANIFEST].clone())
        .unwrap()
        .replace("id=\"contract\"", "id=\"snake-pit\" clearKey=\"tombs\"");
    bytes.insert(MANIFEST.into(), m.into_bytes());
    error(bytes, "stable clear key belongs");
}
#[test]
fn xml_compile_string_lists_tags_buckets_and_text_tokens_are_linked() {
    let mut bytes = simple(
        r#"<Behavior tagName="awake">SetTag</Behavior><Behavior bucket="movement">Wander</Behavior><Behavior listId="Contract Taunts">Taunt</Behavior><Behavior text="Hello {PLAYER}!">Say</Behavior><State id="A"><Transition hasTag="awake">B</Transition></State><State id="B"/>"#,
    );
    bytes.insert(MANIFEST.into(),br#"<Game schema="1"><Include file="rules.xml"/><Include file="strings.xml"/><Root object="Pirate"/></Game>"#.to_vec());
    bytes.insert("game/123.5.1/strings.xml".into(),br#"<StringLists><StringList id="Contract Taunts"><Strings id="everySoOften"><String>I have {HITPOINTS} HP, {PLAYER}!</String></Strings></StringList></StringLists>"#.to_vec());
    let game = load(bytes.clone());
    assert_eq!(game.symbols, ["awake", "movement"]);
    let b = &game.definitions[&0x600].states[0].behaviors;
    assert_matches!(b[0].fields[&Field::TagName], Value::Symbol(0));
    assert_eq!(b[1].bucket, Some(1));
    assert_matches!(b[2].fields[&Field::ListId], Value::StringList(0));
    assert_matches!(b[3].fields[&Field::Text],Value::TextChoices(ref choices) if matches!(choices[0][1],TextPart::Player));
    error(
        simple("<Behavior listId=\"Missing\">Taunt</Behavior>"),
        "unresolved StringList",
    );
    error(
        simple("<Behavior text=\"{UNKNOWN}\">Say</Behavior>"),
        "unknown text token",
    );
    let m = String::from_utf8(bytes["game/123.5.1/strings.xml"].clone())
        .unwrap()
        .replace("{HITPOINTS}", "{UNSUPPORTED}");
    bytes.insert("game/123.5.1/strings.xml".into(), m.into_bytes());
    error(bytes, "unknown text token");
}
#[test]
fn xml_compile_fingerprints_depend_on_xml_assets_and_preserved_evidence() {
    let bytes = map_inputs();
    let baseline = load(bytes.clone());
    assert_eq!(baseline.fingerprint, load(bytes.clone()).fingerprint);
    let mut mutation = bytes.clone();
    mutation.get_mut(GEOMETRY).unwrap().push(b' ');
    assert_ne!(baseline.fingerprint, load(mutation).fingerprint);
    let mut mutation = bytes.clone();
    mutation.get_mut(RULES).unwrap().push(b'\n');
    assert_ne!(baseline.fingerprint, load(mutation).fingerprint);
    let file = "reference/release-7.0/xml/dungeons/snakePit.xml";
    let original = std::fs::read(format!("data/{file}")).unwrap();
    let mut mutation = bytes;
    mutation.insert(file.into(), original.clone());
    let manifest = String::from_utf8(mutation[MANIFEST].clone())
        .unwrap()
        .replace("</Game>", &format!("<Evidence file=\"{file}\"/></Game>"));
    mutation.insert(MANIFEST.into(), manifest.into_bytes());
    let with_evidence = load(mutation.clone());
    mutation.get_mut(file).unwrap().push(b' ');
    assert_ne!(with_evidence.fingerprint, load(mutation).fingerprint);
    assert_eq!(original, std::fs::read(format!("data/{file}")).unwrap());
    if let Some(dir) = std::env::var_os("QA_RESULTS_DIR") {
        std::fs::write(Path::new(&dir).join("xml-compile-fingerprints.json"),serde_json::to_vec_pretty(&json!({"baseline":baseline.fingerprint,"with_evidence":with_evidence.fingerprint,"inputs":with_evidence.inputs,"qualification":"synthetic compile fingerprint; no production closure"})).unwrap()).unwrap();
    }
}
#[test]
fn xml_compile_manifest_paths_schema_and_selected_hash_negatives() {
    let err = format!(
        "{:#}",
        GameData::load(
            Path::new("data/nonexistent-xml-compiler-regression"),
            &CONTENT
        )
        .unwrap_err()
    );
    assert!(
        err.contains(
            "required input data/nonexistent-xml-compiler-regression/game/123.5.1/game.xml"
        )
    );
    for (m, expected) in [
        (r#"<Game schema="2"/>"#, "unsupported Game schema"),
        (
            r#"<Game schema="1"><Include file="../rules.xml"/></Game>"#,
            "invalid relative input path",
        ),
        (
            r#"<Game schema="1"><Include file="rules.xml"/><Include file="rules.xml"/></Game>"#,
            "duplicate Include",
        ),
        (
            r#"<Game schema="1"><Include file="rules.xml"/><Unknown/></Game>"#,
            "unknown Game element",
        ),
        (
            r#"<Game schema="1"><Include file="rules.xml"/></Game>"#,
            "no production roots",
        ),
    ] {
        let mut bytes = simple("<Behavior>Wander</Behavior>");
        bytes.insert(MANIFEST.into(), m.as_bytes().to_vec());
        error(bytes, expected);
    }
    let mut bytes = simple("<Behavior>Wander</Behavior>");
    bytes.get_mut("objects.xml").unwrap().push(b' ');
    error(bytes, "selected-client fingerprint mismatch");
    let mut bytes = simple("<Behavior>Wander</Behavior>");
    bytes.remove(MANIFEST);
    error(bytes, "missing required input game/123.5.1/game.xml");
}

#[test]
fn xml_compile_source_documented_units_and_context_specific_fields() {
    let game = pirate(
        r#"<Behavior centerObject="true" childId="Grey Wall">ObjectToss</Behavior>
        <Behavior predictive="true">Follow</Behavior><Behavior predictive=".25">Shoot</Behavior>
        <Behavior rate="-.2">ChangeSize</Behavior><Behavior turnRate="-1">Buzz</Behavior>
        <Behavior angle="1">BackAndForth</Behavior><Behavior angle="90">Move</Behavior>
        <Behavior text="Hello {PLAYER}; Goodbye {PLAYER}">Say</Behavior>
        <State id="A"><Transition onTile="Grass,Dark Grass">B</Transition></State><State id="B"/>"#,
    );
    let def = &game.definitions[&0x600];
    let b = &def.states[0].behaviors;
    assert_matches!(b[0].fields[&Field::CenterObject], Value::Bool(true));
    assert_matches!(b[1].fields[&Field::Predictive], Value::Bool(true));
    assert_matches!(b[2].fields[&Field::Predictive],Value::Number(n) if n == 0.25);
    assert_matches!(b[3].fields[&Field::Rate],Value::Number(n) if n == -0.2);
    assert_matches!(b[4].fields[&Field::TurnRate],Value::Number(n) if n == -1.);
    assert_matches!(b[5].fields[&Field::Angle],Value::Radians(n) if n == 1.);
    assert_matches!(b[6].fields[&Field::Angle],Value::Radians(n) if n == std::f64::consts::FRAC_PI_2);
    assert_matches!(b[7].fields[&Field::Text],Value::TextChoices(ref choices) if choices.len()==2);
    assert_matches!(def.states[1].transitions[0].predicates[0],Predicate::OnGround(ref grounds) if grounds.len()==2);
    error(
        simple(
            "<DefineEmbed name=\"Unused\"><Embed name=\"Unused\"/></DefineEmbed><Behavior>Wander</Behavior>",
        ),
        "recursive embed",
    );
    let body = r#"<DefineEmbed name="Leaf"><Behavior>Wander</Behavior></DefineEmbed><DefineEmbed name="Outer"><Embed name="Leaf"/></DefineEmbed><Embed name="Outer"/>"#;
    assert_eq!(
        pirate(body).definitions[&0x600].states[0].behaviors.len(),
        1
    );
}

#[test]
fn xml_compile_server_property_ownership_without_client_overlay() {
    let game = pirate(
        "<Behavior>Wander</Behavior><SpawnProb>.3</SpawnProb><Terrain>ShoreSand</Terrain><PerRealmMax>4</PerRealmMax><XpMult>2</XpMult><Spawn><Mean>3</Mean><StdDev>1</StdDev><Min>2</Min><Max>4</Max></Spawn>",
    );
    let def = &game.definitions[&0x600];
    assert_eq!(def.population.weight, Some(0.3));
    assert_eq!(def.population.terrain, Some(11));
    assert_eq!(def.population.group.as_ref().unwrap().max, 4);
    assert_eq!(def.population.per_world_max, Some(4));
    assert_eq!(def.xp_multiplier, 2.);
    assert_eq!(
        game.properties[&0x600].xp_multiplier,
        CONTENT.objects[&0x600].xp_multiplier
    );
    assert_eq!(
        serde_json::to_value(&game.properties).unwrap(),
        serde_json::to_value(&CONTENT.objects).unwrap()
    );
    for (fields, expected) in [
        (
            "<SpawnProb>1</SpawnProb><SpawnProb>.5</SpawnProb>",
            "duplicate server property",
        ),
        ("<Terrain>Guess</Terrain>", "unknown selected terrain"),
        ("<Spawn><Mean>3</Mean></Spawn>", "incomplete Spawn"),
        (
            "<Spawn><Mean>3</Mean><Mean>2</Mean></Spawn>",
            "duplicate Spawn field",
        ),
        (
            "<PerRealmMax>0</PerRealmMax>",
            "invalid bounded per-world population",
        ),
    ] {
        error(
            simple(&format!("<Behavior>Wander</Behavior>{fields}")),
            expected,
        );
    }
}

fn generator_xml(kind: &str, id: &str) -> String {
    if kind == "beach" {
        return format!(
            r#"<Generator id="{id}" {QUALIFIED} kind="beach" seed="none" width="64" height="64" padding="1" start="start" boss="start" center="map"><Tile role="background" ground="Grass"/><Room id="start" shape="template" weight="1" maxCount="1" padding="1"><Variant template="room" weight="1"/><Population object="Grey Wall" min="1" max="1" prob="1"/></Room></Generator>"#
        );
    }
    format!(
        r#"<Generator id="{id}" {QUALIFIED} kind="{kind}" seed="world" width="64" height="64" mainBranchMin="3" mainBranchMax="5" subBranchesMin="1" subBranchesMax="2" subBranchMin="1" subBranchMax="2" corridorLengthMin="2" corridorLengthMax="5" corridorWidthMin="1" corridorWidthMax="3" padding="1" start="start" boss="boss" center="map" ensurePath="true">
        <Tile role="background" ground="Grass"/><Tile role="hallway" ground="Grass"/>
        <Room id="start" shape="template" weight="1" maxCount="1" padding="1"><Variant template="room" weight="1"/></Room>
        <Room id="boss" shape="circle" radiusMin="2" radiusMax="4" floor="Grass" weight="1" maxCount="1" padding="1"><Population object="Grey Wall" min="1" max="2" prob=".5"/></Room>
        <Room id="normal" shape="rectangle" widthMin="3" widthMax="5" heightMin="2" heightMax="6" floor="Grass" weight="3" maxCount="5" padding="1"/>
        <Replace from="Grass" ground="Dark Grass" distribution="uniform" prob=".2" scope="cell"/><Scatter on="Grass" object="Grey Wall" distribution="uniform" prob=".1" scope="cell" max="4"/>
    </Generator>"#
    )
}
#[test]
fn xml_compile_room_corridor_recovered_davy_zero_is_not_inheritance() {
    use geometry::{GeneratorLayout, Geometry};
    let text = std::fs::read_to_string("data/reference/release-7.0/xml/dungeons.xml").unwrap();
    let source = roxmltree::Document::parse(&text).unwrap();
    let davy = source
        .descendants()
        .find(|n| n.has_tag_name("Dungeon") && n.attribute("id") == Some("Davy Jones's Locker"))
        .unwrap();
    let literal = |n: roxmltree::Node<'_, '_>, tag: &str| {
        n.children()
            .find(|n| n.has_tag_name(tag))
            .unwrap()
            .text()
            .unwrap()
            .trim()
            .to_owned()
    };
    assert_eq!(literal(davy, "MinHallwayLen"), "1");
    assert_eq!(literal(davy, "MaxHallwayLen"), "1");
    let base = generator_xml("davy", "Contract Map")
        .replace(
            "corridorLengthMin=\"2\" corridorLengthMax=\"5\"",
            "corridorLengthMin=\"1\" corridorLengthMax=\"1\"",
        )
        .replace("padding=\"1\"", "padding=\"0\"");
    let load =
        |xml: &str| production_syntax_load(geometry_inputs(&(template_xml("room") + xml))).unwrap();
    let inherited = load(&base);
    let Geometry::Generator(g) = &inherited.maps[1].geometry else {
        panic!("generator")
    };
    assert!(g.rooms.iter().all(|r| r.corridor_length.is_none()));
    let mut checked = Vec::new();
    for row in davy
        .children()
        .filter(|n| n.has_tag_name("Room") && n.children().any(|n| n.has_tag_name("MinHallwayLen")))
    {
        let min = literal(row, "MinHallwayLen");
        let max = literal(row, "MaxHallwayLen");
        assert_eq!(
            (
                min.as_str(),
                max.as_str(),
                literal(row, "MinPadding").as_str()
            ),
            ("0", "0", "0")
        );
        // All three generic Room shapes exercise the same recovered override, not Davy dispatch.
        let xml = base.replace(
            "<Room ",
            &format!("<Room corridorLengthMin=\"{min}\" corridorLengthMax=\"{max}\" "),
        );
        let game = load(&xml);
        same_selected_properties(&game);
        assert_ne!(inherited.fingerprint, game.fingerprint);
        let Geometry::Generator(g) = &game.maps[1].geometry else {
            panic!("generator")
        };
        let GeneratorLayout::Branches {
            corridor_length, ..
        } = g.layout
        else {
            panic!("branches")
        };
        assert_eq!((corridor_length.min, corridor_length.max), (1, 1));
        for room in &g.rooms {
            let span = room.corridor_length.unwrap();
            assert_eq!((span.min, span.max, room.padding), (0, 0, 0));
        }
        let changed = load(&xml.replacen("corridorLengthMax=\"0\"", "corridorLengthMax=\"1\"", 1));
        assert_ne!(game.fingerprint, changed.fingerprint);
        same_selected_properties(&changed);
        let Geometry::Generator(g) = &changed.maps[1].geometry else {
            panic!("generator")
        };
        assert_eq!(g.rooms[0].corridor_length.unwrap().max, 1);
        assert!(
            g.rooms[1..]
                .iter()
                .all(|r| r.corridor_length.unwrap().max == 0)
        );
        checked.push(json!({"source_room":row.attribute("id").unwrap(),"source_line":source.text_pos_at(row.range().start).row,"min":min,"max":max,"padding":0,"shapes":["template","circle","rectangle"]}));
    }
    assert_eq!(checked.len(), 7);
    let cap = load(&base.replacen(
        "<Room ",
        "<Room corridorLengthMin=\"0\" corridorLengthMax=\"2048\" ",
        1,
    ));
    let Geometry::Generator(g) = &cap.maps[1].geometry else {
        panic!("generator")
    };
    assert_eq!(g.rooms[0].corridor_length.unwrap().max, 2048);
    let dir = std::env::var("QA_RESULTS_DIR").unwrap();
    std::fs::write(Path::new(&dir).join("xml-compile-room-corridors.json"),serde_json::to_vec_pretty(&json!({"qualification":"compiler contract from actual Davy source overrides across3generic fixture shapes; NOT current-production emission or native generation acceptance","global":[1,1],"overrides":checked,"inherited_fixture_fingerprint":inherited.fingerprint})).unwrap()).unwrap();
}
#[test]
fn xml_compile_room_corridor_refusals_are_strict_and_contextual() {
    for (attrs, expected) in [
        (r#"corridorLengthMin="0""#, "missing corridorLengthMax"),
        (r#"corridorLengthMax="0""#, "missing corridorLengthMin"),
        (
            r#"corridorLengthMin="-1" corridorLengthMax="0""#,
            "invalid nonnegative integer",
        ),
        (
            r#"corridorLengthMin="1" corridorLengthMax="0""#,
            "invalid geometry range",
        ),
        (
            r#"corridorLengthMin="0" corridorLengthMax="2049""#,
            "invalid geometry range",
        ),
        (
            r#"corridorLengthMin="0" corridorLengthMax="4294967296""#,
            "invalid nonnegative integer",
        ),
        (
            r#"corridorLengthMin="NaN" corridorLengthMax="1""#,
            "invalid nonnegative integer",
        ),
        (
            r#"corridorLengthMin="0" corridorLengthMax="inf""#,
            "invalid nonnegative integer",
        ),
        (
            r#"corridorLengthMin="0.5" corridorLengthMax="1""#,
            "invalid nonnegative integer",
        ),
        (
            r#"corridorLengthMin="" corridorLengthMax="1""#,
            "missing corridorLengthMin",
        ),
        (
            r#"corridorLengthMin="0" corridorLengthMx="0""#,
            "unknown Room attribute corridorLengthMx",
        ),
    ] {
        for room in ["start", "boss", "normal"] {
            let xml = template_xml("room")
                + &generator_xml("pirate", "Contract Map").replace(
                    &format!("<Room id=\"{room}\""),
                    &format!("<Room {attrs} id=\"{room}\""),
                );
            let err = format!(
                "{:#}",
                production_syntax_load(geometry_inputs(&xml)).unwrap_err()
            );
            assert!(
                err.contains(expected)
                    && err.contains("geometry.xml")
                    && err.contains("geometry=Contract Map"),
                "{room} {attrs}: {err}"
            );
        }
    }
    let xml = template_xml("room")
        + &generator_xml("beach", "Contract Map").replace(
            "<Room ",
            "<Room corridorLengthMin=\"0\" corridorLengthMax=\"0\" ",
        );
    let err = format!(
        "{:#}",
        production_syntax_load(geometry_inputs(&xml)).unwrap_err()
    );
    assert!(
        err.contains("unused Room start corridor override on unseeded Beach")
            && err.contains("geometry.xml"),
        "{err}"
    );
}
#[test]
fn xml_compile_geometry_twelve_seeded_generators_and_unseeded_beach() {
    use super::geometry::{GeneratorLayout, Geometry, RoomShape, SeedPolicy};
    for kind in [
        "pirate", "snake", "sprite", "undead", "abyss", "trench", "spider", "lab", "jungle",
        "manor", "beach", "davy", "tomb",
    ] {
        let xml = template_xml("room") + &generator_xml(kind, "Contract Map");
        let mut inputs = geometry_inputs(&xml);
        if kind == "lab" {
            let manifest = String::from_utf8(inputs[MANIFEST].clone()).unwrap();
            inputs.insert(MANIFEST.into(), manifest.replace("</World>", &format!(
                r#"<CoilPlacement rooms="1" perRoom="1" order="boss-first" {QUALIFIED}/></World>"#
            )).into_bytes());
        }
        let game = load(inputs);
        let Geometry::Generator(g) = &game.maps[1].geometry else {
            panic!("expected generator")
        };
        assert_eq!(
            g.seed,
            if kind == "beach" {
                SeedPolicy::Unseeded
            } else {
                SeedPolicy::World
            }
        );
        if let GeneratorLayout::Branches {
            main_branch,
            corridor_width,
            ..
        } = &g.layout
        {
            assert_eq!(main_branch.max, 5);
            assert_eq!(corridor_width.min, 1);
            assert_eq!(g.rooms.len(), 3);
            assert_eq!(g.paint.len(), 2);
            assert!(g.ensure_path);
        } else {
            assert_eq!(kind, "beach");
            assert_eq!(g.rooms.len(), 1);
            assert!(g.paint.is_empty());
        }
        assert_matches!(&g.rooms[0].shape, RoomShape::Templates(v) if v[0].template==0);
        assert_matches!(g.center, super::geometry::Centering::Map);
        assert!(
            game.maps[1].objects.contains(
                &CONTENT
                    .objects
                    .values()
                    .find(|o| o.name == "Grey Wall")
                    .unwrap()
                    .kind
            )
        );
    }
}
#[test]
fn xml_compile_geometry_unknown_unused_and_snapshot_refusals() {
    for (from, to, expected) in [
        (
            "kind=\"pirate\"",
            "kind=\"ninja\"",
            "unknown geometry generator",
        ),
        (
            "seed=\"world\"",
            "seed=\"123\"",
            "fixed/snapshot seeds are forbidden",
        ),
        (
            "padding=\"1\" start",
            "unused=\"1\" padding=\"1\" start",
            "unknown Generator attribute unused",
        ),
        (
            "template=\"room\"",
            "template=\"absent\"",
            "unresolved room template",
        ),
        (
            "start=\"start\"",
            "start=\"absent\"",
            "unresolved generator start",
        ),
        (
            "corridorWidthMin=\"1\"",
            "corridorWidthMin=\"0\"",
            "invalid geometry range",
        ),
        (
            "radiusMin=\"2\"",
            "radiusMin=\"NaN\"",
            "invalid nonnegative integer",
        ),
        (
            "scope=\"cell\"",
            "scope=\"ignored\"",
            "unknown replacement draw scope",
        ),
    ] {
        error(
            geometry_inputs(
                &(template_xml("room")
                    + &generator_xml("pirate", "Contract Map").replace(from, to)),
            ),
            expected,
        );
    }
    error(
        geometry_inputs(
            &(template_xml("room")
                + &generator_xml("beach", "Contract Map")
                    .replace("seed=\"none\"", "seed=\"world\"")),
        ),
        "Beach geometry is unseeded",
    );
    error(
        geometry_inputs(&template_xml("Contract Map").replace(
            "<CharMap char=\".\"",
            "<CharMap char=\"x\" ground=\"Grass\"/><CharMap char=\".\"",
        )),
        "unused template CharMap",
    );
    let mut bytes = map_inputs();
    let m = String::from_utf8(bytes[MANIFEST].clone()).unwrap().replace(
        "</World>",
        "<Parameter name=\"width\" value=\"99\"/></World>",
    );
    bytes.insert(MANIFEST.into(), m.into_bytes());
    error(bytes, "unknown World child Parameter");
    let mut bytes = map_inputs();
    let m = String::from_utf8(bytes[MANIFEST].clone()).unwrap().replace(
        "<Geometry file=\"geometry.xml\"/>",
        "<Map name=\"Contract Map\" file=\"grid.json\" format=\"json\"/>",
    );
    bytes.insert(MANIFEST.into(), m.into_bytes());
    bytes.insert("grid.json".into(), b"{}".to_vec());
    error(bytes, "missing provenance");
}
#[test]
fn xml_compile_geometry_pool_links_all_choices_without_freezing_seed() {
    use super::geometry::Geometry;
    use rand::SeedableRng;
    let mut xml = String::new();
    for i in 0..16 {
        xml += &template_xml(&format!("layout-{i}"));
    }
    xml += &format!(r#"<Pool id="Contract Map" {QUALIFIED} seed="world" expectedCount="16">"#);
    for i in 0..16 {
        xml += &format!(r#"<Entry map="layout-{i}" weight="1"/>"#);
    }
    xml += "</Pool>";
    let game = load(geometry_inputs(&xml));
    let Geometry::Pool(pool) = &game.maps[16].geometry else {
        panic!("expected pool")
    };
    let select = |seed| {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        (0..1024).map(|_| pool.choose(&mut rng)).collect::<Vec<_>>()
    };
    let a = select(1);
    assert_eq!(a, select(1));
    assert_ne!(a, select(2));
    assert_eq!(a.iter().copied().collect::<BTreeSet<_>>().len(), 16);
    for (from, to, expected) in [
        (
            "map=\"layout-0\"",
            "map=\"Missing\"",
            "unresolved pool entry",
        ),
        (
            "expectedCount=\"16\"",
            "expectedCount=\"15\"",
            "expectedCount",
        ),
        (
            "weight=\"1\"",
            "weight=\"0\"",
            "invalid positive geometry count",
        ),
    ] {
        error(geometry_inputs(&xml.replace(from, to)), expected);
    }
    let cycle = format!(
        r#"<Pool id="Contract Map" {QUALIFIED} seed="world" expectedCount="1"><Entry map="Contract Map" weight="1"/></Pool>"#
    );
    error(geometry_inputs(&cycle), "cyclic geometry reference");
}
fn geometry_fixture(
    xml: &str,
    assets: &BTreeMap<String, Vec<u8>>,
) -> anyhow::Result<(Vec<super::MapBinding>, BTreeMap<String, String>)> {
    let doc = super::compile::parse(
        "geometry-fixture.xml",
        format!("<Geometry>{xml}</Geometry>").as_bytes(),
    )?;
    let mut link = super::compile::Link::new(&CONTENT)?;
    for (i, e) in doc.children.iter().enumerate() {
        assert!(
            link.maps
                .insert(super::geometry::name(e)?.into(), i as u32)
                .is_none()
        );
    }
    let mut used = BTreeSet::new();
    let mut inputs = BTreeMap::new();
    let maps = super::geometry::compile(
        &doc.children,
        &link,
        assets,
        &mut used,
        Some(Path::new("data")),
        &mut inputs,
    )?;
    for p in used {
        inputs.insert(p.clone(), super::compile::hash(&assets[&p]));
    }
    Ok((maps, inputs))
}
#[test]
fn xml_compile_real_realm_catalogue_recipes_and_transitive_fingerprints() {
    use super::geometry::Geometry;
    let catalog: serde_json::Value =
        serde_json::from_slice(&std::fs::read("data/realm-catalog.json").unwrap()).unwrap();
    let mut xml = String::new();
    let mut assets = BTreeMap::new();
    let mut ids = Vec::new();
    let mut dimensions = Vec::new();
    for entry in catalog.as_array().unwrap() {
        let hash = entry["sha256"].as_str().unwrap();
        if entry["kind"] == "baseline" {
            let id = entry["id"].as_str().unwrap();
            let file = entry["path"].as_str().unwrap();
            ids.push(id.to_owned());
            dimensions.push([2048, 2048]);
            assets.insert(file.into(), std::fs::read(format!("data/{file}")).unwrap());
            xml += &format!(
                r#"<Map name="{id}" {QUALIFIED} file="{file}" sha256="{hash}" format="realm-wmap" width="2048" height="2048"/>"#
            );
        } else {
            let file = entry["recipe"].as_str().unwrap();
            let bytes = std::fs::read(format!("data/realms/{file}")).unwrap();
            let r: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let id = r["id"].as_str().unwrap();
            let w = r["width"].as_u64().unwrap();
            let h = r["height"].as_u64().unwrap();
            ids.push(id.into());
            dimensions.push([w as u32, h as u32]);
            xml += &format!(
                r#"<Reconstruction id="{id}" {QUALIFIED} recipe="{file}" sha256="{}" outputSha256="{hash}" width="{w}" height="{h}"/>"#,
                super::compile::hash(&bytes)
            );
        }
    }
    xml += &format!(r#"<Pool id="realm" {QUALIFIED} seed="world" expectedCount="16">"#);
    for id in &ids {
        xml += &format!(r#"<Entry map="{id}" weight="1"/>"#);
    }
    xml += "</Pool>";
    let (maps, mut inputs) = geometry_fixture(&xml, &assets).unwrap();
    assert_eq!(maps.len(), 17);
    assert_eq!(
        maps[..16]
            .iter()
            .filter(|m| matches!(m.geometry, Geometry::Encoded(_)))
            .count(),
        3
    );
    assert_eq!(
        maps[..16]
            .iter()
            .filter(|m| matches!(m.geometry, Geometry::Reconstruction(_)))
            .count(),
        13
    );
    for (i, m) in maps[..16].iter().enumerate() {
        let actual = match &m.geometry {
            Geometry::Encoded(m) => m.dimensions,
            Geometry::Reconstruction(r) => r.dimensions,
            _ => panic!("not a Realm layout"),
        };
        assert_eq!(actual, dimensions[i]);
    }
    let retained_encoded_bytes: usize = maps
        .iter()
        .map(|m| match &m.geometry {
            Geometry::Encoded(a) => a.retained_encoded_bytes(),
            Geometry::Reconstruction(r) => r.retained_encoded_bytes(),
            _ => 0,
        })
        .sum();
    assert!(
        retained_encoded_bytes < 16 * 1024 * 1024,
        "retained encoded assets {retained_encoded_bytes}"
    );
    let Geometry::Reconstruction(recipe) = &maps[3].geometry else {
        panic!("expected first reconstruction")
    };
    let mut decoded = recipe.decode(&CONTENT).unwrap();
    recipe.validate_loaded(&decoded, &CONTENT).unwrap();
    let lava = decoded
        .cells
        .iter_mut()
        .find(|c| c.ground == 244)
        .expect("reviewed first reconstruction has scoped lava");
    lava.ground = 0xff;
    assert!(
        recipe.validate_loaded(&decoded, &CONTENT).is_err(),
        "source-bound lava support must remain enforced"
    );
    drop(decoded);
    assert_eq!(inputs.len(), 85); // 3 direct maps + manifest + 16 elevation layers + 13 * 5 reconstruction inputs.
    inputs.insert(
        "realm-catalog.json".into(),
        super::compile::hash(&std::fs::read("data/realm-catalog.json").unwrap()),
    );
    if let Some(dir) = std::env::var_os("QA_RESULTS_DIR") {
        std::fs::write(Path::new(&dir).join("xml-compile-real-geometries.json"),serde_json::to_vec_pretty(&json!({"qualification":"real asset/reconstruction compilation, NOT native generation or actor-behavior production closure","ids":ids,"dimensions":dimensions,"retained_encoded_bytes":retained_encoded_bytes,"inputs":inputs,"bindings":maps})).unwrap()).unwrap();
    }
}
#[test]
fn xml_compile_reconstruction_refuses_source_hash_and_snapshot_substitution() {
    let recipe = std::fs::read("data/realms/world02.json").unwrap();
    let r: serde_json::Value = serde_json::from_slice(&recipe).unwrap();
    let output = std::fs::read(format!("data/realms/{}", r["output"].as_str().unwrap())).unwrap();
    let output = super::compile::hash(&output);
    let bad = format!(
        r#"<Reconstruction id="x" {QUALIFIED} recipe="world02.json" sha256="{}" outputSha256="{output}" width="2100" height="2100"/>"#,
        "0".repeat(64)
    );
    let err = format!(
        "{:#}",
        geometry_fixture(&bad, &BTreeMap::new()).unwrap_err()
    );
    assert!(err.contains("input world02.json checksum"), "{err}");
    let wrong = bad
        .replace(&"0".repeat(64), &super::compile::hash(&recipe))
        .replace("width=\"2100\"", "width=\"2048\"");
    assert!(
        format!(
            "{:#}",
            geometry_fixture(&wrong, &BTreeMap::new()).unwrap_err()
        )
        .contains("metadata/asset dimensions disagree")
    );
    let api = crate::terrain::prepare::compile_geometry_recipe(
        Path::new("data"),
        "../world02.json",
        &super::compile::hash(&recipe),
        &output,
        &CONTENT,
    )
    .unwrap_err();
    assert!(format!("{api:#}").contains("unsafe recipe path"));
}

fn xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
#[test]
fn xml_compile_selected_dungeon_parameter_families_are_typed_not_ignored() {
    use super::geometry::{Centering, Distribution, Geometry, Paint, PlacementPosition};
    let extra = r#"<Replace ground="Dark Grass" distribution="perlin-prob" dx=".01" dy=".02" mult=".2"/>
        <Replace ground="Grass" distribution="perlin-mod" dx=".001" dy=".001" mod="3"/>
        <Replace ground="Grass" distribution="around-spawn" dx="3" dy="3"/>
        <Replace ground="Grass" distribution="random-lines-x" dx=".01" dy=".01" mod="6" val="0"/>
        <Replace ground="Grass" distribution="random-lines-y" dx=".01" dy=".01" mod="6" val="1"/>
        <Replace ground="Grass" distribution="cg-noise"/>
        <Tile role="hallway-end" ground="Grass"/><Tile role="end-cap" object="Grey Wall"/><Tile role="surround" ground="Dark Grass"/>
        <WallVariant object="Grey Wall"/><WallVariant a="6" b="7" object="Grey Wall"/>"#;
    let source=template_xml("room")+&generator_xml("pirate","Contract Map").replace("center=\"map\"","center=\"start\" pathObject=\"Grey Wall\" randomizeReplacements=\"true\" removeHallwayEndObjects=\"true\"").replace("</Generator>",&format!("{extra}</Generator>"))
        .replace("<Variant template=\"room\" weight=\"1\"/>","<Variant template=\"room\" weight=\"1\"/><Variant template=\"other\" weight=\"2\"/>")
        .replace("</Room>",r#"<PopulationGroup selection="weighted"><Population object="Grey Wall" min="1" max="2" prob=".4" position="edge" ground="Grass"/><Population object="Blue Wall" min="1" max="1" prob=".6" position="center-plus-salt"/></PopulationGroup></Room>"#)+&template_xml("other");
    let game = load(geometry_inputs(&source));
    let Geometry::Generator(g) = &game.maps[1].geometry else {
        panic!("expected generator")
    };
    assert_matches!(g.center, Centering::StartRoom);
    assert!(g.path_object.is_some());
    assert!(g.randomize_replacements && g.remove_hallway_end_objects);
    assert!(g.hallway_end.is_some() && g.end_cap.is_some() && g.surround.is_some());
    assert_eq!(g.alternate_walls.len(), 2);
    assert_matches!(g.paint[2],Paint::Replace{distribution:Distribution::PerlinProbability{dx,..},..} if dx==0.01);
    assert_matches!(
        g.paint[7],
        Paint::Replace {
            distribution: Distribution::CgNoise,
            ..
        }
    );
    assert_matches!(
        g.rooms[0].groups[0].choices[0].position,
        PlacementPosition::Edge
    );
    for (from, to, expected) in [
        (
            "mult=\".2\"",
            "mult=\".2\" unused=\"2\"",
            "unknown Replace attribute unused",
        ),
        (
            "mod=\"6\" val=\"1\"",
            "mod=\"6\" val=\"6\"",
            "val must be less than mod",
        ),
        (
            "distribution=\"cg-noise\"",
            "distribution=\"cg-noise\" dx=\"1\"",
            "unknown Replace attribute dx",
        ),
        ("a=\"6\" b=\"7\"", "a=\"6\"", "a/b must be paired"),
        (
            "position=\"edge\"",
            "position=\"guess\"",
            "unknown geometry population position",
        ),
        ("prob=\".4\"", "prob=\".8\"", "probability exceeds one"),
    ] {
        error(geometry_inputs(&source.replace(from, to)), expected);
    }
    let unused = generator_xml("beach", "Contract Map").replace(
        "padding=\"1\" start",
        "mainBranchMin=\"1\" padding=\"1\" start",
    );
    error(
        geometry_inputs(&(template_xml("room") + &unused)),
        "unknown Generator attribute mainBranchMin",
    );
}

fn role_fixture(tile: &str) -> String {
    template_xml("room")
        + &generator_xml("jungle", "Contract Map")
            .replace(r#"<Tile role="background" ground="Grass"/>"#, tile)
}
#[test]
fn xml_compile_recovered_jungle_probability_tiles_keep_source_masses_and_layers() {
    use super::geometry::Geometry;
    let text = std::fs::read_to_string("data/reference/release-7.0/xml/dungeons.xml").unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let jungle = doc
        .descendants()
        .find(|n| n.has_tag_name("Dungeon") && n.attribute("id") == Some("Forbidden Jungle"))
        .unwrap();
    let mut xml = template_xml("room") + &generator_xml("jungle", "Contract Map");
    let mut expected = Vec::new();
    for (container, ground_tag, role) in [
        ("DefaultObj", "DefaultTile", "background"),
        ("WallObj", "WallTile", "wall"),
    ] {
        let ground = jungle
            .children()
            .find(|n| n.has_tag_name(ground_tag))
            .unwrap()
            .text()
            .unwrap();
        let mut tile = format!(r#"<Tile role="{role}" ground="{}">"#, xml_attr(ground));
        let mut rows = Vec::new();
        for row in jungle
            .children()
            .find(|n| n.has_tag_name(container))
            .unwrap()
            .children()
            .filter(|n| n.is_element())
        {
            let name = row.text().unwrap().trim();
            let prob = row.attribute("prob").unwrap();
            tile += &format!(r#"<Choice prob="{prob}" object="{}"/>"#, xml_attr(name));
            rows.push((
                prob.parse::<f64>().unwrap(),
                CONTENT
                    .objects
                    .values()
                    .find(|o| o.name == name)
                    .unwrap()
                    .kind,
            ));
        }
        tile += "</Tile>";
        if role == "background" {
            xml = xml.replace(r#"<Tile role="background" ground="Grass"/>"#, &tile);
        } else {
            xml = xml.replace("</Generator>", &format!("{tile}</Generator>"));
        }
        expected.push((ground.to_owned(), rows));
    }
    assert_eq!(
        expected[0].1.iter().map(|r| r.0).collect::<Vec<_>>(),
        [0.30, 0.20, 0.20, 0.15, 0.15]
    );
    assert_eq!(
        expected[1].1.iter().map(|r| r.0).collect::<Vec<_>>(),
        [0.75, 0.25]
    );
    let (maps, _) = geometry_fixture(&xml, &BTreeMap::new()).unwrap();
    let Geometry::Generator(g) = &maps[1].geometry else {
        panic!("generator")
    };
    for (role, (ground, rows)) in [
        (&g.background, &expected[0]),
        (g.wall.as_ref().unwrap(), &expected[1]),
    ] {
        assert_eq!(CONTENT.ground_names[&role.common.ground.unwrap()], *ground);
        assert!(role.common.object.is_none());
        assert_eq!(
            role.choices
                .iter()
                .map(|c| (c.probability, c.tile.object.unwrap()))
                .collect::<Vec<_>>(),
            *rows
        );
        assert!(rows.iter().all(|(_, id)| maps[1].objects.contains(id)));
    }
    for (draw, index) in [(0., 0), (0.31, 1), (0.51, 2), (0.71, 3), (0.86, 4)] {
        assert_eq!(
            g.background.choose(draw).unwrap().unwrap().object,
            Some(expected[0].1[index].1)
        );
    }
    assert_eq!(
        g.wall
            .as_ref()
            .unwrap()
            .choose(0.75)
            .unwrap()
            .unwrap()
            .object,
        Some(expected[1].1[1].1)
    );
    let half = r#"<Tile role="background" ground="Grass"><Choice prob=".3" object="Grey Wall"/><Choice prob=".2" object="Blue Wall"/></Tile>"#;
    let game = load(geometry_inputs(&role_fixture(half)));
    let Geometry::Generator(g) = &game.maps[1].geometry else {
        panic!("generator")
    };
    assert!(g.background.choose(0.5).unwrap().is_none());
    assert!(g.background.choose(0.99).unwrap().is_none());
    assert!(g.background.common.object.is_none());
    let changed = load(geometry_inputs(&role_fixture(&half.replace(".3", ".29"))));
    assert_ne!(game.fingerprint, changed.fingerprint);
    let missing = role_fixture(
        r#"<Tile role="background" ground="Grass"><Choice prob=".5" object="Piratess"/><Choice prob=".5" object="Grey Wall"/></Tile>"#,
    );
    error(
        geometry_inputs(&missing),
        "Piratess has no explicit admission definition/disposition",
    );
}
#[test]
fn xml_compile_probability_tile_refusals_are_contextual_and_not_socket_weights() {
    for (body, expected) in [
        (r#"<Choice prob=".5"/>"#, "empty Tile Choice layers"),
        (r#"<Choice object="Grey Wall"/>"#, "missing prob"),
        (r#"<Choice prob="NaN" object="Grey Wall"/>"#, "nonfinite"),
        (r#"<Choice prob="inf" object="Grey Wall"/>"#, "nonfinite"),
        (
            r#"<Choice prob="-.1" object="Grey Wall"/>"#,
            "probability outside",
        ),
        (
            r#"<Choice prob="1.1" object="Grey Wall"/>"#,
            "probability outside",
        ),
        (
            r#"<Choice prob="0" object="Grey Wall"/>"#,
            "probability mass must be",
        ),
        (
            r#"<Choice prob=".6" object="Grey Wall"/><Choice prob=".5" object="Blue Wall"/>"#,
            "probability mass must be",
        ),
        (
            r#"<Choice prob=".3" ground="Grass"/>"#,
            "ground layers overlap",
        ),
        (
            r#"<Choice prob=".3" object="Imaginary Object"/>"#,
            "unresolved selected object",
        ),
        (
            r#"<Choice weight="3" object="Grey Wall"/>"#,
            "unknown Choice attribute weight",
        ),
    ] {
        let tile = format!("<Tile role=\"background\" ground=\"Grass\">{body}</Tile>");
        error(geometry_inputs(&role_fixture(&tile)), expected);
    }
    let err = geometry_fixture(
        &role_fixture(
            r#"<Tile role="background" ground="Grass"><Choice prob=".5" object="Unknown"/></Tile>"#,
        ),
        &BTreeMap::new(),
    )
    .unwrap_err();
    let err = format!("{err:#}");
    assert!(
        err.contains("geometry-fixture.xml:") && err.contains("geometry=Contract Map"),
        "{err}"
    );
    error(
        geometry_inputs(&role_fixture(
            r#"<Tile role="background" object="Grey Wall"><Choice prob=".5" object="Blue Wall"/></Tile>"#,
        )),
        "object layers overlap",
    );
    let beach=template_xml("room")+&generator_xml("beach","Contract Map").replace(r#"<Tile role="background" ground="Grass"/>"#,r#"<Tile role="background" ground="Grass"><Choice prob=".5" object="Grey Wall"/></Tile>"#);
    error(
        geometry_inputs(&beach),
        "random role Tile on unseeded Beach",
    );
}
#[test]
fn xml_compile_recovered_group_population_keeps_one_row_and_caller_rng() {
    use super::geometry::{Geometry, MemberSelection, PopulationSelector};
    use rand::{Rng, RngCore, SeedableRng};
    let text =
        std::fs::read_to_string("data/reference/release-7.0/xml/dungeons/pirateCave.xml").unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let mut members = Vec::new();
    let mut objects = String::new();
    for o in doc.root_element().children().filter(|n| {
        n.is_element()
            && n.children()
                .any(|c| c.has_tag_name("Group") && c.text() == Some("Cave Pirates"))
    }) {
        let id = super::super::kind(o.attribute("type").unwrap()).unwrap();
        members.push(id);
        objects += &object(
            o.attribute("id").unwrap(),
            id,
            "<Group>Cave Pirates</Group><Behavior>Wander</Behavior>",
        );
    }
    members.sort();
    assert_eq!(members, [0x687, 0x688, 0x689]);
    let d = std::fs::read_to_string("data/reference/release-7.0/xml/dungeons.xml").unwrap();
    let d = roxmltree::Document::parse(&d).unwrap();
    let cave = d
        .descendants()
        .find(|n| n.has_tag_name("Dungeon") && n.attribute("id") == Some("Pirate Cave"))
        .unwrap();
    let mut rows = String::new();
    for e in cave
        .descendants()
        .filter(|n| n.has_tag_name("Enemy") && n.text().map(str::trim) == Some("Cave Pirates"))
    {
        rows += &format!(
            r#"<Population group="Cave Pirates" min="{}" max="{}"/>"#,
            e.attribute("minNum").unwrap(),
            e.attribute("maxNum").unwrap()
        );
    }
    // Nondefault placement metadata is authored test input; the counts/group come from recovered XML.
    rows = rows.replace(
        "max=\"7\"",
        "max=\"7\" prob=\".4\" position=\"edge\" ground=\"Grass\"",
    );
    let geometry = template_xml("room")
        + &generator_xml("pirate", "Contract Map").replace(
            r#"<Variant template="room" weight="1"/>"#,
            &format!(r#"<Variant template="room" weight="1"/>{rows}"#),
        );
    let mut bytes = geometry_inputs(&geometry);
    let actor = object("Pirate", 0x600, "<Behavior>Wander</Behavior>");
    bytes.insert(RULES.into(), fixture_rules(&(actor + &objects)));
    let baseline = load(bytes.clone());
    let Geometry::Generator(g) = &baseline.maps[1].geometry else {
        panic!("generator")
    };
    let pack = &g.rooms[0].pack;
    assert_eq!(pack.len(), 2);
    assert_eq!((pack[0].population.min, pack[0].population.max), (6, 6));
    assert_eq!((pack[1].population.min, pack[1].population.max), (4, 7));
    let PopulationSelector::Group {
        name,
        members: linked,
        selection,
    } = &pack[1].population.selector
    else {
        panic!("group selector")
    };
    assert_eq!(name, "Cave Pirates");
    assert_eq!(*linked, members);
    assert_matches!(selection, MemberSelection::UniformEligibleAuthored);
    let geometry_before = bytes[GEOMETRY].clone();
    let changed_rules = String::from_utf8(bytes[RULES].clone()).unwrap().replacen(
        "<Group>Cave Pirates</Group>",
        "<Group>Other Source Group</Group>",
        1,
    );
    bytes.insert(RULES.into(), changed_rules.into_bytes());
    let changed = load(bytes.clone());
    let Geometry::Generator(g2) = &changed.maps[1].geometry else {
        panic!("generator")
    };
    assert_eq!(bytes[GEOMETRY], geometry_before);
    assert_ne!(baseline.fingerprint, changed.fingerprint);
    assert_eq!(g2.rooms[0].pack.len(), 2);
    assert_eq!(
        (
            g2.rooms[0].pack[1].population.min,
            g2.rooms[0].pack[1].population.max
        ),
        (4, 7)
    );
    assert_eq!(pack[1].probability, 0.4);
    assert_eq!(g2.rooms[0].pack[1].probability, pack[1].probability);
    assert_matches!(pack[1].position, super::geometry::PlacementPosition::Edge);
    assert_matches!(
        g2.rooms[0].pack[1].position,
        super::geometry::PlacementPosition::Edge
    );
    assert!(pack[1].ground.is_some());
    assert_eq!(g2.rooms[0].pack[1].ground, pack[1].ground);
    assert_eq!(
        g2.rooms[0].pack[1].population.selector.candidates(),
        [0x688, 0x689]
    );
    assert!(baseline.reachable.contains(&0x687));
    assert!(!changed.reachable.contains(&0x687));
    let eligible = members.iter().copied().collect::<BTreeSet<_>>();
    let mut rng = rand::rngs::StdRng::seed_from_u64(91);
    for selector in [
        &pack[1].population.selector,
        &g2.rooms[0].pack[1].population.selector,
    ] {
        let mut expected = rng.clone();
        let expected_id = selector.candidates()[expected.gen_range(0..selector.candidates().len())];
        assert_eq!(selector.choose(&eligible, &mut rng), Some(expected_id));
        assert_eq!(rng.next_u64(), expected.next_u64());
    }
    let mut expected = rng.clone();
    assert_eq!(
        pack[1]
            .population
            .selector
            .choose(&BTreeSet::new(), &mut rng),
        None
    );
    assert_eq!(rng.next_u64(), expected.next_u64());
    for (row, expected) in [
        (
            r#"<Population group="Unknown" min="4" max="7"/>"#,
            "unknown/empty admitted group",
        ),
        (r#"<Population group="" min="4" max="7"/>"#, "missing group"),
        (
            r#"<Population group="Cave Pirates" object="Pirate" min="4" max="7"/>"#,
            "exactly one of object or group",
        ),
        (
            r#"<Population object="Cave Pirates" min="4" max="7"/>"#,
            "unresolved selected object",
        ),
    ] {
        let mut input = bytes.clone();
        input.insert(
            GEOMETRY.into(),
            String::from_utf8(geometry_before.clone())
                .unwrap()
                .replace(&rows, row)
                .into_bytes(),
        );
        error(input, expected);
    }
    let mut empty = bytes.clone();
    let rules = String::from_utf8(empty[RULES].clone()).unwrap().replace(
        "<Group>Cave Pirates</Group>",
        "<Group>Other Source Group</Group>",
    );
    empty.insert(RULES.into(), rules.into_bytes());
    error(empty, "unknown/empty admitted group");
}
#[test]
fn xml_compile_recovered_buzz_accepts_corrected_acquire_range_and_valid_defaults() {
    let source =
        std::fs::read_to_string("data/reference/release-7.0/xml/dungeons/oryxCastle.xml").unwrap();
    let doc = roxmltree::Document::parse(&source).unwrap();
    let recovered = doc
        .descendants()
        .find(|n| {
            n.has_tag_name("Behavior")
                && n.text() == Some("Buzz")
                && n.attribute("aquireRange").is_some()
        })
        .unwrap();
    let range = recovered
        .attribute("aquireRange")
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert_eq!(range, 4.);
    let attrs = recovered
        .attributes()
        .map(|a| {
            format!(
                "{}=\"{}\"",
                if a.name() == "aquireRange" {
                    "acquireRange"
                } else {
                    a.name()
                },
                xml_attr(a.value())
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let corrected = format!("<Behavior {attrs}>Buzz</Behavior>");
    for (body, range) in [
        (corrected.as_str(), range),
        ("<Behavior>Buzz</Behavior>", 20.),
    ] {
        let game = pirate(body);
        let b = &game.definitions[&0x600].states[0].behaviors[0];
        assert_matches!(b.fields[&Field::AcquireRange],Value::Number(n) if n==range);
        assert!(b.fields.keys().all(|f| b.kind.fields().contains(f)));
    }
    error(
        simple(&source[recovered.range()]),
        "unknown server attribute aquireRange",
    );
    let game = pirate(
        "<Behavior>Shoot</Behavior><Behavior>Decay</Behavior><Behavior>ChangeSize</Behavior><Behavior>Buzz</Behavior><Behavior>Move</Behavior><Behavior>Swirl</Behavior><Behavior>BackAndForth</Behavior><Behavior>StayInTerrain</Behavior><Behavior>SetAltTexture</Behavior>",
    );
    for s in &game.definitions[&0x600].states {
        for b in &s.behaviors {
            assert!(
                b.fields.keys().all(|f| b.kind.fields().contains(f)),
                "{:?}",
                b.kind
            );
        }
    }
}

// Read-only hard links retain the real selected inputs inside the loader's path sandbox.
fn production_syntax_load(bytes: BTreeMap<String, Vec<u8>>) -> anyhow::Result<GameData> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = Path::new(&std::env::var("QA_RESULTS_DIR").expect("run via qa.py"))
        .join("production-syntax-cases")
        .join(
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string(),
        );
    std::fs::create_dir_all(&dir)?;
    for (name, bytes) in bytes {
        let path = dir.join(&name);
        std::fs::create_dir_all(path.parent().unwrap())?;
        if matches!(name.as_str(), "objects.xml" | "grounds.xml") {
            std::fs::hard_link(Path::new("data").join(name), path)?;
        } else {
            std::fs::write(path, bytes)?;
        }
    }
    GameData::load(&dir, &CONTENT)
}
fn production_syntax_error(bytes: BTreeMap<String, Vec<u8>>, expected: &str) {
    let err = format!("{:#}", production_syntax_load(bytes).unwrap_err());
    assert!(
        err.contains(expected) && err.contains("rules.xml"),
        "{expected}: {err}"
    );
}
#[test]
fn xml_compile_production_syntax_real_sheep_separate_namespaces() {
    use geometry::{Geometry, PopulationSelector};
    let text = std::fs::read_to_string("data/reference/release-7.0/xml/objects.xml").unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let sheep = doc
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("id") == Some("Sheep"))
        .unwrap();
    assert_eq!(sheep.attribute("type"), Some("0x01ff"));
    assert_eq!(
        sheep
            .children()
            .find(|n| n.has_tag_name("Class"))
            .unwrap()
            .text(),
        Some("Character")
    );
    let body: String = sheep
        .children()
        .filter(|n| n.is_element() && ["Group", "XpMult", "State"].contains(&n.tag_name().name()))
        .map(|n| &text[n.range()])
        .collect();
    assert!(body.contains("<Group>Sheep</Group>"));
    let rules = object("Sheep", 0x1ff, &body)
        + &object(
            "Piratess",
            0x601,
            "<Group>Sheep</Group><Group>Only Sheep Group</Group><Behavior>Wander</Behavior>",
        );
    let actor = object(
        "Pirate",
        0x600,
        "<Behavior groupId=\"Sheep\" maxChildren=\"2\">MakeMinions</Behavior>",
    );
    let geometry = template_xml("room") + &generator_xml("pirate", "Contract Map").replace(
        "<Variant template=\"room\" weight=\"1\"/>",
        "<Variant template=\"room\" weight=\"1\"/><Population object=\"Sheep\" min=\"1\" max=\"1\"/><Population group=\"Sheep\" min=\"1\" max=\"1\"/>");
    let mut bytes = geometry_inputs(&geometry);
    bytes.insert(RULES.into(), fixture_rules(&(actor + &rules)));
    let game = production_syntax_load(bytes.clone()).unwrap();
    let sheep = &game.definitions[&0x1ff];
    assert_matches!(sheep.disposition, Disposition::Active);
    let walking = sheep.states.iter().find(|s| s.name == "Walking").unwrap();
    assert_eq!(
        walking.behaviors.iter().map(|b| b.kind).collect::<Vec<_>>(),
        [BehaviorKind::Walk, BehaviorKind::Say]
    );
    assert_eq!(
        sheep.states[walking.transitions[0].targets[0] as usize].name,
        "Eating"
    );
    let Geometry::Generator(g) = &game.maps[1].geometry else {
        panic!("generator")
    };
    assert_matches!(
        g.rooms[0].pack[0].population.selector,
        PopulationSelector::Object(0x1ff)
    );
    assert_matches!(&g.rooms[0].pack[1].population.selector, PopulationSelector::Group { members, .. } if members == &[0x1ff,0x601]);
    assert_matches!(&game.definitions[&0x600].states[0].behaviors[0].fields[&Field::GroupId], Value::Objects(ids) if ids == &[0x1ff,0x601]);
    for (from, to, expected) in [
        (
            "object=\"Sheep\"",
            "object=\"Only Sheep Group\"",
            "unresolved selected object",
        ),
        (
            "group=\"Sheep\"",
            "group=\"Pirate\"",
            "unknown/empty admitted group",
        ),
        (
            "object=\"Sheep\"",
            "object=\"Sheep\" group=\"Sheep\"",
            "exactly one",
        ),
    ] {
        let mut changed = bytes.clone();
        changed.insert(
            GEOMETRY.into(),
            format!(
                "<Geometry schema=\"1\">{}</Geometry>",
                geometry.replace(from, to)
            )
            .into_bytes(),
        );
        let err = format!("{:#}", production_syntax_load(changed).unwrap_err());
        assert!(
            err.contains(expected) && err.contains("geometry.xml"),
            "{err}"
        );
    }
    for mixed in [
        "<State id=\"A\"><Transition exist=\"Sheep\">B</Transition></State><State id=\"B\"/>",
        "<Behavior targetIds=\"Sheep\">Heal</Behavior>",
    ] {
        production_syntax_error(
            inputs(object("Pirate", 0x600, mixed) + &rules),
            "ambiguous object/group reference",
        );
    }
    production_syntax_error(
        inputs(
            object(
                "Pirate",
                0x600,
                "<Behavior targetGroups=\"Pirate\">Heal</Behavior>",
            ) + &rules,
        ),
        "unknown/empty admitted group",
    );
    // Exact object fields remain exact even when an identically named group exists.
    production_syntax_load(inputs(
        object(
            "Pirate",
            0x600,
            "<Behavior protecteeId=\"Sheep\">Protect</Behavior>",
        ) + &rules,
    ))
    .unwrap();
}
#[test]
fn xml_compile_production_syntax_source_destination_lists_and_immediate() {
    use rand::{Rng, RngCore, SeedableRng};
    let mut cases = 0;
    for file in [
        "dungeons/abyssOfDemons.xml",
        "encounters.xml",
        "dungeons/ghostShip.xml",
        "dungeons/oryxChamber.xml",
    ] {
        let text =
            std::fs::read_to_string(format!("data/reference/release-7.0/xml/{file}")).unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();
        for t in doc.descendants().filter(|n| {
            n.has_tag_name("Transition")
                && (n.text().unwrap().contains(',')
                    || (file.ends_with("oryxChamber.xml") && n.attributes().len() == 0))
        }) {
            let source = t.parent().unwrap();
            let mut body = String::from("<Behavior>Wander</Behavior><State id=\"Scope\">");
            for sibling in source
                .parent()
                .unwrap()
                .children()
                .filter(|n| n.has_tag_name("State"))
            {
                body += &format!(
                    "<State id=\"{}\">{}</State>",
                    sibling.attribute("id").unwrap(),
                    if sibling == source {
                        &text[t.range()]
                    } else {
                        ""
                    }
                );
            }
            body += "</State>";
            let game = production_syntax_load(simple(&body)).unwrap();
            let def = &game.definitions[&0x600];
            let state = def
                .states
                .iter()
                .find(|s| s.name == source.attribute("id").unwrap())
                .unwrap();
            let transition = &state.transitions[0];
            let expected: Vec<_> = t.text().unwrap().split(',').map(str::trim).collect();
            assert_eq!(
                transition
                    .targets
                    .iter()
                    .map(|i| def.states[*i as usize].name.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(transition.predicates.is_empty(), t.attributes().len() == 0);
            let mut rng = rand::rngs::StdRng::seed_from_u64(71);
            let mut witness = rng.clone();
            assert_eq!(transition.choose_destination(false, &mut rng), None);
            assert_eq!(rng.next_u64(), witness.next_u64());
            for _ in 0..32 {
                let index = if expected.len() == 1 {
                    0
                } else {
                    witness.gen_range(0..expected.len())
                };
                assert_eq!(
                    transition.choose_destination(true, &mut rng),
                    Some(transition.targets[index])
                );
            }
            assert_eq!(rng.next_u64(), witness.next_u64());
            cases += 1;
        }
    }
    assert_eq!(cases, 40);
    let body = "<Behavior>Wander</Behavior><State id=\"First\"><Embed name=\"pick\"/></State><State id=\"B\"/><State id=\"A\"/><DefineEmbed name=\"pick\"><Transition>B,A,B</Transition><Transition afterTime=\"0\">A</Transition></DefineEmbed>";
    let game = production_syntax_load(simple(body)).unwrap();
    let def = &game.definitions[&0x600];
    let transitions = &def.states[1].transitions;
    assert_eq!(transitions.len(), 2);
    assert_eq!(
        transitions[0]
            .targets
            .iter()
            .map(|i| def.states[*i as usize].name.as_str())
            .collect::<Vec<_>>(),
        ["B", "A", "B"]
    );
    assert!(transitions[0].predicates.is_empty());
    assert_eq!(def.states[transitions[1].targets[0] as usize].name, "A");
    assert_eq!(
        transitions[0].source.embeds,
        ["pick@game/123.5.1/rules.xml:1"]
    );
    let mut rng = rand::rngs::StdRng::seed_from_u64(93);
    let mut witness = rng.clone();
    for _ in 0..32 {
        assert_eq!(
            transitions[0].choose_destination(true, &mut rng),
            Some(transitions[0].targets[witness.gen_range(0..3)])
        );
    }
    assert_eq!(rng.next_u64(), witness.next_u64());
    let changed = production_syntax_load(simple(&body.replace("B,A,B", "A,B,B"))).unwrap();
    assert_ne!(game.fingerprint, changed.fingerprint);
}
#[test]
fn xml_compile_production_syntax_source_player_radius_counts() {
    let mut cases = 0;
    for file in ["ghostShip.xml", "madLab.xml"] {
        let text =
            std::fs::read_to_string(format!("data/reference/release-7.0/xml/dungeons/{file}"))
                .unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();
        for t in doc.descendants().filter(|n| {
            n.has_tag_name("Transition")
                && n.attribute("playerWithin").is_some_and(|s| s.contains('|'))
        }) {
            let spec = t.attribute("playerWithin").unwrap();
            let body = format!(
                "<Behavior>Wander</Behavior><State id=\"A\"><Transition playerWithin=\"{spec}\">B</Transition></State><State id=\"B\"/>"
            );
            let game = production_syntax_load(simple(&body)).unwrap();
            let Predicate::PlayerWithin(p) =
                &game.definitions[&0x600].states[1].transitions[0].predicates[0]
            else {
                panic!("proximity")
            };
            let (radius, count) = spec.split_once('|').unwrap();
            assert_eq!(p.radius, radius.parse::<f64>().unwrap());
            assert_eq!(p.min_players, count.parse::<u32>().unwrap());
            assert!(
                !p.matches_eligible_distances(vec![p.radius; p.min_players as usize - 1])
                    .unwrap()
            );
            assert!(
                p.matches_eligible_distances(vec![p.radius; p.min_players as usize])
                    .unwrap()
            );
            assert!(
                p.matches_eligible_distances(vec![0.; p.min_players as usize + 1])
                    .unwrap()
            );
            assert!(
                !p.matches_eligible_distances(vec![p.radius + 0.001; p.min_players as usize + 1])
                    .unwrap()
            );
            assert!(!p.matches_eligible_distances([]).unwrap());
            assert!(p.matches_eligible_distances([f64::NAN]).is_err());
            cases += 1;
        }
    }
    assert_eq!(cases, 10);
    for spec in ["0", "12", "23"] {
        let body = format!(
            "<Behavior>Wander</Behavior><State id=\"A\"><Transition playerWithin=\"{spec}\">B</Transition></State><State id=\"B\"/>"
        );
        let game = production_syntax_load(simple(&body)).unwrap();
        let Predicate::PlayerWithin(p) =
            &game.definitions[&0x600].states[1].transitions[0].predicates[0]
        else {
            panic!("proximity")
        };
        assert_eq!(p.min_players, 1);
        assert!(
            p.matches_eligible_distances([spec.parse().unwrap()])
                .unwrap()
        );
        let with_count =
            production_syntax_load(simple(&body.replace(spec, &format!("{spec}|1")))).unwrap();
        assert_ne!(game.fingerprint, with_count.fingerprint);
    }
}
#[test]
fn xml_compile_production_syntax_strict_destination_and_count_refusals() {
    for destination in ["", " ", ",A", "A,", "A,,B", "A,Missing"] {
        production_syntax_error(
            simple(&format!(
                "<State id=\"A\"><Transition>{destination}</Transition></State><State id=\"B\"/>"
            )),
            if destination.contains("Missing") {
                "unresolved sibling"
            } else {
                "empty reference"
            },
        );
    }
    production_syntax_error(
        simple(
            "<State id=\"Uncle\"/><State id=\"Parent\"><State id=\"Child\"><Transition>Child,Uncle</Transition></State></State>",
        ),
        "unresolved sibling state \"Uncle\"",
    );
    production_syntax_error(
        simple(
            "<State id=\"A\"><Transition playerWithinn=\"2|3\">B</Transition></State><State id=\"B\"/>",
        ),
        "unknown Transition attribute",
    );
    for spec in [
        "",
        "|1",
        "12|",
        "12|0",
        "12|-1",
        "12|1.5",
        "12|NaN",
        "12|inf",
        "12|4294967296",
        "12|2|3",
        "-1|2",
        "NaN|2",
        "inf|2",
        "12||",
        "12,2",
    ] {
        let bytes = simple(&format!(
            "<State id=\"A\"><Transition playerWithin=\"{spec}\">B</Transition></State><State id=\"B\"/>"
        ));
        let err = format!("{:#}", production_syntax_load(bytes).unwrap_err());
        assert!(
            err.contains("rules.xml") && err.contains("Pirate") && err.contains("@root/A"),
            "{spec}: {err}"
        );
    }
}

fn recovered_behaviors(file: &str, name: &str, kind: Option<&str>) -> (u16, String) {
    let text = std::fs::read_to_string(format!("data/reference/release-7.0/xml/{file}")).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let o = doc
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("id") == Some(name))
        .unwrap();
    let id = super::super::kind(o.attribute("type").unwrap()).unwrap();
    let body = o
        .descendants()
        .filter(|n| n.has_tag_name("Behavior") && kind.is_none_or(|k| n.text() == Some(k)))
        .map(|n| &text[n.range()])
        .collect::<String>();
    (id, body)
}
fn emission_inputs(
    name: &str,
    id: u16,
    body: &str,
    dependencies: &str,
) -> BTreeMap<String, Vec<u8>> {
    let mut bytes = inputs(
        object(name, id, body)
            + dependencies
            + &fixture_natives(&[
                (2, "projectile"),
                (3, "projectile"),
                (0x11, "projectile"),
                (0x5d, "projectile"),
                (0x5f, "projectile"),
                (0x62, "projectile"),
                (0x63, "projectile"),
                (0xa4, "projectile"),
            ]),
    );
    bytes.insert(
        MANIFEST.into(),
        format!("<Game schema=\"1\"><Include file=\"rules.xml\"/><Root object=\"{name}\"/></Game>")
            .into_bytes(),
    );
    bytes
}
fn same_selected_properties(game: &GameData) {
    assert_eq!(
        serde_json::to_value(&game.properties).unwrap(),
        serde_json::to_value(&CONTENT.objects).unwrap()
    );
}
#[test]
fn xml_compile_emission_services_recovered_terminal_explode_keeps_death_births() {
    let (id, body) = recovered_behaviors("objects.xml", "Spider Egg Sac", None);
    let (child, child_body) =
        recovered_behaviors("objects.xml", "Green Den Spider Hatchling", None);
    assert_eq!(id, 0x21b);
    assert!(CONTENT.objects[&id].projectiles.is_empty());
    let bytes = emission_inputs(
        "Spider Egg Sac",
        id,
        &body,
        &object("Green Den Spider Hatchling", child, &child_body),
    );
    let game = production_syntax_load(bytes.clone()).unwrap();
    same_selected_properties(&game);
    let b = &game.definitions[&id].states[0].behaviors;
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].kind, BehaviorKind::ReleaseSpawnAtDeath);
    assert_matches!(&b[0].fields[&Field::ChildId],Value::Objects(ids) if ids==&[child]);
    assert_eq!(b[1].emission, Some(Emission::Terminal));
    assert!(!b[1].fields.contains_key(&Field::ProjectileId));
    assert!(!b[1].fields.contains_key(&Field::ArcDegrees));
    assert_matches!(b[1].fields[&Field::NumShots], Value::Integer(0));
    assert_matches!(b[1].fields[&Field::DropLoot], Value::Bool(false));
    assert!(game.reachable.contains(&child));
    let changed = production_syntax_load(emission_inputs(
        "Spider Egg Sac",
        id,
        &body.replace("range=\"2.0\"", "range=\"3.0\""),
        &object("Green Den Spider Hatchling", child, &child_body),
    ))
    .unwrap();
    assert_ne!(game.fingerprint, changed.fingerprint);
    for (body, expected) in [
        (
            "<Behavior numShots=\"1\">Explode</Behavior>",
            "missing required ProjectileId",
        ),
        (
            "<Behavior numShots=\"1\" projectileId=\"0\">Explode</Behavior>",
            "unresolved selected projectile",
        ),
        (
            "<Behavior numShots=\"0\" projectileId=\"typo\">Explode</Behavior>",
            "invalid nonnegative integer",
        ),
        (
            "<Behavior numShots=\"0\" arcDegrees=\"90\">Explode</Behavior>",
            "terminal Explode cannot",
        ),
    ] {
        production_syntax_error(emission_inputs("Spider Egg Sac", id, body, ""), expected);
    }
    production_syntax_error(
        simple("<Behavior numShots=\"0\" projectileId=\"0\">Explode</Behavior>"),
        "terminal Explode cannot",
    );
    let projectile = production_syntax_load(simple(
        "<Behavior numShots=\"4\" projectileId=\"0\">Explode</Behavior>",
    ))
    .unwrap();
    assert_eq!(
        projectile.definitions[&0x600].states[0].behaviors[0].emission,
        Some(Emission::Projectile(0))
    );
}
#[test]
fn xml_compile_emission_services_recovered_child_shoot_has_no_projectile_default() {
    for (file, name, child_name, expected_counts) in [
        (
            "dungeons/forbiddenJungle.xml",
            "Great Snake Egg",
            "Great Temple Snake",
            vec![1, 2],
        ),
        (
            "dungeons/oryxWineCellar.xml",
            "Monstrosity of Oryx",
            "Monstrosity Scarab",
            vec![1],
        ),
    ] {
        let (id, body) = recovered_behaviors(file, name, Some("Shoot"));
        assert!(CONTENT.objects[&id].projectiles.is_empty());
        let child = CONTENT
            .objects
            .values()
            .find(|o| o.name == child_name)
            .unwrap()
            .kind;
        // Child behavior is isolated here; actual production closure tests its complete recovered rules.
        let dependency = object(child_name, child, "<Behavior>Wander</Behavior>");
        let bytes = emission_inputs(name, id, &body, &dependency);
        let game = production_syntax_load(bytes).unwrap();
        same_selected_properties(&game);
        assert!(game.reachable.contains(&child));
        let b = &game.definitions[&id].states[0].behaviors;
        assert_eq!(
            b.iter()
                .map(|b| if let Value::Integer(n) = b.fields[&Field::NumShots] {
                    n
                } else {
                    panic!("shots")
                })
                .collect::<Vec<_>>(),
            expected_counts
        );
        for b in b {
            assert_eq!(b.emission, Some(Emission::Child(child)));
            assert!(!b.fields.contains_key(&Field::ProjectileId));
        }
        for (from, to, expected) in [
            (child_name, "Unknown Child", "unresolved selected object"),
            (
                "<Behavior",
                "<Behavior projectileId=\"0\"",
                "unresolved selected projectile",
            ),
        ] {
            production_syntax_error(
                emission_inputs(name, id, &body.replace(from, to), &dependency),
                expected,
            );
        }
        let missing = format!(
            "{:#}",
            production_syntax_load(emission_inputs(name, id, &body, "")).unwrap_err()
        );
        assert!(
            missing.contains(child_name)
                && missing.contains("has no explicit admission definition/disposition"),
            "{missing}"
        );
        let changed = production_syntax_load(emission_inputs(
            name,
            id,
            &body.replace(child_name, "Pirate"),
            &object("Pirate", 0x600, "<Behavior>Wander</Behavior>"),
        ))
        .unwrap();
        assert_ne!(game.fingerprint, changed.fingerprint);
        assert!(changed.definitions[&id].dependencies.contains(&0x600));
    }
    for (body, expected) in [
        (
            "<Behavior childId=\"Pirate\" projectileId=\"0\">Shoot</Behavior>",
            "cannot emit both",
        ),
        (
            "<Behavior childId=\"Pirate\" numShots=\"0\">Shoot</Behavior>",
            "requires positive numShots",
        ),
        (
            "<Behavior childId=\"Pirate,Piratess\">Shoot</Behavior>",
            "requires exactly one",
        ),
        (
            "<Behavior childId=\"Pirate\" projectileId=\"oops\">Shoot</Behavior>",
            "invalid nonnegative integer",
        ),
    ] {
        production_syntax_error(simple(body), expected);
    }
}
#[test]
fn xml_compile_emission_services_recovered_aura_lists_keep_one_local_and_order() {
    for name in ["Hexxer", "DeHexxer"] {
        let (id, body) = recovered_behaviors("dungeons/madLab.xml", name, None);
        let game = production_syntax_load(emission_inputs(name, id, &body, "")).unwrap();
        same_selected_properties(&game);
        let b = &game.definitions[&id].states[0].behaviors;
        assert_eq!(b.len(), if name == "Hexxer" { 2 } else { 1 });
        let aura = b.last().unwrap();
        assert_matches!(&aura.fields[&Field::Effect],Value::Conditions(v) if matches!(v.as_slice(),[Condition::Selected(7),Condition::Selected(15),Condition::Selected(28)]));
        assert_matches!(aura.fields[&Field::Cooldown], Value::Millis(200));
        assert_eq!(
            b.iter().map(|b| b.local).collect::<BTreeSet<_>>().len(),
            b.len()
        );
        if name == "Hexxer" {
            assert_matches!(&b[0].fields[&Field::Effect],Value::Conditions(v) if matches!(v.as_slice(),[Condition::Selected(13)]));
        }
        let changed = production_syntax_load(emission_inputs(
            name,
            id,
            &body.replace("Stunned,Speedy,Hexed", "Hexed,Speedy,Stunned"),
            "",
        ))
        .unwrap();
        assert_ne!(game.fingerprint, changed.fingerprint);
        for list in [
            "",
            ",Stunned",
            "Stunned,",
            "Stunned,,Hexed",
            "Stunned|Speedy",
            "Stunned,Unknown",
        ] {
            let err = format!(
                "{:#}",
                production_syntax_load(emission_inputs(
                    name,
                    id,
                    &body.replace("Stunned,Speedy,Hexed", list),
                    ""
                ))
                .unwrap_err()
            );
            assert!(err.contains("rules.xml") && err.contains(name), "{err}");
        }
    }
    let scalar = production_syntax_load(simple(
        "<Behavior effect=\"Speedy\">ConditionEffect</Behavior>",
    ))
    .unwrap();
    assert_matches!(
        scalar.definitions[&0x600].states[0].behaviors[0].fields[&Field::Effect],
        Value::Condition(Condition::Selected(15))
    );
    production_syntax_error(
        simple("<Behavior effect=\"Stunned,Speedy\">ConditionEffect</Behavior>"),
        "multiple conditions",
    );
    production_syntax_error(
        simple("<Behavior effect=\"Stunned,Speedy\">GrenadeToss</Behavior>"),
        "multiple conditions",
    );
    let server = production_syntax_load(simple(
        "<Behavior effect=\"Stun Immune,Speedy\">ConditionEffectAura</Behavior>",
    ))
    .unwrap();
    assert_matches!(&server.definitions[&0x600].states[0].behaviors[0].fields[&Field::Effect],Value::Conditions(v) if matches!(v.as_slice(),[Condition::StunImmune,Condition::Selected(15)]));
}
fn service_inputs(service: &str) -> BTreeMap<String, Vec<u8>> {
    let mut bytes = map_inputs();
    bytes.insert(MANIFEST.into(),format!("<Game schema=\"1\"><Include file=\"rules.xml\"/><Geometry file=\"geometry.xml\"/><Root object=\"Pirate\"/><World id=\"contract\" map=\"Contract Map\">{service}</World></Game>").into_bytes());
    bytes
}
fn service_xml(tag: &str, value: &str) -> String {
    format!(
        "<{tag} provenance=\"recovered\" origin=\"data/reference/release-7.0/xml/dungeons.xml:source-service-fixture\" reason=\"isolated recovered service value\">{value}</{tag}>"
    )
}
#[test]
fn xml_compile_emission_services_recovered_world_services_are_typed_and_fingerprinted() {
    // Client Background.as factory at pin d15f321edcce0ef890001890c6991c25a1b0f718 consumes0/1/2, not sentinel3.
    // Source SHA256 eefb056066cf28664719cd936ff07e6be8c86ab0bd44a257852c02dab7e615b3 (see handoff reference receipt).
    let text = std::fs::read_to_string("data/reference/release-7.0/xml/dungeons.xml").unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let mut values: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for n in doc.descendants().filter(|n| {
        n.is_element()
            && [
                "Visibility",
                "Difficulty",
                "Background",
                "UsesBreath",
                "HelloNotification",
            ]
            .contains(&n.tag_name().name())
    }) {
        values
            .entry(n.tag_name().name().into())
            .or_default()
            .insert(n.text().unwrap().trim().into());
    }
    assert_eq!(
        values["Visibility"],
        BTreeSet::from(["Full".into(), "Path".into(), "LineOfSight".into()])
    );
    assert_eq!(
        values["Difficulty"],
        BTreeSet::from_iter((0..=5).map(|n| n.to_string()))
    );
    assert!(values["UsesBreath"].contains("7.25"));
    assert_eq!(
        values["HelloNotification"],
        BTreeSet::from(["showKeyUI".into()])
    );
    let omitted = production_syntax_load(service_inputs("")).unwrap();
    assert!(omitted.worlds[0].services.difficulty.is_none());
    assert!(omitted.worlds[0].services.visibility.is_none());
    assert!(omitted.worlds[0].services.background.is_none());
    assert!(omitted.worlds[0].services.uses_breath.is_none());
    assert!(omitted.worlds[0].services.hello_notification.is_none());
    for (tag, vals) in values {
        for value in vals {
            let game = production_syntax_load(service_inputs(&service_xml(&tag, &value))).unwrap();
            same_selected_properties(&game);
            assert_ne!(omitted.fingerprint, game.fingerprint);
            let s = &game.worlds[0].services;
            match tag.as_str() {
                "Visibility" => assert_eq!(
                    serde_json::to_value(&s.visibility.as_ref().unwrap().value).unwrap(),
                    value
                ),
                "Difficulty" => assert_eq!(
                    s.difficulty.as_ref().unwrap().value,
                    value.parse::<u8>().unwrap()
                ),
                "Background" => assert_eq!(
                    s.background.as_ref().unwrap().value.clone() as i32,
                    value.parse::<i32>().unwrap()
                ),
                "UsesBreath" => assert_eq!(s.uses_breath.as_ref().unwrap().value, 7.25),
                "HelloNotification" => assert_matches!(
                    s.hello_notification.as_ref().unwrap().value,
                    HelloNotification::ShowKeyUi
                ),
                _ => unreachable!(),
            }
            assert!(game.inputs.contains_key(MANIFEST));
        }
    }
    let xml = service_xml("Visibility", "Full")
        + &service_xml("Difficulty", "0")
        + &service_xml("Background", "0")
        + &service_xml("UsesBreath", "7.25")
        + &service_xml("HelloNotification", "showKeyUI");
    let game = production_syntax_load(service_inputs(&xml)).unwrap();
    let s = game.worlds[0].services.uses_breath.as_ref().unwrap();
    assert_eq!(s.source.path, "World/contract/UsesBreath");
    assert_eq!(s.source.file, MANIFEST);
    assert_matches!(s.provenance, Provenance::Recovered);
    assert!(!s.origin.is_empty() && !s.reason.is_empty());
    let changed = production_syntax_load(service_inputs(&xml.replace("7.25", "7.5"))).unwrap();
    assert_ne!(game.fingerprint, changed.fingerprint);
    for (tag, value) in [
        ("Visibility", "Fog"),
        ("Visibility", ""),
        ("Difficulty", "6"),
        ("Difficulty", "-1"),
        ("Difficulty", "1.5"),
        ("Background", "3"),
        ("Background", "2147483648"),
        ("Background", "-1"),
        ("UsesBreath", "0"),
        ("UsesBreath", "NaN"),
        ("UsesBreath", "true"),
        ("UsesBreath", "-7.25"),
        ("HelloNotification", "showOtherUI"),
        ("Parameter", "5"),
    ] {
        let err = format!(
            "{:#}",
            production_syntax_load(service_inputs(&service_xml(tag, value))).unwrap_err()
        );
        assert!(
            err.contains("game.xml") && err.contains("world=contract"),
            "{err}"
        );
    }
    for tag in [
        "Visibility",
        "Difficulty",
        "Background",
        "UsesBreath",
        "HelloNotification",
    ] {
        let value = match tag {
            "Visibility" => "Full",
            "Difficulty" => "1",
            "Background" => "0",
            "UsesBreath" => "7.25",
            _ => "showKeyUI",
        };
        let xml = service_xml(tag, value);
        let err = format!(
            "{:#}",
            production_syntax_load(service_inputs(&(xml.clone() + &xml))).unwrap_err()
        );
        assert!(err.contains("duplicate world service"), "{err}");
        for bad in [
            xml.replace("provenance=\"recovered\"", "provenance=\"unknown\""),
            xml.replace(
                " origin=\"data/reference/release-7.0/xml/dungeons.xml:source-service-fixture\"",
                "",
            ),
            xml.replace("reason=\"isolated recovered service value\"", "reason=\"\""),
            xml.replace("provenance=", "world=\"other\" provenance="),
        ] {
            assert!(production_syntax_load(service_inputs(&bad)).is_err());
        }
    }
    let mut bad = service_inputs(&xml);
    bad.insert(
        MANIFEST.into(),
        String::from_utf8(bad[MANIFEST].clone())
            .unwrap()
            .replace("map=\"Contract Map\"", "map=\"Unknown World Map\"")
            .into_bytes(),
    );
    assert!(
        format!("{:#}", production_syntax_load(bad).unwrap_err()).contains("unresolved world map")
    );
}

#[test]
fn xml_compile_emission_services_recovered_class_special_pool_keeps_source_category() {
    let text = std::fs::read_to_string("data/reference/release-7.0/xml/realm/low.xml").unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let elf = doc
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("id") == Some("Elf Wizard"))
        .unwrap();
    let row = elf
        .children()
        .find(|n| n.has_tag_name("SoulboundLoot") && n.text() == Some("ClassSpecial"))
        .unwrap();
    assert_eq!(row.attribute("tier"), Some("1"));
    assert_eq!(row.attribute("prob"), Some(".39"));
    let body = "<Behavior>Wander</Behavior>".to_string() + &text[row.range()];
    let dependencies = fixture_natives(LOOT_FIXTURE_NATIVES);
    let game =
        production_syntax_load(emission_inputs("Elf Wizard", 0x609, &body, &dependencies)).unwrap();
    same_selected_properties(&game);
    let row = &game.definitions[&0x609].loot[0];
    assert_matches!(row.category, Some(LootCategory::ClassSpecial));
    assert_eq!(row.tier, Some(1));
    assert_eq!(row.probability, 0.39);
    let expected = CONTENT
        .objects
        .values()
        .filter(|o| {
            o.tier == Some(1)
                && [4, 5, 11, 12, 13, 15, 16, 18, 19, 20, 21, 22, 23].contains(&o.slot_type)
        })
        .map(|o| o.kind)
        .collect::<Vec<_>>();
    assert!(!expected.is_empty());
    assert_eq!(row.pool, expected);
    let ability = production_syntax_load(emission_inputs(
        "Elf Wizard",
        0x609,
        &body.replace("ClassSpecial", "Ability"),
        &dependencies,
    ))
    .unwrap();
    assert_eq!(ability.definitions[&0x609].loot[0].pool, expected);
    assert_matches!(
        ability.definitions[&0x609].loot[0].category,
        Some(LootCategory::Ability)
    );
    assert_ne!(game.fingerprint, ability.fingerprint);
    let tutorial =
        std::fs::read_to_string("data/reference/release-7.0/xml/tutorial/tutorial_objects.xml")
            .unwrap();
    let tutorial = roxmltree::Document::parse(&tutorial).unwrap();
    let bag = tutorial
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("id") == Some("Tutorial Loot Bag"))
        .unwrap();
    assert!(
        bag.children()
            .any(|n| n.has_tag_name("Loot") && n.text().is_none() && n.attributes().len() == 0)
    );
    production_syntax_error(
        emission_inputs(
            "Tutorial Loot Bag",
            0x806,
            "<Loot/><Behavior>RemoveOnEmpty</Behavior>",
            "",
        ),
        "empty Loot row",
    );
    for (from, to, expected) in [
        ("ClassSpecial", "ClassSpecialTypo", "unknown tier pool"),
        ("tier=\"1\"", "tier=\"255\"", "empty selected tier pool"),
    ] {
        production_syntax_error(
            emission_inputs("Elf Wizard", 0x609, &body.replace(from, to), ""),
            expected,
        );
    }
}

fn native_declaration(kind: u16, service: &str) -> String {
    let name = xml_attr(&CONTENT.objects[&kind].name);
    format!(
        r#"<Object type="{kind}" id="{name}" disposition="native" nativeService="{service}" provenance="authored" origin="selected native service fixture" reason="compiler admission regression, not historical proof"/>"#
    )
}
fn admission_inputs(declarations: &str, roots: &str) -> BTreeMap<String, Vec<u8>> {
    let mut bytes = raw_inputs(declarations.to_owned());
    bytes.insert(
        MANIFEST.into(),
        format!(r#"<Game schema="1"><Include file="rules.xml"/>{roots}</Game>"#).into_bytes(),
    );
    bytes
}
fn admission_entity_fixture(kind: u16, declarations: &str, map: bool) -> BTreeMap<String, Vec<u8>> {
    let name = xml_attr(&CONTENT.objects[&kind].name);
    let mut bytes = admission_inputs(declarations, &format!(r#"<Root object="{name}"/>"#));
    if map {
        bytes.insert(MANIFEST.into(), br#"<Game schema="1"><Include file="rules.xml"/><Geometry file="geometry.xml"/><World id="admission" map="room"/></Game>"#.to_vec());
        let template = template_xml("room").replace(
            "ground=\"Grass\"",
            &format!("ground=\"Grass\" object=\"{name}\""),
        );
        bytes.insert(
            GEOMETRY.into(),
            format!(r#"<Geometry schema="1">{template}</Geometry>"#).into_bytes(),
        );
    } else {
        let pirate = object(
            "Pirate",
            0x600,
            &format!(r#"<Behavior childId="{name}">ObjectToss</Behavior>"#),
        );
        bytes.insert(
            RULES.into(),
            format!(
                "<Objects>{declarations}{pirate}{}</Objects>",
                native_declaration(6, "projectile")
            )
            .into_bytes(),
        );
        bytes.insert(
            MANIFEST.into(),
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/></Game>"#
                .to_vec(),
        );
    }
    bytes
}

#[test]
fn xml_compile_admission_non_enemy_omissions_never_become_native() {
    for kind in [0x1c7, 0x173, 0xd7d] {
        let name = &CONTENT.objects[&kind].name;
        let bytes = admission_inputs(
            &native_declaration(0x120, "scenery"),
            &format!(r#"<Root object="{}"/>"#, xml_attr(name)),
        );
        let err = format!("{:#}", compile(&CONTENT, bytes).unwrap_err());
        for expected in [
            "game.xml:1",
            "Root",
            name,
            "no explicit admission definition",
        ] {
            assert!(err.contains(expected), "{err}");
        }
        let game = load(admission_inputs(
            &native_declaration(0x120, "scenery"),
            r#"<Root object="Grey Wall"/>"#,
        ));
        for role in [
            AdmissionRole::Scripted,
            AdmissionRole::Native(NativeService::Scenery),
        ] {
            let err = format!(
                "{:#}",
                game.entity_admission(kind, role, "world=partial birth=raw")
                    .unwrap_err()
            );
            assert!(
                err.contains("world=partial birth=raw")
                    && err.contains("missing explicit admission definition"),
                "{err}"
            );
        }
        let mut bytes = admission_inputs(
            &object(name, kind, "<Behavior>Decay</Behavior>"),
            &format!(r#"<Root object="{}"/>"#, xml_attr(name)),
        );
        let game = load(bytes.clone());
        assert_eq!(
            game.entity_admission(
                kind,
                AdmissionRole::Scripted,
                "world=partial birth=scripted"
            )
            .unwrap()
            .kind,
            kind
        );
        bytes.insert(RULES.into(), b"<Objects/>".to_vec());
        error(bytes, "no explicit admission definition");
    }
}

#[test]
fn xml_compile_admission_native_services_and_selected_companion_entitlement() {
    let declarations: String = [
        (0x120, "scenery"),
        (0xc2b, "inventory"),
        (0x1407, "companion"),
        (0x701, "inventory"),
        (0x71a, "portal"),
        (0x1ca, "merchant"),
        (0x500, "container"),
    ]
    .into_iter()
    .map(|(kind, service)| native_declaration(kind, service))
    .collect();
    let roots = r#"<Root object="Grey Wall"/><Root object="Small Firecracker"/><Root object="Undead Lair Key"/><Root object="Merchant"/><Root object="Loot Bag"/>"#;
    let bytes = admission_inputs(&declarations, roots);
    let game = load(bytes.clone());
    assert!(game.property_dependencies[&0xc2b].contains(&0x1407));
    assert!(game.property_dependencies[&0x701].contains(&0x71a));
    for (kind, service) in [
        (0x120, NativeService::Scenery),
        (0x1407, NativeService::Companion),
        (0x71a, NativeService::Portal),
        (0x1ca, NativeService::Merchant),
        (0x500, NativeService::Container),
    ] {
        let d = game
            .entity_admission(
                kind,
                AdmissionRole::Native(service),
                "world=fixture birth=native-service",
            )
            .unwrap();
        assert_eq!(d.native_service, Some(service));
        assert_matches!(d.provenance, Provenance::Authored);
        assert_eq!(d.source.object, CONTENT.objects[&kind].name);
        assert!(
            game.entity_admission(kind, AdmissionRole::Scripted, "world=fixture birth=actor")
                .is_err()
        );
    }
    let mut no_activation = CONTENT.clone();
    no_activation.objects.get_mut(&0xc2b).unwrap().companion = None;
    assert!(
        format!("{:#}", compile(&no_activation, bytes.clone()).unwrap_err())
            .contains("missing selected companion activation entitlement")
    );
    let missing = declarations.replace(&native_declaration(0x1407, "companion"), "");
    error(
        admission_inputs(&missing, roots),
        "objects.xml selected property dependency owner=0xc2b",
    );
    assert_eq!(
        serde_json::to_value(&game.properties).unwrap(),
        serde_json::to_value(&CONTENT.objects).unwrap()
    );
    if let Some(dir) = std::env::var_os("QA_RESULTS_DIR") {
        std::fs::write(Path::new(&dir).join("xml-admission-native-ir.json"),serde_json::to_vec_pretty(&json!({"qualification":"explicit compiler fixtures and per-call lookups; not World ownership/reservation or historical/native acceptance","fingerprint":game.fingerprint,"inputs":game.inputs,"definitions":game.definitions,"reachable":game.reachable,"property_dependencies":game.property_dependencies})).unwrap()).unwrap();
    }
}

#[test]
fn xml_compile_admission_property_roles_and_entity_edge_refusals() {
    let declarations: String = [
        (0x120, "scenery"),
        (0xa22, "inventory"),
        (0x1200, "inventory"),
        (0x300, "player"),
        (0xa14, "inventory"),
        (0xa56, "inventory"),
        (6, "projectile"),
    ]
    .into_iter()
    .map(|(kind, service)| native_declaration(kind, service))
    .collect();
    let roots = r#"<Root object="Health Potion"/><Root object="Large Purple Pinstripe Cloth"/><Root object="Rogue"/>"#;
    let game = load(admission_inputs(&declarations, roots));
    for (kind, service) in [
        (0xa22, NativeService::Inventory),
        (0x1200, NativeService::Inventory),
        (0x300, NativeService::Player),
        (6, NativeService::Projectile),
    ] {
        assert!(
            game.admission(kind, AdmissionRole::Native(service), "property consumer")
                .is_ok()
        );
        let err = format!(
            "{:#}",
            game.entity_admission(
                kind,
                AdmissionRole::Native(service),
                "world=fixture birth=raw"
            )
            .unwrap_err()
        );
        assert!(
            err.contains("not a world entity") && err.contains("world=fixture birth=raw"),
            "{err}"
        );
        for map in [true, false] {
            let mut decl = declarations.replace(&native_declaration(6, "projectile"), "");
            if map {
                decl += &native_declaration(6, "projectile");
            }
            error(
                admission_entity_fixture(kind, &decl, map),
                "not a world entity",
            );
        }
        let mut population = admission_entity_fixture(kind, &declarations, true);
        population.insert(
            GEOMETRY.into(),
            format!(
                r#"<Geometry schema="1">{}</Geometry>"#,
                template_xml("room")
            )
            .into_bytes(),
        );
        population.insert(MANIFEST.into(), format!(r#"<Game schema="1"><Include file="rules.xml"/><Geometry file="geometry.xml"/><World id="admission" map="room"><Population object="{}" min="1" max="1"/></World></Game>"#, xml_attr(&CONTENT.objects[&kind].name)).into_bytes());
        error(population, "world=admission Population");
    }
    assert!(game.property_dependencies[&0x300].contains(&0xa14));
    assert!(game.property_dependencies[&0xa14].contains(&6));
    let missing = declarations.replace(&native_declaration(6, "projectile"), "");
    error(
        admission_inputs(&missing, roots),
        "objects.xml selected property dependency owner=0xa14",
    );
    let loot = object(
        "Pirate",
        0x600,
        "<Behavior>Decay</Behavior><Loot>Large Purple Pinstripe Cloth</Loot>",
    );
    let loot_game = load(admission_inputs(
        &(declarations.clone() + &loot),
        r#"<Root object="Pirate"/>"#,
    ));
    assert!(loot_game.definitions[&0x600].dependencies.contains(&0x1200));
    let no_player_item = declarations.replace(&native_declaration(0xa56, "inventory"), "");
    error(
        admission_inputs(&no_player_item, roots),
        "objects.xml selected property dependency owner=0x300",
    );
    for map in [true, false] {
        let game = load(admission_entity_fixture(
            0x120,
            &native_declaration(0x120, "scenery"),
            map,
        ));
        assert!(game.reachable.contains(&0x120));
    }
}

#[test]
fn xml_compile_admission_native_schema_is_explicit_and_strict() {
    let wall = native_declaration(0x120, "scenery");
    let roots = r#"<Root object="Grey Wall"/>"#;
    for (from, to, expected) in [
        (" nativeService=\"scenery\"", "", "missing nativeService"),
        (
            "nativeService=\"scenery\"",
            "nativeService=\"unknown\"",
            "unknown nativeService",
        ),
        (
            "nativeService=\"scenery\"",
            "nativeService=\"inventory\"",
            "incompatible with selected",
        ),
        (
            "disposition=\"native\"",
            "disposition=\"passive\"",
            "nativeService requires native disposition",
        ),
        (
            "id=\"Grey Wall\"",
            "id=\"White Fountain\"",
            "selected identity mismatch",
        ),
        (" provenance=\"authored\"", "", "missing provenance"),
        (
            "origin=\"selected native service fixture\"",
            "origin=\"\"",
            "missing origin",
        ),
        (
            "/>",
            "><Loot>Health Potion</Loot></Object>",
            "native definition cannot contain server payload",
        ),
        (
            "/>",
            "><SpawnProb>1</SpawnProb></Object>",
            "native definition cannot contain server payload",
        ),
        (
            "/>",
            "><Behavior>Decay</Behavior></Object>",
            "native definition cannot contain server payload",
        ),
        (
            "/>",
            " surprise=\"1\"/>",
            "unknown Object attribute surprise",
        ),
    ] {
        error(admission_inputs(&wall.replace(from, to), roots), expected);
    }
    error(
        admission_inputs(
            &(wall.clone() + &object("Grey Wall", 0x120, "<Behavior>Decay</Behavior>")),
            roots,
        ),
        "duplicate selected definition",
    );
    for kind in [0xa22, 0x1200, 0x300, 6] {
        error(
            admission_inputs(
                &object(
                    &CONTENT.objects[&kind].name,
                    kind,
                    "<Behavior>Decay</Behavior>",
                ),
                &format!(
                    r#"<Root object="{}"/>"#,
                    xml_attr(&CONTENT.objects[&kind].name)
                ),
            ),
            "property-only class",
        );
    }
    for (kind, service) in [
        (0x600, "scenery"),
        (0x1c7, "companion"),
        (0xd7d, "companion"),
        (0x1407, "scenery"),
        (0xa22, "portal"),
        (0x1200, "merchant"),
    ] {
        error(
            admission_inputs(
                &native_declaration(kind, service),
                &format!(
                    r#"<Root object="{}"/>"#,
                    xml_attr(&CONTENT.objects[&kind].name)
                ),
            ),
            "incompatible with selected",
        );
    }
}

#[test]
fn xml_compile_admission_owning_data_lookup_cannot_reuse_stale_role() {
    let roots = r#"<Root object="Grey Wall"/>"#;
    let native = load(admission_inputs(
        &native_declaration(0x120, "scenery"),
        roots,
    ));
    let scripted = load(admission_inputs(
        &object("Grey Wall", 0x120, "<Behavior>Decay</Behavior>"),
        roots,
    ));
    let stale = native.definitions[&0x120].admission_role().unwrap();
    assert!(
        native
            .entity_admission(0x120, stale, "world=first birth=map")
            .is_ok()
    );
    let err = format!(
        "{:#}",
        scripted
            .entity_admission(0x120, stale, "world=second birth=map")
            .unwrap_err()
    );
    for expected in [
        "world=second birth=map",
        "kind=0x120",
        "role mismatch",
        "rules.xml:1",
        "object=Grey Wall",
    ] {
        assert!(err.contains(expected), "{err}");
    }
    assert!(
        scripted
            .entity_admission(0x120, AdmissionRole::Scripted, "world=second birth=map")
            .is_ok()
    );
    assert!(native.admission(0xffff, stale, "unknown birth").is_err());
    assert_ne!(native.fingerprint, scripted.fingerprint);
    let mut contradictory = native.clone();
    contradictory
        .definitions
        .get_mut(&0x120)
        .unwrap()
        .native_service = None;
    assert!(
        format!(
            "{:#}",
            contradictory
                .admission(0x120, stale, "corrupt IR")
                .unwrap_err()
        )
        .contains("contradictory admission")
    );
}

#[test]
fn xml_compile_admission_selected_successor_closure_is_not_an_entity_birth() {
    let declarations: String = [
        (0xc34, "inventory"),
        (0xc35, "inventory"),
        (0xc36, "inventory"),
        (0xc37, "inventory"),
        (0xc38, "inventory"),
        (0x140a, "companion"),
    ]
    .into_iter()
    .map(|(kind, service)| native_declaration(kind, service))
    .collect();
    let name = xml_attr(&CONTENT.objects[&0xc34].name);
    let roots = format!(r#"<Root object="{name}"/>"#);
    let game = load(admission_inputs(&declarations, &roots));
    for kind in 0xc34..0xc38 {
        assert!(game.property_dependencies[&kind].contains(&(kind + 1)));
    }
    assert!(game.reachable.contains(&0x140a));
    let missing = declarations.replace(&native_declaration(0xc36, "inventory"), "");
    error(
        admission_inputs(&missing, &roots),
        "objects.xml selected property dependency owner=0xc35",
    );
}

fn deployment_fixture() -> BTreeMap<String, Vec<u8>> {
    let declarations: String = [
        (0x711, "trap"),
        (0x715, "decoy"),
        (0x1409, "spirit-bomb"),
        (0xab1, "inventory"),
        (0xae3, "inventory"),
        (0xc0d, "inventory"),
        (0xc2a, "inventory"),
    ]
    .into_iter()
    .map(|(kind, service)| native_declaration(kind, service))
    .collect();
    admission_inputs(
        &declarations,
        r#"<Root object="Hunting Trap"/><Root object="Decoy Prism"/><Root object="Prism of Dancing Swords"/><Root object="Ghostly Prism"/>"#,
    )
}

#[test]
fn xml_compile_native_class_roots_map_placement_and_enemy_refusals() {
    let cases = [
        (0x1c9, NativeService::Scenery),
        (0x505, NativeService::Container),
        (0x72f, NativeService::Portal),
        (0x1cb, NativeService::Interaction),
        (0x1cc, NativeService::Interaction),
        (0x1dc, NativeService::Interaction),
        (0x1df, NativeService::Interaction),
        (0x1e5, NativeService::Interaction),
        (0x1e7, NativeService::Interaction),
    ];
    let declarations: String = cases
        .iter()
        .map(|&(kind, service)| {
            native_declaration(
                kind,
                match service {
                    NativeService::Scenery => "scenery",
                    NativeService::Container => "container",
                    NativeService::Portal => "portal",
                    NativeService::Interaction => "interaction",
                    _ => unreachable!(),
                },
            )
        })
        .collect();
    for (kind, service) in cases {
        let roots = format!(
            r#"<Root object="{}"/>"#,
            xml_attr(&CONTENT.objects[&kind].name)
        );
        let game = load(admission_inputs(&declarations, &roots));
        assert_eq!(
            game.entity_admission(
                kind,
                AdmissionRole::Native(service),
                "world=native birth=map"
            )
            .unwrap()
            .kind,
            kind
        );
        assert!(
            game.entity_admission(kind, AdmissionRole::Scripted, "world=native birth=scripted")
                .is_err()
        );
        let mut placement = admission_entity_fixture(kind, &declarations, true);
        if kind == 0x72f {
            let manifest = placement.get_mut(MANIFEST).unwrap();
            let mut xml = String::from_utf8(manifest.clone()).unwrap();
            xml.truncate(xml.len() - "</Game>".len());
            xml.push_str(r#"<PortalRoute object="Guild Hall Portal" destination="guild-hall" earnedExit="false" provenance="authored" origin="compiler fixture" reason="Require a linked Guild Hall service for a physically placed portal."/>"#);
            for level in 1..=4 {
                xml.push_str(&format!(r#"<World id="guild-hall-{level}" map="room"/>"#));
            }
            xml.push_str("</Game>");
            *manifest = xml.into_bytes();
        }
        let map = load(placement);
        assert!(map.reachable.contains(&kind));
        let mut enemy = CONTENT.clone();
        enemy.objects.get_mut(&kind).unwrap().enemy = true;
        assert!(
            format!(
                "{:#}",
                compile(&enemy, admission_inputs(&declarations, &roots)).unwrap_err()
            )
            .contains("incompatible with selected")
        );
    }
}

#[test]
fn xml_compile_admission_deployment_scope_and_live_selected_item_refusals() {
    let game = load(deployment_fixture());
    let item = &game.properties[&0xc2a];
    assert_eq!(item.slot_type, 22);
    assert_eq!(item.activations, 2);
    assert_matches!(item.abilities[0], super::super::Ability::SpiritBomb);
    assert_matches!(item.abilities[1], super::super::Ability::Teleport { .. });
    assert_eq!(
        game.deployment_admission(
            0x1409,
            item,
            0,
            DeploymentKind::SpiritBomb,
            "world=fixture owner=current slot=ability"
        )
        .unwrap()
        .kind,
        0x1409
    );
    for (target, index, kind) in [
        (0x711, 0, DeploymentKind::SpiritBomb),
        (0x1409, 1, DeploymentKind::SpiritBomb),
        (0x1409, 9, DeploymentKind::SpiritBomb),
        (0x1409, 0, DeploymentKind::Decoy),
    ] {
        let err = format!(
            "{:#}",
            game.deployment_admission(
                target,
                item,
                index,
                kind,
                "world=fixture birth=invalid ability"
            )
            .unwrap_err()
        );
        assert!(
            err.contains("world=fixture birth=invalid ability") && err.contains("mismatch"),
            "{err}"
        );
    }
    for index in [0, 1] {
        game.deployment_admission(
            0x715,
            &game.properties[&0xc0d],
            index,
            DeploymentKind::Decoy,
            "double Decoy composite",
        )
        .unwrap();
    }
    let forged = item.clone();
    assert!(
        format!(
            "{:#}",
            game.deployment_admission(
                0x1409,
                &forged,
                0,
                DeploymentKind::SpiritBomb,
                "forged known item"
            )
            .unwrap_err()
        )
        .contains("not the owning GameData selected item")
    );
    let other = load(deployment_fixture());
    assert!(
        game.deployment_admission(
            0x1409,
            &other.properties[&0xc2a],
            0,
            DeploymentKind::SpiritBomb,
            "cross-catalog"
        )
        .is_err()
    );
    let scenery = load(admission_inputs(
        &native_declaration(0x120, "scenery"),
        r#"<Root object="Grey Wall"/>"#,
    ));
    assert!(
        scenery
            .deployment_binding(DeploymentKind::Trap, "missing XML binding")
            .is_err()
    );
    assert!(
        scenery
            .deployment_admission(
                0x1409,
                &scenery.properties[&0xc2a],
                0,
                DeploymentKind::SpiritBomb,
                "undeclared inventory item"
            )
            .is_err()
    );
    for (target, service) in [(0x711, "trap"), (0x715, "decoy"), (0x1409, "spirit-bomb")] {
        let roots = format!(
            r#"<Root object="{}"/>"#,
            xml_attr(&CONTENT.objects[&target].name)
        );
        let descriptor = load(admission_inputs(
            &native_declaration(target, service),
            &roots,
        ));
        let role = descriptor.definitions[&target].admission_role().unwrap();
        assert!(
            descriptor
                .admission(target, role, "descriptor only")
                .is_ok()
        );
        assert!(
            descriptor
                .entity_admission(target, role, "unscoped native birth")
                .is_err()
        );
        for map in [true, false] {
            error(
                admission_entity_fixture(target, &native_declaration(target, service), map),
                "not a world entity",
            );
        }
        let mut population =
            admission_entity_fixture(target, &native_declaration(target, service), true);
        population.insert(
            GEOMETRY.into(),
            format!(
                r#"<Geometry schema="1">{}</Geometry>"#,
                template_xml("room")
            )
            .into_bytes(),
        );
        population.insert(MANIFEST.into(),format!(r#"<Game schema="1"><Include file="rules.xml"/><Geometry file="geometry.xml"/><World id="scoped" map="room"><Population object="{}" min="1" max="1"/></World></Game>"#,xml_attr(&CONTENT.objects[&target].name)).into_bytes());
        error(population, "world=scoped Population");
    }
}

#[test]
fn xml_compile_admission_deployment_binding_is_xml_authority_not_rust_identity() {
    let mut bytes = deployment_fixture();
    let original = String::from_utf8(bytes[RULES].clone()).unwrap();
    let changed = original.replace(
        &native_declaration(0x711, "trap"),
        &native_declaration(0x1407, "trap"),
    );
    bytes.insert(RULES.into(), changed.into_bytes());
    let game = load(bytes);
    assert_eq!(
        game.deployment_binding(DeploymentKind::Trap, "authored binding")
            .unwrap()
            .kind,
        0x1407
    );
    assert!(game.property_dependencies[&0xab1].contains(&0x1407));
    assert!(!game.property_dependencies[&0xab1].contains(&0x711));
    game.deployment_admission(
        0x1407,
        &game.properties[&0xab1],
        0,
        DeploymentKind::Trap,
        "new XML target",
    )
    .unwrap();
    assert!(
        game.deployment_admission(
            0x711,
            &game.properties[&0xab1],
            0,
            DeploymentKind::Trap,
            "stale target"
        )
        .is_err()
    );
    let old = load(deployment_fixture());
    assert!(
        game.deployment_admission(
            0x1407,
            &old.properties[&0xab1],
            0,
            DeploymentKind::Trap,
            "stale item catalog"
        )
        .is_err()
    );
    for (from, to, expected) in [
        (
            native_declaration(0x711, "trap"),
            String::new(),
            "missing Trap deployment binding",
        ),
        (
            native_declaration(0x711, "trap"),
            native_declaration(0x711, "trap") + &native_declaration(0x1407, "trap"),
            "ambiguous Trap deployment bindings",
        ),
        (
            native_declaration(0x1409, "spirit-bomb"),
            native_declaration(0x1407, "spirit-bomb"),
            "selected SpiritBomb Pet target disagrees",
        ),
        (
            native_declaration(0xab1, "inventory"),
            String::new(),
            "no explicit admission definition",
        ),
        (
            "nativeService=\"trap\"".into(),
            "nativeService=\"companion\"".into(),
            "incompatible with selected",
        ),
        (
            "nativeService=\"decoy\"".into(),
            "nativeService=\"companion\"".into(),
            "incompatible with selected",
        ),
        (
            "nativeService=\"spirit-bomb\"".into(),
            "nativeService=\"companion\"".into(),
            "incompatible with selected",
        ),
    ] {
        let mut bytes = deployment_fixture();
        bytes.insert(RULES.into(), original.replace(&from, &to).into_bytes());
        error(bytes, expected);
    }
}

#[test]
fn xml_compile_admission_extension_schema_entitlements_and_no_spurious_xml_trap_birth() {
    for (kind, service) in [
        (0x1c9, "interaction"),
        (0x1cb, "scenery"),
        (0x505, "merchant"),
        (0x72f, "scenery"),
        (0x711, "interaction"),
        (0x600, "trap"),
        (0x120, "trap"),
        (0xa22, "decoy"),
    ] {
        error(
            admission_inputs(
                &native_declaration(kind, service),
                &format!(
                    r#"<Root object="{}"/>"#,
                    xml_attr(&CONTENT.objects[&kind].name)
                ),
            ),
            "incompatible with selected",
        );
    }
    let mut bytes = deployment_fixture();
    let rules = String::from_utf8(bytes[RULES].clone()).unwrap();
    let native = native_declaration(0x711, "trap");
    bytes.insert(
        RULES.into(),
        rules
            .replace(
                &native,
                &native.replace("provenance=\"authored\"", "provenance=\"recovered\""),
            )
            .into_bytes(),
    );
    error(
        bytes,
        "native deployment binding must be qualified authored/corrected",
    );
    for (from, to, expected) in [
        (
            "nativeService=\"trap\"",
            "nativeService=\"Trap\"",
            "unknown nativeService",
        ),
        (" nativeService=\"trap\"", "", "missing nativeService"),
        ("id=\"Trap\"", "id=\"Decoy\"", "selected identity mismatch"),
    ] {
        let mut bytes = deployment_fixture();
        bytes.insert(RULES.into(), rules.replace(from, to).into_bytes());
        error(bytes, expected);
    }
    let mut content = CONTENT.clone();
    content.objects.get_mut(&0x711).unwrap().enemy = true;
    assert!(
        format!("{:#}", compile(&content, deployment_fixture()).unwrap_err())
            .contains("incompatible with selected")
    );
    let mut content = CONTENT.clone();
    content
        .objects
        .get_mut(&0xc2a)
        .unwrap()
        .abilities
        .swap(0, 1);
    assert!(
        format!("{:#}", compile(&content, deployment_fixture()).unwrap_err())
            .contains("selected deployment activation disagrees")
    );
    let rules = object("Pirate", 0x600, "<Behavior>TrapTrigger</Behavior>")
        + &native_declaration(6, "projectile");
    let game = load(admission_inputs(&rules, r#"<Root object="Pirate"/>"#));
    assert!(game.deployment_activations.is_empty());
    assert!(!game.reachable.contains(&0x711));
}

#[test]
fn xml_compile_movement_documentary_defaults_units_and_explicit_values() {
    for (kind, expected) in [
        (
            "Shadow",
            json!({"range":{"Number":3.0},"speed":{"Number":3.0},"reshadowRange":{"Number":0.5}}),
        ),
        (
            "Protect",
            json!({"acquireRange":{"Number":15.0},"protectionRange":{"Number":3.0},"reprotectRange":{"Number":3.0},"speed":{"Number":0.8}}),
        ),
        (
            "Orbit",
            json!({"acquireRange":{"Number":15.0},"speed":{"Number":0.8},"radius":{"Number":5.0},"clockwise":{"Bool":false},"speedVariability":{"Number":0.2},"radiusVariability":{"Number":0.5},"radiusWobbleDistance":{"Number":0.0},"radiusWobblePeriod":{"Millis":15000}}),
        ),
        (
            "Follow",
            json!({"acquireRange":{"Number":10.0},"range":{"Number":3.0},"speed":{"Number":1.0},"duration":{"Millis":30000},"cooldown":{"Millis":5000},"predictive":{"Bool":false}}),
        ),
        (
            "Charge",
            json!({"range":{"Number":20.0},"speed":{"Number":3.0},"cooldown":{"Millis":3000}}),
        ),
        (
            "KeepDistance",
            json!({"acquireRange":{"Number":15.0},"distance":{"Number":10.0},"speed":{"Number":1.0}}),
        ),
        (
            "StayBack",
            json!({"distance":{"Number":10.0},"speed":{"Number":1.0}}),
        ),
        (
            "StayCloseToSpawn",
            json!({"range":{"Number":7.0},"speed":{"Number":1.0},"cooldown":{"Millis":3000}}),
        ),
        (
            "Circle",
            json!({"acquireRange":{"Number":15.0},"distance":{"Number":8.0},"speed":{"Number":1.0}}),
        ),
        ("Walk", json!({"speed":{"Number":0.4}})),
        (
            "Wander",
            json!({"speed":{"Number":0.4},"avoidWater":{"Bool":false}}),
        ),
    ] {
        let expected = expected.as_object().unwrap();
        let game = pirate(&format!("<Behavior>{kind}</Behavior>"));
        let b = &game.definitions[&0x600].states[0].behaviors[0];
        for (name, value) in expected {
            let field = Field::parse(name).unwrap();
            assert_eq!(
                serde_json::to_value(&b.fields[&field]).unwrap(),
                *value,
                "{kind}/{name}"
            );
            assert!(b.kind.fields().contains(&field));
        }
        assert!(!b.fields.contains_key(&Field::ProtecteeId));
        assert!(!b.fields.contains_key(&Field::GroundTypes));
        if kind == "Orbit" {
            assert!(!b.fields.contains_key(&Field::ProtectionRange));
            assert!(!b.fields.contains_key(&Field::ReprotectRange));
        }
        for zero in [false, true] {
            let attrs = expected
                .iter()
                .map(|(name, value)| {
                    format!(
                        "{name}=\"{}\"",
                        if value.get("Bool").is_some() {
                            if zero { "false" } else { "true" }
                        } else if zero {
                            "0"
                        } else {
                            "2.5"
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let game = pirate(&format!("<Behavior {attrs}>{kind}</Behavior>"));
            let b = &game.definitions[&0x600].states[0].behaviors[0];
            for (name, value) in expected {
                let expected = if value.get("Bool").is_some() {
                    json!({"Bool":!zero})
                } else if value.get("Millis").is_some() {
                    json!({"Millis":if zero{0}else{2500}})
                } else {
                    json!({"Number":if zero{0.0}else{2.5}})
                };
                assert_eq!(
                    serde_json::to_value(&b.fields[&Field::parse(name).unwrap()]).unwrap(),
                    expected,
                    "explicit {kind}/{name}"
                );
            }
        }
    }
}

#[test]
fn xml_compile_movement_aliases_precede_defaults_and_keep_distinct_meanings() {
    for (kind, legacy, canonical, field, retain) in [
        ("Buzz", "range", "acquireRange", Field::AcquireRange, true),
        (
            "Protect",
            "range",
            "reprotectRange",
            Field::ReprotectRange,
            true,
        ),
        ("Circle", "range", "distance", Field::Distance, false),
        ("KeepDistance", "range", "distance", Field::Distance, false),
        (
            "Follow",
            "distance",
            "acquireRange",
            Field::AcquireRange,
            false,
        ),
    ] {
        for value in ["0", "1", "2.5"] {
            for pair in [false, true] {
                let attrs = format!(
                    "{legacy}=\"{value}\"{}",
                    if pair {
                        format!(" {canonical}=\"{value}\"")
                    } else {
                        String::new()
                    }
                );
                let body =
                    format!("<State id=\"Aliased\"><Behavior {attrs}>{kind}</Behavior></State>");
                let bytes = simple(&body);
                let source = String::from_utf8(bytes[RULES].clone()).unwrap();
                let doc = roxmltree::Document::parse(&source).unwrap();
                let node = doc
                    .descendants()
                    .find(|n| n.has_tag_name("Behavior"))
                    .unwrap();
                let game = load(bytes);
                let b = &game.definitions[&0x600].states[1].behaviors[0];
                assert_matches!(b.fields[&field],Value::Number(n) if n==value.parse::<f64>().unwrap());
                assert_eq!(
                    b.fields.contains_key(&Field::parse(legacy).unwrap()),
                    retain,
                    "{kind}"
                );
                assert_eq!(b.source.file, RULES);
                assert_eq!(b.source.object, "Pirate");
                assert_eq!(b.source.path, "@root/Aliased");
                assert_eq!(b.source.offset, node.range().start);
            }
        }
        let body = format!(
            r#"<DefineEmbed name="Alias"><Behavior {legacy}="1" {canonical}="2">{kind}</Behavior></DefineEmbed><State id="Conflict"><Embed name="Alias"/></State>"#
        );
        let err = format!("{:#}", compile(&CONTENT, simple(&body)).unwrap_err());
        for expected in [
            "conflicting explicit",
            kind,
            "rules.xml:1",
            "object=Pirate",
            "state=@root/Conflict",
            "Alias@",
        ] {
            assert!(err.contains(expected), "{err}");
        }
    }
    for (attrs, acquire, stop) in [
        ("distance=\"8\" range=\"3.5\"", 8., 3.5),
        ("distance=\"6\" range=\"3.5\"", 6., 3.5),
        ("distance=\"8\" range=\"2\"", 8., 2.),
    ] {
        let game = pirate(&format!("<Behavior {attrs}>Follow</Behavior>"));
        let b = &game.definitions[&0x600].states[0].behaviors[0];
        assert_matches!(b.fields[&Field::AcquireRange],Value::Number(n) if n==acquire);
        assert_matches!(b.fields[&Field::Range],Value::Number(n) if n==stop);
        assert!(!b.fields.contains_key(&Field::Distance));
    }
}

#[test]
fn xml_compile_movement_protect_inherits_final_outer_and_orbit_stays_unpaired() {
    for (attrs, outer, stop) in [
        ("protectionRange=\"7\"", 7., 7.),
        ("protectionRange=\"0\"", 0., 0.),
        ("protectionRange=\"7\" range=\"0\"", 7., 0.),
        ("protectionRange=\"7\" reprotectRange=\"2\"", 7., 2.),
        (
            "protectionRange=\"7\" range=\"2\" reprotectRange=\"2\"",
            7.,
            2.,
        ),
    ] {
        let game = pirate(&format!("<Behavior {attrs}>Protect</Behavior>"));
        let b = &game.definitions[&0x600].states[0].behaviors[0];
        assert_matches!(b.fields[&Field::ProtectionRange],Value::Number(n) if n==outer);
        assert_matches!(b.fields[&Field::ReprotectRange],Value::Number(n) if n==stop);
    }
    error(
        simple("<Behavior protectionRange=\"1\" reprotectRange=\"2\">Protect</Behavior>"),
        "reprotectRange exceeds final protectionRange",
    );
    error(
        simple("<Behavior radiusWobbleDistance=\"8\" radiusWobblePeriod=\"0\">Orbit</Behavior>"),
        "positive Orbit wobble requires positive radiusWobblePeriod",
    );
    for attrs in ["", "protectionRange=\"3\" reprotectRange=\"0\""] {
        let game = pirate(&format!("<Behavior {attrs}>Orbit</Behavior>"));
        let b = &game.definitions[&0x600].states[0].behaviors[0];
        assert_eq!(
            b.fields.contains_key(&Field::ProtectionRange),
            !attrs.is_empty()
        );
        assert_eq!(
            b.fields.contains_key(&Field::ReprotectRange),
            !attrs.is_empty()
        );
        assert_matches!(b.fields[&Field::RadiusWobblePeriod], Value::Millis(15000));
    }
}

#[test]
fn xml_compile_movement_actual_selected_aliases_defaults_and_source_frames() {
    let raw = std::fs::read_to_string("data/game/123.5.1/actors.xml").unwrap();
    let before = super::compile::hash(raw.as_bytes());
    let game = GameData::load(Path::new("data"), &CONTENT).unwrap();
    let doc = roxmltree::Document::parse(&raw).unwrap();
    let mut counts = [0; 3];
    let mut rows = Vec::new();
    for o in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Object"))
    {
        let id = super::super::kind(o.attribute("type").unwrap()).unwrap();
        for node in o.descendants().filter(|n| n.has_tag_name("Behavior")) {
            let kind = node.text().unwrap();
            let b = game.definitions[&id]
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
                .find(|b| b.source.offset == node.range().start);
            if matches!(kind, "Circle" | "KeepDistance") && node.attribute("range").is_some() {
                let b = b.unwrap();
                let expected = node.attribute("range").unwrap().parse::<f64>().unwrap();
                assert_matches!(b.fields[&Field::Distance],Value::Number(n) if n==expected);
                assert!(!b.fields.contains_key(&Field::Range));
                let acquire = node
                    .attribute("acquireRange")
                    .map(|s| s.parse::<f64>().unwrap())
                    .unwrap_or(15.);
                assert_matches!(b.fields[&Field::AcquireRange],Value::Number(n) if n==acquire);
                counts[usize::from(kind == "KeepDistance")] += 1;
                rows.push(serde_json::to_value(b).unwrap());
            }
            if id == 0xd53 && kind == "Follow" && node.attribute("distance") == Some("8") {
                let b = b.unwrap();
                assert_matches!(b.fields[&Field::AcquireRange], Value::Number(8.));
                assert_matches!(b.fields[&Field::Range], Value::Number(3.5));
                assert!(!b.fields.contains_key(&Field::Distance));
                assert_matches!(b.fields[&Field::Duration], Value::Millis(2000));
                assert_matches!(b.fields[&Field::Cooldown], Value::Millis(1500));
                counts[2] += 1;
                rows.push(serde_json::to_value(b).unwrap());
            }
        }
    }
    assert_eq!(counts, [6, 15, 1]);
    for (id, kind, field, expected) in [
        (0x606, BehaviorKind::Buzz, Field::AcquireRange, 1.),
        (0x904, BehaviorKind::Protect, Field::ReprotectRange, 0.1),
        (0xd5a, BehaviorKind::Orbit, Field::RadiusWobbleDistance, 8.),
    ] {
        let b = game.definitions[&id]
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .find(|b| b.kind == kind)
            .unwrap();
        assert_matches!(b.fields[&field],Value::Number(n) if n==expected);
        if id == 0x904 {
            assert_matches!(b.fields[&Field::ProtectionRange], Value::Number(3.));
            assert_matches!(b.fields[&Field::AcquireRange], Value::Number(15.));
        }
        if id == 0xd5a {
            assert_matches!(b.fields[&Field::RadiusWobblePeriod], Value::Millis(15000));
        }
        assert_eq!(b.source.file, "game/123.5.1/actors.xml");
        assert_eq!(b.source.object, CONTENT.objects[&id].name);
        rows.push(serde_json::to_value(b).unwrap());
    }
    let walk = game.definitions[&0x1ff]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == BehaviorKind::Walk)
        .unwrap();
    assert_matches!(walk.fields[&Field::Speed], Value::Number(0.4));
    assert_matches!(&walk.fields[&Field::GroundTypes],Value::Grounds(grounds) if grounds.len()==5);
    assert_eq!(game.definitions.len(), 2021);
    assert!(game.reachable.contains(&0xd40));
    assert_eq!(game.reachable.len(), 2007);
    same_selected_properties(&game);
    assert_eq!(
        before,
        super::compile::hash(&std::fs::read("data/game/123.5.1/actors.xml").unwrap())
    );
    if let Some(dir) = std::env::var_os("QA_RESULTS_DIR") {
        std::fs::write(Path::new(&dir).join("xml-compile-movement-source.json"),serde_json::to_vec_pretty(&json!({"qualification":"actual selected GameData metadata, qualified documentary defaults and supervisor-authored legacy aliases; not native/historical acceptance","input_fingerprint":game.fingerprint,"actors_sha256":before,"legacy_counts":counts,"behaviors":rows})).unwrap()).unwrap();
    }
}

#[test]
fn xml_compile_stayabove_qualified_defaults_explicit_zero_types_and_context() {
    let bare = pirate("<Behavior>StayAbove</Behavior>");
    for (attrs, altitude, speed) in [
        ("", 200., 0.5),
        (r#"speed="0""#, 200., 0.),
        (r#"minAltitude="0""#, 0., 0.5),
        (r#"minAltitude="0" speed="0""#, 0., 0.),
        (r#"minAltitude="120" speed="0.8""#, 120., 0.8),
        (r#"minAltitude="70""#, 70., 0.5),
        (r#"minAltitude="200" speed="0.5""#, 200., 0.5),
    ] {
        let bytes = simple(&format!("<Behavior {attrs}>StayAbove</Behavior>"));
        let original = bytes.clone();
        let game = load(bytes);
        let b = &game.definitions[&0x600].states[0].behaviors[0];
        assert_eq!(b.kind, BehaviorKind::StayAbove);
        assert_eq!(b.fields.len(), 2);
        assert_matches!(b.fields[&Field::MinAltitude], Value::Number(n) if n == altitude);
        assert_matches!(b.fields[&Field::Speed], Value::Number(n) if n == speed);
        assert_eq!(b.source.file, RULES);
        assert_eq!(b.source.object, "Pirate");
        assert_eq!(b.source.path, "@root");
        assert_eq!(b.source.embeds.len(), 0);
        assert_eq!(game.inputs[RULES], super::compile::hash(&original[RULES]));
        let source = String::from_utf8(original[RULES].clone()).unwrap();
        let doc = roxmltree::Document::parse(&source).unwrap();
        assert_eq!(
            b.source.offset,
            doc.descendants()
                .find(|n| n.has_tag_name("Behavior"))
                .unwrap()
                .range()
                .start
        );
        if attrs == r#"minAltitude="200" speed="0.5""# {
            assert_eq!(
                serde_json::to_value(&b.fields).unwrap(),
                serde_json::to_value(&bare.definitions[&0x600].states[0].behaviors[0].fields)
                    .unwrap()
            );
            assert_ne!(game.fingerprint, bare.fingerprint);
        }
    }
    for (attrs, diagnostic) in [
        (r#"speed="fast""#, "malformed number"),
        (r#"speed="NaN""#, "nonfinite number"),
        (r#"speed="inf""#, "nonfinite number"),
        (r#"speed="true""#, "malformed number"),
        (r#"speed="-1""#, "negative value"),
        (r#"minAltitude="false""#, "malformed number"),
        (r#"minAltitude="NaN""#, "nonfinite number"),
    ] {
        let body = format!(
            r#"<DefineEmbed name="Altitude"><Behavior {attrs}>StayAbove</Behavior></DefineEmbed><State id="High"><Embed name="Altitude"/></State>"#
        );
        let err = format!("{:#}", compile(&CONTENT, simple(&body)).unwrap_err());
        for expected in [
            diagnostic,
            "rules.xml:1",
            "object=Pirate",
            "state=@root/High",
            "Altitude@",
        ] {
            assert!(err.contains(expected), "{err}");
        }
    }
    error(
        simple(r#"<Behavior range="1">StayAbove</Behavior>"#),
        "attribute range not valid for StayAbove",
    );
}

#[test]
fn xml_compile_stayabove_actual_source_thresholds_speed_and_provenance() {
    let raw = std::fs::read_to_string("data/game/123.5.1/actors.xml").unwrap();
    let before = super::compile::hash(raw.as_bytes());
    let game = GameData::load(Path::new("data"), &CONTENT).unwrap();
    let doc = roxmltree::Document::parse(&raw).unwrap();
    let mut literal_count = 0;
    for o in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Object"))
    {
        let id = super::super::kind(o.attribute("type").unwrap()).unwrap();
        for node in o
            .descendants()
            .filter(|n| n.has_tag_name("Behavior") && n.text().map(str::trim) == Some("StayAbove"))
        {
            assert!(node.attribute("speed").is_none());
            let altitude = node
                .attribute("minAltitude")
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((11. ..=200.).contains(&altitude));
            let b = game.definitions[&id]
                .states
                .iter()
                .flat_map(|s| &s.behaviors)
                .find(|b| {
                    b.kind == BehaviorKind::StayAbove && b.source.offset == node.range().start
                })
                .unwrap();
            assert_matches!(b.fields[&Field::MinAltitude], Value::Number(n) if n == altitude);
            assert_matches!(b.fields[&Field::Speed], Value::Number(0.5));
            assert_eq!(b.source.file, "game/123.5.1/actors.xml");
            assert_eq!(b.source.object, CONTENT.objects[&id].name);
            assert_eq!(b.source.line, doc.text_pos_at(node.range().start).row);
            literal_count += 1;
        }
    }
    assert_eq!(literal_count, 130);
    let rows: Vec<_> = game
        .definitions
        .iter()
        .flat_map(|(id, d)| {
            d.states
                .iter()
                .flat_map(|s| &s.behaviors)
                .filter(|b| b.kind == BehaviorKind::StayAbove)
                .map(move |b| json!({"kind":id,"behavior":b}))
        })
        .collect();
    assert_eq!(rows.len(), 133);
    let definitions: BTreeSet<_> = rows.iter().map(|r| r["kind"].as_u64().unwrap()).collect();
    assert_eq!(definitions.len(), 85);
    let pink = game.definitions[&0x205]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == BehaviorKind::StayAbove)
        .unwrap();
    assert_matches!(pink.fields[&Field::MinAltitude], Value::Number(70.));
    assert_matches!(pink.fields[&Field::Speed], Value::Number(0.5));
    assert_eq!(pink.source.line, 108);
    same_selected_properties(&game);
    assert_eq!(
        before,
        super::compile::hash(&std::fs::read("data/game/123.5.1/actors.xml").unwrap())
    );
    let dir = std::env::var_os("QA_RESULTS_DIR").expect("run via qa.py");
    std::fs::write(Path::new(&dir).join("xml-compile-stayabove-source.json"), serde_json::to_vec_pretty(&json!({"qualification":"normal GameData::load: qualified later-DECA StayAbove Speed0.5 and source thresholds, not elevation/native/historical acceptance", "input_fingerprint":game.fingerprint,"actors_sha256":before,"literal_count":literal_count,"expanded_count":rows.len(),"definitions":definitions.len(),"behaviors":rows})).unwrap()).unwrap();
}
#[test]
fn xml_compile_executor_defaults_are_declared_once_for_permitted_fields() {
    for kind in BehaviorKind::ALL {
        let rows = super::compile::executor_defaults(*kind);
        let fields: BTreeSet<_> = rows.iter().map(|(field, _)| field).collect();
        assert_eq!(
            fields.len(),
            rows.len(),
            "{kind:?} declares a default twice"
        );
        assert!(
            rows.iter().all(|(field, _)| kind.fields().contains(field)),
            "{kind:?} defaults an attribute it cannot carry"
        );
        assert!(
            super::compile::executor_reads(*kind)
                .iter()
                .all(|field| kind.fields().contains(field)),
            "{kind:?} declares a read of an attribute it cannot carry"
        );
    }
}
/// Every unguarded executor read site names a declared field; each read also debug-asserts its kind's declaration.
#[test]
fn xml_compile_executor_reads_declare_every_read_site() {
    let declared: BTreeSet<String> = BehaviorKind::ALL
        .iter()
        .flat_map(|kind| super::compile::executor_reads(*kind))
        .map(|field| format!("{field:?}"))
        .collect();
    let mut files = vec![
        std::path::PathBuf::from("src/behavior.rs"),
        std::path::PathBuf::from("src/world/xml.rs"),
    ];
    for entry in std::fs::read_dir("src/world/xml").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs")
            && !path.to_string_lossy().ends_with("tests.rs")
        {
            files.push(path);
        }
    }
    let mut sites = BTreeSet::new();
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        for (line, text) in source.lines().enumerate() {
            for accessor in ["n(", "ms(", "flag(", "required("] {
                for (at, _) in text.match_indices(accessor) {
                    if text[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let Some((_, rest)) = text[at + accessor.len()..].split_once(", F::") else {
                        continue;
                    };
                    let field: String = rest
                        .chars()
                        .take_while(char::is_ascii_alphanumeric)
                        .collect();
                    let site = format!("{}:{} {accessor}{field}", path.display(), line + 1);
                    assert!(
                        declared.contains(&field),
                        "{site} reads a field no kind declares in executor_reads"
                    );
                    sites.insert(site);
                }
            }
        }
    }
    assert!(sites.len() > 100, "read-site scan found only {sites:?}");
}
/// A declared read that the data neither sets nor defaults refuses GameData load instead of panicking in a world.
#[test]
fn xml_compile_undefaulted_executor_read_refuses_load_with_source_context() {
    let err = format!(
        "{:#}",
        compile(
            &CONTENT,
            simple(r#"<State id="Bad"><Behavior childId="Pirate">Metamorphose</Behavior></State>"#)
        )
        .unwrap_err()
    );
    for context in [
        "Metamorphose executor reads Time, but the behavior neither sets it nor has a compiled default",
        "rules.xml:1",
        "object=Pirate",
        "state=@root/Bad",
    ] {
        assert!(err.contains(context), "{err}");
    }
}
