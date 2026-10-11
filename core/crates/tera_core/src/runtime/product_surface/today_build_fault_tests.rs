use super::super::{
    performance_tests::counted_storage::{CountedStorage, QueryCounts},
    tests::{context, keys, signed, visible_admission},
};
use super::*;
use std::sync::Arc;
use tokio::time::{Duration, timeout};

const NOW: u64 = 2_000_000_000;
async fn counted(root: &tempfile::TempDir) -> (Arc<TeraRuntime>, Arc<QueryCounts>) {
    let mut runtime = super::super::performance_tests::reopen_counted_sqlite(root).await;
    let counts = Arc::new(QueryCounts::default());
    runtime.client = radroots_sdk::ClientBuilder::new()
        .storage(Arc::new(CountedStorage {
            client: runtime.client.clone(),
            counts: counts.clone(),
        }))
        .build()
        .unwrap();
    (Arc::new(runtime), counts)
}
async fn fixture() -> (tempfile::TempDir, Arc<TeraRuntime>, Arc<QueryCounts>) {
    let root = tempfile::tempdir().unwrap();
    let (runtime, counts) = counted(&root).await;
    for index in 0..130 {
        EventStore::admit(
            runtime.client.storage().unwrap(),
            visible_admission(
                signed(
                    1,
                    vec![],
                    &format!("Synthetic durable build row{index}"),
                    NOW,
                ),
                NOW * 1_000,
            ),
        )
        .await
        .unwrap();
    }
    counts.take();
    (root, runtime, counts)
}
async fn build(runtime: &TeraRuntime) -> Result<BuildOutcome, TodayError> {
    let selected = context(None, 1);
    let scope = paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
    let _guard = runtime.today_projection_lock.lock().await;
    run(runtime, &selected, scope, NOW * 1_000).await
}
fn complete(result: BuildOutcome) -> BuildProgress {
    let BuildOutcome::Complete(value) = result else {
        panic!("small source must complete");
    };
    *value
}

#[tokio::test]
async fn every_batch_and_promotion_refusal_reopens_to_exact_complete_rows() {
    let batches = 130usize.div_ceil(usize::from(SOURCE_ROWS));
    for (operation, ordinal) in (1..=batches)
        .map(|ordinal| ("batch", ordinal))
        .chain([("promotion", 1)])
    {
        for after in [false, true] {
            let (root, runtime, counts) = fixture().await;
            counts.faults.failure(operation, ordinal, after);
            assert!(
                matches!(
                    build(&runtime).await,
                    Err(TodayError::Storage(
                        radroots_storage::Error::BackendUnavailable
                    ))
                ),
                "{operation}/{ordinal}/{after}"
            );
            assert!(!runtime.today_projection_lock.can_collect());
            let active = ProjectionStore::status(
                runtime.client.storage().unwrap(),
                ProjectionId::parse(progress::ID).unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(active.is_some(), operation == "promotion" && after);
            runtime.shutdown().await.unwrap();
            drop(runtime);
            let (reopened, _) = counted(&root).await;
            let built = complete(build(&reopened).await.unwrap());
            assert_eq!(built.cards, 130);
            assert_eq!(built.document_rows().unwrap(), 262);
            let active = ProjectionStore::status(
                reopened.client.storage().unwrap(),
                ProjectionId::parse(progress::ID).unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(active.generation(), built.generation);
            assert_eq!(active.checkpoint().unwrap().projected_rows(), 262);
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn actual_cancelled_sqlite_batch_resumes_only_the_unscanned_rows_after_reopen() {
    let (root, runtime, counts) = fixture().await;
    let pause = counts.faults.pause("batch", 2, true);
    let selected = runtime.clone();
    let pending = tokio::spawn(async move { build(&selected).await });
    timeout(Duration::from_secs(5), pause.entered.notified())
        .await
        .unwrap();
    let before = progress::pointer(runtime.client.storage().unwrap())
        .await
        .unwrap()
        .unwrap();
    let acknowledged = progress::load(
        runtime.client.storage().unwrap(),
        ProjectionId::parse(progress::ID).unwrap(),
        before.generation,
        progress::PROGRESS,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(acknowledged.cards, 2 * u64::from(SOURCE_ROWS));
    pending.abort();
    assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
    assert!(!runtime.today_projection_lock.can_collect());
    assert!(
        ProjectionStore::status(
            runtime.client.storage().unwrap(),
            ProjectionId::parse(progress::ID).unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let (reopened, calls) = counted(&root).await;
    let built = complete(build(&reopened).await.unwrap());
    assert_eq!(built.generation, before.generation);
    assert_eq!(built.cards, 130);
    let observed = calls.take();
    let remaining = 130 - acknowledged.cards;
    assert_eq!(observed.get("event.rows"), Some(&remaining));
    assert_eq!(
        observed.get("projection.put_projection_batch"),
        Some(&remaining.div_ceil(u64::from(SOURCE_ROWS)))
    );
    assert!(observed["event.max_rows"] <= 63 && observed["event.max_work_bytes"] <= WORK_BYTES);
    assert!(
        observed["projection.max_batch_rows"] <= 64
            && observed["projection.max_batch_bytes"] <= WORK_BYTES
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_canonical_source_cannot_promote_a_staged_generation() {
    let (root, runtime, counts) = fixture().await;
    let pause = counts.faults.pause("batch", 1, true);
    let selected = runtime.clone();
    let pending = tokio::spawn(async move { build(&selected).await });
    timeout(Duration::from_secs(5), pause.entered.notified())
        .await
        .unwrap();
    let prior = progress::pointer(runtime.client.storage().unwrap())
        .await
        .unwrap()
        .unwrap();
    EventStore::admit(
        runtime.client.storage().unwrap(),
        visible_admission(
            signed(
                1,
                vec![],
                "Synthetic source changed during staging",
                NOW + 1,
            ),
            (NOW + 1) * 1_000,
        ),
    )
    .await
    .unwrap();
    pause.resume.notify_one();
    assert!(matches!(
        pending.await.unwrap(),
        Err(TodayError::Storage(
            radroots_storage::Error::SourceEpochChanged
        ))
    ));
    assert!(
        ProjectionStore::status(
            runtime.client.storage().unwrap(),
            ProjectionId::parse(progress::ID).unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let (reopened, _) = counted(&root).await;
    let built = complete(build(&reopened).await.unwrap());
    assert_ne!(built.generation, prior.generation);
    assert_eq!(built.cards, 131);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn changed_policy_after_terminal_batch_refuses_promotion_and_rebuilds_under_current_policy() {
    use crate::runtime::visibility::AuthorVisibility;
    let (root, runtime, counts) = fixture().await;
    let pause = counts.faults.pause("batch", 3, true);
    let selected = runtime.clone();
    let pending = tokio::spawn(async move { build(&selected).await });
    timeout(Duration::from_secs(5), pause.entered.notified())
        .await
        .unwrap();
    let policy_owner = TeraRuntime::test_memory().unwrap();
    policy_owner
        .set_author_visibility(&keys().public_key().to_string(), AuthorVisibility::Muted)
        .await
        .unwrap();
    let id = ProjectionId::parse("tera.author_visibility.v1").unwrap();
    let generation = ProjectionGeneration::new(Sha256::digest(id.as_str()).into()).unwrap();
    let document = ProjectionStore::projection_document(
        policy_owner.client.storage().unwrap(),
        id.clone(),
        generation,
        "policy".into(),
    )
    .await
    .unwrap()
    .unwrap();
    ProjectionStore::put_projection_document(
        runtime.client.storage().unwrap(),
        id,
        generation,
        document,
    )
    .await
    .unwrap();
    pause.resume.notify_one();
    assert!(matches!(
        pending.await.unwrap(),
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    assert!(
        ProjectionStore::status(
            runtime.client.storage().unwrap(),
            ProjectionId::parse(progress::ID).unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let (reopened, _) = counted(&root).await;
    let built = complete(build(&reopened).await.unwrap());
    assert_eq!(built.cards, 0);
    assert_eq!(built.source_events, 130);
    assert_eq!(built.document_rows().unwrap(), 2);
    reopened.shutdown().await.unwrap();
}
