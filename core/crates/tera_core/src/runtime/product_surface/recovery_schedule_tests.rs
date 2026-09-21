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
async fn continuation_survives_reopen_and_refuses_stale_or_other_account_writes() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = RuntimeBuilder::new(config(dir.path(), AUTHOR))
        .build()
        .await
        .unwrap();
    let initial = runtime.native_recovery_schedule().await.unwrap();
    assert_eq!(initial.revision, 0);
    let (a, b) = tokio::join!(
        runtime.advance_native_recovery_schedule(initial.clone(), Some([1; 32])),
        runtime.advance_native_recovery_schedule(initial.clone(), Some([2; 32]))
    );
    let saved = match (a, b) {
        (Ok(a), Err(E::InvalidInventoryCursor)) => a,
        (Err(E::InvalidInventoryCursor), Ok(b)) => b,
        value => panic!("unexpected {value:?}"),
    };
    assert_eq!(saved.revision, 1);
    assert_eq!(
        runtime
            .advance_native_recovery_schedule(initial, None)
            .await
            .unwrap_err(),
        E::InvalidInventoryCursor
    );
    assert!(
        runtime
            .recovery_page(64, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
    assert!(runtime.native_recovery_schedule().await.is_err());
    drop(runtime);
    let runtime = RuntimeBuilder::new(config(dir.path(), AUTHOR))
        .build()
        .await
        .unwrap();
    assert_eq!(runtime.native_recovery_schedule().await.unwrap(), saved);
    let reset = runtime
        .advance_native_recovery_schedule(saved.clone(), None)
        .await
        .unwrap();
    assert_eq!(reset.revision, 2);
    assert_eq!(reset.after, None);
    assert_eq!(
        runtime
            .advance_native_recovery_schedule(reset.clone(), None)
            .await
            .unwrap(),
        reset
    );
    runtime.shutdown().await.unwrap();
    let other = RuntimeBuilder::new(config(dir.path(), OTHER))
        .build()
        .await
        .unwrap();
    assert_eq!(other.native_recovery_schedule().await.unwrap().revision, 0);
    assert_eq!(
        other
            .advance_native_recovery_schedule(saved, None)
            .await
            .unwrap_err(),
        E::InvalidInventoryCursor
    );
    other.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupt_continuation_is_preserved_without_touching_operations() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = RuntimeBuilder::new(config(dir.path(), AUTHOR))
        .build()
        .await
        .unwrap();
    let initial = runtime.native_recovery_schedule().await.unwrap();
    let author = initial.author;
    let store = runtime.client.storage().unwrap();
    let bytes = b"unknown schema".to_vec();
    ProjectionStore::put_projection_document(
        store,
        projection(author).unwrap(),
        generation().unwrap(),
        ProjectionDocument::new(KEY.into(), bytes.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        runtime.native_recovery_schedule().await.unwrap_err(),
        E::Corrupt
    );
    assert_eq!(
        runtime
            .advance_native_recovery_schedule(initial, None)
            .await
            .unwrap_err(),
        E::Corrupt
    );
    assert_eq!(
        ProjectionStore::projection_document(
            store,
            projection(author).unwrap(),
            generation().unwrap(),
            KEY.into()
        )
        .await
        .unwrap()
        .unwrap()
        .value(),
        bytes
    );
    assert!(
        runtime
            .recovery_page(64, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
}

#[test]
fn cursor_wire_is_bounded_canonical_and_versioned() {
    let original = NativeRecoverySchedule {
        schema_version: 1,
        author: [1; 32],
        revision: 1,
        after: Some([2; 32]),
    };
    let bytes = serde_json::to_vec(&original).unwrap();
    assert_eq!(decode(&bytes, [1; 32]).unwrap(), original);
    assert_eq!(decode(&bytes, [2; 32]), Err(E::Corrupt));
    assert_eq!(decode(&vec![b' '; MAX_BYTES + 1], [1; 32]), Err(E::Corrupt));
    for value in [
        NativeRecoverySchedule {
            schema_version: 2,
            ..original.clone()
        },
        NativeRecoverySchedule {
            revision: 0,
            ..original.clone()
        },
        NativeRecoverySchedule {
            revision: u64::MAX,
            ..original
        },
    ] {
        assert_eq!(
            decode(&serde_json::to_vec(&value).unwrap(), [1; 32]),
            Err(E::Corrupt)
        );
    }
    let mut noncanonical = bytes;
    noncanonical.push(b' ');
    assert_eq!(decode(&noncanonical, [1; 32]), Err(E::Corrupt));
}
