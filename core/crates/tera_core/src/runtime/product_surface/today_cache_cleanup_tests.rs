use super::*;

#[tokio::test]
async fn capacity_cleanup_preserves_shared_files_then_collects_last_owner_without_touching_pending_work()
 {
    for sqlite in [false, true] {
        let f = Fixture::backend(sqlite).await;
        f.file().await;
        f.state(1, true).await;
        f.state(2, true).await;
        let pending = f.root.path().join("pending-receipt");
        std::fs::write(&pending, b"unresolved upload").unwrap();
        let first = f
            .runtime
            .phase1_cleanup_media_cache(&context(None, 1))
            .await
            .unwrap();
        assert_eq!(first.invalidated_entries, 1);
        assert_eq!(first.retained_candidates, 1);
        assert_eq!(first.remaining_entries, 0);
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        let second = f
            .runtime
            .phase1_cleanup_media_cache(&context(None, 2))
            .await
            .unwrap();
        assert_eq!(second.retained_candidates, 0);
        assert!(!f.path().exists());
        assert_eq!(std::fs::read(pending).unwrap(), b"unresolved upload");
        let replay = f
            .runtime
            .phase1_cleanup_media_cache(&context(None, 2))
            .await
            .unwrap();
        assert_eq!(replay.invalidated_entries, 0);
        f.runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn capacity_cleanup_retains_files_under_unknown_ownership_or_unsettled_writes() {
    for dirty in [false, true] {
        let f = Fixture::new().await;
        f.file().await;
        f.state(1, true).await;
        if dirty {
            drop(f.runtime.today_projection_lock.begin_write());
        } else {
            ProjectionStore::put_projection_document(
                f.runtime.client.storage().unwrap(),
                projection_id().unwrap(),
                projection_generation().unwrap(),
                ProjectionDocument::new("unknown".into(), b"{}".to_vec()).unwrap(),
            )
            .await
            .unwrap();
        }
        let result = f
            .runtime
            .phase1_cleanup_media_cache(&context(None, 1))
            .await
            .unwrap();
        assert_eq!(result.retained_candidates, 1);
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
    }
}

#[tokio::test]
async fn capacity_cleanup_exposes_remaining_entries_after_one_bounded_batch() {
    let f = Fixture::new().await;
    let selected = context(None, 1);
    let mut state = f.empty.clone();
    for i in 0..65 {
        let bytes = format!("cache item {i}").into_bytes();
        let hash = BlobHash::digest(&bytes).to_hex();
        let url = format!("https://media.example/{hash}.png");
        let reference = Phase1StructuralMediaReference::new(
            &url,
            Some(hash),
            Some("image/png".into()),
            Some(2),
            Some(3),
            Some(bytes.len() as u64),
            None,
        )
        .unwrap();
        let receipt = Phase1VerifiedMediaReceipt::from_commitment(
            &reference,
            BlobUrl::parse(&url).unwrap(),
            &ByteCommitment::from_bytes(&bytes, MediaType::parse("image/png").unwrap()),
            2,
            3,
            Phase1MediaConfigurationFingerprint::new([9; 32]).unwrap(),
            10,
        )
        .unwrap();
        state
            .media_cache
            .admit(&receipt, Phase1MediaCachePolicy::default(), 11)
            .unwrap();
    }
    persist_media_state(
        &f.runtime,
        f.runtime.client.storage().unwrap(),
        &selected,
        projection_generation().unwrap(),
        &mut state,
    )
    .await
    .unwrap();
    let result = f
        .runtime
        .phase1_cleanup_media_cache(&selected)
        .await
        .unwrap();
    assert_eq!(result.invalidated_entries, 64);
    assert_eq!(result.remaining_entries, 1);
    assert_eq!(
        f.runtime
            .phase1_cleanup_media_cache(&selected)
            .await
            .unwrap()
            .invalidated_entries,
        1
    );
}
