use tera_core::error::recovery;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRecoveryDisposition {
    Unknown,
    InvalidInput,
    StaleRevision,
    IdempotencyConflict,
    StaleCursor,
    ProtectedDataUnavailable,
    IdentityUnavailable,
    StorageFailure,
    QuotaExhausted,
    CancelledBeforeEffect,
    OutcomeUnknown,
    UnsupportedVersion,
    NetworkUnavailable,
    PartialResult,
    NetworkPolicy,
    MediaCorrupt,
    RuntimeUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRetryDisposition {
    NotAllowed,
    AfterRecovery,
    ReconcileExistingOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRecoveryDecision {
    pub disposition: FfiRecoveryDisposition,
    pub retry: FfiRetryDisposition,
}

/// Pure guidance; never admits, retries, or publishes an operation.
#[uniffi::export]
pub fn classify_error_recovery(schema_version: u16, code: String) -> FfiRecoveryDecision {
    let decision = recovery::classify(schema_version, &code);
    let retry = match decision.retry {
        recovery::RetryDisposition::NotAllowed => FfiRetryDisposition::NotAllowed,
        recovery::RetryDisposition::AfterRecovery => FfiRetryDisposition::AfterRecovery,
        recovery::RetryDisposition::ReconcileExistingOperation => {
            FfiRetryDisposition::ReconcileExistingOperation
        }
    };
    FfiRecoveryDecision {
        disposition: decision.disposition.into(),
        retry,
    }
}

impl From<recovery::RecoveryDisposition> for FfiRecoveryDisposition {
    fn from(value: recovery::RecoveryDisposition) -> Self {
        use recovery::RecoveryDisposition as Core;
        match value {
            Core::Unknown => Self::Unknown,
            Core::InvalidInput => Self::InvalidInput,
            Core::StaleRevision => Self::StaleRevision,
            Core::IdempotencyConflict => Self::IdempotencyConflict,
            Core::StaleCursor => Self::StaleCursor,
            Core::ProtectedDataUnavailable => Self::ProtectedDataUnavailable,
            Core::IdentityUnavailable => Self::IdentityUnavailable,
            Core::StorageFailure => Self::StorageFailure,
            Core::QuotaExhausted => Self::QuotaExhausted,
            Core::CancelledBeforeEffect => Self::CancelledBeforeEffect,
            Core::OutcomeUnknown => Self::OutcomeUnknown,
            Core::UnsupportedVersion => Self::UnsupportedVersion,
            Core::NetworkUnavailable => Self::NetworkUnavailable,
            Core::PartialResult => Self::PartialResult,
            Core::NetworkPolicy => Self::NetworkPolicy,
            Core::MediaCorrupt => Self::MediaCorrupt,
            Core::RuntimeUnavailable => Self::RuntimeUnavailable,
        }
    }
}
