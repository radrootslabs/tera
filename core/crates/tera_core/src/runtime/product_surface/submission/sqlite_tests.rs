use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    lifecycle::RuntimeLifecycleError,
    product_surface::submission::test_support::*,
    product_surface::{ComposerEditSequence, SubmissionCommandId},
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};

fn config(root: &std::path::Path, author: &str, generation: u8) -> MobileUserStoreConfig {
    let config = MobileUserStoreConfig::from_encoded(
        root,
        author,
        &hex::encode([generation; 32]),
        NOW,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    config
}

#[tokio::test]
async fn actual_sqlite_replays_old_source_after_newer_edits_and_runtime_reconstruction() {
    let root = tempfile::tempdir().unwrap();
    let runtime = RuntimeBuilder::new(config(root.path(), AUTHOR, 1))
        .build()
        .await
        .unwrap();
    let request = request();
    let saved = runtime
        .composer_create(
            request.scope(),
            request.composer_id(),
            ComposerEditSequence::INITIAL,
            form("PRIVATE initial partial"),
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        runtime.submission_reserve(&request),
        runtime.submission_reserve(&request)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.reservation_id(), b.reservation_id());
    assert_ne!(a.is_replay(), b.is_replay());
    assert_eq!(a.captured(), saved.draft());
    let newer = runtime
        .composer_save(
            request.scope(),
            request.composer_id(),
            request.expected_revision(),
            ComposerEditSequence::new(2).unwrap(),
            form("PRIVATE later edit"),
        )
        .await
        .unwrap();
    assert_ne!(newer.draft(), a.captured());
    let mut foreign = request.clone();
    foreign.scope = scope(OTHER, "nearby");
    assert_eq!(
        runtime.submission_reserve(&foreign).await.unwrap_err(),
        E::Source(SourceError::ScopeMismatch)
    );
    runtime.shutdown().await.unwrap();
    assert_eq!(
        runtime.submission_reserve(&request).await.unwrap_err(),
        E::Lifecycle(RuntimeLifecycleError::Closed)
    );
    drop(runtime);
    // Storage generation is durable identity, unlike the native session guard.
    assert!(
        RuntimeBuilder::new(config(root.path(), AUTHOR, 9))
            .build()
            .await
            .is_err()
    );
    let runtime = RuntimeBuilder::new(config(root.path(), AUTHOR, 1))
        .build()
        .await
        .unwrap();
    let replay = runtime.submission_reserve(&request).await.unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.captured(), saved.draft());
    assert_eq!(replay.reservation_id(), a.reservation_id());
    assert_eq!(replay.reserved_at_unix_ms(), a.reserved_at_unix_ms());
    assert_eq!(
        runtime
            .composer_load(request.scope(), request.composer_id())
            .await
            .unwrap(),
        *newer.draft()
    );
    let mut changed = request.clone();
    changed.expected_revision = newer.draft().revision();
    assert_eq!(
        runtime.submission_reserve(&changed).await.unwrap_err(),
        E::IdempotencyConflict
    );
    changed.command_id = SubmissionCommandId::generate().unwrap();
    let fresh = runtime.submission_reserve(&changed).await.unwrap();
    assert_eq!(fresh.captured(), newer.draft());
    let heads = runtime
        .client
        .storage()
        .unwrap()
        .authored_draft_heads(*request.scope.author().as_bytes(), 20)
        .await
        .unwrap();
    assert_eq!(heads.len(), 3);
    assert!(heads.iter().all(|draft| draft.operation_id().is_none()));
    assert_eq!(
        runtime
            .composer_list(request.scope(), 10, None)
            .await
            .unwrap()
            .entries()
            .len(),
        1
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_command_in_another_account_is_isolated_and_an_ownerless_runtime_rejects_it() {
    let root = tempfile::tempdir().unwrap();
    let request = request();
    let runtime = TeraRuntime::test_memory().unwrap();
    assert_eq!(
        runtime.submission_reserve(&request).await.unwrap_err(),
        E::Source(SourceError::OwnerUnavailable)
    );
    runtime.shutdown().await.unwrap();
    let mut receipts = Vec::new();
    for author in [AUTHOR, OTHER] {
        let runtime = RuntimeBuilder::new(config(root.path(), author, 1))
            .build()
            .await
            .unwrap();
        let mut request = request.clone();
        request.scope = scope(author, "nearby");
        runtime
            .composer_create(
                request.scope(),
                request.composer_id(),
                ComposerEditSequence::INITIAL,
                form("identical input"),
            )
            .await
            .unwrap();
        receipts.push(runtime.submission_reserve(&request).await.unwrap());
        runtime.shutdown().await.unwrap();
    }
    assert_ne!(receipts[0].reservation_id(), receipts[1].reservation_id());
    assert_eq!(receipts[0].captured().form(), receipts[1].captured().form());
}
