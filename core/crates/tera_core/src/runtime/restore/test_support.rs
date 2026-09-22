use super::*;
use crate::runtime::{backup::*, builder::RuntimeBuilder, store::*};
use radroots_transport::BoxFuture;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub(super) struct Host {
    pub guard_path: PathBuf,
    pub manifest: Mutex<Option<Vec<u8>>>,
    pub fail_after_stage: AtomicBool,
    checks: AtomicUsize,
}

impl BackupHost for Host {
    fn load_candidate(
        &self,
        _: BackupRequest,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, BackupError>> {
        Box::pin(async { Ok(self.manifest.lock().unwrap().clone()) })
    }
    fn retain_media(
        &self,
        _: BackupRequest,
        media: Vec<BackupMediaRequirement>,
    ) -> BoxFuture<'_, Result<Vec<BackupMediaLease>, BackupError>> {
        Box::pin(async move {
            assert!(media.is_empty());
            Ok(vec![])
        })
    }
    fn persist_candidate(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async move {
            *self.manifest.lock().unwrap() = Some(manifest.encode()?);
            Ok(())
        })
    }
    fn publish_complete(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        self.persist_candidate(manifest)
    }
}

impl RestoreHost for Host {
    fn require_quiescent(&self) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async {
            let index = self.checks.fetch_add(1, Ordering::SeqCst);
            if index != 0 && self.fail_after_stage.load(Ordering::SeqCst) {
                Err(RestoreError::Busy)
            } else {
                Ok(())
            }
        })
    }
    fn load_completed(&self, _: RestoreRequest) -> BoxFuture<'_, Result<Vec<u8>, RestoreError>> {
        Box::pin(async {
            self.manifest
                .lock()
                .unwrap()
                .clone()
                .ok_or(RestoreError::Unavailable)
        })
    }
    fn restore_media(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async move {
            assert!(manifest.media().is_empty());
            Ok(())
        })
    }
    fn arm_guard(&self, guard: ApplicationRestoreGuard) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async move {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.guard_path)
                .unwrap();
            file.write_all(&guard.encode()?).unwrap();
            file.sync_all().unwrap();
            Ok(())
        })
    }
}

pub(super) async fn fixture() -> (
    tempfile::TempDir,
    MobileUserStoreConfig,
    crate::TeraRuntime,
    Host,
    RestoreRequest,
) {
    let root = tempfile::tempdir().unwrap();
    let config = MobileUserStoreConfig::from_encoded(
        root.path(),
        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        &hex::encode([7; 32]),
        100,
        ProtectedDataAvailability::Available,
    )
    .unwrap()
    .with_local_backups();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    std::fs::create_dir_all(config.backup_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    let host = Host {
        guard_path: config.restore_guard_path(),
        manifest: Mutex::new(None),
        fail_after_stage: AtomicBool::new(false),
        checks: AtomicUsize::new(0),
    };
    let backup = BackupRequest::new(
        [1; 16],
        config.public_key().into_bytes(),
        [7; 32],
        200,
        128 * 1024 * 1024,
    )
    .unwrap();
    let restore = RestoreRequest::new([2; 16], backup, 300).unwrap();
    (root, config, runtime, host, restore)
}
