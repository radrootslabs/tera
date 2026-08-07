use radroots_mobile_core::{
    RadrootsAppError,
    runtime::{
        builder::RuntimeBuilder,
        store::{MobileUserStoreConfig, ProtectedDataAvailability},
    },
};

mod support;

fn other_generation_store(root: &std::path::Path) -> MobileUserStoreConfig {
    MobileUserStoreConfig::from_encoded(
        root,
        support::PUBLIC_KEY,
        "0505050505050505050505050505050505050505050505050505050505050505",
        1_800_000_000_001,
        ProtectedDataAvailability::Available,
    )
    .expect("alternate store config")
}

#[tokio::test]
async fn cold_create_shutdown_and_reopen_preserve_the_sqlite_store() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = support::store(root.path());
    let runtime = RuntimeBuilder::new(store.clone())
        .build()
        .await
        .expect("cold create");
    let status = runtime.sdk_storage_status().await.expect("status");
    assert_eq!(
        runtime.authenticated_store_public_key_hex().as_deref(),
        Some(support::PUBLIC_KEY)
    );
    assert_eq!(status.backend, "sqlite");
    assert_eq!(status.open_mode, "create");
    assert_eq!(status.integrity, "unknown");
    assert!(store.owner_directory().join("runtime.sqlite").is_file());
    assert!(store.owner_directory().join("private.sqlite").is_file());
    runtime.shutdown().await.expect("shutdown");

    let reopened = RuntimeBuilder::new(store).build().await.expect("reopen");
    assert_eq!(
        reopened.sdk_storage_status().await.expect("status").backend,
        "sqlite"
    );
    reopened.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn one_authenticated_user_store_has_one_writable_runtime() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = support::store(root.path());
    let first = RuntimeBuilder::new(store.clone())
        .build()
        .await
        .expect("first runtime");
    let second = RuntimeBuilder::new(store.clone()).build().await;
    let Err(RadrootsAppError::Sdk { report }) = second else {
        panic!("second writable runtime must fail with a typed SDK error");
    };
    assert_eq!(report.code, "database_busy");
    assert!(report.retryable);

    first.shutdown().await.expect("first shutdown");
    let recovered = RuntimeBuilder::new(store)
        .build()
        .await
        .expect("writer lock recovery");
    recovered.shutdown().await.expect("recovered shutdown");
}

#[tokio::test]
async fn source_generation_mismatch_is_integrity_classified() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = support::store(root.path());
    let runtime = RuntimeBuilder::new(store).build().await.expect("runtime");
    runtime.shutdown().await.expect("shutdown");

    let result = RuntimeBuilder::new(other_generation_store(root.path()))
        .build()
        .await;
    let Err(RadrootsAppError::Sdk { report }) = result else {
        panic!("generation mismatch must fail with a typed SDK error");
    };
    assert_eq!(report.code, "storage_integrity_failed");
    assert!(!report.retryable);
}

#[tokio::test]
async fn unrecognized_sqlite_bytes_are_corruption_classified() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = support::store(root.path());
    let runtime = RuntimeBuilder::new(store.clone())
        .build()
        .await
        .expect("runtime");
    runtime.shutdown().await.expect("shutdown");
    std::fs::write(
        store.owner_directory().join("runtime.sqlite"),
        b"not a sqlite database",
    )
    .expect("replace runtime database with corrupt fixture");

    let result = RuntimeBuilder::new(store).build().await;
    let Err(RadrootsAppError::Sdk { report }) = result else {
        panic!("corrupt store must fail with a typed SDK error");
    };
    assert_eq!(report.code, "storage_integrity_failed");
    assert!(!report.retryable);
}

#[cfg(feature = "mobile-social")]
#[tokio::test]
async fn signer_selection_cannot_cross_the_authenticated_store_identity() {
    const OTHER_SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000002";

    let root = tempfile::tempdir().expect("tempdir");
    let runtime = RuntimeBuilder::new(support::store(root.path()))
        .build()
        .await
        .expect("runtime");
    let error = runtime
        .nostr_identity_restore_host_custody_secret(OTHER_SECRET.to_owned(), None, true)
        .expect_err("different identity must not select this user store");
    assert!(matches!(error, RadrootsAppError::Runtime(_)));
    assert!(!runtime.nostr_identity_has_selected_signing_identity());
    runtime.shutdown().await.expect("shutdown");
}
