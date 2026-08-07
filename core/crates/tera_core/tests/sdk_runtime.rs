use std::sync::Arc;

use radroots_mobile_core::{RadrootsAppError, RadrootsRuntime};

mod support;

#[tokio::test]
async fn runtime_is_send_sync_and_shares_one_sdk_lifecycle() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<RadrootsRuntime>();

    let (_root, runtime) = support::runtime().await;
    let runtime = Arc::new(runtime);
    let worker = {
        let runtime = Arc::clone(&runtime);
        std::thread::spawn(move || runtime.sdk_capabilities())
    };
    let capabilities = worker.join().expect("worker");
    assert!(
        capabilities
            .iter()
            .any(|capability| capability.id == "storage.canonical")
    );
    let first = runtime.shutdown().await.expect("first shutdown");
    let second = runtime.shutdown().await.expect("second shutdown");
    assert!(!first.already_closed);
    assert!(second.already_closed);
    assert!(runtime.info().sdk_closed);
}

#[tokio::test]
async fn operations_fail_safely_after_explicit_close() {
    let (_root, runtime) = support::runtime().await;
    assert_eq!(
        runtime.sdk_storage_status().await.expect("status").backend,
        "sqlite"
    );
    runtime.shutdown().await.expect("shutdown");
    assert!(matches!(
        runtime.sdk_storage_status().await,
        Err(RadrootsAppError::Sdk { .. })
    ));
}

#[tokio::test]
async fn dropping_unpolled_shutdown_has_no_effect_and_retry_closes() {
    let (_root, runtime) = support::runtime().await;
    drop(runtime.shutdown());
    assert!(!runtime.info().sdk_closed);
    assert!(!runtime.info().app.shutting_down);

    runtime.shutdown().await.expect("retry shutdown");
    assert!(runtime.info().sdk_closed);
    assert!(runtime.info().app.shutting_down);
}
