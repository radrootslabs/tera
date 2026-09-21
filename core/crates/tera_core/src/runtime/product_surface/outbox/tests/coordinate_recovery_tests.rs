use super::{coordinate_support::*, *};
use crate::runtime::product_surface::{coordinate, recovery_inventory::RecoveryOwner};
use radroots_storage::authored_draft_query::AuthoredDraftQuery;
use std::{future::Future, task::Poll};

#[tokio::test]
async fn coordinate_cancelled_preparation_recovers_both_or_neither_ownership_records() {
    let mut interrupted = 0;
    for stop_after in 0..32 {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        std::fs::create_dir_all(config.owner_directory()).unwrap();
        let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
        let prior = signed_head(SECRET, 31_923, "cancel:prepare", 1_700_000_000);
        retain(&runtime, prior.clone()).await;
        let mut pending = 0;
        let mut future =
            Box::pin(runtime.prepare_revision_intent([171; 16], intent(&prior, "Captured")));
        let completed = std::future::poll_fn(|cx| match future.as_mut().poll(cx) {
            Poll::Ready(result) => Poll::Ready(Some(result)),
            Poll::Pending if pending == stop_after => Poll::Ready(None),
            Poll::Pending => {
                pending += 1;
                Poll::Pending
            }
        })
        .await;
        drop(future);
        if let Some(result) = completed {
            result.unwrap();
        } else {
            interrupted += 1;
        }
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let runtime = RuntimeBuilder::new(config).build().await.unwrap();
        let store = runtime.client.storage().unwrap();
        let author = PublicKey::from_hex(AUTHOR).unwrap().into_bytes();
        let mut counts = Vec::new();
        for schema in [coordinate::CLAIM_SCHEMA, coordinate::BINDING_SCHEMA] {
            let query = AuthoredDraftQuery::new(author, schema, None, 10).unwrap();
            let rows = store.query_authored_drafts(query).await.unwrap();
            counts.push(rows.records().len());
        }
        assert_eq!(
            counts[0], counts[1],
            "cancel at Pending boundary {stop_after}"
        );
        assert!(counts[0] <= 1);
        let recovered = runtime
            .prepare_revision_intent([171; 16], intent(&prior, "Captured"))
            .await
            .unwrap();
        let id = *recovered.replacement().draft().draft_id().as_bytes();
        let queued = queue(&runtime, recovered.replacement()).await.unwrap();
        assert!(queued.coordinate_captured());
        assert!(queued.coordinate_writable());
        let replay = runtime
            .prepare_revision_intent([171; 16], intent(&prior, "Captured"))
            .await
            .unwrap();
        assert_eq!(replay.replacement().draft().draft_id().as_bytes(), &id);
        assert_eq!(replay.replacement().draft(), queued.draft());
        assert!(
            replay
                .replacement()
                .push()
                .unwrap()
                .artifact()
                .signed()
                .is_none()
        );
        assert_eq!(runtime.phase1_draft_heads(100).await.unwrap().len(), 1);
        runtime.shutdown().await.unwrap();
    }
    assert!(interrupted > 0, "must cancel real pending SQLite work");
}

#[tokio::test]
async fn coordinate_corrupt_metadata_holds_effects_and_blocks_destructive_media_inventory() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let first = saved(&runtime, [181; 16], "corrupt:claim").await;
    let queued = queue(&runtime, &first).await.unwrap();
    let intent = coordinate::intent_from_draft(queued.draft())
        .unwrap()
        .unwrap();
    let store = runtime.client.storage().unwrap();
    let claim = store
        .authored_draft_head(intent.claim_id().unwrap())
        .await
        .unwrap()
        .unwrap();
    let broken = claim
        .successor(
            b"{}".to_vec(),
            AuthoredDraftStage::Draft,
            None,
            claim.updated_at_unix_ms() + 1,
        )
        .unwrap();
    store
        .append_authored_draft(broken, Some(claim.revision()))
        .await
        .unwrap();
    assert_eq!(
        runtime
            .phase1_sign_queued_draft([181; 16], queued.draft().revision().get())
            .await
            .unwrap_err(),
        Phase1DraftError::Corrupt
    );
    let page = runtime.recovery_page(64, None).await.unwrap();
    assert!(
        page.entries
            .iter()
            .any(|entry| entry.key == *claim.draft_id().as_bytes()
                && matches!(entry.owner, RecoveryOwner::Repair(_)))
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    assert!(
        crate::runtime::product_surface::media_gc::inspect_media_references(root.path(), vec![])
            .await
            .is_err()
    );
}
