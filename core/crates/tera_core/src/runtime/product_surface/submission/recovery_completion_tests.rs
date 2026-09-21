use super::{media_test_support::*, operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{Phase1NativeUploadJob, recovery_completion::*};

pub(super) fn native(
    status: &SubmissionOperationStatus,
    job: &Phase1NativeUploadJob,
    origin: &str,
) -> RecoveryNativeReceipt {
    RecoveryNativeReceipt::new(
        RecoveryNativeIdentity::new(
            *status.intent().draft_id().as_bytes(),
            status.intent().revision().get(),
            job.operation_id(),
            job.upload_url().into(),
        )
        .unwrap(),
        RecoveryNativeMedia::new(
            radroots_blossom::Sha256::digest(&photo().1),
            radroots_blossom::MediaType::parse("image/png").unwrap(),
            photo().0.byte_size,
        )
        .unwrap(),
        response(origin),
    )
}

pub(super) fn source() -> RecoveryMedia {
    let (item, bytes) = photo();
    RecoveryMedia::new(
        item.opaque_reference,
        bytes,
        radroots_blossom::MediaType::parse(&item.media_type).unwrap(),
        item.width,
        item.height,
    )
    .unwrap()
}

#[tokio::test]
async fn recovery_completion_restarts_and_replays_original_durable_verification_after_policy_change()
 {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    let (origin, server) = blob_server(photo().1).await;
    let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, &origin).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (uploading, job) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    // Native response survives while the runtime is gone, before any completion.
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, &origin).await;
    let verified = runtime
        .recover_native_upload(native(&uploading, &job, &origin), source())
        .await
        .unwrap();
    assert!(server.await.unwrap().starts_with("GET /"));
    let committed = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(
        committed.intent().revision().get(),
        uploading.intent().revision().get() + 1
    );
    assert_eq!(
        Some(verified.verified_at_unix_ms),
        committed.media()[0].verified_at_unix_ms()
    );
    // A lost reply/restart and changed current settings must not mint a newer
    // verification, upload, signature, or parent revision.
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
    configure(&runtime, "http://127.0.0.1:3001").await;
    for _ in 0..3 {
        assert_eq!(
            runtime
                .recover_native_upload(native(&uploading, &job, &origin), source())
                .await
                .unwrap(),
            verified
        );
    }
    assert_eq!(
        runtime
            .submission_operation_status(&request)
            .await
            .unwrap()
            .intent(),
        committed.intent()
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_completion_remote_mismatch_leaves_native_attempt_uncommitted() {
    let signer = CountingSigner::new();
    let mut changed = photo().1.to_vec();
    changed[19] = 3;
    let (origin, server) = blob_server(changed.into()).await;
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, &origin).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (uploading, job) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    assert!(
        runtime
            .recover_native_upload(native(&uploading, &job, &origin), source())
            .await
            .is_err()
    );
    server.await.unwrap();
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        uploading
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_completion_rejects_inconsistent_attempt_history_body_and_bytes_without_effects() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let origin = "http://127.0.0.1:3000";
    configure(&runtime, origin).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (uploading, job) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    for case in 0..9 {
        let mut receipt = native(&uploading, &job, origin);
        let mut bytes = source();
        match case {
            0 => {
                receipt.identity.attempt =
                    radroots_signing::SigningOperationId::new([91; 16]).unwrap()
            }
            1 => {
                receipt.identity.revision =
                    radroots_storage::authored_draft::AuthoredDraftRevision::INITIAL
            }
            2 => {
                receipt.identity.parent =
                    radroots_storage::authored_draft::AuthoredDraftId::new([92; 16]).unwrap()
            }
            3 => receipt.identity.upload_url = "http://127.0.0.1:3001/upload".into(),
            4 => receipt.media.byte_size += 1,
            5 => {
                receipt.response = SubmissionMediaResponse::new(
                    200,
                    Some("application/json".into()),
                    None,
                    b"{}".to_vec(),
                )
                .unwrap()
            }
            6 => bytes.bytes = vec![1; photo().1.len()].into(),
            7 => {
                bytes.dimensions =
                    radroots_sdk::transport::BlossomImageDimensions::new(3, 2).unwrap()
            }
            _ => bytes.reference = "media:different".into(),
        }
        assert!(
            runtime.recover_native_upload(receipt, bytes).await.is_err(),
            "case {case}"
        );
        assert_eq!(
            runtime.submission_operation_status(&request).await.unwrap(),
            uploading
        );
    }
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_completion_cancelled_remote_verification_retains_attempt_for_later_pass() {
    use tokio::{io::AsyncReadExt, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, &origin).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (uploading, job) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    for _ in 0..2 {
        let completion = runtime.recover_native_upload(native(&uploading, &job, &origin), source());
        let observation = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            assert_eq!(byte, [b'G']);
            // Concurrent duplicate cannot enter another network operation.
            assert!(
                runtime
                    .recover_native_upload(native(&uploading, &job, &origin), source())
                    .await
                    .is_err()
            );
            stream
        };
        tokio::select! {
            result = completion => panic!("verification completed before remote response: {result:?}"),
            _stream = observation => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("no recovery retrieval"),
        }
        assert_eq!(
            runtime.submission_operation_status(&request).await.unwrap(),
            uploading
        );
    }
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}
