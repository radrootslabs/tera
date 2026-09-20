use super::*;

#[tokio::test]
async fn uncertain_database_write_retains_files_even_after_later_success() {
    for sqlite in [false, true] {
        let f = Fixture::backend(sqlite).await;
        f.file().await;
        // Model backend work whose caller was cancelled before acknowledgement.
        // The owned file can still be referenced by that late completion.
        let projection = f.runtime.today_projection_lock.lock().await;
        let write = f.runtime.today_projection_lock.begin_write();
        drop(projection);
        f.collect().await.unwrap();
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        drop(write);
        f.collect().await.unwrap();
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        f.state(2, true).await; // A late successful backend completion.
        f.state(2, false).await; // Ordinary later writes cannot clear uncertainty.
        assert!(
            f.candidates()
                .await
                .unwrap()
                .contains(&f.receipt.artifact_id())
        );
        f.collect().await.unwrap();
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        f.runtime.shutdown().await.unwrap();
    }
}

#[test]
fn collection_cannot_dispatch_file_work_beyond_its_mutation_fence() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
        time::Duration,
    };

    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let f = executor.block_on(Fixture::new());
    executor.block_on(f.file());
    let (ready, started) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let worker = executor.spawn_blocking(move || {
        ready.send(()).unwrap();
        blocked.recv().unwrap();
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed = {
        let _entered = executor.enter();
        let mut future = Box::pin(f.collect());
        matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(()))
        )
        // Dropping the caller must not leave dispatched filesystem mutations
        // that could race the next owner after these guards are released.
    };
    std::fs::write(f.path(), &f.bytes).unwrap();
    executor.block_on(f.state(2, true));
    release.send(()).unwrap();
    executor.block_on(worker).unwrap();
    executor.block_on(f.runtime.shutdown()).unwrap();
    executor.shutdown_timeout(Duration::from_secs(5));
    assert!(
        completed,
        "physical collection escaped its synchronous fence"
    );
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
}

#[tokio::test]
async fn payload_budget_retains_bytes_before_decoding_another_document() {
    let f = Fixture::new().await;
    let mut state = f.empty.clone();
    // These opaque source IDs remain bounded by the storage document limit.
    // Their contents are irrelevant to the cache ownership being inspected.
    state.quarantined_source_ids = vec!["a".repeat(14 * 1024 * 1024)];
    for generation in 2..=6 {
        state.context_generation = generation;
        persist_media_state(
            &f.runtime,
            f.runtime.client.storage().unwrap(),
            &context(None, generation),
            projection_generation().unwrap(),
            &mut state,
        )
        .await
        .unwrap();
    }
    f.file().await;
    assert!(f.candidates().await.is_none());
    f.collect().await.unwrap();
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
}

#[tokio::test]
async fn quota_eviction_in_one_context_preserves_another_contexts_bytes() {
    let f = Fixture::new().await;
    f.file().await;
    f.state(1, true).await;
    f.state(2, true).await;
    let bytes = b"different cached content";
    let hash = BlobHash::digest(bytes).to_hex();
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
        &ByteCommitment::from_bytes(bytes, MediaType::parse("image/png").unwrap()),
        2,
        3,
        f.receipt.configuration(),
        12,
    )
    .unwrap();
    let files = f.runtime.inbound_media_lock.lock().await;
    let projection = f.runtime.today_projection_lock.lock().await;
    let storage = f.runtime.client.storage().unwrap();
    let selected = context(None, 1);
    let mut state = load_state(storage, &selected, projection_generation().unwrap())
        .await
        .unwrap()
        .unwrap();
    let evicted = state
        .media_cache
        .admit(&receipt, Phase1MediaCachePolicy::new(1_024, 1).unwrap(), 12)
        .unwrap();
    assert_eq!(evicted, vec![f.receipt.artifact_id()]);
    persist_media_state(
        &f.runtime,
        storage,
        &selected,
        projection_generation().unwrap(),
        &mut state,
    )
    .await
    .unwrap();
    collect(&f.runtime, f.directory(), &evicted, &files, &projection)
        .await
        .unwrap();
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
    assert_eq!(
        f.runtime
            .phase1_media_cache_status(&context(None, 2))
            .await
            .unwrap()
            .artifacts,
        1
    );
}
