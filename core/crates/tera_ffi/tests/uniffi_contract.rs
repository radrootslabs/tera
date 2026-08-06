use radroots_mobile_ffi::{RadrootsAppError, RadrootsRuntime, SdkErrorRecord};

#[tokio::test]
async fn final_mobile_abi_uses_async_sdk_dtos_and_versioned_errors() {
    let runtime = RadrootsRuntime::new().expect("runtime");
    let storage = runtime.sdk_storage_status().await.expect("storage status");
    assert_eq!(storage.backend, "memory");

    runtime.shutdown().await.expect("shutdown");
    let error = runtime
        .sdk_storage_status()
        .await
        .expect_err("closed client must reject operations");
    let RadrootsAppError::Sdk {
        report:
            SdkErrorRecord {
                schema_version,
                code,
                class,
                retryable,
                message,
                ..
            },
    } = error
    else {
        panic!("expected versioned SDK error record");
    };
    assert_eq!(schema_version, 1);
    assert_eq!(code, "client_closed");
    assert_eq!(class, "runtime");
    assert!(!retryable);
    assert_eq!(message, "SDK client is closed");
}
