use std::{sync::atomic::Ordering, time::Duration};

use radroots_storage::{authored::SigningState, authored_draft::AuthoredDraftStage};

use super::{operation_test_support::*, repository::SubmissionRepository, test_support::*, *};
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerPartialForm, Phase1DraftError, Phase1OutboxState,
};

#[tokio::test]
async fn scoped_operation_memory_and_sqlite_execute_original_preparation_once() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let (url, server) = relay().await;
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), &url).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let initial = runtime.submission_operation_status(&request).await.unwrap();
        assert_redacted(&initial);
        assert_eq!(initial.state(), Phase1OutboxState::ReadyToSign);
        assert_eq!(
            initial.push().artifact().signing_state(),
            SigningState::Planned
        );
        assert_eq!(signer.count(), 0);
        let frozen = initial.intent().payload().to_vec();
        let later = runtime
            .composer_save(
                request.scope(),
                request.composer_id(),
                request.expected_revision(),
                ComposerEditSequence::new(2).unwrap(),
                ComposerPartialForm::new(input(AddCommandType::CreateAsk)).unwrap(),
            )
            .await
            .unwrap();
        let loaded = SubmissionRepository {
            store: runtime.client.storage().unwrap(),
        }
        .load_operation(&request)
        .await
        .unwrap();
        // Ordinary Sync prepare replays the composite's receipt despite a later clock.
        let replay = runtime
            .sync()
            .unwrap()
            .prepare_push(loaded.request)
            .await
            .unwrap();
        assert!(replay.is_replay());
        assert_eq!(
            replay.operation().operation_id(),
            initial.receipt().operation_id()
        );
        let done = runtime.submission_advance(&request, 1).await.unwrap();
        assert_redacted(&done);
        assert_eq!(done.state(), Phase1OutboxState::Complete);
        assert_eq!(done.intent().payload(), frozen);
        assert_eq!(
            done.receipt().operation_id(),
            initial.receipt().operation_id()
        );
        assert_eq!(signer.count(), 1);
        let sent = server.await.unwrap();
        assert_eq!(sent["content"], "PRIVATE harvest café");
        assert_eq!(
            sent["id"],
            hex::encode(
                done.push()
                    .artifact()
                    .signed()
                    .unwrap()
                    .event()
                    .id()
                    .as_bytes()
            )
        );
        assert_eq!(done.push().delivery_plan().attempt_count(), 1);
        let replay = runtime
            .submission_advance(&request, done.intent().revision().get())
            .await
            .unwrap();
        assert_eq!(replay, done);
        assert_eq!(signer.count(), 1);
        assert_eq!(
            runtime
                .composer_load(request.scope(), request.composer_id())
                .await
                .unwrap(),
            *later.draft()
        );
        let identity = *done.receipt().operation_id().as_bytes();
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let reopened =
                self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
            let recovered = reopened
                .submission_operation_status(&request)
                .await
                .unwrap();
            assert_eq!(recovered.state(), Phase1OutboxState::Complete);
            assert_eq!(recovered.receipt().operation_id().as_bytes(), &identity);
            assert_eq!(recovered.intent().payload(), frozen);
            let replay = reopened
                .submission_advance(&request, recovered.intent().revision().get())
                .await
                .unwrap();
            assert_eq!(replay, recovered);
            assert_eq!(signer.count(), 1);
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn scoped_operation_waiting_scope_missing_and_stale_requests_have_no_effects() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
        let request = request();
        assert_eq!(
            runtime
                .submission_operation_status(&request)
                .await
                .unwrap_err(),
            SubmissionOperationError::NotFound
        );
        prepare(&runtime, &request, true).await;
        let original = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(
            original.intent().stage(),
            AuthoredDraftStage::MediaPreparing
        );
        for queue in [false, true] {
            let error = if queue {
                runtime.submission_queue(&request, 1).await.unwrap_err()
            } else {
                runtime.submission_advance(&request, 1).await.unwrap_err()
            };
            assert_eq!(error, SubmissionOperationError::PrerequisitesPending);
        }
        assert_eq!(
            runtime.submission_advance(&request, 2).await.unwrap_err(),
            SubmissionOperationError::Operation(Phase1DraftError::RevisionConflict)
        );
        for foreign in [scope(OTHER, "nearby"), scope(AUTHOR, "different")] {
            let request = SubmissionReservationRequest::new(
                request.command_id(),
                foreign,
                request.composer_id(),
                request.expected_revision(),
            );
            assert!(runtime.submission_advance(&request, 1).await.is_err());
        }
        assert_eq!(
            runtime.submission_operation_status(&request).await.unwrap(),
            original
        );
        assert_eq!(signer.count(), 0);
        assert!(original.push().delivery_plan().attempts().is_empty());
        // Shared storage permits waiting progress, but this consumer rejects an altered capture.
        let mut payload: serde_json::Value =
            serde_json::from_slice(original.intent().payload()).unwrap();
        payload["policy"]["delivery_deadline_unix_ms"] = serde_json::json!(2_000_000_000_000u64);
        let next = original
            .intent()
            .successor(
                serde_json::to_vec(&payload).unwrap(),
                AuthoredDraftStage::MediaPreparing,
                None,
                original.intent().updated_at_unix_ms() + 1,
            )
            .unwrap();
        runtime
            .client
            .storage()
            .unwrap()
            .append_authored_draft(next, Some(original.intent().revision()))
            .await
            .unwrap();
        assert_eq!(
            runtime.submission_advance(&request, 2).await.unwrap_err(),
            SubmissionOperationError::Corrupt
        );
        assert_eq!(signer.count(), 0);
        assert!(
            runtime
                .submission_recover(&request)
                .await
                .unwrap()
                .is_some()
        );
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn scoped_operation_queue_is_effect_free_and_uses_frozen_policy_after_settings_change() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, false).await;
    let before = runtime.submission_operation_status(&request).await.unwrap();
    runtime
        .client
        .configure_nostr(profile("ws://127.0.0.1:19998"))
        .unwrap();
    let queued = runtime.submission_queue(&request, 1).await.unwrap();
    assert_eq!(queued.state(), Phase1OutboxState::Queued);
    assert_eq!(queued.push(), before.push());
    assert_eq!(queued.intent().payload(), before.intent().payload());
    assert_eq!(signer.count(), 0);
    assert_eq!(
        runtime.submission_queue(&request, 1).await.unwrap_err(),
        SubmissionOperationError::Operation(Phase1DraftError::RevisionConflict)
    );
    assert_eq!(runtime.submission_queue(&request, 2).await.unwrap(), queued);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn scoped_operation_slow_signer_blocks_duplicate_operation_but_allows_editing() {
    let signer = CountingSigner::new();
    signer.pause.store(true, Ordering::SeqCst);
    let (url, server) = relay().await;
    let runtime = runtime(None, signer.clone(), &url).await;
    let request = request();
    prepare(&runtime, &request, false).await;
    let task = {
        let runtime = runtime.clone();
        let request = request.clone();
        tokio::spawn(async move { runtime.submission_advance(&request, 1).await })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    assert_eq!(
        runtime.submission_advance(&request, 2).await.unwrap_err(),
        SubmissionOperationError::Operation(Phase1DraftError::OperationInProgress)
    );
    let later = runtime
        .composer_save(
            request.scope(),
            request.composer_id(),
            request.expected_revision(),
            ComposerEditSequence::new(2).unwrap(),
            ComposerPartialForm::new(input(AddCommandType::CreateAsk)).unwrap(),
        )
        .await
        .unwrap();
    signer.resume.notify_one();
    assert_eq!(
        task.await.unwrap().unwrap().state(),
        Phase1OutboxState::Complete
    );
    assert_eq!(server.await.unwrap()["content"], "PRIVATE harvest café");
    assert_eq!(signer.count(), 1);
    assert_eq!(
        runtime
            .composer_load(request.scope(), request.composer_id())
            .await
            .unwrap(),
        *later.draft()
    );
    runtime.shutdown().await.unwrap();
}
