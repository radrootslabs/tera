use super::super::tests::{context, signed, visible_admission};
use super::*;
use radroots_storage::projection::document_query::{
    ProjectionDocumentGenerations, ProjectionDocumentQuery,
};

#[tokio::test]
async fn actual_promoted_generation_with_missing_rank_index_refuses_false_empty_eof() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = if sqlite {
            super::super::performance_tests::reopen_counted_sqlite(&root).await
        } else {
            TeraRuntime::test_memory().unwrap()
        };
        let selected = context(None, 1);
        let storage = runtime.client.storage().unwrap();
        EventStore::admit(
            storage,
            visible_admission(
                signed(1, vec![], "Synthetic indexed card", 2_000_000_000),
                2_000_000_000_000,
            ),
        )
        .await
        .unwrap();
        let source = EventStore::source_token(storage).await.unwrap();
        while !EventStore::maintain_visibility(
            storage,
            VisibilityIndexBounds::new(64, WORK_BYTES).unwrap(),
        )
        .await
        .unwrap()
        .is_complete()
        {}
        let events = EventStore::query_visible_at(
            storage,
            EventQuery::all(EventQueryBounds::first(1).unwrap()),
            source,
        )
        .await
        .unwrap();
        let row = rows::reduce(&selected, &events.items()[0])
            .unwrap()
            .unwrap();
        let rows::TodayRow::Card(card) = &row else {
            panic!("fixture card");
        };
        let reference = page_index::documents(card).unwrap().pop().unwrap();
        let query_scope = paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
        let mut built = BuildProgress::new(source, query_scope, None, None, 1).unwrap();
        built.cards = 1;
        built.index_rows = 1;
        built.complete = true;
        let id = ProjectionId::parse(progress::ID).unwrap();
        ProjectionStore::configure_projection_cache(
            storage,
            id.clone(),
            ProjectionCacheLimits::new(64 * 1024 * 1024, 2, 32, 1_800_000).unwrap(),
        )
        .await
        .unwrap();
        // Opaque storage mechanics can count a misplaced reference. The app
        // must still reject incomplete indexed coverage rather than expose EOF.
        ProjectionStore::put_projection_batch(
            storage,
            ProjectionDocumentBatch::new(
                built.scope().unwrap(),
                vec![
                    row.document().unwrap(),
                    ProjectionDocument::new("missing.rank".into(), reference.value().into())
                        .unwrap(),
                    ProjectionDocument::new(progress::PROGRESS.into(), encode(&built).unwrap())
                        .unwrap(),
                ],
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let checkpoint = ProjectionCheckpoint::new(
            id,
            built.generation,
            Some(EventPosition::new(
                source.generation(),
                EventSequence::new(1).unwrap(),
            )),
            4,
            2_000_000_000_000,
        )
        .unwrap();
        ProjectionStore::promote_projection_batch(
            storage,
            ProjectionDocumentBatch::new(
                built.scope().unwrap(),
                vec![
                    ProjectionDocument::new(progress::HEADER.into(), encode(&built).unwrap())
                        .unwrap(),
                ],
            )
            .unwrap(),
            checkpoint,
        )
        .await
        .unwrap();
        let local =
            sidecar::LocalState::new(&selected, source, built.generation, query_scope, None);
        let guard = runtime.today_projection_lock.lock().await;
        sidecar::save(&runtime, &selected, &local).await.unwrap();
        drop(guard);
        assert!(matches!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::first(20, 2_000_000_000, "UTC"))
                .await,
            Err(TodayError::CorruptProjection)
        ));
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn complete_generation_contains_independent_rows_and_exact_checkpoint() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let storage = runtime.client.storage().unwrap();
    for index in 0..130 {
        EventStore::admit(
            storage,
            visible_admission(
                signed(1, vec![], &format!("Synthetic row{index}"), 2_000_000_000),
                2_000_000_000_000,
            ),
        )
        .await
        .unwrap();
    }
    let _guard = runtime.today_projection_lock.lock().await;
    let scope = paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
    let BuildOutcome::Complete(built) = run(&runtime, &selected, scope, 2_000_000_000_000)
        .await
        .unwrap()
    else {
        panic!("complete small fixture");
    };
    assert_eq!(built.cards, 130);
    assert_eq!(built.index_rows, 130);
    assert_eq!(built.document_rows().unwrap(), 262);
    let status = ProjectionStore::status(storage, ProjectionId::parse(progress::ID).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.generation(), built.generation);
    assert_eq!(status.health(), ProjectionHealth::Ready);
    let mut page = ProjectionStore::query_projection_documents_at(
        storage,
        ProjectionDocumentQuery::new(
            ProjectionId::parse(progress::ID).unwrap(),
            ProjectionDocumentGenerations::Exact(built.generation),
            256,
        )
        .unwrap(),
        built.source,
        built.generation,
    )
    .await
    .unwrap();
    let mut records = page.records().to_vec();
    while let Some(cursor) = page.next_cursor() {
        let query = ProjectionDocumentQuery::new(
            ProjectionId::parse(progress::ID).unwrap(),
            ProjectionDocumentGenerations::Exact(built.generation),
            256,
        )
        .unwrap()
        .with_cursor(cursor)
        .unwrap();
        page = ProjectionStore::query_projection_documents_at(
            storage,
            query,
            built.source,
            built.generation,
        )
        .await
        .unwrap();
        records.extend_from_slice(page.records());
    }
    assert_eq!(records.len(), 262);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.key().starts_with(page_index::FIRST))
            .inspect(|record| {
                page_index::Reference::from_document(record.document().unwrap()).unwrap();
            })
            .count(),
        130
    );
    let mut rows = 0;
    for record in records
        .iter()
        .filter(|record| record.key().starts_with("card."))
    {
        assert!(matches!(
            rows::TodayRow::from_document(record.document().unwrap()).unwrap(),
            rows::TodayRow::Card(_)
        ));
        rows += 1;
    }
    assert_eq!(rows, 130);
    let BuildOutcome::Complete(again) = run(&runtime, &selected, scope, 2_000_000_001_000)
        .await
        .unwrap()
    else {
        panic!("complete unchanged generation");
    };
    assert_eq!(again.generation, built.generation);
    assert_eq!(again.cards, built.cards);
}

#[tokio::test]
async fn terminal_staged_progress_resumes_promotion_without_recounting() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let storage = runtime.client.storage().unwrap();
    EventStore::admit(
        storage,
        visible_admission(
            signed(1, vec![], "Synthetic pending promotion", 2_000_000_000),
            2_000_000_000_000,
        ),
    )
    .await
    .unwrap();
    let source = EventStore::source_token(storage).await.unwrap();
    let scope = paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
    let id = ProjectionId::parse(progress::ID).unwrap();
    let _guard = runtime.today_projection_lock.lock().await;
    while !EventStore::maintain_visibility(
        storage,
        VisibilityIndexBounds::new(64, WORK_BYTES).unwrap(),
    )
    .await
    .unwrap()
    .is_complete()
    {}
    ProjectionStore::configure_projection_cache(
        storage,
        id.clone(),
        ProjectionCacheLimits::new(64 * 1024 * 1024, 2, 32, 1_800_000).unwrap(),
    )
    .await
    .unwrap();
    let mut staged = BuildProgress::new(source, scope, None, None, 1).unwrap();
    let events = EventStore::query_visible_at(
        storage,
        EventQuery::all(EventQueryBounds::first(1).unwrap()),
        source,
    )
    .await
    .unwrap();
    let row = rows::reduce(&selected, &events.items()[0])
        .unwrap()
        .unwrap();
    let rows::TodayRow::Card(card) = &row else {
        panic!("fixture card");
    };
    let mut documents = page_index::documents(card).unwrap();
    staged.index_rows = documents.len() as u64;
    staged.count(&row).unwrap();
    staged.complete = true;
    progress::save_pointer(&runtime, &staged).await.unwrap();
    documents.push(row.document().unwrap());
    documents.push(
        ProjectionDocument::new(progress::PROGRESS.into(), encode(&staged).unwrap()).unwrap(),
    );
    ProjectionStore::put_projection_batch(
        storage,
        ProjectionDocumentBatch::new(staged.scope().unwrap(), documents).unwrap(),
    )
    .await
    .unwrap();
    assert!(
        ProjectionStore::status(storage, id.clone())
            .await
            .unwrap()
            .is_none()
    );
    let BuildOutcome::Complete(completed) = run(&runtime, &selected, scope, 2_000_000_000_000)
        .await
        .unwrap()
    else {
        panic!("resume completed scan");
    };
    assert_eq!(completed.cards, 1);
    assert_eq!(completed.generation, staged.generation);
    assert_eq!(
        ProjectionStore::status(storage, id)
            .await
            .unwrap()
            .unwrap()
            .checkpoint()
            .unwrap()
            .projected_rows(),
        4
    );
}

#[tokio::test]
async fn returning_to_an_unchanged_context_does_not_reuse_retired_staging_identity() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let first = context(Some("Victoria"), 1);
    let second = context(Some("Toronto"), 1);
    let _guard = runtime.today_projection_lock.lock().await;
    for selected in [&first, &second, &first] {
        let scope = paging_scope::query_scope(selected, runtime.store_public_key).unwrap();
        let result = run(&runtime, selected, scope, 2_000_000_000_000)
            .await
            .unwrap();
        assert!(matches!(result, BuildOutcome::Complete(_)));
    }
}
