//! Pure decisions over one transfer's authoritative facts. An action is a
//! request to the existing owner, never evidence that an effect has happened.
//! Callers re-read facts and validate the stored response/owned bytes before
//! completion, and require durable Rust success before native settlement.

use radroots_blossom::{BlobUrl, MediaType};
use radroots_identity::PublicKey;
use radroots_signing::SigningOperationId;
use radroots_storage::authored_draft::AuthoredDraftId;

use super::Phase1DraftError;

// Match the native destination and individual header admission ceilings.
// Use byte bounds so retained canonical text has an explicit memory ceiling.
pub const RECOVERY_URL_MAX_BYTES: usize = 4096;
pub const RECOVERY_MEDIA_TYPE_MAX_BYTES: usize = 8192;

/// Frozen association, not the current editable draft or current settings.
/// The canonical blob URL includes the exact hash, origin and path. The attempt
/// is the persisted signing operation, not a newly minted recovery identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryAssociation {
    author: PublicKey,
    parent: AuthoredDraftId,
    attempt: SigningOperationId,
    canonical_url: BlobUrl,
    media_type: MediaType,
    byte_size: u64,
}

impl RecoveryAssociation {
    pub fn new(
        author: PublicKey,
        parent: AuthoredDraftId,
        attempt: SigningOperationId,
        canonical_url: BlobUrl,
        media_type: MediaType,
        byte_size: u64,
    ) -> Result<Self, Phase1DraftError> {
        if byte_size == 0
            || canonical_url.as_str().len() > RECOVERY_URL_MAX_BYTES
            || media_type.as_str().len() > RECOVERY_MEDIA_TYPE_MAX_BYTES
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        Ok(Self {
            author,
            parent,
            attempt,
            canonical_url,
            media_type,
            byte_size,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustCompletion {
    Pending,
    /// Only a validated durable journal result establishes this fact.
    Verified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryParent {
    /// Includes absence from a displayed page. Exact lookup has not run yet.
    Unqueried,
    ProtectedDataUnavailable,
    StorageUnavailable,
    /// Only an authoritative exact lookup may establish absence.
    Missing,
    InvalidRecord,
    /// Historical records without an exact attempt must retain their evidence.
    AssociationUnconfirmed,
    Known {
        association: Box<RecoveryAssociation>,
        completion: RustCompletion,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRecoveryState {
    Active,
    /// The stored response is available for validation, not already trusted.
    ReceiptAvailable,
    Settled,
    /// Requires authoritative native reconciliation, not a timeout or an
    /// isolated failed callback while an OS task might still be running.
    DefinitivelyInactive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeRecoveryEvidence {
    Unknown,
    /// Conflicting identity/body evidence is retained for isolated repair.
    Conflicting,
    Known {
        association: Box<RecoveryAssociation>,
        state: NativeRecoveryState,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryPause {
    ProtectedData,
    StorageUnavailable,
    NativeActive,
    RetryAuthorityRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryRepair {
    ParentMissing,
    InvalidParent,
    AssociationUnconfirmed,
    ConflictingEvidence,
    AssociationMismatch,
    SettlementWithoutCompletion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDecision {
    LookupParent,
    QueryNative,
    Pause(RecoveryPause),
    Complete,
    Settle,
    Reconciled,
    Quarantine(RecoveryRepair),
}

/// Constant work and memory for one exact association. No clock, mutable head
/// revision, bearer token, upload, signing, storage write or native effect.
pub fn reconcile(parent: &RecoveryParent, native: &NativeRecoveryEvidence) -> RecoveryDecision {
    use RecoveryDecision as D;
    let (expected, completion) = match parent {
        RecoveryParent::Unqueried => return D::LookupParent,
        RecoveryParent::ProtectedDataUnavailable => {
            return D::Pause(RecoveryPause::ProtectedData);
        }
        RecoveryParent::StorageUnavailable => {
            return D::Pause(RecoveryPause::StorageUnavailable);
        }
        RecoveryParent::Missing => return D::Quarantine(RecoveryRepair::ParentMissing),
        RecoveryParent::InvalidRecord => return D::Quarantine(RecoveryRepair::InvalidParent),
        RecoveryParent::AssociationUnconfirmed => {
            return D::Quarantine(RecoveryRepair::AssociationUnconfirmed);
        }
        RecoveryParent::Known {
            association,
            completion,
        } => (association, completion),
    };
    let (observed, state) = match native {
        NativeRecoveryEvidence::Unknown => return D::QueryNative,
        NativeRecoveryEvidence::Conflicting => {
            return D::Quarantine(RecoveryRepair::ConflictingEvidence);
        }
        NativeRecoveryEvidence::Known { association, state } => (association, state),
    };
    if expected != observed {
        return D::Quarantine(RecoveryRepair::AssociationMismatch);
    }
    match (completion, state) {
        (RustCompletion::Pending, NativeRecoveryState::ReceiptAvailable) => D::Complete,
        (RustCompletion::Verified, NativeRecoveryState::ReceiptAvailable) => D::Settle,
        (RustCompletion::Verified, NativeRecoveryState::Settled) => D::Reconciled,
        (RustCompletion::Pending, NativeRecoveryState::Settled) => {
            D::Quarantine(RecoveryRepair::SettlementWithoutCompletion)
        }
        (RustCompletion::Pending, NativeRecoveryState::Active) => {
            D::Pause(RecoveryPause::NativeActive)
        }
        (RustCompletion::Pending, NativeRecoveryState::DefinitivelyInactive) => {
            D::Pause(RecoveryPause::RetryAuthorityRequired)
        }
        (RustCompletion::Verified, _) => D::QueryNative,
    }
}

#[cfg(test)]
mod tests;
