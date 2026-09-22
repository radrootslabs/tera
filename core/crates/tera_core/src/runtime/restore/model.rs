use radroots_storage::backup::BackupId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RestoreError as E;
use crate::runtime::backup::{ApplicationBackupManifest, BackupRequest};

pub const RESTORE_GUARD_MAX_BYTES: usize = 4096;
const RESTORE_VERSION: u16 = 1;

/// A distinct restore attempt over an exact retained backup. Retrying this
/// attempt cannot select another backup, account, generation or request time.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    version: u16,
    attempt_id: [u8; 16],
    backup: BackupRequest,
    requested_at_ms: u64,
}

impl std::fmt::Debug for RestoreRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RestoreRequest")
            .finish_non_exhaustive()
    }
}

impl RestoreRequest {
    pub fn new(
        attempt_id: [u8; 16],
        backup: BackupRequest,
        requested_at_ms: u64,
    ) -> Result<Self, E> {
        let request = Self {
            version: RESTORE_VERSION,
            attempt_id,
            backup,
            requested_at_ms,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), E> {
        if self.version != RESTORE_VERSION {
            return Err(E::UnsupportedFormat);
        }
        self.backup.validate()?;
        BackupId::new(self.attempt_id).map_err(|_| E::InvalidRequest)?;
        if self.requested_at_ms < self.backup.requested_at_ms()
            || self.requested_at_ms > i64::MAX as u64
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }

    pub const fn attempt_id(&self) -> [u8; 16] {
        self.attempt_id
    }

    pub const fn backup(&self) -> &BackupRequest {
        &self.backup
    }

    pub const fn requested_at_ms(&self) -> u64 {
        self.requested_at_ms
    }
}

/// Canonical bytes for native create-only recovery evidence outside the two
/// database files. This binding alone is never permission to resume delivery.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ApplicationRestoreGuard {
    request: RestoreRequest,
    manifest_sha256: [u8; 32],
}

impl std::fmt::Debug for ApplicationRestoreGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationRestoreGuard")
            .finish_non_exhaustive()
    }
}

impl ApplicationRestoreGuard {
    pub fn new(request: RestoreRequest, manifest: &ApplicationBackupManifest) -> Result<Self, E> {
        request.validate()?;
        if manifest.request().author() != request.backup.author() {
            return Err(E::IdentityMismatch);
        }
        if manifest.request().generation() != request.backup.generation() {
            return Err(E::GenerationMismatch);
        }
        if manifest.request() != &request.backup {
            return Err(E::Conflict);
        }
        let guard = Self {
            request,
            manifest_sha256: Sha256::digest(manifest.encode()?).into(),
        };
        guard.encode()?;
        Ok(guard)
    }

    pub const fn request(&self) -> &RestoreRequest {
        &self.request
    }

    pub const fn manifest_sha256(&self) -> [u8; 32] {
        self.manifest_sha256
    }

    pub fn verify_manifest(&self, manifest: &ApplicationBackupManifest) -> Result<(), E> {
        if &Self::new(self.request.clone(), manifest)? != self {
            return Err(E::VerificationFailed);
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, E> {
        self.request.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| E::VerificationFailed)?;
        if bytes.len() > RESTORE_GUARD_MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, E> {
        if bytes.len() > RESTORE_GUARD_MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            request: RestoreRequest,
            manifest_sha256: [u8; 32],
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| E::VerificationFailed)?;
        let guard = Self {
            request: wire.request,
            manifest_sha256: wire.manifest_sha256,
        };
        if guard.encode()? != bytes {
            return Err(E::VerificationFailed);
        }
        Ok(guard)
    }
}
