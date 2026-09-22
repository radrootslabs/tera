//! Bounded native file-owner bridge for local application backup.
use crate::TeraAppError;
use tera_core::runtime::backup::{BackupError, BackupMediaLease, BackupRequest};

mod adapter;
pub(crate) use adapter::BackupHostAdapter;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBackupLimits {
    pub schema_version: u16,
    pub manifest_bytes: u64,
    pub media_bytes: u64,
    pub media_references: u64,
}

#[uniffi::export]
pub fn application_backup_limits() -> FfiBackupLimits {
    use tera_core::runtime::{backup, product_surface::media_gc};
    FfiBackupLimits {
        schema_version: backup::APPLICATION_BACKUP_VERSION,
        manifest_bytes: backup::BACKUP_MANIFEST_MAX_BYTES as u64,
        media_bytes: backup::BACKUP_MEDIA_MAX_BYTES,
        media_references: media_gc::MEDIA_REFERENCE_BUDGET as u64,
    }
}

#[uniffi::export]
pub fn validate_application_backup_request(request: FfiBackupRequest) -> Result<(), TeraAppError> {
    request.decode().map(|_| ()).map_err(Into::into)
}

#[uniffi::export]
pub fn validate_application_backup_media(
    request: FfiBackupRequest,
    media: Vec<FfiBackupMedia>,
) -> Result<(), TeraAppError> {
    let request = request.decode()?;
    let leases: Vec<_> = media.into_iter().map(Into::into).collect();
    request.validate_media(&leases).map_err(Into::into)
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBackupRequest {
    pub schema_version: u16,
    pub backup_id: String,
    pub public_key: String,
    pub source_generation: String,
    pub requested_at_unix_ms: u64,
    pub maximum_bytes: u64,
}

impl FfiBackupRequest {
    pub(crate) fn decode(&self) -> Result<BackupRequest, BackupError> {
        if self.schema_version != tera_core::runtime::backup::APPLICATION_BACKUP_VERSION {
            return Err(BackupError::UnsupportedFormat);
        }
        BackupRequest::new(
            bytes(&self.backup_id)?,
            bytes(&self.public_key)?,
            bytes(&self.source_generation)?,
            self.requested_at_unix_ms,
            self.maximum_bytes,
        )
    }
}

fn bytes<const N: usize>(value: &str) -> Result<[u8; N], BackupError> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err(BackupError::InvalidRequest);
    }
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| BackupError::InvalidRequest)?;
    Ok(bytes)
}

impl From<&BackupRequest> for FfiBackupRequest {
    fn from(value: &BackupRequest) -> Self {
        Self {
            schema_version: tera_core::runtime::backup::APPLICATION_BACKUP_VERSION,
            backup_id: hex::encode(value.id()),
            public_key: hex::encode(value.author()),
            source_generation: hex::encode(value.generation()),
            requested_at_unix_ms: value.requested_at_ms(),
            maximum_bytes: value.maximum_bytes(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBackupMedia {
    pub sha256: String,
    pub byte_length: u64,
    pub lease_identifier: String,
}

impl From<FfiBackupMedia> for BackupMediaLease {
    fn from(value: FfiBackupMedia) -> Self {
        Self {
            sha256: value.sha256,
            byte_length: value.byte_length,
            identifier: value.lease_identifier,
        }
    }
}

/// Complete is returned only after owner verification and durable native
/// publication. The bytes are the canonical application manifest, not DB data.
#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBackupManifest {
    pub request: FfiBackupRequest,
    pub media: Vec<FfiBackupMedia>,
    pub manifest: Vec<u8>,
}

#[uniffi::export(callback_interface)]
#[async_trait::async_trait]
pub trait TeraBackupHost: Send + Sync {
    async fn load_candidate(
        &self,
        request: FfiBackupRequest,
    ) -> Result<Option<Vec<u8>>, TeraAppError>;
    async fn retain_media(
        &self,
        request: FfiBackupRequest,
        media: Vec<FfiBackupMedia>,
    ) -> Result<Vec<FfiBackupMedia>, TeraAppError>;
    async fn persist_candidate(&self, manifest: FfiBackupManifest) -> Result<(), TeraAppError>;
    async fn publish_complete(&self, manifest: FfiBackupManifest) -> Result<(), TeraAppError>;
}

impl From<BackupError> for TeraAppError {
    fn from(value: BackupError) -> Self {
        Self::failure(
            value.code(),
            "backup",
            matches!(
                value,
                BackupError::Busy | BackupError::Unavailable | BackupError::PublicationIncomplete
            ),
            &["inspect_local_stores"],
            &value.to_string(),
        )
    }
}

pub(crate) fn host_error(value: TeraAppError) -> BackupError {
    match value.report().code.as_str() {
        "backup_busy" => BackupError::Busy,
        "backup_invalid_request" => BackupError::InvalidRequest,
        "backup_identity_mismatch" => BackupError::IdentityMismatch,
        "backup_generation_mismatch" => BackupError::GenerationMismatch,
        "backup_media_unavailable" => BackupError::MediaUnavailable,
        "backup_capacity_exceeded" => BackupError::CapacityExceeded,
        "backup_verification_failed" => BackupError::VerificationFailed,
        "backup_publication_incomplete" => BackupError::PublicationIncomplete,
        "backup_conflict" => BackupError::Conflict,
        _ => BackupError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_rejects_noncanonical_identity_and_future_version() {
        let request = BackupRequest::new(
            [1; 16],
            hex::decode("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
                .unwrap()
                .try_into()
                .unwrap(),
            [7; 32],
            200,
            128 * 1024 * 1024,
        )
        .unwrap();
        let encoded = FfiBackupRequest::from(&request);
        assert_eq!(encoded.decode().unwrap(), request);
        let mut bad = encoded.clone();
        bad.backup_id = "../outside".into();
        assert_eq!(bad.decode().unwrap_err(), BackupError::InvalidRequest);
        bad = encoded.clone();
        bad.public_key = bad.public_key.to_ascii_uppercase();
        assert_eq!(bad.decode().unwrap_err(), BackupError::InvalidRequest);
        bad = encoded;
        bad.schema_version += 1;
        assert_eq!(bad.decode().unwrap_err(), BackupError::UnsupportedFormat);
    }

    #[test]
    fn unknown_native_error_payload_never_crosses_the_safe_boundary() {
        let supplied = TeraAppError::failure(
            "unknown",
            "unsafe",
            true,
            &["unsafe"],
            "PRIVATE_PATH_OR_CONTENT",
        );
        let sanitized: TeraAppError = host_error(supplied).into();
        assert_eq!(sanitized.report().code, "backup_unavailable");
        assert!(!format!("{sanitized:?}").contains("PRIVATE_PATH_OR_CONTENT"));
        let limits = application_backup_limits();
        assert_eq!(limits.schema_version, 1);
        assert_eq!(limits.manifest_bytes, 16 * 1024 * 1024);
    }

    #[test]
    fn native_media_admission_uses_canonical_binding_and_capacity_policy() {
        let request = FfiBackupRequest {
            schema_version: 1,
            backup_id: "01".repeat(16),
            public_key: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into(),
            source_generation: "07".repeat(32),
            requested_at_unix_ms: 200,
            maximum_bytes: 100,
        };
        let hash = "a".repeat(64);
        let media = FfiBackupMedia {
            lease_identifier: request.decode().unwrap().lease_identifier(&hash),
            sha256: hash,
            byte_length: 20,
        };
        validate_application_backup_media(request.clone(), vec![media.clone()]).unwrap();
        let mut foreign = media.clone();
        foreign.lease_identifier = "another_backup".into();
        let error = validate_application_backup_media(request.clone(), vec![foreign]).unwrap_err();
        assert_eq!(error.report().code, "backup_media_unavailable");
        let error =
            validate_application_backup_media(request.clone(), vec![media.clone(), media.clone()])
                .unwrap_err();
        assert_eq!(error.report().code, "backup_media_unavailable");
        let mut oversized = media;
        oversized.byte_length = request.maximum_bytes + 1;
        let error = validate_application_backup_media(request, vec![oversized]).unwrap_err();
        assert_eq!(error.report().code, "backup_capacity_exceeded");
    }
}
