use super::{coordinate_effect_support as effects, coordinate_support::*, *};
use crate::runtime::product_surface::{
    LocalNetwork, LocalNetworkRelayPolicy, PublicationActionReason, PublicationRetryDecision,
};
use radroots_storage::EventStore;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[tokio::test]
async fn coordinate_signer_suspension_fences_other_writer_and_rechecks_remote_winner_before_admission()
 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = format!("ws://{}", listener.local_addr().unwrap());
    let signer = effects::PausedSigner::new();
    let runtime = effects::runtime(signer.clone(), &relay);
    let first = saved(&runtime, [141; 16], "signing:race").await;
    let second = saved(&runtime, [142; 16], "signing:race").await;
    let queued = queue(&runtime, &first).await.unwrap();
    let owner = Arc::clone(&runtime);
    let revision = queued.draft().revision().get();
    let signing =
        tokio::spawn(async move { owner.phase1_advance_draft([141; 16], revision).await });
    signer.wait().await;
    assert_eq!(
        queue(&runtime, &second).await.unwrap_err(),
        Phase1DraftError::OperationInProgress
    );
    retain(
        &runtime,
        signed_head(SECRET, 31_923, "signing:race", 1_950_000_000),
    )
    .await;
    signer.resume.notify_one();
    assert_eq!(
        signing.await.unwrap().unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let status = runtime.phase1_draft_status([141; 16]).await.unwrap();
    assert!(!status.coordinate_writable());
    assert!(status.push().unwrap().artifact().signed().is_some());
    assert!(
        !status
            .push()
            .unwrap()
            .artifact()
            .admission_state()
            .is_admitted()
    );
    assert!(status.push().unwrap().delivery_plan().attempts().is_empty());
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        EventStore::status(runtime.client.storage().unwrap())
            .await
            .unwrap()
            .raw_events(),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn coordinate_scoped_capture_preserves_identity_and_cannot_bypass_legacy_ownership() {
    let signer = effects::PausedSigner::new();
    signer.pause.store(false, Ordering::SeqCst);
    let runtime = effects::runtime(signer.clone(), "ws://127.0.0.1:19999");
    let legacy = saved(&runtime, [151; 16], "shared:coordinate").await;
    let legacy = queue(&runtime, &legacy).await.unwrap();
    let request = effects::scoped(&runtime, 153, "shared:coordinate").await;
    let captured = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(
        captured.retry_decision(),
        PublicationRetryDecision::NeedsAction(PublicationActionReason::CoordinateChanged)
    );
    assert!(
        runtime
            .submission_queue(&request, captured.intent().revision().get())
            .await
            .is_err()
    );
    assert!(
        runtime
            .submission_advance(&request, captured.intent().revision().get())
            .await
            .is_err()
    );
    assert_eq!(signer.calls.load(Ordering::SeqCst), 0);
    runtime
        .phase1_cancel_draft(
            [151; 16],
            legacy.draft().revision().get(),
            legacy.draft().updated_at_unix_ms() + 1,
        )
        .await
        .unwrap();
    let scoped = runtime
        .submission_queue(&request, captured.intent().revision().get())
        .await
        .unwrap();
    assert_eq!(scoped.intent().payload(), captured.intent().payload());
    assert_eq!(scoped.receipt(), captured.receipt());
    assert!(scoped.retry_decision().may_start());
    let contender = saved(&runtime, [155; 16], "shared:coordinate").await;
    assert_eq!(
        queue(&runtime, &contender).await.unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
}

#[tokio::test]
async fn coordinate_scoped_signing_rechecks_winner_and_local_reconciliation_cannot_bypass_hold() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = format!("ws://{}", listener.local_addr().unwrap());
    let signer = effects::PausedSigner::new();
    let runtime = effects::runtime(signer.clone(), &relay);
    let request = effects::scoped(&runtime, 161, "scoped:race").await;
    let initial = runtime.submission_operation_status(&request).await.unwrap();
    let owner = runtime.clone();
    let captured_request = request.clone();
    let signing = tokio::spawn(async move { owner.submission_advance(&captured_request, 1).await });
    signer.wait().await;
    retain(
        &runtime,
        signed_head(SECRET, 31_923, "scoped:race", 1_950_000_000),
    )
    .await;
    signer.resume.notify_one();
    assert!(signing.await.unwrap().is_err());
    let status = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(status.receipt(), initial.receipt());
    assert_eq!(status.intent().payload(), initial.intent().payload());
    assert_eq!(
        status.retry_decision(),
        PublicationRetryDecision::NeedsAction(PublicationActionReason::CoordinateChanged)
    );
    assert!(status.push().artifact().signed().is_some());
    assert!(!status.push().artifact().admission_state().is_admitted());
    let context = LocalNetwork::new_for_relay_policy(
        "nearby".into(),
        "Nearby".into(),
        vec![relay],
        None,
        vec![],
        1,
        LocalNetworkRelayPolicy::Simulator,
    )
    .unwrap();
    assert!(
        runtime
            .submission_reconcile_local(&request, &context)
            .await
            .is_err()
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        status
    );
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        EventStore::status(runtime.client.storage().unwrap())
            .await
            .unwrap()
            .raw_events(),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}
