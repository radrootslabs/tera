//! Actual caller custody and cross-context orphan recovery boundaries.
use super::*;

#[test]
fn inbound_file_write_finishes_inside_the_caller_poll_with_blocking_pool_occupied() {
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
    let (ready, started) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let worker = executor.spawn_blocking(move || {
        ready.send(()).unwrap();
        blocked.recv().unwrap();
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let completed = {
        let _entered = executor.enter();
        let _files = executor.block_on(f.runtime.inbound_media_lock.lock());
        let mut future = Box::pin(super::super::super::super::media::write_verified_artifact(
            f.directory(),
            &f.receipt,
            &f.bytes,
        ));
        matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(_))
        )
        // Dropping the future and file guard must leave no dispatched mutation.
    };
    std::fs::create_dir_all(f.directory()).unwrap();
    std::fs::write(f.path(), b"new current owner").unwrap();
    release.send(()).unwrap();
    executor.block_on(worker).unwrap();
    executor.block_on(f.runtime.shutdown()).unwrap();
    executor.shutdown_timeout(Duration::from_secs(5));
    assert!(
        completed,
        "write/link/sync must stay inside the caller's file fence"
    );
    assert_eq!(std::fs::read(f.path()).unwrap(), b"new current owner");
    assert_eq!(std::fs::read_dir(f.directory()).unwrap().count(), 1);
}

#[tokio::test]
async fn cleanup_recovers_unindexed_files_only_after_every_context_releases_ownership() {
    for sqlite in [false, true] {
        let f = Fixture::backend(sqlite).await;
        f.file().await;
        f.state(2, true).await;
        f.runtime
            .phase1_cleanup_media_cache(&context(None, 1))
            .await
            .unwrap();
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        f.state(2, false).await;
        f.runtime
            .phase1_cleanup_media_cache(&context(None, 1))
            .await
            .unwrap();
        assert!(
            !f.path().exists(),
            "unindexed bytes must be discovered after final owner leaves"
        );
        f.runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cleanup_recovers_owned_temporary_names_and_preserves_unknown_and_pending_files() {
    for sqlite in [false, true] {
        let f = Fixture::backend(sqlite).await;
        f.file().await;
        let temporary = f.directory().join(format!(
            ".{}.{}.tmp",
            f.receipt.artifact_id().to_hex(),
            "ab".repeat(16)
        ));
        std::fs::write(&temporary, b"interrupted write").unwrap();
        let unknown = f.directory().join("pending-receipt");
        let malformed = f
            .directory()
            .join(format!(".{}.invalid.tmp", f.receipt.artifact_id().to_hex()));
        let outside = f.root.path().join("authored-pending");
        for path in [&unknown, &malformed, &outside] {
            std::fs::write(path, b"preserve me").unwrap();
        }
        let result = f
            .runtime
            .phase1_cleanup_media_cache(&context(None, 1))
            .await
            .unwrap();
        assert_eq!(result.recovered_orphan_files, 2);
        assert!(!result.scan_pending);
        assert!(!temporary.exists());
        assert!(!f.path().exists());
        for path in [&unknown, &malformed, &outside] {
            assert_eq!(std::fs::read(path).unwrap(), b"preserve me");
        }
        f.runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn orphan_recovery_retains_bytes_when_ownership_is_unknown_or_write_is_unsettled() {
    for uncertain in [false, true] {
        let f = Fixture::backend(true).await;
        f.file().await;
        if uncertain {
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
        assert_eq!(result.recovered_orphan_files, 0);
        assert!(result.scan_pending);
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        f.runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn verified_sidecar_receipt_without_cache_index_retains_uncertain_file() {
    let f = Fixture::backend(true).await;
    f.file().await;
    f.state(2, true).await;
    let storage = f.runtime.client.storage().unwrap();
    let selected = context(None, 2);
    let state = load_state(storage, &selected, projection_generation().unwrap())
        .await
        .unwrap()
        .unwrap();
    let mut local = sidecar::LocalState::from_state(&state).unwrap();
    local.cache = Phase1MediaCacheIndex::default();
    local.media = vec![(
        [7; 32],
        Phase1InboundMediaState::Verified(Box::new(f.receipt.clone())),
    )];
    ProjectionStore::put_projection_document(
        storage,
        ProjectionId::parse(sidecar::ID).unwrap(),
        projection_generation().unwrap(),
        ProjectionDocument::new(projection_document_key(&selected), encode(&local).unwrap())
            .unwrap(),
    )
    .await
    .unwrap();
    let report = f
        .runtime
        .phase1_cleanup_media_cache(&context(None, 1))
        .await
        .unwrap();
    assert_eq!(report.recovered_orphan_files, 0);
    assert!(report.scan_pending);
    assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn bounded_directory_continuation_reaches_orphans_beyond_retained_entries() {
    let f = Fixture::backend(true).await;
    f.file().await;
    f.state(2, true).await;
    let mut orphans = Vec::new();
    for i in 0..145 {
        let path = f.directory().join(format!(
            "{}.png",
            BlobHash::digest(format!("orphan {i}").as_bytes()).to_hex()
        ));
        std::fs::write(&path, b"unindexed file").unwrap();
        orphans.push(path);
    }
    let mut recovered = 0;
    let mut passes = 0;
    loop {
        let report = f
            .runtime
            .phase1_cleanup_media_cache(&context(None, 1))
            .await
            .unwrap();
        assert!(report.recovered_orphan_files <= MAX_CANDIDATES as u32);
        recovered += report.recovered_orphan_files;
        passes += 1;
        assert_eq!(std::fs::read(f.path()).unwrap(), f.bytes);
        if !report.scan_pending {
            break;
        }
        assert!(
            passes <= 4,
            "a bounded cursor must make progress past retained entries"
        );
    }
    assert_eq!(recovered, 145);
    assert!(passes >= 3);
    assert!(orphans.iter().all(|path| !path.exists()));
    f.runtime.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn root_replacement_invalidates_inventory_and_symlinks_never_grant_deletion() {
    let f = Fixture::backend(true).await;
    f.file().await;
    f.state(2, true).await;
    let old_directory = f.root.path().join("old-inbound");
    let batch = f
        .runtime
        .inbound_media_lock
        .lock()
        .await
        .inventory(f.directory(), 1)
        .unwrap();
    std::fs::rename(f.directory(), &old_directory).unwrap();
    std::fs::create_dir(f.directory()).unwrap();
    assert_eq!(
        batch.confirm_root(f.directory()),
        Err(Phase1InboundMediaError::CorruptArtifact)
    );
    let hash = BlobHash::digest(b"new root orphan").to_hex();
    let new_orphan = f.directory().join(format!("{hash}.png"));
    std::fs::write(&new_orphan, b"new root orphan").unwrap();
    let outside = f.root.path().join("outside-photo");
    std::fs::write(&outside, b"authored photo").unwrap();
    let link = f.directory().join(format!(
        "{}.jpg",
        BlobHash::digest(b"outside-photo").to_hex()
    ));
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let report = f
        .runtime
        .phase1_cleanup_media_cache(&context(None, 1))
        .await
        .unwrap();
    assert_eq!(report.recovered_orphan_files, 1);
    assert!(!report.scan_pending);
    assert!(!new_orphan.exists());
    assert_eq!(std::fs::read(&outside).unwrap(), b"authored photo");
    assert!(
        std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(old_directory.join(f.path().file_name().unwrap())).unwrap(),
        f.bytes
    );
    f.runtime.shutdown().await.unwrap();
}
