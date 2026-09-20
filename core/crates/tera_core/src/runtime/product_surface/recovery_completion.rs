//! Host-requested completion of one retained native receipt. Native settlement
//! remains host-owned and follows, rather than implies, durable Rust success.

use super::{
    Phase1DraftError, SubmissionMediaResponse, SubmissionOperationError,
    recovery_inventory::RecoveryOwner,
};
use crate::TeraRuntime;

mod types;
mod validation;
pub use types::{
    RecoveryMedia, RecoveryNativeIdentity, RecoveryNativeMedia, RecoveryNativeReceipt,
};
pub(super) use validation::{admit, validate_history};

#[derive(Debug, thiserror::Error)]
pub enum RecoveryCompletionError {
    #[error("saved transfer could not be confirmed")]
    Draft(#[from] Phase1DraftError),
    #[error("saved submission transfer could not be confirmed")]
    Submission(#[from] SubmissionOperationError),
}

/// A snapshot of existing durable verification, not a new transfer or receipt
/// timestamp. Later parent revisions do not change this completion identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryCompletionReceipt {
    pub parent: [u8; 16],
    pub attempt: [u8; 16],
    pub canonical_url: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub verified_at_unix_ms: u64,
}

impl TeraRuntime {
    pub async fn recover_native_upload(
        &self,
        native: RecoveryNativeReceipt,
        source: RecoveryMedia,
    ) -> Result<RecoveryCompletionReceipt, RecoveryCompletionError> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        let key = *native.identity.parent.as_bytes();
        let owner = self
            .recovery_parent(key)
            .await?
            .ok_or(Phase1DraftError::NotFound)?;
        match owner.owner {
            RecoveryOwner::Legacy => Ok(self.recover_legacy_upload(native, source).await?),
            RecoveryOwner::Submission(request) => Ok(self
                .recover_submission_upload(&request, native, source)
                .await?),
            RecoveryOwner::Repair(_) => Err(Phase1DraftError::Corrupt.into()),
        }
    }
}
