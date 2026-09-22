//! Native-owned recovery files and typed application restore commands.
use crate::{FfiBackupManifest, FfiBackupRequest, TeraAppError};
use tera_core::runtime::restore::{ApplicationRestoreGuard, RestoreError, RestoreRequest};

mod adapter;
pub(crate) use adapter::RestoreHostAdapter;
mod records;
pub use records::*;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRestoreRequest {
    pub attempt_id: String,
    pub backup: FfiBackupRequest,
    pub requested_at_ms: u64,
}

impl FfiRestoreRequest {
    pub(crate) fn decode(&self) -> Result<RestoreRequest, RestoreError> {
        RestoreRequest::new(
            bytes(&self.attempt_id)?,
            self.backup.decode()?,
            self.requested_at_ms,
        )
    }
}

impl From<&RestoreRequest> for FfiRestoreRequest {
    fn from(value: &RestoreRequest) -> Self {
        Self {
            attempt_id: hex::encode(value.attempt_id()),
            backup: value.backup().into(),
            requested_at_ms: value.requested_at_ms(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRestoreGuard {
    pub request: FfiRestoreRequest,
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

impl TryFrom<ApplicationRestoreGuard> for FfiRestoreGuard {
    type Error = RestoreError;
    fn try_from(value: ApplicationRestoreGuard) -> Result<Self, Self::Error> {
        Ok(Self {
            request: value.request().into(),
            relative_path: relative_path(value.request().backup().author()),
            bytes: value.encode()?,
        })
    }
}

#[uniffi::export]
pub fn application_restore_guard_limit() -> u64 {
    tera_core::runtime::restore::RESTORE_GUARD_MAX_BYTES as u64
}

#[uniffi::export]
pub fn application_restore_guard_path(public_key: String) -> Result<String, TeraAppError> {
    let author = bytes::<32>(&public_key)?;
    // Reuse validated account construction without touching the filesystem.
    crate::backup::validate_application_backup_request(FfiBackupRequest {
        schema_version: tera_core::runtime::backup::APPLICATION_BACKUP_VERSION,
        backup_id: hex::encode([1; 16]),
        public_key,
        source_generation: hex::encode([1; 32]),
        requested_at_unix_ms: 1,
        maximum_bytes: 4096,
    })?;
    Ok(relative_path(author))
}

fn relative_path(author: [u8; 32]) -> String {
    format!(
        "radroots/users/{}/{}",
        hex::encode(author),
        tera_core::runtime::store::RESTORE_GUARD_FILENAME
    )
}

#[uniffi::export]
pub fn validate_application_restore_guard(bytes: Vec<u8>) -> Result<FfiRestoreGuard, TeraAppError> {
    ApplicationRestoreGuard::decode(&bytes)?
        .try_into()
        .map_err(Into::into)
}

#[uniffi::export(callback_interface)]
#[async_trait::async_trait]
pub trait TeraRestoreHost: Send + Sync {
    async fn require_quiescent(&self) -> Result<(), TeraAppError>;
    async fn load_completed(&self, request: FfiRestoreRequest) -> Result<Vec<u8>, TeraAppError>;
    async fn restore_media(&self, manifest: FfiBackupManifest) -> Result<(), TeraAppError>;
    async fn arm_guard(&self, guard: FfiRestoreGuard) -> Result<(), TeraAppError>;
}

pub(crate) fn bytes<const N: usize>(value: &str) -> Result<[u8; N], RestoreError> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err(RestoreError::InvalidRequest);
    }
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| RestoreError::InvalidRequest)?;
    Ok(bytes)
}

impl From<RestoreError> for TeraAppError {
    fn from(value: RestoreError) -> Self {
        Self::failure(
            value.code(),
            "restore",
            matches!(value, RestoreError::Busy | RestoreError::Unavailable),
            &["review_restore"],
            &value.to_string(),
        )
    }
}

fn host_error(value: TeraAppError) -> RestoreError {
    match value.report().code.as_str() {
        "restore_busy" => RestoreError::Busy,
        "restore_identity_mismatch" => RestoreError::IdentityMismatch,
        "restore_generation_mismatch" => RestoreError::GenerationMismatch,
        "restore_media_unavailable" => RestoreError::MediaUnavailable,
        "restore_capacity_exceeded" => RestoreError::CapacityExceeded,
        "restore_conflict" => RestoreError::Conflict,
        _ => RestoreError::RecoveryRequired,
    }
}
