use std::{sync::atomic::Ordering, time::Duration};

use super::{operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{Phase1OutboxState, PublicationDeliveryState};

#[tokio::test]
async fn stop_before_effects_is_durable_idempotent_and_preserves_capture() {
    for sqlite in [false, true] {
        for media in [false, true] {
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
                runtime.submission_request_stop(&request).await.unwrap_err(),
                SubmissionOperationError::NotFound
            );
            prepare(&runtime, &request, media).await;
            let before = runtime.submission_operation_status(&request).await.unwrap();
            let stopped = runtime.submission_request_stop(&request).await.unwrap();
            assert_eq!(stopped.state(), Phase1OutboxState::Cancelled);
            assert_eq!(
                stopped.delivery_evidence().state,
                PublicationDeliveryState::NotIssued
            );
            assert!(
                stopped
                    .delivery_evidence()
                    .stop_requested_at_unix_ms
                    .is_some()
            );
            assert_eq!(stopped.intent(), before.intent());
            assert_eq!(stopped.receipt(), before.receipt());
            assert_eq!(stopped.captured(), before.captured());
            assert_eq!(
                runtime.submission_request_stop(&request).await.unwrap(),
                stopped
            );
            assert_eq!(
                runtime.submission_queue(&request, 1).await.unwrap_err(),
                SubmissionOperationError::Stopped
            );
            assert_eq!(
                runtime.submission_advance(&request, 1).await.unwrap_err(),
                SubmissionOperationError::Stopped
            );
            if media {
                assert_eq!(
                    runtime
                        .submission_prepare_native_upload(super::media_test_support::upload(
                            &request, 1
                        ))
                        .await
                        .err()
                        .unwrap(),
                    SubmissionOperationError::Stopped
                );
            }
            assert_eq!(signer.count(), 0);
            runtime.shutdown().await.unwrap();
            drop(runtime);
            if sqlite {
                let reopened =
                    self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
                assert_eq!(
                    reopened
                        .submission_operation_status(&request)
                        .await
                        .unwrap(),
                    stopped
                );
                assert_eq!(
                    reopened.submission_request_stop(&request).await.unwrap(),
                    stopped
                );
                assert_eq!(signer.count(), 0);
                reopened.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn stop_during_signer_does_not_wait_for_application_admission_or_erase_signature() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        signer.pause.store(true, Ordering::SeqCst);
        let runtime = runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
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
        let stopped = tokio::time::timeout(
            Duration::from_secs(2),
            runtime.submission_request_stop(&request),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            stopped.delivery_evidence().state,
            PublicationDeliveryState::NotIssued
        );
        signer.resume.notify_one();
        let _result = task.await.unwrap();
        let late = runtime.submission_operation_status(&request).await.unwrap();
        assert!(late.push().artifact().signed().is_some());
        assert!(!late.push().artifact().admission_state().is_admitted());
        assert_eq!(late.delivery_evidence(), stopped.delivery_evidence());
        assert_eq!(signer.count(), 1);
        assert_eq!(late.state(), Phase1OutboxState::Cancelled);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let reopened =
                self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
            assert_eq!(
                reopened
                    .submission_operation_status(&request)
                    .await
                    .unwrap(),
                late
            );
            assert_eq!(
                reopened
                    .submission_advance(&request, late.intent().revision().get())
                    .await
                    .unwrap_err(),
                SubmissionOperationError::Stopped
            );
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn stop_after_acceptance_preserves_success_and_first_stop() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let (url, server) = relay().await;
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), &url).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let done = runtime.submission_advance(&request, 1).await.unwrap();
        server.await.unwrap();
        let stopped = runtime.submission_request_stop(&request).await.unwrap();
        assert_eq!(stopped.state(), Phase1OutboxState::Complete);
        assert_eq!(
            stopped.delivery_evidence().state,
            PublicationDeliveryState::Accepted
        );
        assert_eq!(
            stopped.push().delivery_plan().delivery_facts(),
            done.push().delivery_plan().delivery_facts()
        );
        assert_eq!(
            runtime.submission_request_stop(&request).await.unwrap(),
            stopped
        );
        assert_eq!(signer.count(), 1);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let reopened =
                self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
            assert_eq!(
                reopened
                    .submission_operation_status(&request)
                    .await
                    .unwrap(),
                stopped
            );
            reopened.shutdown().await.unwrap();
        }
    }
}
