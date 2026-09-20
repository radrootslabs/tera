use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    product_surface::{CreateUpdate, Phase1AddCommand},
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_storage::authored_draft::AuthoredDraftStage;

const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
fn key(value: u128) -> [u8; 16] {
    value.to_be_bytes()
}

#[test]
fn recovery_cursors_are_canonical_author_bound_and_bounded() {
    let author = [7; 32];
    let cursor = encode_cursor(author, key(1));
    assert_eq!(decode_cursor(author, &cursor).unwrap(), key(1));
    for invalid in [
        cursor.to_uppercase(),
        format!("{cursor}0"),
        cursor.replace("v1:", "v2:"),
        "x".repeat(4096),
    ] {
        assert_eq!(
            decode_cursor(author, &invalid),
            Err(E::InvalidInventoryCursor)
        );
    }
    assert_eq!(
        decode_cursor([8; 32], &cursor),
        Err(E::InvalidInventoryCursor)
    );
    assert_eq!(
        decode_cursor(author, &encode_cursor(author, [0; 16])).unwrap(),
        [0; 16]
    );
}

#[tokio::test]
async fn thousand_equal_time_recovery_positions_resume_and_revisit_changed_work() {
    let root = tempfile::tempdir().unwrap();
    let config = MobileUserStoreConfig::from_encoded(
        root.path(),
        AUTHOR,
        &"02".repeat(32),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    let seed = runtime
        .phase1_save_draft(
            key(2),
            Phase1AddCommand::CreateUpdate(CreateUpdate::new("recovery fixture").unwrap()),
            1_800_000_000,
            vec![],
            None,
            1_800_000_000_000,
        )
        .await
        .unwrap()
        .draft()
        .clone();
    let store = runtime.client.storage().unwrap();
    for number in 3..=1001 {
        let row = AuthoredDraft::initial(
            AuthoredDraftId::new(key(number)).unwrap(),
            *seed.author(),
            seed.payload_schema(),
            seed.payload().to_vec(),
            AuthoredDraftStage::Draft,
            None,
            seed.created_at_unix_ms(),
        )
        .unwrap();
        store.append_authored_draft(row, None).await.unwrap();
    }
    for (number, schema, author) in [
        (1002, seed.payload_schema(), *seed.author()),
        (1003, "fixture.unknown.v1", *seed.author()),
        (1004, seed.payload_schema(), [8; 32]),
    ] {
        let row = AuthoredDraft::initial(
            AuthoredDraftId::new(key(number)).unwrap(),
            author,
            schema,
            b"PRIVATE corrupt fixture".to_vec(),
            AuthoredDraftStage::Draft,
            None,
            seed.created_at_unix_ms(),
        )
        .unwrap();
        store.append_authored_draft(row, None).await.unwrap();
    }
    let first = runtime.recovery_page(37, None).await.unwrap();
    assert_eq!(first.scanned, 37);
    let cursor = first.next_cursor.clone().unwrap();
    let mut entries = first.entries;
    for number in [2, 1000] {
        let original = store
            .authored_draft_head(AuthoredDraftId::new(key(number)).unwrap())
            .await
            .unwrap()
            .unwrap();
        let next = original
            .successor(
                original.payload().to_vec(),
                original.stage(),
                None,
                original.updated_at_unix_ms() + 1,
            )
            .unwrap();
        store
            .append_authored_draft(next, Some(original.revision()))
            .await
            .unwrap();
    }
    let inserted = AuthoredDraft::initial(
        AuthoredDraftId::new(key(1)).unwrap(),
        *seed.author(),
        seed.payload_schema(),
        seed.payload().to_vec(),
        AuthoredDraftStage::Draft,
        None,
        seed.created_at_unix_ms(),
    )
    .unwrap();
    store.append_authored_draft(inserted, None).await.unwrap();
    runtime.shutdown().await.unwrap();
    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let mut continuation = Some(cursor);
    while let Some(cursor) = continuation {
        let page = runtime.recovery_page(37, Some(&cursor)).await.unwrap();
        assert!(page.scanned <= 37);
        continuation = page.next_cursor;
        entries.extend(page.entries);
    }
    assert_eq!(entries.len(), 1002);
    for (index, entry) in entries[..1000].iter().enumerate() {
        assert_eq!(entry.key, key(index as u128 + 2));
        assert_eq!(entry.owner, RecoveryOwner::Legacy);
    }
    assert_eq!(entries[998].revision, 2);
    assert!(matches!(
        entries[1000].owner,
        RecoveryOwner::Repair(Phase1DraftRepairReason::CorruptRecord)
    ));
    assert!(matches!(
        entries[1001].owner,
        RecoveryOwner::Repair(Phase1DraftRepairReason::UnsupportedSchema)
    ));
    assert!(!format!("{entries:?}").contains("PRIVATE"));
    let fresh = runtime.recovery_page(2, None).await.unwrap();
    assert_eq!(fresh.entries[0].key, key(1));
    assert_eq!(fresh.entries[1].revision, 2);
    let exact = runtime.recovery_parent(key(1000)).await.unwrap().unwrap();
    assert_eq!(exact.revision, 2);
    assert_eq!(exact.owner, RecoveryOwner::Legacy);
    assert!(runtime.recovery_parent(key(9999)).await.unwrap().is_none());
    assert_eq!(runtime.recovery_parent(key(1004)).await, Err(E::Corrupt));
    for limit in [0, RECOVERY_PAGE_LIMIT_MAX + 1] {
        assert_eq!(
            runtime.recovery_page(limit, None).await,
            Err(E::InvalidInventoryRequest)
        );
    }
    runtime.shutdown().await.unwrap();
}
