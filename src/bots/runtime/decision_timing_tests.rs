use super::record_decision_phase_elapsed;
use crate::performance::Window;
use std::time::Duration;

fn reportable_window() -> Window {
    let mut window = Window::default();
    window.record("test_report_seed", Duration::from_secs(6));
    window
}

#[test]
fn decision_phase_elapsed_separates_awaits_without_repartitioning_inclusive_total() {
    let mut window = reportable_window();
    let value = String::from("unchanged decision");
    let identity = value.as_ptr();
    let value = record_decision_phase_elapsed(
        &mut window,
        "decide_sync_elapsed",
        Duration::from_millis(5),
        Ok(value),
    )
    .unwrap();
    assert_eq!(value.as_ptr(), identity);
    record_decision_phase_elapsed(
        &mut window,
        "market_revoke_await_elapsed",
        Duration::from_millis(40),
        Ok(true),
    )
    .unwrap();
    record_decision_phase_elapsed(
        &mut window,
        "mind_store_await_elapsed",
        Duration::from_millis(400),
        Ok(7_i64),
    )
    .unwrap();
    window.record("decision", Duration::from_millis(500));
    let timings = &window.report().unwrap()["timings"];
    for (name, expected) in [
        ("decide_sync_elapsed", 5_000),
        ("market_revoke_await_elapsed", 40_000),
        ("mind_store_await_elapsed", 400_000),
        ("decision", 500_000),
    ] {
        assert_eq!(timings[name]["count"], 1);
        assert_eq!(timings[name]["mean_us"], expected);
    }
    let measured = [
        "decide_sync_elapsed",
        "market_revoke_await_elapsed",
        "mind_store_await_elapsed",
    ]
    .into_iter()
    .map(|name| timings[name]["mean_us"].as_u64().unwrap())
    .sum::<u64>();
    assert!(measured < timings["decision"]["mean_us"].as_u64().unwrap());
}

#[test]
fn decision_phase_elapsed_preserves_false_revoke_and_does_not_fabricate_store_sample() {
    let mut window = reportable_window();
    record_decision_phase_elapsed(
        &mut window,
        "decide_sync_elapsed",
        Duration::from_millis(2),
        Ok(()),
    )
    .unwrap();
    let revoked = record_decision_phase_elapsed(
        &mut window,
        "market_revoke_await_elapsed",
        Duration::from_millis(20),
        Ok(false),
    )
    .unwrap();
    assert!(!revoked);
    let timings = &window.report().unwrap()["timings"];
    assert_eq!(timings["decide_sync_elapsed"]["mean_us"], 2_000);
    assert_eq!(timings["market_revoke_await_elapsed"]["mean_us"], 20_000);
    assert!(timings.get("mind_store_await_elapsed").is_none());
    assert!(timings.get("decision").is_none());
}

#[test]
fn decision_phase_elapsed_preserves_error_identity_context_and_earlier_samples() {
    for failed_phase in ["market_revoke_await_elapsed", "mind_store_await_elapsed"] {
        let mut window = reportable_window();
        record_decision_phase_elapsed(
            &mut window,
            "decide_sync_elapsed",
            Duration::from_millis(3),
            Ok(()),
        )
        .unwrap();
        let error = anyhow::Error::new(std::io::Error::from_raw_os_error(5))
            .context("own decision phase failed");
        let identity = error.downcast_ref::<std::io::Error>().unwrap() as *const _;
        let context = format!("{error:#}");
        let result: anyhow::Result<i64> = record_decision_phase_elapsed(
            &mut window,
            failed_phase,
            Duration::from_millis(30),
            Err(error),
        );
        let error = result.unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap() as *const _,
            identity
        );
        assert_eq!(format!("{error:#}"), context);
        let timings = &window.report().unwrap()["timings"];
        assert_eq!(timings["decide_sync_elapsed"]["mean_us"], 3_000);
        assert_eq!(timings[failed_phase]["count"], 1);
        assert_eq!(timings[failed_phase]["mean_us"], 30_000);
        assert!(timings.get("decision").is_none());
        if failed_phase == "market_revoke_await_elapsed" {
            assert!(timings.get("mind_store_await_elapsed").is_none());
        }
    }
}
