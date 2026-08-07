use std::{sync::Arc, time::Duration};

use radroots_mobile_ffi::RadrootsAppError;

mod support;

#[tokio::test]
async fn host_release_ordering_retains_close_and_finishes_within_deadline() {
    let (_root, runtime) = support::runtime().await;
    let host = Arc::new(runtime);
    let closing_owner = Arc::clone(&host);
    let close = tokio::spawn(async move { closing_owner.shutdown().await });
    drop(host);

    let result = tokio::time::timeout(Duration::from_secs(1), close)
        .await
        .expect("mobile shutdown exceeded its host deadline")
        .expect("shutdown task panicked")
        .expect("shutdown failed");
    assert_eq!(result.state, "closed");
    assert!(!result.already_closed);
}

#[tokio::test]
async fn concurrent_host_references_converge_and_repeated_close_is_idempotent() {
    let (_root, runtime) = support::runtime().await;
    let runtime = Arc::new(runtime);
    let first = Arc::clone(&runtime);
    let second = Arc::clone(&runtime);
    let (first, second) = tokio::join!(first.shutdown(), second.shutdown());

    for outcome in [&first, &second] {
        assert!(
            outcome.is_ok()
                || matches!(
                    outcome,
                    Err(RadrootsAppError::Failure { report })
                        if report.code == "client_close_in_progress"
                )
        );
    }
    let repeated = runtime.shutdown().await.expect("repeated close");
    assert!(repeated.already_closed);
}
