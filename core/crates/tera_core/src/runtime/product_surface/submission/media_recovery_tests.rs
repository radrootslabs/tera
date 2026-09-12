use radroots_storage::{authored::SigningState, authored_draft::AuthoredDraftStage};

use super::{media_test_support::*, operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerPartialForm, Phase1MediaStage,
};

#[tokio::test]
async fn pending_native_upload_recovers_after_policy_refusal_edits_and_sqlite_restart() {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    let (origin, server) = blob_server(photo().1).await;
    let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, &origin).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (uploading, _) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
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
    configure(&runtime, "http://127.0.0.1:3001").await;
    assert!(matches!(
        runtime
            .submission_complete_native_upload(upload(&request, 2), response(&origin))
            .await,
        Err(SubmissionOperationError::MediaPolicyChanged)
    ));
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        uploading
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
    configure(&runtime, &origin).await;
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        uploading
    );
    let ready = runtime
        .submission_complete_native_upload(upload(&request, 2), response(&origin))
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(ready.intent().stage(), AuthoredDraftStage::ReadyToSign);
    assert_eq!(ready.captured(), uploading.captured());
    assert_eq!(
        ready.receipt().operation_id(),
        uploading.receipt().operation_id()
    );
    assert_eq!(
        ready.push().artifact().signing_state(),
        SigningState::Planned
    );
    assert_eq!(
        runtime
            .composer_load(request.scope(), request.composer_id())
            .await
            .unwrap(),
        *later.draft()
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn mismatched_remote_bytes_retain_failed_media_and_never_sign_publication() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let mut bytes = photo().1.to_vec();
        bytes[19] = 3;
        let (origin, server) = blob_server(bytes.into()).await;
        let runtime = runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
        configure(&runtime, &origin).await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let (uploading, _) = runtime
            .submission_prepare_native_upload(upload(&request, 1))
            .await
            .unwrap();
        assert!(
            runtime
                .submission_complete_native_upload(upload(&request, 2), response(&origin))
                .await
                .is_err()
        );
        server.await.unwrap();
        let failed = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(failed.media()[0].stage(), Phase1MediaStage::Failed);
        assert!(failed.media()[0].orphan().is_some());
        assert_eq!(failed.intent().operation_id(), None);
        assert_eq!(
            failed.receipt().operation_id(),
            uploading.receipt().operation_id()
        );
        assert_eq!(
            failed.push().artifact().signing_state(),
            SigningState::Planned
        );
        assert!(matches!(
            runtime
                .submission_advance(&request, failed.intent().revision().get())
                .await,
            Err(SubmissionOperationError::PrerequisitesPending)
        ));
        assert_eq!(signer.count(), 1);
        assert!(
            runtime
                .submission_prepare(&request, vec![])
                .await
                .unwrap()
                .is_replay()
        );
        runtime.shutdown().await.unwrap();
    }
}
