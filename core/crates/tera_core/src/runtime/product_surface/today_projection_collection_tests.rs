use super::super::{
    performance_tests::{
        counted_storage::{CountedStorage, QueryCounts},
        reopen_counted_sqlite,
    },
    tests::{context, signed, visible_admission},
};
use super::*;
use radroots_storage::projection::batch::{ProjectionDocumentBatch, ProjectionWriteScope};
use std::{sync::Arc, time::Duration};
const NOW: u64 = 2_000_000_100;

async fn open(root: &tempfile::TempDir) -> (TeraRuntime, Arc<QueryCounts>) {
    let mut runtime = reopen_counted_sqlite(root).await;
    let counts = instrument(&mut runtime);
    (runtime, counts)
}
fn instrument(runtime: &mut TeraRuntime) -> Arc<QueryCounts> {
    let counts = Arc::new(QueryCounts::default());
    runtime.client = radroots_sdk::ClientBuilder::new()
        .storage(Arc::new(CountedStorage {
            client: runtime.client.clone(),
            counts: counts.clone(),
        }))
        .build()
        .unwrap();
    counts
}
async fn refresh(runtime: &TeraRuntime) {
    runtime
        .phase1_refresh_today(&context(None, 1), NOW, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
}
async fn source(runtime: &TeraRuntime) -> EventSourceToken {
    EventStore::source_token(runtime.client.storage().unwrap())
        .await
        .unwrap()
}
async fn active(runtime: &TeraRuntime) -> ProjectionGeneration {
    ProjectionStore::status(
        runtime.client.storage().unwrap(),
        ProjectionId::parse(build::progress::ID).unwrap(),
    )
    .await
    .unwrap()
    .unwrap()
    .generation()
}
async fn usage(runtime: &TeraRuntime) -> radroots_storage::projection::cache::ProjectionCacheUsage {
    ProjectionStore::configure_projection_cache(
        runtime.client.storage().unwrap(),
        ProjectionId::parse(build::progress::ID).unwrap(),
        limits().unwrap(),
    )
    .await
    .unwrap()
}
async fn pass(runtime: &TeraRuntime, deadline: Instant) -> Option<ProjectionCacheCollection> {
    let _guard = runtime.today_projection_lock.lock().await;
    run(
        runtime,
        runtime.client.storage().unwrap(),
        source(runtime).await,
        Some(active(runtime).await),
        deadline,
    )
    .await
    .unwrap()
}
async fn seed_rows(runtime: &TeraRuntime, generation: ProjectionGeneration, retired: bool) {
    let storage = runtime.client.storage().unwrap();
    let id = ProjectionId::parse(build::progress::ID).unwrap();
    let source = source(runtime).await;
    let active = Some(active(runtime).await);
    let scope = ProjectionWriteScope::new(id.clone(), generation, source, active).unwrap();
    for range in [0..64, 64..65] {
        ProjectionStore::put_projection_batch(
            storage,
            ProjectionDocumentBatch::new(
                scope.clone(),
                range
                    .map(|index| {
                        ProjectionDocument::new(
                            format!("synthetic.{index:03}"),
                            br#"{"derived":"obsolete"}"#.to_vec(),
                        )
                        .unwrap()
                    })
                    .collect(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }
    if retired {
        ProjectionStore::retire_projection_generation(
            storage,
            ProjectionCacheScope::new(id, generation, source, active).unwrap(),
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn retired_rows_resume_after_reopen_and_keep_live_cursor_and_staging() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, counts) = open(&root).await;
    for index in 0..3 {
        EventStore::admit(
            runtime.client.storage().unwrap(),
            visible_admission(
                signed(1, vec![], &format!("Synthetic card{index}"), NOW - 1),
                NOW * 1000,
            ),
        )
        .await
        .unwrap();
    }
    refresh(&runtime).await;
    let page = runtime
        .phase1_today_page(&context(None, 1), TodayPageRequest::first(1, NOW, "UTC"))
        .await
        .unwrap();
    let request = TodayPageRequest::after(1, page.next_cursor.unwrap());
    let expected = runtime
        .phase1_today_page(&context(None, 1), request.clone())
        .await
        .unwrap();
    let first = ProjectionGeneration::new([11; 32]).unwrap();
    let second = ProjectionGeneration::new([12; 32]).unwrap();
    let staging = ProjectionGeneration::new([13; 32]).unwrap();
    seed_rows(&runtime, first, true).await;
    seed_rows(&runtime, second, true).await;
    seed_rows(&runtime, staging, false).await;
    let source_before = source(&runtime).await;
    let active_before = active(&runtime).await;
    counts.take();
    refresh(&runtime).await;
    assert_eq!(counts.take().get("projection.collected_rows"), Some(&64));
    assert_eq!(
        runtime
            .phase1_today_page(&context(None, 1), request.clone())
            .await
            .unwrap(),
        expected
    );
    runtime.shutdown().await.unwrap();
    let (runtime, counts) = open(&root).await;
    for removed in [2, 64, 2, 0] {
        counts.take();
        refresh(&runtime).await;
        assert_eq!(
            counts.take().get("projection.collected_rows"),
            Some(&removed)
        );
    }
    assert_eq!(source(&runtime).await, source_before);
    assert_eq!(active(&runtime).await, active_before);
    assert_eq!(
        runtime
            .phase1_today_page(&context(None, 1), request)
            .await
            .unwrap(),
        expected
    );
    let storage = runtime.client.storage().unwrap();
    let id = ProjectionId::parse(build::progress::ID).unwrap();
    for generation in [first, second] {
        assert!(
            ProjectionStore::projection_document(
                storage,
                id.clone(),
                generation,
                "synthetic.000".into()
            )
            .await
            .unwrap()
            .is_none()
        );
    }
    assert!(
        ProjectionStore::projection_document(storage, id, staging, "synthetic.064".into())
            .await
            .unwrap()
            .is_some()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn acknowledged_legacy_snapshots_retire_in_bounded_passes_preserving_custody() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, counts) = open(&root).await;
    calendar_migration_tests::seed_legacy(&runtime).await;
    let storage = runtime.client.storage().unwrap();
    let selected = context(None, 1);
    let generation = calendar_migration::legacy_generation().unwrap();
    let custody = ProjectionStore::projection_document(
        storage,
        projection_id().unwrap(),
        generation,
        projection_document_key(&selected),
    )
    .await
    .unwrap()
    .unwrap();
    let canonical = EventStore::query_raw(
        storage,
        EventQuery::all(EventQueryBounds::first(20).unwrap()),
    )
    .await
    .unwrap();
    let old = calendar_migration::legacy_state(storage, &selected)
        .await
        .unwrap()
        .unwrap();
    for index in 0..65u64 {
        let snapshot = FrozenTodaySnapshot {
            schema_version: TODAY_SNAPSHOT_SCHEMA_VERSION,
            context_id: selected.id.clone().into(),
            context_generation: selected.generation,
            as_of: NOW + index,
            calendar: super::super::super::ViewerCalendarContext::new(NOW + index, "UTC").unwrap(),
            store_generation: old.store_generation,
            projection_generation: old.content_generation,
            query_scope: paging_scope::query_scope(&selected, runtime.store_public_key).unwrap(),
            items: Vec::new(),
        };
        ProjectionStore::put_projection_snapshot(
            storage,
            ProjectionSnapshot::new(
                projection_id().unwrap(),
                Sha256::digest(index.to_be_bytes()).into(),
                generation,
                (NOW + index) * 1000,
                encode(&snapshot).unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }
    refresh(&runtime).await; // Acknowledges migration before any legacy snapshot effects.
    counts.take();
    refresh(&runtime).await;
    assert_eq!(counts.take().get("projection.collected_rows"), Some(&64));
    runtime.shutdown().await.unwrap();
    let (runtime, counts) = open(&root).await;
    refresh(&runtime).await;
    assert_eq!(counts.take().get("projection.collected_rows"), Some(&1));
    let storage = runtime.client.storage().unwrap();
    assert_eq!(
        ProjectionStore::projection_document(
            storage,
            projection_id().unwrap(),
            generation,
            projection_document_key(&selected)
        )
        .await
        .unwrap(),
        Some(custody)
    );
    assert_eq!(
        EventStore::query_raw(
            storage,
            EventQuery::all(EventQueryBounds::first(20).unwrap())
        )
        .await
        .unwrap(),
        canonical
    );
    let after = ProjectionStore::configure_projection_cache(
        storage,
        projection_id().unwrap(),
        limits().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(after.legacy_snapshots, 0);
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn elapsed_quantum_and_uncertain_write_dispatch_no_collection() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, counts) = open(&root).await;
    refresh(&runtime).await;
    let retired = ProjectionGeneration::new([11; 32]).unwrap();
    seed_rows(&runtime, retired, true).await;
    let before = usage(&runtime).await;
    counts.take();
    assert!(
        pass(&runtime, Instant::now() - Duration::from_secs(1))
            .await
            .is_none()
    );
    assert!(!counts.take().contains_key("projection.collect_retired"));
    drop(runtime.today_projection_lock.begin_write());
    assert!(
        pass(&runtime, Instant::now() + Duration::from_secs(5))
            .await
            .is_none()
    );
    assert!(!counts.take().contains_key("projection.collect_retired"));
    assert_eq!(usage(&runtime).await, before);
    runtime.shutdown().await.unwrap();
    let (runtime, _) = open(&root).await;
    assert_eq!(
        pass(&runtime, Instant::now() + Duration::from_secs(5))
            .await
            .unwrap()
            .removed_rows,
        64
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn collection_failure_preserves_source_and_recovers_only_after_reopen() {
    for (after, capacity) in [(false, false), (true, false), (false, true)] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, counts) = open(&root).await;
        let receipt = EventStore::admit(
            runtime.client.storage().unwrap(),
            visible_admission(
                signed(1, vec![], "Synthetic retained canonical record", NOW - 1),
                NOW * 1000,
            ),
        )
        .await
        .unwrap();
        let canonical = EventStore::query_raw(
            runtime.client.storage().unwrap(),
            EventQuery::all(EventQueryBounds::first(20).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(canonical.items()[0].position(), receipt.position());
        refresh(&runtime).await;
        seed_rows(&runtime, ProjectionGeneration::new([11; 32]).unwrap(), true).await;
        let before = source(&runtime).await;
        if capacity {
            counts.faults.disk_full("collection", 1, after);
        } else {
            counts.faults.failure("collection", 1, after);
        }
        let result = runtime
            .phase1_refresh_today(&context(None, 1), NOW, TodayProjectionUpdate::Incremental)
            .await;
        assert!(match result {
            Err(TodayError::Storage(radroots_storage::Error::SpaceInsufficient)) => capacity,
            Err(TodayError::Storage(radroots_storage::Error::BackendUnavailable)) => !capacity,
            _ => false,
        });
        assert_eq!(source(&runtime).await, before);
        assert!(!runtime.today_projection_lock.can_collect());
        let bytes = usage(&runtime).await;
        counts.take();
        refresh(&runtime).await;
        assert!(!counts.take().contains_key("projection.collect_retired"));
        assert_eq!(usage(&runtime).await, bytes);
        runtime.shutdown().await.unwrap();
        let (runtime, counts) = open(&root).await;
        refresh(&runtime).await;
        assert!(counts.take().contains_key("projection.collect_retired"));
        assert_eq!(source(&runtime).await, before);
        assert_eq!(
            EventStore::query_raw(
                runtime.client.storage().unwrap(),
                EventQuery::all(EventQueryBounds::first(20).unwrap())
            )
            .await
            .unwrap(),
            canonical
        );
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancelled_collection_keeps_uncertainty_and_serializes_live_reads() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, counts) = open(&root).await;
    refresh(&runtime).await;
    seed_rows(&runtime, ProjectionGeneration::new([11; 32]).unwrap(), true).await;
    let pause = counts.faults.pause("collection", 1, false);
    let selected = context(None, 1);
    let mut refresh_call =
        Box::pin(runtime.phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Incremental));
    tokio::select! { _ = &mut refresh_call => panic!("actual collection must pause"), _ = pause.entered.notified() => {} }
    let mut read =
        Box::pin(runtime.phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC")));
    tokio::select! { value = &mut read => panic!("read escaped held Today mutex: {value:?}"), _ = tokio::task::yield_now() => {} }
    drop(refresh_call);
    assert!(!runtime.today_projection_lock.can_collect());
    assert!(read.await.unwrap().items.is_empty());
    counts.take();
    refresh(&runtime).await;
    assert!(!counts.take().contains_key("projection.collect_retired"));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_admission_refuses_collection_before_deletion() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, counts) = open(&root).await;
    refresh(&runtime).await;
    seed_rows(&runtime, ProjectionGeneration::new([11; 32]).unwrap(), true).await;
    let before = usage(&runtime).await;
    let pause = counts.faults.pause("collection", 1, false);
    let selected = context(None, 1);
    let mut pending =
        Box::pin(runtime.phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Incremental));
    tokio::select! { _ = &mut pending => panic!("actual collection must pause"), _ = pause.entered.notified() => {} }
    let receipt = EventStore::admit(
        runtime.client.storage().unwrap(),
        visible_admission(
            signed(1, vec![], "Synthetic concurrently admitted record", NOW - 1),
            NOW * 1000,
        ),
    )
    .await
    .unwrap();
    pause.resume.notify_one();
    assert!(matches!(
        pending.await,
        Err(TodayError::Storage(
            radroots_storage::Error::SourceEpochChanged
        ))
    ));
    assert_eq!(usage(&runtime).await, before);
    let raw = EventStore::query_raw(
        runtime.client.storage().unwrap(),
        EventQuery::all(EventQueryBounds::first(20).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(raw.items()[0].position(), receipt.position());
    assert!(
        runtime.today_projection_lock.can_collect(),
        "A proven pre-effect source refusal must not disable future collection"
    );
    counts.take();
    refresh(&runtime).await;
    assert_eq!(counts.take().get("projection.collected_rows"), Some(&64));
    runtime.shutdown().await.unwrap();
    let (runtime, _) = open(&root).await;
    refresh(&runtime).await;
    assert_eq!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        EventStore::query_raw(
            runtime.client.storage().unwrap(),
            EventQuery::all(EventQueryBounds::first(20).unwrap())
        )
        .await
        .unwrap(),
        raw
    );
    runtime.shutdown().await.unwrap();
}

#[cfg(feature = "mobile-social")]
#[tokio::test]
async fn capacity_refusal_preserves_queued_intent_and_legacy_custody_after_reopen() {
    use super::super::super::{
        CreateUpdate, Phase1AddCommand, Phase1CancellationPolicy, Phase1QueuePolicy,
        Phase1RelaySatisfaction,
    };
    let root = tempfile::tempdir().unwrap();
    let mut runtime = reopen_counted_sqlite(&root).await;
    calendar_migration_tests::seed_legacy(&runtime).await;
    let selected = context(None, 1);
    let generation = calendar_migration::legacy_generation().unwrap();
    let custody = ProjectionStore::projection_document(
        runtime.client.storage().unwrap(),
        projection_id().unwrap(),
        generation,
        projection_document_key(&selected),
    )
    .await
    .unwrap()
    .unwrap();
    let saved = runtime
        .phase1_save_draft(
            [19; 16],
            Phase1AddCommand::CreateUpdate(
                CreateUpdate::new("Synthetic queued collection fixture").unwrap(),
            ),
            NOW,
            vec![],
            None,
            NOW * 1000,
        )
        .await
        .unwrap();
    let queued = runtime
        .phase1_queue_draft(
            [19; 16],
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
    // Queue/status require the original SDK sync coordinator. The counting
    // client wraps only the exact same real storage during cache maintenance.
    let configured_client = runtime.client.clone();
    let counts = instrument(&mut runtime);
    refresh(&runtime).await;
    seed_rows(&runtime, ProjectionGeneration::new([11; 32]).unwrap(), true).await;
    let source_before = source(&runtime).await;
    counts.faults.disk_full("collection", 1, false);
    assert!(matches!(
        runtime
            .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Incremental)
            .await,
        Err(TodayError::Storage(
            radroots_storage::Error::SpaceInsufficient
        ))
    ));
    runtime.client = configured_client;
    assert_eq!(runtime.phase1_draft_status([19; 16]).await.unwrap(), queued);
    assert_eq!(source(&runtime).await, source_before);
    runtime.shutdown().await.unwrap();
    let runtime = reopen_counted_sqlite(&root).await;
    refresh(&runtime).await;
    assert_eq!(runtime.phase1_draft_status([19; 16]).await.unwrap(), queued);
    assert_eq!(
        ProjectionStore::projection_document(
            runtime.client.storage().unwrap(),
            projection_id().unwrap(),
            generation,
            projection_document_key(&selected)
        )
        .await
        .unwrap(),
        Some(custody)
    );
    assert_eq!(source(&runtime).await, source_before);
    runtime.shutdown().await.unwrap();
}
