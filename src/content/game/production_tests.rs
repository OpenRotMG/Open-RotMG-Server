use super::*;
use crate::assert_matches;
use serde_json::json;
use std::sync::LazyLock;

static CONTENT: LazyLock<Content> = LazyLock::new(|| Content::load(Path::new("data")).unwrap());
static GAME: LazyLock<GameData> = LazyLock::new(|| {
    GameData::load(Path::new("data"), &CONTENT)
        .unwrap_or_else(|e| panic!("production XML closure: {e:#}"))
});

#[test]
fn xml_compile_production_closure_and_selected_client_identity() {
    let game = &*GAME;
    assert_eq!(
        serde_json::to_value(&game.properties).unwrap(),
        serde_json::to_value(&CONTENT.objects).unwrap()
    );
    let admission: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/xml_runtime/admission.json"
    ))
    .unwrap();
    let profiles = admission["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 496);
    for id in profiles {
        let id = u16::from_str_radix(id.as_str().unwrap(), 16).unwrap();
        assert!(
            game.definitions.contains_key(&id),
            "missing selected family {id:x}"
        );
    }
    for id in &game.reachable {
        let p = &game.properties[id];
        assert!(
            game.definitions.contains_key(id),
            "undeclared reachable {id:#x} {}",
            p.name
        );
        let d = &game.definitions[id];
        game.admission(
            *id,
            d.admission_role().unwrap(),
            "actual production closure",
        )
        .unwrap();
    }
    let closure = serde_json::to_vec_pretty(&json!({
        "qualification":"actual production GameData::load, not executor/native/historical acceptance",
        "fingerprint":game.fingerprint,"inputs":game.inputs,"roots":game.roots,
        "reachable":game.reachable,"definitions":game.definitions,"maps":game.maps,"worlds":game.worlds,
        "properties":game.properties,"property_dependencies":game.property_dependencies,"deployment_activations":game.deployment_activations
    })).unwrap();
    assert_eq!(
        super::compile::hash(&closure),
        "121b36cc5c516a5e7af67a02a3e117cf6ffb27c1e1bb71b23134bb3637727991"
    );
    assert_eq!(
        game.fingerprint,
        "c45004a8b7475507fc5b8935020a15d168da3b5a45e53db2b36835d534dea54d"
    );
    let compact =
        serde_json::to_vec(&(&game.definitions, &game.symbols, &game.string_lists)).unwrap();
    assert_eq!(
        super::compile::hash(&compact),
        "a4f7c075ceac81ae5c501548f4166df62dbdafcc9515248f76663682bb18ede2"
    );
    let dir = std::env::var("QA_RESULTS_DIR").expect("run via qa.py");
    std::fs::write(Path::new(&dir).join("xml-production-closure.json"), closure).unwrap();
}

#[test]
fn xml_compile_production_archive_bytes_and_property_pins() {
    use sha2::{Digest, Sha256};
    let pins: BTreeMap<String, String> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/xml_runtime/archive-hashes.json"
    ))
    .unwrap();
    assert_eq!(pins.len(), 52);
    for (file, hash) in pins {
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&file).unwrap())),
            hash,
            "archive changed: {file}"
        );
    }
    let selected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/xml_contract/sources.json"
    ))
    .unwrap();
    for file in ["data/objects.xml", "data/grounds.xml"] {
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(file).unwrap())),
            selected["sha256"][file].as_str().unwrap()
        );
    }
}

#[test]
fn xml_compile_production_experimental_passive_alias_and_exit_census() {
    let game = &*GAME;
    assert_eq!(
        game.definitions
            .values()
            .filter(
                |d| d.disposition != Disposition::Native && d.origin.contains("testingObjects.xml")
            )
            .count(),
        33
    );
    for id in (0x91b..=0x926)
        .chain(0x928..=0x92d)
        .chain([0x932])
        .chain(0x936..=0x93f)
        .chain(0x941..=0x944)
    {
        let def = &game.definitions[&id];
        assert!(def.origin.contains("testingObjects.xml"));
        assert_matches!(def.disposition, Disposition::Active);
    }
    for id in [
        0x172, 0x175, 0x176, 0x177, 0x178, 0x179, 0xd70, 0xd71, 0xdd8, 0xde5, 0x181f, 0x182e,
        0x188a, 0x188b, 0xd39,
    ] {
        assert_matches!(game.definitions[&id].disposition, Disposition::Passive);
    }
    for id in 0x1806..=0x1809 {
        assert_matches!(game.definitions[&id].disposition, Disposition::Active);
        assert!(
            game.reachable.contains(&id),
            "guild target lifecycle {id:x}"
        );
    }
    assert_eq!(game.properties[&0x954].name, "Bandit");
    assert_matches!(game.definitions[&0x954].provenance, Provenance::Corrected);
    assert!(game.definitions[&0xd71].loot.len() == 2);
    assert_eq!(
        game.worlds.iter().filter(|w| w.exit_key.is_some()).count(),
        10
    );
}

#[test]
fn xml_compile_production_geometry_retains_all_layouts_and_source_stamps() {
    use super::geometry::{Geometry, SeedPolicy};
    let game = &*GAME;
    let realm = game.maps.iter().find(|m| m.name == "realm").unwrap();
    let Geometry::Pool(pool) = &realm.geometry else {
        panic!("realm must retain a pool")
    };
    assert_eq!(pool.entries.len(), 16);
    assert_eq!(
        pool.entries
            .iter()
            .filter(|i| matches!(game.maps[**i as usize].geometry, Geometry::Encoded(_)))
            .count(),
        3
    );
    assert_eq!(
        pool.entries
            .iter()
            .filter(|i| matches!(
                game.maps[**i as usize].geometry,
                Geometry::Reconstruction(_)
            ))
            .count(),
        13
    );
    for id in [0x1e15, 0x1e16, 0xdc4, 0xe28, 0xd65, 0xe3c, 0xe3d] {
        assert!(
            game.reachable.contains(&id),
            "existing concrete dependency must be admitted {id:x}"
        );
    }
    let generators: Vec<_> = game
        .maps
        .iter()
        .filter_map(|m| {
            if let Geometry::Generator(g) = &m.geometry {
                Some(g)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(generators.len(), 13);
    assert_eq!(
        generators
            .iter()
            .filter(|g| g.seed == SeedPolicy::World)
            .count(),
        12
    );
    assert_eq!(
        generators
            .iter()
            .filter(|g| g.seed == SeedPolicy::Unseeded)
            .count(),
        1
    );
    for (family, count) in [("lab", 12), ("manor", 35), ("davy", 28), ("spider", 1)] {
        let bytes = std::fs::read(format!("data/{family}-templates.json")).unwrap();
        let lib: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(lib["templates"].as_object().unwrap().len(), count);
        for (name, source) in lib["templates"].as_object().unwrap() {
            let map = game
                .maps
                .iter()
                .find(|m| m.name == format!("{family}-{name}"))
                .unwrap();
            let Geometry::Template(t) = &map.geometry else {
                panic!("normalized template")
            };
            let rows = source["rows"].as_array().unwrap();
            assert_eq!(t.dimensions[1] as usize, rows.len());
            assert_eq!(
                t.dimensions[0] as usize,
                rows.iter()
                    .map(|r| r.as_str().unwrap().len())
                    .max()
                    .unwrap()
            );
            assert_eq!(
                t.ports.len(),
                source["ports"].as_array().map_or(0, |p| p.len())
            );
            assert_eq!(
                t.pack.len(),
                source["pack"].as_array().map_or(0, |p| p.len())
            );
            assert!(
                map.qualification
                    .origin
                    .contains(&format!("{family}-templates.json:{name}"))
            );
            assert_eq!(t.rooms, source["rooms"].as_u64().unwrap_or(1) as u32);
            assert_eq!(
                matches!(t.rotation, super::geometry::Rotation::Fixed),
                source["fixed"].as_bool() == Some(true)
            );
            for (y, row) in rows.iter().enumerate() {
                for x in 0..t.dimensions[0] as usize {
                    let ch = row.as_str().unwrap().chars().nth(x).unwrap_or(' ');
                    let cell = t.cells[y * t.dimensions[0] as usize + x];
                    if ch == ' ' {
                        assert!(cell.is_none());
                        continue;
                    }
                    let tile = &t.palette[cell.unwrap() as usize];
                    let original = &lib["legend"][ch.to_string()];
                    let source_ground = original[0].as_str().unwrap();
                    let ground = if source_ground.is_empty() {
                        if family == "davy" { "Space" } else { "Empty" }
                    } else {
                        source_ground
                    };
                    assert_eq!(
                        CONTENT.ground_names[&tile.tile.ground.unwrap()],
                        ground,
                        "{family}/{name} {x},{y}"
                    );
                    let object = original[1].as_str().unwrap();
                    if matches!(
                        object,
                        "table slot"
                            | "vat field"
                            | "secret shelf"
                            | "key slot"
                            | "door slot"
                            | "loose wall"
                    ) {
                        assert!(
                            tile.socket.is_some(),
                            "missing typed marker {family}/{name}/{object}"
                        );
                    } else if object == "spawn" && family == "manor" {
                        assert_eq!(tile.tile.object, Some(0x703));
                    } else if object.is_empty() || object == "spawn" {
                        if let Some(id) = tile.tile.object {
                            assert_eq!(family, "lab");
                            assert_eq!(
                                (ground, id),
                                if ground == "Lab Bad Vat Rug" {
                                    ("Lab Bad Vat Rug", 0x1e15)
                                } else {
                                    ("Lab Good Vat Rug", 0x1e16)
                                }
                            );
                        }
                    } else {
                        assert_eq!(CONTENT.objects[&tile.tile.object.unwrap()].name, object);
                    }
                }
            }
            if let Some(pack) = source["pack"].as_array() {
                for (actual, expected) in t.pack.iter().zip(pack) {
                    assert_eq!(actual.population.min, 1);
                    assert_eq!(actual.population.max, 1);
                    let super::geometry::PopulationSelector::Object(id) =
                        actual.population.selector
                    else {
                        panic!("source exact pack")
                    };
                    assert_eq!(CONTENT.objects[&id].name, expected.as_str().unwrap());
                }
            }
        }
    }
}

#[test]
fn xml_compile_production_beach_candidate_claims_match_selected_cells() {
    use super::geometry::Geometry;
    let Geometry::Template(t) = &GAME
        .maps
        .iter()
        .find(|m| m.name == "beachzone.jm")
        .unwrap()
        .geometry
    else {
        panic!("unseeded Beach template")
    };
    assert_eq!(t.dimensions, [136, 127]);
    let object_count = |id| {
        t.cells
            .iter()
            .flatten()
            .filter(|i| t.palette[**i as usize].tile.object == Some(id))
            .count()
    };
    assert_eq!(CONTENT.objects[&0x18a7].name, "Beach Umbrella");
    assert_eq!(object_count(0x18a7), 16);
    assert_eq!(object_count(0xdc6), 4);
    assert_eq!(object_count(0xe56), 1);
    assert_eq!(object_count(0x703), 1);
    assert_eq!(
        object_count(0x163f),
        0,
        "no false Beach Ball placement claim"
    );
    let ground_count = |g| {
        t.cells
            .iter()
            .flatten()
            .filter(|i| t.palette[**i as usize].tile.ground == Some(g))
            .count()
    };
    assert_eq!(ground_count(0xb4) + ground_count(0xb5), 32);
    assert_eq!(ground_count(0xb6), 5);
    assert_eq!(t.spawns, [[46.5, 65.5]]);
}

#[test]
fn xml_compile_production_world_services_are_compiled_not_sidecar_evidence() {
    let game = &*GAME;
    let find = |id: &str| &game.worlds.iter().find(|w| w.id == id).unwrap().services;
    assert_eq!(
        find("ocean-trench").uses_breath.as_ref().unwrap().value,
        7.25
    );
    assert_eq!(find("ocean-trench").difficulty.as_ref().unwrap().value, 5);
    assert_matches!(
        find("sprite-world").background.as_ref().unwrap().value,
        Background::Stars
    );
    assert_matches!(
        find("pirate-cave").visibility.as_ref().unwrap().value,
        Visibility::Full
    );
    assert_matches!(
        find("mad-lab").visibility.as_ref().unwrap().value,
        Visibility::Path
    );
    assert_matches!(
        find("manor-of-the-immortals")
            .visibility
            .as_ref()
            .unwrap()
            .value,
        Visibility::LineOfSight
    );
    assert_matches!(
        find("davy-jones-locker")
            .hello_notification
            .as_ref()
            .unwrap()
            .value,
        HelloNotification::ShowKeyUi
    );
    assert!(find("nexus").uses_breath.is_none());
    let service = find("ocean-trench").uses_breath.as_ref().unwrap();
    assert_eq!(service.source.file, "game/123.5.1/game.xml");
    assert_eq!(service.source.path, "World/ocean-trench/UsesBreath");
    assert_matches!(service.provenance, Provenance::Recovered);
    assert!(service.origin.contains("dungeons.xml"));
    assert!(game.inputs.contains_key("game/123.5.1/game.xml"));
    assert!(!game.inputs.contains_key("game/123.5.1/world-services.xml"));
}

fn production_inputs() -> BTreeMap<String, Vec<u8>> {
    super::test_inputs::production(&GAME, Path::new("data"))
}

#[test]
fn xml_compile_production_rejects_reached_actor_state_and_asset_mutations() {
    // Mutate in memory: no shadow source tree, original archive or selected-property edits.
    let mut inputs = production_inputs();
    inputs.remove("tutorial.wmap");
    let error = format!(
        "{:#}",
        super::compile::compile_at(&CONTENT, inputs, Some(Path::new("data"))).unwrap_err()
    );
    assert!(
        error.contains("tutorial.wmap") && error.contains("missing required"),
        "{error}"
    );
    for (from, to, expected) in [
        (
            "childIds=\"Stheno Pet\"",
            "childIds=\"No Such Selected Actor\"",
            "unresolved",
        ),
        (
            "<Transition afterTime=\"3\">attack1</Transition>",
            "<Transition afterTime=\"3\">MissingSibling</Transition>",
            "unresolved sibling state",
        ),
    ] {
        let mut inputs = production_inputs();
        let key = "game/123.5.1/actors.xml";
        let text = String::from_utf8(inputs[key].clone()).unwrap();
        assert!(
            text.contains(from),
            "actual production mutation site missing {from}"
        );
        inputs.insert(key.into(), text.replacen(from, to, 1).into_bytes());
        let error = format!(
            "{:#}",
            super::compile::compile_at(&CONTENT, inputs, Some(Path::new("data"))).unwrap_err()
        );
        assert!(
            error.contains(expected) && error.contains("actors.xml"),
            "{error}"
        );
    }
}

#[test]
fn xml_compile_production_xml_only_loot_and_service_mutation_changes_ir_fingerprint() {
    let original = &*GAME;
    let mut inputs = production_inputs();
    let key = "game/123.5.1/actors.xml";
    let text = String::from_utf8(inputs[key].clone()).unwrap();
    let from = "<SoulboundLoot prob=\".001\" threshold=\".15\">Wand of the Bulwark</SoulboundLoot>";
    assert_eq!(text.matches(from).count(), 2);
    inputs.insert(
        key.into(),
        text.replacen(from, &from.replace(".001", ".002"), 1)
            .into_bytes(),
    );
    let key = "game/123.5.1/game.xml";
    let text = String::from_utf8(inputs[key].clone()).unwrap();
    assert_eq!(text.matches(">7.25</UsesBreath>").count(), 1);
    inputs.insert(
        key.into(),
        text.replace(">7.25</UsesBreath>", ">8.25</UsesBreath>")
            .into_bytes(),
    );
    let changed = super::compile::compile_at(&CONTENT, inputs, Some(Path::new("data"))).unwrap();
    assert_ne!(original.fingerprint, changed.fingerprint);
    assert_eq!(
        serde_json::to_value(&changed.properties).unwrap(),
        serde_json::to_value(&original.properties).unwrap()
    );
    let rate = |g: &GameData| {
        g.definitions[&0x917]
            .loot
            .iter()
            .find(|r| r.pool.contains(&0xc04))
            .unwrap()
            .probability
    };
    assert_eq!(rate(original), 0.001);
    assert_eq!(rate(&changed), 0.002);
    assert_eq!(
        changed
            .worlds
            .iter()
            .find(|w| w.id == "ocean-trench")
            .unwrap()
            .services
            .uses_breath
            .as_ref()
            .unwrap()
            .value,
        8.25
    );
    let dir = std::env::var("QA_RESULTS_DIR").unwrap();
    std::fs::write(Path::new(&dir).join("xml-production-mutation.json"),serde_json::to_vec_pretty(&json!({
        "qualification":"same Rust test executable compiles actual production XML mutations in memory; NOT executor/native mutation acceptance",
        "original":original.fingerprint,"mutated":changed.fingerprint,"original_bulwark":0.001,"mutated_bulwark":0.002,
        "original_breath":7.25,"mutated_breath":8.25
    })).unwrap()).unwrap();
}

#[test]
fn xml_compile_production_recovered_room_count_caps() {
    use super::geometry::Geometry;
    let source = std::fs::read_to_string("data/reference/release-7.0/xml/dungeons.xml").unwrap();
    let source = roxmltree::Document::parse(&source).unwrap();
    let mut checked = Vec::new();
    for (source_name, world) in [
        ("Mad Lab", "mad-lab"),
        ("Davy Jones's Locker", "davy-jones-locker"),
        ("Manor of the Immortals", "manor-of-the-immortals"),
        ("Forbidden Jungle", "forbidden-jungle"),
        ("Ocean Trench", "ocean-trench"),
        ("Pirate Cave", "pirate-cave"),
        ("Spider Den", "spider-den"),
        ("Undead Lair", "undead-lair"),
        ("Abyss of Demons", "abyss-of-demons"),
        ("Snake Pit", "snake-pit"),
        ("Sprite World", "sprite-world"),
        ("Tomb of the Ancients", "tomb-of-the-ancients"),
        ("Beachzone", "beachzone"),
    ] {
        let original = source
            .root_element()
            .children()
            .find(|n| n.attribute("id") == Some(source_name))
            .unwrap();
        let map = GAME.maps.iter().find(|m| m.name == world).unwrap();
        let Geometry::Generator(generator) = &map.geometry else {
            panic!("selected generator {world}")
        };
        let mut duplicates = BTreeMap::<String, usize>::new();
        for room in original.children().filter(|n| n.has_tag_name("Room")) {
            let base = room.attribute("id").unwrap();
            let index = duplicates.entry(base.into()).or_default();
            let id = if *index == 0 {
                base.into()
            } else {
                format!("{base}-{index}")
            };
            *index += 1;
            let literal = |tag| {
                room.children()
                    .find(|n| n.has_tag_name(tag))
                    .map(|n| n.text().unwrap().trim().parse::<u32>().unwrap())
            };
            let cap = literal("MaxCount");
            let padding = literal("MinPadding");
            let corridor = match (literal("MinHallwayLen"), literal("MaxHallwayLen")) {
                (None, None) => None,
                (Some(min), Some(max)) => Some((min, max)),
                _ => panic!("unpaired selected source corridor {world}/{id}"),
            };
            let actual: Vec<_> = generator
                .rooms
                .iter()
                .filter(|r| r.id == id || r.id.starts_with(&format!("{id}-shape-")))
                .collect();
            assert!(
                !actual.is_empty(),
                "selected source room absent {world}/{id}"
            );
            for emitted in actual {
                if let Some(expected) = cap {
                    assert_eq!(
                        emitted.max_count, expected,
                        "recovered cap {world}/{}",
                        emitted.id
                    );
                }
                assert_eq!(
                    emitted.padding,
                    padding.unwrap_or(1),
                    "source padding {world}/{}",
                    emitted.id
                );
                assert_eq!(
                    emitted.corridor_length.map(|s| (s.min, s.max)),
                    corridor,
                    "source corridor override/absence {world}/{}",
                    emitted.id
                );
                checked.push(json!({"world":world,"room":emitted.id,"source_line":source.text_pos_at(room.range().start).row,"max_count":cap,"padding":padding,"corridor":corridor}));
            }
        }
    }
    assert_eq!(
        checked.len(),
        55,
        "54 selected source rooms plus the second Undead shape variant"
    );
    assert_eq!(
        checked
            .iter()
            .filter(|r| r["world"] == "mad-lab" && !r["max_count"].is_null())
            .map(|r| r["max_count"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 1, 5, 1]
    );
    assert_eq!(
        checked
            .iter()
            .filter(|r| r["world"] == "davy-jones-locker" && !r["max_count"].is_null())
            .map(|r| r["max_count"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 1, 1, 4, 1, 2, 3]
    );
    assert_eq!(
        checked.iter().filter(|r| !r["max_count"].is_null()).count(),
        11
    );
    assert_eq!(
        checked.iter().filter(|r| !r["padding"].is_null()).count(),
        10
    );
    assert_eq!(
        checked
            .iter()
            .filter(|r| r["corridor"] == json!([0, 0]))
            .count(),
        7
    );
    assert_eq!(
        checked
            .iter()
            .filter(|r| r["world"] == "davy-jones-locker" && r["corridor"].is_null())
            .count(),
        3
    );
    let dir = std::env::var("QA_RESULTS_DIR").expect("run via qa.py");
    std::fs::write(
        Path::new(&dir).join("xml-production-room-caps.json"),
        serde_json::to_vec_pretty(&checked).unwrap(),
    )
    .unwrap();
}

#[test]
fn xml_compile_production_explicit_native_services_and_scoped_selected_activations() {
    use sha2::{Digest, Sha256};
    let game = &*GAME;
    let count = |disposition| {
        game.definitions
            .values()
            .filter(|d| d.disposition == disposition)
            .count()
    };
    assert_eq!(
        (
            count(Disposition::Active),
            count(Disposition::Passive),
            count(Disposition::Native)
        ),
        (542, 19, 1460)
    );
    let xml = std::fs::read_to_string("data/game/123.5.1/actors.xml").unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let mut inventory = BTreeMap::new();
    let mut material = String::new();
    for n in doc
        .root_element()
        .children()
        .filter(|n| n.has_tag_name("Object") && n.attribute("disposition") == Some("native"))
    {
        let kind =
            u16::from_str_radix(n.attribute("type").unwrap().trim_start_matches("0x"), 16).unwrap();
        let service = n.attribute("nativeService").unwrap();
        let d = &game.definitions[&kind];
        let p = &game.properties[&kind];
        assert_eq!(d.disposition, Disposition::Native);
        assert_eq!(d.provenance, Provenance::Authored);
        assert_eq!(n.attribute("id"), Some(p.name.as_str()));
        assert!(!p.enemy && !d.reason.is_empty() && !d.origin.is_empty());
        assert!(!n.children().any(|c| c.is_element()));
        assert!(d.loot.is_empty() && d.states.iter().all(|s| s.behaviors.is_empty()));
        let role = d.admission_role().unwrap();
        assert!(std::ptr::eq(
            game.admission(kind, role, "production native metadata")
                .unwrap(),
            d
        ));
        let typed = d.native_service.unwrap();
        assert_eq!(
            game.entity_admission(kind, role, "ordinary native entity eligibility only")
                .is_ok(),
            typed.is_world_entity()
        );
        if matches!(typed, NativeService::Inventory) {
            assert_matches!(p.class.as_str(), "Equipment" | "Dye");
        }
        if matches!(typed, NativeService::Companion) {
            assert_eq!(p.class, "Character");
            assert!(
                game.properties
                    .values()
                    .any(|item| item.class == "Equipment"
                        && item.slot_type == 10
                        && item.activations == 1
                        && item.companion.as_ref().is_some_and(|c| c.object == p.name))
            );
        }
        material.push_str(&format!("{kind:04x}:{service}\n"));
        inventory.insert(kind,json!({"name":p.name,"class":p.class,"service":typed,"origin":d.origin,"reason":d.reason}));
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(material.as_bytes())),
        "44849f189e78251414fd1721b1e75e88c209b78a673775155aefa4134401f971",
        "independent reviewed exact-kind/service inventory"
    );
    for (kind, service) in [
        (0x120, NativeService::Scenery),
        (0x1407, NativeService::Companion),
        (0x1c9, NativeService::Scenery),
        (0x505, NativeService::Container),
        (0x72f, NativeService::Portal),
    ] {
        assert_eq!(game.definitions[&kind].native_service, Some(service));
    }
    for kind in [0x1cb, 0x1cc, 0x1dc, 0x1df, 0x1e5, 0x1e7] {
        assert_eq!(
            game.definitions[&kind].native_service,
            Some(NativeService::Interaction)
        );
    }
    for (kind, target) in [
        (DeploymentKind::Trap, 0x711),
        (DeploymentKind::Decoy, 0x715),
        (DeploymentKind::SpiritBomb, 0x1409),
    ] {
        assert_eq!(
            game.deployment_binding(kind, "actual reviewed production binding")
                .unwrap()
                .kind,
            target
        );
    }
    let mut expected_edges = 0;
    for item in game.properties.values().filter(|p| {
        game.definitions
            .get(&p.kind)
            .is_some_and(|d| d.native_service == Some(NativeService::Inventory))
    }) {
        for (index, ability) in item.abilities.iter().enumerate() {
            if let Some(kind) = DeploymentKind::of_ability(ability) {
                expected_edges += 1;
                let edge = game.deployment_activations[&item.kind]
                    .iter()
                    .find(|e| e.activation == index)
                    .unwrap();
                assert_eq!(edge.kind, kind);
                assert!(game.property_dependencies[&item.kind].contains(&edge.target));
                assert_eq!(edge.source.file, "objects.xml");
                assert_eq!(edge.source.object, item.name);
                assert_eq!(edge.source.path, format!("@Activate[{index}]"));
                assert!(edge.source.line > 0);
                game.deployment_admission(
                    edge.target,
                    item,
                    index,
                    kind,
                    "actual production selected activation",
                )
                .unwrap();
                assert!(
                    game.deployment_admission(
                        edge.target,
                        &item.clone(),
                        index,
                        kind,
                        "cloned item is not canonical"
                    )
                    .is_err()
                );
            }
        }
    }
    assert!(expected_edges > 0);
    assert_eq!(
        expected_edges,
        game.deployment_activations
            .values()
            .map(Vec::len)
            .sum::<usize>()
    );
    let prism = &game.properties[&0xc2a];
    assert_eq!((prism.slot_type, prism.activations), (22, 2));
    assert_matches!(prism.abilities[0], crate::content::Ability::SpiritBomb);
    assert_matches!(prism.abilities[1], crate::content::Ability::Teleport { .. });
    assert_eq!(game.deployment_activations[&0xc2a][0].activation, 0);
    let dancing = &game.properties[&0xc0d];
    assert_eq!(dancing.activations, 2);
    assert_eq!(
        dancing
            .abilities
            .iter()
            .map(DeploymentKind::of_ability)
            .collect::<Vec<_>>(),
        [Some(DeploymentKind::Decoy), Some(DeploymentKind::Decoy)]
    );
    // Every selected inventory item stays usable: earned, vaulted and traded items need no loot route.
    assert_eq!(
        game.definitions[&0xc0d].admission_role().unwrap(),
        AdmissionRole::Native(NativeService::Inventory)
    );
    assert!(game.deployment_activations.contains_key(&0xc0d));
    assert_eq!(
        serde_json::to_value(&game.properties).unwrap(),
        serde_json::to_value(&CONTENT.objects).unwrap()
    );
    std::fs::write(Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("xml-production-native-admission.json"),serde_json::to_vec_pretty(&json!({
        "qualification":"actual production GameData::load explicit native metadata/typed property-edge parity only; NOT World ownership, activation, reservation or native runtime acceptance",
        "fingerprint":game.fingerprint,"inventory":inventory,"deployment_activations":game.deployment_activations,
        "items_with_deployments":game.deployment_activations.len(),"deployment_edges":expected_edges,
        "active":534,"passive":19,"native":881
    })).unwrap()).unwrap();
}

#[test]
fn xml_compile_production_generated_catalogue_and_recovered_locked_portal_are_not_native_ai() {
    let game = &*GAME;
    let scenery = [
        0x183, 0xdc5, 0x170f, 0x18ae, 0x18af, 0x18b0, 0x18b1, 0x18b2, 0x18b3, 0x18b4, 0x18b5,
        0x18bc, 0x18bd, 0x18be, 0x18bf,
    ];
    for kind in scenery {
        let p = &game.properties[&kind];
        let d = &game.definitions[&kind];
        assert_eq!(p.class, "GameObject");
        assert!(!p.enemy && p.static_object && p.hp == 0);
        assert_eq!(d.disposition, Disposition::Native);
        assert_eq!(d.native_service, Some(NativeService::Scenery));
        assert_eq!(d.provenance, Provenance::Authored);
        assert!(d.states.iter().all(|s| s.behaviors.is_empty()) && d.loot.is_empty());
        assert!(game.roots.contains(&kind) && game.reachable.contains(&kind));
        game.entity_admission(
            kind,
            AdmissionRole::Native(NativeService::Scenery),
            "reviewed finite generated catalogue, not birth or all-seed permission",
        )
        .unwrap();
    }
    let portal = &game.definitions[&0x700];
    assert_eq!(portal.native_service, Some(NativeService::Portal));
    assert_eq!(portal.provenance, Provenance::Authored);
    assert!(portal.states.iter().all(|s| s.behaviors.is_empty()));
    assert_eq!(game.properties[&0x700].name, "Dungeon Portal");
    let locked = &game.definitions[&0x721];
    assert_eq!(game.properties[&0x721].class, "Portal");
    assert_eq!(game.properties[&0x721].name, "Locked Wine Cellar Portal");
    assert_eq!(locked.disposition, Disposition::Active);
    assert_eq!(locked.native_service, None);
    assert_eq!(locked.provenance, Provenance::Recovered);
    let source = std::fs::read_to_string("data/reference/release-7.0/xml/objects.xml").unwrap();
    let doc = roxmltree::Document::parse(&source).unwrap();
    let original = doc
        .descendants()
        .find(|n| {
            n.has_tag_name("Object") && n.attribute("id") == Some("Locked Wine Cellar Portal")
        })
        .unwrap();
    assert_eq!(original.attribute("type"), Some("0x0721"));
    assert!(original.children().any(|n| n.has_tag_name("LockedPortal")));
    let rule = original
        .children()
        .find(|n| n.has_tag_name("Behavior"))
        .unwrap();
    assert_eq!(rule.attribute("time"), Some("120"));
    assert_eq!(rule.text(), Some("Decay"));
    let line = doc.text_pos_at(original.range().start).row;
    assert_eq!(
        locked.origin,
        format!("data/reference/release-7.0/xml/objects.xml:{line}")
    );
    let behaviors: Vec<_> = locked.states.iter().flat_map(|s| &s.behaviors).collect();
    assert_eq!(behaviors.len(), 1);
    let decay = behaviors[0];
    assert_eq!(decay.kind, BehaviorKind::Decay);
    assert_matches!(decay.fields[&Field::Time], Value::Millis(120000));
    assert_matches!(decay.fields[&Field::DropLoot], Value::Bool(false));
    assert_matches!(decay.fields[&Field::GiveXP], Value::Bool(false));
    assert!(
        game.roots.contains(&0x700)
            && game.roots.contains(&0x721)
            && game.reachable.contains(&0x721)
    );
    assert!(
        game.admission(
            0x721,
            AdmissionRole::Native(NativeService::Portal),
            "scripted lifecycle cannot be reclassified"
        )
        .is_err()
    );
    let mut inputs = production_inputs();
    let key = "game/123.5.1/actors.xml";
    let mut xml = String::from_utf8(inputs[key].clone()).unwrap();
    let runtime = roxmltree::Document::parse(&xml).unwrap();
    let declaration = runtime
        .descendants()
        .find(|n| {
            n.has_tag_name("Object") && n.attribute("id") == Some("Locked Wine Cellar Portal")
        })
        .unwrap();
    let range = declaration.range();
    let changed = xml[range.clone()].replacen("time=\"120\"", "time=\"121\"", 1);
    xml.replace_range(range, &changed);
    inputs.insert(key.into(), xml.into_bytes());
    let mutated = super::compile::compile_at(&CONTENT, inputs, Some(Path::new("data"))).unwrap();
    assert_ne!(mutated.fingerprint, game.fingerprint);
    let new_decay = mutated.definitions[&0x721]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == BehaviorKind::Decay)
        .unwrap();
    assert_matches!(new_decay.fields[&Field::Time], Value::Millis(121000));
    assert_eq!(game.deployment_activations.len(), 17);
    assert_eq!(
        game.deployment_activations
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        18
    );
    std::fs::write(Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("xml-production-generated-catalogue.json"),serde_json::to_vec_pretty(&json!({
        "qualification":"exact reviewed generated/service catalogue and recovered721 XML authority; NOT all-seed/world/factory/native/committed-lifecycle acceptance",
        "source_scenery":scenery,"native_portal":0x700,"scripted_portal":0x721,"reference_origin":locked.origin,
        "decay_ms":120000,"mutated_decay_ms":121000,"input_fingerprint":game.fingerprint,"mutated_fingerprint":mutated.fingerprint,
        "definitions":game.definitions.len(),"reachable":game.reachable.len(),"roots":game.roots.len(),"scoped_activation_edges":16
    })).unwrap()).unwrap();
}

#[test]
fn xml_compile_landmark_chest_refusals_are_contextual() {
    let key = "game/123.5.1/game.xml";
    let original = String::from_utf8(production_inputs()[key].clone()).unwrap();
    let start = original
        .find("<LandmarkChest landmark=\"Deathmage\"")
        .unwrap();
    let end =
        start + original[start..].find("</LandmarkChest>").unwrap() + "</LandmarkChest>".len();
    let chest = &original[start..end];
    for (mutated, expected) in [
        (
            chest.replacen("provenance=\"authored\"", "provenance=\"recovered\"", 1),
            "landmark chest rows are authored reconstructions",
        ),
        (
            chest.replacen("ceilingMin=\"3\"", "ceilingMin=\"9\"", 1),
            "invalid chest success ceiling",
        ),
        (
            chest.replacen(" ceilingMax=\"7\"", "", 1),
            "chest success ceiling needs ceilingMin and ceilingMax",
        ),
        (format!("{chest}\n  {chest}"), "duplicate landmark chest"),
    ] {
        let mut xml = original.clone();
        xml.replace_range(start..end, &mutated);
        let mut inputs = production_inputs();
        inputs.insert(key.into(), xml.into_bytes());
        let error = format!(
            "{:#}",
            super::compile::compile_at(&CONTENT, inputs, Some(Path::new("data"))).unwrap_err()
        );
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn xml_compile_production_snake_pit_portal_sources_for_bot_advice() {
    // The five Godlands leaders (actors.xml:1267-1510) and the Ghost King's dying state (actors.xml:3947).
    let sources: Vec<u16> = (GAME.definitions.keys().copied())
        .filter(|kind| GAME.births_at_death(*kind, 0x718))
        .collect();
    assert_eq!(sources, [0x640, 0x645, 0x646, 0x649, 0x650, 0x928]);
}
