//! Durable interruption fixtures. Product corrections have separate regressions.
use super::performance_tests::reopen_counted_sqlite;
use super::tests::{context, ingest, keys, signed, visible_admission};
use super::*;
use radroots_event::{SignedEvent, admission::RawEvent};
use radroots_event_codec::verify::Nip01SignatureVerifier;
use radroots_transport::{
    Target, TransportId,
    source::{EventProvenance, ObservedEvent},
};

const NOW: u64 = 2_000_000_100;

fn observed(event: SignedEvent) -> ObservedEvent {
    let target = Target::nostr_relay("wss://relay.example").unwrap();
    ObservedEvent::new(
        event,
        EventProvenance::new(
            TransportId::NOSTR,
            target.fingerprint().clone(),
            NOW * 1_000,
        )
        .unwrap(),
    )
}

fn verified_admission(event: SignedEvent) -> EventAdmission {
    let verified = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Nip01SignatureVerifier)
        .unwrap();
    EventAdmission::verified(observed(event), verified).unwrap()
}

async fn retained_receipt(runtime: &TeraRuntime, receipt: &AdmissionReceipt) {
    let storage = runtime.client.storage().unwrap();
    let query = EventQuery::for_ids(
        EventQueryBounds::first(1).unwrap(),
        vec![*receipt.event_id()],
    )
    .unwrap();
    let retained = EventStore::query_raw(storage, query).await.unwrap();
    assert_eq!(retained.items().len(), 1);
    assert_eq!(retained.items()[0].position(), receipt.position());
    assert_eq!(retained.items()[0].stage(), receipt.stage());
}

async fn page_ids(runtime: &TeraRuntime, selected: &LocalNetwork, at: u64) -> Vec<String> {
    runtime
        .phase1_today_page(selected, TodayPageRequest::first(10, at, "UTC"))
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.card.source_event_id)
        .collect()
}

async fn observe_read_boundary(
    label: &str,
    root: &tempfile::TempDir,
    receipt: &AdmissionReceipt,
    expected_ids: Vec<String>,
) {
    let reopened = reopen_counted_sqlite(root).await;
    retained_receipt(&reopened, receipt).await;
    let selected = context(None, 1);
    let before = page_ids(&reopened, &selected, NOW + 10).await;
    let visibility = EventStore::rebuild_visibility(reopened.client.storage().unwrap())
        .await
        .unwrap();
    reopened
        .phase1_refresh_today(&selected, NOW + 11, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    let after = page_ids(&reopened, &selected, NOW + 12).await;
    assert_eq!(after, expected_ids, "fixture explicit refresh control");
    println!(
        "{}",
        serde_json::json!({
            "scenario": label,
            "boundary": "durable event admission, close/reopen, before projection refresh",
            "durable_receipt_recovered": true,
            "canonical_visible_ids": visibility.visible_event_ids().iter().map(|id| id.to_hex()).collect::<Vec<_>>(),
            "expected_current_page_ids": expected_ids,
            "observed_before_refresh_ids": before,
            "observed_after_refresh_ids": after,
            "current_read_matches_canonical": before == expected_ids,
            "product_correction_qualified": false
        })
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn deletion_admission_survives_reopen_before_projection_refresh() {
    let root = tempfile::tempdir().unwrap();
    let runtime = reopen_counted_sqlite(&root).await;
    let selected = context(None, 1);
    let original = signed(1, vec![], "retained note", NOW - 100);
    ingest(&runtime, &selected, original.clone(), NOW).await;
    assert_eq!(page_ids(&runtime, &selected, NOW + 1).await.len(), 1);
    let deletion = signed(5, vec![vec!["e", &original.id().to_hex()]], "", NOW - 99);
    let storage = runtime.client.storage().unwrap();
    let absent = EventStore::query_raw(
        storage,
        EventQuery::for_ids(EventQueryBounds::first(1).unwrap(), vec![*deletion.id()]).unwrap(),
    )
    .await
    .unwrap();
    assert!(absent.items().is_empty(), "before admission control");
    let receipt = EventStore::admit(storage, visible_admission(deletion, NOW * 1_000))
        .await
        .unwrap();
    retained_receipt(&runtime, &receipt).await;
    runtime.shutdown().await.unwrap();
    observe_read_boundary("deletion_after_admission", &root, &receipt, vec![]).await;
}

#[tokio::test]
async fn verified_unsupported_winner_survives_reopen_before_projection_refresh() {
    let root = tempfile::tempdir().unwrap();
    let runtime = reopen_counted_sqlite(&root).await;
    let selected = context(None, 1);
    let original = signed(
        0,
        vec![],
        r#"{"display_name":"Original Farm","bot":false}"#,
        NOW - 100,
    );
    let newer = signed(0, vec![], "malformed profile", NOW - 99);
    assert!(admit_verified_event(verify_nip01_event(newer.envelope().clone()).unwrap()).is_err());
    ingest(&runtime, &selected, original.clone(), NOW).await;
    let storage = runtime.client.storage().unwrap();
    let generation = projection_generation().unwrap();
    let prior = load_state(storage, &selected, generation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(prior.profiles.len(), 1);
    let receipt = EventStore::admit(storage, verified_admission(newer.clone()))
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    let reopened = reopen_counted_sqlite(&root).await;
    retained_receipt(&reopened, &receipt).await;
    let storage = reopened.client.storage().unwrap();
    let canonical = EventStore::rebuild_visibility(storage).await.unwrap();
    assert_eq!(canonical.current_heads()[0].event_id, *newer.id());
    let before = load_state(storage, &selected, generation)
        .await
        .unwrap()
        .unwrap();
    let presented = reopened
        .phase1_me(&selected, &keys().public_key().to_string(), NOW + 10, "UTC")
        .await
        .unwrap();
    reopened
        .phase1_refresh_today(&selected, NOW + 11, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    let after = load_state(storage, &selected, generation)
        .await
        .unwrap()
        .unwrap();
    assert!(
        after.profiles.is_empty(),
        "fixture explicit refresh control"
    );
    println!(
        "{}",
        serde_json::json!({
            "scenario": "verified_unsupported_winner_after_admission",
            "durable_receipt_recovered": true,
            "canonical_selected_unsupported_winner": true,
            "profiles_before_read": before.profiles.len(),
            "profile_presented_before_refresh": presented.profile.is_some(),
            "profiles_after_explicit_refresh": after.profiles.len(),
            "product_correction_qualified": false
        })
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_count_promotion_survives_reopen_before_projection_refresh() {
    let root = tempfile::tempdir().unwrap();
    let runtime = reopen_counted_sqlite(&root).await;
    let selected = context(None, 1);
    let event = signed(1, vec![], "promoted note", NOW - 100);
    let storage = runtime.client.storage().unwrap();
    EventStore::admit(storage, EventAdmission::raw(observed(event.clone())))
        .await
        .unwrap();
    runtime
        .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert!(page_ids(&runtime, &selected, NOW + 1).await.is_empty());
    let before_count = EventStore::status(storage).await.unwrap().raw_events();
    let receipt = EventStore::admit(storage, visible_admission(event.clone(), NOW * 1_000))
        .await
        .unwrap();
    assert_eq!(
        EventStore::status(storage).await.unwrap().raw_events(),
        before_count
    );
    runtime.shutdown().await.unwrap();
    observe_read_boundary(
        "same_count_promotion_after_admission",
        &root,
        &receipt,
        vec![event.id().to_hex()],
    )
    .await;
}

#[tokio::test]
async fn advancing_as_of_and_duplicate_control_use_real_retained_snapshots() {
    for count in [1, 4, 12] {
        let root = match std::env::var_os("TERA_DIAGNOSTIC_STORE_ROOT") {
            Some(directory) => tempfile::Builder::new()
                .prefix("snapshot-control-")
                .tempdir_in(directory)
                .unwrap(),
            None => tempfile::tempdir().unwrap(),
        };
        let runtime = reopen_counted_sqlite(&root).await;
        let selected = context(None, 1);
        ingest(
            &runtime,
            &selected,
            signed(1, vec![], "snapshot note", NOW - 100),
            NOW,
        )
        .await;
        for offset in 0..count {
            let at = NOW + 10 + offset;
            let first = page_ids(&runtime, &selected, at).await;
            let duplicate = page_ids(&runtime, &selected, at).await;
            assert_eq!(first, duplicate);
            assert_eq!(first.len(), 1);
        }
        runtime.shutdown().await.unwrap();
        if std::env::var_os("TERA_DIAGNOSTIC_STORE_ROOT").is_some() {
            let retained = root.keep();
            println!(
                "{}",
                serde_json::json!({
                    "scenario": "advancing_as_of_snapshot_retention",
                    "distinct_as_of_requests": count,
                    "duplicate_requests": count,
                    "fixture_store_root": retained,
                    "runtime_shutdown": true,
                    "product_correction_qualified": false
                })
            );
        }
    }
}
