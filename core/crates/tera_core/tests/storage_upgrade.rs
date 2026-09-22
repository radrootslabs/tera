use tera_core::{
    TeraAppError,
    runtime::{
        builder::RuntimeBuilder,
        store::{MobileUserStoreConfig, ProtectedDataAvailability},
    },
};

mod support;

async fn initialized() -> (tempfile::TempDir, MobileUserStoreConfig) {
    let root = tempfile::tempdir().expect("synthetic store");
    let store = support::store(root.path());
    RuntimeBuilder::new(store.clone())
        .build()
        .await
        .expect("initialize fixture")
        .shutdown()
        .await
        .expect("close fixture");
    (root, store)
}

async fn missing_member_is_not_recreated(missing: &str, retained: &str) {
    let (_root, store) = initialized().await;
    let missing = store.owner_directory().join(missing);
    let retained = store.owner_directory().join(retained);
    let original = std::fs::read(&retained).expect("retained member");
    std::fs::remove_file(&missing).expect("remove only synthetic fixture member");
    let result = RuntimeBuilder::new(store).build().await;
    if let Ok(runtime) = result {
        runtime
            .shutdown()
            .await
            .expect("close unexpected replacement");
        panic!("an initialized store must not recreate its missing database member");
    }
    let Err(TeraAppError::Store { report }) = result else {
        panic!("incomplete storage must produce its typed recovery report");
    };
    assert_eq!(report.code, "store_incomplete");
    assert!(!report.retryable);
    assert!(report.recovery_actions.is_empty());
    assert!(!report.message.contains(missing.to_str().unwrap()));
    assert_eq!(
        tera_core::error::recovery::classify(report.schema_version, &report.code).disposition,
        tera_core::error::recovery::RecoveryDisposition::StorageFailure
    );
    assert!(!missing.exists(), "no empty replacement may be installed");
    assert_eq!(std::fs::read(retained).unwrap(), original);
}

#[tokio::test]
async fn missing_runtime_member_requires_recovery_without_replacement() {
    missing_member_is_not_recreated("runtime.sqlite", "private.sqlite").await;
}

#[tokio::test]
async fn missing_private_member_requires_recovery_without_replacement() {
    missing_member_is_not_recreated("private.sqlite", "runtime.sqlite").await;
}

#[tokio::test]
async fn corrupt_member_and_its_companion_are_retained_on_repeated_startup() {
    let (_root, store) = initialized().await;
    let path = store.owner_directory().join("runtime.sqlite");
    let companion = store.owner_directory().join("private.sqlite");
    let retained = std::fs::read(&companion).unwrap();
    let corrupt = b"synthetic incompatible database bytes: preserve original";
    std::fs::write(&path, corrupt).unwrap();
    for _ in 0..2 {
        let Err(TeraAppError::Sdk { report }) = RuntimeBuilder::new(store.clone()).build().await
        else {
            panic!("corrupt data must remain a typed failure");
        };
        assert_eq!(report.code, "storage_integrity_failed");
        assert!(!report.retryable);
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);
        assert_eq!(std::fs::read(&companion).unwrap(), retained);
    }
}

#[tokio::test]
async fn protected_data_absence_preserves_existing_members_and_identity() {
    let (root, store) = initialized().await;
    let runtime = store.owner_directory().join("runtime.sqlite");
    let private = store.owner_directory().join("private.sqlite");
    let before = (
        std::fs::read(&runtime).unwrap(),
        std::fs::read(&private).unwrap(),
    );
    let locked = MobileUserStoreConfig::from_encoded(
        root.path(),
        support::PUBLIC_KEY,
        support::GENERATION,
        1_800_000_000_000,
        ProtectedDataAvailability::Unavailable,
    )
    .unwrap();
    let Err(TeraAppError::Store { report }) = RuntimeBuilder::new(locked).build().await else {
        panic!("protected data must fail before opening storage");
    };
    assert_eq!(report.code, "protected_data_unavailable");
    assert!(report.retryable);
    assert_eq!(std::fs::read(runtime).unwrap(), before.0);
    assert_eq!(std::fs::read(private).unwrap(), before.1);
    let reopened = RuntimeBuilder::new(store).build().await.unwrap();
    assert_eq!(
        reopened.authenticated_store_public_key_hex().as_deref(),
        Some(support::PUBLIC_KEY)
    );
    reopened.shutdown().await.unwrap();
}

fn change_fixture_header(path: &std::path::Path, offset: usize, value: u32) {
    let mut bytes = std::fs::read(path).expect("closed synthetic SQLite fixture");
    assert!(bytes.starts_with(b"SQLite format 3\0"));
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    std::fs::write(path, bytes).expect("alter synthetic header only");
}

async fn refused_header_preserves_pair(member: &str, offset: usize, value: u32, code: &str) {
    use tera_core::error::recovery::{RecoveryDisposition, classify};
    let (_root, store) = initialized().await;
    change_fixture_header(&store.owner_directory().join(member), offset, value);
    let runtime_path = store.owner_directory().join("runtime.sqlite");
    let private_path = store.owner_directory().join("private.sqlite");
    let runtime_bytes = std::fs::read(&runtime_path).unwrap();
    let private_bytes = std::fs::read(&private_path).unwrap();
    for _ in 0..2 {
        let Err(TeraAppError::Sdk { report }) = RuntimeBuilder::new(store.clone()).build().await
        else {
            panic!("incompatible storage must remain a typed failure");
        };
        assert_eq!(report.code, code);
        assert!(!report.retryable);
        assert_eq!(
            classify(report.schema_version, &report.code).disposition,
            if code == "schema_too_new" {
                RecoveryDisposition::UnsupportedVersion
            } else {
                RecoveryDisposition::StorageFailure
            }
        );
        assert!(
            std::fs::read(&runtime_path).unwrap() == runtime_bytes,
            "runtime changed after refused startup"
        );
        assert!(
            std::fs::read(&private_path).unwrap() == private_bytes,
            "private changed after refused startup"
        );
    }
}

#[tokio::test]
async fn future_member_versions_preserve_both_files_and_require_supported_software() {
    for member in ["runtime.sqlite", "private.sqlite"] {
        refused_header_preserves_pair(member, 60, 999, "schema_too_new").await;
    }
}

#[tokio::test]
async fn foreign_member_namespaces_preserve_both_files_and_require_recovery() {
    for member in ["runtime.sqlite", "private.sqlite"] {
        refused_header_preserves_pair(member, 68, 42, "storage_integrity_failed").await;
    }
}
