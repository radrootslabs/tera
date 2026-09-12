use std::collections::BTreeMap;

use radroots_storage::authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage};

use super::{operation_test_support as operation, test_support::*, *};
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerPartialForm, ComposerStorageRecord, CreateUpdate,
    Phase1AddCommand, Phase1DraftError, Phase1DraftRepairReason, Phase1OutboxState,
};

fn numbered_request(value: u64) -> SubmissionReservationRequest {
    let mut command = [0; 16];
    command[8..].copy_from_slice(&value.to_be_bytes());
    SubmissionReservationRequest::new(
        SubmissionCommandId::new(command).unwrap(),
        scope(AUTHOR, "nearby"),
        ComposerId::new([3; 16]).unwrap(),
        ComposerRevision::INITIAL,
    )
}

fn key(entry: &SubmissionListEntry) -> [u8; 16] {
    match entry {
        SubmissionListEntry::Submission(value) => *value.reservation_id().as_bytes(),
        SubmissionListEntry::Repair {
            reservation_key, ..
        } => *reservation_key,
    }
}

#[tokio::test]
async fn memory_and_sqlite_pages_recover_scoped_operations_and_isolate_bad_records() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = operation::CountingSigner::new();
        let mut runtime = operation::runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
        let selected = scope(AUTHOR, "nearby");
        let source = runtime
            .composer_create(
                &selected,
                numbered_request(1).composer_id(),
                ComposerEditSequence::INITIAL,
                ComposerPartialForm::new(input(AddCommandType::CreateUpdate)).unwrap(),
            )
            .await
            .unwrap();
        let mut expected = BTreeMap::new();
        for number in 1..=257 {
            let request = numbered_request(number);
            let reservation = runtime.submission_reserve(&request).await.unwrap();
            let state = if number <= 2 {
                let receipt = runtime.submission_prepare(&request, vec![]).await.unwrap();
                SubmissionSummaryState::Operation {
                    intent_id: receipt.intent_id(),
                    operation_id: receipt.operation_id(),
                    revision: 1,
                    state: Phase1OutboxState::ReadyToSign,
                }
            } else {
                SubmissionSummaryState::Reserved
            };
            expected.insert(*reservation.reservation_id().as_bytes(), (request, state));
        }
        // A reservation with an existing intent but no atomic receipt is repair
        // evidence, not an unsubmitted draft that may be started again.
        let dangling = numbered_request(258);
        let reservation = runtime.submission_reserve(&dangling).await.unwrap();
        let digest = ComposerStorageRecord::scope_digest(&selected).unwrap();
        let store = runtime.client.storage().unwrap();
        store
            .append_authored_draft(
                AuthoredDraft::initial(
                    intent::intent_id(&dangling).unwrap(),
                    selected.author().into_bytes(),
                    SUBMISSION_INTENT_PAYLOAD_SCHEMA,
                    b"unconfirmed intent".to_vec(),
                    AuthoredDraftStage::MediaPreparing,
                    None,
                    NOW,
                )
                .unwrap()
                .with_scope(digest)
                .unwrap(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            runtime
                .submission_operation_status(&dangling)
                .await
                .unwrap_err(),
            SubmissionOperationError::Corrupt
        );
        let mut expected_repairs = BTreeMap::from([(
            *reservation.reservation_id().as_bytes(),
            Phase1DraftRepairReason::CorruptRecord,
        )]);
        for (id, author, record_scope, payload, included) in [
            (
                [0xef; 16],
                selected.author(),
                digest,
                b"{PRIVATE malformed".to_vec(),
                Some(Phase1DraftRepairReason::CorruptRecord),
            ),
            (
                [0xf0; 16],
                selected.author(),
                digest,
                br#"{"schema_version":2,"schema_sha256":"future"}"#.to_vec(),
                Some(Phase1DraftRepairReason::UnsupportedSchema),
            ),
            (
                [0xf1; 16],
                selected.author(),
                ComposerStorageRecord::scope_digest(&scope(AUTHOR, "other")).unwrap(),
                b"PRIVATE other context".to_vec(),
                None,
            ),
            (
                [0xf2; 16],
                scope(OTHER, "nearby").author(),
                digest,
                b"PRIVATE other author".to_vec(),
                None,
            ),
        ] {
            store
                .append_authored_draft(
                    AuthoredDraft::initial(
                        AuthoredDraftId::new(id).unwrap(),
                        author.into_bytes(),
                        SUBMISSION_RESERVATION_PAYLOAD_SCHEMA,
                        payload,
                        AuthoredDraftStage::Draft,
                        None,
                        NOW,
                    )
                    .unwrap()
                    .with_scope(record_scope)
                    .unwrap(),
                    None,
                )
                .await
                .unwrap();
            if let Some(reason) = included {
                expected_repairs.insert(id, reason);
            }
        }
        let legacy = runtime
            .phase1_save_draft(
                [0xee; 16],
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("legacy fixture").unwrap()),
                NOW / 1000,
                vec![],
                None,
                NOW,
            )
            .await
            .unwrap();
        let maximum = runtime.submission_page(&selected, 256, None).await.unwrap();
        assert_eq!(maximum.entries().len(), 256);
        assert!(maximum.next_cursor().is_some());
        let first = runtime.submission_page(&selected, 100, None).await.unwrap();
        assert_eq!(first.entries().len(), 100);
        let cursor = first.next_cursor().unwrap().to_owned();
        assert_eq!(first.scope(), &selected);
        let mut entries = first.entries().to_vec();
        if sqlite {
            runtime.shutdown().await.unwrap();
            drop(runtime);
            runtime =
                operation::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
        }
        let mut cursor = Some(cursor);
        while let Some(current) = cursor {
            let page = runtime
                .submission_page(&selected, 37, Some(&current))
                .await
                .unwrap();
            assert!(page.entries().len() <= 37);
            assert!(!format!("{page:?}").contains("PRIVATE"));
            entries.extend_from_slice(page.entries());
            cursor = page.next_cursor().map(str::to_owned);
        }
        let mut keys: Vec<_> = expected
            .keys()
            .chain(expected_repairs.keys())
            .copied()
            .collect();
        keys.sort();
        assert_eq!(entries.iter().map(key).collect::<Vec<_>>(), keys);
        assert_eq!(entries.len(), 260);
        for entry in entries {
            match entry {
                SubmissionListEntry::Submission(summary) => {
                    let (request, state) = &expected[summary.reservation_id().as_bytes()];
                    assert_eq!(summary.request(), request);
                    assert_eq!(summary.state(), *state);
                    assert!(summary.reserved_at_unix_ms() > 0);
                }
                SubmissionListEntry::Repair {
                    reservation_key,
                    revision,
                    reason,
                } => {
                    assert_eq!(revision, 1);
                    assert_eq!(reason, expected_repairs[&reservation_key]);
                }
            }
        }
        assert_eq!(
            runtime
                .composer_load(&selected, numbered_request(1).composer_id())
                .await
                .unwrap(),
            *source.draft()
        );
        let legacy_heads = runtime.phase1_draft_heads(100).await.unwrap();
        assert_eq!(legacy_heads.len(), 1);
        assert_eq!(legacy_heads[0].draft(), legacy.draft());
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn inventory_limits_and_cursors_reject_noncanonical_and_foreign_requests() {
    let signer = operation::CountingSigner::new();
    let runtime = operation::runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let selected = scope(AUTHOR, "nearby");
    operation::prepare(&runtime, &numbered_request(1), false).await;
    runtime
        .submission_reserve(&numbered_request(2))
        .await
        .unwrap();
    let first = runtime.submission_page(&selected, 1, None).await.unwrap();
    let cursor = first.next_cursor().unwrap();
    for invalid in [
        String::new(),
        cursor.to_uppercase(),
        cursor.replace("_v1:", "_v2:"),
        format!("{cursor}0"),
        cursor[..cursor.len() - 1].to_owned(),
        format!("{}g", &cursor[..cursor.len() - 1]),
        "x".repeat(100_000),
    ] {
        assert_eq!(
            runtime
                .submission_page(&selected, 1, Some(&invalid))
                .await
                .unwrap_err(),
            SubmissionOperationError::Operation(Phase1DraftError::InvalidInventoryCursor)
        );
    }
    assert_eq!(
        runtime
            .submission_page(&scope(AUTHOR, "other"), 1, Some(cursor))
            .await
            .unwrap_err(),
        SubmissionOperationError::Operation(Phase1DraftError::InvalidInventoryCursor)
    );
    assert!(
        runtime
            .submission_page(&scope(OTHER, "nearby"), 1, Some(cursor))
            .await
            .is_err()
    );
    for limit in [0, 257, u16::MAX] {
        assert_eq!(
            runtime
                .submission_page(&selected, limit, None)
                .await
                .unwrap_err(),
            SubmissionOperationError::Operation(Phase1DraftError::InvalidInventoryRequest)
        );
    }
    assert_eq!(
        runtime.submission_page(&selected, 1, None).await.unwrap(),
        first
    );
    assert_eq!(signer.count(), 0);
    runtime.shutdown().await.unwrap();
}
