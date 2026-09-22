use radroots_storage::backup::RestoreCapabilityError;

use crate::runtime::{backup::BackupError, lifecycle::RuntimeLifecycleError};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RestoreError {
    #[error("restore requires exclusive recovery admission")]
    Busy,
    #[error("restore request is invalid")]
    InvalidRequest,
    #[error("restore identity does not match this account")]
    IdentityMismatch,
    #[error("restore source generation does not match")]
    GenerationMismatch,
    #[error("restore format is unsupported")]
    UnsupportedFormat,
    #[error("required restore media is unavailable or invalid")]
    MediaUnavailable,
    #[error("restore exceeds the admitted capacity")]
    CapacityExceeded,
    #[error("restore storage is unavailable")]
    Unavailable,
    #[error("restore verification failed")]
    VerificationFailed,
    #[error("restore state conflicts with retained evidence")]
    Conflict,
    #[error("restore requires explicit recovery")]
    RecoveryRequired,
    #[error("restored work requires reconciliation and explicit resume")]
    ReconciliationRequired,
}

impl RestoreError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Busy => "restore_busy",
            Self::InvalidRequest => "restore_invalid_request",
            Self::IdentityMismatch => "restore_identity_mismatch",
            Self::GenerationMismatch => "restore_generation_mismatch",
            Self::UnsupportedFormat => "restore_unsupported_format",
            Self::MediaUnavailable => "restore_media_unavailable",
            Self::CapacityExceeded => "restore_capacity_exceeded",
            Self::Unavailable => "restore_unavailable",
            Self::VerificationFailed => "restore_verification_failed",
            Self::Conflict => "restore_conflict",
            Self::RecoveryRequired => "restore_recovery_required",
            Self::ReconciliationRequired => "restore_reconciliation_required",
        }
    }
}

impl From<BackupError> for RestoreError {
    fn from(value: BackupError) -> Self {
        match value {
            BackupError::Busy => Self::Busy,
            BackupError::InvalidRequest => Self::InvalidRequest,
            BackupError::IdentityMismatch => Self::IdentityMismatch,
            BackupError::GenerationMismatch => Self::GenerationMismatch,
            BackupError::UnsupportedFormat => Self::UnsupportedFormat,
            BackupError::MediaUnavailable => Self::MediaUnavailable,
            BackupError::CapacityExceeded => Self::CapacityExceeded,
            BackupError::Unavailable => Self::Unavailable,
            BackupError::VerificationFailed => Self::VerificationFailed,
            BackupError::Conflict => Self::Conflict,
            BackupError::PublicationIncomplete => Self::RecoveryRequired,
        }
    }
}

impl From<RestoreCapabilityError> for RestoreError {
    fn from(value: RestoreCapabilityError) -> Self {
        match value {
            RestoreCapabilityError::UnsupportedVersion => Self::UnsupportedFormat,
            RestoreCapabilityError::InvalidConfiguration => Self::InvalidRequest,
            RestoreCapabilityError::Conflict => Self::Conflict,
            RestoreCapabilityError::VerificationFailed => Self::VerificationFailed,
            RestoreCapabilityError::Failed => Self::RecoveryRequired,
            _ => Self::Unavailable,
        }
    }
}

impl From<RuntimeLifecycleError> for RestoreError {
    fn from(value: RuntimeLifecycleError) -> Self {
        match value {
            RuntimeLifecycleError::MaintenanceInProgress => Self::Busy,
            _ => Self::Unavailable,
        }
    }
}
