use super::*;
use crate::content::Content;
use std::path::Path;

#[test]
fn researched_ranges_keep_source_and_do_not_promote_video_or_quarantines() {
    let content = Content::load(Path::new("data")).unwrap();
    let values = &content.item_values;
    assert_eq!(
        values.quote_range(DEFENSE),
        Ok(ValueRange { min: 12, max: 12 })
    );
    assert_eq!(values.quote_range(2593), Ok(ValueRange { min: 3, max: 4 }));
    assert_eq!(
        values.quote_range(2794),
        Ok(ValueRange { min: 36, max: 48 })
    );
    assert_eq!(
        values.get(2794).unwrap().evidence.as_ref().unwrap().record,
        "w007"
    );
    assert_eq!(
        values.get(2794).unwrap().evidence.as_ref().unwrap().kind,
        "guide_estimate"
    );
    assert_eq!(
        values.get(2794).unwrap().evidence.as_ref().unwrap().date,
        "2012-10-08"
    );
    // Elder uses the unconditional first payment, never its conditional one-Life alternative.
    assert_eq!(
        values.get(2710).unwrap().range,
        Some(ValueRange { min: 36, max: 48 })
    );
    assert_eq!(
        values.get(2760).unwrap().range,
        Some(ValueRange { min: 48, max: 60 })
    );
    assert_eq!(
        values.get(2760).unwrap().evidence.as_ref().unwrap().record,
        "w072"
    );
    assert_eq!(values.quote_range(2612), Err("contradictory_source"));
    assert_eq!(values.quote_range(2721), Err("missing_denomination"));
    assert_eq!(values.quote_range(2727), Err("strict_or_unbounded_price"));
    assert_eq!(
        values.quote_range(2806),
        Err("additional_constraint_not_supported")
    );
    assert_eq!(values.quote_range(2784), Err("no_selected_guide_record"));
    assert_eq!(values.quote_range(-1), Err("unknown_catalog_item"));
    assert_eq!(values.quote_range(65535), Err("unknown_catalog_item"));
    assert_eq!(
        values.quote_range(2878),
        Err("runtime_trade_safety_restriction")
    );
    assert!(
        values.get(2878).unwrap().range.is_some(),
        "historical currency is distinct from safe native inventory"
    );
    assert_eq!(values.kinds().count(), 137);
    assert!(
        values
            .kinds()
            .filter(|k| values.quote_range(*k).is_ok())
            .count()
            > 25
    );
}

#[test]
fn malformed_catalog_fails_initialization_instead_of_defaulting_prices() {
    let content = Content::load(Path::new("data")).unwrap();
    let original: serde_json::Value = serde_json::from_str(CATALOG).unwrap();
    let mut mutations = Vec::new();
    let mut bad = original.clone();
    bad["schema_version"] = 2.into();
    mutations.push(bad);
    let mut bad = original.clone();
    bad["records"][0]["payments"][0][0]["quantity"]["min"] = serde_json::json!([1, 0]);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["records"][0]["payments"][0][0]["quantity"]["min"] = serde_json::json!([2, 1]);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["records"][0]["source"] = "missing".into();
    mutations.push(bad);
    let mut bad = original.clone();
    bad["items"][0]["name"] = "invented identity".into();
    mutations.push(bad);
    let mut bad = original.clone();
    let duplicate = bad["records"][0].clone();
    bad["records"].as_array_mut().unwrap().push(duplicate);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["records"][4]["status"] = "clear".into();
    mutations.push(bad);
    for mutation in mutations {
        assert!(ItemValues::parse(&mutation.to_string(), &content.objects).is_err());
    }
    assert!(ItemValues::parse("not JSON", &content.objects).is_err());
}
