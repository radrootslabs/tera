use super::super::{
    calendar_migration_tests::{open, seed_legacy},
    tests::context,
};
use super::*;
const NOW: u64 = 2_000_000_000;

#[tokio::test]
async fn promoted_rows_before_retirement_intent_recover_after_source_or_context_changes() {
    use super::super::tests::{signed, visible_admission};
    for (changed_source, interrupted_retirement) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let root = tempfile::tempdir().unwrap();
        let runtime = open(root.path()).await;
        seed_legacy(&runtime).await;
        let selected = context(None, 1);
        let prior = {
            let _guard = runtime.today_projection_lock.lock().await;
            let scope = paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
            let build::BuildOutcome::Complete(value) =
                build::run(&runtime, &selected, scope, NOW * 1_000)
                    .await
                    .unwrap()
            else {
                panic!("small legacy source");
            };
            value
        };
        let storage = runtime.client.storage().unwrap();
        let row_header = ProjectionStore::projection_document(
            storage,
            ProjectionId::parse(build::progress::ID).unwrap(),
            prior.generation,
            build::progress::HEADER.into(),
        )
        .await
        .unwrap()
        .unwrap();
        let legacy = ProjectionStore::projection_document(
            storage,
            projection_id().unwrap(),
            calendar_migration::legacy_generation().unwrap(),
            projection_document_key(&selected),
        )
        .await
        .unwrap()
        .unwrap();
        if interrupted_retirement {
            ProjectionStore::invalidate(
                storage,
                ProjectionInvalidation::new(
                    ProjectionId::parse(build::progress::ID).unwrap(),
                    prior.generation,
                    ProjectionGeneration::new([7; 32]).unwrap(),
                    InvalidationReason::ProjectionGenerationChanged,
                    NOW * 1_000 + 1,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
        assert!(
            load(runtime.client.storage().unwrap())
                .await
                .unwrap()
                .is_none()
        );
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = open(root.path()).await;
        if changed_source {
            EventStore::admit(
                reopened.client.storage().unwrap(),
                visible_admission(
                    signed(1, vec![], "Synthetic post-promotion update", NOW + 1),
                    (NOW + 1) * 1_000,
                ),
            )
            .await
            .unwrap();
        }
        let selected = context((!changed_source).then_some("Victoria"), 1);
        let receipt = reopened
            .phase1_refresh_today(&selected, NOW + 2, TodayProjectionUpdate::Rebuild)
            .await
            .unwrap();
        assert_eq!(receipt.source_events, if changed_source { 2 } else { 1 });
        assert!(
            load(reopened.client.storage().unwrap())
                .await
                .unwrap()
                .unwrap()
                .complete
        );
        let state = load_state(
            reopened.client.storage().unwrap(),
            &selected,
            projection_generation().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.cards.len(), if changed_source { 2 } else { 1 });
        let storage = reopened.client.storage().unwrap();
        assert_eq!(
            ProjectionStore::projection_document(
                storage,
                ProjectionId::parse(build::progress::ID).unwrap(),
                prior.generation,
                build::progress::HEADER.into()
            )
            .await
            .unwrap(),
            if interrupted_retirement {
                None
            } else {
                Some(row_header)
            }
        );
        let current = state
            .row_generation
            .expect("current complete row generation");
        assert_ne!(current, prior.generation);
        let header = build::progress::load(
            storage,
            ProjectionId::parse(build::progress::ID).unwrap(),
            current,
            build::progress::HEADER,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(header.complete);
        assert_eq!(header.generation, current);
        assert_eq!(
            ProjectionStore::projection_document(
                storage,
                projection_id().unwrap(),
                calendar_migration::legacy_generation().unwrap(),
                projection_document_key(&context(None, 1))
            )
            .await
            .unwrap()
            .unwrap(),
            legacy
        );
        let usage = ProjectionStore::configure_projection_cache(
            storage,
            ProjectionId::parse(build::progress::ID).unwrap(),
            radroots_storage::projection::cache::ProjectionCacheLimits::new(
                64 * 1024 * 1024,
                2,
                32,
                1_800_000,
            )
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(usage.protected_generations, 1);
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn pending_coordination_retirement_reopens_at_each_owner_boundary_without_losing_legacy_bytes()
 {
    for phase in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let runtime = open(root.path()).await;
        seed_legacy(&runtime).await;
        let selected = context(None, 1);
        let storage = runtime.client.storage().unwrap();
        let old = ProjectionStore::projection_document(
            storage,
            projection_id().unwrap(),
            calendar_migration::legacy_generation().unwrap(),
            projection_document_key(&selected),
        )
        .await
        .unwrap()
        .unwrap();
        {
            let _guard = runtime.today_projection_lock.lock().await;
            let scope = paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
            let build::BuildOutcome::Complete(progress) =
                build::run(&runtime, &selected, scope, NOW * 1_000)
                    .await
                    .unwrap()
            else {
                panic!("small legacy fixture");
            };
            let events = EventStore::status(storage).await.unwrap();
            let ticket = calendar_migration::begin(storage, NOW * 1_000, &events)
                .await
                .unwrap()
                .unwrap();
            calendar_migration::complete(
                storage,
                &ticket,
                ProjectionCheckpoint::new(
                    projection_id().unwrap(),
                    projection_generation().unwrap(),
                    Some(EventPosition::new(
                        events.generation(),
                        EventSequence::new(events.raw_events()).unwrap(),
                    )),
                    events.raw_events(),
                    NOW * 1_000,
                )
                .unwrap(),
            )
            .await
            .unwrap();
            let marker = Retirement {
                schema_version: 1,
                source: progress.source,
                rows: progress.generation,
                now_unix_ms: NOW * 1_000,
                complete: false,
            };
            save(&runtime, &marker).await.unwrap();
            if phase >= 1 {
                ProjectionStore::invalidate(storage, marker.invalidation().unwrap())
                    .await
                    .unwrap();
            }
            if phase >= 2 {
                ProjectionStore::retire_projection_generation(
                    storage,
                    ProjectionCacheScope::new(
                        projection_id().unwrap(),
                        projection_generation().unwrap(),
                        marker.source,
                        Some(projection_generation().unwrap()),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            }
            assert!(!load(storage).await.unwrap().unwrap().complete);
        }
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = open(root.path()).await;
        reopened
            .phase1_refresh_today(&selected, NOW + 1, TodayProjectionUpdate::Incremental)
            .await
            .unwrap();
        let storage = reopened.client.storage().unwrap();
        let marker = load(storage).await.unwrap().unwrap();
        assert!(marker.complete);
        let status = ProjectionStore::status(storage, projection_id().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.health(), ProjectionHealth::Invalidated);
        assert!(retired(storage, &status).await.unwrap());
        assert_eq!(
            ProjectionStore::projection_document(
                storage,
                projection_id().unwrap(),
                calendar_migration::legacy_generation().unwrap(),
                projection_document_key(&selected)
            )
            .await
            .unwrap()
            .unwrap(),
            old
        );
        let page = reopened
            .phase1_today_page(&selected, TodayPageRequest::first(20, NOW + 1, "UTC"))
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        reopened.shutdown().await.unwrap();
    }
}

#[test]
fn retirement_marker_refuses_unknown_schema_generation_and_time() {
    let source = EventSourceToken::new(
        radroots_storage::event::SourceGeneration::new([1; 32]).unwrap(),
        1,
    );
    let value = Retirement {
        schema_version: 1,
        source,
        rows: ProjectionGeneration::new([2; 32]).unwrap(),
        now_unix_ms: 1,
        complete: false,
    };
    value.validate().unwrap();
    for variant in 0..3 {
        let mut invalid = value.clone();
        match variant {
            0 => invalid.schema_version += 1,
            1 => invalid.rows = projection_generation().unwrap(),
            _ => invalid.now_unix_ms = 0,
        }
        assert!(invalid.validate().is_err());
    }
}
