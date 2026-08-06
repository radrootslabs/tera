use std::sync::Arc;

use radroots_mobile_core::{RadrootsAppError, RadrootsRuntime};

#[tokio::test]
async fn runtime_is_send_sync_and_shares_one_sdk_lifecycle() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<RadrootsRuntime>();

    let runtime = Arc::new(RadrootsRuntime::new().expect("runtime"));
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
    let runtime = RadrootsRuntime::new().expect("runtime");
    assert_eq!(
        runtime.sdk_storage_status().await.expect("status").backend,
        "memory"
    );
    runtime.shutdown().await.expect("shutdown");
    assert!(matches!(
        runtime.sdk_storage_status().await,
        Err(RadrootsAppError::Sdk { .. })
    ));
}

#[tokio::test]
async fn dropping_unpolled_shutdown_has_no_effect_and_retry_closes() {
    let runtime = RadrootsRuntime::new().expect("runtime");
    drop(runtime.shutdown());
    assert!(!runtime.info().sdk_closed);
    assert!(!runtime.info().app.shutting_down);

    runtime.shutdown().await.expect("retry shutdown");
    assert!(runtime.info().sdk_closed);
    assert!(runtime.info().app.shutting_down);
}
