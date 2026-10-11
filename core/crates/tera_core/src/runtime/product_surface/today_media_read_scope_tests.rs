//! Actual post-file and acknowledged local-write authorization boundaries.
use super::super::super::performance_tests::counted_storage::{CountedStorage, QueryCounts};
use super::*;

#[tokio::test]
async fn cached_media_after_file_read_and_acknowledged_touch_refuses_source_change() {
    for boundary in ["after_file", "after_touch"] {
        let f = fixture().await;
        let (started, entered) = oneshot::channel();
        let (resume, wait) = oneshot::channel();
        let server = tokio::spawn(serve(f.listener, f.bytes, started, wait));
        resume.send(()).unwrap();
        let artifact = retrieve(&f.runtime, &f.selected, f.fingerprint)
            .await
            .unwrap();
        entered.await.unwrap();
        server.await.unwrap();
        let mut runtime = Arc::try_unwrap(f.runtime)
            .unwrap_or_else(|_| panic!("fixture has no outstanding owner"));
        let counts = Arc::new(QueryCounts::default());
        // This read-only cached-artifact command needs no network SDK. Preserve
        // the actual owner/runtime and file cache, wrapping its real storage.
        runtime.client = radroots_sdk::ClientBuilder::new()
            .storage(Arc::new(CountedStorage {
                client: runtime.client.clone(),
                counts: counts.clone(),
            }))
            .build()
            .unwrap();
        let pause = if boundary == "after_file" {
            counts.faults.pause("read_query", 4, true)
        } else {
            counts.faults.pause("document_write", 1, true)
        };
        let runtime = Arc::new(runtime);
        let reader = {
            let runtime = runtime.clone();
            let selected = f.selected.clone();
            let id = artifact.artifact_id();
            tokio::spawn(async move {
                runtime
                    .phase1_verified_media_artifact(&selected, id, (NOW + 100) * 1_000)
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(2), pause.entered.notified())
            .await
            .unwrap();
        let receipt = EventStore::admit(
            runtime.client.storage().unwrap(),
            visible_admission(
                signed(5, vec![vec!["e", &f.source]], "", NOW + 4),
                (NOW + 4) * 1_000,
            ),
        )
        .await
        .unwrap();
        pause.resume.notify_one();
        assert!(
            reader.await.unwrap().is_err(),
            "{boundary} must not publish superseded render authority"
        );
        assert!(
            artifact.local_path().exists(),
            "An authorization refusal does not delete shared custody bytes"
        );
        let retained = EventStore::query_raw(
            runtime.client.storage().unwrap(),
            EventQuery::for_ids(
                EventQueryBounds::first(1).unwrap(),
                vec![*receipt.event_id()],
            )
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(retained.items()[0].position(), receipt.position());
        runtime.shutdown().await.unwrap();
    }
}
