//! Application recovery policy. Diagnostics and localized text are never inputs.

use radroots_protocol::error::v1::{KnownCode, SCHEMA_VERSION};

mod application;

pub use application::inbound_media_code;

/// What must be resolved before the caller considers another operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDisposition {
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

/// Guidance only: no disposition grants publication or authorizes a new intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryDisposition {
    NotAllowed,
    AfterRecovery,
    ReconcileExistingOperation,
}

impl RecoveryDisposition {
    const fn retry(self) -> RetryDisposition {
        match self {
            Self::Unknown
            | Self::InvalidInput
            | Self::IdempotencyConflict
            | Self::UnsupportedVersion
            | Self::MediaCorrupt => RetryDisposition::NotAllowed,
            Self::OutcomeUnknown => RetryDisposition::ReconcileExistingOperation,
            Self::StaleRevision
            | Self::StaleCursor
            | Self::ProtectedDataUnavailable
            | Self::IdentityUnavailable
            | Self::StorageFailure
            | Self::QuotaExhausted
            | Self::CancelledBeforeEffect
            | Self::NetworkUnavailable
            | Self::NetworkPolicy
            | Self::PartialResult
            | Self::RuntimeUnavailable => RetryDisposition::AfterRecovery,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryDecision {
    pub disposition: RecoveryDisposition,
    pub retry: RetryDisposition,
}

/// Classifies only the versioned stable identity. Unknown codes fail closed.
pub fn classify(schema_version: u16, code: &str) -> RecoveryDecision {
    let known = KnownCode::parse(code);
    let disposition = if schema_version != SCHEMA_VERSION {
        RecoveryDisposition::UnsupportedVersion
    } else {
        known.map_or_else(|| application::classify(code), shared)
    };
    let retry = match disposition.retry() {
        RetryDisposition::AfterRecovery
            if known.is_some_and(|code| !code.descriptor().retryable) =>
        {
            RetryDisposition::NotAllowed
        }
        value => value,
    };
    RecoveryDecision { disposition, retry }
}

fn shared(code: KnownCode) -> RecoveryDisposition {
    use KnownCode as Code;
    use RecoveryDisposition as Recovery;
    match code {
        Code::InvalidArgument
        | Code::NotFound
        | Code::AmbiguousTrade
        | Code::RevisionRequired
        | Code::ValidationExpired
        | Code::ValidatorSetInvalid => Recovery::InvalidInput,
        Code::UnsupportedContractVersion
        | Code::UnsupportedProfileSchema
        | Code::SchemaTooNew
        | Code::UnsupportedCapability => Recovery::UnsupportedVersion,
        Code::StaleListingRevision
        | Code::PreconditionChanged
        | Code::InventoryUnavailable
        | Code::ProjectionStale
        | Code::ProjectionFailed
        | Code::ProjectionGenerationChanged => Recovery::StaleRevision,
        Code::IdempotencyConflict => Recovery::IdempotencyConflict,
        Code::InvalidCursor => Recovery::StaleCursor,
        Code::ApprovalRequired
        | Code::ApprovalInvalid
        | Code::ApprovalExpired
        | Code::ApprovalReplayed
        | Code::AuthorizationDenied
        | Code::SignerCapabilityMissing
        | Code::SignerUnavailable
        | Code::SignerRejected
        | Code::SignerCancelled
        | Code::SignerOutputInvalid
        | Code::PrivateDataUnavailable => Recovery::IdentityUnavailable,
        Code::OperationInProgress
        | Code::SignerTimeout
        | Code::TransportPartial
        | Code::SyncPartial
        | Code::DeadlineExceeded
        | Code::LocalCommittedDeliveryPending
        | Code::ValidationPending
        | Code::ClientCloseInProgress
        | Code::ClientClosing => Recovery::OutcomeUnknown,
        Code::CancelledNoCommit => Recovery::CancelledBeforeEffect,
        Code::RelayAuthRequired
        | Code::RelayAuthRejected
        | Code::RelayPaymentRequired
        | Code::RelayPolicyRestricted
        | Code::RelayRateLimited
        | Code::RelayPowRequired
        | Code::TransportOperationUnavailable
        | Code::DmRelayUnconfigured
        | Code::SignerWithoutSink
        | Code::MediaPolicyDenied => Recovery::NetworkPolicy,
        Code::DatabaseBusy
        | Code::ProfileWriterInUse
        | Code::MaintenanceInProgress
        | Code::StorageIntegrityFailed
        | Code::BackupInvalid
        | Code::BackupAuthenticationFailed
        | Code::RestoreFailed
        | Code::MissingStorage
        | Code::StorageCloseFailed => Recovery::StorageFailure,
        Code::StorageSpaceInsufficient => Recovery::QuotaExhausted,
        Code::SyncSaturated | Code::Backpressure | Code::ClientClosed => {
            Recovery::RuntimeUnavailable
        }
        Code::InternalError => Recovery::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shared_code_preserves_the_catalog_retry_restriction() {
        for code in KnownCode::ALL {
            let decision = classify(SCHEMA_VERSION, code.as_str());
            if !code.descriptor().retryable {
                assert_ne!(decision.retry, RetryDisposition::AfterRecovery, "{code:?}");
            }
            if *code != KnownCode::InternalError {
                assert_ne!(
                    decision.disposition,
                    RecoveryDisposition::Unknown,
                    "{code:?}"
                );
            }
        }
    }
}
