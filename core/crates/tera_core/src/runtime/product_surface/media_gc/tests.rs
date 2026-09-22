use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    product_surface::*,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_identity::PublicKey;
use radroots_storage::authored_draft::{AuthoredDraftId, AuthoredDraftStage};

const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const OTHER: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
const NOW: u64 = 1_800_000_000_000;

async fn runtime(root: &std::path::Path, author: &str) -> crate::TeraRuntime {
    let config = MobileUserStoreConfig::from_encoded(
        root,
        author,
        &"01".repeat(32),
        NOW,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    RuntimeBuilder::new(config).build().await.unwrap()
}

fn scope(author: &str, context: &str) -> ComposerScope {
    ComposerScope::new(
        PublicKey::from_hex(author).unwrap(),
        LocalNetworkId::new(context.into()).unwrap(),
    )
}

fn form(hash: Option<&str>) -> ComposerPartialForm {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateUpdate);
    input.content = "cleanup fixture".into();
    if let Some(hash) = hash {
        input.media.push(ComposerMediaInput {
            opaque_reference: format!("media:{hash}"),
            sha256: hash.into(),
            media_type: "image/png".into(),
            byte_size: 100,
            width: 2,
            height: 2,
            alt: String::new(),
            prepared_at_unix_s: NOW / 1000,
        });
    }
    ComposerPartialForm::new(input).unwrap()
}

#[tokio::test]
async fn complete_sqlite_inventory_unions_other_accounts_contexts_and_pages() {
    let root = tempfile::tempdir().unwrap();
    for (author, hash) in [(AUTHOR, "a".repeat(64)), (OTHER, "b".repeat(64))] {
        let runtime = runtime(root.path(), author).await;
        for id in 1..=70 {
            runtime
                .composer_create(
                    &scope(author, if id % 2 == 0 { "nearby" } else { "other" }),
                    ComposerId::new([id; 16]).unwrap(),
                    ComposerEditSequence::INITIAL,
                    form(Some(&hash)),
                )
                .await
                .unwrap();
        }
        runtime.shutdown().await.unwrap();
    }
    let inventory = inspect_media_references(root.path(), vec!["c".repeat(64)])
        .await
        .unwrap();
    for byte in ["a", "b", "c"] {
        assert!(!inventory.permits_orphan(&byte.repeat(64), 1, NOW));
    }
    assert!(inventory.permits_orphan(&"d".repeat(64), 1, NOW));
}

#[tokio::test]
async fn removing_one_shared_reference_and_reediting_reserved_source_cannot_collect_bytes() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path(), AUTHOR).await;
    let scope = scope(AUTHOR, "nearby");
    let hash = "a".repeat(64);
    for id in [1, 2] {
        runtime
            .composer_create(
                &scope,
                ComposerId::new([id; 16]).unwrap(),
                ComposerEditSequence::INITIAL,
                form(Some(&hash)),
            )
            .await
            .unwrap();
    }
    runtime
        .composer_save(
            &scope,
            ComposerId::new([1; 16]).unwrap(),
            ComposerRevision::INITIAL,
            ComposerEditSequence::new(2).unwrap(),
            form(None),
        )
        .await
        .unwrap();
    let mut references = BTreeSet::new();
    collect_account(
        runtime.client.storage().unwrap(),
        scope.author().into_bytes(),
        &mut references,
        &mut InventoryBudget::default(),
        None,
    )
    .await
    .unwrap();
    assert!(references.contains(&hash));
    let request = SubmissionReservationRequest::new(
        SubmissionCommandId::new([7; 16]).unwrap(),
        scope.clone(),
        ComposerId::new([2; 16]).unwrap(),
        ComposerRevision::INITIAL,
    );
    runtime.submission_reserve(&request).await.unwrap();
    runtime
        .composer_save(
            &scope,
            request.composer_id(),
            ComposerRevision::INITIAL,
            ComposerEditSequence::new(2).unwrap(),
            form(None),
        )
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    let inventory = inspect_media_references(root.path(), Vec::new())
        .await
        .unwrap();
    assert!(!inventory.permits_orphan(&hash, 1, NOW));
    assert!(inventory.permits_orphan(&"b".repeat(64), 1, NOW));
}

#[tokio::test]
async fn unknown_schema_in_a_later_page_invalidates_the_entire_inventory() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path(), AUTHOR).await;
    let scope = scope(AUTHOR, "nearby");
    for id in 1..=33 {
        runtime
            .composer_create(
                &scope,
                ComposerId::new([id; 16]).unwrap(),
                ComposerEditSequence::INITIAL,
                form(None),
            )
            .await
            .unwrap();
    }
    let stored = AuthoredDraft::initial(
        AuthoredDraftId::new([254; 16]).unwrap(),
        scope.author().into_bytes(),
        "future.media.schema",
        b"{}".to_vec(),
        AuthoredDraftStage::Draft,
        None,
        NOW,
    )
    .unwrap();
    runtime
        .client
        .storage()
        .unwrap()
        .append_authored_draft(stored, None)
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    assert!(
        inspect_media_references(root.path(), Vec::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn missing_store_and_unknown_native_reference_never_prove_absence() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        inspect_media_references(root.path(), vec!["unknown".into()])
            .await
            .is_err()
    );
    std::fs::create_dir_all(root.path().join("radroots/users").join(AUTHOR)).unwrap();
    assert!(
        inspect_media_references(root.path(), Vec::new())
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn linked_account_directory_is_not_an_empty_account() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let users = root.path().join("radroots/users");
    std::fs::create_dir_all(&users).unwrap();
    std::os::unix::fs::symlink(other.path(), users.join(AUTHOR)).unwrap();
    assert!(
        inspect_media_references(root.path(), Vec::new())
            .await
            .is_err()
    );
}

#[test]
fn grace_is_inclusive_and_future_invalid_opaque_or_referenced_entries_are_retained() {
    let hash = "a".repeat(64);
    let inventory = MediaReferenceInventory {
        hashes: BTreeSet::from([hash.clone()]),
    };
    let orphan = "b".repeat(64);
    assert!(!inventory.permits_orphan(&hash, 1, NOW));
    assert!(inventory.permits_orphan(&orphan, NOW - MEDIA_ORPHAN_GRACE_MS, NOW));
    for modified in [0, NOW + 1, NOW - MEDIA_ORPHAN_GRACE_MS + 1] {
        assert!(!inventory.permits_orphan(&orphan, modified, NOW));
    }
    assert!(!inventory.permits_orphan(&orphan, 1, u64::MAX));
    assert!(inventory.permits_orphan(
        ".radroots_pending_12345678-1234-1234-1234-123456789abc",
        1,
        NOW
    ));
    for name in [
        ".radroots_pending_bad",
        "opaque",
        "../staged_blobs",
        &"A".repeat(64),
        ".radroots_pending_12345678-1234-1234-1234-123456789abC",
    ] {
        assert!(!inventory.permits_orphan(name, 1, NOW));
    }
}

#[test]
fn policy_contract_and_aggregate_budgets_are_enforced() {
    let policy: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/media_cleanup_policy_v1.json"
    )))
    .unwrap();
    for (key, value) in [
        ("orphan_grace_ms", MEDIA_ORPHAN_GRACE_MS),
        ("account_limit", MEDIA_ACCOUNT_LIMIT as u64),
        ("page_limit", MEDIA_PAGE_LIMIT.into()),
        ("page_budget", MEDIA_PAGE_BUDGET as u64),
        ("record_budget", MEDIA_RECORD_BUDGET as u64),
        ("payload_byte_budget", MEDIA_PAYLOAD_BYTE_BUDGET as u64),
        ("reference_budget", MEDIA_REFERENCE_BUDGET as u64),
        (
            "directory_entry_budget",
            MEDIA_DIRECTORY_ENTRY_BUDGET as u64,
        ),
        ("removal_budget", MEDIA_REMOVAL_BUDGET as u64),
    ] {
        assert_eq!(policy[key], value);
    }
    let stored = ComposerStorageRecord::initial(
        ComposerId::new([1; 16]).unwrap(),
        scope(AUTHOR, "nearby"),
        ComposerEditSequence::INITIAL,
        form(None),
        NOW,
    )
    .unwrap()
    .into_stored();
    let mut records = InventoryBudget {
        records: MEDIA_RECORD_BUDGET,
        ..Default::default()
    };
    assert!(records.record(&stored).is_err());
    let mut bytes = InventoryBudget {
        bytes: MEDIA_PAYLOAD_BYTE_BUDGET - stored.payload().len(),
        ..Default::default()
    };
    assert!(bytes.record(&stored).is_ok());
    assert!(bytes.record(&stored).is_err());
}
