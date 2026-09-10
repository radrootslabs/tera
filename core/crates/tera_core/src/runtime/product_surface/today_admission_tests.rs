use super::sync_tests::runtime;
use super::tests::{context, signed};
use super::*;
use radroots_event::{
    SignedEvent,
    admission::{RawEvent, SignatureVerifier, VisibilityPolicy},
};
use radroots_event_codec::verify::Nip01SignatureVerifier;
use radroots_transport::{
    Error, EventSource, FetchPage, FetchRequest, SourceStatus, Target, TransportId,
    outcome::{FetchTargetOutcome, FetchTargetState},
    source::{EventProvenance, NextPage, ObservedEvent},
};
use std::sync::Arc;

struct AdmissionSource(Vec<SignedEvent>);

impl EventSource for AdmissionSource {
    fn status(&self) -> radroots_transport::BoxFuture<'_, Result<SourceStatus, Error>> {
        Box::pin(async { Err(Error::UnsupportedOperation) })
    }

    fn fetch(
        &self,
        request: FetchRequest,
    ) -> radroots_transport::BoxFuture<'_, Result<FetchPage, Error>> {
        Box::pin(async move {
            let target = request.target_set().targets()[0].fingerprint().clone();
            let provenance =
                EventProvenance::new(TransportId::NOSTR, target.clone(), 2_000_000_100_000)
                    .unwrap();
            FetchPage::for_request(
                &request,
                self.0
                    .iter()
                    .cloned()
                    .map(|event| ObservedEvent::new(event, provenance.clone()))
                    .collect(),
                vec![FetchTargetOutcome::new(target, FetchTargetState::Complete)],
                NextPage::Complete,
            )
        })
    }
}

fn invalid_signature() -> SignedEvent {
    let original = signed(1, vec![], "invalid signature", 2_000_000_000);
    let mut wire = original.wire().clone();
    wire.sig = "0".repeat(128);
    let raw = serde_json::to_string(&wire).unwrap();
    let event = radroots_event_codec::decode::signed_event(&raw).unwrap();
    assert!(verify_nip01_event(event.envelope().clone()).is_err());
    event
}

fn observed(event: SignedEvent) -> ObservedEvent {
    let target = Target::nostr_relay("wss://relay.example").unwrap();
    ObservedEvent::new(
        event,
        EventProvenance::new(
            TransportId::NOSTR,
            target.fingerprint().clone(),
            2_000_000_100_000,
        )
        .unwrap(),
    )
}

// Shared typestates accept caller-supplied verifier/policy implementations.
// The app's public direct-ingest boundary must use its actual crypto/profile
// rules before writing even if another host supplied permissive evidence.
pub(super) struct PermissiveEvidence;
impl SignatureVerifier for PermissiveEvidence {
    fn verify_signature(
        &self,
        _: &radroots_event::envelope::EventEnvelope,
    ) -> Result<(), radroots_event::admission::Error> {
        Ok(())
    }
}
impl radroots_event::admission::AdmissionPolicy for PermissiveEvidence {
    type Error = std::convert::Infallible;
    fn policy_id(&self) -> &'static str {
        "tera.test.permissive-admission"
    }
    fn admit(
        &self,
        _: &radroots_event::admission::ContractValidatedEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
impl VisibilityPolicy for PermissiveEvidence {
    type Error = std::convert::Infallible;
    fn policy_id(&self) -> &'static str {
        "tera.test.permissive-visibility"
    }
    fn make_visible(
        &self,
        _: &radroots_event::admission::AdmittedEvent,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn direct_visible_ingest_cannot_poison_storage_with_permissive_signature_evidence() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let valid = signed(1, vec![], "valid", 2_000_000_000);
    let contract = admit_verified_event(verify_nip01_event(valid.envelope().clone()).unwrap())
        .unwrap()
        .contract_id();
    let event = invalid_signature();
    let visible = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&PermissiveEvidence)
        .unwrap()
        .validate_contract_for_admission(contract)
        .unwrap()
        .admit_with(&PermissiveEvidence)
        .unwrap()
        .make_visible_with(&PermissiveEvidence)
        .unwrap();
    let admission = EventAdmission::visible(observed(event), visible).unwrap();
    let result = runtime
        .phase1_ingest_visible(admission, &selected, 2_000_000_100)
        .await;
    assert_eq!(
        EventStore::status(runtime.client.storage().unwrap())
            .await
            .unwrap()
            .raw_events(),
        0,
        "an invalid signature must be rejected before durable admission"
    );
    assert!(matches!(result, Err(TodayError::EventNotVisible)));
    super::tests::ingest(&runtime, &selected, valid, 2_000_000_101).await;
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_102, "UTC"))
        .await
        .unwrap();
    assert_eq!(
        page.items.len(),
        1,
        "legitimate visible ingestion still works after refusal"
    );
}

#[tokio::test]
async fn raw_and_verified_only_observations_cannot_enter_the_direct_visible_path() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let event = signed(1, vec![], "verified but not authorized", 2_000_000_000);
    let verified = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Nip01SignatureVerifier)
        .unwrap();
    let raw = EventAdmission::raw(observed(event.clone()));
    let verified = EventAdmission::verified(observed(event), verified).unwrap();
    for admission in [raw, verified] {
        assert!(matches!(
            runtime
                .phase1_ingest_visible(admission, &context(None, 1), 2_000_000_100)
                .await,
            Err(TodayError::EventNotVisible)
        ));
    }
    assert_eq!(
        EventStore::status(runtime.client.storage().unwrap())
            .await
            .unwrap()
            .raw_events(),
        0
    );
}

#[test]
fn invalid_ids_and_explicit_unregistered_contract_versions_fail_at_shared_boundaries() {
    let event = signed(1, vec![], "current wire", 2_000_000_000);
    let mut changed: serde_json::Value = serde_json::from_str(event.raw_json()).unwrap();
    changed["content"] = serde_json::json!("tampered after signing");
    assert!(radroots_event_codec::decode::signed_event(&changed.to_string()).is_err());
    changed["id"] = serde_json::json!("0".repeat(64));
    assert!(radroots_event_codec::decode::signed_event(&changed.to_string()).is_err());
    let current = admit_verified_event(verify_nip01_event(event.envelope().clone()).unwrap())
        .unwrap()
        .contract_id();
    let unsupported = format!("{}.v999", current.strip_suffix(".v1").unwrap());
    let verified = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Nip01SignatureVerifier)
        .unwrap();
    assert!(
        verified
            .validate_contract_for_admission(&unsupported)
            .is_err()
    );
}

#[tokio::test]
async fn malformed_known_profiles_and_bad_signatures_do_not_displace_valid_batch_content() {
    let malformed = [
        signed(0, vec![], "not profile JSON", 2_000_000_001),
        signed(
            31_922,
            vec![
                vec!["d", "date"],
                vec!["title", "Invalid date"],
                vec!["start", "2026-02-30"],
            ],
            "",
            2_000_000_002,
        ),
        signed(
            31_923,
            vec![
                vec!["d", "time"],
                vec!["title", "Invalid time"],
                vec!["start", "-1"],
            ],
            "",
            2_000_000_003,
        ),
        signed(
            30_402,
            vec![
                vec!["d", "food"],
                vec!["title", "Food"],
                vec!["price", "-1", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "active"],
            ],
            "",
            2_000_000_004,
        ),
        signed(
            1,
            vec![vec!["e", "not-an-event-id", "", "root"]],
            "hostile required reference",
            2_000_000_005,
        ),
    ];
    for event in &malformed {
        assert!(
            admit_verified_event(verify_nip01_event(event.envelope().clone()).unwrap()).is_err(),
            "fixture must fail shared admission"
        );
    }
    let valid = signed(
        1,
        vec![],
        "ordinary prose about food and events",
        2_000_000_010,
    );
    let valid_id = valid.id().to_hex();
    let mut events = malformed.to_vec();
    events.push(invalid_signature());
    events.push(valid);
    let runtime = runtime(Arc::new(AdmissionSource(events)));
    let selected = context(None, 1);
    let receipt = runtime
        .phase1_sync_today(&selected, 2_000_000_100, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(
        (
            receipt.events_observed,
            receipt.events_admitted,
            receipt.events_rejected
        ),
        (7, 5, 2)
    );
    assert_eq!(
        (
            receipt.projection.source_events,
            receipt.projection.visible_cards
        ),
        (5, 1)
    );
    let raw = EventStore::query_raw(
        runtime.client.storage().unwrap(),
        EventQuery::all(EventQueryBounds::first(10).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(
        raw.items()
            .iter()
            .filter(|row| row.stage() == radroots_storage::event::AdmissionStage::Verified)
            .count(),
        4,
        "malformed signed heads are retained as evidence, never visible content"
    );
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_100, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items[0].card.source_event_id, valid_id);
    assert_eq!(
        page.items[0].card.card_type,
        TodayCardType::Update,
        "prose does not select a product profile"
    );
}

#[tokio::test]
async fn an_out_of_selector_kind_is_rejected_before_today_ingest() {
    let runtime = runtime(Arc::new(AdmissionSource(vec![signed(
        42,
        vec![],
        "not a Today kind",
        2_000_000_000,
    )])));
    let receipt = runtime
        .phase1_sync_today(
            &context(None, 1),
            2_000_000_100,
            TodayProjectionUpdate::Incremental,
        )
        .await
        .unwrap();
    assert_eq!(receipt.termination, TodaySyncTermination::SourceFailed);
    assert_eq!(
        (
            receipt.events_observed,
            receipt.projection.source_events,
            receipt.projection.visible_cards
        ),
        (0, 0, 0)
    );
}

#[tokio::test]
async fn all_current_card_families_survive_shared_ingest_while_supporting_records_stay_supporting()
{
    let photo_hash = format!("x {}", "a".repeat(64));
    let roots = vec![
        (
            signed(1, vec![], "root update", 2_000_000_000),
            TodayCardType::Update,
        ),
        (
            signed(
                1,
                vec![vec![
                    "imeta",
                    "url https://media.example/photo.jpg",
                    &photo_hash,
                    "m image/jpeg",
                    "dim 10x20",
                    "size 123",
                    "alt Field photo",
                ]],
                "photo https://media.example/photo.jpg",
                2_000_000_001,
            ),
            TodayCardType::PhotoUpdate,
        ),
        (
            signed(
                1,
                vec![vec!["t", "RADROOTS-ASK"]],
                "Anyone have carrots?",
                2_000_000_002,
            ),
            TodayCardType::Ask,
        ),
        (
            signed(
                31_922,
                vec![
                    vec!["d", "all-day"],
                    vec!["title", "Market day"],
                    vec!["start", "2033-05-18"],
                ],
                "",
                2_000_000_003,
            ),
            TodayCardType::Event,
        ),
        (
            signed(
                31_923,
                vec![
                    vec!["d", "timed"],
                    vec!["title", "Market hour"],
                    vec!["start", "2000000200"],
                    vec!["D", "23148"],
                ],
                "",
                2_000_000_004,
            ),
            TodayCardType::Event,
        ),
        (
            signed(
                30_402,
                vec![
                    vec!["d", "carrots"],
                    vec!["title", "Carrots"],
                    vec!["summary", "Fresh"],
                    vec!["published_at", "2000000005"],
                    vec!["location", "Victoria"],
                    vec!["price", "3", "CAD"],
                    vec!["radroots:price_unit", "lb"],
                    vec!["status", "active"],
                ],
                "available",
                2_000_000_005,
            ),
            TodayCardType::FoodAvailability,
        ),
    ];
    let root_id = roots[0].0.id().to_hex();
    let mut events = roots
        .iter()
        .map(|(event, _)| event.clone())
        .collect::<Vec<_>>();
    events.extend([
        signed(0, vec![], r#"{"name":"Supported profile"}"#, 2_000_000_006),
        signed(
            1,
            vec![vec!["e", &root_id, "", "root"]],
            "reply stays in its thread",
            2_000_000_007,
        ),
    ]);
    let runtime = runtime(Arc::new(AdmissionSource(events)));
    let selected = context(None, 1);
    let receipt = runtime
        .phase1_sync_today(&selected, 2_000_000_100, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(
        (
            receipt.events_observed,
            receipt.events_admitted,
            receipt.events_rejected
        ),
        (8, 8, 0)
    );
    assert_eq!(
        (
            receipt.projection.visible_cards,
            receipt.projection.profiles,
            receipt.projection.thread_entries
        ),
        (6, 1, 1)
    );
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_100, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 6);
    for (source, kind) in roots {
        let item = page
            .items
            .iter()
            .find(|item| item.card.source_event_id == source.id().to_hex())
            .unwrap();
        assert_eq!(item.card.card_type, kind);
    }
}
