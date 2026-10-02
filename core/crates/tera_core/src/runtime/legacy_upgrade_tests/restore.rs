use super::{fixture::*, restore_host::Files, restore_source::*};
use crate::runtime::{backup::*, builder::RuntimeBuilder, product_surface::*, restore::*};
use std::sync::{Arc, atomic::Ordering};

#[tokio::test]
async fn actual_legacy_backup_restore_keeps_hidden_media_and_requires_fresh_original_target_review()
{
    let fixture = Fixture::copy();
    let config = fixture.config(true);
    let runtime = fixture.open(true).await;
    fixture.assert_revisions(&runtime).await;
    fixture.assert_statuses(&runtime).await;
    let files = Files::new(&fixture);
    let now = phase1_operation_now_unix_ms().unwrap();
    let backup = BackupRequest::new(
        [17; 16],
        config.public_key().into_bytes(),
        *config.source_generation().as_bytes(),
        now,
        128 * 1024 * 1024,
    )
    .unwrap();
    let manifest = runtime
        .capture_application_backup(backup.clone(), &files)
        .await
        .unwrap();
    assert_eq!(manifest.media().len(), 1);
    assert_eq!(
        manifest.media()[0].sha256,
        field(&fixture.host, "media_sha256")
    );
    assert_eq!(
        manifest.media()[0].byte_length,
        fixture.host["media_bytes"].as_u64().unwrap()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    std::fs::remove_file(fixture.blob()).unwrap();
    let request =
        RestoreRequest::new([18; 16], backup, phase1_operation_now_unix_ms().unwrap()).unwrap();
    let guard = restore_application_backup(config.clone(), request, &files)
        .await
        .unwrap();
    assert_eq!(
        hash(&std::fs::read(fixture.blob()).unwrap()),
        field(&fixture.host, "media_sha256")
    );
    assert!(RuntimeBuilder::new(config.clone()).build().await.is_err());
    let source = Arc::new(OriginalTargets::new(&fixture));
    let runtime = controlled_runtime(&fixture, guard.clone(), source.clone()).await;
    fixture.assert_revisions(&runtime).await;
    fixture.assert_statuses(&runtime).await;
    assert_eq!(
        runtime.review_restored_work().await,
        Err(RestoreError::ReconciliationRequired)
    );
    let status = runtime.restore_status().await.unwrap().unwrap();
    assert_eq!(status.phase, RestorePhase::Held);
    assert_eq!(status.targets.len(), 4);
    let first = &status.targets[0];
    assert_eq!(
        runtime
            .phase1_advance_draft(
                first.draft_id,
                runtime
                    .phase1_draft_status(first.draft_id)
                    .await
                    .unwrap()
                    .draft()
                    .revision()
                    .get()
            )
            .await
            .unwrap_err(),
        Phase1DraftError::Restore(RestoreError::ReconciliationRequired)
    );
    let partial = runtime
        .reconcile_restored_target(first.draft_id, &first.target_fingerprint)
        .await
        .unwrap();
    assert_eq!(partial.observation, RestoreObservation::Incomplete);
    assert_eq!(
        runtime.review_restored_work().await,
        Err(RestoreError::ReconciliationRequired)
    );
    source.incomplete.store(false, Ordering::SeqCst);
    let mut observed = 0;
    for target in &status.targets {
        let fresh = runtime
            .reconcile_restored_target(target.draft_id, &target.target_fingerprint)
            .await
            .unwrap();
        assert_eq!(
            fresh.operation_id,
            bytes::<16>(field(
                fixture.rust["drafts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| field(row, "draft_id") == hex::encode(target.draft_id))
                    .unwrap(),
                "operation_id"
            ))
        );
        assert_eq!(fresh.event_id, target.event_id);
        assert_ne!(fresh.observation, RestoreObservation::Incomplete);
        observed += usize::from(fresh.observation == RestoreObservation::Observed);
    }
    assert_eq!(
        observed, 1,
        "Only the old signed event from relay-one was freshly observed"
    );
    let inventory = runtime.review_restored_work().await.unwrap();
    runtime.resume_restored_work(inventory).await.unwrap();
    runtime.resume_restored_work(inventory).await.unwrap();
    assert_eq!(
        runtime.restore_status().await.unwrap().unwrap().phase,
        RestorePhase::Resumed
    );
    fixture.assert_statuses(&runtime).await;
    assert_eq!(source.deliveries.load(Ordering::SeqCst), 0);
    assert_eq!(source.requests.lock().unwrap().len(), 5);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let reopened = RuntimeBuilder::new(config)
        .restore_guard(guard)
        .build()
        .await
        .unwrap();
    fixture.assert_revisions(&reopened).await;
    fixture.assert_statuses(&reopened).await;
    assert_eq!(
        reopened.restore_status().await.unwrap().unwrap().phase,
        RestorePhase::Resumed
    );
    reopened.shutdown().await.unwrap();
}
