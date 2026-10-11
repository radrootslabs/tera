//! Current reads exercise real providers and committed source boundaries.
use super::performance_tests::{
    counted_storage::{CountedStorage, QueryCounts},
    reopen_counted_sqlite,
};
use super::tests::{context, ingest, keys, signed, visible_admission};
use super::*;
use radroots_event::{SignedEvent, admission::RawEvent};
use radroots_event_codec::verify::Nip01SignatureVerifier;
use radroots_transport::{
    Target, TransportId,
    source::{EventProvenance, ObservedEvent},
};
use std::{sync::Arc, time::Duration};

const NOW: u64 = 2_000_000_100;

#[path = "today_read_scope_reopen_tests.rs"]
mod reopen_cases;

#[path = "today_read_scope_conflict_tests.rs"]
mod conflict_cases;

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

fn verified(event: SignedEvent) -> EventAdmission {
    let evidence = RawEvent::new(event.envelope().clone())
        .verify_id()
        .unwrap()
        .verify_signature(&Nip01SignatureVerifier)
        .unwrap();
    EventAdmission::verified(observed(event), evidence).unwrap()
}

async fn read_ids(
    runtime: &TeraRuntime,
    selected: &LocalNetwork,
    operation: usize,
    ids: &[String],
) -> Result<Vec<String>, TodayError> {
    let cards = match operation {
        0 => {
            runtime
                .phase1_today_page(selected, TodayPageRequest::first(100, NOW + 10, "UTC"))
                .await?
                .items
        }
        1 => runtime
            .phase1_search(selected, "scope", 100, NOW + 10, "UTC")
            .await?
            .into_iter()
            .filter_map(|result| result.card)
            .collect(),
        2 => {
            runtime
                .phase1_me(selected, &keys().public_key().to_string(), NOW + 10, "UTC")
                .await?
                .cards
        }
        _ => {
            runtime
                .phase1_today_reconcile(selected, NOW + 10, ids, None, "UTC")
                .await?
                .items
        }
    };
    Ok(cards
        .into_iter()
        .map(|card| card.card.source_event_id)
        .collect())
}

#[tokio::test]
async fn complete_same_id_generation_locality_repairs_search_me_and_paging() {
    for sqlite in [false, true] {
        for operation in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let runtime = if sqlite {
                reopen_counted_sqlite(&root).await
            } else {
                TeraRuntime::test_memory().unwrap()
            };
            let first = context(Some("first"), 1);
            let second = context(Some("second"), 1);
            let mut expected = Vec::new();
            for locality in ["first", "second"] {
                for suffix in ["alpha", "bravo"] {
                    let event = signed(
                        1,
                        vec![vec!["g", locality]],
                        &format!("scope {suffix}"),
                        NOW,
                    );
                    if locality == "second" {
                        expected.push(event.id().to_hex());
                    }
                    ingest(&runtime, &first, event, NOW).await;
                }
            }
            assert_eq!(
                read_ids(&runtime, &first, operation, &[])
                    .await
                    .unwrap()
                    .len(),
                2
            );
            let mut actual = read_ids(&runtime, &second, operation, &[]).await.unwrap();
            actual.sort();
            expected.sort();
            assert_eq!(
                actual, expected,
                "{sqlite}/{operation}: include previously excluded current records"
            );
            runtime.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn sqlite_commit_without_projection_reopen_qualifies_every_supporting_reader() {
    for scenario in ["deletion", "same_count_promotion", "unsupported_profile"] {
        for operation in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let runtime = reopen_counted_sqlite(&root).await;
            let selected = context(None, 1);
            let original = signed(1, vec![], "scope original", NOW - 100);
            let source_ids = vec![original.id().to_hex()];
            ingest(&runtime, &selected, original.clone(), NOW).await;
            ingest(
                &runtime,
                &selected,
                signed(0, vec![], r#"{"name":"Scope Farm"}"#, NOW - 100),
                NOW,
            )
            .await;
            let ids = runtime
                .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|card| card.card.card_id.to_hex())
                .collect::<Vec<_>>();
            let storage = runtime.client.storage().unwrap();
            let admission = match scenario {
                "deletion" => visible_admission(
                    signed(5, vec![vec!["e", &source_ids[0]]], "", NOW - 99),
                    NOW * 1_000,
                ),
                "same_count_promotion" => {
                    let event = signed(1, vec![], "scope promoted", NOW - 99);
                    EventStore::admit(storage, EventAdmission::raw(observed(event.clone())))
                        .await
                        .unwrap();
                    runtime
                        .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Incremental)
                        .await
                        .unwrap();
                    visible_admission(event, NOW * 1_000)
                }
                _ => verified(signed(0, vec![], "unsupported valid profile", NOW - 99)),
            };
            let count = EventStore::status(storage).await.unwrap().raw_events();
            let receipt = EventStore::admit(storage, admission).await.unwrap();
            if scenario == "same_count_promotion" {
                assert_eq!(
                    EventStore::status(storage).await.unwrap().raw_events(),
                    count
                );
            }
            // No projection/refresh between the acknowledged source commit and close.
            runtime.shutdown().await.unwrap();
            let runtime = reopen_counted_sqlite(&root).await;
            let retained = EventStore::query_raw(
                runtime.client.storage().unwrap(),
                EventQuery::for_ids(
                    EventQueryBounds::first(1).unwrap(),
                    vec![*receipt.event_id()],
                )
                .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(retained.items()[0].position(), receipt.position());
            let actual = read_ids(&runtime, &selected, operation, &ids)
                .await
                .unwrap();
            match scenario {
                "deletion" => assert!(
                    actual.is_empty(),
                    "{operation} must not return deleted content"
                ),
                "same_count_promotion" if operation != 3 => assert_eq!(actual.len(), 2),
                _ => assert_eq!(actual, source_ids, "{scenario}/{operation}"),
            }
            let me = runtime
                .phase1_me(&selected, &keys().public_key().to_string(), NOW + 11, "UTC")
                .await
                .unwrap();
            assert_eq!(me.profile.is_none(), scenario == "unsupported_profile");
            runtime.shutdown().await.unwrap();
        }
    }
}

async fn instrumented(root: &tempfile::TempDir) -> (TeraRuntime, Arc<QueryCounts>) {
    let mut runtime = reopen_counted_sqlite(root).await;
    let counts = Arc::new(QueryCounts::default());
    runtime.client = radroots_sdk::ClientBuilder::new()
        .storage(Arc::new(CountedStorage {
            client: runtime.client.clone(),
            counts: counts.clone(),
        }))
        .build()
        .unwrap();
    (runtime, counts)
}

#[tokio::test]
async fn actual_sqlite_reads_before_and_after_await_refuse_concurrent_source_change() {
    for operation in 0..4 {
        for ordinal in 1..=3 {
            for after in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (runtime, counts) = instrumented(&root).await;
                let selected = context(None, 1);
                let old = signed(1, vec![], "scope old", NOW);
                ingest(&runtime, &selected, old, NOW).await;
                let ids = runtime
                    .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
                    .await
                    .unwrap()
                    .items
                    .into_iter()
                    .map(|card| card.card.card_id.to_hex())
                    .collect::<Vec<_>>();
                counts.take();
                let pause = counts.faults.pause("read_query", ordinal, after);
                let runtime = Arc::new(runtime);
                let reader = {
                    let runtime = runtime.clone();
                    let selected = selected.clone();
                    let ids = ids.clone();
                    tokio::spawn(
                        async move { read_ids(&runtime, &selected, operation, &ids).await },
                    )
                };
                tokio::time::timeout(Duration::from_secs(2), pause.entered.notified())
                    .await
                    .unwrap();
                EventStore::admit(
                    runtime.client.storage().unwrap(),
                    visible_admission(signed(1, vec![], "scope new-current", NOW + 1), NOW * 1_000),
                )
                .await
                .unwrap();
                pause.resume.notify_one();
                assert!(
                    reader.await.unwrap().is_err(),
                    "{operation}/{ordinal}/{after} stale read must refuse"
                );
                let reads = counts.take();
                assert_eq!(reads.get("event.rows").copied().unwrap_or(0), 0);
                assert_eq!(
                    runtime
                        .phase1_search(&selected, "new-current", 10, NOW + 10, "UTC")
                        .await
                        .unwrap()
                        .len(),
                    1,
                    "A read refusal must not fence acknowledged canonical work"
                );
                runtime.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn sqlite_same_source_policy_change_fences_supporting_payload_read() {
    use crate::runtime::visibility::{AuthorVisibility, AuthorVisibilityPolicy};
    for operation in 1..=3 {
        let root = tempfile::tempdir().unwrap();
        let (runtime, counts) = instrumented(&root).await;
        let selected = context(None, 1);
        let old = signed(1, vec![], "scope old", NOW);
        ingest(&runtime, &selected, old, NOW).await;
        let ids = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|card| card.card.card_id.to_hex())
            .collect::<Vec<_>>();
        let pause = counts.faults.pause("read_query", 2, true);
        let runtime = Arc::new(runtime);
        let reader = {
            let runtime = runtime.clone();
            let selected = selected.clone();
            let ids = ids.clone();
            tokio::spawn(async move { read_ids(&runtime, &selected, operation, &ids).await })
        };
        tokio::time::timeout(Duration::from_secs(2), pause.entered.notified())
            .await
            .unwrap();
        let source = EventStore::source_token(runtime.client.storage().unwrap())
            .await
            .unwrap();
        let mut policy = serde_json::to_value(AuthorVisibilityPolicy::default()).unwrap();
        policy["revision"] = 1.into();
        policy["entries"][keys().public_key().to_string()] =
            serde_json::to_value(AuthorVisibility::Blocked).unwrap();
        let policy: AuthorVisibilityPolicy = serde_json::from_value(policy).unwrap();
        let (id, generation) = crate::runtime::visibility::identity().unwrap();
        ProjectionStore::put_projection_document(
            runtime.client.storage().unwrap(),
            id,
            generation,
            ProjectionDocument::new(
                crate::runtime::visibility::KEY.into(),
                serde_json::to_vec(&policy).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            EventStore::source_token(runtime.client.storage().unwrap())
                .await
                .unwrap(),
            source
        );
        pause.resume.notify_one();
        assert!(reader.await.unwrap().is_err());
        assert!(
            read_ids(&runtime, &selected, operation, &ids)
                .await
                .unwrap()
                .is_empty()
        );
        runtime.shutdown().await.unwrap();
    }
}
