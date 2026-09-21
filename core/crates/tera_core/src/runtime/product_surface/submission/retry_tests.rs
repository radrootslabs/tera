use std::{sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{operation_test_support::*, test_support::*};
use crate::runtime::product_surface::{
    Phase1OutboxState, PublicationActionReason as Reason, PublicationRetryDecision as Decision,
};

#[tokio::test]
async fn delivery_deadline_blocks_signing_and_retains_signed_intent_across_reopen() {
    for signed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let signer = CountingSigner::new();
        let runtime = runtime(Some(root.path()), signer.clone(), &url).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        runtime.submission_queue(&request, 1).await.unwrap();
        let (loaded, _) = runtime.load_submission_operation(&request).await.unwrap();
        if signed {
            runtime
                .sync()
                .unwrap()
                .sign_prepared(loaded.request.clone())
                .await
                .unwrap();
            runtime
                .sync()
                .unwrap()
                .admit_signed(loaded.request.operation_id())
                .await
                .unwrap();
        }
        let before = runtime.submission_operation_status(&request).await.unwrap();
        let deadline = before.push().delivery_plan().intent().deadline_unix_ms();
        let reserved = runtime.submission_reserve(&request).await.unwrap();
        assert_eq!(
            deadline - reserved.reserved_at_unix_ms(),
            24 * 60 * 60 * 1000
        );
        assert_eq!(
            runtime
                .publication_retry_at(before.push(), deadline - 1)
                .unwrap(),
            Decision::Ready
        );
        for now in [deadline, deadline + 1] {
            let expired = runtime
                .submission_operation_status_at(&request, now)
                .await
                .unwrap();
            assert_eq!(expired.state(), Phase1OutboxState::Terminal);
            assert_eq!(expired.captured(), before.captured());
            assert_eq!(
                runtime.publication_retry_at(before.push(), now).unwrap(),
                Decision::NeedsAction(Reason::DeadlineExceeded)
            );
            runtime
                .advance_push_request_with_clock(loaded.request.clone(), || Ok(now))
                .await
                .unwrap();
        }
        assert_eq!(signer.count(), usize::from(signed));
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        let after = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(after, before);
        let saved = before
            .push()
            .artifact()
            .signed()
            .map(|artifact| artifact.event().raw_json().to_owned());
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = self::runtime(Some(root.path()), signer.clone(), &url).await;
        let restored = reopened
            .submission_operation_status(&request)
            .await
            .unwrap();
        assert_eq!(restored.intent(), before.intent());
        assert_eq!(restored.captured(), before.captured());
        assert!(restored.target_details() == before.target_details());
        assert_eq!(
            restored
                .push()
                .artifact()
                .signed()
                .map(|artifact| artifact.event().raw_json().to_owned()),
            saved
        );
        assert_eq!(
            reopened
                .publication_retry_at(restored.push(), deadline)
                .unwrap(),
            Decision::NeedsAction(Reason::DeadlineExceeded)
        );
        assert_eq!(signer.count(), usize::from(signed));
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn delivery_deadline_is_rechecked_after_signing_before_any_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), &url).await;
    let request = request();
    prepare(&runtime, &request, false).await;
    runtime.submission_queue(&request, 1).await.unwrap();
    let (loaded, before) = runtime.load_submission_operation(&request).await.unwrap();
    let deadline = before.delivery_plan().intent().deadline_unix_ms();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    runtime
        .advance_push_request_with_clock(loaded.request, || {
            let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if call == 0 { deadline - 1 } else { deadline })
        })
        .await
        .unwrap();
    let after = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(signer.count(), 1);
    assert!(after.push().artifact().signed().is_some());
    assert!(after.push().artifact().admission_state().is_admitted());
    assert_eq!(after.delivery_evidence().recorded_attempts, 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    runtime.shutdown().await.unwrap();
}

async fn refusal(text: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(15), async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            while let Some(message) = socket.next().await {
                if let Message::Text(wire) = message.unwrap() {
                    let frame: serde_json::Value = serde_json::from_str(&wire).unwrap();
                    if frame[0] == "EVENT" {
                        socket
                            .send(Message::Text(
                                serde_json::json!(["OK", frame[1]["id"], false, text])
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                        return frame[1]["id"].as_str().unwrap().to_owned();
                    }
                }
            }
            panic!("missing authored event");
        })
        .await
        .unwrap()
    });
    (url, task)
}

#[tokio::test]
async fn delivery_refusals_require_specific_action_without_retry_or_resigning() {
    for (message, reason) in [
        (
            "auth-required: private-account-detail",
            Reason::AuthenticationRequired,
        ),
        (
            "quota exceeded: private-account-detail",
            Reason::QuotaExceeded,
        ),
        ("malformed: private-payload-detail", Reason::InvalidPayload),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (url, server) = refusal(message).await;
        let signer = CountingSigner::new();
        let runtime = runtime(Some(root.path()), signer.clone(), &url).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let status = runtime.submission_advance(&request, 1).await.unwrap();
        let id = server.await.unwrap();
        assert_eq!(status.retry_decision(), Decision::NeedsAction(reason));
        let raw = status
            .push()
            .artifact()
            .signed()
            .unwrap()
            .event()
            .raw_json()
            .to_owned();
        assert_eq!(
            hex::encode(
                status
                    .push()
                    .artifact()
                    .signed()
                    .unwrap()
                    .event()
                    .id()
                    .as_bytes()
            ),
            id
        );
        assert_eq!(status.target_details().targets.len(), 1);
        assert!(status.target_details().targets[0].attempted);
        assert!(!status.target_details().targets[0].accepted);
        for _ in 0..2 {
            let next = runtime
                .submission_advance(&request, status.intent().revision().get())
                .await
                .unwrap();
            assert_eq!(next.retry_decision(), Decision::NeedsAction(reason));
            assert_eq!(next.delivery_evidence(), status.delivery_evidence());
            assert_eq!(
                next.push().artifact().signed().unwrap().event().raw_json(),
                raw
            );
            assert_eq!(next.intent(), status.intent());
        }
        assert_eq!(signer.count(), 1);
        assert_redacted(&status);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = self::runtime(Some(root.path()), Arc::clone(&signer), &url).await;
        let restored = reopened
            .submission_operation_status(&request)
            .await
            .unwrap();
        assert_eq!(restored.retry_decision(), Decision::NeedsAction(reason));
        assert_eq!(restored.intent(), status.intent());
        assert!(restored.target_details() == status.target_details());
        assert_eq!(
            restored
                .push()
                .artifact()
                .signed()
                .unwrap()
                .event()
                .raw_json(),
            raw
        );
        assert_eq!(signer.count(), 1);
        reopened.shutdown().await.unwrap();
    }
}
