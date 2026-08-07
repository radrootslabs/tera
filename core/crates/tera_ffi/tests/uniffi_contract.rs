use radroots_mobile_ffi::{
    ProtectedDataAvailability, RadrootsAppError, RadrootsRuntime, SdkErrorRecord,
};

mod support;

#[test]
fn swift_module_names_preserve_the_host_contract() {
    let config = include_str!("../uniffi.toml");
    assert_eq!(
        config,
        "[bindings.swift]\nmodule_name = \"RadrootsKitBindings\"\nffi_module_name = \"RadrootsFFI\"\n"
    );
}

#[tokio::test]
async fn protected_data_failure_is_typed_and_opens_no_store() {
    let root = tempfile::tempdir().expect("tempdir");
    support::prepare(root.path());
    let result = RadrootsRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.to_owned(),
        support::GENERATION.to_owned(),
        1_800_000_000_000,
        ProtectedDataAvailability::Unavailable,
    )
    .await;
    let Err(RadrootsAppError::Store { report }) = result else {
        panic!("protected data failure must remain typed across UniFFI");
    };
    assert_eq!(report.code, "protected_data_unavailable");
    assert!(report.retryable);
    assert!(
        !root
            .path()
            .join("radroots/users")
            .join(support::PUBLIC_KEY)
            .join("runtime.sqlite")
            .exists()
    );
}

#[tokio::test]
async fn final_mobile_abi_uses_async_sdk_dtos_and_versioned_errors() {
    let (_root, runtime) = support::runtime().await;
    let storage = runtime.sdk_storage_status().await.expect("storage status");
    assert_eq!(storage.backend, "sqlite");

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
