use roxmltree::Document;

const ARCHIVE: &str = "data/reference/release-7.0/xml";

fn hex(text: &str) -> u16 {
    u16::from_str_radix(text.trim_start_matches("0x"), 16).unwrap()
}

#[test]
fn xml_contract_selected_transition_gap_requirements() {
    let ocean = std::fs::read_to_string(format!("{ARCHIVE}/dungeons/oceanTrench.xml")).unwrap();
    let doc = Document::parse(&ocean).unwrap();
    let thessal = doc
        .descendants()
        .find(|n| n.attribute("type") == Some("0x1706"))
        .unwrap();
    for part in ["Part1", "Part2", "Part3"] {
        let state = thessal
            .children()
            .find(|n| n.attribute("id") == Some(part))
            .unwrap();
        assert!(
            state
                .children()
                .any(|n| n.has_tag_name("Embed") && n.attribute("name") == Some("throwCreature"))
        );
        assert_eq!(
            state
                .children()
                .any(|n| n.has_tag_name("State") && n.attribute("id") == Some("BigWait")),
            part == "Part1"
        );
    }
    let castle = std::fs::read_to_string(format!("{ARCHIVE}/dungeons/oryxCastle.xml")).unwrap();
    let doc = Document::parse(&castle).unwrap();
    let eye = doc
        .descendants()
        .find(|n| n.attribute("id") == Some("Oryx Eye Warrior"))
        .unwrap();
    assert!(
        eye.descendants()
            .any(|n| n.has_tag_name("Transition") && n.text() == Some("Attack1"))
    );
    assert!(
        !eye.descendants()
            .any(|n| n.has_tag_name("State") && n.attribute("id") == Some("Attack1"))
    );
    assert!(eye.descendants().any(|n| n.attribute("id") == Some("buzz")
        && n.children().any(|t| t.has_tag_name("Transition")
            && t.text() == Some("search")
            && t.attribute("afterTime") == Some("5"))));
}

#[test]
fn xml_contract_stheno_recovered_state_durations_not_legacy_schedule() {
    let text = std::fs::read_to_string(format!("{ARCHIVE}/dungeons/snakePit.xml")).unwrap();
    let doc = Document::parse(&text).unwrap();
    let stheno = doc
        .descendants()
        .find(|n| n.has_tag_name("Object") && n.attribute("type").is_some_and(|s| hex(s) == 0x917))
        .unwrap();
    for (state, seconds, target) in [
        ("wait_for_pets", "3", "attack1"),
        ("attack1", "8", "summon_snakes"),
        ("summon_snakes", "12", "chance_to_clear_snakes"),
        ("chance_to_clear_snakes", "5", "attack2"),
        ("attack2", "8", "attack1"),
        ("unshielded1", "1", "shielded1"),
        ("shielded1", "3", "unshielded1"),
        ("unshielded2", "1.5", "shielded2"),
        ("shielded2", "2.5", "unshielded2"),
    ] {
        let node = stheno
            .descendants()
            .find(|n| n.has_tag_name("State") && n.attribute("id") == Some(state))
            .unwrap();
        assert!(
            node.children().any(|n| n.has_tag_name("Transition")
                && n.attribute("afterTime") == Some(seconds)
                && n.text() == Some(target)),
            "source state {state}"
        );
    }
}
