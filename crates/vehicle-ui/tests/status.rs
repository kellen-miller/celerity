use std::time::{Duration, Instant};

use vehicle_diagnostics::DiagnosticsSnapshot;
use vehicle_ui::{PresentationState, StatusObserver};

#[test]
fn observer_retains_two_misses_then_removes_values_on_third() {
    let started = Instant::now();
    let mut observer = StatusObserver::new();

    observer.observe(Err("not connected".to_owned()), started);
    let initial = observer.envelope(started);
    assert_eq!(initial.state, PresentationState::Unavailable);
    assert_eq!(initial.consecutive_failures, 1);
    assert_eq!(initial.last_success_age_ms, None);
    assert_eq!(initial.snapshot, None);

    observer.observe(Ok(fresh_snapshot()), started + Duration::from_millis(10));
    let current = observer.envelope(started + Duration::from_millis(25));
    assert_eq!(current.state, PresentationState::Current);
    assert_eq!(current.consecutive_failures, 0);
    assert_eq!(current.last_success_age_ms, Some(15));
    assert!(current.snapshot.is_some());

    observer.observe(
        Err("miss 1".to_owned()),
        started + Duration::from_millis(30),
    );
    assert_eq!(
        observer.envelope(started + Duration::from_millis(40)).state,
        PresentationState::Stale
    );
    observer.observe(
        Err("miss 2".to_owned()),
        started + Duration::from_millis(50),
    );
    assert!(
        observer
            .envelope(started + Duration::from_millis(60))
            .snapshot
            .is_some()
    );
    observer.observe(
        Err("miss 3".to_owned()),
        started + Duration::from_millis(70),
    );
    let unavailable = observer.envelope(started + Duration::from_millis(80));
    assert_eq!(unavailable.state, PresentationState::Unavailable);
    assert_eq!(unavailable.consecutive_failures, 3);
    assert_eq!(unavailable.last_success_age_ms, Some(70));
    assert_eq!(unavailable.snapshot, None);
}

#[test]
fn producer_age_controls_success_state_and_recovery() {
    let started = Instant::now();
    let mut observer = StatusObserver::new();
    let mut producer_stale = fresh_snapshot();
    producer_stale.runtime_update_age_ms = 101;

    observer.observe(Ok(producer_stale), started);
    let stale = observer.envelope(started);
    assert_eq!(stale.state, PresentationState::Stale);
    assert_eq!(stale.consecutive_failures, 0);

    observer.observe(Err("miss".to_owned()), started + Duration::from_millis(10));
    observer.observe(Ok(fresh_snapshot()), started + Duration::from_millis(20));
    let recovered = observer.envelope(started + Duration::from_millis(25));
    assert_eq!(recovered.state, PresentationState::Current);
    assert_eq!(recovered.consecutive_failures, 0);
}

const fn fresh_snapshot() -> DiagnosticsSnapshot {
    let mut snapshot = DiagnosticsSnapshot::startup_fallback();
    snapshot.runtime_update_age_ms = 20;
    snapshot.runtime_update_stale_after_ms = 100;
    snapshot
}
