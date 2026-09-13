//! Qualify the application consumer of the shared durable signing owner.

use std::{sync::atomic::Ordering, time::Duration};

use radroots_signing::{SignRequest, error::Kind};
use radroots_storage::authored::SigningState;

use super::{operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{AddCommandType, ComposerEditSequence, ComposerPartialForm};

fn assert_frozen(request: &SignRequest, status: &SubmissionOperationStatus) {
    let artifact = status.push().artifact();
    let persisted = artifact.plan().unwrap().decode().unwrap().into_plan();
    assert_eq!(request.authored_plan(), Some(&persisted));
    assert_eq!(
        *request.expected_author(),
        status.receipt().request().scope().author()
    );
    assert_eq!(request.expected_event_id(), persisted.expected_event_id());
    assert_eq!(
        request.created_at(),
        status.receipt().captured_at_unix_ms() / 1000
    );
    assert_eq!(
        request.intent_id().operation_id().as_bytes(),
        status.receipt().operation_id().as_bytes()
    );
    assert_eq!(
        request.intent_id().artifact_id().as_bytes(),
        artifact.artifact_id().as_bytes()
    );
}

#[tokio::test]
async fn signing_preparation_sqlite_is_committed_and_unlocked_before_callback() {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    signer.pause.store(true, Ordering::SeqCst);
    let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, false).await;
    let original = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(signer.count(), 0);
    let task = {
        let runtime = runtime.clone();
        let request = request.clone();
        tokio::spawn(async move { runtime.submission_advance(&request, 1).await })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    let waiting = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.submission_operation_status(&request),
    )
    .await
    .unwrap()
    .unwrap();
    let callback = signer.requests.lock().unwrap()[0].clone();
    assert_frozen(&callback, &waiting);
    assert_eq!(
        waiting.push().artifact().plan(),
        original.push().artifact().plan()
    );
    assert!(waiting.push().artifact().signing_claim().is_some());
    assert!(waiting.push().artifact().signed().is_none());
    assert!(waiting.push().delivery_plan().attempts().is_empty());
    // A real write on the same SQLite owner must finish while the signer remains suspended.
    let later = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.composer_save(
            request.scope(),
            request.composer_id(),
            request.expected_revision(),
            ComposerEditSequence::new(2).unwrap(),
            ComposerPartialForm::new(input(AddCommandType::CreateAsk)).unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let reopened = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
    let recovered = reopened
        .submission_operation_status(&request)
        .await
        .unwrap();
    assert_eq!(recovered, waiting);
    assert_frozen(&callback, &recovered);
    assert_eq!(
        reopened
            .composer_load(request.scope(), request.composer_id())
            .await
            .unwrap(),
        *later.draft()
    );
    assert_eq!(signer.count(), 1);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn signing_preparation_lock_and_denial_preserve_exact_request_across_restart() {
    for failure in [
        Kind::SignerUnavailable,
        Kind::AuthorizationDenied,
        Kind::SignerRejected,
    ] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        signer.pause.store(true, Ordering::SeqCst);
        let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let before = runtime.submission_operation_status(&request).await.unwrap();
        let task = {
            let runtime = runtime.clone();
            let request = request.clone();
            tokio::spawn(async move { runtime.submission_advance(&request, 1).await })
        };
        tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
            .await
            .unwrap();
        // Model a lock/denial after status was ready and the durable claim was installed.
        *signer.failure.lock().unwrap() = Some(failure);
        signer.resume.notify_one();
        assert!(task.await.unwrap().is_err());
        let failed = runtime.submission_operation_status(&request).await.unwrap();
        let callback = signer.requests.lock().unwrap()[0].clone();
        assert_frozen(&callback, &failed);
        assert_eq!(
            failed.push().artifact().plan(),
            before.push().artifact().plan()
        );
        assert!(failed.push().artifact().signed().is_none());
        assert!(failed.push().delivery_plan().attempts().is_empty());
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened =
            self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
        let recovered = reopened
            .submission_operation_status(&request)
            .await
            .unwrap();
        assert_eq!(recovered, failed);
        assert_frozen(&callback, &recovered);
        assert!(
            reopened
                .submission_prepare(&request, vec![])
                .await
                .unwrap()
                .is_replay()
        );
        if failure == Kind::SignerUnavailable {
            assert_eq!(
                recovered.push().artifact().signing_state(),
                SigningState::Retryable
            );
            tokio::time::sleep(Duration::from_millis(1100)).await;
            let (loaded, _) = reopened.load_submission_operation(&request).await.unwrap();
            reopened
                .sync()
                .unwrap()
                .sign_prepared(loaded.request)
                .await
                .unwrap();
            let retried = signer.requests.lock().unwrap()[1].clone();
            assert_frozen(&retried, &recovered);
            assert_eq!(retried.signer_request_id(), callback.signer_request_id());
            assert_eq!(retried.intent_id(), callback.intent_id());
            let signed = reopened
                .submission_operation_status(&request)
                .await
                .unwrap();
            assert_eq!(
                signed.push().artifact().signing_state(),
                SigningState::Signed
            );
            assert!(signed.push().delivery_plan().attempts().is_empty());
            assert_eq!(signer.count(), 2);
        } else {
            assert_eq!(
                recovered.push().artifact().signing_state(),
                SigningState::FailedTerminal
            );
            let after = reopened
                .submission_advance(&request, recovered.intent().revision().get())
                .await
                .unwrap();
            assert_eq!(after, recovered);
            assert_eq!(signer.count(), 1);
        }
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn signing_preparation_restart_before_commit_retains_reserved_time_and_author() {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
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
    let plan = captured.plan().clone();
    assert!(
        runtime
            .submission_recover(&request)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(signer.count(), 0);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let reopened = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    assert!(
        reopened
            .submission_recover(&request)
            .await
            .unwrap()
            .is_none()
    );
    let recaptured = reopened.submission_capture(&request, vec![]).await.unwrap();
    assert_eq!(recaptured.plan(), &plan);
    let committed = reopened.submission_commit(&recaptured).await.unwrap();
    let status = reopened
        .submission_operation_status(&request)
        .await
        .unwrap();
    assert_eq!(
        status
            .push()
            .artifact()
            .plan()
            .unwrap()
            .decode()
            .unwrap()
            .into_plan(),
        plan
    );
    assert_eq!(
        committed.captured_at_unix_ms(),
        captured.reservation().reserved_at_unix_ms()
    );
    assert_eq!(signer.count(), 0);
    assert!(status.push().artifact().signing_claim().is_none());
    assert!(status.push().delivery_plan().attempts().is_empty());
    reopened.shutdown().await.unwrap();
}
