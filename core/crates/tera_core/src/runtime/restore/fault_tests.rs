use super::{test_support::fixture, *};
use crate::runtime::{builder::RuntimeBuilder, product_surface::ProfileMetadataCommand};

#[tokio::test]
async fn unexpected_bundle_entry_retains_live_data_and_external_guard() {
    corrupt_backup(false).await;
}

#[tokio::test]
async fn tampered_owner_member_retains_live_data_and_external_guard() {
    corrupt_backup(true).await;
}

async fn corrupt_backup(tamper_member: bool) {
    let (_root, config, runtime, host, request) = fixture().await;
    let original = runtime
        .phase1_save_profile_metadata(
            ProfileMetadataCommand::new("retained".into(), None, None, None, None, None, None)
                .unwrap(),
        )
        .await
        .unwrap()
        .draft()
        .clone();
    let manifest = runtime
        .capture_application_backup(request.backup().clone(), &host)
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    let member = config
        .backup_directory()
        .join(format!(
            "radroots-backup-{}",
            hex::encode(request.backup().id())
        ))
        .join(if tamper_member {
            manifest
                .owner()
                .members()
                .iter()
                .find(|member| member.kind() == radroots_storage::backup::BackupMemberKind::Runtime)
                .unwrap()
                .relative_path()
        } else {
            "runtime/events.sqlite3"
        });
    let corrupt = if tamper_member {
        let mut bytes = std::fs::read(&member).unwrap();
        assert!(!bytes.is_empty());
        bytes[0] ^= 1;
        bytes
    } else {
        b"unexpected bundle entry".to_vec()
    };
    std::fs::write(&member, &corrupt).unwrap();
    assert_eq!(
        restore_application_backup(config.clone(), request, &host).await,
        Err(RestoreError::VerificationFailed)
    );
    assert!(host.guard_path.is_file());
    assert!(RuntimeBuilder::new(config.clone()).build().await.is_err());
    // Inspect through the canonical owner only; never bypass application holds
    // by returning this owner as a usable application runtime.
    let client = radroots_sdk::ClientBuilder::sqlite(config.sqlite_options().unwrap())
        .await
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        client
            .storage()
            .unwrap()
            .authored_draft_head(original.draft_id())
            .await
            .unwrap(),
        Some(original)
    );
    client.close().await.unwrap();
    assert_eq!(std::fs::read(member).unwrap(), corrupt);
}

#[tokio::test]
async fn a_missing_pair_under_a_valid_guard_cannot_be_initialized_as_empty_recovery() {
    let (_root, config, runtime, host, request) = fixture().await;
    let manifest = runtime
        .capture_application_backup(request.backup().clone(), &host)
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    let guard = ApplicationRestoreGuard::new(request, &manifest).unwrap();
    let paths =
        radroots_sdk::storage::SqlitePaths::from_directory(config.owner_directory()).unwrap();
    // Fault injection only: production has no deletion or empty fallback port.
    std::fs::remove_file(paths.runtime()).unwrap();
    std::fs::remove_file(paths.private()).unwrap();
    std::fs::write(config.restore_guard_path(), guard.encode().unwrap()).unwrap();
    assert!(
        RuntimeBuilder::new(config)
            .restore_guard(guard)
            .build()
            .await
            .is_err()
    );
    assert!(!paths.runtime().exists());
    assert!(!paths.private().exists());
}
