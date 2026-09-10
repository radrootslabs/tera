use super::tests::{context, keys, signed, visible_admission};
use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_storage::projection::{
    InvalidationReason, ProjectionHealth, ProjectionInvalidation, RebuildStage, RebuildTicket,
    RebuildTicketId,
};
use std::path::Path;
const NOW: u64 = 2_000_000_000;

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("calendar_v1_fixture.json")).unwrap()
}

async fn open(root: &Path) -> TeraRuntime {
    let config = MobileUserStoreConfig::from_encoded(
        root,
        &keys().public_key().to_string(),
        &"01".repeat(32),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    RuntimeBuilder::new(config).build().await.unwrap()
}

async fn seed_legacy(runtime: &TeraRuntime) {
    let fixture = fixture();
    let raw = fixture["event"].as_str().unwrap();
    let event =
        radroots_event::SignedEvent::from_wire_verified_id(serde_json::from_str(raw).unwrap(), raw)
            .unwrap();
    let storage = runtime.client.storage().unwrap();
    EventStore::admit(storage, visible_admission(event, NOW * 1_000))
        .await
        .unwrap();
    let source = EventStore::status(storage).await.unwrap();
    ProjectionStore::put_projection_document(
        storage,
        projection_id().unwrap(),
        calendar_migration::legacy_generation().unwrap(),
        ProjectionDocument::new(
            projection_document_key(&context(None, 1)),
            fixture["projection"].as_str().unwrap().as_bytes().to_vec(),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    ProjectionStore::checkpoint(
        storage,
        ProjectionCheckpoint::new(
            projection_id().unwrap(),
            calendar_migration::legacy_generation().unwrap(),
            Some(EventPosition::new(
                source.generation(),
                EventSequence::new(1).unwrap(),
            )),
            1,
            NOW * 1_000,
        )
        .unwrap(),
    )
    .await
    .unwrap();
}

async fn old_bytes(runtime: &TeraRuntime) -> Vec<u8> {
    ProjectionStore::projection_document(
        runtime.client.storage().unwrap(),
        projection_id().unwrap(),
        calendar_migration::legacy_generation().unwrap(),
        projection_document_key(&context(None, 1)),
    )
    .await
    .unwrap()
    .unwrap()
    .value()
    .to_vec()
}

async fn raw(
    runtime: &TeraRuntime,
) -> radroots_storage::event::EventPage<radroots_storage::event::StoredRawEvent> {
    EventStore::query_raw(
        runtime.client.storage().unwrap(),
        EventQuery::all(EventQueryBounds::first(20).unwrap()),
    )
    .await
    .unwrap()
}

async fn assert_current(runtime: &TeraRuntime) {
    let state = load_state(
        runtime.client.storage().unwrap(),
        &context(None, 1),
        projection_generation().unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(state.schema_version, 2);
    assert_eq!(state.cards.len(), 1);
    let card = &state.cards[0].card;
    assert_eq!(card.schema_version, 2);
    assert_eq!(card.card_id.to_hex(), fixture()["cardId"]);
    assert_eq!(
        card.source_event_id,
        serde_json::from_str::<serde_json::Value>(fixture()["event"].as_str().unwrap()).unwrap()["id"]
    );
    let Some(CalendarTiming::DateBased(timing)) = &card.calendar_timing else {
        panic!("date timing");
    };
    assert_eq!(timing.start().as_str(), "2026-09-05");
    assert_eq!(timing.end_exclusive().unwrap().as_str(), "2026-09-07");
    assert_eq!(card.effective_at, 1_800_000_000);
    let encoded = String::from_utf8(encode(&state).unwrap()).unwrap();
    assert!(!encoded.contains("eventStart") && !encoded.contains("eventEnd"));
    let status =
        ProjectionStore::status(runtime.client.storage().unwrap(), projection_id().unwrap())
            .await
            .unwrap()
            .unwrap();
    assert_eq!(status.generation(), projection_generation().unwrap());
    assert_eq!(status.health(), ProjectionHealth::Ready);
}

#[cfg(feature = "mobile-social")]
#[tokio::test]
async fn old_sqlite_calendar_migrates_on_all_readers_without_changing_pending_identity() {
    use super::super::{
        CreateUpdate, Phase1AddCommand, Phase1CancellationPolicy, Phase1QueuePolicy,
        Phase1RelaySatisfaction,
    };
    for reader in ["page", "search", "me", "reconcile"] {
        let root = tempfile::tempdir().unwrap();
        let runtime = open(root.path()).await;
        seed_legacy(&runtime).await;
        let original = old_bytes(&runtime).await;
        let signed_before = raw(&runtime).await;
        let saved = runtime
            .phase1_save_draft(
                [17; 16],
                Phase1AddCommand::CreateUpdate(
                    CreateUpdate::new("Synthetic pending migration fixture").unwrap(),
                ),
                NOW,
                vec![],
                None,
                NOW * 1_000,
            )
            .await
            .unwrap();
        let queued = runtime
            .phase1_queue_draft(
                [17; 16],
                saved.draft().revision().get(),
                Phase1QueuePolicy::new(
                    vec!["wss://relay.example".into()],
                    Phase1RelaySatisfaction::AllAccepted,
                    (NOW + 1000) * 1000,
                    Phase1CancellationPolicy::LocalCooperative,
                )
                .unwrap(),
                NOW * 1000 + 1,
            )
            .await
            .unwrap();
        assert!(queued.draft().operation_id().is_some());
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let runtime = open(root.path()).await;
        let selected = context(None, 1);
        let count = match reader {
            "page" => runtime
                .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
                .await
                .unwrap()
                .items
                .len(),
            "search" => runtime
                .phase1_search(&selected, "Harvest", 20, NOW, "UTC")
                .await
                .unwrap()
                .len(),
            "me" => runtime
                .phase1_me(&selected, &keys().public_key().to_string(), NOW, "UTC")
                .await
                .unwrap()
                .cards
                .len(),
            "reconcile" => runtime
                .phase1_today_reconcile(
                    &selected,
                    NOW,
                    &[fixture()["cardId"].as_str().unwrap().into()],
                    None,
                    "UTC",
                )
                .await
                .unwrap()
                .items
                .len(),
            _ => unreachable!(),
        };
        assert_eq!(count, 1, "{reader}");
        assert_current(&runtime).await;
        assert_eq!(old_bytes(&runtime).await, original);
        assert_eq!(raw(&runtime).await, signed_before);
        assert_eq!(runtime.phase1_draft_status([17; 16]).await.unwrap(), queued);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = open(root.path()).await;
        assert_current(&reopened).await;
        assert_eq!(
            reopened.phase1_draft_status([17; 16]).await.unwrap(),
            queued
        );
        assert_eq!(old_bytes(&reopened).await, original);
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn every_durable_rebuild_boundary_resumes_after_sqlite_reopen() {
    for boundary in ["invalidated", "requested", "running", "staged"] {
        let root = tempfile::tempdir().unwrap();
        let runtime = open(root.path()).await;
        seed_legacy(&runtime).await;
        let storage = runtime.client.storage().unwrap();
        let source = calendar_migration::source_snapshot(storage).await.unwrap();
        let invalidation = ProjectionInvalidation::new(
            projection_id().unwrap(),
            calendar_migration::legacy_generation().unwrap(),
            projection_generation().unwrap(),
            InvalidationReason::ProjectionGenerationChanged,
            NOW * 1000,
        )
        .unwrap();
        ProjectionStore::invalidate(storage, invalidation.clone())
            .await
            .unwrap();
        let id = RebuildTicketId::new([7; 16]).unwrap();
        if boundary != "invalidated" {
            ProjectionStore::request_rebuild(
                storage,
                RebuildTicket::requested(
                    id,
                    invalidation,
                    source.generation,
                    source.high_water,
                    source.digest,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
        if matches!(boundary, "running" | "staged") {
            calendar_migration::begin(
                storage,
                NOW * 1000,
                &EventStore::status(storage).await.unwrap(),
            )
            .await
            .unwrap();
        }
        if boundary == "staged" {
            // An unpublished partial/corrupt replacement must never be read;
            // reconstruction overwrites it while retaining the old generation.
            ProjectionStore::put_projection_document(
                storage,
                projection_id().unwrap(),
                projection_generation().unwrap(),
                ProjectionDocument::new(
                    projection_document_key(&context(None, 1)),
                    b"unpublished interrupted candidate".to_vec(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
        assert!(
            load_state(storage, &context(None, 1), projection_generation().unwrap())
                .await
                .unwrap()
                .is_none()
        );
        let original = old_bytes(&runtime).await;
        let signed_before = raw(&runtime).await;
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let runtime = open(root.path()).await;
        runtime
            .phase1_today_page(&context(None, 1), TodayPageRequest::first(20, NOW, "UTC"))
            .await
            .unwrap();
        assert_current(&runtime).await;
        assert_eq!(old_bytes(&runtime).await, original, "{boundary}");
        assert_eq!(raw(&runtime).await, signed_before, "{boundary}");
        if boundary != "invalidated" {
            assert_eq!(
                ProjectionStore::rebuild(runtime.client.storage().unwrap(), id)
                    .await
                    .unwrap()
                    .unwrap()
                    .stage(),
                RebuildStage::Completed
            );
        }
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn changed_source_is_never_promoted_and_retries_from_current_history() {
    use radroots_transport::{
        Target, TransportId,
        source::{EventProvenance, ObservedEvent},
    };
    for same_count in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = open(root.path()).await;
        seed_legacy(&runtime).await;
        let storage = runtime.client.storage().unwrap();
        let extra = signed(1, vec![], "Synthetic source change", NOW - 1);
        if same_count {
            let target = Target::nostr_relay("wss://relay.example").unwrap();
            let observed = ObservedEvent::new(
                extra.clone(),
                EventProvenance::new(TransportId::NOSTR, target.fingerprint().clone(), NOW * 1000)
                    .unwrap(),
            );
            EventStore::admit(storage, EventAdmission::raw(observed))
                .await
                .unwrap();
        }
        let ticket = calendar_migration::begin(
            storage,
            NOW * 1000,
            &EventStore::status(storage).await.unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        EventStore::admit(storage, visible_admission(extra, NOW * 1000))
            .await
            .unwrap();
        let signed_before = raw(&runtime).await;
        let rejected = runtime
            .phase1_today_page(&context(None, 1), TodayPageRequest::first(20, NOW, "UTC"))
            .await;
        assert!(matches!(
            rejected,
            Err(TodayError::Storage(
                radroots_storage::Error::SourceGenerationChanged
            ))
        ));
        let failed = ProjectionStore::rebuild(storage, ticket.ticket_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed.stage(), RebuildStage::Failed);
        assert_eq!(
            ProjectionStore::status(storage, projection_id().unwrap())
                .await
                .unwrap()
                .unwrap()
                .generation(),
            calendar_migration::legacy_generation().unwrap()
        );
        let page = runtime
            .phase1_today_page(&context(None, 1), TodayPageRequest::first(20, NOW, "UTC"))
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(raw(&runtime).await, signed_before);
        assert_eq!(
            old_bytes(&runtime).await,
            fixture()["projection"].as_str().unwrap().as_bytes()
        );
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn obsolete_snapshots_and_unknown_generations_fail_without_rewriting_history() {
    let root = tempfile::tempdir().unwrap();
    let runtime = open(root.path()).await;
    seed_legacy(&runtime).await;
    let storage = runtime.client.storage().unwrap();
    let selected = context(None, 1);
    let old = calendar_migration::legacy_state(storage, &selected)
        .await
        .unwrap()
        .unwrap();
    let scope = CursorScope::new(
        selected.id.clone().into(),
        selected.generation,
        NOW,
        old.store_generation,
        old.content_generation,
        paging_scope::query_scope(&selected, runtime.store_public_key).unwrap(),
        crate::runtime::product_surface::ViewerCalendarContext::new(NOW, "UTC").expect("calendar"),
    )
    .unwrap();
    ProjectionStore::put_projection_snapshot(
        storage,
        ProjectionSnapshot::new(
            projection_id().unwrap(),
            snapshot_id(&scope),
            calendar_migration::legacy_generation().unwrap(),
            NOW * 1000,
            br#"{"schemaVersion":2}"#.to_vec(),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    assert!(matches!(
        load_snapshot(
            storage,
            projection_id().unwrap(),
            projection_generation().unwrap(),
            &scope
        )
        .await,
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    for version in [1, 2] {
        assert!(matches!(
            decode_snapshot(format!("{{\"schemaVersion\":{version}}}").as_bytes()),
            Err(TodayError::Cursor(CursorError::Stale))
        ));
    }
    assert_eq!(
        old_bytes(&runtime).await,
        fixture()["projection"].as_str().unwrap().as_bytes()
    );
    runtime.shutdown().await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let runtime = open(root.path()).await;
    let storage = runtime.client.storage().unwrap();
    let future = ProjectionCheckpoint::new(
        projection_id().unwrap(),
        ProjectionGeneration::new([9; 32]).unwrap(),
        None,
        0,
        NOW * 1000,
    )
    .unwrap();
    let original = ProjectionStore::checkpoint(storage, future).await.unwrap();
    assert!(matches!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
            .await,
        Err(TodayError::UnsupportedProjectionVersion)
    ));
    assert!(matches!(
        runtime
            .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Rebuild)
            .await,
        Err(TodayError::UnsupportedProjectionVersion)
    ));
    assert_eq!(
        ProjectionStore::status(storage, projection_id().unwrap())
            .await
            .unwrap()
            .unwrap(),
        original
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_historical_visible_heads_are_quarantined_without_reviving_old_cards() {
    use super::admission_tests::PermissiveEvidence;
    use radroots_event::admission::RawEvent;
    use radroots_transport::{
        Target, TransportId,
        source::{EventProvenance, ObservedEvent},
    };
    let root = tempfile::tempdir().unwrap();
    let runtime = open(root.path()).await;
    seed_legacy(&runtime).await;
    let storage = runtime.client.storage().unwrap();
    let invalid = signed(
        31_922,
        vec![
            vec!["d", "harvest-day"],
            vec!["title", "Invalid newer head"],
            vec!["start", "2026-09-05"],
        ],
        "",
        NOW - 1,
    );
    let mut wire: radroots_event::wire::Nip01EventWire =
        serde_json::from_str(invalid.raw_json()).unwrap();
    wire.sig = "00".repeat(64);
    let raw_json = serde_json::to_string(&wire).unwrap();
    let invalid = radroots_event::SignedEvent::from_wire_verified_id(wire, raw_json).unwrap();
    assert!(verify_nip01_event(invalid.envelope().clone()).is_err());
    let visible = RawEvent::new(invalid.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&PermissiveEvidence)
        .unwrap()
        .validate_contract_for_admission("radroots.calendar.date_event.v1")
        .unwrap()
        .admit_with(&PermissiveEvidence)
        .unwrap()
        .make_visible_with(&PermissiveEvidence)
        .unwrap();
    let target = Target::nostr_relay("wss://relay.example").unwrap();
    let observed = ObservedEvent::new(
        invalid.clone(),
        EventProvenance::new(TransportId::NOSTR, target.fingerprint().clone(), NOW * 1000).unwrap(),
    );
    EventStore::admit(storage, EventAdmission::visible(observed, visible).unwrap())
        .await
        .unwrap();
    let signed_before = raw(&runtime).await;
    let page = runtime
        .phase1_today_page(&context(None, 1), TodayPageRequest::first(20, NOW, "UTC"))
        .await
        .unwrap();
    assert!(page.items.is_empty());
    let current = load_state(storage, &context(None, 1), projection_generation().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.quarantined_source_ids, vec![invalid.id().to_hex()]);
    assert_eq!(raw(&runtime).await, signed_before);
    assert_eq!(
        old_bytes(&runtime).await,
        fixture()["projection"].as_str().unwrap().as_bytes()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupt_legacy_bytes_and_source_changes_before_promotion_preserve_original_state() {
    let root = tempfile::tempdir().unwrap();
    let runtime = open(root.path()).await;
    seed_legacy(&runtime).await;
    let storage = runtime.client.storage().unwrap();
    let mut damaged = old_bytes(&runtime).await;
    let offset = damaged
        .windows(b"Harvest day".len())
        .position(|part| part == b"Harvest day")
        .unwrap();
    damaged[offset] = b'X';
    ProjectionStore::put_projection_document(
        storage,
        projection_id().unwrap(),
        calendar_migration::legacy_generation().unwrap(),
        ProjectionDocument::new(projection_document_key(&context(None, 1)), damaged.clone())
            .unwrap(),
    )
    .await
    .unwrap();
    let original_status = ProjectionStore::status(storage, projection_id().unwrap())
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .phase1_today_page(&context(None, 1), TodayPageRequest::first(20, NOW, "UTC"))
            .await,
        Err(TodayError::CorruptProjection)
    ));
    assert_eq!(old_bytes(&runtime).await, damaged);
    assert_eq!(
        ProjectionStore::status(storage, projection_id().unwrap())
            .await
            .unwrap(),
        original_status
    );
    runtime.shutdown().await.unwrap();

    let root = tempfile::tempdir().unwrap();
    let runtime = open(root.path()).await;
    seed_legacy(&runtime).await;
    let storage = runtime.client.storage().unwrap();
    let source = EventStore::status(storage).await.unwrap();
    let ticket = calendar_migration::begin(storage, NOW * 1_000, &source)
        .await
        .unwrap()
        .unwrap();
    ProjectionStore::put_projection_document(
        storage,
        projection_id().unwrap(),
        projection_generation().unwrap(),
        ProjectionDocument::new(
            projection_document_key(&context(None, 1)),
            b"unpublished candidate".to_vec(),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    EventStore::admit(
        storage,
        visible_admission(
            signed(1, vec![], "Arrived after candidate write", NOW),
            NOW * 1000,
        ),
    )
    .await
    .unwrap();
    let checkpoint = ProjectionCheckpoint::new(
        projection_id().unwrap(),
        projection_generation().unwrap(),
        ticket.source_high_water(),
        source.raw_events(),
        NOW * 1000,
    )
    .unwrap();
    assert!(matches!(
        calendar_migration::complete(storage, &ticket, checkpoint).await,
        Err(TodayError::Storage(
            radroots_storage::Error::SourceGenerationChanged
        ))
    ));
    assert_eq!(
        ProjectionStore::rebuild(storage, ticket.ticket_id())
            .await
            .unwrap()
            .unwrap()
            .stage(),
        RebuildStage::Failed
    );
    assert!(!calendar_migration::ready(storage).await.unwrap());
    assert_eq!(
        old_bytes(&runtime).await,
        fixture()["projection"].as_str().unwrap().as_bytes()
    );
    let before = raw(&runtime).await;
    let page = runtime
        .phase1_today_page(&context(None, 1), TodayPageRequest::first(20, NOW, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(raw(&runtime).await, before);
    runtime.shutdown().await.unwrap();
}
