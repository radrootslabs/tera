use super::*;
use crate::runtime::product_surface::{
    ComposerRevision, SubmissionCommandId, submission::test_support::*,
};
use radroots_storage::authored_draft::AuthoredDraftStage;

use super::super::fault_store::{Fault, FaultStore};
use super::super::transaction_test_support::capture;

async fn assert_records<S: AuthoredDraftStore + AuthoredAtomicStorage + ?Sized>(
    store: &S,
    capture: &CapturedSubmission,
    installed: bool,
) {
    let request = capture.reservation().request();
    assert_eq!(
        store
            .authored_receipt(intent::commit_id(request))
            .await
            .unwrap()
            .is_some(),
        installed
    );
    assert_eq!(
        store
            .authored_draft_head(intent::intent_id(request).unwrap())
            .await
            .unwrap()
            .is_some(),
        installed
    );
    let operation = store
        .authored_operation(intent::operation_id(request).unwrap())
        .await
        .unwrap();
    assert_eq!(operation.is_some(), installed);
    if let Some(operation) = operation {
        for id in operation.artifact_ids() {
            let artifact = store.authored_artifact(*id).await.unwrap().unwrap();
            assert_eq!(
                artifact.signing_state(),
                radroots_storage::authored::SigningState::Planned
            );
            assert!(artifact.signed().is_none());
            assert_eq!(
                artifact.admission_state(),
                radroots_storage::authored::AdmissionState::Pending
            );
        }
    }
    let source = store
        .authored_draft_head(capture.reservation().source.draft_id())
        .await
        .unwrap()
        .unwrap();
    assert!(capture.reservation().source.matches(&source));
    assert_eq!(
        store
            .authored_draft_heads(*source.author(), 20)
            .await
            .unwrap()
            .len(),
        if installed { 3 } else { 2 }
    );
}

#[tokio::test]
async fn racing_identical_commits_install_one_operation_with_exact_source_and_no_effects() {
    for media in [false, true] {
        let client = radroots_sdk::ClientBuilder::memory_default()
            .build()
            .unwrap();
        let inner = client.storage().unwrap();
        let request = request();
        let captured = capture(inner, &request, media).await;
        let mut store = FaultStore::new(inner, Fault::None);
        store.atomic_fault = Fault::Race(tokio::sync::Barrier::new(2));
        let repo = SubmissionRepository { store: &store };
        let (a, b) = tokio::join!(repo.commit(&captured), repo.commit(&captured));
        let a = a.unwrap();
        let b = b.unwrap();
        assert_eq!(a.operation_id(), b.operation_id());
        assert_eq!(a.intent_id(), b.intent_id());
        assert_ne!(a.is_replay(), b.is_replay());
        assert_eq!(a.captured_at_unix_ms(), NOW);
        assert!(a.committed_at_unix_ms() >= NOW);
        assert_ne!(a.intent_id(), captured.reservation().reservation_id());
        assert_ne!(a.intent_id().as_bytes(), a.operation_id().as_bytes());
        assert_ne!(a.intent_id(), captured.reservation().source.draft_id());
        assert!(!format!("{a:?}").contains("PRIVATE"));
        assert_records(inner, &captured, true).await;
        let draft = inner
            .authored_draft_head(a.intent_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            draft.stage(),
            if media {
                AuthoredDraftStage::MediaPreparing
            } else {
                AuthoredDraftStage::ReadyToSign
            }
        );
        assert_eq!(
            draft.operation_id(),
            if media { None } else { Some(a.operation_id()) }
        );
        let recovered = repo.recover(&request).await.unwrap().unwrap();
        assert!(recovered.is_replay());
        assert_eq!(recovered.operation_id(), a.operation_id());
        assert_eq!(store.append_count(), 0);
    }
}

#[tokio::test]
async fn failed_or_lost_commit_acknowledgements_never_return_dangling_success() {
    for media in [false, true] {
        for fault in [
            Fault::BeforeCommit,
            Fault::LostCallback,
            Fault::WrongReceipt,
        ] {
            let client = radroots_sdk::ClientBuilder::memory_default()
                .build()
                .unwrap();
            let inner = client.storage().unwrap();
            let request = request();
            let captured = capture(inner, &request, media).await;
            let before = matches!(fault, Fault::BeforeCommit);
            let wrong = matches!(fault, Fault::WrongReceipt);
            let mut store = FaultStore::new(inner, Fault::None);
            store.atomic_fault = fault;
            let repo = SubmissionRepository { store: &store };
            assert_eq!(
                repo.commit(&captured).await.unwrap_err(),
                if wrong {
                    E::InvalidReceipt
                } else {
                    E::Storage(Error::BackendUnavailable)
                }
            );
            assert_records(inner, &captured, !before).await;
            assert_eq!(repo.recover(&request).await.unwrap().is_none(), before);
            let retry = repo.commit(&captured).await.unwrap();
            assert_eq!(retry.is_replay(), !before);
            assert_records(inner, &captured, true).await;
        }
    }
}

#[tokio::test]
async fn full_changed_policy_conflicts_but_distinct_equal_actions_have_distinct_operations() {
    for media in [false, true] {
        let client = radroots_sdk::ClientBuilder::memory_default()
            .build()
            .unwrap();
        let inner = client.storage().unwrap();
        let request = request();
        let captured = capture(inner, &request, media).await;
        let repo = SubmissionRepository { store: inner };
        let original = repo.commit(&captured).await.unwrap();
        let bytes = if media { vec![photo().1] } else { vec![] };
        let slot = blossom();
        let changed = CapturedSubmission::capture(
            captured.reservation().clone(),
            policy("wss://new.example"),
            Some(&slot),
            bytes.clone(),
        )
        .unwrap();
        assert_eq!(
            repo.commit(&changed).await.unwrap_err(),
            E::IdempotencyConflict
        );
        if media {
            slot.configure(
                radroots_sdk::transport::BlossomConfig::from_profile(slot.profile().unwrap())
                    .with_limits(1024 * 1024, 8192, 1)
                    .unwrap(),
            )
            .unwrap();
            let changed = CapturedSubmission::capture(
                captured.reservation().clone(),
                captured.policy().clone(),
                Some(&slot),
                bytes.clone(),
            )
            .unwrap();
            assert_eq!(
                repo.commit(&changed).await.unwrap_err(),
                E::IdempotencyConflict
            );
        }
        let mut new = request.clone();
        new.command_id = SubmissionCommandId::new([8; 16]).unwrap();
        let reservation = repo.reserve(&new, Some(NOW)).await.unwrap();
        let second = CapturedSubmission::capture(
            reservation,
            captured.policy().clone(),
            Some(&blossom()),
            bytes,
        )
        .unwrap();
        assert_eq!(captured.plan(), second.plan());
        let second = repo.commit(&second).await.unwrap();
        assert_ne!(original.operation_id(), second.operation_id());
        assert_ne!(original.intent_id(), second.intent_id());
        for changed in [
            SubmissionReservationRequest::new(
                request.command_id(),
                scope(AUTHOR, "other"),
                request.composer_id(),
                request.expected_revision(),
            ),
            SubmissionReservationRequest::new(
                request.command_id(),
                request.scope().clone(),
                request.composer_id(),
                ComposerRevision::new(2).unwrap(),
            ),
        ] {
            assert_eq!(
                repo.recover(&changed).await.unwrap_err(),
                E::IdempotencyConflict
            );
        }
    }
}

#[tokio::test]
async fn original_composer_cas_rejects_first_commit_after_edit_but_preserves_committed_replay() {
    for committed_first in [false, true] {
        let client = radroots_sdk::ClientBuilder::memory_default()
            .build()
            .unwrap();
        let inner = client.storage().unwrap();
        let request = request();
        let captured = capture(inner, &request, false).await;
        let repo = SubmissionRepository { store: inner };
        if committed_first {
            repo.commit(&captured).await.unwrap();
        }
        let source = inner
            .authored_draft_head(captured.reservation().source.draft_id())
            .await
            .unwrap()
            .unwrap();
        // Even a new revision with identical form bytes loses the original CAS.
        let newer = source
            .successor(
                source.payload().to_vec(),
                AuthoredDraftStage::Draft,
                None,
                NOW + 1,
            )
            .unwrap();
        inner
            .append_authored_draft(newer, Some(source.revision()))
            .await
            .unwrap();
        if committed_first {
            assert!(repo.commit(&captured).await.unwrap().is_replay());
            assert!(repo.recover(&request).await.unwrap().unwrap().is_replay());
        } else {
            assert_eq!(
                repo.commit(&captured).await.unwrap_err(),
                E::RevisionConflict
            );
            assert!(repo.recover(&request).await.unwrap().is_none());
            assert!(
                inner
                    .authored_operation(intent::operation_id(&request).unwrap())
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(
                inner
                    .authored_draft_head(intent::intent_id(&request).unwrap())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn untrusted_receipt_and_historical_source_cannot_acknowledge_or_replace_work() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let inner = client.storage().unwrap();
    let request = request();
    let captured = capture(inner, &request, false).await;
    let repo = SubmissionRepository { store: inner };
    let saved = repo.commit(&captured).await.unwrap();
    let receipt = inner
        .authored_receipt(intent::commit_id(&request))
        .await
        .unwrap()
        .unwrap();
    let AuthoredAtomicOutcome::Submitted(original) = receipt.outcome() else {
        panic!("expected submission")
    };
    let prepared = original.preparation();
    let wrong = AuthoredAtomicReceipt::from_durable_parts(
        receipt.commit_id(),
        receipt.digest(),
        receipt.disposition(),
        receipt.committed_at_unix_ms(),
        AuthoredAtomicOutcome::Prepared {
            operation: prepared.operation().clone(),
            artifacts: prepared.artifacts().to_vec(),
            delivery_plans: prepared.delivery_plans().to_vec(),
        },
    )
    .unwrap();
    let mut store = FaultStore::new(inner, Fault::None);
    store.receipt_override = Some(Some(wrong));
    assert_eq!(
        SubmissionRepository { store: &store }
            .recover(&request)
            .await
            .unwrap_err(),
        E::InvalidReceipt
    );
    let mut different = request.clone();
    different.command_id = SubmissionCommandId::new([8; 16]).unwrap();
    store.receipt_override = Some(Some(receipt.clone()));
    assert_eq!(
        SubmissionRepository { store: &store }
            .recover(&different)
            .await
            .unwrap_err(),
        E::InvalidReceipt
    );
    store.receipt_override = None;
    let source = inner
        .authored_draft_head(captured.reservation().source.draft_id())
        .await
        .unwrap()
        .unwrap();
    store.historical_override = Some(
        radroots_storage::authored_draft::AuthoredDraft::initial(
            source.draft_id(),
            *source.author(),
            source.payload_schema(),
            b"corrupt source bytes".to_vec(),
            source.stage(),
            None,
            NOW,
        )
        .unwrap()
        .with_scope(source.scope().unwrap())
        .unwrap(),
    );
    assert_eq!(
        SubmissionRepository { store: &store }
            .recover(&request)
            .await
            .unwrap_err(),
        E::Reservation(super::super::SubmissionReservationError::CorruptRecord)
    );
    assert_eq!(
        SubmissionRepository { store: &store }
            .commit(&captured)
            .await
            .unwrap_err(),
        E::Reservation(super::super::SubmissionReservationError::CorruptRecord)
    );
    assert_eq!(store.commits.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(store.append_count(), 0);
    assert_eq!(
        repo.recover(&request)
            .await
            .unwrap()
            .unwrap()
            .operation_id(),
        saved.operation_id()
    );
}
