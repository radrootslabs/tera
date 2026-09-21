use super::{operation_test_support::*, signed_artifact_tests::*, test_support::*};
use crate::runtime::product_surface::{
    LocalNetwork, LocalNetworkRelayPolicy, TodayPageRequest, phase1_operation_now_unix_ms,
};
use radroots_storage::EventStore;
use std::{
    io::{Read, Write},
    time::Duration,
};
use tokio::net::TcpListener;

const CHILD: &str =
    "runtime::product_surface::submission::local_admission_tests::local_overlay_child";
const BARRIER: &str = "TERA_LOCAL_OVERLAY_COMMITTED_NO_CALLER_RECEIPT";

fn context(relay: &str) -> LocalNetwork {
    LocalNetwork::new_for_relay_policy(
        "nearby".into(),
        "Nearby".into(),
        vec![relay.into()],
        None,
        vec![],
        1,
        LocalNetworkRelayPolicy::Simulator,
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "private child exercised by local_admission_recovery_survives_both_process_death_boundaries"]
async fn local_overlay_child() {
    let mut input = String::new();
    std::io::stdin()
        .take(4097)
        .read_to_string(&mut input)
        .unwrap();
    assert!(input.len() <= 4096);
    let (root, relay): (String, String) = serde_json::from_str(&input).unwrap();
    let signer = CountingSigner::new();
    let runtime = runtime(Some(std::path::Path::new(&root)), signer.clone(), &relay).await;
    let receipt = runtime
        .submission_reconcile_local(&request(), &context(&relay))
        .await
        .unwrap();
    assert!(receipt.changed());
    assert_eq!(receipt.status().push().settlement().admitted(), 1);
    assert_eq!(receipt.status().push().settlement().delivery_satisfied(), 0);
    assert_eq!(signer.count(), 0);
    // The caller outside this process never receives the typed result. Kill
    // without shutdown after the local projection write but before that handoff.
    println!("{BARRIER}");
    std::io::stdout().flush().unwrap();
    std::future::pending::<()>().await;
}

#[tokio::test]
async fn local_admission_recovery_survives_both_process_death_boundaries() {
    for after_overlay in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay = format!("ws://{}", listener.local_addr().unwrap());
        kill_after_signed_commit(root.path(), &relay);
        if after_overlay {
            kill_at_barrier(root.path(), &relay, CHILD, BARRIER);
        }
        let signer = CountingSigner::new();
        *signer.failure.lock().unwrap() = Some(radroots_signing::error::Kind::SignerUnavailable);
        let runtime = runtime(Some(root.path()), signer.clone(), &relay).await;
        let request = request();
        let before = runtime.submission_operation_status(&request).await.unwrap();
        let selected = context(&relay);
        let repaired = runtime
            .submission_reconcile_local(&request, &selected)
            .await
            .unwrap();
        assert_eq!(repaired.changed(), !after_overlay);
        let status = repaired.status();
        assert_eq!(status.receipt(), before.receipt());
        assert_eq!(status.captured(), before.captured());
        assert_eq!(
            status.push().artifact().signed(),
            before.push().artifact().signed()
        );
        assert_eq!(status.push().settlement().signed(), 1);
        assert_eq!(status.push().settlement().admitted(), 1);
        assert_eq!(status.push().settlement().delivery_satisfied(), 0);
        assert!(status.push().delivery_plan().attempts().is_empty());
        let page_request = TodayPageRequest {
            limit: 20,
            cursor: None,
            as_of: Some(phase1_operation_now_unix_ms().unwrap() / 1000),
            viewer_time_zone: Some("UTC".into()),
        };
        let page = runtime
            .phase1_today_page(&selected, page_request.clone())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let overlay = page.items[0].local_overlay.as_ref().unwrap();
        assert_eq!(
            overlay.operation_id,
            hex::encode(status.receipt().operation_id().as_bytes())
        );
        assert_eq!(overlay.state, status.state().label());
        assert_eq!(
            overlay.source_draft_id,
            Some(hex::encode(status.receipt().intent_id().as_bytes()))
        );
        let card = &page.items[0].card;
        let target = crate::runtime::product_surface::Phase1RevisionTarget::from_source(
            status.captured().form().input().command_type,
            card.card_id,
            card.source_event_id.clone(),
            card.source_address.clone(),
            card.author_pubkey.clone(),
        )
        .unwrap();
        let original_form = runtime
            .revision_source_form(*status.receipt().intent_id().as_bytes(), &target)
            .await
            .unwrap();
        assert_eq!(
            original_form.content,
            status.captured().form().input().content
        );
        assert_eq!(
            original_form.command_type,
            status.captured().form().input().command_type
        );
        assert!(
            runtime
                .revision_source_form(*status.receipt().operation_id().as_bytes(), &target)
                .await
                .is_err()
        );
        for _ in 0..3 {
            let replay = runtime
                .submission_reconcile_local(&request, &selected)
                .await
                .unwrap();
            assert!(!replay.changed());
            assert_eq!(replay.status(), status);
            assert_eq!(
                runtime
                    .phase1_today_page(&selected, page_request.clone())
                    .await
                    .unwrap(),
                page
            );
        }
        assert_eq!(
            EventStore::status(runtime.client.storage().unwrap())
                .await
                .unwrap()
                .raw_events(),
            1
        );
        assert_eq!(signer.count(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn local_reconciliation_rejects_foreign_scope_and_never_signs_an_unsigned_intent() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, false).await;
    let before = runtime.submission_operation_status(&request).await.unwrap();
    let selected = context("ws://127.0.0.1:19999");
    let receipt = runtime
        .submission_reconcile_local(&request, &selected)
        .await
        .unwrap();
    assert!(!receipt.changed());
    assert_eq!(receipt.status(), &before);
    let mut foreign = selected;
    foreign.id = crate::runtime::product_surface::LocalNetworkId::new("elsewhere".into()).unwrap();
    assert!(
        runtime
            .submission_reconcile_local(&request, &foreign)
            .await
            .is_err()
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        before
    );
    assert_eq!(signer.count(), 0);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_admission_rechecks_untrusted_stored_signature_before_event_store() {
    use radroots_storage::{authored::WorkClaim, authored_atomic::*};
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
        let request = request();
        prepare(&runtime, &request, false).await;
        runtime.submission_queue(&request, 1).await.unwrap();
        let (loaded, before) = runtime.load_submission_operation(&request).await.unwrap();
        let plan = loaded.request.plan();
        let wire = radroots_event::wire::v1::Nip01EventWire {
            id: plan.expected_event_id().to_hex(),
            pubkey: plan.author().to_hex(),
            created_at: plan.created_at(),
            kind: plan.body().kind(),
            tags: plan.body().tags().to_vec(),
            content: plan.body().content().to_owned(),
            sig: "0".repeat(128),
            extra: Default::default(),
        };
        let invalid = radroots_event::SignedEvent::from_wire_verified_id(
            wire.clone(),
            serde_json::to_string(&wire).unwrap(),
        )
        .unwrap();
        let artifact = before.artifact();
        let now = phase1_operation_now_unix_ms()
            .unwrap()
            .max(artifact.updated_at_unix_ms());
        let claim = WorkClaim::new(
            [99; 16],
            "test_untrusted_signing",
            std::num::NonZeroU64::MIN,
            now,
            now + 30000,
            artifact.revision(),
        )
        .unwrap();
        let fence =
            WorkFence::new(*claim.token(), claim.generation(), claim.row_revision()).unwrap();
        let store = runtime.client.storage().unwrap();
        store
            .execute_authored(AuthoredAtomicCommand::Claim(ClaimAuthoredWork::new(
                ClaimAuthoredTarget::ArtifactSigning(artifact.artifact_id()),
                claim,
            )))
            .await
            .unwrap();
        // The storage owner validates identity/state, not the application's
        // cryptographic trust boundary. Deliberately bypass Sync as a bad peer.
        store
            .execute_authored(AuthoredAtomicCommand::ApplySigned(
                ApplySignedArtifact::new(artifact.artifact_id(), fence, invalid, now).unwrap(),
            ))
            .await
            .unwrap();
        let untrusted = runtime.submission_operation_status(&request).await.unwrap();
        assert!(
            runtime
                .submission_reconcile_local(&request, &context("ws://127.0.0.1:19999"))
                .await
                .is_err()
        );
        assert_eq!(
            runtime.submission_operation_status(&request).await.unwrap(),
            untrusted
        );
        assert_eq!(EventStore::status(store).await.unwrap().raw_events(), 0);
        assert_eq!(signer.count(), 0);
        runtime.shutdown().await.unwrap();
    }
}
