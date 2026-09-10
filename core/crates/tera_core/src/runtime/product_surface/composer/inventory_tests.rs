use super::super::tests::{AUTHOR, form, scope};
use super::*;
use crate::runtime::product_surface::{
    CreateUpdate, LocalNetworkId, Phase1AddCommand, ProfileMetadataCommand,
};
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_identity::PublicKey;
use radroots_storage::authored_draft::{
    AuthoredDraft, AuthoredDraftRevision, AuthoredDraftStage, AuthoredDraftStore,
};

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

fn id(value: u64) -> ComposerId {
    let mut bytes = [0; 16];
    bytes[8..].copy_from_slice(&value.to_be_bytes());
    ComposerId::new(bytes).unwrap()
}

fn key(entry: &ComposerListEntry) -> [u8; 16] {
    match entry {
        ComposerListEntry::Draft(summary) => *summary.id().as_bytes(),
        ComposerListEntry::Repair { draft_key, .. } => *draft_key,
    }
}

#[test]
fn continuation_is_bounded_versioned_canonical_and_independently_scope_bound() {
    let selected = scope();
    let expected = format!(
        "tera_composer_cursor_v1:{}{}{}",
        AUTHOR,
        "40da79f35835c1cd90c8d8ddb9712da70a9727a8d3d07f44f40235a5ed300800",
        "07".repeat(16)
    );
    assert_eq!(encode_cursor(&selected, [7; 16]).unwrap(), expected);
    assert_eq!(decode_cursor(&selected, &expected).unwrap(), [7; 16]);
    assert_eq!(
        decode_cursor(&selected, &encode_cursor(&selected, [0; 16]).unwrap()).unwrap(),
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
            decode_cursor(&selected, &invalid),
            Err(ComposerPersistenceError::InvalidCursor)
        );
    }
    let foreign = ComposerScope::new(
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap(),
        selected.local_network().clone(),
    );
    let other_context = ComposerScope::new(
        selected.author(),
        LocalNetworkId::new("other".into()).unwrap(),
    );
    for other in [&foreign, &other_context] {
        assert_eq!(
            decode_cursor(other, &expected),
            Err(ComposerPersistenceError::ScopeMismatch)
        );
    }
    assert_eq!(
        entry(
            AuthoredDraftQueryRecord::Corrupt {
                draft_key: [0; 16],
                revision: AuthoredDraftRevision::INITIAL
            },
            &selected
        ),
        ComposerListEntry::Repair {
            draft_key: [0; 16],
            revision: 1,
            reason: ComposerRepairReason::CorruptRecord
        }
    );
}

#[tokio::test]
async fn sqlite_inventory_pages_a_thousand_records_and_isolates_bad_or_foreign_payloads_after_restart()
 {
    let (_root, config, runtime) = runtime().await;
    let selected = scope();
    for value in 10..1010 {
        runtime
            .composer_create(
                &selected,
                id(value),
                ComposerEditSequence::INITIAL,
                form("PRIVATE partial -\n2026-"),
            )
            .await
            .unwrap();
    }
    let other = ComposerScope::new(
        selected.author(),
        LocalNetworkId::new("other".into()).unwrap(),
    );
    runtime
        .composer_create(
            &other,
            id(2000),
            ComposerEditSequence::INITIAL,
            form("foreign context"),
        )
        .await
        .unwrap();
    let digest = ComposerStorageRecord::scope_digest(&selected).unwrap();
    let unsupported = serde_json::to_vec(&serde_json::json!({"schema_version": 2, "schema_sha256": "future", "form": ["incompatible"]})).unwrap();
    for (value, schema, payload) in [
        (1010, COMPOSER_PAYLOAD_SCHEMA, unsupported),
        (
            1011,
            COMPOSER_PAYLOAD_SCHEMA,
            b"{malformed PRIVATE".to_vec(),
        ),
        (2001, "fixture.foreign.v1", b"foreign schema".to_vec()),
    ] {
        let draft = AuthoredDraft::initial(
            AuthoredDraftId::new(*id(value).as_bytes()).unwrap(),
            selected.author().into_bytes(),
            schema,
            payload,
            AuthoredDraftStage::Draft,
            None,
            1_800_000_000_000,
        )
        .unwrap()
        .with_scope(digest)
        .unwrap();
        AuthoredDraftStore::append_authored_draft(runtime.client.storage().unwrap(), draft, None)
            .await
            .unwrap();
    }
    let first = runtime.composer_list(&selected, 37, None).await.unwrap();
    assert_eq!(first.scope(), &selected);
    assert_eq!(first.entries().len(), 37);
    assert!(!format!("{first:?}").contains("PRIVATE"));
    let mut keys: Vec<_> = first.entries().iter().map(key).collect();
    let mut cursor = first.next_cursor().unwrap().to_owned();
    runtime
        .composer_create(
            &selected,
            id(1),
            ComposerEditSequence::INITIAL,
            form("inserted behind the cursor"),
        )
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    drop(runtime);

    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let mut repairs = Vec::new();
    loop {
        let page = runtime
            .composer_list(&selected, 37, Some(&cursor))
            .await
            .unwrap();
        assert!(page.entries().len() <= 37);
        for entry in page.entries() {
            keys.push(key(entry));
            match entry {
                ComposerListEntry::Draft(summary) => {
                    assert_eq!(summary.revision(), ComposerRevision::INITIAL);
                    assert_eq!(summary.edit_sequence(), ComposerEditSequence::INITIAL);
                    assert_eq!(
                        summary.command_type(),
                        AddCommandType::CreateFoodAvailability
                    );
                    assert!(summary.created_at_unix_ms() > 0);
                    assert!(summary.updated_at_unix_ms() >= summary.created_at_unix_ms());
                }
                ComposerListEntry::Repair { reason, .. } => repairs.push(*reason),
            }
        }
        let Some(next) = page.next_cursor() else {
            break;
        };
        cursor = next.to_owned();
    }
    assert_eq!(
        keys,
        (10..1012)
            .map(|value| *id(value).as_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        repairs,
        [
            ComposerRepairReason::UnsupportedSchema,
            ComposerRepairReason::CorruptRecord
        ]
    );
    let resnapshot = runtime.composer_list(&selected, 1, None).await.unwrap();
    assert_eq!(key(&resnapshot.entries()[0]), *id(1).as_bytes());
    assert_eq!(
        runtime
            .composer_list(&other, 37, first.next_cursor())
            .await
            .unwrap_err(),
        ComposerPersistenceError::ScopeMismatch
    );
    for limit in [0, COMPOSER_PAGE_LIMIT_MAX + 1] {
        assert_eq!(
            runtime
                .composer_list(&selected, limit, None)
                .await
                .unwrap_err(),
            ComposerPersistenceError::InvalidListRequest
        );
    }
    assert_eq!(
        runtime
            .composer_list(&selected, 1, Some("malformed"))
            .await
            .unwrap_err(),
        ComposerPersistenceError::InvalidCursor
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn aggregate_storage_byte_budget_returns_continuation_instead_of_losing_valid_composers() {
    let (_root, _config, runtime) = runtime().await;
    let selected = scope();
    let content = "\0".repeat(crate::runtime::product_surface::COMPOSER_CONTENT_MAX_BYTES);
    for value in 1..=12 {
        runtime
            .composer_create(
                &selected,
                id(value),
                ComposerEditSequence::INITIAL,
                form(&content),
            )
            .await
            .unwrap();
    }
    let first = runtime
        .composer_list(&selected, COMPOSER_PAGE_LIMIT_MAX, None)
        .await
        .unwrap();
    assert!(!first.entries().is_empty() && first.entries().len() < 12);
    let mut keys: Vec<_> = first.entries().iter().map(key).collect();
    let mut cursor = first.next_cursor().map(str::to_owned);
    assert!(cursor.is_some());
    while let Some(current) = cursor {
        let page = runtime
            .composer_list(&selected, COMPOSER_PAGE_LIMIT_MAX, Some(&current))
            .await
            .unwrap();
        keys.extend(page.entries().iter().map(key));
        cursor = page.next_cursor().map(str::to_owned);
    }
    assert_eq!(
        keys,
        (1..=12)
            .map(|value| *id(value).as_bytes())
            .collect::<Vec<_>>()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_reader_filters_schema_before_limit_and_preserves_direct_id_reopen() {
    let (_root, _config, runtime) = runtime().await;
    let selected = scope();
    let mut originals = Vec::new();
    for value in [1, 2] {
        let time = 1_700_000_000_000 + value * 1000;
        let saved = runtime
            .phase1_save_draft(
                [value as u8; 16],
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("legacy fixture").unwrap()),
                time / 1000,
                Vec::new(),
                None,
                time,
            )
            .await
            .unwrap();
        originals.push(saved.draft().clone());
    }
    for value in 1..=101 {
        runtime
            .composer_create(
                &selected,
                id(value),
                ComposerEditSequence::INITIAL,
                form("newer composer"),
            )
            .await
            .unwrap();
    }
    runtime
        .phase1_save_profile_metadata(
            ProfileMetadataCommand::new("grower".into(), None, None, None, None, None, None)
                .unwrap(),
        )
        .await
        .unwrap();
    // Compatibility now exposes a stable-ID head page, not the old updated-time window.
    let first = runtime.phase1_draft_heads(1).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].draft(), &originals[0]);
    let heads = runtime.phase1_draft_heads(100).await.unwrap();
    assert_eq!(heads.len(), 2);
    for (head, original) in heads.iter().zip(&originals) {
        assert_eq!(head.draft(), original);
    }
    for original in &originals {
        assert_eq!(
            runtime
                .phase1_draft_status(*original.draft_id().as_bytes())
                .await
                .unwrap()
                .draft(),
            original
        );
    }
    runtime.shutdown().await.unwrap();
}
