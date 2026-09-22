use radroots_storage::backup::BackupCapabilityError;

use crate::runtime::lifecycle::RuntimeLifecycleError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BackupError {
    #[error("backup requires an idle runtime")]
    Busy,
    #[error("backup request is invalid")]
    InvalidRequest,
    #[error("backup identity does not match this account")]
    IdentityMismatch,
    #[error("backup source generation does not match")]
    GenerationMismatch,
    #[error("backup format is unsupported")]
    UnsupportedFormat,
    #[error("required backup media is unavailable or invalid")]
    MediaUnavailable,
    #[error("backup exceeds the admitted capacity")]
    CapacityExceeded,
    #[error("backup storage is unavailable")]
    Unavailable,
    #[error("backup verification failed")]
    VerificationFailed,
    #[error("backup publication is incomplete")]
    PublicationIncomplete,
    #[error("backup state requires reconciliation")]
    Conflict,
}

impl BackupError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Busy => "backup_busy",
            Self::InvalidRequest => "backup_invalid_request",
            Self::IdentityMismatch => "backup_identity_mismatch",
            Self::GenerationMismatch => "backup_generation_mismatch",
            Self::UnsupportedFormat => "backup_unsupported_format",
            Self::MediaUnavailable => "backup_media_unavailable",
            Self::CapacityExceeded => "backup_capacity_exceeded",
            Self::Unavailable => "backup_unavailable",
            Self::VerificationFailed => "backup_verification_failed",
            Self::PublicationIncomplete => "backup_publication_incomplete",
            Self::Conflict => "backup_conflict",
        }
    }
}

impl From<RuntimeLifecycleError> for BackupError {
    fn from(value: RuntimeLifecycleError) -> Self {
        match value {
            RuntimeLifecycleError::MaintenanceInProgress => Self::Busy,
            _ => Self::Unavailable,
        }
    }
}

impl From<BackupCapabilityError> for BackupError {
    fn from(value: BackupCapabilityError) -> Self {
        match value {
            BackupCapabilityError::UnsupportedVersion => Self::UnsupportedFormat,
            BackupCapabilityError::Conflict => Self::Conflict,
            BackupCapabilityError::VerificationFailed => Self::VerificationFailed,
            BackupCapabilityError::Failed => Self::PublicationIncomplete,
            _ => Self::Unavailable,
        }
    }
}
