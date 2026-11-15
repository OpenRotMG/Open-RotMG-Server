use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub account: i32,
    pub reviewed_on: String,
}

// Shared so a reload reaches every clone of Content: worlds, store and HTTP handlers.
#[derive(Clone)]
pub struct Reviews(Arc<RwLock<Vec<Review>>>);
impl Reviews {
    pub fn new(rows: Vec<Review>) -> Self {
        Self(Arc::new(RwLock::new(rows)))
    }
    pub fn contains(&self, account: i32) -> bool {
        self.0
            .read()
            .expect("reviewed accounts lock poisoned")
            .iter()
            .any(|review| review.account == account)
    }
    pub fn rows(&self) -> Vec<Review> {
        self.0
            .read()
            .expect("reviewed accounts lock poisoned")
            .clone()
    }
    /// Appends to the operator file and applies it at once; returns false when already marked.
    pub fn mark(&self, path: &Path, account: i32, reviewed_on: String) -> Result<bool> {
        let mut rows = load(path)?;
        if rows.iter().any(|review| review.account == account) {
            return Ok(false);
        }
        rows.push(Review {
            account,
            reviewed_on,
        });
        let temp = path.with_extension("json.tmp");
        let mut file = std::fs::File::create_new(&temp)
            .with_context(|| format!("create {}", temp.display()))?;
        std::io::Write::write_all(&mut file, serde_json::to_string_pretty(&rows)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))?;
        self.replace(rows, path);
        Ok(true)
    }
    fn replace(&self, rows: Vec<Review>, path: &Path) {
        let ids = |rows: &[Review]| rows.iter().map(|r| r.account).collect::<BTreeSet<_>>();
        let (before, after) = (ids(&self.rows()), ids(&rows));
        *self.0.write().expect("reviewed accounts lock poisoned") = rows;
        tracing::info!(path=%path.display(), decisions=after.len(), added=?after.difference(&before).collect::<Vec<_>>(), removed=?before.difference(&after).collect::<Vec<_>>(), "server reviewed account decisions loaded");
    }
}

pub(crate) fn load(path: &Path) -> Result<Vec<Review>> {
    parse(&std::fs::read_to_string(path).context("read reviewed account decisions")?)
}

/// Loads the file now (failing startup if invalid), then applies later edits without a restart.
pub(crate) fn watch(path: PathBuf, reviews: Reviews) -> Result<()> {
    let mut seen = read(&path);
    reviews.replace(parse(&seen.clone().map_err(anyhow::Error::msg)?)?, &path);
    std::thread::Builder::new()
        .name("reviewed-accounts".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                poll(&path, &mut seen, &reviews);
            }
        })?;
    Ok(())
}

pub fn today() -> String {
    let days = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
        / 86_400) as i64;
    // Howard Hinnant's days-to-civil conversion: https://howardhinnant.github.io/date_algorithms.html#civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn read(path: &Path) -> std::result::Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read reviewed account decisions: {e}"))
}

fn poll(path: &Path, seen: &mut std::result::Result<String, String>, reviews: &Reviews) {
    let current = read(path);
    if current == *seen {
        return;
    }
    *seen = current.clone();
    match current
        .map_err(anyhow::Error::msg)
        .and_then(|text| parse(&text))
    {
        Ok(rows) => reviews.replace(rows, path),
        Err(error) => {
            tracing::error!(path=%path.display(), error=%format!("{error:#}"), "reviewed account decisions rejected; current marks unchanged")
        }
    }
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
    fn edits_apply_without_restart_and_invalid_edits_keep_current_marks() {
        let path = std::env::temp_dir().join(format!("reviews-{}.json", std::process::id()));
        std::fs::write(&path, "[]").unwrap();
        let reviews = Reviews::new(Vec::new());
        let mut seen = read(&path);
        std::fs::write(&path, r#"[{"account":7,"reviewed_on":"2026-11-15"}]"#).unwrap();
        poll(&path, &mut seen, &reviews);
        assert!(reviews.contains(7));
        std::fs::write(&path, "[").unwrap();
        poll(&path, &mut seen, &reviews);
        assert!(reviews.contains(7));
        std::fs::write(&path, "[]").unwrap();
        poll(&path, &mut seen, &reviews);
        assert!(!reviews.contains(7));
        std::fs::remove_file(&path).unwrap();
    }
    #[test]
    fn marks_keep_hand_edits_and_ignore_repeats() {
        let path = std::env::temp_dir().join(format!("reviews-{}.json", rand::random::<u64>()));
        std::fs::write(&path, r#"[{"account":8,"reviewed_on":"2026-11-06"}]"#).unwrap();
        let reviews = Reviews::new(Vec::new());
        assert!(reviews.mark(&path, 9, today()).unwrap());
        assert!(!reviews.mark(&path, 9, today()).unwrap());
        assert_eq!(
            load(&path)
                .unwrap()
                .iter()
                .map(|r| r.account)
                .collect::<Vec<_>>(),
            [8, 9]
        );
        assert!(reviews.contains(8) && reviews.contains(9) && valid_date(&today()));
        std::fs::remove_file(path).unwrap();
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
