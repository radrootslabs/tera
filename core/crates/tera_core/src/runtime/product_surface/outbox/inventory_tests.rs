use super::*;
use crate::runtime::builder::RuntimeBuilder;
use crate::runtime::product_surface::{
    ComposerEditSequence, ComposerFormInput, ComposerId, ComposerPartialForm, ComposerScope,
    CreateUpdate, LocalNetworkId,
};
use crate::runtime::store::{MobileUserStoreConfig, ProtectedDataAvailability};

const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

fn key(value: u128) -> [u8; 16] {
    value.to_be_bytes()
}

async fn runtime() -> (tempfile::TempDir, MobileUserStoreConfig, TeraRuntime) {
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
    (root, config, runtime)
}

#[test]
fn cursors_are_canonical_bounded_author_bound_and_allow_opaque_zero_locators() {
    let author = PublicKey::from_hex(AUTHOR).unwrap().into_bytes();
    let expected = format!("{CURSOR_PREFIX}{AUTHOR}{}", hex::encode(key(7)));
    assert_eq!(encode_cursor(author, key(7)), expected);
    assert_eq!(decode_cursor(author, &expected).unwrap(), key(7));
    assert_eq!(
        decode_cursor(author, &encode_cursor(author, [0; 16])).unwrap(),
        [0; 16]
    );
    for invalid in [
        String::new(),
        expected.to_uppercase(),
        expected.replace("_v1:", "_v2:"),
        format!("{expected}0"),
        expected[..expected.len() - 1].to_owned(),
        format!("{}g", &expected[..expected.len() - 1]),
        "x".repeat(100_000),
    ] {
        assert_eq!(
            decode_cursor(author, &invalid),
            Err(Phase1DraftError::InvalidInventoryCursor)
        );
    }
    assert_eq!(
        decode_cursor([9; 32], &expected),
        Err(Phase1DraftError::InvalidInventoryCursor)
    );
}

#[tokio::test]
async fn sqlite_inventory_pages_all_legacy_records_and_isolates_repairs_across_restart() {
    let (_root, config, runtime) = runtime().await;
    let seed = runtime
        .phase1_save_draft(
            key(1),
            Phase1AddCommand::CreateUpdate(CreateUpdate::new("legacy").unwrap()),
            1_800_000_000,
            Vec::new(),
            None,
            1_800_000_000_000,
        )
        .await
        .unwrap()
        .draft()
        .clone();
    for value in 2..=1002 {
        let payload = match value {
            1001 => br#"{"schema_version":2}"#.to_vec(),
            1002 => b"{}".to_vec(),
            _ => seed.payload().to_vec(),
        };
        let draft = AuthoredDraft::initial(
            AuthoredDraftId::new(key(value)).unwrap(),
            *seed.author(),
            DRAFT_PAYLOAD_SCHEMA,
            payload,
            AuthoredDraftStage::Draft,
            None,
            seed.created_at_unix_ms(),
        )
        .unwrap();
        runtime
            .storage()
            .unwrap()
            .append_authored_draft(draft, None)
            .await
            .unwrap();
    }
    add_foreign_schemas(&runtime).await;
    let first = runtime.phase1_draft_page(37, None).await.unwrap();
    assert_eq!(first.author(), seed.author());
    assert_eq!(first.entries().len(), 37);
    let cursor = first.next_cursor().unwrap().to_owned();
    let mut entries = first.entries().to_vec();
    runtime.shutdown().await.unwrap();
    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let mut cursor = Some(cursor);
    while let Some(current) = cursor {
        let page = runtime.phase1_draft_page(37, Some(&current)).await.unwrap();
        assert!(!page.entries().is_empty() && page.entries().len() <= 37);
        entries.extend_from_slice(page.entries());
        cursor = page.next_cursor().map(str::to_owned);
    }
    assert_eq!(entries.len(), 1002);
    for (index, entry) in entries[..1000].iter().enumerate() {
        let Phase1DraftListEntry::Draft(summary) = entry else {
            panic!("valid legacy row was hidden")
        };
        assert_eq!(*summary.draft_id().as_bytes(), key(index as u128 + 1));
        assert_eq!(summary.revision(), AuthoredDraftRevision::INITIAL);
        assert_eq!(summary.command_type(), AddCommandType::CreateUpdate);
        assert_eq!(summary.kind(), Phase1DraftKind::Add);
        assert_eq!(summary.state(), Phase1OutboxState::Draft);
        assert!(!summary.has_form() && !summary.is_revision());
        assert_eq!(summary.created_at_unix_ms(), seed.created_at_unix_ms());
        assert_eq!(summary.updated_at_unix_ms(), seed.updated_at_unix_ms());
    }
    assert_eq!(
        entries[1000],
        Phase1DraftListEntry::Repair {
            draft_key: key(1001),
            revision: 1,
            reason: Phase1DraftRepairReason::UnsupportedSchema
        }
    );
    assert_eq!(
        entries[1001],
        Phase1DraftListEntry::Repair {
            draft_key: key(1002),
            revision: 1,
            reason: Phase1DraftRepairReason::CorruptRecord
        }
    );
    assert_eq!(
        runtime.phase1_draft_status(key(1)).await.unwrap().draft(),
        &seed
    );
    assert_eq!(
        runtime.phase1_draft_status(key(9999)).await.unwrap_err(),
        Phase1DraftError::NotFound
    );
    for limit in [0, AUTHORED_DRAFT_QUERY_LIMIT_MAX + 1] {
        assert_eq!(
            runtime.phase1_draft_page(limit, None).await.unwrap_err(),
            Phase1DraftError::InvalidInventoryRequest
        );
    }
    assert_eq!(
        runtime
            .phase1_draft_page(1, Some("invalid"))
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidInventoryCursor
    );
    runtime.shutdown().await.unwrap();
}

async fn add_foreign_schemas(runtime: &TeraRuntime) {
    runtime
        .phase1_save_profile_metadata(
            ProfileMetadataCommand::new("grower".into(), None, None, None, None, None, None)
                .unwrap(),
        )
        .await
        .unwrap();
    let scope = ComposerScope::new(
        PublicKey::from_hex(AUTHOR).unwrap(),
        LocalNetworkId::new("default".into()).unwrap(),
    );
    runtime
        .composer_create(
            &scope,
            ComposerId::new(key(3000)).unwrap(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(ComposerFormInput::empty(AddCommandType::CreateEvent))
                .unwrap(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn opaque_corrupt_locators_remain_repairs_without_becoming_editing_ids() {
    let (_root, _config, runtime) = runtime().await;
    assert_eq!(
        runtime
            .draft_inventory_entry(AuthoredDraftQueryRecord::Corrupt {
                draft_key: [0; 16],
                revision: AuthoredDraftRevision::INITIAL,
            })
            .await,
        Phase1DraftListEntry::Repair {
            draft_key: [0; 16],
            revision: 1,
            reason: Phase1DraftRepairReason::CorruptRecord
        }
    );
    runtime.shutdown().await.unwrap();
}
