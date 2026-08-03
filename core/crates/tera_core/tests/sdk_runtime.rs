use std::sync::Arc;

use radroots_app_core::{RadrootsAppError, RadrootsRuntime};

#[test]
fn runtime_is_send_sync_and_shares_one_sdk_lifecycle() {
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
    runtime.stop();
    runtime.stop();
    assert!(runtime.info().sdk_closed);
}

#[test]
fn operations_fail_safely_after_explicit_close() {
    let runtime = RadrootsRuntime::new().expect("runtime");
    assert_eq!(
        runtime.sdk_storage_status().expect("status").backend,
        "memory"
    );
    runtime.stop();
    assert!(matches!(
        runtime.sdk_storage_status(),
        Err(RadrootsAppError::Runtime(_))
    ));
}
