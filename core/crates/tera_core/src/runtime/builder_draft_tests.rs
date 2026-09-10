use super::RuntimeBuilder;
use crate::runtime::store::{MobileUserStoreConfig, ProtectedDataAvailability};
use radroots_storage::{
    Error,
    authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage, AuthoredDraftStore},
    authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord, AuthoredDraftScope},
};

// This is an opaque storage-consumption fixture, not the product composer schema.
const SCHEMA: &str = "fixture.tera.storage-adoption.v1";
const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[tokio::test]
async fn scoped_drafts_reopen_through_the_existing_sdk_database_owner() {
    let root = tempfile::tempdir().unwrap();
    let config = MobileUserStoreConfig::from_encoded(
        root.path(),
        AUTHOR,
        &"02".repeat(32),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    assert_eq!(
        config.owner_directory(),
        root.path().join("radroots/users").join(AUTHOR)
    );
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    let author: [u8; 32] = hex::decode(AUTHOR).unwrap().try_into().unwrap();
    let scope = AuthoredDraftScope::new([3; 32]).unwrap();
    let draft = AuthoredDraft::initial(
        AuthoredDraftId::new([4; 16]).unwrap(),
        author,
        SCHEMA,
        b"unfinished 1.\n2026-".to_vec(),
        AuthoredDraftStage::Draft,
        None,
        1_800_000_000_001,
    )
    .unwrap()
    .with_scope(scope)
    .unwrap();
    let store = runtime.client.storage().unwrap();
    AuthoredDraftStore::append_authored_draft(store, draft.clone(), None)
        .await
        .unwrap();
    let changed = draft
        .successor(
            b"unfinished 1.2\n2026-09-".to_vec(),
            AuthoredDraftStage::Draft,
            None,
            1_800_000_000_002,
        )
        .unwrap();
    AuthoredDraftStore::append_authored_draft(store, changed.clone(), Some(draft.revision()))
        .await
        .unwrap();
    let stale = draft
        .successor(
            b"a different stale edit".to_vec(),
            AuthoredDraftStage::Draft,
            None,
            1_800_000_000_003,
        )
        .unwrap();
    assert_eq!(
        AuthoredDraftStore::append_authored_draft(store, stale, Some(draft.revision())).await,
        Err(Error::DraftRevisionConflict)
    );
    let query = AuthoredDraftQuery::new(author, SCHEMA, Some(scope), 1).unwrap();
    let page = AuthoredDraftStore::query_authored_drafts(store, query.clone())
        .await
        .unwrap();
    assert_eq!(
        page.records(),
        [AuthoredDraftQueryRecord::Draft(changed.clone())]
    );
    assert!(page.next_cursor().is_none());
    runtime.shutdown().await.unwrap();
    drop(runtime);

    let reopened = RuntimeBuilder::new(config).build().await.unwrap();
    assert_eq!(
        reopened.sdk_storage_status().await.unwrap().backend,
        "sqlite"
    );
    let store = reopened.client.storage().unwrap();
    let page = AuthoredDraftStore::query_authored_drafts(store, query)
        .await
        .unwrap();
    assert_eq!(page.records(), [AuthoredDraftQueryRecord::Draft(changed)]);
    assert!(page.next_cursor().is_none());
    for (schema, selected_scope) in [(SCHEMA, None), ("fixture.other.v1", Some(scope))] {
        let query = AuthoredDraftQuery::new(author, schema, selected_scope, 1).unwrap();
        assert!(
            AuthoredDraftStore::query_authored_drafts(store, query)
                .await
                .unwrap()
                .records()
                .is_empty()
        );
    }
    reopened.shutdown().await.unwrap();
}
