use super::tests::{context, ingest, keys, signed};
use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
    visibility::AuthorVisibility,
};

const NOW: u64 = 2_000_000_000;

#[tokio::test]
async fn replaced_policy_at_same_revision_cannot_reuse_cached_authority() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let author = keys().public_key().to_string();
    let other = nostr::Keys::parse(&"02".repeat(32))
        .unwrap()
        .public_key()
        .to_string();
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "visible root", NOW),
        NOW + 1,
    )
    .await;
    let policy = runtime
        .set_author_visibility(&other, AuthorVisibility::Muted)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(100, NOW + 2, "UTC"))
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let mut document = serde_json::to_value(policy).unwrap();
    document["entries"] = serde_json::json!({author: "blocked"});
    let id = "tera.author_visibility.v1";
    ProjectionStore::put_projection_document(
        runtime.client.storage().unwrap(),
        ProjectionId::parse(id).unwrap(),
        ProjectionGeneration::new(Sha256::digest(id).into()).unwrap(),
        ProjectionDocument::new("policy".to_owned(), serde_json::to_vec(&document).unwrap())
            .unwrap(),
    )
    .await
    .unwrap();
    assert!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::first(100, NOW + 3, "UTC"))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn muted_reply_author_does_not_remove_an_allowed_root_or_its_profile() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let root = signed(1, vec![], "visible root", NOW);
    let root_id = root.id().to_hex();
    let reply = super::visibility_tests::sign_as_other_author(&signed(
        1,
        vec![vec!["e", &root_id, "", "root"]],
        "hidden reply",
        NOW + 1,
    ));
    let other_author = reply.envelope().author().to_hex();
    ingest(&runtime, &selected, root, NOW + 3).await;
    ingest(&runtime, &selected, reply, NOW + 3).await;
    ingest(
        &runtime,
        &selected,
        signed(0, vec![], r#"{"name":"visible profile"}"#, NOW),
        NOW + 3,
    )
    .await;
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(100, NOW + 4, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].thread.len(), 1);
    runtime
        .set_author_visibility(&other_author, AuthorVisibility::Muted)
        .await
        .unwrap();
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(100, NOW + 5, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].thread.is_empty());
    assert!(page.items[0].author_profile.is_some());
    runtime.shutdown().await.unwrap();
}

async fn open(root: &std::path::Path) -> TeraRuntime {
    open_for(root, &keys().public_key().to_string()).await
}

async fn open_for(root: &std::path::Path, author: &str) -> TeraRuntime {
    let store = MobileUserStoreConfig::from_encoded(
        root,
        author,
        &"18".repeat(32),
        NOW * 1000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(store.owner_directory()).unwrap();
    RuntimeBuilder::new(store).build().await.unwrap()
}

#[tokio::test]
async fn author_policy_revokes_feed_search_details_cursors_and_survives_restart() {
    for mode in [AuthorVisibility::Muted, AuthorVisibility::Blocked] {
        let root = tempfile::tempdir().unwrap();
        let runtime = open(root.path()).await;
        let selected = context(None, 1);
        let other_context = context(Some("Victoria"), 2);
        let author = keys().public_key().to_string();
        for index in 0..2 {
            ingest(
                &runtime,
                &selected,
                signed(1, vec![], &format!("visible post {index}"), NOW + index),
                NOW + 3,
            )
            .await;
        }
        ingest(
            &runtime,
            &selected,
            signed(0, vec![], r#"{"name":"visible profile"}"#, NOW),
            NOW + 3,
        )
        .await;
        let page = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(1, NOW + 4, "UTC"))
            .await
            .unwrap();
        let cursor = page.next_cursor.unwrap();
        let ids = vec![page.items[0].card.card_id.to_hex()];
        assert!(
            !runtime
                .phase1_search(&selected, "visible", 100, NOW + 4, "UTC")
                .await
                .unwrap()
                .is_empty()
        );
        runtime
            .phase1_today_page(&other_context, TodayPageRequest::first(100, NOW + 4, "UTC"))
            .await
            .unwrap();
        runtime.set_author_visibility(&author, mode).await.unwrap();
        assert!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::after(1, cursor.clone()))
                .await
                .is_err()
        );
        for context in [&selected, &other_context] {
            assert!(
                runtime
                    .phase1_today_page(context, TodayPageRequest::first(100, NOW + 5, "UTC"))
                    .await
                    .unwrap()
                    .items
                    .is_empty()
            );
            assert!(
                runtime
                    .phase1_search(context, "visible", 100, NOW + 5, "UTC")
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                runtime
                    .phase1_today_reconcile(context, NOW + 5, &ids, None, "UTC")
                    .await
                    .unwrap()
                    .items
                    .is_empty()
            );
            assert!(
                runtime
                    .phase1_me(context, &author, NOW + 5, "UTC")
                    .await
                    .unwrap()
                    .profile
                    .is_none()
            );
        }
        assert_eq!(
            EventStore::status(runtime.client.storage().unwrap())
                .await
                .unwrap()
                .raw_events(),
            3
        );
        runtime.shutdown().await.unwrap();
        let other = nostr::Keys::parse(&"02".repeat(32))
            .unwrap()
            .public_key()
            .to_string();
        let other_runtime = open_for(root.path(), &other).await;
        assert!(
            other_runtime
                .author_visibility()
                .await
                .unwrap()
                .entries()
                .is_empty()
        );
        other_runtime.shutdown().await.unwrap();
        let runtime = open(root.path()).await;
        assert_eq!(
            runtime
                .author_visibility()
                .await
                .unwrap()
                .entries()
                .get(&author),
            Some(&mode)
        );
        assert!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::first(100, NOW + 6, "UTC"))
                .await
                .unwrap()
                .items
                .is_empty()
        );
        runtime
            .set_author_visibility(&author, AuthorVisibility::Visible)
            .await
            .unwrap();
        assert!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::after(1, cursor))
                .await
                .is_err()
        );
        let restored = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(100, NOW + 7, "UTC"))
            .await
            .unwrap();
        assert_eq!(restored.items.len(), 2);
        assert_eq!(
            runtime
                .phase1_search(&selected, "visible", 100, NOW + 7, "UTC")
                .await
                .unwrap()
                .len(),
            3
        );
        runtime.shutdown().await.unwrap();
    }
}
