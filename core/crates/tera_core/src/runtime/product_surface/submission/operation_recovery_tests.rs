use std::{sync::atomic::Ordering, time::Duration};

use radroots_storage::{authored::SigningState, authored_draft::AuthoredDraftStage};

use super::{operation_test_support::*, test_support::*, *};

#[tokio::test]
async fn abandoned_signer_wait_keeps_exact_sqlite_operation_for_restart() {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    signer.pause.store(true, Ordering::SeqCst);
    let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, false).await;
    let original = runtime.submission_operation_status(&request).await.unwrap();
    let task = {
        let runtime = runtime.clone();
        let request = request.clone();
        tokio::spawn(async move { runtime.submission_advance(&request, 1).await })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let waiting = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(
        waiting.receipt().operation_id(),
        original.receipt().operation_id()
    );
    assert!(waiting.push().artifact().signing_claim().is_some());
    assert!(waiting.push().artifact().signed().is_none());
    assert!(waiting.push().delivery_plan().attempts().is_empty());
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let reopened = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
    let status = reopened
        .submission_operation_status(&request)
        .await
        .unwrap();
    assert_eq!(status, waiting);
    assert!(
        reopened
            .submission_prepare(&request, vec![])
            .await
            .unwrap()
            .is_replay()
    );
    assert_eq!(signer.count(), 1);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn forged_waiting_association_is_rejected_before_any_signer_callback() {
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
        prepare(&runtime, &request, true).await;
        let original = runtime.submission_operation_status(&request).await.unwrap();
        let forged = original
            .intent()
            .successor(
                original.intent().payload().to_vec(),
                AuthoredDraftStage::ReadyToSign,
                Some(original.receipt().operation_id()),
                original.intent().updated_at_unix_ms() + 1,
            )
            .unwrap();
        runtime
            .client
            .storage()
            .unwrap()
            .append_authored_draft(forged, Some(original.intent().revision()))
            .await
            .unwrap();
        assert_eq!(
            runtime.submission_advance(&request, 2).await.unwrap_err(),
            SubmissionOperationError::Corrupt
        );
        assert_eq!(signer.count(), 0);
        assert_eq!(signer.statuses.load(Ordering::SeqCst), 0);
        let push = runtime
            .sync()
            .unwrap()
            .push_status(
                radroots_sync::policy::SyncId::new(*original.receipt().operation_id().as_bytes())
                    .unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(push.artifact().signing_state(), SigningState::Planned);
        assert!(push.delivery_plan().attempts().is_empty());
        runtime.shutdown().await.unwrap();
    }
}
