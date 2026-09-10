use super::{tests::*, *};
use crate::runtime::{
    builder::RuntimeBuilder,
    product_surface::{
        CreateUpdate, Phase1AddCommand, Phase1CancellationPolicy, Phase1QueuePolicy,
        Phase1RelaySatisfaction, ProfileMetadataCommand,
    },
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_storage::{
    Error,
    authored_draft::AuthoredDraftStore,
    authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord},
};

#[tokio::test]
async fn existing_sdk_owner_reopens_mixed_records_without_rewriting_drafts_or_operations() {
    let root = tempfile::tempdir().unwrap();
    let config = MobileUserStoreConfig::from_encoded(
        root.path(),
        AUTHOR,
        &"02".repeat(32),
        NOW,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    assert_eq!(
        config.owner_directory(),
        root.path().join("radroots/users").join(AUTHOR)
    );
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    assert_eq!(
        runtime.sdk_storage_status().await.unwrap().backend,
        "sqlite"
    );
    let query = AuthoredDraftQuery::new(
        scope().author().into_bytes(),
        COMPOSER_PAYLOAD_SCHEMA,
        Some(ComposerStorageRecord::scope_digest(&scope()).unwrap()),
        20,
    )
    .unwrap();
    assert!(
        AuthoredDraftStore::query_authored_drafts(runtime.client.storage().unwrap(), query.clone())
            .await
            .unwrap()
            .records()
            .is_empty()
    );
    let mut originals = Vec::new();
    for id in [1, 2] {
        let saved = runtime
            .phase1_save_draft(
                [id; 16],
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("legacy draft fixture").unwrap()),
                NOW / 1000,
                Vec::new(),
                None,
                NOW,
            )
            .await
            .unwrap();
        originals.push(saved.draft().clone());
    }
    let queued = runtime
        .phase1_queue_draft(
            [2; 16],
            1,
            Phase1QueuePolicy::new(
                vec!["wss://relay.example".into()],
                Phase1RelaySatisfaction::AllAccepted,
                NOW + 60_000,
                Phase1CancellationPolicy::LocalCooperative,
            )
            .unwrap(),
            NOW + 1,
        )
        .await
        .unwrap();
    assert!(queued.draft().operation_id().is_some());
    let operation = queued.push().unwrap().clone();
    originals.push(queued.draft().clone());
    let profile = runtime
        .phase1_save_profile_metadata(
            ProfileMetadataCommand::new(
                "grower".into(),
                Some("Local Grower".into()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    originals.push(profile.draft().clone());
    let good = record(10);
    AuthoredDraftStore::append_authored_draft(
        runtime.client.storage().unwrap(),
        good.stored().clone(),
        None,
    )
    .await
    .unwrap();
    runtime.shutdown().await.unwrap();
    drop(runtime);

    // Opening existing mixed state uses the same owner, schema and namespace.
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    let store = runtime.client.storage().unwrap();
    let page = AuthoredDraftStore::query_authored_drafts(store, query.clone())
        .await
        .unwrap();
    assert_eq!(
        page.records(),
        [AuthoredDraftQueryRecord::Draft(good.stored().clone())]
    );
    let restored = ComposerStorageRecord::decode(good.stored().clone(), &scope()).unwrap();
    assert_eq!(restored.draft(), good.draft());

    // An ID collision must not replace the active strict journal head.
    assert_eq!(
        AuthoredDraftStore::append_authored_draft(store, record(1).into_stored(), None).await,
        Err(Error::DraftRevisionConflict)
    );
    let mut future: serde_json::Value = serde_json::from_slice(good.stored().payload()).unwrap();
    future["schema_sha256"] = serde_json::json!("injected incompatible schema checksum");
    let unknown = with_payload(11, serde_json::to_vec(&future).unwrap());
    let corrupt = with_payload(12, b"{injected incomplete conversion".to_vec());
    for (draft, expected) in [
        (&unknown, ComposerStorageError::UnsupportedSchema),
        (&corrupt, ComposerStorageError::CorruptRecord),
    ] {
        AuthoredDraftStore::append_authored_draft(store, draft.clone(), None)
            .await
            .unwrap();
        let persisted = AuthoredDraftStore::authored_draft_head(store, draft.draft_id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            ComposerStorageRecord::decode(persisted, &scope()).unwrap_err(),
            expected
        );
        assert_eq!(
            AuthoredDraftStore::authored_draft_head(store, draft.draft_id())
                .await
                .unwrap()
                .as_ref(),
            Some(draft)
        );
    }
    runtime.shutdown().await.unwrap();
    drop(runtime);

    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let store = runtime.client.storage().unwrap();
    for original in &originals {
        assert_eq!(
            AuthoredDraftStore::authored_draft_revision(
                store,
                original.draft_id(),
                original.revision()
            )
            .await
            .unwrap()
            .as_ref(),
            Some(original)
        );
    }
    for original in [&originals[0], &originals[2], &originals[3]] {
        assert_eq!(
            AuthoredDraftStore::authored_draft_head(store, original.draft_id())
                .await
                .unwrap()
                .as_ref(),
            Some(original)
        );
    }
    assert_eq!(
        runtime.phase1_draft_status([2; 16]).await.unwrap().push(),
        Some(&operation)
    );
    assert_eq!(
        runtime.phase1_draft_status([2; 16]).await.unwrap().draft(),
        queued.draft()
    );
    assert_eq!(
        runtime
            .phase1_profile_status(*profile.draft().draft_id().as_bytes())
            .await
            .unwrap()
            .draft(),
        profile.draft()
    );
    let page = AuthoredDraftStore::query_authored_drafts(store, query)
        .await
        .unwrap();
    assert_eq!(
        page.records(),
        [good.into_stored(), unknown, corrupt].map(AuthoredDraftQueryRecord::Draft)
    );
    assert!(page.next_cursor().is_none());
    runtime.shutdown().await.unwrap();
}
