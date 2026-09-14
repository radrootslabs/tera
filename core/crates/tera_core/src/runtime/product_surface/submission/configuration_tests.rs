use std::{sync::atomic::Ordering, time::Duration};

use super::{operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerPartialForm, IdentityLockState, IdentityRecord,
    IdentityState, MobileNetworkEnvironment, PublicationDeliveryState, RelayAccessPreference,
    RelayEndpointPreference, RelayPreferences, ReplaceMobileSettings,
};

const ORIGINAL: &str = "ws://127.0.0.1:19999";
const OTHER_RELAY: &str = "ws://127.0.0.1:19998";

#[tokio::test]
async fn relay_addition_preserves_exact_frozen_targets_and_signed_author() {
    let (url, server) = relay().await;
    let additional = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let added = format!("ws://{}", additional.local_addr().unwrap());
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), &url).await;
    let request = request();
    prepare(&runtime, &request, false).await;
    let original = runtime.submission_operation_status(&request).await.unwrap();
    runtime
        .configure_simulator_relays(vec![url, added])
        .await
        .unwrap();
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        original
    );
    let delivered = runtime.submission_advance(&request, 1).await.unwrap();
    let event = server.await.unwrap();
    assert_eq!(event["pubkey"], AUTHOR);
    assert_eq!(
        delivered.push().delivery_plan().intent(),
        original.push().delivery_plan().intent()
    );
    assert_eq!(
        delivered.delivery_evidence().state,
        PublicationDeliveryState::Accepted
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), additional.accept())
            .await
            .is_err()
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn removed_or_read_only_relay_cannot_revive_after_readdition_and_restart() {
    for sqlite in [false, true] {
        for read_only in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let signer = CountingSigner::new();
            let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), ORIGINAL).await;
            let request = request();
            prepare(&runtime, &request, false).await;
            let queued = runtime.submission_queue(&request, 1).await.unwrap();
            if read_only {
                let relays = RelayPreferences::new(
                    MobileNetworkEnvironment::Simulator,
                    vec![
                        RelayEndpointPreference::new(
                            MobileNetworkEnvironment::Simulator,
                            ORIGINAL,
                            RelayAccessPreference::ReadOnly,
                        )
                        .unwrap(),
                    ],
                )
                .unwrap();
                runtime.configure_relay_preferences(&relays).await.unwrap();
            } else {
                runtime
                    .configure_simulator_relays(vec![OTHER_RELAY.into()])
                    .await
                    .unwrap();
            }
            let stopped = runtime.submission_operation_status(&request).await.unwrap();
            assert!(
                stopped
                    .delivery_evidence()
                    .stop_requested_at_unix_ms
                    .is_some()
            );
            assert_eq!(stopped.intent(), queued.intent());
            assert_eq!(
                stopped.push().delivery_plan().intent(),
                queued.push().delivery_plan().intent()
            );
            runtime
                .configure_simulator_relays(vec![ORIGINAL.into(), OTHER_RELAY.into()])
                .await
                .unwrap();
            assert_eq!(
                runtime.submission_operation_status(&request).await.unwrap(),
                stopped
            );
            assert_eq!(
                runtime
                    .submission_advance(&request, stopped.intent().revision().get())
                    .await
                    .unwrap_err(),
                SubmissionOperationError::Stopped
            );
            assert_eq!(signer.count(), 0);
            runtime.shutdown().await.unwrap();
            drop(runtime);
            if sqlite {
                let reopened = self::runtime(Some(root.path()), signer.clone(), ORIGINAL).await;
                assert_eq!(
                    reopened
                        .submission_operation_status(&request)
                        .await
                        .unwrap(),
                    stopped
                );
                assert_eq!(
                    reopened
                        .submission_advance(&request, stopped.intent().revision().get())
                        .await
                        .unwrap_err(),
                    SubmissionOperationError::Stopped
                );
                reopened.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn restrictive_configuration_stops_held_signer_without_losing_late_signature() {
    let signer = CountingSigner::new();
    signer.pause.store(true, Ordering::SeqCst);
    let runtime = runtime(None, signer.clone(), ORIGINAL).await;
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
    tokio::time::timeout(
        Duration::from_secs(2),
        runtime.configure_simulator_relays(vec![OTHER_RELAY.into()]),
    )
    .await
    .unwrap()
    .unwrap();
    runtime
        .configure_simulator_relays(vec![ORIGINAL.into()])
        .await
        .unwrap();
    signer.resume.notify_one();
    let _result = task.await.unwrap();
    let status = runtime.submission_operation_status(&request).await.unwrap();
    assert!(
        status
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
    );
    assert!(status.push().artifact().signed().is_some());
    assert!(!status.push().artifact().admission_state().is_admitted());
    assert_eq!(
        status.delivery_evidence().state,
        PublicationDeliveryState::NotIssued
    );
    assert_eq!(
        status.intent().author(),
        request.scope().author().as_bytes()
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn settings_author_switch_and_switch_back_keep_original_operation_stopped() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), ORIGINAL).await;
        let identity = |author| {
            IdentityState::new(
                vec![IdentityRecord::new("selected", author).unwrap()],
                Some("selected".into()),
                IdentityLockState::Locked,
                None,
            )
            .unwrap()
        };
        let initial = runtime.phase1_settings().await.unwrap();
        runtime
            .phase1_replace_settings(
                ReplaceMobileSettings::new(
                    initial.revision(),
                    initial.with_identity(identity(AUTHOR)),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let request = request();
        prepare(&runtime, &request, false).await;
        let original = runtime.submission_operation_status(&request).await.unwrap();
        for author in [OTHER, AUTHOR] {
            let settings = runtime.phase1_settings().await.unwrap();
            runtime
                .phase1_replace_settings(
                    ReplaceMobileSettings::new(
                        settings.revision(),
                        settings.with_identity(identity(author)),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        let stopped = runtime.submission_operation_status(&request).await.unwrap();
        assert!(
            stopped
                .delivery_evidence()
                .stop_requested_at_unix_ms
                .is_some()
        );
        assert_eq!(stopped.intent(), original.intent());
        assert_eq!(stopped.captured(), original.captured());
        assert_eq!(
            runtime.submission_advance(&request, 1).await.unwrap_err(),
            SubmissionOperationError::Stopped
        );
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let reopened = self::runtime(Some(root.path()), signer, ORIGINAL).await;
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

#[tokio::test]
async fn transient_capture_cannot_cross_configuration_change() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), ORIGINAL).await;
    let request = request();
    runtime
        .composer_create(
            request.scope(),
            request.composer_id(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(input(AddCommandType::CreateUpdate)).unwrap(),
        )
        .await
        .unwrap();
    let captured = runtime.submission_capture(&request, vec![]).await.unwrap();
    runtime
        .configure_simulator_relays(vec![OTHER_RELAY.into()])
        .await
        .unwrap();
    runtime
        .configure_simulator_relays(vec![ORIGINAL.into()])
        .await
        .unwrap();
    assert_eq!(
        runtime.submission_commit(&captured).await.unwrap_err(),
        SubmissionCommitError::Capture(SubmissionCaptureError::PolicyUnavailable)
    );
    assert!(
        runtime
            .submission_recover(&request)
            .await
            .unwrap()
            .is_none()
    );
    runtime.submission_prepare(&request, vec![]).await.unwrap();
    assert_eq!(signer.count(), 0);
    runtime.shutdown().await.unwrap();
}
