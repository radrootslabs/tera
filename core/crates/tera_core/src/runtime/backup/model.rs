use radroots_identity::PublicKey;
use radroots_storage::{
    backup::{
        BackupFormatVersion, BackupId, BackupManifest, BackupMemberKind, BackupPlan,
        BackupSecretPolicy,
    },
    event::SourceGeneration,
};
use serde::{Deserialize, Serialize};

use super::BackupError as E;
use crate::runtime::product_surface::media_gc::MEDIA_REFERENCE_BUDGET;

pub const APPLICATION_BACKUP_VERSION: u16 = 1;
pub const BACKUP_MANIFEST_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const BACKUP_MEDIA_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// A caller-retained, idempotent request for a local account backup. Reusing
/// the ID with another time, account, generation or capacity is a conflict.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupRequest {
    version: u16,
    id: [u8; 16],
    author: [u8; 32],
    generation: [u8; 32],
    requested_at_ms: u64,
    maximum_bytes: u64,
}

impl BackupRequest {
    pub fn new(
        id: [u8; 16],
        author: [u8; 32],
        generation: [u8; 32],
        requested_at_ms: u64,
        maximum_bytes: u64,
    ) -> Result<Self, E> {
        let request = Self {
            version: APPLICATION_BACKUP_VERSION,
            id,
            author,
            generation,
            requested_at_ms,
            maximum_bytes,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), E> {
        if self.version != APPLICATION_BACKUP_VERSION {
            return Err(E::UnsupportedFormat);
        }
        BackupId::new(self.id).map_err(|_| E::InvalidRequest)?;
        PublicKey::from_hex(&hex::encode(self.author)).map_err(|_| E::InvalidRequest)?;
        SourceGeneration::new(self.generation).map_err(|_| E::InvalidRequest)?;
        if self.requested_at_ms == 0
            || self.requested_at_ms > i64::MAX as u64
            || self.maximum_bytes == 0
            || self.maximum_bytes > i64::MAX as u64
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }

    pub const fn id(&self) -> [u8; 16] {
        self.id
    }
    pub const fn author(&self) -> [u8; 32] {
        self.author
    }
    pub const fn generation(&self) -> [u8; 32] {
        self.generation
    }
    pub const fn requested_at_ms(&self) -> u64 {
        self.requested_at_ms
    }
    pub const fn maximum_bytes(&self) -> u64 {
        self.maximum_bytes
    }

    pub(super) fn plan(&self) -> Result<BackupPlan, E> {
        self.validate()?;
        BackupPlan::new(
            BackupId::new(self.id).map_err(|_| E::InvalidRequest)?,
            BackupFormatVersion::V1,
            BackupSecretPolicy::IncludeProtectedStorage,
            self.requested_at_ms,
        )
        .map_err(|_| E::InvalidRequest)
    }

    pub fn lease_identifier(&self, sha256: &str) -> String {
        format!("tera_backup_{}_{}", hex::encode(self.id), sha256)
    }

    /// Validates native lease admission with the same policy as a manifest.
    pub fn validate_media(&self, leases: &[BackupMediaLease]) -> Result<(), E> {
        self.validate()?;
        validate_leases(self, leases).map(|_| ())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupMediaRequirement {
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupMediaLease {
    pub sha256: String,
    pub byte_length: u64,
    pub identifier: String,
}

/// Only constructed or decoded after complete application binding checks.
/// The manifest contains inventory, never user payloads or secret material.
#[derive(Clone, Eq, PartialEq, Serialize)]
pub struct ApplicationBackupManifest {
    request: BackupRequest,
    owner: BackupManifest,
    media: Vec<BackupMediaLease>,
}

impl std::fmt::Debug for ApplicationBackupManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplicationBackupManifest")
            .finish_non_exhaustive()
    }
}

impl ApplicationBackupManifest {
    pub(super) fn new(
        request: BackupRequest,
        owner: BackupManifest,
        media: Vec<BackupMediaLease>,
    ) -> Result<Self, E> {
        let manifest = Self {
            request,
            owner,
            media,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn request(&self) -> &BackupRequest {
        &self.request
    }
    pub fn owner(&self) -> &BackupManifest {
        &self.owner
    }
    pub fn media(&self) -> &[BackupMediaLease] {
        &self.media
    }

    pub fn decode(bytes: &[u8], request: &BackupRequest) -> Result<Self, E> {
        if bytes.len() > BACKUP_MANIFEST_MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            request: BackupRequest,
            owner: BackupManifest,
            media: Vec<BackupMediaLease>,
        }
        let wire: Wire = serde_json::from_slice(bytes).map_err(|_| E::VerificationFailed)?;
        let manifest = Self::new(wire.request, wire.owner, wire.media)?;
        if manifest.request.author != request.author {
            return Err(E::IdentityMismatch);
        }
        if manifest.request.generation != request.generation {
            return Err(E::GenerationMismatch);
        }
        if &manifest.request != request {
            return Err(E::Conflict);
        }
        Ok(manifest)
    }

    pub fn encode(&self) -> Result<Vec<u8>, E> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| E::VerificationFailed)?;
        if bytes.len() > BACKUP_MANIFEST_MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        Ok(bytes)
    }

    fn validate(&self) -> Result<(), E> {
        let plan = self.request.plan()?;
        if self.owner.format_version() != BackupFormatVersion::V1 {
            return Err(E::UnsupportedFormat);
        }
        if self.owner.backup_id() != plan.backup_id()
            || self.owner.created_at_unix_ms() != plan.requested_at_unix_ms()
            || self.owner.secret_policy() != BackupSecretPolicy::IncludeProtectedStorage
            || self.owner.members().len() != 2
            || self
                .owner
                .members()
                .iter()
                .filter(|v| v.kind() == BackupMemberKind::Runtime)
                .count()
                != 1
            || self
                .owner
                .members()
                .iter()
                .filter(|v| v.kind() == BackupMemberKind::Protected)
                .count()
                != 1
        {
            return Err(E::VerificationFailed);
        }
        let media_bytes = validate_leases(&self.request, &self.media)?;
        let total = self
            .owner
            .total_bytes()
            .checked_add(media_bytes)
            .ok_or(E::CapacityExceeded)?;
        if total > self.request.maximum_bytes {
            return Err(E::CapacityExceeded);
        }
        Ok(())
    }
}

pub(super) fn validate_leases(
    request: &BackupRequest,
    leases: &[BackupMediaLease],
) -> Result<u64, E> {
    if leases.len() > MEDIA_REFERENCE_BUDGET {
        return Err(E::CapacityExceeded);
    }
    let mut previous: Option<&str> = None;
    let mut total = 0_u64;
    for lease in leases {
        if lease.sha256.len() != 64
            || !lease
                .sha256
                .bytes()
                .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
            || previous.is_some_and(|v| v >= lease.sha256.as_str())
            || lease.identifier != request.lease_identifier(&lease.sha256)
            || lease.byte_length == 0
        {
            return Err(E::MediaUnavailable);
        }
        if lease.byte_length > BACKUP_MEDIA_MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        total = total
            .checked_add(lease.byte_length)
            .ok_or(E::CapacityExceeded)?;
        if total > request.maximum_bytes {
            return Err(E::CapacityExceeded);
        }
        previous = Some(&lease.sha256);
    }
    Ok(total)
}
