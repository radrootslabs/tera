use std::sync::atomic::Ordering;

use radroots_storage::{authored::SigningState, authored_draft::AuthoredDraftStage};

use super::{media_test_support::*, operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{Phase1MediaStage, Phase1OutboxState};

#[tokio::test]
async fn scoped_media_memory_and_sqlite_verify_before_original_operation_can_sign() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let (relay_url, relay_task) = relay().await;
        let (origin, blob_task) = blob_server(photo().1).await;
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), &relay_url).await;
        configure(&runtime, &origin).await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let original = runtime.submission_operation_status(&request).await.unwrap();
        let (uploading, job) = runtime
            .submission_prepare_native_upload(upload(&request, 1))
            .await
            .unwrap();
        assert_eq!(
            uploading.intent().stage(),
            AuthoredDraftStage::MediaUploading
        );
        assert_eq!(uploading.media()[0].stage(), Phase1MediaStage::Uploading);
        assert_eq!(
            uploading.push().artifact().signing_state(),
            SigningState::Planned
        );
        assert_eq!(signer.count(), 1); // Only the separate HTTP authorization.
        assert_ne!(
            job.operation_id(),
            *original.receipt().operation_id().as_bytes()
        );
        assert!(job.authorization_header().starts_with("Nostr "));
        assert_eq!(uploading.captured(), original.captured());
        assert_eq!(
            uploading.receipt().operation_id(),
            original.receipt().operation_id()
        );
        let ready = runtime
            .submission_complete_native_upload(
                upload(&request, uploading.intent().revision().get()),
                response(&origin),
            )
            .await
            .unwrap();
        let observed = blob_task.await.unwrap();
        assert!(observed.starts_with(&format!("GET /{}.png ", photo().0.sha256)));
        assert_eq!(ready.intent().stage(), AuthoredDraftStage::ReadyToSign);
        assert_eq!(ready.media()[0].stage(), Phase1MediaStage::Verified);
        assert_eq!(ready.media()[0].upload_attempts(), 2);
        assert!(
            ready.media()[0].verified_at_unix_ms().unwrap() <= ready.intent().updated_at_unix_ms()
        );
        assert_eq!(
            ready.intent().operation_id(),
            Some(original.receipt().operation_id())
        );
        assert_eq!(ready.push(), original.push());
        assert!(
            runtime
                .submission_prepare(&request, vec![])
                .await
                .unwrap()
                .is_replay()
        );
        let done = runtime
            .submission_advance(&request, ready.intent().revision().get())
            .await
            .unwrap();
        assert_eq!(done.state(), Phase1OutboxState::Complete);
        assert_eq!(done.intent().payload(), ready.intent().payload());
        assert_eq!(
            done.receipt().operation_id(),
            original.receipt().operation_id()
        );
        assert_eq!(signer.count(), 2);
        assert_eq!(*signer.kinds.lock().unwrap(), vec![24242, 1]);
        assert_eq!(
            relay_task.await.unwrap()["content"],
            format!("PRIVATE harvest café\n{}", job.remote_url())
        );
        assert_redacted(&done);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let reopened = self::runtime(Some(root.path()), signer.clone(), &relay_url).await;
            let recovered = reopened
                .submission_operation_status(&request)
                .await
                .unwrap();
            assert_eq!(recovered, done);
            assert_eq!(
                reopened
                    .submission_advance(&request, recovered.intent().revision().get())
                    .await
                    .unwrap(),
                done
            );
            assert_eq!(signer.count(), 2);
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn scoped_media_wrong_bytes_policy_revision_and_scope_never_invoke_signer() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let initial = runtime.submission_operation_status(&request).await.unwrap();
    let wrong = SubmissionMediaRequest::new(
        request.clone(),
        1,
        photo().0.opaque_reference,
        b"wrong".as_slice().into(),
    )
    .unwrap();
    assert!(
        runtime
            .submission_prepare_native_upload(wrong)
            .await
            .is_err()
    );
    assert!(
        runtime
            .submission_prepare_native_upload(upload(&request, 2))
            .await
            .is_err()
    );
    let foreign = SubmissionReservationRequest::new(
        request.command_id(),
        scope(AUTHOR, "elsewhere"),
        request.composer_id(),
        request.expected_revision(),
    );
    assert!(
        runtime
            .submission_prepare_native_upload(upload(&foreign, 1))
            .await
            .is_err()
    );
    configure(&runtime, "http://127.0.0.1:3001").await;
    assert!(matches!(
        runtime
            .submission_prepare_native_upload(upload(&request, 1))
            .await,
        Err(SubmissionOperationError::MediaPolicyChanged)
    ));
    assert_eq!(signer.count(), 0);
    assert_eq!(signer.statuses.load(Ordering::SeqCst), 0);
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        initial
    );
    runtime.shutdown().await.unwrap();
}

#[test]
fn native_media_response_bounds_include_body_and_headers() {
    assert!(SubmissionMediaResponse::new(200, None, None, vec![0; 16_384]).is_ok());
    assert!(SubmissionMediaResponse::new(200, Some("a".into()), None, vec![0; 16_384]).is_err());
    assert!(SubmissionMediaResponse::new(200, None, None, vec![0; 16_385]).is_err());
}
