use super::{
    tests::{AUTHOR, form, scope},
    *,
};
use crate::runtime::product_surface::LocalNetworkId;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_identity::PublicKey;
use radroots_storage::authored_draft::{AuthoredDraftStage, AuthoredDraftStore};

#[tokio::test]
async fn concurrent_sdk_sqlite_saves_keep_one_revision_winner_and_reopen_exactly() {
    let root = tempfile::tempdir().unwrap();
    let config = MobileUserStoreConfig::from_encoded(
        root.path(),
        AUTHOR,
        &"02".repeat(32),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    let selected = scope();
    let id = ComposerId::generate().unwrap();
    let initial = runtime
        .composer_create(
            &selected,
            id,
            ComposerEditSequence::INITIAL,
            form("\n unfinished café "),
        )
        .await
        .unwrap();
    assert_eq!(initial.draft().id(), id);
    assert_eq!(initial.draft().scope(), &selected);
    let (left, right) = tokio::join!(
        runtime.composer_save(
            &selected,
            id,
            ComposerRevision::INITIAL,
            ComposerEditSequence::new(2).unwrap(),
            form("left")
        ),
        runtime.composer_save(
            &selected,
            id,
            ComposerRevision::INITIAL,
            ComposerEditSequence::new(3).unwrap(),
            form("right")
        ),
    );
    let outcomes = [left, right];
    assert_eq!(outcomes.iter().filter(|value| value.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|value| matches!(value, Err(ComposerPersistenceError::RevisionConflict)))
            .count(),
        1
    );
    let winner = outcomes.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(winner.draft().revision().get(), 2);
    assert_eq!(
        runtime.composer_load(&selected, id).await.unwrap(),
        *winner.draft()
    );
    let saved = runtime
        .composer_save(
            &selected,
            id,
            winner.draft().revision(),
            ComposerEditSequence::new(10).unwrap(),
            form("newest 1.\n2026-"),
        )
        .await
        .unwrap();
    assert_eq!(saved.draft().revision().get(), 3);
    assert_eq!(saved.draft().edit_sequence().get(), 10);
    let foreign = ComposerScope::new(
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap(),
        selected.local_network().clone(),
    );
    let other_context = ComposerScope::new(
        selected.author(),
        LocalNetworkId::new("other".into()).unwrap(),
    );
    for other in [&foreign, &other_context] {
        assert_eq!(
            runtime.composer_load(other, id).await.unwrap_err(),
            ComposerPersistenceError::ScopeMismatch
        );
        assert_eq!(
            runtime
                .composer_save(
                    other,
                    id,
                    saved.draft().revision(),
                    ComposerEditSequence::new(11).unwrap(),
                    form("foreign")
                )
                .await
                .unwrap_err(),
            ComposerPersistenceError::ScopeMismatch
        );
    }
    let uncreated = ComposerId::generate().unwrap();
    assert_eq!(
        runtime
            .composer_create(
                &foreign,
                uncreated,
                ComposerEditSequence::INITIAL,
                form("foreign")
            )
            .await
            .unwrap_err(),
        ComposerPersistenceError::ScopeMismatch
    );
    assert_eq!(
        runtime
            .composer_load(&selected, uncreated)
            .await
            .unwrap_err(),
        ComposerPersistenceError::NotFound
    );
    let stored = AuthoredDraftStore::authored_draft_head(
        runtime.client.storage().unwrap(),
        AuthoredDraftId::new(*id.as_bytes()).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(stored.stage(), AuthoredDraftStage::Draft);
    assert!(stored.operation_id().is_none());
    runtime.shutdown().await.unwrap();
    assert_eq!(
        runtime.composer_load(&selected, id).await.unwrap_err(),
        ComposerPersistenceError::Lifecycle(RuntimeLifecycleError::Closed)
    );
    assert_eq!(
        runtime
            .composer_create(
                &selected,
                uncreated,
                ComposerEditSequence::INITIAL,
                form("closed")
            )
            .await
            .unwrap_err(),
        ComposerPersistenceError::Lifecycle(RuntimeLifecycleError::Closed)
    );
    assert_eq!(
        runtime
            .composer_save(
                &selected,
                id,
                saved.draft().revision(),
                ComposerEditSequence::new(11).unwrap(),
                form("closed")
            )
            .await
            .unwrap_err(),
        ComposerPersistenceError::Lifecycle(RuntimeLifecycleError::Closed)
    );
    drop(runtime);
    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    assert_eq!(
        runtime.sdk_storage_status().await.unwrap().backend,
        "sqlite"
    );
    assert_eq!(
        runtime.composer_load(&selected, id).await.unwrap(),
        *saved.draft()
    );
    let store = runtime.client.storage().unwrap();
    assert_eq!(
        AuthoredDraftStore::authored_draft_head(store, stored.draft_id())
            .await
            .unwrap()
            .as_ref(),
        Some(&stored)
    );
    let original = AuthoredDraftStore::authored_draft_revision(
        store,
        stored.draft_id(),
        radroots_storage::authored_draft::AuthoredDraftRevision::INITIAL,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        ComposerStorageRecord::decode(original, &selected)
            .unwrap()
            .draft(),
        initial.draft()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_runtime_owner_cannot_create_read_or_save_a_composer() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = scope();
    let id = ComposerId::generate().unwrap();
    assert_eq!(
        runtime
            .composer_create(
                &selected,
                id,
                ComposerEditSequence::INITIAL,
                form("missing owner")
            )
            .await
            .unwrap_err(),
        ComposerPersistenceError::OwnerUnavailable
    );
    assert_eq!(
        runtime.composer_load(&selected, id).await.unwrap_err(),
        ComposerPersistenceError::OwnerUnavailable
    );
    assert_eq!(
        runtime
            .composer_save(
                &selected,
                id,
                ComposerRevision::INITIAL,
                ComposerEditSequence::new(2).unwrap(),
                form("missing owner")
            )
            .await
            .unwrap_err(),
        ComposerPersistenceError::OwnerUnavailable
    );
    runtime.shutdown().await.unwrap();
}
