use super::*;
use super::{relay_test_support::Relay, test_support::fixture};
use crate::runtime::{builder::RuntimeBuilder, product_surface::*};
use radroots_sdk::transport::{
    RelayAccess, RelayEndpoint, RelayProfile, RelayProfileKind, RelayUrlPolicy,
};
use std::sync::Arc;

#[tokio::test]
async fn fresh_exact_reconciliation_and_consent_preserve_the_original_signed_event() {
    for accepted_before_restore in [false, true] {
        let relay = Relay::start().await;
        let (_root, config, runtime, host, request) = fixture().await;
        runtime.shutdown().await.unwrap();
        let signer = Arc::new(
            radroots_nostr::signing::LocalSigner::new(
                radroots_nostr::key::SecretKey::parse(
                    "0000000000000000000000000000000000000000000000000000000000000001",
                )
                .unwrap(),
            )
            .unwrap(),
        );
        let profile = RelayProfile::explicit(
            RelayProfileKind::Simulator,
            [
                RelayEndpoint::new(&relay.url, RelayUrlPolicy::Local, RelayAccess::ReadWrite)
                    .unwrap(),
            ],
        )
        .unwrap();
        let runtime = RuntimeBuilder::new(config.clone())
            .signer(signer.clone())
            .relay_profile(profile.clone())
            .build()
            .await
            .unwrap();
        let now = phase1_operation_now_unix_ms().unwrap();
        let saved = runtime
            .phase1_save_draft(
                [6; 16],
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("restore original").unwrap()),
                now / 1000,
                vec![],
                None,
                now,
            )
            .await
            .unwrap();
        let policy = Phase1QueuePolicy::new(
            vec![relay.url.clone()],
            Phase1RelaySatisfaction::AnyAccepted,
            now + 120_000,
            Phase1CancellationPolicy::LocalCooperative,
        )
        .unwrap();
        let queued = runtime
            .phase1_queue_draft([6; 16], saved.draft().revision().get(), policy, now + 1)
            .await
            .unwrap();
        let signed = runtime
            .phase1_sign_queued_draft([6; 16], queued.draft().revision().get())
            .await
            .unwrap();
        let push = signed.push().unwrap();
        let original = push.artifact().signed().unwrap().clone();
        let wire: serde_json::Value = serde_json::from_str(original.event().raw_json()).unwrap();
        if accepted_before_restore {
            *relay.event.lock().unwrap() = Some(wire.clone());
        }
        let target = push.delivery_plan().intent().target_set().targets()[0]
            .fingerprint()
            .as_str()
            .to_owned();
        runtime
            .capture_application_backup(request.backup().clone(), &host)
            .await
            .unwrap();
        runtime.shutdown().await.unwrap();
        let guard = restore_application_backup(config.clone(), request, &host)
            .await
            .unwrap();
        // Native guarded construction has no explicit launch relay profile.
        // Loading saved defaults must not cancel the original loopback target.
        let startup = RuntimeBuilder::new(config.clone())
            .restore_guard(guard.clone())
            .build()
            .await
            .unwrap();
        let preserved = startup.phase1_draft_status([6; 16]).await.unwrap();
        let preserved = preserved.push().unwrap();
        assert_eq!(preserved.artifact().signed().unwrap(), &original);
        assert!(
            preserved
                .delivery_plan()
                .stop_requested_at_unix_ms()
                .is_none()
        );
        assert_eq!(
            preserved.operation().operation_id(),
            push.operation().operation_id()
        );
        startup.shutdown().await.unwrap();
        let restored = RuntimeBuilder::new(config.clone())
            .restore_guard(guard.clone())
            .signer(signer.clone())
            .relay_profile(profile.clone())
            .build()
            .await
            .unwrap();
        assert_eq!(
            restored.review_restored_work().await,
            Err(RestoreError::ReconciliationRequired)
        );
        assert_eq!(
            restored
                .phase1_advance_draft([6; 16], signed.draft().revision().get())
                .await
                .unwrap_err(),
            Phase1DraftError::Restore(RestoreError::ReconciliationRequired)
        );
        assert!(relay.sent.lock().unwrap().is_empty());
        relay
            .interrupt_query
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let interrupted = restored
            .reconcile_restored_target([6; 16], &target)
            .await
            .unwrap();
        assert_eq!(interrupted.observation, RestoreObservation::Incomplete);
        assert_eq!(
            restored.review_restored_work().await,
            Err(RestoreError::ReconciliationRequired)
        );
        assert_eq!(
            restored.require_restore_effects_allowed().await,
            Err(RestoreError::ReconciliationRequired)
        );
        assert!(relay.sent.lock().unwrap().is_empty());
        relay
            .interrupt_query
            .store(false, std::sync::atomic::Ordering::SeqCst);
        // Reopen under the same durable guard after the socket interruption.
        // This also avoids depending on the shared transport's reconnect timer.
        restored.shutdown().await.unwrap();
        let restored = RuntimeBuilder::new(config)
            .restore_guard(guard)
            .signer(signer)
            .relay_profile(profile)
            .build()
            .await
            .unwrap();
        assert_eq!(
            restored.restore_status().await.unwrap().unwrap().targets[0].observation,
            Some(RestoreObservation::Incomplete)
        );
        assert_eq!(
            restored
                .reconcile_restored_target([6; 16], "foreign target")
                .await,
            Err(RestoreError::InvalidRequest)
        );
        let receipt = restored
            .reconcile_restored_target([6; 16], &target)
            .await
            .unwrap();
        assert_eq!(
            receipt.observation,
            if accepted_before_restore {
                RestoreObservation::Observed
            } else {
                RestoreObservation::NotObserved
            }
        );
        assert_eq!(hex::encode(receipt.event_id), wire["id"].as_str().unwrap());
        let review = restored.review_restored_work().await.unwrap();
        assert_eq!(
            restored.require_restore_effects_allowed().await,
            Err(RestoreError::ReconciliationRequired)
        );
        assert!(relay.sent.lock().unwrap().is_empty());
        for query in relay.requests.lock().unwrap().iter() {
            assert_eq!(query[2]["authors"][0], wire["pubkey"]);
            assert_eq!(query[2]["kinds"][0], wire["kind"]);
            assert_eq!(query[2]["since"], wire["created_at"]);
            assert_eq!(query[2]["until"], wire["created_at"]);
        }
        assert!(!relay.requests.lock().unwrap().is_empty());
        restored.resume_restored_work(review).await.unwrap();
        let after = restored
            .phase1_advance_draft([6; 16], signed.draft().revision().get())
            .await
            .unwrap();
        assert_eq!(
            after.push().unwrap().artifact().signed().unwrap(),
            &original
        );
        assert_eq!(
            after.push().unwrap().operation().operation_id(),
            push.operation().operation_id()
        );
        assert!(
            relay
                .sent
                .lock()
                .unwrap()
                .iter()
                .all(|event| event == &wire)
        );
        if !accepted_before_restore {
            assert!(!relay.sent.lock().unwrap().is_empty());
        }
        restored.resume_restored_work(review).await.unwrap();
        assert_eq!(
            restored.review_restored_work().await,
            Err(RestoreError::Conflict)
        );
        restored.shutdown().await.unwrap();
        relay.finish().await;
    }
}

#[tokio::test]
async fn local_edit_after_review_requires_new_review_before_resume() {
    let (_root, config, runtime, host, request) = fixture().await;
    runtime
        .capture_application_backup(request.backup().clone(), &host)
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    let guard = restore_application_backup(config.clone(), request, &host)
        .await
        .unwrap();
    let restored = RuntimeBuilder::new(config)
        .restore_guard(guard)
        .build()
        .await
        .unwrap();
    let review = restored.review_restored_work().await.unwrap();
    restored
        .phase1_save_profile_metadata(
            ProfileMetadataCommand::new("local".into(), None, None, None, None, None, None)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        restored.resume_restored_work(review).await,
        Err(RestoreError::Conflict)
    );
    assert_eq!(
        restored.require_restore_effects_allowed().await,
        Err(RestoreError::ReconciliationRequired)
    );
    let current = restored.review_restored_work().await.unwrap();
    assert_ne!(review, current);
    restored.resume_restored_work(current).await.unwrap();
    assert_eq!(restored.require_restore_effects_allowed().await, Ok(()));
    restored.shutdown().await.unwrap();
}
