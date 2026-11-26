use super::{compile::compile, geometry::Geometry, *};
use std::sync::LazyLock;

static CONTENT: LazyLock<Content> = LazyLock::new(|| Content::load(Path::new("data")).unwrap());
const MANIFEST: &str = "game/123.5.1/game.xml";
const QUALIFIED: &str = r#"provenance="authored" origin="synthetic compiler fixture" reason="typed geometry contract test""#;

fn inputs(world: &str, geometry: &str, objects: &str) -> BTreeMap<String, Vec<u8>> {
    test_inputs::selected([
        (MANIFEST, format!(r#"<Game schema="1"><Include file="rules.xml"/><Geometry file="geometry.xml"/>{world}</Game>"#).into_bytes()),
        ("game/123.5.1/rules.xml", format!("<Objects>{objects}</Objects>").into_bytes()),
        ("game/123.5.1/geometry.xml", format!(r#"<Geometry schema="1">{geometry}</Geometry>"#).into_bytes()),
    ])
}

fn floor(id: &str, tile: &str) -> String {
    format!(
        r#"<Template id="{id}" {QUALIFIED} width="1" height="1" rotation="fixed" rooms="1">
        <Anchor x="0" y="0"/><CharMap char="." ground="Grass" {tile}/><Pattern><Row value="."/></Pattern>
        </Template>"#
    )
}

#[test]
fn xml_world_rejects_map_portal_without_linked_route() {
    let geometry = floor("room", r#"object="Realm Portal""#);
    let objects = format!(
        r#"<Object type="0x704" id="Realm Portal" disposition="native" nativeService="portal" {QUALIFIED}/>"#
    );
    let err = compile(
        &CONTENT,
        inputs(r#"<World id="fixture" map="room"/>"#, &geometry, &objects),
    )
    .unwrap_err();
    let err = format!("{err:#}");
    assert!(
        err.contains("World fixture map=room: physical portal")
            && err.contains("no linked portal route kind=0x704"),
        "{err}"
    );
}

#[test]
fn xml_world_rejects_map_rebind_without_required_placement() {
    let geometry = floor("with-wall", r#"object="Grey Wall""#) + &floor("empty-room", "");
    let wall = format!(
        r#"<Object type="0x120" id="Grey Wall" disposition="native" nativeService="scenery" {QUALIFIED}/>"#
    );
    let world = |map| {
        format!(r#"<World id="fixture" map="{map}"><RequiredActor object="Grey Wall"/></World>"#)
    };
    compile(&CONTENT, inputs(&world("with-wall"), &geometry, &wall)).unwrap();
    let err = compile(&CONTENT, inputs(&world("empty-room"), &geometry, &wall)).unwrap_err();
    let err = format!("{err:#}");
    assert!(
        err.contains("World fixture RequiredActor must reference a unique selected map placement"),
        "{err}"
    );
}

#[test]
fn xml_generator_rejects_wrong_placement_roster_cardinality() {
    let content = crate::world::test_content();
    let game = content.game.as_ref().unwrap();
    let world = game.world_template("snake-pit").unwrap();
    let Geometry::Generator(mut generator) = game.maps[world.map as usize].geometry.clone() else {
        panic!("expected Snake Pit generator");
    };
    assert!(generator.placements::<4>("rooms").is_ok());
    generator
        .placements
        .get_mut("rooms")
        .unwrap()
        .pop()
        .unwrap();
    let err = crate::dungeon::generate(&content, world, &generator, || 1).unwrap_err();
    assert!(
        err.to_string()
            .contains("Snake PlacementRoster rooms requires 4 entries"),
        "{err}"
    );
}
