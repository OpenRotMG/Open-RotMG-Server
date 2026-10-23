use super::*;
use std::time::Duration;

#[test]
fn move_pacing_exact_f32_boundary_without_epsilon_or_extra_credit() {
    let anchor = Instant::now();
    for credit in [0., MOVE_DISTANCE_CREDIT] {
        let budget = MoveBudget {
            anchor,
            validated: anchor,
            speed: 4.,
            credit,
            requested: credit + 0.5,
        };
        let boundary = budget.wait_boundary();
        assert_eq!(boundary.duration_since(anchor), Duration::from_millis(125));
        let due = budget.deadline(boundary).unwrap();
        let allowance = |at: Instant| 4. * at.duration_since(anchor).as_secs_f32() + credit;
        assert!(budget.requested <= allowance(due));
        assert!(budget.requested > allowance(due - Duration::from_nanos(1)));
        assert!(due <= boundary);
        let over = MoveBudget {
            requested: budget.requested.next_up(),
            ..budget
        };
        assert!(over.deadline(boundary).is_none());
    }
}

#[test]
fn move_pacing_retry_uses_original_boundary_not_new_speed_cap() {
    let anchor = Instant::now();
    let initial = MoveBudget {
        anchor,
        validated: anchor,
        speed: 8.,
        credit: 0.5,
        requested: 0.99,
    };
    let boundary = initial.wait_boundary();
    let slower = MoveBudget {
        validated: anchor + Duration::from_millis(60),
        speed: 4.,
        ..initial
    };
    assert!(slower.deadline(slower.wait_boundary()).is_some());
    assert!(slower.deadline(boundary).is_none());
    let faster = MoveBudget {
        validated: anchor + Duration::from_millis(50),
        speed: 9.,
        ..slower
    };
    assert!(faster.deadline(boundary).unwrap() <= boundary);
    let late = MoveBudget {
        validated: boundary + Duration::from_nanos(1),
        ..slower
    };
    assert!(late.deadline(boundary).is_none());
}
