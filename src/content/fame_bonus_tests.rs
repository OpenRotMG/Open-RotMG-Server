use super::*;

#[test]
fn fame_bonus_uses_only_absent_field_default_and_rejects_malformed_present_values() {
    for (fields, expected) in [
        ("", 0),
        ("<FameBonus>0</FameBonus>", 0),
        ("<FameBonus>3</FameBonus>", 3),
    ] {
        let xml = format!("<Object id=\"fixture\">{fields}</Object>");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert_eq!(fame_bonus(doc.root_element()).unwrap(), expected);
    }
    for fields in [
        "<FameBonus/>",
        "<FameBonus>-1</FameBonus>",
        "<FameBonus>1.5</FameBonus>",
        "<FameBonus>NaN</FameBonus>",
        "<FameBonus>4294967296</FameBonus>",
        "<FameBonus>3</FameBonus><FameBonus>4</FameBonus>",
    ] {
        let xml = format!("<Object id=\"fixture\">{fields}</Object>");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        assert!(fame_bonus(doc.root_element()).is_err(), "{fields}");
    }
    let content = Content::load(Path::new("data")).unwrap();
    for (kind, percent) in [(0xa97, 0), (0xaa2, 3), (0xabf, 3), (0xb05, 4), (0xc01, 6)] {
        assert_eq!(content.get(kind).unwrap().fame_bonus, percent);
    }
}
