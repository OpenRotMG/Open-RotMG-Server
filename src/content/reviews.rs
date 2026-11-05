use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub account: i32,
    pub reviewed_on: String,
}

pub(crate) fn load(path: &Path) -> Result<Vec<Review>> {
    parse(&std::fs::read_to_string(path).context("read reviewed account decisions")?)
}

fn parse(text: &str) -> Result<Vec<Review>> {
    let reviews: Vec<Review> =
        serde_json::from_str(text).context("parse reviewed account decisions")?;
    let mut seen = BTreeSet::new();
    for review in &reviews {
        ensure!(
            review.account > 0 && seen.insert(review.account),
            "reviewed account IDs must be positive and unique"
        );
        ensure!(
            valid_date(&review.reviewed_on),
            "invalid review date for account {}",
            review.account
        );
    }
    Ok(reviews)
}

fn valid_date(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return false;
    }
    let year: u32 = text[..4].parse().expect("validated year digits");
    let month: usize = text[5..7].parse().expect("validated month digits");
    let day: u32 = text[8..].parse().expect("validated day digits");
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 => 28 + u32::from(leap),
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_unique_account_decisions_are_accepted() {
        assert!(parse("[]").unwrap().is_empty());
        let rows = parse(r#"[{"account":7,"reviewed_on":"2026-11-05"}]"#).unwrap();
        assert_eq!(rows[0].account, 7);
        for bad in [
            "null",
            r#"[{"account":0,"reviewed_on":"2026-11-05"}]"#,
            r#"[{"account":7,"reviewed_on":"2026-11-05","reported":true}]"#,
            r#"[{"account":7,"reviewed_on":"2026-11-05"},{"account":7,"reviewed_on":"2026-11-05"}]"#,
            r#"[{"account":7,"reviewed_on":"2026-02-30"}]"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn review_dates_are_calendar_dates_not_just_matching_strings() {
        for date in ["2024-02-29", "2000-02-29", "2026-11-05"] {
            assert!(valid_date(date));
        }
        for date in [
            "1900-02-29",
            "2026-02-29",
            "0000-01-01",
            "2026-13-01",
            "2026-01-00",
            "2026-1-01",
            "２０２６-11-05",
        ] {
            assert!(!valid_date(date), "{date}");
        }
    }
}
