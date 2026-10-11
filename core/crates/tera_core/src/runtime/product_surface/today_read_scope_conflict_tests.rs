//! Actual SQLite payload/repair conflicts retain their current-read meaning.
use super::*;
use radroots_storage::projection::{InvalidationReason, ProjectionInvalidation};

async fn seeded(runtime: &TeraRuntime, selected: &LocalNetwork) -> Vec<String> {
    // More than64 projected rows makes the second supporting payload page real.
    for index in 0..33 {
        EventStore::admit(
            runtime.client.storage().unwrap(),
            visible_admission(
                signed(1, vec![], &format!("scope item {index}"), NOW),
                NOW * 1_000,
            ),
        )
        .await
        .unwrap();
    }
    runtime
        .phase1_refresh_today(selected, NOW, TodayProjectionUpdate::Rebuild)
        .await
        .unwrap();
    runtime
        .phase1_today_page(selected, TodayPageRequest::first(100, NOW, "UTC"))
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|card| card.card.card_id.to_hex())
        .collect()
}

async fn conflict(runtime: &TeraRuntime, projection: bool) {
    let storage = runtime.client.storage().unwrap();
    if projection {
        let id = ProjectionId::parse(build::progress::ID).unwrap();
        let generation = ProjectionStore::status(storage, id.clone())
            .await
            .unwrap()
            .unwrap()
            .generation();
        ProjectionStore::invalidate(
            storage,
            ProjectionInvalidation::new(
                id,
                generation,
                ProjectionGeneration::new([19; 32]).unwrap(),
                InvalidationReason::OperatorRequested,
                NOW * 1_000 + 1,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    } else {
        EventStore::admit(
            storage,
            visible_admission(signed(1, vec![], "scope new-current", NOW + 1), NOW * 1_000),
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn sqlite_payload_pages_refuse_source_and_projection_conflicts_but_keep_offline_errors() {
    for operation in 1..=3 {
        for ordinal in [1, 2] {
            for after in [false, true] {
                for projection in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let (runtime, counts) = instrumented(&root).await;
                    let selected = context(None, 1);
                    let ids = seeded(&runtime, &selected).await;
                    counts.take();
                    let pause = counts.faults.pause("read_payload", ordinal, after);
                    let runtime = Arc::new(runtime);
                    let reader = {
                        let runtime = runtime.clone();
                        let selected = selected.clone();
                        tokio::spawn(
                            async move { read_ids(&runtime, &selected, operation, &ids).await },
                        )
                    };
                    tokio::time::timeout(Duration::from_secs(2), pause.entered.notified())
                        .await
                        .unwrap();
                    conflict(&runtime, projection).await;
                    pause.resume.notify_one();
                    let result = reader.await.unwrap();
                    assert!(
                        matches!(result, Err(TodayError::Cursor(CursorError::Stale))),
                        "{operation}/{ordinal}/{after}/{projection}: {result:?}"
                    );
                    let reads = counts.take();
                    assert_eq!(reads.get("event.rows").copied().unwrap_or(0), 0);
                    let attempted = reads
                        .get("projection.payload_queries")
                        .copied()
                        .unwrap_or(0);
                    assert!(attempted >= ordinal as u64);
                    assert!(attempted <= ordinal as u64 + u64::from(after));
                    runtime.shutdown().await.unwrap();
                }
                let root = tempfile::tempdir().unwrap();
                let (runtime, counts) = instrumented(&root).await;
                let selected = context(None, 1);
                let ids = seeded(&runtime, &selected).await;
                counts.faults.failure("read_payload", ordinal, after);
                assert!(matches!(
                    read_ids(&runtime, &selected, operation, &ids).await,
                    Err(TodayError::Storage(
                        radroots_storage::Error::BackendUnavailable
                    ))
                ));
                runtime.shutdown().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn sqlite_initial_read_repair_refuses_source_races_with_the_typed_stale_error() {
    for operation in 0..4 {
        for after in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, counts) = instrumented(&root).await;
            let selected = context(None, 1);
            let ids = seeded(&runtime, &selected).await;
            conflict(&runtime, false).await;
            let pause = counts.faults.pause("read_visible", 1, after);
            let runtime = Arc::new(runtime);
            let reader = {
                let runtime = runtime.clone();
                let selected = selected.clone();
                tokio::spawn(async move { read_ids(&runtime, &selected, operation, &ids).await })
            };
            tokio::time::timeout(Duration::from_secs(2), pause.entered.notified())
                .await
                .unwrap();
            EventStore::admit(
                runtime.client.storage().unwrap(),
                visible_admission(signed(1, vec![], "scope newer", NOW + 2), NOW * 1_000),
            )
            .await
            .unwrap();
            pause.resume.notify_one();
            let result = reader.await.unwrap();
            assert!(
                matches!(result, Err(TodayError::Cursor(CursorError::Stale))),
                "{operation}/{after}: {result:?}"
            );
            runtime.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn known_stale_read_repair_offline_failure_refuses_cache_and_preserves_space_recovery() {
    for operation in 0..4 {
        for space in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, counts) = instrumented(&root).await;
            let selected = context(None, 1);
            let ids = seeded(&runtime, &selected).await;
            // A committed source mutation makes the previously Ready cache stale.
            conflict(&runtime, false).await;
            if space {
                counts.faults.disk_full("read_visible", 1, false);
            } else {
                counts.faults.failure("read_visible", 1, false);
            }
            let result = read_ids(&runtime, &selected, operation, &ids).await;
            if space {
                assert!(
                    matches!(
                        result,
                        Err(TodayError::Storage(
                            radroots_storage::Error::SpaceInsufficient
                        ))
                    ),
                    "{operation}: preserve the existing capacity recovery: {result:?}"
                );
            } else {
                assert!(
                    matches!(result, Err(TodayError::ProjectionMissing)),
                    "{operation}: cannot retain cache already proved stale: {result:?}"
                );
            }
            runtime.shutdown().await.unwrap();
        }
    }
}
