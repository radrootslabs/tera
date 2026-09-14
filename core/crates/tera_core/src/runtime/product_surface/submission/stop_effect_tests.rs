use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{net::TcpListener, sync::Notify};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{media_test_support, operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{
    Phase1MediaStage, Phase1OutboxState, PublicationDeliveryState,
};

#[tokio::test]
async fn stopped_socket_retains_unknown_then_late_ok_across_sqlite_reopen() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let entered = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        let server = {
            let entered = entered.clone();
            let resume = resume.clone();
            tokio::spawn(async move {
                tokio::time::timeout(Duration::from_secs(15), async move {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = accept_async(stream).await.unwrap();
                    while let Some(message) = socket.next().await {
                        if let Message::Text(text) = message.unwrap() {
                            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                            if value[0] == "EVENT" {
                                entered.notify_one();
                                resume.notified().await;
                                socket
                                    .send(Message::Text(
                                        serde_json::json!(["OK", value[1]["id"], true, ""])
                                            .to_string()
                                            .into(),
                                    ))
                                    .await
                                    .unwrap();
                                return value[1].clone();
                            }
                        }
                    }
                    panic!("expected original EVENT");
                })
                .await
                .unwrap()
            })
        };
        let signer = CountingSigner::new();
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), &url).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let task = {
            let runtime = runtime.clone();
            let request = request.clone();
            tokio::spawn(async move { runtime.submission_advance(&request, 1).await })
        };
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
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
            PublicationDeliveryState::Unknown
        );
        assert!(stopped.delivery_evidence().unresolved_claims);
        assert_eq!(stopped.state(), Phase1OutboxState::Cancelled);
        assert_eq!(
            runtime.submission_request_stop(&request).await.unwrap(),
            stopped
        );
        resume.notify_one();
        let _result = task.await.unwrap();
        let sent = server.await.unwrap();
        let late = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(
            late.delivery_evidence().state,
            PublicationDeliveryState::Accepted
        );
        assert_eq!(late.state(), Phase1OutboxState::Complete);
        assert_eq!(late.delivery_evidence().retained_facts, 1);
        assert_eq!(
            late.delivery_evidence().stop_requested_at_unix_ms,
            stopped.delivery_evidence().stop_requested_at_unix_ms
        );
        assert_eq!(
            sent["id"],
            hex::encode(
                late.push()
                    .artifact()
                    .signed()
                    .unwrap()
                    .event()
                    .id()
                    .as_bytes()
            )
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
                late
            );
            assert_eq!(
                reopened.submission_request_stop(&request).await.unwrap(),
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
async fn stop_during_upload_authorization_cannot_return_a_new_native_job() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    signer.pause.store(true, Ordering::SeqCst);
    let task = {
        let runtime = runtime.clone();
        let input = media_test_support::upload(&request, 1);
        tokio::spawn(async move { runtime.submission_prepare_native_upload(input).await })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    let stopped = runtime.submission_request_stop(&request).await.unwrap();
    signer.resume.notify_one();
    assert_eq!(
        task.await.unwrap().err().unwrap(),
        SubmissionOperationError::Stopped
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        stopped
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn stopped_native_upload_can_verify_retained_response_after_reopen_without_resigning() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
        let (origin, server) = media_test_support::blob_server(photo().1).await;
        media_test_support::configure(&runtime, &origin).await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let (uploading, _) = runtime
            .submission_prepare_native_upload(media_test_support::upload(&request, 1))
            .await
            .unwrap();
        let stopped = runtime.submission_request_stop(&request).await.unwrap();
        assert_eq!(stopped.media()[0].stage(), Phase1MediaStage::Uploading);
        assert_eq!(
            stopped.delivery_evidence().state,
            PublicationDeliveryState::NotIssued
        );
        let runtime = if sqlite {
            runtime.shutdown().await.unwrap();
            drop(runtime);
            let reopened =
                self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
            media_test_support::configure(&reopened, &origin).await;
            assert_eq!(
                reopened
                    .submission_operation_status(&request)
                    .await
                    .unwrap(),
                stopped
            );
            reopened
        } else {
            runtime
        };
        let verified = runtime
            .submission_complete_native_upload(
                media_test_support::upload(&request, uploading.intent().revision().get()),
                media_test_support::response(&origin),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(verified.media()[0].stage(), Phase1MediaStage::Verified);
        assert_eq!(verified.delivery_evidence(), stopped.delivery_evidence());
        assert_eq!(verified.state(), Phase1OutboxState::Cancelled);
        assert_eq!(signer.count(), 1);
        assert_eq!(
            runtime
                .submission_advance(&request, verified.intent().revision().get())
                .await
                .unwrap_err(),
            SubmissionOperationError::Stopped
        );
        runtime.shutdown().await.unwrap();
    }
}
