//! Actual SQLite receipt rejection/cancellation and settled offline recovery.
use super::super::super::performance_tests::counted_storage::{CountedStorage, QueryCounts};
use super::*;

#[tokio::test]
async fn rejected_receipt_write_preserves_uncertain_file_until_shutdown_and_reopen() {
    for after in [false, true] {
        let f = fixture().await;
        let (runtime, counts) = counted_runtime(f.runtime);
        counts.faults.failure("document_write", 2, after);
        let (started, entered) = oneshot::channel();
        let (resume, wait) = oneshot::channel();
        let expected = f.bytes.clone();
        let server = tokio::spawn(serve(f.listener, f.bytes, started, wait));
        resume.send(()).unwrap();
        assert!(
            retrieve(&runtime, &f.selected, f.fingerprint)
                .await
                .is_err()
        );
        entered.await.unwrap();
        server.await.unwrap();
        let directory = runtime.inbound_media_directory.as_ref().unwrap().clone();
        let artifact = directory.join(format!("{}.png", BlobHash::digest(&expected).to_hex()));
        assert_eq!(std::fs::read(&artifact).unwrap(), expected);
        let report = runtime
            .phase1_cleanup_media_cache(&f.selected)
            .await
            .unwrap();
        assert_eq!(report.recovered_orphan_files, 0);
        assert!(report.scan_pending);
        assert!(
            artifact.exists(),
            "unacknowledged ownership must preserve custody"
        );
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = reopen(f._root.path()).await;
        let report = reopened
            .phase1_cleanup_media_cache(&f.selected)
            .await
            .unwrap();
        assert_eq!(
            report.recovered_orphan_files + report.invalidated_entries,
            1
        );
        assert!(!artifact.exists());
        reopened.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn dropped_receipt_future_preserves_custody_before_and_after_provider_acknowledgement() {
    for after in [false, true] {
        let f = fixture().await;
        let (runtime, counts) = counted_runtime(f.runtime);
        let pause = counts.faults.pause("document_write", 2, after);
        let (started, entered) = oneshot::channel();
        let (resume, wait) = oneshot::channel();
        let expected = f.bytes.clone();
        let server = tokio::spawn(serve(f.listener, f.bytes, started, wait));
        resume.send(()).unwrap();
        let caller = {
            let runtime = runtime.clone();
            let selected = f.selected.clone();
            tokio::spawn(async move { retrieve(&runtime, &selected, f.fingerprint).await })
        };
        tokio::time::timeout(Duration::from_secs(2), pause.entered.notified())
            .await
            .unwrap();
        let artifact = runtime
            .inbound_media_directory
            .as_ref()
            .unwrap()
            .join(format!("{}.png", BlobHash::digest(&expected).to_hex()));
        assert_eq!(std::fs::read(&artifact).unwrap(), expected);
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        entered.await.unwrap();
        server.await.unwrap();
        assert!(!runtime.today_projection_lock.can_collect());
        let report = runtime
            .phase1_cleanup_media_cache(&f.selected)
            .await
            .unwrap();
        assert_eq!(report.recovered_orphan_files, 0);
        assert!(artifact.exists());
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = reopen(f._root.path()).await;
        let report = reopened
            .phase1_cleanup_media_cache(&f.selected)
            .await
            .unwrap();
        assert_eq!(
            report.recovered_orphan_files + report.invalidated_entries,
            1
        );
        assert!(!artifact.exists());
        reopened.shutdown().await.unwrap();
    }
}

fn counted_runtime(runtime: Arc<TeraRuntime>) -> (Arc<TeraRuntime>, Arc<QueryCounts>) {
    let mut runtime = Arc::try_unwrap(runtime).unwrap_or_else(|_| panic!("no outstanding owner"));
    let counts = Arc::new(QueryCounts::default());
    let blossom = runtime.client.blossom().unwrap().cloned().unwrap();
    runtime.client = radroots_sdk::ClientBuilder::new()
        .storage(Arc::new(CountedStorage {
            client: runtime.client.clone(),
            counts: counts.clone(),
        }))
        .blossom(blossom)
        .build()
        .unwrap();
    (Arc::new(runtime), counts)
}

async fn reopen(root: &std::path::Path) -> TeraRuntime {
    let store = MobileUserStoreConfig::from_encoded(
        root,
        &keys().public_key().to_string(),
        &"86".repeat(32),
        NOW * 1_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    RuntimeBuilder::new(store).build().await.unwrap()
}
