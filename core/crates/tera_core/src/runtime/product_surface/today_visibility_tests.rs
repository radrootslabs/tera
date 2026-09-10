use super::sync_tests::runtime;
use super::tests::{context, ingest, signed};
use super::*;
use radroots_event::{SignedEvent, admission::RawEvent};
use radroots_event_codec::verify::Nip01SignatureVerifier;
use radroots_transport::{
    Error, EventSource, FetchPage, FetchRequest, SourceStatus, Target, TransportId,
    outcome::{FetchTargetOutcome, FetchTargetState},
    source::{EventProvenance, NextPage, ObservedEvent},
};
use std::sync::Arc;

struct ReplacementSource(Vec<SignedEvent>);

impl EventSource for ReplacementSource {
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
                    .map(|e| ObservedEvent::new(e, provenance.clone()))
                    .collect(),
                vec![FetchTargetOutcome::new(target, FetchTargetState::Complete)],
                NextPage::Complete,
            )
        })
    }
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

fn head(kind: u32, valid: bool, timestamp: u64) -> SignedEvent {
    let tags = match kind {
        0 => vec![],
        30_402 => vec![
            vec!["d", "same"],
            vec!["title", "Carrots"],
            vec!["summary", "Fresh"],
            vec!["published_at", "2000000000"],
            vec!["location", "Victoria"],
            vec!["price", if valid { "3" } else { "-1" }, "CAD"],
            vec!["radroots:price_unit", "lb"],
            vec!["status", "active"],
        ],
        31_922 => vec![
            vec!["d", "same"],
            vec!["title", "Market"],
            vec!["start", if valid { "2033-05-18" } else { "2033-02-30" }],
        ],
        31_923 => vec![
            vec!["d", "same"],
            vec!["title", "Market"],
            vec!["start", if valid { "2000000300" } else { "-1" }],
            vec!["D", "23148"],
        ],
        _ => unreachable!("closed fixture families"),
    };
    let content = if kind == 0 {
        if valid {
            r#"{"display_name":"Original Farm","bot":false}"#
        } else {
            "malformed profile"
        }
    } else if kind == 30_402 {
        "available"
    } else {
        ""
    };
    signed(kind, tags, content, timestamp)
}

#[tokio::test]
async fn malformed_newer_signed_head_never_revives_an_old_card_or_profile() {
    let selected = context(None, 1);
    for kind in [0, 30_402, 31_922, 31_923] {
        let old = head(kind, true, 2_000_000_000);
        let newer = head(kind, false, 2_000_000_001);
        let valid = admit_verified_event(verify_nip01_event(old.envelope().clone()).unwrap());
        assert!(valid.is_ok(), "valid kind {kind}: {valid:?}");
        assert!(
            admit_verified_event(verify_nip01_event(newer.envelope().clone()).unwrap()).is_err()
        );
        let runtime = runtime(Arc::new(ReplacementSource(vec![newer.clone()])));
        ingest(&runtime, &selected, old.clone(), 2_000_000_100).await;
        let receipt = runtime
            .phase1_sync_today(&selected, 2_000_000_101, TodayProjectionUpdate::Incremental)
            .await
            .unwrap();
        assert_eq!(receipt.events_admitted, 1, "retain signed head kind {kind}");
        assert_eq!(receipt.projection.source_events, 2);
        assert_eq!(receipt.projection.visible_cards, 0);
        assert_eq!(receipt.projection.profiles, 0);
        let snapshot = EventStore::rebuild_visibility(runtime.client.storage().unwrap())
            .await
            .unwrap();
        assert_eq!(snapshot.current_heads()[0].event_id, *newer.id());
        assert_eq!(snapshot.superseded_event_ids(), &[*old.id()]);
        assert!(snapshot.visible_event_ids().is_empty());
        assert!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_102))
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }
}

#[tokio::test]
async fn same_raw_count_visibility_advancement_rebuilds_the_cached_projection() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let event = signed(1, vec![], "newly visible", 2_000_000_000);
    let storage = runtime.client.storage().unwrap();
    EventStore::admit(storage, EventAdmission::raw(observed(event.clone())))
        .await
        .unwrap();
    let before = runtime
        .phase1_refresh_today(&selected, 2_000_000_100, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!((before.source_events, before.visible_cards), (1, 0));
    let verified = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Nip01SignatureVerifier)
        .unwrap();
    EventStore::admit(
        storage,
        EventAdmission::verified(observed(event.clone()), verified).unwrap(),
    )
    .await
    .unwrap();
    ingest(&runtime, &selected, event.clone(), 2_000_000_101).await;
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_102))
        .await
        .unwrap();
    assert_eq!(
        page.items.len(),
        1,
        "same raw count must not hide newly visible content"
    );
    assert_eq!(page.items[0].card.source_event_id, event.id().to_hex());
    let after = runtime
        .phase1_refresh_today(&selected, 2_000_000_103, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_ne!(after.content_generation, before.content_generation);
    assert_eq!(EventStore::status(storage).await.unwrap().raw_events(), 1);
}

#[tokio::test]
async fn invalid_signature_cannot_gain_replacement_authority() {
    let selected = context(None, 1);
    let old = head(30_402, true, 2_000_000_000);
    let mut wire = head(30_402, false, 2_000_000_001).wire().clone();
    wire.sig = "0".repeat(128);
    let invalid =
        radroots_event_codec::decode::signed_event(&serde_json::to_string(&wire).unwrap()).unwrap();
    let runtime = runtime(Arc::new(ReplacementSource(vec![invalid])));
    ingest(&runtime, &selected, old.clone(), 2_000_000_100).await;
    let receipt = runtime
        .phase1_sync_today(&selected, 2_000_000_101, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!((receipt.events_admitted, receipt.events_rejected), (0, 1));
    assert_eq!(receipt.projection.source_events, 1);
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_102))
        .await
        .unwrap();
    assert_eq!(page.items[0].card.source_event_id, old.id().to_hex());
}

fn sign_as_other_author(event: &SignedEvent) -> SignedEvent {
    use nostr::secp256k1::{Keypair, Message, SECP256K1};
    let keys = nostr::Keys::parse(&"02".repeat(32)).unwrap();
    let mut wire = event.wire().clone();
    wire.pubkey = keys.public_key().to_string();
    let id = wire.computed_event_id().unwrap();
    wire.id = id.to_hex();
    wire.sig = SECP256K1
        .sign_schnorr_no_aux_rand(
            &Message::from_digest(*id.as_bytes()),
            &Keypair::from_secret_key(SECP256K1, keys.secret_key()),
        )
        .to_string();
    let raw = serde_json::to_string(&wire).unwrap();
    SignedEvent::from_wire_verified_id(wire, raw).unwrap()
}

#[tokio::test]
async fn forged_author_deletion_is_ineffective_and_valid_deletion_before_target_is_retained() {
    let selected = context(None, 1);
    let target = head(30_402, true, 2_000_000_000);
    let deletion = signed(5, vec![vec!["e", &target.id().to_hex()]], "", 2_000_000_010);
    let forged = sign_as_other_author(&deletion);
    assert!(verify_nip01_event(forged.envelope().clone()).is_ok());
    let runtime = runtime(Arc::new(ReplacementSource(vec![forged])));
    ingest(&runtime, &selected, target.clone(), 2_000_000_100).await;
    runtime
        .phase1_sync_today(&selected, 2_000_000_101, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_102))
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let prior = TeraRuntime::test_memory().unwrap();
    ingest(&prior, &selected, deletion, 2_000_000_100).await;
    ingest(&prior, &selected, target, 2_000_000_101).await;
    assert!(
        prior
            .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_102))
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn same_count_deletion_admission_removes_current_cached_content() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let target = head(30_402, true, 2_000_000_000);
    let deletion = signed(5, vec![vec!["e", &target.id().to_hex()]], "", 2_000_000_010);
    ingest(&runtime, &selected, target, 2_000_000_100).await;
    let storage = runtime.client.storage().unwrap();
    EventStore::admit(storage, EventAdmission::raw(observed(deletion.clone())))
        .await
        .unwrap();
    let before = runtime
        .phase1_refresh_today(&selected, 2_000_000_101, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!((before.source_events, before.visible_cards), (2, 1));
    ingest(&runtime, &selected, deletion, 2_000_000_102).await;
    assert_eq!(EventStore::status(storage).await.unwrap().raw_events(), 2);
    assert!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_103))
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn missing_visibility_metadata_is_readable_and_rebuilt_from_current_truth() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "retained content", 2_000_000_000),
        2_000_000_100,
    )
    .await;
    let storage = runtime.client.storage().unwrap();
    let generation = projection_generation().unwrap();
    let mut old = load_state(storage, &selected, generation)
        .await
        .unwrap()
        .unwrap();
    old.visibility_digest = None;
    old.content_generation = content_generation(&old).unwrap();
    let bytes = encode(&old).unwrap();
    assert!(
        !serde_json::from_slice::<serde_json::Value>(&bytes)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("visibilityDigest")
    );
    assert_eq!(decode_state(&bytes).unwrap(), old);
    ProjectionStore::put_projection_document(
        storage,
        projection_id().unwrap(),
        generation,
        ProjectionDocument::new(projection_document_key(&selected), bytes).unwrap(),
    )
    .await
    .unwrap();
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_101))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    let current = load_state(storage, &selected, generation)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(current.content_generation, old.content_generation);
    assert_eq!(
        current.visibility_digest,
        Some(
            *EventStore::rebuild_visibility(storage)
                .await
                .unwrap()
                .digest()
                .as_bytes()
        )
    );
}

#[tokio::test]
async fn equal_time_invalid_selected_head_is_order_independent() {
    let selected = context(None, 1);
    let old = head(0, true, 2_000_000_000);
    let newer = (0..256)
        .map(|i| signed(0, vec![], &format!("invalid {i}"), 2_000_000_000))
        .find(|candidate| candidate.id() < old.id())
        .expect("deterministic lower-ID malformed fixture");
    for events in [
        vec![old.clone(), newer.clone()],
        vec![newer.clone(), old.clone()],
    ] {
        let runtime = runtime(Arc::new(ReplacementSource(events)));
        let receipt = runtime
            .phase1_sync_today(&selected, 2_000_000_100, TodayProjectionUpdate::Incremental)
            .await
            .unwrap();
        assert_eq!(receipt.events_admitted, 2);
        assert_eq!(receipt.projection.profiles, 0);
        let snapshot = EventStore::rebuild_visibility(runtime.client.storage().unwrap())
            .await
            .unwrap();
        assert_eq!(snapshot.current_heads()[0].event_id, *newer.id());
        assert!(snapshot.visible_event_ids().is_empty());
    }
}
