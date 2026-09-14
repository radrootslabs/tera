use super::{operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerPartialForm, CreateUpdate, Phase1AddCommand,
    Phase1DraftError, Phase1OutboxState, Phase1QueueIntent, ProfileMetadataCommand,
};
use radroots_storage::authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage};
use std::{sync::atomic::Ordering, time::Duration};

const ORIGINAL: &str = "ws://127.0.0.1:19999";
const REMOVED: &str = "ws://127.0.0.1:19998";

#[tokio::test]
async fn configuration_and_commit_race_has_one_ordered_local_outcome() {
    for change_first in [false, true] {
        let signer = CountingSigner::new();
        let runtime = runtime(None, signer.clone(), ORIGINAL).await;
        let request = request();
        runtime
            .composer_create(
                request.scope(),
                request.composer_id(),
                ComposerEditSequence::INITIAL,
                ComposerPartialForm::new(input(AddCommandType::CreateUpdate)).unwrap(),
            )
            .await
            .unwrap();
        let captured = runtime.submission_capture(&request, vec![]).await.unwrap();
        let guard = runtime.publication_configuration.write().await;
        let mut commit = Box::pin(runtime.submission_commit(&captured));
        let mut change = Box::pin(runtime.configure_simulator_relays(vec![REMOVED.into()]));
        // Poll into the fair configuration lock in both orders. Neither local
        // transaction may escape the guard held here, and no timing sleep is
        // needed to make the competing admission deterministic.
        if change_first {
            assert!(futures_util::poll!(change.as_mut()).is_pending());
            assert!(futures_util::poll!(commit.as_mut()).is_pending());
        } else {
            assert!(futures_util::poll!(commit.as_mut()).is_pending());
            assert!(futures_util::poll!(change.as_mut()).is_pending());
        }
        drop(guard);
        let (committed, changed) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(commit, change)
        })
        .await
        .unwrap();
        changed.unwrap();
        if change_first {
            assert_eq!(
                committed.unwrap_err(),
                SubmissionCommitError::Capture(SubmissionCaptureError::PolicyUnavailable)
            );
            assert!(
                runtime
                    .submission_recover(&request)
                    .await
                    .unwrap()
                    .is_none()
            );
        } else {
            let committed = committed.unwrap();
            let stopped = runtime.submission_operation_status(&request).await.unwrap();
            assert_eq!(stopped.receipt().operation_id(), committed.operation_id());
            assert!(
                stopped
                    .delivery_evidence()
                    .stop_requested_at_unix_ms
                    .is_some()
            );
        }
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn committed_capture_replays_after_configuration_but_changed_request_conflicts() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), ORIGINAL).await;
        let request = request();
        runtime
            .composer_create(
                request.scope(),
                request.composer_id(),
                ComposerEditSequence::INITIAL,
                ComposerPartialForm::new(input(AddCommandType::CreateUpdate)).unwrap(),
            )
            .await
            .unwrap();
        let captured = runtime.submission_capture(&request, vec![]).await.unwrap();
        let original = runtime.submission_commit(&captured).await.unwrap();
        runtime
            .configure_simulator_relays(vec![REMOVED.into()])
            .await
            .unwrap();
        let replay = runtime.submission_commit(&captured).await.unwrap();
        assert!(replay.is_replay());
        assert_eq!(replay.operation_id(), original.operation_id());
        assert_eq!(replay.intent_id(), original.intent_id());
        assert_eq!(
            replay.committed_at_unix_ms(),
            original.committed_at_unix_ms()
        );
        let changed = CapturedSubmission::capture(
            captured.reservation().clone(),
            policy(REMOVED),
            Some(&blossom()),
            vec![],
        )
        .unwrap();
        assert_eq!(
            runtime.submission_commit(&changed).await.unwrap_err(),
            SubmissionCommitError::IdempotencyConflict
        );
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn configuration_pages_all_scopes_and_isolates_invalid_inventory_rows() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), ORIGINAL).await;
        let mut requests = Vec::new();
        for id in 1_u128..=65 {
            let request = SubmissionReservationRequest::new(
                SubmissionCommandId::new(id.to_be_bytes()).unwrap(),
                scope(AUTHOR, &format!("context-{id}")),
                ComposerId::new(id.to_be_bytes()).unwrap(),
                ComposerRevision::INITIAL,
            );
            prepare(&runtime, &request, false).await;
            requests.push(request);
        }
        let original = runtime
            .submission_operation_status(&requests[0])
            .await
            .unwrap();
        let storage = runtime.client.storage().unwrap();
        // One malformed payload and one copied payload with a different row ID
        // are visible inventory entries, but neither has effect authority.
        for (id, bytes) in [
            (900_u128, b"malformed".to_vec()),
            (901, original.intent().payload().to_vec()),
        ] {
            let value = AuthoredDraft::initial(
                AuthoredDraftId::new(id.to_be_bytes()).unwrap(),
                scope(AUTHOR, "nearby").author().into_bytes(),
                SUBMISSION_INTENT_PAYLOAD_SCHEMA,
                bytes,
                AuthoredDraftStage::Draft,
                None,
                NOW,
            )
            .unwrap();
            storage.append_authored_draft(value, None).await.unwrap();
        }
        runtime
            .configure_simulator_relays(vec![REMOVED.into()])
            .await
            .unwrap();
        runtime
            .configure_simulator_relays(vec![ORIGINAL.into()])
            .await
            .unwrap();
        for request in &requests {
            let status = runtime.submission_operation_status(request).await.unwrap();
            assert!(
                status
                    .delivery_evidence()
                    .stop_requested_at_unix_ms
                    .is_some()
            );
            assert_eq!(
                status.intent().author(),
                request.scope().author().as_bytes()
            );
            assert_eq!(
                runtime
                    .submission_advance(request, status.intent().revision().get())
                    .await
                    .unwrap_err(),
                SubmissionOperationError::Stopped
            );
        }
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn explicit_startup_environment_restricts_recovered_original_work() {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    let original = runtime(Some(root.path()), signer.clone(), ORIGINAL).await;
    let request = request();
    prepare(&original, &request, false).await;
    let before = original
        .submission_operation_status(&request)
        .await
        .unwrap();
    original.shutdown().await.unwrap();
    drop(original);
    let reopened = runtime(Some(root.path()), signer.clone(), REMOVED).await;
    let stopped = reopened
        .submission_operation_status(&request)
        .await
        .unwrap();
    assert_eq!(stopped.intent(), before.intent());
    assert!(
        stopped
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
    );
    reopened
        .configure_simulator_relays(vec![ORIGINAL.into()])
        .await
        .unwrap();
    assert_eq!(
        reopened.submission_advance(&request, 1).await.unwrap_err(),
        SubmissionOperationError::Stopped
    );
    assert_eq!(signer.count(), 0);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn legacy_queued_and_signed_work_cannot_revive_after_removal_and_restart() {
    for sqlite in [false, true] {
        for signed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let signer = CountingSigner::new();
            let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), ORIGINAL).await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let saved = runtime
                .phase1_save_draft(
                    [81; 16],
                    Phase1AddCommand::CreateUpdate(
                        CreateUpdate::new("Frozen legacy request").unwrap(),
                    ),
                    now / 1000,
                    vec![],
                    None,
                    now,
                )
                .await
                .unwrap();
            let queued = runtime
                .phase1_queue_add_intent(
                    Phase1QueueIntent::new([81; 16], saved.draft().revision().get()).unwrap(),
                )
                .await
                .unwrap();
            if signed {
                runtime
                    .phase1_sign_queued_draft([81; 16], queued.draft().revision().get())
                    .await
                    .unwrap();
            }
            let before = runtime.phase1_draft_status([81; 16]).await.unwrap();
            runtime
                .configure_simulator_relays(vec![REMOVED.into()])
                .await
                .unwrap();
            runtime
                .configure_simulator_relays(vec![ORIGINAL.into()])
                .await
                .unwrap();
            let stopped = runtime.phase1_draft_status([81; 16]).await.unwrap();
            assert_eq!(stopped.draft(), before.draft());
            assert_eq!(
                stopped.push().unwrap().delivery_plan().intent(),
                before.push().unwrap().delivery_plan().intent()
            );
            assert!(
                stopped
                    .push()
                    .unwrap()
                    .delivery_plan()
                    .stop_requested_at_unix_ms()
                    .is_some()
            );
            assert_eq!(
                runtime
                    .phase1_advance_draft([81; 16], queued.draft().revision().get())
                    .await
                    .unwrap_err(),
                Phase1DraftError::Terminal
            );
            assert_eq!(signer.count(), usize::from(signed));
            runtime.shutdown().await.unwrap();
            drop(runtime);
            if sqlite {
                let reopened = self::runtime(Some(root.path()), signer, ORIGINAL).await;
                let status = reopened.phase1_draft_status([81; 16]).await.unwrap();
                assert_eq!(status.draft(), stopped.draft());
                assert_eq!(status.push(), stopped.push());
                reopened.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn legacy_profile_held_signer_is_stopped_without_waiting_for_its_callback() {
    let signer = CountingSigner::new();
    signer.pause.store(true, Ordering::SeqCst);
    let runtime = runtime(None, signer.clone(), ORIGINAL).await;
    let saved = runtime
        .phase1_save_profile_metadata(
            ProfileMetadataCommand::new("grower".into(), None, None, None, None, None, None)
                .unwrap(),
        )
        .await
        .unwrap();
    let id = *saved.draft().draft_id().as_bytes();
    let task = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime.phase1_advance_profile(id).await })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        runtime.configure_simulator_relays(vec![REMOVED.into()]),
    )
    .await
    .unwrap()
    .unwrap();
    runtime
        .configure_simulator_relays(vec![ORIGINAL.into()])
        .await
        .unwrap();
    signer.resume.notify_one();
    let _result = task.await.unwrap();
    let stopped = runtime.phase1_profile_status(id).await.unwrap();
    let push = stopped.push().unwrap();
    assert!(push.delivery_plan().stop_requested_at_unix_ms().is_some());
    assert!(push.artifact().signed().is_some());
    assert!(!push.artifact().admission_state().is_admitted());
    assert_eq!(stopped.state(), Phase1OutboxState::Cancelled);
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}
