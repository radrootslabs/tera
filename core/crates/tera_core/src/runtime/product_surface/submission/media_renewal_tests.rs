use super::{media_test_support::*, operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::phase1_operation_now_unix_ms;
use std::{sync::atomic::Ordering, time::Duration};

#[tokio::test]
async fn exact_predecessor_receipt_completes_after_renewal_and_replays_without_new_effects() {
    use super::recovery_completion_tests::{native, source};
    let signer = CountingSigner::new();
    let (origin, server) = blob_server(photo().1).await;
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, &origin).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (original, first) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    let (renewed, _) = runtime
        .prepare_submission_upload_at(
            upload(&request, 2),
            Some(renewal(&original, false)),
            expiration(&original),
        )
        .await
        .unwrap();
    let mut mismatched = native(&original, &first, &origin);
    mismatched.identity.revision = renewed.intent().revision();
    assert!(
        runtime
            .recover_native_upload(mismatched, source())
            .await
            .is_err()
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        renewed
    );
    let receipt = runtime
        .recover_native_upload(native(&original, &first, &origin), source())
        .await
        .unwrap();
    assert_eq!(receipt.attempt, first.operation_id());
    assert!(server.await.unwrap().starts_with("GET /"));
    let complete = runtime.submission_operation_status(&request).await.unwrap();
    assert!(complete.media()[0].is_remote_verified());
    assert_eq!(
        complete.media()[0].upload_authorizations(),
        renewed.media()[0].upload_authorizations()
    );
    assert_eq!(
        runtime
            .recover_native_upload(native(&original, &first, &origin), source())
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        complete
    );
    assert_eq!(signer.count(), 2);
    runtime.shutdown().await.unwrap();
}

fn renewal(status: &SubmissionOperationStatus, failed: bool) -> SubmissionUploadRenewal {
    let attempts = status.media()[0].upload_authorizations();
    let last = attempts.last().unwrap();
    SubmissionUploadRenewal::new(last.revision.unwrap(), last.operation_id, failed).unwrap()
}

fn expiration(status: &SubmissionOperationStatus) -> u64 {
    status.media()[0]
        .upload_authorizations()
        .last()
        .unwrap()
        .expiration_unix_s
        * 1000
}

#[tokio::test]
async fn expired_authority_renews_same_parent_with_bounded_distinct_lineage() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (original, first) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    let mut current = original.clone();
    for generation in 2..=3 {
        let now = expiration(&current);
        let (next, job) = runtime
            .prepare_submission_upload_at(
                upload(&request, current.intent().revision().get()),
                Some(renewal(&current, false)),
                now,
            )
            .await
            .unwrap();
        assert_eq!(next.receipt(), original.receipt());
        assert_eq!(next.captured(), original.captured());
        assert_eq!(next.push(), original.push());
        assert_eq!(next.intent().draft_id(), original.intent().draft_id());
        assert_eq!(next.media()[0].upload_authorizations().len(), generation);
        assert_ne!(job.operation_id(), first.operation_id());
        assert_ne!(job.authorization_header(), first.authorization_header());
        assert_eq!(job.expected_sha256(), first.expected_sha256());
        assert_eq!(job.upload_url(), first.upload_url());
        assert!(!String::from_utf8_lossy(next.intent().payload()).contains("Nostr "));
        current = next;
    }
    assert_eq!(
        runtime
            .prepare_submission_upload_at(
                upload(&request, current.intent().revision().get()),
                Some(renewal(&current, false)),
                expiration(&current)
            )
            .await
            .err(),
        Some(SubmissionOperationError::UploadAttemptsExhausted)
    );
    assert_eq!(signer.count(), 3);
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        current
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn renewal_checks_expiry_backoff_clock_and_exact_current_history_before_signing() {
    let signer = CountingSigner::new();
    let config = radroots_sdk::transport::BlossomConfig::from_profile(blossom().profile().unwrap())
        .with_network_policy(
            Duration::from_secs(10),
            Duration::from_secs(60),
            3,
            Duration::from_secs(20),
        )
        .unwrap();
    let runtime = runtime_with_blossom(None, signer.clone(), "ws://127.0.0.1:19999", config).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let now = phase1_operation_now_unix_ms().unwrap();
    let (original, _) = runtime
        .prepare_submission_upload_at(upload(&request, 1), None, now)
        .await
        .unwrap();
    for (time, failed) in [(now - 1, true), (now + 19_999, true), (now + 25_000, false)] {
        assert!(
            runtime
                .prepare_submission_upload_at(
                    upload(&request, 2),
                    Some(renewal(&original, failed)),
                    time
                )
                .await
                .is_err()
        );
    }
    for (revision, attempt) in [
        (
            1,
            original.media()[0].upload_authorizations()[0].operation_id,
        ),
        (2, [99; 16]),
    ] {
        assert!(
            runtime
                .prepare_submission_upload_at(
                    upload(&request, 2),
                    Some(SubmissionUploadRenewal::new(revision, attempt, true).unwrap()),
                    now + 25_000
                )
                .await
                .is_err()
        );
    }
    assert_eq!(signer.count(), 1);
    let (next, _) = runtime
        .prepare_submission_upload_at(
            upload(&request, 2),
            Some(renewal(&original, true)),
            now + 25_000,
        )
        .await
        .unwrap();
    assert!(
        runtime
            .prepare_submission_upload_at(
                upload(&request, 3),
                Some(renewal(&original, true)),
                now + 60_000
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .prepare_submission_upload_at(
                upload(&request, 2),
                Some(renewal(&next, true)),
                now + 60_000
            )
            .await
            .is_err()
    );
    assert_eq!(signer.count(), 2);
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        next
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn denied_and_lost_renewal_consume_durable_budget_across_reopen() {
    let root = tempfile::tempdir().unwrap();
    let signer = CountingSigner::new();
    let runtime = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (original, _) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    *signer.failure.lock().unwrap() = Some(radroots_signing::error::Kind::AuthorizationDenied);
    assert!(
        runtime
            .prepare_submission_upload_at(
                upload(&request, 2),
                Some(renewal(&original, false)),
                expiration(&original)
            )
            .await
            .is_err()
    );
    let denied = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(denied.media()[0].upload_authorizations().len(), 2);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        denied
    );
    signer.pause.store(true, Ordering::SeqCst);
    let task = {
        let runtime = runtime.clone();
        let input = upload(&request, 3);
        let retry = renewal(&denied, false);
        let now = expiration(&denied);
        tokio::spawn(async move {
            runtime
                .prepare_submission_upload_at(input, Some(retry), now)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    let lost = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(lost.media()[0].upload_authorizations().len(), 3);
    assert!(
        runtime
            .prepare_submission_upload_at(
                upload(&request, 4),
                Some(renewal(&lost, false)),
                expiration(&lost)
            )
            .await
            .is_err()
    );
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    assert_eq!(
        runtime
            .prepare_submission_upload_at(
                upload(&request, 4),
                Some(renewal(&lost, false)),
                expiration(&lost)
            )
            .await
            .err(),
        Some(SubmissionOperationError::UploadAttemptsExhausted)
    );
    assert_eq!(signer.count(), 3);
    runtime.shutdown().await.unwrap();
}
