use super::tests::{context, keys, signed, visible_admission};
use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use std::{sync::Arc, time::Instant};
#[path = "today_counted_storage_tests.rs"]
mod counted_storage;
use counted_storage::{CountedStorage, QueryCounts};
const EVENT_COUNT: u64 = 10_000;
const NOW: u64 = 2_000_000_000;

#[tokio::test]
async fn cached_sqlite_pages_do_not_decode_ten_thousand_source_events() {
    let (_root, runtime, counts) = counted_sqlite().await;
    let selected = context(None, 1);
    for index in 0..EVENT_COUNT {
        let event = signed(1, vec![], "Synthetic bounded query fixture", NOW - index);
        EventStore::admit(
            runtime.client.storage().unwrap(),
            visible_admission(event, NOW * 1_000),
        )
        .await
        .unwrap();
    }
    counts.take();
    let refresh = Instant::now();
    runtime
        .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Rebuild)
        .await
        .unwrap();
    let rebuild_ms = refresh.elapsed().as_secs_f64() * 1_000.0;
    let rebuild_calls = counts.take();
    let samples = async {
        let mut samples = Vec::new();
        for _ in 0..if cfg!(debug_assertions) { 5 } else { 20 } {
            counts.take();
            let start = Instant::now();
            let first = runtime
                .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
                .await
                .unwrap();
            assert_eq!(first.items.len(), 20);
            let calls = counts.take();
            samples.push(("first", start.elapsed().as_secs_f64() * 1_000.0, calls));
            counts.take();
            let start = Instant::now();
            let next = runtime
                .phase1_today_page(
                    &selected,
                    TodayPageRequest::after(20, first.next_cursor.unwrap()),
                )
                .await
                .unwrap();
            assert_eq!(next.items.len(), 20);
            assert_ne!(first.items[0].card.card_id, next.items[0].card.card_id);
            let calls = counts.take();
            samples.push(("after", start.elapsed().as_secs_f64() * 1_000.0, calls));
        }
        samples
    }
    .await;
    println!(
        "{}",
        serde_json::json!({
            "scenario": "tera_10000_sqlite_cached_pages",
            "events": EVENT_COUNT, "page_size": 20, "rebuild_ms": rebuild_ms,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "samples": samples, "rebuild_calls": rebuild_calls,
            "count_boundary": "actual storage SPI calls and returned event rows; SQL logging remains disabled",
            "physical_device_qualification": false
        })
    );
    runtime.shutdown().await.unwrap();
    assert!(samples.iter().all(|(_, _, calls)| {
        calls
            .get("projection.projection_document")
            .copied()
            .unwrap_or(0)
            > 0
    }));
    assert!(
        samples.iter().all(
            |(_, _, calls)| calls.get("event.status").copied().unwrap_or(0) == 0
                && calls.get("event.query_visible").copied().unwrap_or(0) == 0
                && calls.get("event.rows").copied().unwrap_or(0) <= 1
        ),
        "Cached pages must not read full event history for generation validation"
    );
}

async fn counted_sqlite() -> (tempfile::TempDir, TeraRuntime, Arc<QueryCounts>) {
    let root = tempfile::tempdir().unwrap();
    let mut runtime = reopen_counted_sqlite(&root).await;
    let counts = Arc::new(QueryCounts::default());
    runtime.client = radroots_sdk::ClientBuilder::new()
        .storage(Arc::new(CountedStorage {
            client: runtime.client.clone(),
            counts: counts.clone(),
        }))
        .build()
        .unwrap();
    (root, runtime, counts)
}

pub(super) async fn reopen_counted_sqlite(root: &tempfile::TempDir) -> TeraRuntime {
    let store = MobileUserStoreConfig::from_encoded(
        root.path(),
        &keys().public_key().to_string(),
        &"03".repeat(32),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(store.owner_directory()).unwrap();
    RuntimeBuilder::new(store).build().await.unwrap()
}

#[tokio::test]
async fn invalid_page_boundaries_reject_before_any_storage_query() {
    let (_root, runtime, counts) = counted_sqlite().await;
    let selected = context(None, 1);
    let mut wrong = selected.clone();
    wrong.generation += 1;
    let scope = CursorScope::new(
        wrong.id.clone().into(),
        wrong.generation,
        NOW,
        [3; 32],
        1,
        paging_scope::query_scope(&wrong, runtime.store_public_key).unwrap(),
        crate::runtime::product_surface::ViewerCalendarContext::new(NOW, "UTC").expect("calendar"),
    )
    .unwrap();
    // Malformed and oversized opaque inputs, invalid limit/as-of, and a valid
    // cursor for a different authenticated context all precede storage work.
    let position = TodayCursorPosition {
        rank: TodayRank::derive(TodayRankInput {
            card_id: CardId::parse(&"ab".repeat(32)).unwrap(),
            context_rank: super::super::ContextRank::MissingLocalityFallback,
            card_type: TodayCardType::Update,
            time: TimeRelevance::Published,
            effective_at: NOW,
            as_of: NOW,
        })
        .unwrap(),
    };
    let wrong_cursor = TodayCursor::encode(&scope, position)
        .unwrap()
        .as_str()
        .to_owned();
    for request in [
        TodayPageRequest::after(20, "bad".into()),
        TodayPageRequest::after(20, "x".repeat(100_000)),
        TodayPageRequest::first(0, NOW, "UTC"),
        TodayPageRequest::first(20, 0, "UTC"),
        TodayPageRequest::first(20, u64::MAX, "UTC"),
        TodayPageRequest::first(20, NOW, "Invalid/Zone"),
        TodayPageRequest::first(20, NOW, &"x".repeat(256)),
        TodayPageRequest::after(20, format!("rrtc3:{}", "0".repeat(1384 - 6))),
        TodayPageRequest::after(20, format!("rrtc3:{}", "0".repeat(1385 - 6))),
        TodayPageRequest::after(20, format!("rrtc2:{}", "0".repeat(858 - 6))),
        TodayPageRequest::after(20, wrong_cursor),
    ] {
        counts.take();
        assert!(runtime.phase1_today_page(&selected, request).await.is_err());
        assert!(
            counts.take().is_empty(),
            "Invalid scope must not touch the store"
        );
    }
    for (as_of, zone) in [(u64::MAX, "UTC"), (NOW, "Invalid/Zone")] {
        counts.take();
        assert!(
            runtime
                .phase1_today_reconcile(&selected, as_of, &[], None, zone)
                .await
                .is_err()
        );
        assert!(
            runtime
                .phase1_search(&selected, "Harvest", 20, as_of, zone)
                .await
                .is_err()
        );
        assert!(
            runtime
                .phase1_me(&selected, &keys().public_key().to_string(), as_of, zone)
                .await
                .is_err()
        );
        assert!(
            counts.take().is_empty(),
            "Invalid calendar context must precede every ranked storage reader"
        );
    }
    runtime.shutdown().await.unwrap();
}
