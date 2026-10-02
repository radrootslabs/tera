use super::fixture::*;
use crate::runtime::{backup::*, restore::*};
use radroots_transport::BoxFuture;
use std::{path::PathBuf, sync::Mutex};

pub(super) struct Files {
    pub fixture_blob: PathBuf,
    pub leases: PathBuf,
    pub guard: PathBuf,
    pub manifest: Mutex<Option<Vec<u8>>>,
}

impl Files {
    pub fn new(fixture: &Fixture) -> Self {
        let leases = fixture.root.path().join("controlled_media_leases");
        std::fs::create_dir(&leases).unwrap();
        Self {
            fixture_blob: fixture.blob(),
            leases,
            guard: fixture.config(true).restore_guard_path(),
            manifest: Mutex::new(None),
        }
    }
    fn verify(&self, path: &std::path::Path, sha: &str, length: u64) {
        let content = std::fs::read(path).unwrap();
        assert_eq!(content.len() as u64, length);
        assert_eq!(hash(&content), sha);
    }
}

impl BackupHost for Files {
    fn load_candidate(
        &self,
        _: BackupRequest,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, BackupError>> {
        Box::pin(async { Ok(self.manifest.lock().unwrap().clone()) })
    }
    fn retain_media(
        &self,
        request: BackupRequest,
        media: Vec<BackupMediaRequirement>,
    ) -> BoxFuture<'_, Result<Vec<BackupMediaLease>, BackupError>> {
        Box::pin(async move {
            assert_eq!(
                media.len(),
                1,
                "Complete paged inventory must find old pending media outside its first 100 heads"
            );
            let mut leases = vec![];
            for item in media {
                self.verify(&self.fixture_blob, &item.sha256, item.byte_length);
                let destination = self.leases.join(&item.sha256);
                if destination.exists() {
                    self.verify(&destination, &item.sha256, item.byte_length);
                } else {
                    std::fs::copy(&self.fixture_blob, &destination).unwrap();
                }
                leases.push(BackupMediaLease {
                    identifier: request.lease_identifier(&item.sha256),
                    sha256: item.sha256,
                    byte_length: item.byte_length,
                });
            }
            Ok(leases)
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

impl RestoreHost for Files {
    fn require_quiescent(&self) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async { Ok(()) })
    }
    fn load_completed(&self, _: RestoreRequest) -> BoxFuture<'_, Result<Vec<u8>, RestoreError>> {
        Box::pin(async { Ok(self.manifest.lock().unwrap().clone().unwrap()) })
    }
    fn restore_media(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async move {
            assert_eq!(manifest.media().len(), 1);
            for item in manifest.media() {
                let source = self.leases.join(&item.sha256);
                self.verify(&source, &item.sha256, item.byte_length);
                if self.fixture_blob.exists() {
                    self.verify(&self.fixture_blob, &item.sha256, item.byte_length);
                } else {
                    std::fs::copy(source, &self.fixture_blob).unwrap();
                }
            }
            Ok(())
        })
    }
    fn arm_guard(&self, guard: ApplicationRestoreGuard) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async move {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.guard)
                .unwrap();
            file.write_all(&guard.encode()?).unwrap();
            file.sync_all().unwrap();
            Ok(())
        })
    }
}
