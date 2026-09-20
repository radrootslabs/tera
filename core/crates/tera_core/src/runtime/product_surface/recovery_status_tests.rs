use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};

const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const OTHER: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

fn config(root: &std::path::Path, author: &str) -> MobileUserStoreConfig {
    let value = MobileUserStoreConfig::from_encoded(
        root,
        author,
        &hex::encode([1; 32]),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(value.owner_directory()).unwrap();
    value
}

#[tokio::test]
async fn sqlite_status_restarts_idempotently_and_never_authorizes_an_effect() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = RuntimeBuilder::new(config(directory.path(), AUTHOR))
        .build()
        .await
        .unwrap();
    let key = [7; 32];
    assert!(
        runtime
            .report_native_recovery_status(key, NativeRecoveryReason::Resolved)
            .await
            .unwrap()
            .is_none()
    );
    let (a, b) = tokio::join!(
        runtime.report_native_recovery_status(key, NativeRecoveryReason::MissingParent),
        runtime.report_native_recovery_status(key, NativeRecoveryReason::MissingParent),
    );
    let original = a.unwrap().unwrap();
    assert_eq!(Some(original.clone()), b.unwrap());
    assert_eq!(original.revision, 1);
    runtime.shutdown().await.unwrap();
    assert!(runtime.native_recovery_status(key).await.is_err());
    drop(runtime);
    let runtime = RuntimeBuilder::new(config(directory.path(), AUTHOR))
        .build()
        .await
        .unwrap();
    assert_eq!(
        runtime.native_recovery_status(key).await.unwrap(),
        Some(original.clone())
    );
    assert_eq!(
        runtime
            .report_native_recovery_status(key, NativeRecoveryReason::MissingParent)
            .await
            .unwrap(),
        Some(original.clone())
    );
    let resolved = runtime
        .report_native_recovery_status(key, NativeRecoveryReason::Resolved)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.revision, 2);
    assert_eq!(
        resolved.first_observed_unix_ms,
        original.first_observed_unix_ms
    );
    assert!(resolved.updated_at_unix_ms >= original.updated_at_unix_ms);
    // A status document does not create any authored operation or draft.
    assert!(
        runtime
            .recovery_page(64, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let other = RuntimeBuilder::new(config(directory.path(), OTHER))
        .build()
        .await
        .unwrap();
    assert!(other.native_recovery_status(key).await.unwrap().is_none());
    other.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupt_status_is_preserved_without_blocking_an_unrelated_transfer() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = RuntimeBuilder::new(config(directory.path(), AUTHOR))
        .build()
        .await
        .unwrap();
    let author = runtime.store_public_key.unwrap().into_bytes();
    let store = runtime.client.storage().unwrap();
    let key = [3; 32];
    let corrupt = b"unknown recovery version".to_vec();
    ProjectionStore::put_projection_document(
        store,
        projection(author).unwrap(),
        generation().unwrap(),
        ProjectionDocument::new(hex::encode(key), corrupt.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        runtime
            .report_native_recovery_status(key, NativeRecoveryReason::MissingParent)
            .await
            .unwrap_err(),
        E::Corrupt
    );
    let retained = ProjectionStore::projection_document(
        store,
        projection(author).unwrap(),
        generation().unwrap(),
        hex::encode(key),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(retained.value(), corrupt);
    assert!(
        runtime
            .report_native_recovery_status([4; 32], NativeRecoveryReason::AssociationMismatch)
            .await
            .unwrap()
            .is_some()
    );
    runtime.shutdown().await.unwrap();
}

#[test]
fn status_wire_bounds_identity_time_and_canonical_encoding_are_exact() {
    let mut value = Wire {
        schema_version: 1,
        author: [1; 32],
        key: [2; 32],
        reason: NativeRecoveryReason::InvalidParent,
        revision: 1,
        first_observed_unix_ms: 10,
        updated_at_unix_ms: 11,
    };
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(decode(&bytes, value.author, value.key).is_ok());
    assert!(decode(&bytes, [9; 32], value.key).is_err());
    assert!(decode(&bytes, value.author, [9; 32]).is_err());
    let mut spaced = bytes.clone();
    spaced.push(b' ');
    assert!(decode(&spaced, value.author, value.key).is_err());
    assert!(
        decode(
            &vec![b' '; NATIVE_RECOVERY_STATUS_MAX_BYTES + 1],
            value.author,
            value.key
        )
        .is_err()
    );
    for revision in [0, u64::MAX] {
        value.revision = revision;
        assert!(
            decode(
                &serde_json::to_vec(&value).unwrap(),
                value.author,
                value.key
            )
            .is_err()
        );
    }
    value.revision = 1;
    for time in [0, 9, u64::MAX] {
        value.updated_at_unix_ms = time;
        assert!(
            decode(
                &serde_json::to_vec(&value).unwrap(),
                value.author,
                value.key
            )
            .is_err()
        );
    }
    value.updated_at_unix_ms = 11;
    value.schema_version = 2;
    assert!(
        decode(
            &serde_json::to_vec(&value).unwrap(),
            value.author,
            value.key
        )
        .is_err()
    );
}
