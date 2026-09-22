use super::test_support::*;
use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    product_surface::{Phase1DraftError, ProfileMetadataCommand},
};
use std::sync::atomic::Ordering;

fn profile(name: &str) -> ProfileMetadataCommand {
    ProfileMetadataCommand::new(name.into(), None, None, None, None, None, None).unwrap()
}

#[tokio::test]
async fn restores_actual_owner_files_and_holds_original_work_across_reopen() {
    let (_root, config, runtime, host, request) = fixture().await;
    let saved = runtime
        .phase1_save_profile_metadata(profile("original"))
        .await
        .unwrap();
    let original = saved.draft().clone();
    runtime
        .capture_application_backup(request.backup().clone(), &host)
        .await
        .unwrap();
    let later = runtime
        .phase1_save_profile_metadata(profile("later"))
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();

    let guard = restore_application_backup(config.clone(), request.clone(), &host)
        .await
        .unwrap();
    assert_eq!(
        ApplicationRestoreGuard::decode(&std::fs::read(&host.guard_path).unwrap()).unwrap(),
        guard
    );
    assert!(RuntimeBuilder::new(config.clone()).build().await.is_err());
    for _ in 0..2 {
        let reopened = RuntimeBuilder::new(config.clone())
            .restore_guard(guard.clone())
            .build()
            .await
            .unwrap();
        assert_eq!(
            reopened
                .phase1_profile_status(*original.draft_id().as_bytes())
                .await
                .unwrap()
                .draft(),
            &original
        );
        assert_eq!(
            reopened
                .phase1_profile_status(*later.draft().draft_id().as_bytes())
                .await
                .unwrap_err(),
            Phase1DraftError::NotFound
        );
        assert_eq!(
            reopened
                .phase1_advance_profile(*original.draft_id().as_bytes())
                .await
                .unwrap_err(),
            Phase1DraftError::Restore(RestoreError::ReconciliationRequired)
        );
        assert_eq!(
            reopened.require_restore_effects_allowed().await,
            Err(RestoreError::ReconciliationRequired)
        );
        // Local creation remains usable without granting any external effect.
        reopened
            .phase1_save_profile_metadata(profile("local edit"))
            .await
            .unwrap();
        let unchanged = reopened
            .phase1_profile_status(*original.draft_id().as_bytes())
            .await
            .unwrap();
        assert_eq!(unchanged.draft(), &original);
        assert!(unchanged.push().is_none());
        let page = reopened.recovery_page(64, None).await.unwrap();
        assert!(page.entries.is_empty());
        let store = reopened.client.storage().unwrap();
        crate::runtime::product_surface::media_gc::backup_references(
            store,
            config.public_key().into_bytes(),
        )
        .await
        .unwrap();
        reopened.shutdown().await.unwrap();
    }
    assert_eq!(
        restore_application_backup(config, request, &host).await,
        Err(RestoreError::RecoveryRequired)
    );
}

#[tokio::test]
async fn interrupted_after_stage_retains_guard_and_never_opens_an_unbound_runtime() {
    let (_root, config, runtime, host, request) = fixture().await;
    runtime
        .capture_application_backup(request.backup().clone(), &host)
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    host.fail_after_stage.store(true, Ordering::SeqCst);
    assert_eq!(
        restore_application_backup(config.clone(), request, &host).await,
        Err(RestoreError::Busy)
    );
    let guard = ApplicationRestoreGuard::decode(&std::fs::read(&host.guard_path).unwrap()).unwrap();
    assert!(RuntimeBuilder::new(config.clone()).build().await.is_err());
    assert!(
        RuntimeBuilder::new(config.clone())
            .restore_guard(guard)
            .build()
            .await
            .is_err()
    );
    // The failed guarded startup explicitly closes, releasing the writer lease.
    let owner = radroots_sdk::ClientBuilder::sqlite(config.sqlite_options().unwrap())
        .await
        .unwrap()
        .build()
        .unwrap();
    owner.close().await.unwrap();
}

#[tokio::test]
async fn malformed_or_foreign_backup_is_rejected_before_guard_or_replacement() {
    let (_root, config, runtime, host, request) = fixture().await;
    let original = runtime
        .phase1_save_profile_metadata(profile("kept"))
        .await
        .unwrap()
        .draft()
        .clone();
    runtime
        .capture_application_backup(request.backup().clone(), &host)
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    *host.manifest.lock().unwrap() = Some(b"{\"path\":\"/arbitrary/database\"}".to_vec());
    assert!(
        restore_application_backup(config.clone(), request, &host)
            .await
            .is_err()
    );
    assert!(!host.guard_path.exists());
    let reopened = RuntimeBuilder::new(config).build().await.unwrap();
    assert_eq!(
        reopened
            .phase1_profile_status(*original.draft_id().as_bytes())
            .await
            .unwrap()
            .draft(),
        &original
    );
    reopened.shutdown().await.unwrap();
}
