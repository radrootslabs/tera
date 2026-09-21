use std::{sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use radroots_event::admission::{AdmissionPolicy, RawEvent, VisibilityPolicy};
use radroots_event_codec::{
    admission::admit_verified_event,
    verify::{Nip01SignatureVerifier, verify_nip01_event},
};
use radroots_storage::event::{EventAdmission, EventQueryBounds};
use radroots_transport::{
    TransportId,
    source::{EventProvenance, ObservedEvent},
};
use tokio::{net::TcpListener, sync::Notify};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{operation_test_support::*, test_support::*};
use crate::runtime::product_surface::{PublicationDeliveryState, PublicationTargetPolicy};

async fn controlled_relay(
    ok: Option<bool>,
    release: Arc<Notify>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(90), async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            while let Some(message) = socket.next().await {
                if let Message::Text(text) = message.unwrap() {
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if value[0] != "EVENT" {
                        continue;
                    }
                    if let Some(ok) = ok {
                        let detail = if ok { "" } else { "blocked: fixture refusal" };
                        let reply = serde_json::json!(["OK", value[1]["id"], ok, detail]);
                        // Duplicate matching acknowledgements must not duplicate a target or attempt.
                        for _ in 0..2 {
                            socket
                                .send(Message::Text(reply.to_string().into()))
                                .await
                                .unwrap();
                        }
                    }
                    release.notified().await;
                    return;
                }
            }
            panic!("missing original event");
        })
        .await
        .unwrap();
    });
    (url, task)
}

struct VisibleTestPolicy;
impl AdmissionPolicy for VisibleTestPolicy {
    type Error = std::convert::Infallible;
    fn policy_id(&self) -> &'static str {
        "tera.target-receipt-test"
    }
    fn admit(
        &self,
        _: &radroots_event::admission::ContractValidatedEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
impl VisibilityPolicy for VisibleTestPolicy {
    type Error = std::convert::Infallible;
    fn policy_id(&self) -> &'static str {
        "tera.target-receipt-test"
    }
    fn make_visible(
        &self,
        _: &radroots_event::admission::AdmittedEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

async fn read_back(
    runtime: &crate::TeraRuntime,
    status: &super::SubmissionOperationStatus,
    index: usize,
    at: u64,
) {
    let event = status.push().artifact().signed().unwrap().event().clone();
    let verified = verify_nip01_event(event.envelope().clone()).unwrap();
    let contract = admit_verified_event(verified.clone())
        .unwrap()
        .contract_id();
    let visible = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Nip01SignatureVerifier)
        .unwrap()
        .validate_contract_for_admission(contract)
        .unwrap()
        .admit_with(&VisibleTestPolicy)
        .unwrap()
        .make_visible_with(&VisibleTestPolicy)
        .unwrap();
    let target = status
        .push()
        .delivery_plan()
        .intent()
        .target_set()
        .targets()[index]
        .fingerprint()
        .clone();
    let observed = ObservedEvent::new(
        event,
        EventProvenance::new(TransportId::NOSTR, target, at).unwrap(),
    );
    runtime
        .client
        .storage()
        .unwrap()
        .admit(EventAdmission::visible(observed, visible).unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn target_receipts_keep_mixed_outcomes_and_lost_ok_readback_distinct() {
    for mixed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut urls = Vec::new();
        let mut servers = Vec::new();
        let outcomes = if mixed {
            vec![Some(true), Some(false), None]
        } else {
            vec![None]
        };
        for outcome in outcomes {
            let release = Arc::new(Notify::new());
            let (url, task) = controlled_relay(outcome, release.clone()).await;
            urls.push(url);
            servers.push((release, task));
        }
        let signer = CountingSigner::new();
        let runtime = runtime(mixed.then_some(root.path()), signer.clone(), &urls[0]).await;
        runtime
            .configure_simulator_relays(urls.clone())
            .await
            .unwrap();
        let request = request();
        prepare(&runtime, &request, false).await;
        let before = runtime.submission_operation_status(&request).await.unwrap();
        assert!(matches!(
            before.target_details().policy,
            PublicationTargetPolicy::All
        ));
        assert_eq!(before.target_details().targets.len(), urls.len());
        assert!(
            before
                .target_details()
                .targets
                .iter()
                .all(|t| !t.attempted && !t.accepted && !t.uncertain)
        );
        let sent = runtime.submission_advance(&request, 1).await.unwrap();
        for (release, task) in servers {
            release.notify_one();
            task.await.unwrap();
        }
        assert_eq!(signer.count(), 1);
        assert_eq!(sent.delivery_evidence().retained_facts, 1);
        assert_eq!(sent.push().delivery_history().claims().len(), 1);
        // A result at lease expiry is retained as a claim-bound fact even if
        // scheduling reconciliation has not yet folded it into attempts.
        assert!(sent.delivery_evidence().recorded_attempts <= 1);
        let targets = &sent.target_details().targets;
        assert!(
            targets
                .iter()
                .all(|t| t.attempted && t.read_back_observed_at_unix_ms.is_none())
        );
        assert_eq!(
            targets.iter().filter(|t| t.accepted).count(),
            usize::from(mixed)
        );
        if mixed {
            assert!(targets[0].accepted);
            assert!(targets[1].rejected);
        }
        let last = targets.len() - 1;
        assert!(targets[last].uncertain && !targets[last].accepted);
        let expected = if mixed {
            PublicationDeliveryState::PartiallyAccepted
        } else {
            PublicationDeliveryState::Unknown
        };
        assert_eq!(sent.delivery_evidence().state, expected);
        // Admit the exact verified event as inbound provenance, twice. It cannot replace a lost OK.
        read_back(&runtime, &sent, last, NOW).await;
        read_back(&runtime, &sent, last, NOW).await;
        let observed = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(observed.delivery_evidence(), sent.delivery_evidence());
        assert_eq!(
            observed.target_details().targets[last].read_back_observed_at_unix_ms,
            Some(NOW)
        );
        assert!(!observed.target_details().targets[last].accepted);
        let page = runtime
            .client
            .storage()
            .unwrap()
            .query_provenance(
                *sent.push().artifact().signed().unwrap().event().id(),
                EventQueryBounds::first(10).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            page.items()
                .iter()
                .filter(|p| p.provenance().transport_id() == TransportId::NOSTR)
                .count(),
            1
        );
        runtime
            .configure_simulator_relays(vec!["ws://127.0.0.1:19998".into()])
            .await
            .unwrap();
        let removed = runtime.submission_operation_status(&request).await.unwrap();
        assert!(
            removed
                .delivery_evidence()
                .stop_requested_at_unix_ms
                .is_some()
        );
        assert!(removed.target_details() == observed.target_details());
        assert_eq!(signer.count(), 1);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if mixed {
            let reopened =
                self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
            assert_eq!(
                reopened
                    .submission_operation_status(&request)
                    .await
                    .unwrap(),
                removed
            );
            assert_eq!(signer.count(), 1);
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn target_readback_is_bounded_and_storage_failure_does_not_erase_delivery() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19997").await;
    let request = request();
    prepare(&runtime, &request, false).await;
    runtime.submission_queue(&request, 1).await.unwrap();
    let (loaded, _) = runtime.load_submission_operation(&request).await.unwrap();
    runtime
        .sync()
        .unwrap()
        .sign_prepared(loaded.request)
        .await
        .unwrap();
    let signed = runtime.submission_operation_status(&request).await.unwrap();
    assert!(signed.target_details().read_back_complete);
    assert!(
        signed.target_details().targets[0]
            .read_back_observed_at_unix_ms
            .is_none()
    );
    for at in NOW..NOW + 1_001 {
        read_back(&runtime, &signed, 0, at).await;
    }
    let observed = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(observed.delivery_evidence(), signed.delivery_evidence());
    assert!(observed.target_details().read_back_available);
    assert!(!observed.target_details().read_back_complete);
    assert!(
        observed.target_details().targets[0]
            .read_back_observed_at_unix_ms
            .is_some()
    );
    assert!(!observed.target_details().targets[0].accepted);
    let store = runtime.client.storage().unwrap();
    runtime.shutdown().await.unwrap();
    let unavailable =
        crate::runtime::product_surface::PublicationTargetDetails::load(observed.push(), store)
            .await;
    assert!(!unavailable.read_back_available && !unavailable.read_back_complete);
    assert!(!unavailable.targets[0].accepted);
    assert_eq!(signer.count(), 1);
}
